//! The cache import merge (ADR-069).
//!
//! The rule, per row: the newer stamp picks the base row — the API
//! `date_last_updated` of both rows parse to timestamps, and where
//! either side has none the comparison falls back to `fetched_at`.
//! For every field, an empty incoming value never erases a stored
//! value, an empty stored value takes the incoming value, and two
//! non-empty values take the base row's value. A tie keeps the stored
//! row. Image blobs compare on `fetched_at` alone; request rows
//! append as-is; the sweep state takes the newer `updated_at`.
//!
//! The Python scripts of ADR-072 implement the same rule, so an app
//! import and a script import cannot diverge.

use std::cmp::Ordering;

use rusqlite::{params, Connection, OptionalExtension};

use super::CacheError;

fn db(e: rusqlite::Error) -> CacheError {
    CacheError::Db(e.to_string())
}

/// The per-table outcome of one import.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TableReport {
    pub name: &'static str,
    pub added: usize,
    pub updated: usize,
    pub skipped: usize,
    pub rejected: usize,
}

/// The whole-import outcome.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub tables: Vec<TableReport>,
}

impl ImportReport {
    fn table(&mut self, name: &'static str) -> &mut TableReport {
        if self.tables.iter().all(|t| t.name != name) {
            self.tables.push(TableReport {
                name,
                ..Default::default()
            });
        }
        self.tables
            .iter_mut()
            .find(|t| t.name == name)
            .expect("the row exists")
    }
}

/// Parses the API `date_last_updated` text (`YYYY-MM-DD HH:MM:SS`, or
/// a bare date) to unix seconds.
fn parse_api_date(value: &str) -> Option<i64> {
    let value = value.trim();
    if let Ok(date_time) = chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S") {
        return Some(date_time.and_utc().timestamp());
    }
    chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .ok()
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|dt| dt.and_utc().timestamp())
}

/// True when the incoming row is newer than the stored row. Both API
/// stamps present → compare them; otherwise compare `fetched_at`. A
/// tie keeps the stored row.
fn incoming_is_newer(
    incoming_date: Option<&str>,
    incoming_at: i64,
    stored_date: Option<&str>,
    stored_at: i64,
) -> bool {
    let order = match (
        incoming_date.and_then(parse_api_date),
        stored_date.and_then(parse_api_date),
    ) {
        (Some(incoming), Some(stored)) => incoming.cmp(&stored),
        _ => incoming_at.cmp(&stored_at),
    };
    order == Ordering::Greater
}

/// True for the empty states of the merge: `NULL` and the empty
/// string both count, so an empty incoming value never erases.
fn empty_text(value: &Option<String>) -> bool {
    value.as_deref().map(str::is_empty).unwrap_or(true)
}

/// The field rule: an empty incoming value never erases, an empty
/// stored value takes the incoming value, two non-empty values take
/// the base row's value.
fn merge_text(
    stored: &Option<String>,
    incoming: &Option<String>,
    base_is_incoming: bool,
) -> Option<String> {
    if empty_text(incoming) {
        return stored.clone();
    }
    if empty_text(stored) {
        return incoming.clone();
    }
    if base_is_incoming {
        incoming.clone()
    } else {
        stored.clone()
    }
}

/// The integer field rule, with `None` as the empty state.
fn merge_int(stored: &Option<i64>, incoming: &Option<i64>, base_is_incoming: bool) -> Option<i64> {
    match (stored, incoming) {
        (_, None) => *stored,
        (None, _) => *incoming,
        (_, Some(incoming)) => {
            if base_is_incoming {
                Some(*incoming)
            } else {
                *stored
            }
        }
    }
}

fn merge_ints(stored: &Option<i32>, incoming: &Option<i32>, base_is_incoming: bool) -> Option<i32> {
    match (stored, incoming) {
        (_, None) => *stored,
        (None, _) => *incoming,
        (_, Some(incoming)) => {
            if base_is_incoming {
                Some(*incoming)
            } else {
                *stored
            }
        }
    }
}

