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

use super::resources::{ResourceKind, ResourceRef};
use super::{CvCache, IssueSkeleton, SweepState, VolumeRow};
use crate::cv::connection::{CvClient, CvError};
use crate::cv::queries::parse_image_url;

/// The current time in unix seconds.
fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

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
    // Offline mode refuses before any work (ADR-071).
    if client.is_offline() {
        return Err(CvError::Offline);
    }
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

        let (rows, inline_volumes) = collect(&dom);
        let page_len = rows.len() as i64;
        if !rows.is_empty() {
            let (volumes, issues, publishers) = split(rows, inline_volumes);
            report.volumes += volumes.len();
            report.issues += issues.len();
            // A cache write failure must not lose the offset, so the
            // sweep reports it and stops.
            cache
                .put_volumes(&volumes)
                .map_err(|e| CvError::BadResponse(e.to_string()))?;
            if !publishers.is_empty() {
                cache
                    .put_resources(&publishers)
                    .map_err(|e| CvError::BadResponse(e.to_string()))?;
            }
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

/// The query one page uses. The field list is the ADR-072 expansion:
/// every list-level field the `/issues` resource documents, beside
/// the id and volume the skeleton needs. The page count does not
/// change; only the response size grows.
fn issues_dom(client: &CvClient, options: &SweepOptions, offset: i64) -> Result<Value, CvError> {
    let mut query = client.base_query();
    query.push((
        "field_list",
        "id,issue_number,volume,name,cover_date,deck,description,store_date,image,date_added,date_last_updated,site_detail_url,api_detail_url"
            .to_string(),
    ));
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

/// One inline volume object of a page. The exact sub-fields are
/// UNKNOWN (ADR-072): the reader takes what is there, and the merge
/// rule protects stored values.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct InlineVolume {
    volume_id: i64,
    name: Option<String>,
    publisher: Option<(i64, String)>,
}

/// The rows of one page: the skeleton data plus the inline volume
/// objects.
fn collect(dom: &Value) -> (Vec<IssueSkeleton>, Vec<InlineVolume>) {
    let Some(results) = dom.get("results").and_then(Value::as_array) else {
        return (Vec::new(), Vec::new());
    };
    let mut issues = Vec::with_capacity(results.len());
    let mut volumes: BTreeMap<i64, InlineVolume> = BTreeMap::new();
    for item in results {
        let Some(issue_id) = item.get("id").and_then(Value::as_i64) else {
            continue;
        };
        let volume = item.get("volume");
        let Some(volume_id) = volume.and_then(|v| v.get("id")).and_then(Value::as_i64) else {
            // An issue with no volume cannot join the skeleton.
            continue;
        };
        let inline = volumes.entry(volume_id).or_insert_with(|| InlineVolume {
            volume_id,
            ..Default::default()
        });
        if inline.name.is_none() {
            inline.name = volume
                .and_then(|v| v.get("name"))
                .and_then(Value::as_str)
                .map(str::to_string);
        }
        if inline.publisher.is_none() {
            inline.publisher = volume.and_then(|v| v.get("publisher")).and_then(|p| {
                let id = p.get("id").and_then(Value::as_i64)?;
                let name = p.get("name").and_then(Value::as_str)?;
                Some((id, name.to_string()))
            });
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
            name: item.get("name").and_then(Value::as_str).map(str::to_string),
            cover_date: item
                .get("cover_date")
                .and_then(Value::as_str)
                .map(str::to_string),
            deck: item.get("deck").and_then(Value::as_str).map(str::to_string),
            description: item
                .get("description")
                .and_then(Value::as_str)
                .map(str::to_string),
            store_date: item
                .get("store_date")
                .and_then(Value::as_str)
                .map(str::to_string),
            image_url: parse_image_url(item),
            date_added: item
                .get("date_added")
                .and_then(Value::as_str)
                .map(str::to_string),
            date_last_updated: item
                .get("date_last_updated")
                .and_then(Value::as_str)
                .map(str::to_string),
            api_detail_url: item
                .get("api_detail_url")
                .and_then(Value::as_str)
                .map(str::to_string),
            site_detail_url: item
                .get("site_detail_url")
                .and_then(Value::as_str)
                .map(str::to_string),
            fetched_at: unix_now(),
        });
    }
    (issues, volumes.into_values().collect())
}

