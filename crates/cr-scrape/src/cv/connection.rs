//! Port of the plugin's `cvconnection.py` — the throttled ComicVine
//! HTTP client. Every query waits until 1100 ms have passed since the
//! previous one (the API's rate limit), errors retry once after
//! 2.5 s, and responses must carry `status_code == 1`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::cache::freshness::FreshnessPolicy;

/// The scraper version that rides the User-Agent (the C#
/// `Resources.SCRIPT_VERSION`).
pub const SCRAPER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The fixed delay between queries (`__QUERY_DELAY_MS`).
const QUERY_DELAY_MS: u64 = 1100;

/// How long the one retry waits (`__get_dom` step 6).
const RETRY_DELAY_MS: u64 = 2500;

/// Errors the connection layer reports (the C# `DatabaseConnectionError`
/// shapes).
#[derive(Debug, thiserror::Error)]
pub enum CvError {
    /// Network/download failure (the C# WebException/IOException wrap).
    #[error("connection error contacting Comic Vine: {0} ({1})")]
    Connection(String, String),
    /// The API answered with a non-1 status code.
    #[error("Comic Vine error, code {0}: {1:?}")]
    Status(i64, Option<String>),
    /// An empty or malformed document (the C# empty-dom guards).
    #[error("bad response from Comic Vine: {0}")]
    BadResponse(String),
    /// The per-resource budget refused the request (ADR-037). The
    /// caller made no request.
    #[error("the Comic Vine request budget for {0} is spent")]
    BudgetSpent(String),
    /// Offline mode refused the request before any network work
    /// (ADR-071). The caller made no request.
    #[error("not in cache (offline mode: no request left the process)")]
    Offline,
}

/// The blocking ComicVine client: request throttling, one retry per
/// query, the plugin's User-Agent, and the per-session series-details
/// cache. The base URL and delays are fields so the mock-server tests
/// can run without sleeps.
pub struct CvClient {
    pub(crate) api_key: String,
    pub(crate) base_url: String,
    query_delay: Duration,
    retry_delay: Duration,
    next_query: Mutex<Option<Instant>>,
    agent: ureq::Agent,
    /// `__series_details_cache` — (volume_year, publisher) per series.
    pub(crate) series_details_cache: Mutex<HashMap<i64, (i32, String)>>,
    /// The per-resource request budget (ADR-037). Every API call
    /// passes it. `None` counts nothing, which is what the older
    /// mock-server gates expect.
    budget: Option<Arc<crate::cache::budget::Budget>>,
    /// The persistent cache that receives reusable response data.
    /// Cache write failures are logged and never stop a scrape.
    cache: Option<Arc<dyn crate::cache::CvCache>>,
    /// The freshness rule the local-first issue-list read applies
    /// (ADR-071). The defaults hold when the wiring does not set it.
    freshness: FreshnessPolicy,
    /// The offline switch (ADR-071): every request dies at the
    /// chokepoint, before any network work.
    offline: bool,
}

impl CvClient {
    /// A client for the real ComicVine API.
    pub fn new(api_key: &str) -> Self {
        Self::with_base_url(api_key, "https://comicvine.gamespot.com/api")
    }

    /// A client with an explicit base URL and delay override (the
    /// mock-server and live-gate seam; production uses [`new`]).
    pub fn with_base_url(api_key: &str, base_url: &str) -> Self {
        Self::with_delays(
            api_key,
            base_url,
            Duration::from_millis(QUERY_DELAY_MS),
            Duration::from_millis(RETRY_DELAY_MS),
        )
    }

    pub fn with_delays(
        api_key: &str,
        base_url: &str,
        query_delay: Duration,
        retry_delay: Duration,
    ) -> Self {
        CvClient {
            api_key: api_key.to_string(),
            base_url: base_url.to_string(),
            query_delay,
            retry_delay,
            next_query: Mutex::new(None),
            agent: ureq::AgentBuilder::new()
                .timeout_connect(Duration::from_secs(15))
                .timeout(Duration::from_secs(60))
                .build(),
            series_details_cache: Mutex::new(HashMap::new()),
            budget: None,
            cache: None,
            freshness: FreshnessPolicy::default(),
            offline: false,
        }
    }