/// `fetched_at` follows the base row: the base row is one whole row,
/// and its stamp is its own (a tie keeps the stored row exactly).
fn merge_stamp(stored: i64, incoming: i64, base_is_incoming: bool) -> i64 {
    if base_is_incoming {
        incoming
    } else {
        stored
    }
}

/// Merges the source database into the live one. The caller holds the
/// live transaction.
pub(crate) fn merge(live: &Connection, source: &Connection) -> Result<ImportReport, CacheError> {
    let mut report = ImportReport::default();
    merge_volumes(live, source, &mut report)?;
    merge_skeletons(live, source, &mut report)?;
    merge_issue_details(live, source, &mut report)?;
    merge_blobs(live, source, &mut report)?;
    merge_searches(live, source, &mut report)?;
    merge_requests(live, source, &mut report)?;
    merge_sweep_state(live, source, &mut report)?;
    merge_pending(live, source, &mut report)?;
    for table in [
        "character",
        "person",
        "team",
        "story_arc",
        "location",
        "concept",
        "object",
        "publisher",
    ] {
        merge_resource(live, source, table, &mut report)?;
    }
    merge_credits(live, source, &mut report)?;
    Ok(report)
}

// --- volume ---

struct VolumeRow {
    volume_id: Option<i64>,
    name: Option<String>,
    publisher: Option<String>,
    start_year: Option<i32>,
    count_of_issues: Option<i32>,
    date_last_updated: Option<String>,
    last_cover_date: Option<String>,
    fetched_at: i64,
    detail_json: Option<String>,
    aliases: Option<String>,
    deck: Option<String>,
    description: Option<String>,
    image_url: Option<String>,
    api_detail_url: Option<String>,
    site_detail_url: Option<String>,
    date_added: Option<String>,
    first_issue_id: Option<i64>,
    last_issue_id: Option<i64>,
}

const VOLUME_COLUMNS: &str = "volume_id, name, publisher, start_year, count_of_issues, \
     date_last_updated, last_cover_date, fetched_at, detail_json, aliases, deck, \
     description, image_url, api_detail_url, site_detail_url, date_added, \
     first_issue_id, last_issue_id";

fn read_volume_row(conn: &Connection, volume_id: i64) -> Result<Option<VolumeRow>, CacheError> {
    let sql = format!("SELECT {VOLUME_COLUMNS} FROM volume WHERE volume_id = ?1");
    conn.query_row(&sql, params![volume_id], |r| {
        Ok(VolumeRow {
            volume_id: r.get(0)?,
            name: r.get(1)?,
            publisher: r.get(2)?,
            start_year: r.get(3)?,
            count_of_issues: r.get(4)?,
            date_last_updated: r.get(5)?,
            last_cover_date: r.get(6)?,
            fetched_at: r.get(7)?,
            detail_json: r.get(8)?,
            aliases: r.get(9)?,
            deck: r.get(10)?,
            description: r.get(11)?,
            image_url: r.get(12)?,
            api_detail_url: r.get(13)?,
            site_detail_url: r.get(14)?,
            date_added: r.get(15)?,
            first_issue_id: r.get(16)?,
            last_issue_id: r.get(17)?,
        })
    })
    .optional()
    .map_err(db)
}

fn read_volume_rows(conn: &Connection) -> Result<Vec<VolumeRow>, CacheError> {
    let sql = format!("SELECT {VOLUME_COLUMNS} FROM volume");
    let mut stmt = conn.prepare(&sql).map_err(db)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(VolumeRow {
                volume_id: r.get(0)?,
                name: r.get(1)?,
                publisher: r.get(2)?,
                start_year: r.get(3)?,
                count_of_issues: r.get(4)?,
                date_last_updated: r.get(5)?,
                last_cover_date: r.get(6)?,
                fetched_at: r.get(7)?,
                detail_json: r.get(8)?,
                aliases: r.get(9)?,
                deck: r.get(10)?,
                description: r.get(11)?,
                image_url: r.get(12)?,
                api_detail_url: r.get(13)?,
                site_detail_url: r.get(14)?,
                date_added: r.get(15)?,
                first_issue_id: r.get(16)?,
                last_issue_id: r.get(17)?,
            })
        })
        .map_err(db)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db)
}

