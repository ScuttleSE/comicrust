//! The incremental Comic Vine sweep (ADR-038).
//!
//! One paged query over `/issues` with
//! `filter=date_last_updated:<start>|<end>` returns every issue that
//! changed in a date window, across the whole of Comic Vine. That is
//! far cheaper than one revalidation per volume, and it is what keeps
//! the cache skeleton current after an MCL import.
//!
//! The `Update Missing` add-on uses this query in production
//! (`update_missing.py`). The API reference page renders its per-field
//! filter marks as images, so the page text does not state that
//! `date_last_updated` is filterable; that script is the evidence.
//!
//! The sweep saves its offset after every page, so an interrupted
//! sweep resumes instead of paying for the same pages again.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};

use serde_json::Value;

use super::{CvCache, IssueSkeleton, SweepState, VolumeRow};
use crate::cv::connection::{CvClient, CvError};

/// The API caps a page at 100 results for `/issues`.
pub const PAGE_SIZE: i64 = 100;

/// What the sweep asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SweepOptions {
    /// The window start, in the API `YYYY-MM-DD` form.
    pub start_date: String,
    /// The window end, in the API `YYYY-MM-DD` form.
    pub end_date: String,
    /// A page cap for one run. `None` runs to the end of the window.
    /// The budget code uses it to spend only what it has.
    pub max_pages: Option<usize>,
}

/// One page of progress.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SweepProgress {
    /// The offset the sweep has now read to.
    pub offset: i64,
    /// The API's `number_of_total_results`, or zero.
    pub total: i64,
}

/// What one run did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SweepReport {
    pub pages: usize,
    pub issues: usize,
    pub volumes: usize,
    /// True when the sweep read the whole window.
    pub complete: bool,
    /// True when the run stopped on the cancel flag or the page cap.
    pub stopped_early: bool,
    /// The state to resume from.
    pub state: SweepState,
}

/// Runs the sweep. It resumes from the stored state when that state
/// covers the same window; a different window starts at offset zero.
///
/// `on_progress` runs after every page. The caller must not block in
/// it, because the sweep runs on a worker thread (Rule 9).
pub fn run(
    client: &CvClient,
    cache: &dyn CvCache,
    options: &SweepOptions,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(SweepProgress),
) -> Result<SweepReport, CvError> {
    let stored = cache.sweep_state().ok().flatten();
    let resume =
        stored.filter(|s| s.start_date == options.start_date && s.end_date == options.end_date);
    let mut state = resume.unwrap_or(SweepState {
        start_date: options.start_date.clone(),
        end_date: options.end_date.clone(),
        offset: 0,
        total: 0,
        updated_at: 0,
    });

    let mut report = SweepReport::default();
    // A resumed sweep that already read its window is complete.
    if state.total > 0 && state.offset >= state.total {
        report.complete = true;
        report.state = state;
        return Ok(report);
    }

    loop {
        if cancel.load(Ordering::Relaxed) {
            report.stopped_early = true;
            break;
        }
        if let Some(cap) = options.max_pages {
            if report.pages >= cap {
                report.stopped_early = true;
                break;
            }
        }

        let dom = issues_dom(client, options, state.offset)?;
        report.pages += 1;
        state.total = dom
            .get("number_of_total_results")
            .and_then(Value::as_i64)
            .unwrap_or(state.total);

        let rows = collect(&dom);
        let page_len = rows.len() as i64;
        if !rows.is_empty() {
            let (volumes, issues) = split(rows);
            report.volumes += volumes.len();
            report.issues += issues.len();
            // A cache write failure must not lose the offset, so the
            // sweep reports it and stops.
            cache
                .put_volumes(&volumes)
                .map_err(|e| CvError::BadResponse(e.to_string()))?;
            cache
                .put_issues(&issues)
                .map_err(|e| CvError::BadResponse(e.to_string()))?;
        }

        state.offset += page_len.max(0);
        state.updated_at = chrono::Utc::now().timestamp();
        // The offset is saved AFTER the data, so a crash re-reads one
        // page instead of losing one.
        let _ = cache.put_sweep_state(&state);
        on_progress(SweepProgress {
            offset: state.offset,
            total: state.total,
        });

        // An empty page ends the window, whatever the reported total
        // says.
        if page_len < PAGE_SIZE || state.offset >= state.total {
            report.complete = true;
            break;
        }
    }

    report.state = state;
    Ok(report)
}

