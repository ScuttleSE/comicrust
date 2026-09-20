//! Port of the plugin's `cvdb.py` + `db.py` — the ComicVine queries:
//! series search (paged), series details (cached per session), issue
//! listing for a series (paged), issue lookup by number (with the
//! alternate-number ladder), issue details, URL decoding, the magic
//! `cvinfo` file, and the search-term cleanup.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use fancy_regex::Regex;
use serde_json::Value;

use super::connection::{CvClient, CvError};
use super::models::{Issue, IssueRef, SeriesRef};
use crate::cache::freshness::{self, Verdict};
use crate::utils::convert_number_words;

/// A progress/cancel callback for the series search: the matches so
/// far and the expected callback count; returns `true` to cancel
/// (C# `callback_function(num_matches, expected)`).
pub type SeriesProgressFn<'a> = dyn FnMut(usize, usize) -> bool + 'a;

/// A progress/cancel callback for the issue-list query: the 0..1
/// completion ratio; returns `true` to cancel.
pub type IssueProgressFn<'a> = dyn FnMut(f64) -> bool + 'a;

/// A progress/cancel callback for paged queries: returns `true` to
/// cancel (C# `callback_function`).
#[deprecated(note = "use SeriesProgressFn / IssueProgressFn")]
pub type ProgressFn<'a> = dyn FnMut() -> bool + 'a;

/// The session-scoped caches the C# `db.py` keeps (`__series_ref_cache`
/// capped at 10 entries with a full reset, `__issue_refs_cache`
/// holding exactly one series).
#[derive(Default)]
struct SessionCaches {
    series_refs: Vec<(String, Vec<SeriesRef>)>,
    issue_refs: Option<(i64, Vec<IssueRef>)>,
}

impl SessionCaches {
    fn get_series(&self, terms: &str) -> Option<Vec<SeriesRef>> {
        self.series_refs
            .iter()
            .find(|(t, _)| t == terms)
            .map(|(_, v)| v.clone())
    }

    fn put_series(&mut self, terms: String, refs: Vec<SeriesRef>) {
        if self.series_refs.len() > 10 {
            self.series_refs.clear();
        }
        self.series_refs.push((terms, refs));
    }
}

/// The public query surface: a client plus its session caches.
pub struct Cv {
    pub client: CvClient,
    caches: SessionCaches,
}

impl Cv {
    pub fn new(client: CvClient) -> Self {
        Cv {
            client,
            caches: SessionCaches::default(),
        }
    }