fn write_volume(conn: &Connection, row: &VolumeRow) -> Result<(), CacheError> {
    conn.execute(
        &format!(
            "INSERT INTO volume ({VOLUME_COLUMNS})
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)
             ON CONFLICT(volume_id) DO UPDATE SET
               name = ?2, publisher = ?3, start_year = ?4, count_of_issues = ?5,
               date_last_updated = ?6, last_cover_date = ?7, fetched_at = ?8,
               detail_json = ?9, aliases = ?10, deck = ?11, description = ?12,
               image_url = ?13, api_detail_url = ?14, site_detail_url = ?15,
               date_added = ?16, first_issue_id = ?17, last_issue_id = ?18"
        ),
        params![
            row.volume_id,
            row.name,
            row.publisher,
            row.start_year,
            row.count_of_issues,
            row.date_last_updated,
            row.last_cover_date,
            row.fetched_at,
            row.detail_json,
            row.aliases,
            row.deck,
            row.description,
            row.image_url,
            row.api_detail_url,
            row.site_detail_url,
            row.date_added,
            row.first_issue_id,
            row.last_issue_id,
        ],
    )
    .map(|_| ())
    .map_err(db)
}

fn merge_volume_rows(
    stored: &VolumeRow,
    incoming: &VolumeRow,
    base_is_incoming: bool,
) -> VolumeRow {
    VolumeRow {
        volume_id: stored.volume_id,
        name: merge_text(&stored.name, &incoming.name, base_is_incoming),
        publisher: merge_text(&stored.publisher, &incoming.publisher, base_is_incoming),
        start_year: merge_ints(&stored.start_year, &incoming.start_year, base_is_incoming),
        count_of_issues: merge_ints(
            &stored.count_of_issues,
            &incoming.count_of_issues,
            base_is_incoming,
        ),
        date_last_updated: merge_text(
            &stored.date_last_updated,
            &incoming.date_last_updated,
            base_is_incoming,
        ),
        last_cover_date: merge_text(
            &stored.last_cover_date,
            &incoming.last_cover_date,
            base_is_incoming,
        ),
        fetched_at: merge_stamp(stored.fetched_at, incoming.fetched_at, base_is_incoming),
        detail_json: merge_text(&stored.detail_json, &incoming.detail_json, base_is_incoming),
        aliases: merge_text(&stored.aliases, &incoming.aliases, base_is_incoming),
        deck: merge_text(&stored.deck, &incoming.deck, base_is_incoming),
        description: merge_text(&stored.description, &incoming.description, base_is_incoming),
        image_url: merge_text(&stored.image_url, &incoming.image_url, base_is_incoming),
        api_detail_url: merge_text(
            &stored.api_detail_url,
            &incoming.api_detail_url,
            base_is_incoming,
        ),
        site_detail_url: merge_text(
            &stored.site_detail_url,
            &incoming.site_detail_url,
            base_is_incoming,
        ),
        date_added: merge_text(&stored.date_added, &incoming.date_added, base_is_incoming),
        first_issue_id: merge_int(
            &stored.first_issue_id,
            &incoming.first_issue_id,
            base_is_incoming,
        ),
        last_issue_id: merge_int(
            &stored.last_issue_id,
            &incoming.last_issue_id,
            base_is_incoming,
        ),
    }
}