    /// Installs the per-resource request budget. Every API call then
    /// waits for its resource to have room, and records itself.
    pub fn set_budget(&mut self, budget: Arc<crate::cache::budget::Budget>) {
        self.budget = Some(budget);
    }

    /// Installs the persistent Comic Vine cache used by normal scrapes.
    pub fn set_cache(&mut self, cache: Arc<dyn crate::cache::CvCache>) {
        self.cache = Some(cache);
    }

    /// Installs the freshness rule for the local-first issue-list
    /// read (ADR-071). The scraper wiring derives it from the same
    /// advanced keys as the budget.
    pub fn set_freshness(&mut self, policy: FreshnessPolicy) {
        self.freshness = policy;
    }

    /// Sets the offline switch (ADR-071): when on, every request dies
    /// at the chokepoint with [`CvError::Offline`].
    pub fn set_offline(&mut self, offline: bool) {
        self.offline = offline;
    }

    /// True when offline mode blocks every request (ADR-071).
    pub fn is_offline(&self) -> bool {
        self.offline
    }

    /// The installed cache, for the local-first reads (ADR-071).
    pub(crate) fn cache(&self) -> Option<&Arc<dyn crate::cache::CvCache>> {
        self.cache.as_ref()
    }

    pub(crate) fn freshness(&self) -> FreshnessPolicy {
        self.freshness
    }

    /// The stored issue detail JSON, when the cache holds one.
    /// A cache failure is a miss, never a scrape failure.
    pub(crate) fn cached_issue_detail(&self, issue_id: i64) -> Option<String> {
        let cache = self.cache.as_ref()?;
        match cache.issue_detail(issue_id) {
            Ok(Some((json, _))) => Some(json),
            Ok(None) => None,
            Err(error) => {
                crate::log::debug(&format!("could not read the cached issue detail: {error}"));
                None
            }
        }
    }

    /// The stored issue detail JSON for one issue. A cache failure is
    /// logged and ignored.
    pub(crate) fn put_issue_detail(&self, issue_id: i64, json: &str) {
        let Some(cache) = &self.cache else {
            return;
        };
        if let Err(error) = cache.put_issue_detail(issue_id, json) {
            crate::log::debug(&format!("could not cache the issue detail: {error}"));
        }
    }

    /// The stored volume row, when the cache holds one.
    pub(crate) fn cached_volume(&self, volume_id: i64) -> Option<crate::cache::VolumeRow> {
        let cache = self.cache.as_ref()?;
        match cache.volume(volume_id) {
            Ok(row) => row,
            Err(error) => {
                crate::log::debug(&format!("could not read the cached volume: {error}"));
                None
            }
        }
    }

    /// The stored volume objects of one cleaned search (ADR-071).
    pub(crate) fn cached_search(&self, terms: &str) -> Option<String> {
        let cache = self.cache.as_ref()?;
        match cache.search(terms) {
            Ok(Some((json, _))) => Some(json),
            Ok(None) => None,
            Err(error) => {
                crate::log::debug(&format!("could not read the cached search: {error}"));
                None
            }
        }
    }

    /// Stores the volume objects of one search under its cleaned
    /// terms. An empty result is not stored, so the alternate-terms
    /// retry keeps its chance on the next run.
    pub(crate) fn cache_search(&self, terms: &str, json: &str) {
        let Some(cache) = &self.cache else {
            return;
        };
        if let Err(error) = cache.put_search(terms, json) {
            crate::log::debug(&format!("could not cache the search result: {error}"));
        }
    }

    /// The stored image bytes for one URL (ADR-071).
    pub(crate) fn cached_image(&self, url: &str) -> Option<Vec<u8>> {
        let cache = self.cache.as_ref()?;
        match cache.image(url) {
            Ok(bytes) => bytes,
            Err(error) => {
                crate::log::debug(&format!("could not read the cached image: {error}"));
                None
            }
        }
    }

