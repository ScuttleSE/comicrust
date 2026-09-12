//! The cache warm task (ADR-037, Phase 15 T6).
//!
//! The library already names Comic Vine volumes through the books that
//! were scraped. The warm task spends idle request budget on those
//! volumes, so the next scrape reads from the cache instead of the
//! API.
//!
//! The task is off by default. It is capped, it is cancellable, and it
//! stops the moment the budget refuses a request. A volume the
//! freshness rule already calls fresh costs nothing.

use std::sync::atomic::{AtomicBool, Ordering};

use super::freshness::{self, FreshnessPolicy, Verdict};
use super::CvCache;
use crate::cv::connection::{CvClient, CvError};

/// What the warm task may spend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WarmOptions {
    /// The request cap for one run. `None` runs until the budget or
    /// the cancel flag stops it.
    pub max_requests: Option<usize>,
    /// The cap on the number of volumes one run looks at.
    pub max_volumes: Option<usize>,
    pub policy: FreshnessPolicy,
}

impl Default for WarmOptions {
    fn default() -> Self {
        WarmOptions {
            // A conservative slice of a 200-per-hour budget.
            max_requests: Some(50),
            max_volumes: None,
            policy: FreshnessPolicy::default(),
        }
    }
}

/// What one run did.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WarmReport {
    /// The volumes the run looked at.
    pub considered: usize,
    /// The volumes the run read from the API.
    pub warmed: usize,
    /// The volumes the freshness rule already called fresh.
    pub already_fresh: usize,
    /// The volumes whose read failed. The run continues past a
    /// failure, because one bad volume must not stop the rest.
    pub failed: usize,
    pub requests: usize,
    /// True when the cancel flag, a cap, or the budget stopped the
    /// run.
    pub stopped_early: bool,
    /// The volume the run stopped at, when it stopped early.
    pub stopped_at: Option<i64>,
}

/// One step of progress.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WarmProgress {
    pub volume_id: i64,
    pub done: usize,
    pub total: usize,
    pub requests: usize,
}

/// Warms the given volumes, in the given order.
///
/// The caller gives the volume ids the library names. Duplicates are
/// the caller's business; the freshness rule makes a repeat cheap.
pub fn run(
    client: &CvClient,
    cache: &dyn CvCache,
    volume_ids: &[i64],
    options: &WarmOptions,
    cancel: &AtomicBool,
    mut on_progress: impl FnMut(WarmProgress),
) -> WarmReport {
    let mut report = WarmReport::default();
    let total = volume_ids.len();

    for (index, &volume_id) in volume_ids.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            report.stopped_early = true;
            report.stopped_at = Some(volume_id);
            break;
        }
        if let Some(cap) = options.max_volumes {
            if report.considered >= cap {
                report.stopped_early = true;
                report.stopped_at = Some(volume_id);
                break;
            }
        }
        if let Some(cap) = options.max_requests {
            if report.requests >= cap {
                report.stopped_early = true;
                report.stopped_at = Some(volume_id);
                break;
            }
        }

        report.considered += 1;
        let now = chrono::Utc::now().timestamp();
        match freshness::issues_of_volume(client, cache, volume_id, &options.policy, now) {
            Ok((_, fetch)) => {
                report.requests += fetch.requests;
                if fetch.verdict_was == Some(Verdict::Fresh) {
                    report.already_fresh += 1;
                } else {
                    report.warmed += 1;
                }
            }
            // A spent budget stops the whole run: every further
            // volume would be refused the same way.
            Err(CvError::BudgetSpent(_)) => {
                report.stopped_early = true;
                report.stopped_at = Some(volume_id);
                break;
            }
            Err(_) => report.failed += 1,
        }
        on_progress(WarmProgress {
            volume_id,
            done: index + 1,
            total,
            requests: report.requests,
        });
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_is_a_conservative_slice() {
        let o = WarmOptions::default();
        assert_eq!(o.max_requests, Some(50));
        assert_eq!(o.max_volumes, None);
    }

    #[test]
    fn no_volume_means_no_work() {
        let cache = crate::cache::SqliteCache::in_memory().expect("cache");
        let client = CvClient::new("");
        let cancel = AtomicBool::new(false);
        let report = run(
            &client,
            &cache,
            &[],
            &WarmOptions::default(),
            &cancel,
            |_| {},
        );
        assert_eq!(report, WarmReport::default());
    }

    #[test]
    fn a_cancelled_run_stops_before_the_first_volume() {
        let cache = crate::cache::SqliteCache::in_memory().expect("cache");
        let client = CvClient::new("");
        let cancel = AtomicBool::new(true);
        let report = run(
            &client,
            &cache,
            &[771, 999],
            &WarmOptions::default(),
            &cancel,
            |_| {},
        );
        assert!(report.stopped_early);
        assert_eq!(report.stopped_at, Some(771));
        assert_eq!(report.considered, 0);
    }
}