fn volume_same(a: &VolumeRow, b: &VolumeRow) -> bool {
    a.name == b.name
        && a.publisher == b.publisher
        && a.start_year == b.start_year
        && a.count_of_issues == b.count_of_issues
        && a.date_last_updated == b.date_last_updated
        && a.last_cover_date == b.last_cover_date
        && a.fetched_at == b.fetched_at
        && a.detail_json == b.detail_json
        && a.aliases == b.aliases
        && a.deck == b.deck
        && a.description == b.description
        && a.image_url == b.image_url
        && a.api_detail_url == b.api_detail_url
        && a.site_detail_url == b.site_detail_url
        && a.date_added == b.date_added
        && a.first_issue_id == b.first_issue_id
        && a.last_issue_id == b.last_issue_id
}

fn merge_volumes(
    live: &Connection,
    source: &Connection,
    report: &mut ImportReport,
) -> Result<(), CacheError> {
    let table = report.table("volume");
    for incoming in read_volume_rows(source)? {
        let Some(volume_id) = incoming.volume_id else {
            table.rejected += 1;
            continue;
        };
        let Some(stored) = read_volume_row(live, volume_id)? else {
            write_volume(live, &incoming)?;
            table.added += 1;
            continue;
        };
        let base_is_incoming = incoming_is_newer(
            incoming.date_last_updated.as_deref(),
            incoming.fetched_at,
            stored.date_last_updated.as_deref(),
            stored.fetched_at,
        );
        let merged = merge_volume_rows(&stored, &incoming, base_is_incoming);
        if volume_same(&merged, &stored) {
            table.skipped += 1;
        } else {
            write_volume(live, &merged)?;
            table.updated += 1;
        }
    }
    Ok(())
}

// --- issue skeleton ---

fn merge_skeletons(
    live: &Connection,
    source: &Connection,
    report: &mut ImportReport,
) -> Result<(), CacheError> {
    let table = report.table("issue_skeleton");
    let mut stmt = source
        .prepare("SELECT issue_id, volume_id, issue_number, cover_date, name FROM issue_skeleton")
        .map_err(db)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, Option<i64>>(0)?,
                r.get::<_, Option<i64>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })
        .map_err(db)?;
    for row in rows {
        let (issue_id, volume_id, issue_number, cover_date, name) = row.map_err(db)?;
        let (Some(issue_id), Some(volume_id), Some(issue_number)) =
            (issue_id, volume_id, issue_number)
        else {
            table.rejected += 1;
            continue;
        };
        let stored: Option<(Option<String>, Option<String>)> = live
            .query_row(
                "SELECT cover_date, name FROM issue_skeleton WHERE issue_id = ?1",
                params![issue_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db)?;
        let Some((stored_cover, stored_name)) = stored else {
            live.execute(
                "INSERT INTO issue_skeleton (issue_id, volume_id, issue_number, cover_date, name)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![issue_id, volume_id, issue_number, cover_date, name],
            )
            .map_err(db)?;
            table.added += 1;
            continue;
        };
        // No stamps exist on the skeleton, so the stored row is the
        // base: only empty stored fields take the incoming value.
        let merged_cover = merge_text(&stored_cover, &cover_date, false);
        let merged_name = merge_text(&stored_name, &name, false);
        if merged_cover == stored_cover && merged_name == stored_name {
            table.skipped += 1;
        } else {
            live.execute(
                "UPDATE issue_skeleton SET cover_date = ?2, name = ?3 WHERE issue_id = ?1",
                params![issue_id, merged_cover, merged_name],
            )
            .map_err(db)?;
            table.updated += 1;
        }
    }
    Ok(())
}

// --- issue detail ---

struct IssueDetailRow {
    issue_id: Option<i64>,
    json: Option<String>,
    fetched_at: i64,
    volume_id: Option<i64>,
    issue_number: Option<String>,
    cover_date: Option<String>,
    name: Option<String>,
    store_date: Option<String>,
    image_url: Option<String>,
    date_added: Option<String>,
    date_last_updated: Option<String>,
}

const ISSUE_DETAIL_COLUMNS: &str = "issue_id, json, fetched_at, volume_id, issue_number, \
     cover_date, name, store_date, image_url, date_added, date_last_updated";

