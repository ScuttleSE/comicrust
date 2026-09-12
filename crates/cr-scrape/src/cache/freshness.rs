//! The freshness rule (ADR-037).
//!
//! The rule reads EVIDENCE, not a clock. A volume that ended cannot
//! gain an issue, so its cached issue list stays good for as long as
//! the user keeps it. A volume that still publishes needs a check, but
//! that check is ONE request, not a re-page of its whole issue list.
//!
//! The probe uses `/volume/4050-<id>/`, the resource the scraper
//! already queries for series details. The `/volumes` list resource
//! would need a filter on `id`, and the API reference page renders its
//! per-field filter marks as images, so that filter is not confirmed.

use std::collections::BTreeMap;

use chrono::NaiveDate;
use serde_json::Value;

use super::{CvCache, IssueSkeleton, VolumeRow};
use crate::cv::connection::{CvClient, CvError};

/// The API caps a page at 100 results for `/issues`.
const PAGE_SIZE: i64 = 100;

/// When to trust the cache, and when to ask.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FreshnessPolicy {
    /// A volume counts as closed when its last issue's cover date is
    /// older than this many days AND its stored `count_of_issues`
    /// equals the number of issues in the cache.
    pub closed_horizon_days: i64,
    /// An open volume is revalidated at most once in this many
    /// seconds.
    pub revalidate_after_seconds: i64,
}

impl Default for FreshnessPolicy {
    fn default() -> Self {
        FreshnessPolicy {
            // About one year. A series with no new issue for a year
            // and a complete issue count has ended, or it is dormant.
            closed_horizon_days: 365,
            // One day.
            revalidate_after_seconds: 86_400,
        }
    }
}

/// What the cache must do for one volume.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// Serve from the cache. No request.
    Fresh,
    /// One cheap probe decides whether the issue list changed.
    Revalidate,
    /// Page the whole issue list.
    Fetch,
}

/// Decides what to do for one volume. `now` is unix seconds.
pub fn verdict(
    volume: Option<&VolumeRow>,
    cached_issues: i64,
    now: i64,
    policy: &FreshnessPolicy,
) -> Verdict {
    let Some(v) = volume else {
        return Verdict::Fetch;
    };
    // `fetched_at` is zero after an MCL import, which fills no API
    // field. The skeleton is still worth having, but the detail must
    // come from the API before anything is served as complete.
    if v.fetched_at == 0 || cached_issues == 0 {
        return Verdict::Fetch;
    }
    if is_closed(v, cached_issues, now, policy) {
        return Verdict::Fresh;
    }
    if now - v.fetched_at < policy.revalidate_after_seconds {
        return Verdict::Fresh;
    }
    Verdict::Revalidate
}

/// True when the volume cannot have gained an issue.
pub fn is_closed(
    volume: &VolumeRow,
    cached_issues: i64,
    now: i64,
    policy: &FreshnessPolicy,
) -> bool {
    // The cache must hold every issue the API counts.
    if volume.count_of_issues.map(i64::from) != Some(cached_issues) {
        return false;
    }
    let Some(last) = volume.last_cover_date.as_deref().and_then(parse_date) else {
        return false;
    };
    let Some(now_date) = chrono::DateTime::from_timestamp(now, 0).map(|d| d.date_naive()) else {
        return false;
    };
    (now_date - last).num_days() > policy.closed_horizon_days
}

/// Parses a Comic Vine date. The API gives `YYYY-MM-DD`, sometimes
/// with a time after it.
fn parse_date(value: &str) -> Option<NaiveDate> {
    let head = value.trim().split([' ', 'T']).next()?;
    NaiveDate::parse_from_str(head, "%Y-%m-%d").ok()
}

/// What one `issues_of_volume` call did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FetchReport {
    /// The number of API requests this call paid for.
    pub requests: usize,
    pub verdict_was: Option<Verdict>,
    /// True when the issue list was re-paged.
    pub repaged: bool,
}

