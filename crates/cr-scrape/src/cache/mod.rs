//! The Comic Vine disk cache (ADR-037).
//!
//! The cache holds two layers, because they have different costs and
//! different lifetimes.
//!
//! * The SKELETON layer (`volume`, `issue_skeleton`) is complete and
//!   cheap. An MCL import (ADR-038) seeds it with no API request, and
//!   the incremental sweep keeps it current.
//! * The DETAIL layer (`issue_detail`, `image_blob`, `search_result`)
//!   is per-issue, expensive, and filled on demand.
//!
//! `request_log` counts every API request per resource, so the budget
//! survives a restart. `sweep_state` lets an interrupted sweep resume.

pub mod mcl;
mod sqlite;

pub use sqlite::SqliteCache;

use std::path::PathBuf;

/// The cache errors. A cache failure must never stop a scrape, so the
/// callers log and continue.
#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("cache database: {0}")]
    Db(String),
    #[error("cache path: {0}")]
    Path(String),
}

/// One volume in the skeleton layer. Every optional field is unknown
/// until a query fills it; an MCL import fills none of them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VolumeRow {
    pub volume_id: i64,
    pub name: Option<String>,
    pub publisher: Option<String>,
    pub start_year: Option<i32>,
    /// The API's `count_of_issues`. The closed-volume rule compares it
    /// against the stored issue count.
    pub count_of_issues: Option<i32>,
    /// The API's `date_last_updated`, stored in the API's own text
    /// form.
    pub date_last_updated: Option<String>,
    /// The latest `cover_date` over the stored issues of this volume.
    pub last_cover_date: Option<String>,
    /// Unix seconds. Zero means never fetched from the API.
    pub fetched_at: i64,
}

/// One issue in the skeleton layer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IssueSkeleton {
    pub issue_id: i64,
    pub volume_id: i64,
    /// The issue number as the API gives it. It is text, not a
    /// number: `1,5`, `1a`, and `v. 1, no. 01` all occur.
    pub issue_number: String,
    pub cover_date: Option<String>,
    pub name: Option<String>,
}

/// The resume point of the incremental sweep (ADR-038). One row only.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SweepState {
    /// The `date_last_updated` window, in the API's `YYYY-MM-DD` form.
    pub start_date: String,
    pub end_date: String,
    /// The next `offset` to request.
    pub offset: i64,
    /// The last reported `number_of_total_results`, or zero.
    pub total: i64,
    pub updated_at: i64,
}

/// The store behind the scraper. `SqliteCache` is the only
/// implementation; `SqliteCache::in_memory` backs the tests.
pub trait CvCache {
    // --- skeleton layer ---

    /// Inserts or updates volumes. Only the fields that are `Some`
    /// overwrite a stored value, so a cheap query never erases what an
    /// expensive one found.
    fn put_volumes(&self, volumes: &[VolumeRow]) -> Result<(), CacheError>;

    fn volume(&self, volume_id: i64) -> Result<Option<VolumeRow>, CacheError>;

    /// Inserts or updates issues. The same merge rule as
    /// `put_volumes` applies to the optional fields.
    fn put_issues(&self, issues: &[IssueSkeleton]) -> Result<(), CacheError>;

    /// Every stored issue of one volume, ordered by issue id (the MCL
    /// order, ADR-038).
    fn issues_of_volume(&self, volume_id: i64) -> Result<Vec<IssueSkeleton>, CacheError>;

    fn issue_count(&self, volume_id: i64) -> Result<i64, CacheError>;

    // --- detail layer ---

    /// Stores the raw API record for one issue. The caller keeps the
    /// API's own JSON text, so a later parser change needs no refetch.
    fn put_issue_detail(&self, issue_id: i64, json: &str) -> Result<(), CacheError>;

    /// The stored record and the time it was fetched, in unix seconds.
    fn issue_detail(&self, issue_id: i64) -> Result<Option<(String, i64)>, CacheError>;

    fn put_image(&self, url: &str, bytes: &[u8]) -> Result<(), CacheError>;

    fn image(&self, url: &str) -> Result<Option<Vec<u8>>, CacheError>;

    /// Stores the result of one series search under its search terms.
    fn put_search(&self, terms: &str, json: &str) -> Result<(), CacheError>;

    fn search(&self, terms: &str) -> Result<Option<(String, i64)>, CacheError>;

    // --- accounting and sweep ---

    /// Records one API request against one resource, at `at` unix
    /// seconds.
    fn log_request(&self, resource: &str, at: i64) -> Result<(), CacheError>;

    /// The number of requests against `resource` since `since` unix
    /// seconds.
    fn requests_since(&self, resource: &str, since: i64) -> Result<i64, CacheError>;

    /// The time of the oldest request against `resource` since
    /// `since`. The budget code needs it to say when the window frees
    /// a slot.
    fn oldest_request_since(&self, resource: &str, since: i64) -> Result<Option<i64>, CacheError>;

    /// Deletes request records older than `before` unix seconds.
    fn prune_requests(&self, before: i64) -> Result<(), CacheError>;

    fn sweep_state(&self) -> Result<Option<SweepState>, CacheError>;

    fn put_sweep_state(&self, state: &SweepState) -> Result<(), CacheError>;
}

/// The cache file:
/// `$XDG_DATA_HOME/comicrust/plugins/comic-vine-scraper/cvcache.sqlite`
/// (default `~/.local/share/...`). ADR-037 keeps it out of `~/.cache`,
/// because a mirror built under a rate limit must survive a cache
/// clean, and out of `~/.config`, which ADR-033 reserves for
/// hand-edited configuration.
pub fn default_cache_path() -> PathBuf {
    cache_path_from(
        std::env::var_os("XDG_DATA_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    )
}

fn cache_path_from(xdg_data_home: Option<PathBuf>, home: Option<PathBuf>) -> PathBuf {
    let root = match xdg_data_home {
        Some(v) if !v.as_os_str().is_empty() => v,
        _ => {
            let mut home = home.unwrap_or_else(|| PathBuf::from("."));
            home.push(".local");
            home.push("share");
            home
        }
    };
    root.join("comicrust")
        .join("plugins")
        .join("comic-vine-scraper")
        .join("cvcache.sqlite")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_path_prefers_xdg_data_home() {
        let p = cache_path_from(
            Some(PathBuf::from("/x/data")),
            Some(PathBuf::from("/home/u")),
        );
        assert_eq!(
            p,
            PathBuf::from("/x/data/comicrust/plugins/comic-vine-scraper/cvcache.sqlite")
        );
    }

    #[test]
    fn cache_path_falls_back_to_local_share() {
        let p = cache_path_from(None, Some(PathBuf::from("/home/u")));
        assert_eq!(
            p,
            PathBuf::from(
                "/home/u/.local/share/comicrust/plugins/comic-vine-scraper/cvcache.sqlite"
            )
        );
        let empty = cache_path_from(Some(PathBuf::new()), Some(PathBuf::from("/home/u")));
        assert_eq!(empty, p);
    }
}