    /// The database name (C# `get_db_name_s`).
    pub fn db_name(&self) -> &'static str {
        "ComicVine"
    }

    /// `create_key_tag_s`: `CVDB<id>` (None for non-positive keys).
    pub fn create_key_tag(&self, issue_key: i64) -> Option<String> {
        if issue_key > 0 {
            Some(format!("CVDB{issue_key}"))
        } else {
            None
        }
    }

    /// `parse_key_tag`: the `CVDB<number>` form, or the legacy
    /// `ComicVine[<number>` form.
    pub fn parse_key_tag(&self, text: &str) -> Option<i64> {
        static CVDB: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)CVDB(\d{1,})").unwrap());
        static LEGACY: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"(?i)ComicVine.?[(\[](\d{1,})").unwrap());
        if let Ok(Some(caps)) = CVDB.captures(text) {
            if let Some(n) = caps.get(1) {
                return n.as_str().parse().ok();
            }
        }
        LEGACY
            .captures(text)
            .ok()
            .flatten()
            .and_then(|c| c.get(1))
            .and_then(|n| n.as_str().parse().ok())
    }

    /// `check_magic_file`: reads `cvinfo.txt`/`cvinfo` from the book's
    /// directory and decodes the ComicVine URL inside into a series.
    /// The given path may be an existing directory OR any file path
    /// inside one (the file itself need not exist, C# parity).
    pub fn check_magic_file(&self, path: &str) -> Option<SeriesRef> {
        let is_dir = std::fs::metadata(path).map(|m| m.is_dir()).unwrap_or(false);
        let dir = if is_dir {
            std::path::PathBuf::from(path)
        } else {
            std::path::Path::new(path).parent()?.to_path_buf()
        };
        if !dir.is_dir() {
            return None;
        }
        // both candidate names are checked; the last one found wins
        // (C# loop parity)
        let mut file_path = None;
        for name in ["cvinfo.txt", "cvinfo"] {
            let candidate = dir.join(name);
            if candidate.is_file() {
                file_path = Some(candidate);
            }
        }
        let line = std::fs::read_to_string(file_path?).ok()?;
        self.url_to_series_ref(line.trim())
    }

    /// `db.query_series_refs`: strips the ignored search terms, tries
    /// a pasted ComicVine URL first, then searches; a failed plain
    /// search retries once with aggressively cleaned terms. Results
    /// are cached per terms string.
    pub fn query_series_refs(
        &mut self,
        search_terms: &str,
        ignored_terms: &[String],
        max_results: i32,
        progress: &mut SeriesProgressFn,
    ) -> Result<Vec<SeriesRef>, CvError> {
        let mut terms = search_terms.to_string();
        if !ignored_terms.is_empty() {
            let pattern = ignored_terms
                .iter()
                .map(|t| t.trim())
                .filter(|t| !t.is_empty() && t.chars().all(|c| c.is_alphanumeric()))
                .collect::<Vec<_>>()
                .join("|");
            if !pattern.is_empty() {
                if let Ok(re) = Regex::new(&format!(r"(?i)\b({pattern})\b")) {
                    terms = re.replace_all(&terms, "").to_string();
                }
            }
        }

        if let Some(cached) = self.caches.get_series(&terms) {
            return Ok(cached);
        }
        // Local-first (ADR-071): the persistent search result under
        // the cleaned terms parses through the same volume parser. An
        // unparseable stored result falls through to the network.
        let cleaned = cleanup_search_terms(&terms, false);
        if let Some(json) = self.client.cached_search(&cleaned) {
            if let Some(refs) = stored_search_refs(&json, max_results) {
                self.caches.put_series(terms, refs.clone());
                return Ok(refs);
            }
        }
        let (refs, raw_results) = self.uncached_series_refs(&terms, max_results, progress)?;
        let now = unix_now();
        let cached: Vec<crate::cache::VolumeRow> = refs
            .iter()
            .map(|series| crate::cache::VolumeRow {
                volume_id: series.series_key,
                name: Some(series.series_name().to_string()),
                publisher: (!series.publisher.is_empty()).then(|| series.publisher.clone()),
                start_year: (series.volume_year > 0).then_some(series.volume_year),
                count_of_issues: (series.issue_count > 0).then_some(series.issue_count),
                date_last_updated: None,
                last_cover_date: None,
                fetched_at: now,
            })
            .collect();
        self.client.cache_volumes(&cached);
        // The raw search results store under the cleaned terms for the
        // next run. An empty result is never stored, so the
        // alternate-terms retry keeps its chance (ADR-071).
        if !refs.is_empty() {
            if let Ok(json) = serde_json::to_string(&raw_results) {
                self.client.cache_search(&cleaned, &json);
            }
        }
        self.caches.put_series(terms, refs.clone());
        Ok(refs)
    }

    fn uncached_series_refs(
        &self,
        terms: &str,
        max_results: i32,
        progress: &mut SeriesProgressFn,
    ) -> Result<(Vec<SeriesRef>, Vec<Value>), CvError> {
        let mut refs = Vec::new();
        let mut raw_results = Vec::new();
        let cleaned = cleanup_search_terms(terms, false);
        if cleaned.is_empty() {
            return Ok((refs, raw_results));
        }

        // a pasted ComicVine URL wins outright
        if let Some(url_ref) = self.url_to_series_ref(terms) {
            refs.push(url_ref);
        }
        if refs.is_empty() {
            (refs, raw_results) = self.paged_series_search(&cleaned, max_results, progress)?;
        }
        if refs.is_empty() {
            let alt = cleanup_search_terms(&cleaned, true);
            if !terms.is_empty() && alt != cleaned {
                refs = self.paged_series_search(&alt, max_results, progress)?.0;
            }
        }
        Ok((refs, raw_results))
    }

    /// The paged volume search (the C# `__query_series_refs`): pages
    /// of 100, capped at `max_results`, cancellable. The raw volume
    /// objects ride along, aligned with the refs, so the persistent
    /// search cache stores what the API actually said (ADR-071).
    fn paged_series_search(
        &self,
        terms: &str,
        max_results: i32,
        progress: &mut SeriesProgressFn,
    ) -> Result<(Vec<SeriesRef>, Vec<Value>), CvError> {
        const PAGE_SIZE: usize = 100;
        let mut refs: Vec<SeriesRef> = Vec::new();
        let mut raw_results: Vec<Value> = Vec::new();
        let mut seen: BTreeSet<i64> = BTreeSet::new();
        let max = max_results.max(0) as usize;

        let dom = self.series_search_dom(terms, 1)?;
        let num_results = dom
            .get("number_of_total_results")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        crate::log::debug(&format!(
            "search \u{201C}{terms}\u{201D} page 1: {num_results} total results"
        ));
        if num_results <= 0 || result_items(&dom, "volume").is_empty() {
            return Ok((refs, raw_results));
        }
        let num_results = num_results as usize;

        collect_volumes(&dom, &mut refs, &mut raw_results, &mut seen, max);
        let mut iteration = PAGE_SIZE;
        let num_remaining_pages = num_results / PAGE_SIZE;
        let mut cancelled = progress(refs.len(), num_remaining_pages);
        while iteration < num_results && refs.len() < max && !cancelled {
            let dom = self.series_search_dom(terms, iteration / PAGE_SIZE + 1)?;
            iteration += PAGE_SIZE;
            cancelled = progress(refs.len(), num_remaining_pages);
            let has_page = dom
                .get("number_of_page_results")
                .and_then(Value::as_i64)
                .unwrap_or(0)
                >= 1;
            if has_page {
                collect_volumes(&dom, &mut refs, &mut raw_results, &mut seen, max);
            }
        }
        Ok((
            if cancelled { Vec::new() } else { refs },
            if cancelled { Vec::new() } else { raw_results },
        ))
    }

    fn series_search_dom(&self, terms: &str, page: usize) -> Result<Value, CvError> {
        // page=1 is left off the query (the C# comment: fixes a bug)
        let mut query = self.client.base_query();
        query.push(("limit", "100".to_string()));
        query.push(("resources", "volume".to_string()));
        query.push((
            "field_list",
            "name,start_year,publisher,id,image,count_of_issues".to_string(),
        ));
        query.push(("query", terms.to_string()));
        if page > 1 {
            query.push(("page", page.to_string()));
        }
        self.client.get_dom("/search/", &query)
    }

    /// All the issues of a series, paged; cached per series (the
    /// cache holds exactly one entry, like the C#).
    pub fn query_issue_refs(
        &mut self,
        series_ref: &SeriesRef,
        progress: &mut IssueProgressFn,
    ) -> Result<Vec<IssueRef>, CvError> {
        if let Some((key, refs)) = &self.caches.issue_refs {
            if *key == series_ref.series_key {
                return Ok(refs.clone());
            }
        }
        // Local-first (ADR-071): a FRESH cached issue list serves with
        // zero requests. Every other verdict — or any cache failure —
        // runs the online path below. The one-request revalidation
        // probe is the refresh switch's business (ADR-071, T4).
        if let Some(refs) = self.fresh_cached_issue_refs(series_ref) {
            self.caches.issue_refs = Some((series_ref.series_key, refs.clone()));
            return Ok(refs);
        }
        let refs = self.uncached_issue_refs(series_ref, progress)?;
        let cached: Vec<crate::cache::IssueSkeleton> = refs
            .iter()
            .map(|issue| crate::cache::IssueSkeleton {
                issue_id: issue.issue_key,
                volume_id: series_ref.series_key,
                issue_number: issue.issue_num.clone(),
                cover_date: None,
                name: (!issue.title.is_empty()).then(|| issue.title.clone()),
                ..Default::default()
            })
            .collect();
        self.client.cache_issues(&cached);
        self.caches.issue_refs = Some((series_ref.series_key, refs.clone()));
        Ok(refs)
    }

    fn uncached_issue_refs(
        &self,
        series_ref: &SeriesRef,
        progress: &mut IssueProgressFn,
    ) -> Result<Vec<IssueRef>, CvError> {
        const PAGE_SIZE: usize = 100;
        let mut refs: Vec<IssueRef> = Vec::new();
        let mut seen: BTreeSet<i64> = BTreeSet::new();

        let dom = self.issues_dom(&series_ref.series_key.to_string(), 1)?;
        let num_results = dom
            .get("number_of_total_results")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        if num_results <= 0 {
            return Ok(refs);
        }

        collect_issues(&dom, &mut refs, &mut seen);
        let mut iteration = PAGE_SIZE;
        let ratio = |done: usize| done as f64 / num_results.max(1) as f64;
        let mut cancelled = progress(ratio(iteration.min(num_results as usize)));
        while iteration < num_results as usize && !cancelled {
            let dom = self.issues_dom(
                &series_ref.series_key.to_string(),
                iteration / PAGE_SIZE + 1,
            )?;
            iteration += PAGE_SIZE;
            let has_page = dom
                .get("number_of_page_results")
                .and_then(Value::as_i64)
                .unwrap_or(0)
                >= 1;
            if has_page {
                collect_issues(&dom, &mut refs, &mut seen);
            }
            cancelled = progress(ratio(iteration.min(num_results as usize)));
        }
        Ok(if cancelled { Vec::new() } else { refs })
    }

    fn issues_dom(&self, series_id: &str, page: usize) -> Result<Value, CvError> {
        let mut query = self.client.base_query();
        query.push(("field_list", "name,issue_number,id,image".to_string()));
        query.push(("filter", format!("volume:{series_id}")));
        if page > 1 {
            query.push(("page", page.to_string()));
            query.push(("offset", ((page - 1) * 100).to_string()));
        }
        self.client.get_dom("/issues/", &query)
    }

    /// The cached issue list of a series, when the freshness rule
    /// calls it fresh (ADR-071). `None` means "go online"; a cache
    /// failure never fails a scrape.
    fn fresh_cached_issue_refs(&self, series_ref: &SeriesRef) -> Option<Vec<IssueRef>> {
        let cache = self.client.cache()?;
        let stored = cache.volume(series_ref.series_key).ok().flatten();
        let count = cache.issue_count(series_ref.series_key).unwrap_or(0);
        if freshness::verdict(stored.as_ref(), count, unix_now(), &self.client.freshness())
            != Verdict::Fresh
        {
            return None;
        }
        let skeletons = cache.issues_of_volume(series_ref.series_key).ok()?;
        Some(
            skeletons
                .into_iter()
                .map(|s| {
                    IssueRef::new(
                        &s.issue_number,
                        s.issue_id,
                        s.name.as_deref().unwrap_or(""),
                        None,
                    )
                })
                .collect(),
        )
    }

    /// `db.query_issue_ref`: the issue in the given series with the
    /// given number, retrying with the ½ alternate form when the
    /// exact number yields nothing (attempts <= 3, C# parity).
    pub fn query_issue_ref(
        &self,
        series_ref: &SeriesRef,
        issue_num: &str,
    ) -> Result<Option<IssueRef>, CvError> {
        let mut issue_num = issue_num.to_string();
        let mut attempts = 1;
        loop {
            let mut query = self.client.base_query();
            query.push(("field_list", "name,issue_number,id,image".to_string()));
            query.push((
                "filter",
                format!(
                    "volume:{},issue_number:{}",
                    series_ref.series_key, issue_num
                ),
            ));
            let dom = self.client.get_dom("/issues/", &query)?;
            let num_results = dom
                .get("number_of_total_results")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            if num_results == 1 {
                // JSON: `results` is the issue object itself (the C#
                // XML dom had the `results.issue` wrapper).
                let found = match dom.get("results") {
                    Some(single) if single.get("id").is_some() => Some(issue_to_issueref(single)),
                    _ => result_items(&dom, "issue")
                        .first()
                        .map(|i| issue_to_issueref(i)),
                };
                return Ok(found);
            }
            if num_results != 0 || attempts > 3 {
                return Ok(None);
            }
            attempts += 1;
            let alt = alternate_issue_num(&issue_num);
            if alt == issue_num {
                return Ok(None);
            }
            issue_num = alt;
        }
    }

    /// `db.query_issue`: the full issue details. `slow_data` adds the
    /// associated-images fetch (the C# passes the SCRAPE_RATING flag
    /// here).
    pub fn query_issue(&self, issue_ref: &IssueRef, slow_data: bool) -> Result<Issue, CvError> {
        // Local-first (ADR-071): a stored detail parses through the
        // same parser as the live response. A stored row that fails
        // to parse falls through to the network and gets replaced.
        if let Some(json) = self.client.cached_issue_detail(issue_ref.issue_key) {
            if let Ok(value) = serde_json::from_str::<Value>(&json) {
                let results = value.get("results").unwrap_or(&value);
                let mut issue = parse_issue_results(results, &self.client)?;
                if slow_data {
                    parse_associated_images_results(results, &mut issue);
                }
                return Ok(issue);
            }
        }
        let query = self.client.base_query();
        let dom = self
            .client
            .get_dom(&format!("/issue/4000-{}/", issue_ref.issue_key), &query)?;
        // The full response stores for the next run (ADR-071): the
        // serialized `results` object, the shape every issue-detail
        // store keeps (ADR-064).
        if let Some(results) = dom.get("results") {
            if let Ok(json) = serde_json::to_string(results) {
                self.client.put_issue_detail(issue_ref.issue_key, &json);
            }
        }
        let mut issue = parse_issue(&dom, &self.client)?;
        if slow_data {
            parse_associated_images(&dom, &mut issue);
        }
        Ok(issue)
    }

    /// The image bytes for a URL (the C# `_query_image`). The blob
    /// cache serves first; a miss downloads and stores. Images ride
    /// the CDN and stay outside the request budget (ADR-071).
    pub fn query_image(&self, url: &str) -> Option<Vec<u8>> {
        if let Some(bytes) = self.client.cached_image(url) {
            return Some(bytes);
        }
        let bytes = self.client.get_bytes(url)?;
        self.client.cache_image(url, &bytes);
        Some(bytes)
    }

    /// `__url_to_seriesref`: a ComicVine URL carrying `4000-<num>`
    /// (an issue — resolved to its series) or `4050-`/`49-<num>` (a
    /// series) becomes a SeriesRef; anything else is None.
    pub fn url_to_series_ref(&self, url: &str) -> Option<SeriesRef> {
        static ISSUE: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"(?i)\A.*?\b(4000)-(?P<num>\d{2,})\b.*$").unwrap());
        static SERIES: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(r"(?i)\A.*?\b(49|4050)-(?P<num>\d{2,})\b.*$").unwrap());

        let mut series_id: Option<String> = None;
        if let Some(num) = ISSUE
            .captures(url)
            .ok()
            .flatten()
            .and_then(|c| c.name("num").map(|m| m.as_str().to_string()))
        {
            // an issue URL: resolve the issue to find its volume
            let query = self.client.base_query();
            if let Ok(dom) = self.client.get_dom(&format!("/issue/4000-{num}/"), &query) {
                if dom.get("number_of_total_results").and_then(Value::as_i64) == Some(1) {
                    series_id =
                        value_i64(dom.pointer("/results/volume/id")).map(|id| id.to_string());
                }
            }
        }
        if series_id.is_none() {
            if let Some(num) = SERIES
                .captures(url)
                .ok()
                .flatten()
                .and_then(|c| c.name("num").map(|m| m.as_str().to_string()))
            {
                series_id = Some(num);
            }
        }
        let id = series_id?;
        let mut query = self.client.base_query();
        query.push((
            "field_list",
            "name,start_year,publisher,image,count_of_issues,id".to_string(),
        ));
        let dom = self
            .client
            .get_dom(&format!("/volume/4050-{id}/"), &query)
            .ok()?;
        if value_i64(dom.get("number_of_total_results")) == Some(1) {
            dom.get("results").and_then(volume_to_seriesref)
        } else {
            None
        }
    }
}

