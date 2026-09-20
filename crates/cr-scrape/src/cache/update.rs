//! The in-app all-endpoint Comic Vine update (ADR-075).
//!
//! One update walks every endpoint (publishers, people, volumes,
//! issues) over `filter=date_last_updated:<since>|<now>`, the one
//! proven filter (a live probe on 2026-09-20 confirmed it narrows all
//! four endpoints). Each endpoint carries its own `sync_state`
//! watermark — the date it is caught up through. A run stamps every
//! row with the real API `date_last_updated`, so this is the mechanism
//! that fills the empty stamps the localcv import left behind.
//!
//! The API is rate-limited per resource per hour, so a user weeks or
//! months behind runs many sessions. The update is resumable: it saves
//! its page offset in `sync_state.resume_state` and continues on the
//! next run. It runs on a worker thread (Rule 9); offline mode refuses
//! before any request (ADR-071). The standalone `scripts/cvcache`
//! `update` command shares the same watermark model and merge rule.

use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;

use super::resources::{ResourceKind, ResourceRef};
use super::{CvCache, IssueSkeleton, ResourceRow, SyncState, VolumeRow};
use crate::cv::connection::{CvClient, CvError};
use crate::cv::queries::parse_image_url;

/// The API caps a page at 100 results.
pub const PAGE_SIZE: i64 = 100;

/// The endpoints an update walks, cheap to expensive.
pub const ENDPOINTS: &[&str] = &["publishers", "people", "volumes", "issues"];

/// The floor date for an endpoint with no watermark yet.
const FLOOR_DATE: &str = "1970-01-01";

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// What one endpoint's update did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EndpointReport {
    pub endpoint: String,
    pub fetched: usize,
    pub pages: usize,
    pub complete: bool,
    /// The window end this endpoint is now caught up through.
    pub last_sync: String,
}

/// What one full update run did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UpdateReport {
    pub endpoints: Vec<EndpointReport>,
}

/// Progress for one page.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UpdateProgress {
    pub endpoint: String,
    pub fetched: usize,
    pub total: i64,
}

/// The list-level field list per endpoint. Only cache-stored fields
/// are requested, to keep the response small.
fn field_list(endpoint: &str) -> &'static str {
    match endpoint {
        "publishers" | "people" => "id,name,image,date_added,date_last_updated",
        "volumes" => {
            "id,name,publisher,start_year,count_of_issues,\
             date_added,date_last_updated"
        }
        // The issues field list matches the sweep expansion (ADR-072).
        _ => {
            "id,issue_number,volume,name,cover_date,deck,description,store_date,\
             image,date_added,date_last_updated,site_detail_url,api_detail_url"
        }
    }
}

/// The current date in the API `YYYY-MM-DD` form.
fn today() -> String {
    chrono::Utc::now().format("%Y-%m-%d").to_string()
}

/// Reads the stored watermark and resume offset for one endpoint.
fn watermark(cache: &dyn CvCache, endpoint: &str) -> (String, i64) {
    let Some(state) = cache.sync_state(endpoint).ok().flatten() else {
        return (FLOOR_DATE.to_string(), 0);
    };
    let since = if state.last_sync.trim().is_empty() {
        FLOOR_DATE.to_string()
    } else {
        state.last_sync
    };
    let offset = state
        .resume_state
        .as_deref()
        .and_then(|s| serde_json::from_str::<Value>(s).ok())
        .and_then(|v| v.get("offset").and_then(Value::as_i64))
        .unwrap_or(0);
    (since, offset)
}

fn save_watermark(cache: &dyn CvCache, endpoint: &str, last_sync: &str, offset: Option<i64>) {
    let resume_state = offset.map(|o| format!("{{\"offset\":{o}}}"));
    let _ = cache.put_sync_state(&SyncState {
        endpoint: endpoint.to_string(),
        last_sync: last_sync.to_string(),
        resume_state,
    });
}

/// Runs a full update over every endpoint in `endpoints`. Offline mode
/// refuses before any request. `on_progress` runs after each page and
/// must not block (the run is on a worker thread, Rule 9).
pub fn run(
    client: &CvClient,
    cache: &dyn CvCache,
    endpoints: &[&str],
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(UpdateProgress),
) -> Result<UpdateReport, CvError> {
    if client.is_offline() {
        return Err(CvError::Offline);
    }
    let now = today();
    let mut report = UpdateReport::default();
    for endpoint in endpoints {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let one = update_endpoint(client, cache, endpoint, &now, cancel, &mut on_progress)?;
        report.endpoints.push(one);
    }
    Ok(report)
}

fn update_endpoint(
    client: &CvClient,
    cache: &dyn CvCache,
    endpoint: &str,
    now: &str,
    cancel: &AtomicBool,
    on_progress: &mut impl FnMut(UpdateProgress),
) -> Result<EndpointReport, CvError> {
    let (since, mut offset) = watermark(cache, endpoint);
    let mut report = EndpointReport {
        endpoint: endpoint.to_string(),
        last_sync: since.clone(),
        ..Default::default()
    };
    loop {
        if cancel.load(Ordering::Relaxed) {
            // Stopped early: hold `since`, keep the resume offset.
            save_watermark(cache, endpoint, &since, Some(offset));
            return Ok(report);
        }
        let dom = page(client, endpoint, &since, now, offset)?;
        report.pages += 1;
        let results = dom
            .get("results")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let page_len = results.len() as i64;
        store_page(cache, endpoint, &results)?;
        report.fetched += results.len();
        offset += page_len;
        let total = dom
            .get("number_of_total_results")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        on_progress(UpdateProgress {
            endpoint: endpoint.to_string(),
            fetched: report.fetched,
            total,
        });
        if page_len < PAGE_SIZE || offset >= total {
            // Caught up: advance the watermark to `now`, clear resume.
            save_watermark(cache, endpoint, now, None);
            report.complete = true;
            report.last_sync = now.to_string();
            return Ok(report);
        }
        // Mid-window: hold `since`, save the offset after every page so
        // a stop resumes.
        save_watermark(cache, endpoint, &since, Some(offset));
    }
}