/// Splits the page rows into the three cache writes: the volume rows
/// (id and name records only, so the merge rule erases nothing), the
/// publisher resource rows, and the skeletons.
fn split(
    issues: Vec<IssueSkeleton>,
    inline_volumes: Vec<InlineVolume>,
) -> (Vec<VolumeRow>, Vec<IssueSkeleton>, Vec<ResourceRef>) {
    let mut volumes: BTreeMap<i64, VolumeRow> = BTreeMap::new();
    let mut publishers: BTreeMap<i64, ResourceRef> = BTreeMap::new();
    for volume in inline_volumes {
        volumes.entry(volume.volume_id).or_insert(VolumeRow {
            volume_id: volume.volume_id,
            name: volume.name,
            ..Default::default()
        });
        if let Some((publisher_id, publisher_name)) = volume.publisher {
            publishers.entry(publisher_id).or_insert(ResourceRef {
                kind: ResourceKind::Publisher,
                id: Some(publisher_id),
                name: Some(publisher_name),
            });
        }
    }
    (
        volumes.into_values().collect(),
        issues,
        publishers.into_values().collect(),
    )
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
        let (issues, volumes) = collect(&d);
        assert_eq!(
            issues.iter().map(|i| i.issue_id).collect::<Vec<_>>(),
            vec![1]
        );
        assert_eq!(volumes.len(), 1);
    }

    #[test]
    fn a_missing_issue_number_reads_as_empty() {
        let d = dom(r#"{"results":[{"id":1,"volume":{"id":10}}]}"#);
        let (issues, _) = collect(&d);
        assert_eq!(issues[0].issue_number, String::new());
    }

    #[test]
    fn a_page_with_no_results_collects_nothing() {
        assert!(collect(&dom(r#"{"status_code":1}"#)).0.is_empty());
        assert!(collect(&dom(r#"{"results":[]}"#)).0.is_empty());
    }

    #[test]
    fn the_split_makes_one_row_per_volume_and_publisher() {
        let (issues, inline) = collect(&dom(r#"{"results":[
                {"id":1,"issue_number":"1","volume":{"id":10,"name":"Ten",
                  "publisher":{"id":7,"name":"Press"}}},
                {"id":2,"issue_number":"2","volume":{"id":10,
                  "publisher":{"id":7,"name":"Press"}}},
                {"id":3,"issue_number":"1","volume":{"id":20}}
            ]}"#));
        let (volumes, issues, publishers) = split(issues, inline);
        assert_eq!(
            volumes.iter().map(|v| v.volume_id).collect::<Vec<_>>(),
            vec![10, 20]
        );
        // The volume name record rides the inline object when the
        // response carries one; the merge rule protects stored values.
        assert_eq!(volumes[0].name.as_deref(), Some("Ten"));
        assert_eq!(volumes[1].name, None);
        assert_eq!(issues.len(), 3);
        // One publisher row per publisher.
        assert_eq!(publishers.len(), 1);
        assert_eq!(publishers[0].kind, ResourceKind::Publisher);
        assert_eq!(publishers[0].id, Some(7));
        assert_eq!(publishers[0].name.as_deref(), Some("Press"));
    }

    #[test]
    fn the_list_fields_fill_the_skeleton() {
        let (issues, _) = collect(&dom(r#"{"results":[
                {"id":1,"issue_number":"1","volume":{"id":10},
                 "name":"The Title","cover_date":"2001-01-01",
                 "deck":"A deck","description":"Text","store_date":"2000-12-10",
                 "image":{"small_url":"http://img/s.jpg"},"date_added":"2026-01-01 00:00:00",
                 "date_last_updated":"2026-09-20 00:00:00",
                 "site_detail_url":"http://site/1","api_detail_url":"http://api/1"}
            ]}"#));
        let issue = &issues[0];
        assert_eq!(issue.name.as_deref(), Some("The Title"));
        assert_eq!(issue.cover_date.as_deref(), Some("2001-01-01"));
        assert_eq!(issue.deck.as_deref(), Some("A deck"));
        assert_eq!(issue.description.as_deref(), Some("Text"));
        assert_eq!(issue.store_date.as_deref(), Some("2000-12-10"));
        assert_eq!(issue.image_url.as_deref(), Some("http://img/s.jpg"));
        assert_eq!(issue.date_added.as_deref(), Some("2026-01-01 00:00:00"));
        assert_eq!(
            issue.date_last_updated.as_deref(),
            Some("2026-09-20 00:00:00")
        );
        assert_eq!(issue.site_detail_url.as_deref(), Some("http://site/1"));
        assert_eq!(issue.api_detail_url.as_deref(), Some("http://api/1"));
        assert!(issue.fetched_at > 0);
    }
}