/// `__volume_to_seriesref`.
fn volume_to_seriesref(volume: &Value) -> Option<SeriesRef> {
    let id = value_i64(volume.get("id"))?;
    let name = value_string(volume.get("name")).unwrap_or_default();
    let publisher = volume
        .get("publisher")
        .filter(|p| p.as_object().is_some_and(|o| o.len() > 1))
        .and_then(|p| p.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let start_year = value_string(volume.get("start_year")).unwrap_or_default();
    let volume_year = parse_year_trailing(&start_year).unwrap_or(-1);
    let count = value_i64(volume.get("count_of_issues")).unwrap_or(0) as i32;
    SeriesRef::new(
        id,
        &name,
        volume_year,
        publisher,
        count,
        parse_image_url(volume),
    )
    .ok()
}

/// `sstr(volume.start_year).rstrip("- ")` — trailing dashes and
/// spaces come off (bug 334) before the parse.
fn parse_year_trailing(s: &str) -> Option<i32> {
    s.trim_end_matches(['-', ' ']).parse::<i32>().ok()
}

/// The result items of a list endpoint. The JSON API returns
/// `results` as a FLAT array (search, the filtered issues list); the
/// C# XML dom wrapped the elements in a container (`results.volume`).
/// Both shapes parse — the flat array is the real one.
fn result_items<'a>(dom: &'a Value, key: &str) -> Vec<&'a Value> {
    let results = match dom.get("results") {
        Some(results) => results,
        None => return Vec::new(),
    };
    match results {
        Value::Array(items) => items.iter().collect(),
        // the XML-shape tolerance: {"results": {"volume": [...]}}
        wrapper => match wrapper.get(key) {
            Some(Value::Array(items)) => items.iter().collect(),
            Some(single) => vec![single],
            None => Vec::new(),
        },
    }
}