fn page(
    client: &CvClient,
    endpoint: &str,
    since: &str,
    now: &str,
    offset: i64,
) -> Result<Value, CvError> {
    let mut query = client.base_query();
    query.push(("field_list", field_list(endpoint).to_string()));
    query.push(("filter", format!("date_last_updated:{since}|{now}")));
    query.push(("sort", "date_last_updated:asc".to_string()));
    query.push(("limit", PAGE_SIZE.to_string()));
    query.push(("offset", offset.to_string()));
    client.get_dom(&format!("/{endpoint}/"), &query)
}

fn store_page(cache: &dyn CvCache, endpoint: &str, results: &[Value]) -> Result<(), CvError> {
    match endpoint {
        "publishers" => store_resources(cache, ResourceKind::Publisher, results),
        "people" => store_resources(cache, ResourceKind::Person, results),
        "volumes" => store_volumes(cache, results),
        "issues" => store_issues(cache, results),
        other => Err(CvError::BadResponse(format!("unknown endpoint {other}"))),
    }
}

fn store_resources(
    cache: &dyn CvCache,
    kind: ResourceKind,
    results: &[Value],
) -> Result<(), CvError> {
    for item in results {
        let Some(id) = item.get("id").and_then(Value::as_i64) else {
            continue;
        };
        let row = ResourceRow {
            kind,
            id,
            name: str_field(item, "name"),
            image_url: parse_image_url(item),
            date_last_updated: str_field(item, "date_last_updated"),
            date_added: str_field(item, "date_added"),
            fetched_at: unix_now(),
            detail_json: None,
        };
        cache
            .put_resource_detail(&row)
            .map_err(|e| CvError::BadResponse(e.to_string()))?;
    }
    Ok(())
}

fn store_volumes(cache: &dyn CvCache, results: &[Value]) -> Result<(), CvError> {
    let mut rows = Vec::with_capacity(results.len());
    for item in results {
        let Some(volume_id) = item.get("id").and_then(Value::as_i64) else {
            continue;
        };
        let publisher = item.get("publisher").and_then(|p| str_field(p, "name"));
        rows.push(VolumeRow {
            volume_id,
            name: str_field(item, "name"),
            publisher,
            start_year: item
                .get("start_year")
                .and_then(int_from_value)
                .map(|v| v as i32),
            count_of_issues: item
                .get("count_of_issues")
                .and_then(int_from_value)
                .map(|v| v as i32),
            date_last_updated: str_field(item, "date_last_updated"),
            last_cover_date: None,
            fetched_at: unix_now(),
        });
    }
    cache
        .put_volumes(&rows)
        .map_err(|e| CvError::BadResponse(e.to_string()))
}

fn store_issues(cache: &dyn CvCache, results: &[Value]) -> Result<(), CvError> {
    let mut issues = Vec::with_capacity(results.len());
    let mut volumes = Vec::new();
    let mut publishers = Vec::new();
    for item in results {
        let Some(issue_id) = item.get("id").and_then(Value::as_i64) else {
            continue;
        };
        let volume = item.get("volume");
        let Some(volume_id) = volume.and_then(|v| v.get("id")).and_then(Value::as_i64) else {
            continue;
        };
        if let Some(name) = volume.and_then(|v| str_field(v, "name")) {
            volumes.push(VolumeRow {
                volume_id,
                name: Some(name),
                fetched_at: unix_now(),
                ..Default::default()
            });
        }
        if let Some(pub_obj) = volume.and_then(|v| v.get("publisher")) {
            if let (Some(id), Some(name)) = (
                pub_obj.get("id").and_then(Value::as_i64),
                str_field(pub_obj, "name"),
            ) {
                publishers.push(ResourceRef {
                    kind: ResourceKind::Publisher,
                    id: Some(id),
                    name: Some(name),
                });
            }
        }
        let number = item
            .get("issue_number")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        issues.push(IssueSkeleton {
            issue_id,
            volume_id,
            issue_number: number,
            name: str_field(item, "name"),
            cover_date: str_field(item, "cover_date"),
            deck: str_field(item, "deck"),
            description: str_field(item, "description"),
            store_date: str_field(item, "store_date"),
            image_url: parse_image_url(item),
            date_added: str_field(item, "date_added"),
            date_last_updated: str_field(item, "date_last_updated"),
            api_detail_url: str_field(item, "api_detail_url"),
            site_detail_url: str_field(item, "site_detail_url"),
            fetched_at: unix_now(),
        });
    }
    if !volumes.is_empty() {
        cache
            .put_volumes(&volumes)
            .map_err(|e| CvError::BadResponse(e.to_string()))?;
    }
    if !publishers.is_empty() {
        cache
            .put_resources(&publishers)
            .map_err(|e| CvError::BadResponse(e.to_string()))?;
    }
    cache
        .put_issues(&issues)
        .map_err(|e| CvError::BadResponse(e.to_string()))
}

fn str_field(item: &Value, key: &str) -> Option<String> {
    item.get(key).and_then(Value::as_str).map(str::to_string)
}

/// The API sometimes carries a numeric as a JSON number and sometimes
/// as a string; take either.
fn int_from_value(v: &Value) -> Option<i64> {
    if let Some(i) = v.as_i64() {
        return Some(i);
    }
    v.as_str().and_then(|s| s.trim().parse::<i64>().ok())
}