    /// Stores one downloaded cover. Images ride the CDN and stay
    /// outside the request budget (ADR-071).
    pub(crate) fn cache_image(&self, url: &str, bytes: &[u8]) {
        let Some(cache) = &self.cache else {
            return;
        };
        if let Err(error) = cache.put_image(url, bytes) {
            crate::log::debug(&format!("could not cache the image: {error}"));
        }
    }

    /// Saves volume metadata without making a cache failure fail the scrape.
    pub(crate) fn cache_volume(&self, row: &crate::cache::VolumeRow) {
        self.cache_volumes(std::slice::from_ref(row));
    }

    /// Saves volume metadata without making a cache failure fail the scrape.
    pub(crate) fn cache_volumes(&self, rows: &[crate::cache::VolumeRow]) {
        let Some(cache) = &self.cache else {
            return;
        };
        if let Err(error) = cache.put_volumes(rows) {
            crate::log::debug(&format!(
                "could not cache Comic Vine volume metadata: {error}"
            ));
        }
    }

    /// Saves an issue-number map without making a cache failure fail the scrape.
    pub(crate) fn cache_issues(&self, issues: &[crate::cache::IssueSkeleton]) {
        let Some(cache) = &self.cache else {
            return;
        };
        if let Err(error) = cache.put_issues(issues) {
            crate::log::debug(&format!("could not cache Comic Vine issue links: {error}"));
        }
    }

    /// The requests left for one API path in the current window, or
    /// `None` when no budget is installed.
    pub fn remaining_budget(&self, path: &str) -> Option<i64> {
        let budget = self.budget.as_ref()?;
        Some(budget.remaining(&crate::cache::budget::resource_of(path)))
    }

    /// The chokepoint: every API request passes here first.
    fn take_budget(&self, path: &str) -> Result<(), CvError> {
        let Some(budget) = &self.budget else {
            return Ok(());
        };
        let resource = crate::cache::budget::resource_of(path);
        if budget.acquire(&resource) {
            Ok(())
        } else {
            Err(CvError::BudgetSpent(resource))
        }
    }

    /// `wait_until_ready`: sleeps until the fixed delay has passed
    /// since the last call, then re-arms the timer.
    pub(crate) fn wait_until_ready(&self) {
        let mut next = self.next_query.lock().unwrap();
        if let Some(t) = *next {
            let now = Instant::now();
            if t > now {
                std::thread::sleep(t - now);
            }
        }
        *next = Some(Instant::now() + self.query_delay);
    }

    fn user_agent(&self) -> String {
        format!(
            "ComicVineScraper/{SCRAPER_VERSION} (https://github.com/cbanack/comic-vine-scraper/)"
        )
    }

    /// One throttled GET returning the response body (the C#
    /// `__get_page`): non-200 statuses and transport errors become
    /// `CvError::Connection`.
    fn get_page(&self, path: &str, query: &[(&str, String)]) -> Result<String, CvError> {
        // The offline chokepoint: before the budget, before the
        // throttle, before any network work (ADR-071).
        if self.offline {
            return Err(CvError::Offline);
        }
        self.take_budget(path)?;
        self.wait_until_ready();
        let url = format!("{}{}", self.base_url, path);
        let ua = self.user_agent();
        let mut request = self.agent.get(&url).set("User-Agent", &ua);
        for (k, v) in query {
            request = request.query(k, v);
        }
        // the request line without the API key
        let logged: Vec<String> = query
            .iter()
            .map(|(k, v)| {
                if *k == "api_key" {
                    format!("api_key=<redacted len={}>", v.len())
                } else {
                    format!("{k}={v}")
                }
            })
            .collect();
        crate::log::debug(&format!("GET {path}?{}", logged.join("&")));
        let response = request.call().map_err(|e| {
            crate::log::debug(&format!("GET {path} transport error: {e}"));
            CvError::Connection(url.clone(), transport_message(&e))
        })?;
        let status = response.status();
        crate::log::debug(&format!("GET {path} -> HTTP {status}"));
        if status != 200 {
            return Err(CvError::Connection(
                url,
                format!("server response code {status}"),
            ));
        }
        response
            .into_string()
            .map_err(|e| CvError::Connection(url, e.to_string()))
    }