fn collect_volumes(
    dom: &Value,
    refs: &mut Vec<SeriesRef>,
    raw: &mut Vec<Value>,
    seen: &mut BTreeSet<i64>,
    max: usize,
) {
    for volume in result_items(dom, "volume") {
        if refs.len() >= max {
            break;
        }
        if let Some(reference) = volume_to_seriesref(volume) {
            if seen.insert(reference.series_key) {
                refs.push(reference);
                raw.push(volume.clone());
            }
        }
    }
}

/// The refs of one stored search result (ADR-071): the raw volume
/// objects parse through the same `volume_to_seriesref` as the live
/// path. `None` means the stored text is unusable — the caller goes
/// online.
fn stored_search_refs(json: &str, max_results: i32) -> Option<Vec<SeriesRef>> {
    let items = serde_json::from_str::<Value>(json).ok()?;
    let items = items.as_array()?;
    let mut refs: Vec<SeriesRef> = Vec::new();
    let mut seen: BTreeSet<i64> = BTreeSet::new();
    let max = max_results.max(0) as usize;
    for volume in items {
        if refs.len() >= max {
            break;
        }
        if let Some(reference) = volume_to_seriesref(volume) {
            if seen.insert(reference.series_key) {
                refs.push(reference);
            }
        }
    }
    (!refs.is_empty()).then_some(refs)
}