/// The issues of one volume, from the cache when the freshness rule
/// allows it.
///
/// * `Fresh` pays for zero requests.
/// * `Revalidate` pays for one, and pays for the pages only when the
///   probe shows a change.
/// * `Fetch` pages the issue list.
pub fn issues_of_volume(
    client: &CvClient,
    cache: &dyn CvCache,
    volume_id: i64,
    policy: &FreshnessPolicy,
    now: i64,
) -> Result<(Vec<IssueSkeleton>, FetchReport), CvError> {
    let stored = cache.volume(volume_id).ok().flatten();
    let cached_issues = cache.issue_count(volume_id).unwrap_or(0);
    let call = verdict(stored.as_ref(), cached_issues, now, policy);
    let mut report = FetchReport {
        verdict_was: Some(call),
        ..Default::default()
    };

    match call {
        Verdict::Fresh => {
            let issues = cache
                .issues_of_volume(volume_id)
                .map_err(|e| CvError::BadResponse(e.to_string()))?;
            Ok((issues, report))
        }
        Verdict::Revalidate => {
            let probe = probe_volume(client, volume_id)?;
            report.requests += 1;
            let unchanged = stored.as_ref().is_some_and(|s| {
                s.count_of_issues == probe.count_of_issues
                    && s.date_last_updated == probe.date_last_updated
            });
            // The probe result is stored either way, so the next call
            // knows when the last check ran.
            let mut row = probe.clone();
            row.fetched_at = now;
            cache
                .put_volumes(&[row])
                .map_err(|e| CvError::BadResponse(e.to_string()))?;
            if unchanged {
                let issues = cache
                    .issues_of_volume(volume_id)
                    .map_err(|e| CvError::BadResponse(e.to_string()))?;
                return Ok((issues, report));
            }
            report.repaged = true;
            let issues = repage(client, cache, volume_id, &probe, now, &mut report)?;
            Ok((issues, report))
        }
        Verdict::Fetch => {
            let probe = probe_volume(client, volume_id)?;
            report.requests += 1;
            report.repaged = true;
            let issues = repage(client, cache, volume_id, &probe, now, &mut report)?;
            Ok((issues, report))
        }
    }
}

/// The one-request probe: the volume's own resource, with the smallest
/// field list that answers "did the issue list change?".
pub fn probe_volume(client: &CvClient, volume_id: i64) -> Result<VolumeRow, CvError> {
    let mut query = client.base_query();
    query.push((
        "field_list",
        "id,name,count_of_issues,date_last_updated,start_year,publisher".to_string(),
    ));
    let dom = client.get_dom(&format!("/volume/4050-{volume_id}/"), &query)?;
    let results = dom
        .get("results")
        .ok_or_else(|| CvError::BadResponse(format!("no details for volume {volume_id}")))?;
    Ok(VolumeRow {
        volume_id,
        name: results
            .get("name")
            .and_then(Value::as_str)
            .map(str::to_string),
        publisher: results
            .pointer("/publisher/name")
            .and_then(Value::as_str)
            .map(str::to_string),
        start_year: results.get("start_year").and_then(|v| match v {
            Value::String(s) => s.parse().ok(),
            other => other.as_i64().map(|n| n as i32),
        }),
        count_of_issues: results
            .get("count_of_issues")
            .and_then(Value::as_i64)
            .map(|n| n as i32),
        date_last_updated: results
            .get("date_last_updated")
            .and_then(Value::as_str)
            .map(str::to_string),
        last_cover_date: None,
        fetched_at: 0,
    })
}

/// Pages the issue list of one volume into the cache, then stores the
/// volume row with the latest cover date it found.
fn repage(
    client: &CvClient,
    cache: &dyn CvCache,
    volume_id: i64,
    probe: &VolumeRow,
    now: i64,
    report: &mut FetchReport,
) -> Result<Vec<IssueSkeleton>, CvError> {
    let mut issues: BTreeMap<i64, IssueSkeleton> = BTreeMap::new();
    let mut offset = 0i64;
    loop {
        let dom = issues_dom(client, volume_id, offset)?;
        report.requests += 1;
        let total = dom
            .get("number_of_total_results")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let page = collect(&dom, volume_id);
        let page_len = page.len() as i64;
        for issue in page {
            issues.insert(issue.issue_id, issue);
        }
        offset += page_len.max(0);
        if page_len < PAGE_SIZE || offset >= total {
            break;
        }
    }

    let issues: Vec<IssueSkeleton> = issues.into_values().collect();
    let last_cover_date = issues
        .iter()
        .filter_map(|i| i.cover_date.as_deref())
        .filter_map(parse_date)
        .max()
        .map(|d| d.format("%Y-%m-%d").to_string());
    let row = VolumeRow {
        last_cover_date,
        fetched_at: now,
        ..probe.clone()
    };
    cache
        .put_volumes(&[row])
        .map_err(|e| CvError::BadResponse(e.to_string()))?;
    cache
        .put_issues(&issues)
        .map_err(|e| CvError::BadResponse(e.to_string()))?;
    Ok(issues)
}

fn issues_dom(client: &CvClient, volume_id: i64, offset: i64) -> Result<Value, CvError> {
    let mut query = client.base_query();
    query.push(("field_list", "id,issue_number,name,cover_date".to_string()));
    query.push(("filter", format!("volume:{volume_id}")));
    query.push(("limit", PAGE_SIZE.to_string()));
    query.push(("offset", offset.to_string()));
    client.get_dom("/issues/", &query)
}

