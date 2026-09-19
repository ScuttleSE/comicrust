//! Forced Comic Vine cache updates for the cache manager (ADR-064).
//!
//! A summary update fetches every volume field and the complete issue-number
//! map. A complete update also fetches every issue detail. The complete mode
//! commits one detail at a time and keeps a durable queue, so a later run can
//! resume after cancellation or failure.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;

use super::{CvCache, IssueSkeleton, SqliteCache, VolumeRow};
use crate::cv::connection::{CvClient, CvError};

const PAGE_SIZE: i64 = 100;
const VOLUME_FIELDS: &str = "aliases,api_detail_url,character_credits,concept_credits,count_of_issues,date_added,date_last_updated,deck,description,first_issue,id,image,last_issue,location_credits,name,object_credits,person_credits,publisher,site_detail_url,start_year,team_credits";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateMode {
    Summary,
    Complete,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdatePhase {
    Volume,
    IssueList,
    IssueDetails,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateProgress {
    pub phase: UpdatePhase,
    pub done: usize,
    pub total: usize,
    pub issue_id: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UpdateReport {
    pub volume_id: i64,
    pub issues: usize,
    pub issue_details: usize,
    pub pending_issue_details: usize,
    pub requests: usize,
    pub resumed: bool,
    pub stopped: bool,
}

/// Forces one cache-manager update. This function always runs on a worker.
pub fn update_volume(
    client: &CvClient,
    cache: &SqliteCache,
    volume_id: i64,
    mode: UpdateMode,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(UpdateProgress),
) -> Result<UpdateReport, CvError> {
    if volume_id <= 0 {
        return Err(CvError::BadResponse(
            "the Comic Vine volume id must be positive".to_string(),
        ));
    }

    let pending = cache
        .pending_issue_details(volume_id)
        .map_err(cache_error)?;
    if mode == UpdateMode::Complete && !pending.is_empty() {
        let issue_count = cache.issue_count(volume_id).map_err(cache_error)? as usize;
        let mut report = UpdateReport {
            volume_id,
            issues: issue_count,
            pending_issue_details: pending.len(),
            resumed: true,
            ..Default::default()
        };
        fetch_details(
            client,
            cache,
            volume_id,
            pending,
            cancel,
            &mut report,
            &mut on_progress,
        )?;
        return Ok(report);
    }

    if cancel.load(Ordering::Relaxed) {
        return Ok(UpdateReport {
            volume_id,
            stopped: true,
            ..Default::default()
        });
    }
    on_progress(UpdateProgress {
        phase: UpdatePhase::Volume,
        done: 0,
        total: 1,
        issue_id: None,
    });
    let (mut volume, volume_json) = fetch_volume(client, volume_id)?;
    let mut report = UpdateReport {
        volume_id,
        requests: 1,
        ..Default::default()
    };

    if let Some(stored) = cache.volume(volume_id).map_err(cache_error)? {
        volume.last_cover_date = stored.last_cover_date;
    }
    let old: HashMap<i64, IssueSkeleton> = cache
        .issues_of_volume(volume_id)
        .map_err(cache_error)?
        .into_iter()
        .map(|issue| (issue.issue_id, issue))
        .collect();
    let (mut issues, pages) = fetch_issue_list(client, volume_id, cancel, &mut on_progress)?;
    report.requests += pages;
    if cancel.load(Ordering::Relaxed) {
        report.stopped = true;
        return Ok(report);
    }
    for issue in &mut issues {
        if let Some(previous) = old.get(&issue.issue_id) {
            issue.cover_date.clone_from(&previous.cover_date);
            issue.name.clone_from(&previous.name);
        }
    }
    report.issues = issues.len();
    cache
        .replace_volume_snapshot(&volume, &volume_json, &issues)
        .map_err(cache_error)?;

    match mode {
        UpdateMode::Summary => {
            cache
                .set_pending_issue_details(volume_id, &[])
                .map_err(cache_error)?;
        }
        UpdateMode::Complete => {
            let ids: Vec<i64> = issues.iter().map(|issue| issue.issue_id).collect();
            cache
                .set_pending_issue_details(volume_id, &ids)
                .map_err(cache_error)?;
            report.pending_issue_details = ids.len();
            fetch_details(
                client,
                cache,
                volume_id,
                ids,
                cancel,
                &mut report,
                &mut on_progress,
            )?;
        }
    }
    Ok(report)
}

fn fetch_volume(client: &CvClient, volume_id: i64) -> Result<(VolumeRow, String), CvError> {
    let mut query = client.base_query();
    query.push(("field_list", VOLUME_FIELDS.to_string()));
    let dom = client.get_dom(&format!("/volume/4050-{volume_id}/"), &query)?;
    let result = dom
        .get("results")
        .filter(|value| value.is_object())
        .ok_or_else(|| CvError::BadResponse(format!("no details for volume {volume_id}")))?;
    let returned_id = value_i64(result.get("id")).unwrap_or(volume_id);
    if returned_id != volume_id {
        return Err(CvError::BadResponse(format!(
            "volume {volume_id} returned id {returned_id}"
        )));
    }
    let publisher = result
        .pointer("/publisher/name")
        .and_then(Value::as_str)
        .map(str::to_string);
    let volume = VolumeRow {
        volume_id,
        name: result
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string),
        publisher,
        start_year: value_i32(result.get("start_year")),
        count_of_issues: value_i32(result.get("count_of_issues")),
        date_last_updated: result
            .get("date_last_updated")
            .and_then(Value::as_str)
            .map(str::to_string),
        last_cover_date: None,
        fetched_at: chrono::Utc::now().timestamp(),
    };
    let json = serde_json::to_string(result)
        .map_err(|error| CvError::BadResponse(format!("volume JSON: {error}")))?;
    Ok((volume, json))
}

fn fetch_issue_list(
    client: &CvClient,
    volume_id: i64,
    cancel: &AtomicBool,
    on_progress: &mut impl FnMut(UpdateProgress),
) -> Result<(Vec<IssueSkeleton>, usize), CvError> {
    let mut issues = Vec::new();
    let mut offset = 0i64;
    let mut pages = 0usize;
    loop {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        on_progress(UpdateProgress {
            phase: UpdatePhase::IssueList,
            done: offset as usize,
            total: 0,
            issue_id: None,
        });
        let mut query = client.base_query();
        query.push(("field_list", "id,issue_number,volume".to_string()));
        query.push(("filter", format!("volume:{volume_id}")));
        query.push(("sort", "id:asc".to_string()));
        query.push(("limit", PAGE_SIZE.to_string()));
        query.push(("offset", offset.to_string()));
        let dom = client.get_dom("/issues/", &query)?;
        pages += 1;
        let total = dom
            .get("number_of_total_results")
            .and_then(Value::as_i64)
            .unwrap_or(0)
            .max(0);
        let page: Vec<IssueSkeleton> = dom
            .get("results")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| {
                let issue_id = value_i64(item.get("id"))?;
                Some(IssueSkeleton {
                    issue_id,
                    volume_id,
                    issue_number: item
                        .get("issue_number")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .trim()
                        .to_string(),
                    ..Default::default()
                })
            })
            .collect();
        let page_len = page.len() as i64;
        issues.extend(page);
        offset += page_len;
        on_progress(UpdateProgress {
            phase: UpdatePhase::IssueList,
            done: offset as usize,
            total: total as usize,
            issue_id: None,
        });
        if page_len < PAGE_SIZE || offset >= total {
            break;
        }
    }
    Ok((issues, pages))
}

fn fetch_details(
    client: &CvClient,
    cache: &SqliteCache,
    volume_id: i64,
    issue_ids: Vec<i64>,
    cancel: &AtomicBool,
    report: &mut UpdateReport,
    on_progress: &mut impl FnMut(UpdateProgress),
) -> Result<(), CvError> {
    let total = issue_ids.len();
    for (index, issue_id) in issue_ids.into_iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            report.stopped = true;
            break;
        }
        on_progress(UpdateProgress {
            phase: UpdatePhase::IssueDetails,
            done: index,
            total,
            issue_id: Some(issue_id),
        });
        let query = client.base_query();
        let dom = client.get_dom(&format!("/issue/4000-{issue_id}/"), &query)?;
        report.requests += 1;
        let result = dom
            .get("results")
            .filter(|value| value.is_object())
            .ok_or_else(|| CvError::BadResponse(format!("no details for issue {issue_id}")))?;
        let returned_id = value_i64(result.get("id")).unwrap_or(issue_id);
        if returned_id != issue_id {
            return Err(CvError::BadResponse(format!(
                "issue {issue_id} returned id {returned_id}"
            )));
        }
        let issue = IssueSkeleton {
            issue_id,
            volume_id,
            issue_number: result
                .get("issue_number")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string(),
            cover_date: result
                .get("cover_date")
                .and_then(Value::as_str)
                .map(str::to_string),
            name: result
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string),
        };
        let json = serde_json::to_string(result)
            .map_err(|error| CvError::BadResponse(format!("issue JSON: {error}")))?;
        cache
            .complete_issue_detail(&issue, &json)
            .map_err(cache_error)?;
        report.issue_details += 1;
        report.pending_issue_details = total - index - 1;
        on_progress(UpdateProgress {
            phase: UpdatePhase::IssueDetails,
            done: index + 1,
            total,
            issue_id: Some(issue_id),
        });
    }
    if report.pending_issue_details == 0 {
        update_last_cover_date(cache, volume_id)?;
    }
    Ok(())
}

fn update_last_cover_date(cache: &SqliteCache, volume_id: i64) -> Result<(), CvError> {
    let last_cover_date = cache
        .issues_of_volume(volume_id)
        .map_err(cache_error)?
        .into_iter()
        .filter_map(|issue| issue.cover_date)
        .max();
    cache
        .set_volume_last_cover_date(volume_id, last_cover_date.as_deref())
        .map_err(cache_error)
}

fn value_i64(value: Option<&Value>) -> Option<i64> {
    match value {
        Some(Value::Number(number)) => number.as_i64(),
        Some(Value::String(text)) => text.trim().parse().ok(),
        _ => None,
    }
}

fn value_i32(value: Option<&Value>) -> Option<i32> {
    value_i64(value).and_then(|value| i32::try_from(value).ok())
}

fn cache_error(error: super::CacheError) -> CvError {
    CvError::BadResponse(error.to_string())
}