fn collect_issues(dom: &Value, refs: &mut Vec<IssueRef>, seen: &mut BTreeSet<i64>) {
    for issue in result_items(dom, "issue") {
        let reference = issue_to_issueref(issue);
        if seen.insert(reference.issue_key) {
            refs.push(reference);
        }
    }
}

/// `__issue_to_issueref`.
fn issue_to_issueref(issue: &Value) -> IssueRef {
    let issue_num = value_string(issue.get("issue_number")).unwrap_or_default();
    let title = value_string(issue.get("name")).unwrap_or_default();
    let key = value_i64(issue.get("id")).unwrap_or(0);
    IssueRef::new(&issue_num, key, &title, parse_image_url(issue))
}

/// `__alternate_issue_num_s` — the `.5`/`½`/`0½` juggling. The `else`
/// branch's `.replace()` calls are string-literal replacements in the
/// C# (regex text never occurs in numbers), so they stay no-ops.
fn alternate_issue_num(issue_num: &str) -> String {
    static HALF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\A0*.50*").unwrap());
    if HALF.is_match(issue_num).unwrap_or(false) || issue_num == "½" {
        "0½".to_string()
    } else if issue_num == "0½" {
        "½".to_string()
    } else {
        issue_num.to_string()
    }
}

/// `__parse_image_url`: small, medium, large, super, thumb — the
/// first string present wins. The cache backfill reuses this rule, so
/// a stored `image_url` is the URL a scrape would use (ADR-070).
pub(crate) fn parse_image_url(dom: &Value) -> Option<String> {
    let image = dom.get("image")?;
    for field in [
        "small_url",
        "medium_url",
        "large_url",
        "super_url",
        "thumb_url",
    ] {
        if let Some(url) = image.get(field).and_then(Value::as_str) {
            if !url.is_empty() {
                return Some(url.to_string());
            }
        }
    }
    None
}