/// The query one page uses. The field list is the smallest that names
/// an issue and its volume, which keeps the response small.
fn issues_dom(client: &CvClient, options: &SweepOptions, offset: i64) -> Result<Value, CvError> {
    let mut query = client.base_query();
    query.push(("field_list", "id,issue_number,volume".to_string()));
    query.push((
        "filter",
        format!(
            "date_last_updated:{}|{}",
            options.start_date, options.end_date
        ),
    ));
    query.push(("sort", "id:asc".to_string()));
    query.push(("limit", PAGE_SIZE.to_string()));
    query.push(("offset", offset.to_string()));
    client.get_dom("/issues/", &query)
}

/// The `(issue id, volume id, issue number)` rows of one page.
fn collect(dom: &Value) -> Vec<(i64, i64, String)> {
    let Some(results) = dom.get("results").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out = Vec::with_capacity(results.len());
    for item in results {
        let Some(issue_id) = item.get("id").and_then(Value::as_i64) else {
            continue;
        };
        let Some(volume_id) = item
            .get("volume")
            .and_then(|v| v.get("id"))
            .and_then(Value::as_i64)
        else {
            // An issue with no volume cannot join the skeleton.
            continue;
        };
        let number = item
            .get("issue_number")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        out.push((issue_id, volume_id, number));
    }
    out
}

/// Splits the page rows into the two cache writes. The volume rows
/// carry only the id, so the merge rule erases nothing.
fn split(rows: Vec<(i64, i64, String)>) -> (Vec<VolumeRow>, Vec<IssueSkeleton>) {
    let mut volumes: BTreeMap<i64, VolumeRow> = BTreeMap::new();
    let mut issues = Vec::with_capacity(rows.len());
    for (issue_id, volume_id, issue_number) in rows {
        volumes.entry(volume_id).or_insert(VolumeRow {
            volume_id,
            ..Default::default()
        });
        issues.push(IssueSkeleton {
            issue_id,
            volume_id,
            issue_number,
            ..Default::default()
        });
    }
    (volumes.into_values().collect(), issues)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dom(json: &str) -> Value {
        serde_json::from_str(json).expect("json")
    }

    #[test]
    fn a_row_with_no_volume_is_dropped() {
        let d = dom(r#"{"results":[
                {"id":1,"issue_number":"1","volume":{"id":10}},
                {"id":2,"issue_number":"2"},
                {"issue_number":"3","volume":{"id":10}}
            ]}"#);
        assert_eq!(collect(&d), vec![(1, 10, "1".to_string())]);
    }

    #[test]
    fn a_missing_issue_number_reads_as_empty() {
        let d = dom(r#"{"results":[{"id":1,"volume":{"id":10}}]}"#);
        assert_eq!(collect(&d), vec![(1, 10, String::new())]);
    }

    #[test]
    fn a_page_with_no_results_collects_nothing() {
        assert!(collect(&dom(r#"{"status_code":1}"#)).is_empty());
        assert!(collect(&dom(r#"{"results":[]}"#)).is_empty());
    }

    #[test]
    fn the_split_makes_one_volume_row_per_volume() {
        let (volumes, issues) = split(vec![
            (1, 10, "1".into()),
            (2, 10, "2".into()),
            (3, 20, "1".into()),
        ]);
        assert_eq!(
            volumes.iter().map(|v| v.volume_id).collect::<Vec<_>>(),
            vec![10, 20]
        );
        // The volume rows carry the id only, so nothing is erased.
        assert!(volumes.iter().all(|v| v.name.is_none()));
        assert_eq!(issues.len(), 3);
    }
}