    /// The C# `__get_dom`: fetch, require a non-empty body, parse as
    /// JSON, require `status_code == 1` — any failure retries once
    /// (after the retry delay and a fresh throttle wait), and the
    /// second failure surfaces.
    pub(crate) fn get_dom(&self, path: &str, query: &[(&str, String)]) -> Result<Value, CvError> {
        match self.try_get_dom(path, query) {
            Ok(dom) => Ok(dom),
            // A budget refusal or an offline refusal made no request,
            // so a retry would only refuse again.
            Err(error @ (CvError::BudgetSpent(_) | CvError::Offline)) => Err(error),
            Err(first) => {
                std::thread::sleep(self.retry_delay);
                self.try_get_dom(path, query).map_err(|second| {
                    CvError::BadResponse(format!(
                        "both attempts failed: {second}; first error: {first}"
                    ))
                })
            }
        }
    }

    fn try_get_dom(&self, path: &str, query: &[(&str, String)]) -> Result<Value, CvError> {
        let body = self.get_page(path, query)?;
        if body.trim().is_empty() {
            return Err(CvError::BadResponse(format!(
                "comicvine query returned an empty document: {path}"
            )));
        }
        let dom: Value = serde_json::from_str(&body).map_err(|e| {
            crate::log::debug(&format!(
                "GET {path} bad JSON: {e} (body starts {})",
                &body[..body.len().min(120)]
            ));
            CvError::BadResponse(format!("bad JSON from {path}: {e}"))
        })?;
        let status = dom
            .get("status_code")
            .and_then(Value::as_i64)
            .ok_or_else(|| CvError::BadResponse("empty comicvine dom: see bug 194".to_string()))?;
        if status != 1 {
            crate::log::debug(&format!(
                "GET {path} API status {status}: {}",
                dom.get("error").and_then(Value::as_str).unwrap_or("?")
            ));
            return Err(CvError::Status(
                status,
                dom.get("error").and_then(Value::as_str).map(String::from),
            ));
        }
        Ok(dom)
    }

    /// A throttled binary download with one retry (the C#
    /// `_query_image`).
    pub(crate) fn get_bytes(&self, url: &str) -> Option<Vec<u8>> {
        self.get_bytes_once(url)
            .or_else(|| {
                std::thread::sleep(self.retry_delay);
                self.get_bytes_once(url)
            })
            .into_iter()
            .next()
    }

    fn get_bytes_once(&self, url: &str) -> Option<Vec<u8>> {
        // The offline chokepoint covers the CDN too (ADR-071).
        if self.offline {
            return None;
        }
        self.wait_until_ready();
        let ua = self.user_agent();
        let response = self
            .agent
            .get(url)
            .set("User-Agent", &ua)
            .call()
            .map_err(|e| {
                crate::log::debug(&format!("GET image {url} transport error: {e}"));
                e
            })
            .ok()?;
        if response.status() != 200 {
            crate::log::debug(&format!("GET image {url} -> HTTP {}", response.status()));
            return None;
        }
        let mut bytes = Vec::new();
        use std::io::Read;
        let read = response
            .into_reader()
            .take(64 * 1024 * 1024)
            .read_to_end(&mut bytes)
            .ok()?;
        crate::log::debug(&format!("GET image {url} -> {read} bytes"));
        Some(bytes)
    }

    /// The shared query parameters every API call carries.
    pub(crate) fn base_query(&self) -> Vec<(&'static str, String)> {
        vec![
            ("api_key", self.api_key.clone()),
            ("format", "json".to_string()),
            ("client", "cvscraper".to_string()),
        ]
    }
}

fn transport_message(e: &ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, _) => format!("status {code}"),
        other => other.to_string(),
    }
}