/// The C# dom values are strings; the JSON API returns numbers for
/// ids. Both parse (C# `is_string`/`int(...)` union behavior).
fn value_i64(v: Option<&Value>) -> Option<i64> {
    match v {
        Some(Value::Number(n)) => n.as_i64(),
        Some(Value::String(s)) => s.trim().parse().ok(),
        _ => None,
    }
}

fn value_string(v: Option<&Value>) -> Option<String> {
    match v {
        Some(Value::String(s)) => Some(s.clone()),
        _ => None,
    }
}

/// The C# `__as_list`: the element itself, a one-element list, or [].
fn as_list(v: Option<&Value>) -> Vec<&Value> {
    match v {
        Some(Value::Array(items)) => items.iter().collect(),
        Some(single) => vec![single],
        None => Vec::new(),
    }
}

// ==========================================================================
// issue details parsing (the __issue_parse_* family)

/// All five `__issue_parse_*` steps over one issue dom.
fn parse_issue(dom: &Value, client: &CvClient) -> Result<Issue, CvError> {
    let results = dom
        .get("results")
        .ok_or_else(|| CvError::BadResponse("issue dom has no results".to_string()))?;
    parse_issue_results(results, client)
}

/// The parser over one issue's `results` object. Both the live path
/// and the stored-detail read (ADR-071) come through here.
fn parse_issue_results(results: &Value, client: &CvClient) -> Result<Issue, CvError> {
    let key = value_i64(results.get("id")).unwrap_or(0);
    let mut issue = Issue::new(key);

    if let Some(id) = value_i64(results.pointer("/volume/id")) {
        issue.series_key = id.to_string();
    }
    if let Some(name) = results.pointer("/volume/name").and_then(Value::as_str) {
        issue.series_name = name.trim().to_string();
    }
    if let Some(num) = results.get("issue_number").and_then(Value::as_str) {
        issue.issue_num = num.trim().to_string();
    }
    if let Some(url) = results.get("site_detail_url").and_then(Value::as_str) {
        if url.starts_with("http") {
            issue.webpage = url.to_string();
        }
    }
    if let Some(title) = results.get("name").and_then(Value::as_str) {
        issue.title = title.trim().to_string();
    }

    // the published (front cover) date, "YYYY-MM-DD"
    if let Some(date) = results.get("cover_date").and_then(Value::as_str) {
        if date.chars().count() > 1 {
            let parts: Vec<i32> = date.split('-').filter_map(|p| p.parse().ok()).collect();
            if !parts.is_empty() {
                issue.pub_year = parts[0];
            }
            if parts.len() >= 2 {
                issue.pub_month = parts[1];
            }
            if parts.len() >= 3 {
                issue.pub_day = parts[2];
            }
        }
    }
    // the released (in store) date
    if let Some(date) = results.get("store_date").and_then(Value::as_str) {
        if date.chars().count() > 1 {
            let parts: Vec<i32> = date.split('-').filter_map(|p| p.parse().ok()).collect();
            if !parts.is_empty() {
                issue.rel_year = parts[0];
            }
            if parts.len() >= 2 {
                issue.rel_month = parts[1];
            }
            if parts.len() >= 3 {
                issue.rel_day = parts[2];
            }
        }
    }
    if let Some(url) = parse_image_url(results) {
        issue.image_urls.push(url);
    }

    // series details (volume year + publisher), through the cache
    let series_id = value_i64(results.pointer("/volume/id")).unwrap_or(0);
    let (volume_year, publisher) = series_details(client, series_id, results)?;
    let parent = super::imprints::find_parent_publisher(&publisher);
    issue.volume_year = volume_year;
    if parent != publisher {
        issue.publisher = parent;
        issue.imprint = publisher;
    } else {
        issue.publisher = publisher;
    }
    parse_story_credits(results, &mut issue);
    parse_summary(results, &mut issue);
    parse_roles(results, &mut issue);
    Ok(issue)
}