fn read_issue_details(conn: &Connection) -> Result<Vec<IssueDetailRow>, CacheError> {
    let sql = format!("SELECT {ISSUE_DETAIL_COLUMNS} FROM issue_detail");
    let mut stmt = conn.prepare(&sql).map_err(db)?;
    let rows = stmt
        .query_map([], |r| {
            Ok(IssueDetailRow {
                issue_id: r.get(0)?,
                json: r.get(1)?,
                fetched_at: r.get(2)?,
                volume_id: r.get(3)?,
                issue_number: r.get(4)?,
                cover_date: r.get(5)?,
                name: r.get(6)?,
                store_date: r.get(7)?,
                image_url: r.get(8)?,
                date_added: r.get(9)?,
                date_last_updated: r.get(10)?,
            })
        })
        .map_err(db)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db)
}

fn write_issue_detail(conn: &Connection, row: &IssueDetailRow) -> Result<(), CacheError> {
    conn.execute(
        &format!(
            "INSERT INTO issue_detail ({ISSUE_DETAIL_COLUMNS})
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(issue_id) DO UPDATE SET
               json = ?2, fetched_at = ?3, volume_id = ?4, issue_number = ?5,
               cover_date = ?6, name = ?7, store_date = ?8, image_url = ?9,
               date_added = ?10, date_last_updated = ?11"
        ),
        params![
            row.issue_id,
            row.json,
            row.fetched_at,
            row.volume_id,
            row.issue_number,
            row.cover_date,
            row.name,
            row.store_date,
            row.image_url,
            row.date_added,
            row.date_last_updated,
        ],
    )
    .map(|_| ())
    .map_err(db)
}

fn merge_issue_details(
    live: &Connection,
    source: &Connection,
    report: &mut ImportReport,
) -> Result<(), CacheError> {
    let table = report.table("issue_detail");
    for incoming in read_issue_details(source)? {
        let Some(issue_id) = incoming.issue_id else {
            table.rejected += 1;
            continue;
        };
        let stored: Option<IssueDetailRow> = {
            let sql =
                format!("SELECT {ISSUE_DETAIL_COLUMNS} FROM issue_detail WHERE issue_id = ?1");
            live.query_row(&sql, params![issue_id], |r| {
                Ok(IssueDetailRow {
                    issue_id: r.get(0)?,
                    json: r.get(1)?,
                    fetched_at: r.get(2)?,
                    volume_id: r.get(3)?,
                    issue_number: r.get(4)?,
                    cover_date: r.get(5)?,
                    name: r.get(6)?,
                    store_date: r.get(7)?,
                    image_url: r.get(8)?,
                    date_added: r.get(9)?,
                    date_last_updated: r.get(10)?,
                })
            })
            .optional()
            .map_err(db)?
        };
        let Some(stored) = stored else {
            write_issue_detail(live, &incoming)?;
            table.added += 1;
            continue;
        };
        let base_is_incoming = incoming_is_newer(
            incoming.date_last_updated.as_deref(),
            incoming.fetched_at,
            stored.date_last_updated.as_deref(),
            stored.fetched_at,
        );
        let merged = IssueDetailRow {
            issue_id: stored.issue_id,
            json: merge_text(&stored.json, &incoming.json, base_is_incoming),
            fetched_at: merge_stamp(stored.fetched_at, incoming.fetched_at, base_is_incoming),
            volume_id: merge_int(&stored.volume_id, &incoming.volume_id, base_is_incoming),
            issue_number: merge_text(
                &stored.issue_number,
                &incoming.issue_number,
                base_is_incoming,
            ),
            cover_date: merge_text(&stored.cover_date, &incoming.cover_date, base_is_incoming),
            name: merge_text(&stored.name, &incoming.name, base_is_incoming),
            store_date: merge_text(&stored.store_date, &incoming.store_date, base_is_incoming),
            image_url: merge_text(&stored.image_url, &incoming.image_url, base_is_incoming),
            date_added: merge_text(&stored.date_added, &incoming.date_added, base_is_incoming),
            date_last_updated: merge_text(
                &stored.date_last_updated,
                &incoming.date_last_updated,
                base_is_incoming,
            ),
        };
        let same = merged.json == stored.json
            && merged.fetched_at == stored.fetched_at
            && merged.volume_id == stored.volume_id
            && merged.issue_number == stored.issue_number
            && merged.cover_date == stored.cover_date
            && merged.name == stored.name
            && merged.store_date == stored.store_date
            && merged.image_url == stored.image_url
            && merged.date_added == stored.date_added
            && merged.date_last_updated == stored.date_last_updated;
        if same {
            table.skipped += 1;
        } else {
            write_issue_detail(live, &merged)?;
            table.updated += 1;
        }
    }
    Ok(())
}