fn collect(dom: &Value, volume_id: i64) -> Vec<IssueSkeleton> {
    let Some(results) = dom.get("results").and_then(Value::as_array) else {
        return Vec::new();
    };
    results
        .iter()
        .filter_map(|item| {
            let issue_id = item.get("id").and_then(Value::as_i64)?;
            Some(IssueSkeleton {
                issue_id,
                volume_id,
                issue_number: item
                    .get("issue_number")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim()
                    .to_string(),
                cover_date: item
                    .get("cover_date")
                    .and_then(Value::as_str)
                    .map(str::to_string),
                name: item.get("name").and_then(Value::as_str).map(str::to_string),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2020-01-01T00:00:00Z.
    const NOW: i64 = 1_577_836_800;

    fn closed_volume() -> VolumeRow {
        VolumeRow {
            volume_id: 771,
            count_of_issues: Some(6),
            last_cover_date: Some("2013-06-01".into()),
            fetched_at: NOW - 10,
            ..Default::default()
        }
    }

    #[test]
    fn no_row_means_fetch() {
        assert_eq!(
            verdict(None, 0, NOW, &FreshnessPolicy::default()),
            Verdict::Fetch
        );
    }

    #[test]
    fn an_mcl_only_volume_means_fetch() {
        // An import fills the skeleton and no API field, so
        // `fetched_at` stays zero.
        let row = VolumeRow {
            volume_id: 771,
            ..Default::default()
        };
        assert_eq!(
            verdict(Some(&row), 6, NOW, &FreshnessPolicy::default()),
            Verdict::Fetch
        );
    }

    #[test]
    fn an_empty_issue_list_means_fetch() {
        assert_eq!(
            verdict(Some(&closed_volume()), 0, NOW, &FreshnessPolicy::default()),
            Verdict::Fetch
        );
    }

    #[test]
    fn a_closed_volume_is_fresh() {
        let policy = FreshnessPolicy::default();
        assert!(is_closed(&closed_volume(), 6, NOW, &policy));
        assert_eq!(
            verdict(Some(&closed_volume()), 6, NOW, &policy),
            Verdict::Fresh
        );
    }

    #[test]
    fn an_incomplete_issue_list_is_not_closed() {
        // The API counts 6 and the cache holds 5. Something is
        // missing, so the volume is not closed.
        let policy = FreshnessPolicy::default();
        assert!(!is_closed(&closed_volume(), 5, NOW, &policy));
    }

    #[test]
    fn an_unknown_issue_count_is_not_closed() {
        let mut v = closed_volume();
        v.count_of_issues = None;
        assert!(!is_closed(&v, 6, NOW, &FreshnessPolicy::default()));
    }

    #[test]
    fn a_recent_last_issue_is_not_closed() {
        let mut v = closed_volume();
        v.last_cover_date = Some("2019-12-01".into());
        assert!(!is_closed(&v, 6, NOW, &FreshnessPolicy::default()));
    }

    #[test]
    fn an_unknown_last_cover_date_is_not_closed() {
        let mut v = closed_volume();
        v.last_cover_date = None;
        assert!(!is_closed(&v, 6, NOW, &FreshnessPolicy::default()));
        v.last_cover_date = Some("not a date".into());
        assert!(!is_closed(&v, 6, NOW, &FreshnessPolicy::default()));
    }

    #[test]
    fn an_open_volume_is_fresh_until_the_revalidate_window_passes() {
        let policy = FreshnessPolicy::default();
        let mut v = closed_volume();
        v.last_cover_date = Some("2019-12-01".into());
        // Checked ten seconds ago.
        assert_eq!(verdict(Some(&v), 6, NOW, &policy), Verdict::Fresh);
        // Checked two days ago.
        v.fetched_at = NOW - 2 * 86_400;
        assert_eq!(verdict(Some(&v), 6, NOW, &policy), Verdict::Revalidate);
    }

    #[test]
    fn the_horizon_is_configurable() {
        let mut v = closed_volume();
        v.last_cover_date = Some("2019-01-01".into());
        let strict = FreshnessPolicy {
            closed_horizon_days: 3650,
            ..FreshnessPolicy::default()
        };
        assert!(!is_closed(&v, 6, NOW, &strict));
        let loose = FreshnessPolicy {
            closed_horizon_days: 30,
            ..FreshnessPolicy::default()
        };
        assert!(is_closed(&v, 6, NOW, &loose));
    }

    #[test]
    fn a_comic_vine_date_parses_with_or_without_a_time() {
        assert_eq!(
            parse_date("2013-06-01"),
            NaiveDate::from_ymd_opt(2013, 6, 1)
        );
        assert_eq!(
            parse_date("2013-06-01 09:14:22"),
            NaiveDate::from_ymd_opt(2013, 6, 1)
        );
        assert_eq!(parse_date(""), None);
        assert_eq!(parse_date("June 2013"), None);
    }
}