/// `__issue_parse_series_details`: volume year + publisher for the
/// series, always through the dedicated details query (the C#
/// `field_list` parse relies on it), cached per session.
fn series_details(
    client: &CvClient,
    series_id: i64,
    _results: &Value,
) -> Result<(i32, String), CvError> {
    if let Some(cached) = client
        .series_details_cache
        .lock()
        .unwrap()
        .get(&series_id)
        .cloned()
    {
        return Ok(cached);
    }
    // Local-first (ADR-071): the cached volume row answers when it
    // holds either field. A row that holds neither (an MCL import, a
    // manual name-only row) stays a miss and the volume query runs.
    if series_id > 0 {
        if let Some(row) = client.cached_volume(series_id) {
            let year = row.start_year.filter(|year| *year > 0);
            let publisher = row.publisher.filter(|p| !p.is_empty());
            if year.is_some() || publisher.is_some() {
                let volume_year = year.unwrap_or(-1);
                let publisher = publisher.unwrap_or_default();
                client
                    .series_details_cache
                    .lock()
                    .unwrap()
                    .insert(series_id, (volume_year, publisher.clone()));
                return Ok((volume_year, publisher));
            }
        }
    }
    let mut volume_year = -1;
    let mut publisher = String::new();
    let mut query = client.base_query();
    query.push((
        "field_list",
        "name,start_year,publisher,image,count_of_issues,id".to_string(),
    ));
    let dom = client.get_dom(&format!("/volume/4050-{series_id}/"), &query)?;
    if dom.get("results").is_none() {
        return Err(CvError::BadResponse(format!(
            "can't get details about series {series_id}"
        )));
    }
    if let Some(y) = dom
        .pointer("/results/start_year")
        .and_then(Value::as_str)
        .and_then(|s| s.parse().ok())
    {
        volume_year = y;
    }
    if let Some(p) = dom
        .pointer("/results/publisher")
        .filter(|p| p.as_object().is_some_and(|o| o.len() > 1))
        .and_then(|p| p.get("name"))
        .and_then(Value::as_str)
    {
        publisher = p.to_string();
    }
    let now = unix_now();
    client.cache_volume(&crate::cache::VolumeRow {
        volume_id: series_id,
        name: dom
            .pointer("/results/name")
            .and_then(Value::as_str)
            .map(str::to_string),
        publisher: (!publisher.is_empty()).then(|| publisher.clone()),
        start_year: (volume_year > 0).then_some(volume_year),
        count_of_issues: dom
            .pointer("/results/count_of_issues")
            .and_then(Value::as_i64)
            .map(|count| count as i32),
        date_last_updated: None,
        last_cover_date: None,
        fetched_at: now,
    });
    client
        .series_details_cache
        .lock()
        .unwrap()
        .insert(series_id, (volume_year, publisher.clone()));
    Ok((volume_year, publisher))
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}
/// `__issue_parse_story_credits`: story arcs (crossovers), characters,
/// teams, locations — each a list of `{name}` objects.
fn parse_story_credits(results: &Value, issue: &mut Issue) {
    let arcs = as_list(results.pointer("/story_arc_credits/story_arc"));
    issue.crossovers = names(&arcs);
    let characters = as_list(results.pointer("/character_credits/character"));
    issue.characters = names(&characters);
    let teams = as_list(results.pointer("/team_credits/team"));
    issue.teams = names(&teams);
    let locations = as_list(results.pointer("/location_credits/location"));
    issue.locations = names(&locations);
}

fn names(items: &[&Value]) -> Vec<String> {
    items
        .iter()
        .filter_map(|item| item.get("name"))
        .filter_map(Value::as_str)
        .map(|s| s.to_string())
        .collect()
}