// --- image blobs, search results, request log, sweep state, pending ---

fn merge_blobs(
    live: &Connection,
    source: &Connection,
    report: &mut ImportReport,
) -> Result<(), CacheError> {
    let table = report.table("image_blob");
    let mut stmt = source
        .prepare("SELECT url, bytes, fetched_at FROM image_blob")
        .map_err(db)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, Option<Vec<u8>>>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })
        .map_err(db)?;
    for row in rows {
        let (url, bytes, fetched_at) = row.map_err(db)?;
        let (Some(url), Some(bytes)) = (url, bytes) else {
            table.rejected += 1;
            continue;
        };
        let stored: Option<i64> = live
            .query_row(
                "SELECT fetched_at FROM image_blob WHERE url = ?1",
                params![url],
                |r| r.get(0),
            )
            .optional()
            .map_err(db)?;
        let Some(stored_at) = stored else {
            live.execute(
                "INSERT INTO image_blob (url, bytes, fetched_at) VALUES (?1, ?2, ?3)",
                params![url, bytes, fetched_at],
            )
            .map_err(db)?;
            table.added += 1;
            continue;
        };
        // Blobs carry no API stamp: fetched_at alone decides.
        if fetched_at > stored_at {
            live.execute(
                "UPDATE image_blob SET bytes = ?2, fetched_at = ?3 WHERE url = ?1",
                params![url, bytes, fetched_at],
            )
            .map_err(db)?;
            table.updated += 1;
        } else {
            table.skipped += 1;
        }
    }
    Ok(())
}

fn merge_searches(
    live: &Connection,
    source: &Connection,
    report: &mut ImportReport,
) -> Result<(), CacheError> {
    let table = report.table("search_result");
    let mut stmt = source
        .prepare("SELECT terms, json, fetched_at FROM search_result")
        .map_err(db)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })
        .map_err(db)?;
    for row in rows {
        let (terms, json, fetched_at) = row.map_err(db)?;
        let (Some(terms), Some(json)) = (terms, json) else {
            table.rejected += 1;
            continue;
        };
        let stored: Option<i64> = live
            .query_row(
                "SELECT fetched_at FROM search_result WHERE terms = ?1",
                params![terms],
                |r| r.get(0),
            )
            .optional()
            .map_err(db)?;
        let Some(stored_at) = stored else {
            live.execute(
                "INSERT INTO search_result (terms, json, fetched_at) VALUES (?1, ?2, ?3)",
                params![terms, json, fetched_at],
            )
            .map_err(db)?;
            table.added += 1;
            continue;
        };
        // No API stamp exists: fetched_at alone decides, as with blobs.
        if fetched_at > stored_at {
            live.execute(
                "UPDATE search_result SET json = ?2, fetched_at = ?3 WHERE terms = ?1",
                params![terms, json, fetched_at],
            )
            .map_err(db)?;
            table.updated += 1;
        } else {
            table.skipped += 1;
        }
    }
    Ok(())
}

fn merge_requests(
    live: &Connection,
    source: &Connection,
    report: &mut ImportReport,
) -> Result<(), CacheError> {
    let table = report.table("request_log");
    let mut stmt = source
        .prepare("SELECT resource, at FROM request_log")
        .map_err(db)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<i64>>(1)?))
        })
        .map_err(db)?;
    for row in rows {
        let (resource, at) = row.map_err(db)?;
        let (Some(resource), Some(at)) = (resource, at) else {
            table.rejected += 1;
            continue;
        };
        // Appended as-is: only rows inside the current one-hour
        // window can affect the budget, so older imported rows
        // change nothing.
        live.execute(
            "INSERT INTO request_log (resource, at) VALUES (?1, ?2)",
            params![resource, at],
        )
        .map_err(db)?;
        table.added += 1;
    }
    Ok(())
}

fn merge_sweep_state(
    live: &Connection,
    source: &Connection,
    report: &mut ImportReport,
) -> Result<(), CacheError> {
    let table = report.table("sweep_state");
    let incoming: Option<(String, String, i64, i64, i64)> = source
        .query_row(
            "SELECT start_date, end_date, offset, total, updated_at FROM sweep_state WHERE id = 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
        )
        .optional()
        .map_err(db)?;
    let Some((start_date, end_date, offset, total, updated_at)) = incoming else {
        return Ok(());
    };
    let stored: Option<i64> = live
        .query_row("SELECT updated_at FROM sweep_state WHERE id = 1", [], |r| {
            r.get(0)
        })
        .optional()
        .map_err(db)?;
    let Some(stored_at) = stored else {
        live.execute(
            "INSERT INTO sweep_state (id, start_date, end_date, offset, total, updated_at)
             VALUES (1, ?1, ?2, ?3, ?4, ?5)",
            params![start_date, end_date, offset, total, updated_at],
        )
        .map_err(db)?;
        table.added += 1;
        return Ok(());
    };
    // The newer `updated_at` wins.
    if updated_at > stored_at {
        live.execute(
            "UPDATE sweep_state SET start_date = ?1, end_date = ?2, offset = ?3, total = ?4,
                updated_at = ?5 WHERE id = 1",
            params![start_date, end_date, offset, total, updated_at],
        )
        .map_err(db)?;
        table.updated += 1;
    } else {
        table.skipped += 1;
    }
    Ok(())
}

fn merge_pending(
    live: &Connection,
    source: &Connection,
    report: &mut ImportReport,
) -> Result<(), CacheError> {
    let table = report.table("pending_issue_detail");
    let mut stmt = source
        .prepare("SELECT volume_id, issue_id, position FROM pending_issue_detail")
        .map_err(db)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, Option<i64>>(0)?,
                r.get::<_, Option<i64>>(1)?,
                r.get::<_, Option<i64>>(2)?,
            ))
        })
        .map_err(db)?;
    for row in rows {
        let (volume_id, issue_id, position) = row.map_err(db)?;
        let (Some(volume_id), Some(issue_id)) = (volume_id, issue_id) else {
            table.rejected += 1;
            continue;
        };
        // The queues union: both sides keep their resume points.
        let inserted = live
            .execute(
                "INSERT OR IGNORE INTO pending_issue_detail (volume_id, issue_id, position)
                 VALUES (?1, ?2, ?3)",
                params![volume_id, issue_id, position.unwrap_or(0)],
            )
            .map_err(db)?;
        if inserted > 0 {
            table.added += 1;
        } else {
            table.skipped += 1;
        }
    }
    Ok(())
}

// --- resource tables and credits ---

const RESOURCE_COLUMNS: &str =
    "id, name, image_url, date_last_updated, date_added, fetched_at, detail_json";

/// The stored side of one resource row, as the merge reads it.
type StoredResource = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    i64,
    Option<String>,
);