/// `__issue_parse_summary`: the description massage.
fn parse_summary(results: &Value, issue: &mut Issue) {
    static OVERVIEW: LazyLock<Regex> = LazyLock::new(|| Regex::new("Overview").unwrap());
    static PARAGRAPH: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"<[bB][rR] ?/?>|<[Pp] ?>").unwrap());
    static NBSP: LazyLock<Regex> = LazyLock::new(|| Regex::new("&nbsp;?").unwrap());
    static MULTISPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(" {2,}").unwrap());
    static STRIP_TAGS: LazyLock<Regex> = LazyLock::new(|| Regex::new("<.*?>").unwrap());
    static LIST_OF_COVERS: LazyLock<Regex> =
        LazyLock::new(|| Regex::new("(?is)list of covers.*$").unwrap());

    let Some(description) = results.get("description").and_then(Value::as_str) else {
        return;
    };
    let mut summary = OVERVIEW.replace_all(description, "").to_string();
    summary = PARAGRAPH.replace_all(&summary, "\n").to_string();
    summary = STRIP_TAGS.replace_all(&summary, "").to_string();
    summary = MULTISPACES.replace_all(&summary, " ").to_string();
    summary = NBSP.replace_all(&summary, " ").to_string();
    // the C# runs the paragraph sub a second time
    summary = PARAGRAPH.replace_all(&summary, "\n").to_string();
    summary = summary.replace("&amp;", "&");
    summary = summary.replace("&quot;", "\"");
    summary = summary.replace("&lt;", "<");
    summary = summary.replace("&gt;", ">");
    summary = LIST_OF_COVERS.replace_all(&summary, "").to_string();
    issue.summary = summary.trim().to_string();
}

/// `__issue_parse_roles`: the ComicVine role dictionary, roles split
/// on commas, `artist` counting for both penciller and inker.
fn parse_roles(results: &Value, issue: &mut Issue) {
    for person in people(&people_list(results)) {
        for role in person.roles.iter().map(|r| r.trim()) {
            match role {
                "writer" => push_name(&mut issue.writers, &person.name),
                "penciler" => push_name(&mut issue.pencillers, &person.name),
                "artist" => {
                    push_name(&mut issue.pencillers, &person.name);
                    push_name(&mut issue.inkers, &person.name);
                }
                "inker" => push_name(&mut issue.inkers, &person.name),
                "cover" => push_name(&mut issue.cover_artists, &person.name),
                "editor" => push_name(&mut issue.editors, &person.name),
                "colorer" => push_name(&mut issue.colorists, &person.name),
                "colorist" => push_name(&mut issue.colorists, &person.name),
                "letterer" => push_name(&mut issue.letterers, &person.name),
                _ => {}
            }
        }
    }
}

struct Person {
    name: String,
    roles: Vec<String>,
}

fn people_list(results: &Value) -> Vec<&Value> {
    as_list(results.pointer("/person_credits/person"))
}

fn people(items: &[&Value]) -> Vec<Person> {
    items
        .iter()
        .map(|item| Person {
            name: item
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            roles: item
                .get("role")
                .and_then(Value::as_str)
                .unwrap_or("")
                .split(',')
                .map(|r| r.trim().to_string())
                .collect(),
        })
        .collect()
}

fn push_name(list: &mut Vec<String>, name: &str) {
    list.push(name.to_string());
}

/// `__parse_associated_images`: the alternate cover urls (the
/// `slow_data` path).
fn parse_associated_images(dom: &Value, issue: &mut Issue) {
    match dom.get("results") {
        Some(results) => parse_associated_images_results(results, issue),
        None => parse_associated_images_results(dom, issue),
    }
}

/// The same parse over one issue's `results` object (the stored
/// detail shape, ADR-071).
fn parse_associated_images_results(results: &Value, issue: &mut Issue) {
    let images = results.get("associated_images");
    let Some(items) = images.and_then(Value::as_array) else {
        return;
    };
    for item in items {
        match item.get("original_url") {
            Some(Value::String(url)) => issue.image_urls.push(url.clone()),
            Some(Value::Array(urls)) => {
                for url in urls {
                    if let Some(u) = url.as_str() {
                        issue.image_urls.push(u.to_string());
                    }
                }
            }
            _ => {}
        }
    }
}

// ==========================================================================
// search-term cleanup (the C# __cleanup_search_terms)

static BAD_SYMBOLS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(c2c|ctc|noads+|tbp)\b").unwrap());
static WORD_CHARS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[\w':.-]{1,}").unwrap());

/// `__cleanup_search_terms`: lowercase, `&` → `and`, drop the scanner
/// symbols, optionally convert number words (expand, then contract if
/// nothing changed), and strip punctuation outside `[\w':.-]`.
pub fn cleanup_search_terms(terms: &str, alt: bool) -> String {
    let mut out = terms.to_lowercase();
    out = out.replace(" & ", " and ");
    out = BAD_SYMBOLS.replace_all(&out, "").to_string();

    if alt {
        let orig = out.clone();
        out = convert_number_words(&out, true);
        if out == orig {
            out = convert_number_words(&out, false);
        }
    }

    let words: Vec<&str> = WORD_CHARS
        .find_iter(&out)
        .flatten()
        .map(|m| m.as_str())
        .collect();
    words.join(" ")
}