fn merge_resource(
    live: &Connection,
    source: &Connection,
    table_name: &'static str,
    report: &mut ImportReport,
) -> Result<(), CacheError> {
    let table = report.table(table_name);
    let sql = format!("SELECT {RESOURCE_COLUMNS} FROM {table_name}");
    let mut stmt = source.prepare(&sql).map_err(db)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, Option<i64>>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, Option<String>>(6)?,
            ))
        })
        .map_err(db)?;
    for row in rows {
        let (id, name, image_url, date_last_updated, date_added, fetched_at, detail_json) =
            row.map_err(db)?;
        let Some(id) = id else {
            table.rejected += 1;
            continue;
        };
        let stored: Option<StoredResource> = {
            let sql = format!(
                "SELECT name, image_url, date_added, date_last_updated, fetched_at, detail_json
                   FROM {table_name} WHERE id = ?1"
            );
            live.query_row(&sql, params![id], |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            })
            .optional()
            .map_err(db)?
        };
        let Some((s_name, s_image, s_added, s_updated, s_at, s_detail)) = stored else {
            live.execute(
                &format!(
                    "INSERT INTO {table_name} ({RESOURCE_COLUMNS})
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"
                ),
                params![
                    id,
                    name,
                    image_url,
                    date_last_updated,
                    date_added,
                    fetched_at,
                    detail_json
                ],
            )
            .map_err(db)?;
            table.added += 1;
            continue;
        };
        let base_is_incoming = incoming_is_newer(
            date_last_updated.as_deref(),
            fetched_at,
            s_updated.as_deref(),
            s_at,
        );
        let merged_name = merge_text(&s_name, &name, base_is_incoming);
        let merged_image = merge_text(&s_image, &image_url, base_is_incoming);
        let merged_updated = merge_text(&s_updated, &date_last_updated, base_is_incoming);
        let merged_added = merge_text(&s_added, &date_added, base_is_incoming);
        let merged_detail = merge_text(&s_detail, &detail_json, base_is_incoming);
        let merged_at = merge_stamp(s_at, fetched_at, base_is_incoming);
        let same = s_name == merged_name
            && s_image == merged_image
            && s_updated == merged_updated
            && s_added == merged_added
            && s_detail == merged_detail
            && s_at == merged_at;
        if same {
            table.skipped += 1;
        } else {
            live.execute(
                &format!(
                    "INSERT INTO {table_name} ({RESOURCE_COLUMNS})
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT(id) DO UPDATE SET
                       name = ?2, image_url = ?3, date_last_updated = ?4,
                       date_added = ?5, fetched_at = ?6, detail_json = ?7"
                ),
                params![
                    id,
                    merged_name,
                    merged_image,
                    merged_updated,
                    merged_added,
                    merged_at,
                    merged_detail,
                ],
            )
            .map_err(db)?;
            table.updated += 1;
        }
    }
    Ok(())
}

fn merge_credits(
    live: &Connection,
    source: &Connection,
    report: &mut ImportReport,
) -> Result<(), CacheError> {
    let table = report.table("credit");
    let mut stmt = source
        .prepare(
            "SELECT owner_kind, owner_id, resource_kind, resource_id, name, role, marker
               FROM credit",
        )
        .map_err(db)?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, Option<i64>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<i64>>(3)?,
                r.get::<_, Option<String>>(4)?,
                r.get::<_, Option<String>>(5)?,
                r.get::<_, Option<String>>(6)?,
            ))
        })
        .map_err(db)?;
    for row in rows {
        let (owner_kind, owner_id, resource_kind, resource_id, name, role, marker) =
            row.map_err(db)?;
        let (
            Some(owner_kind),
            Some(owner_id),
            Some(resource_kind),
            Some(resource_id),
            Some(marker),
        ) = (owner_kind, owner_id, resource_kind, resource_id, marker)
        else {
            table.rejected += 1;
            continue;
        };
        // The natural key makes a re-import idempotent, so the
        // queues of credits union.
        let inserted = live
            .execute(
                "INSERT OR IGNORE INTO credit
                   (owner_kind, owner_id, resource_kind, resource_id, name, role, marker)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    owner_kind,
                    owner_id,
                    resource_kind,
                    resource_id,
                    name,
                    role,
                    marker
                ],
            )
            .map_err(db)?;
        if inserted > 0 {
            table.added += 1;
        } else {
            table.skipped += 1;
        }
    }
    Ok(())
}
