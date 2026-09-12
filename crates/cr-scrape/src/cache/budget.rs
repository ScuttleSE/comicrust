//! The per-resource request budget (ADR-037).
//!
//! Comic Vine limits requests PER RESOURCE, not in total. The user
//! states the limit as 200 requests per resource per hour. The API
//! reference page carries no rate-limit text at all, so that figure is
//! not verified here and the ceiling is a configuration value.
//!
//! Every request passes this one chokepoint, and the count lives in
//! the cache database, so the budget survives a restart. Near the
//! ceiling the client waits and says when it will resume. It does not
//! stall without a word, and it does not fail in a burst.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::CvCache;

/// The default ceiling, from the user's figure. It is not confirmed by
/// the API reference page.
pub const DEFAULT_PER_RESOURCE_PER_HOUR: i64 = 200;

/// One hour, in seconds.
pub const DEFAULT_WINDOW_SECONDS: i64 = 3600;

/// The ceiling and the window it applies over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BudgetPolicy {
    pub per_resource: i64,
    pub window_seconds: i64,
}

impl Default for BudgetPolicy {
    fn default() -> Self {
        BudgetPolicy {
            per_resource: DEFAULT_PER_RESOURCE_PER_HOUR,
            window_seconds: DEFAULT_WINDOW_SECONDS,
        }
    }
}

/// What the budget says about one resource, right now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BudgetState {
    /// The request can go. `remaining` is the count left in the
    /// window.
    Ready { remaining: i64 },
    /// The window is full. The oldest request leaves the window at
    /// `resume_at` unix seconds.
    Full { resume_at: i64 },
}

/// What the user is told while a request waits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WaitNotice {
    pub resource: String,
    /// Unix seconds.
    pub resume_at: i64,
    pub seconds_left: i64,
}

/// A callback that reports a wait. It runs on the worker thread, so it
/// must not block.
pub type WaitFn = dyn Fn(&WaitNotice) + Send + Sync;

/// The chokepoint. Every API request calls `acquire` first.
pub struct Budget {
    cache: Arc<dyn CvCache>,
    policy: BudgetPolicy,
    cancel: Arc<AtomicBool>,
    on_wait: Option<Box<WaitFn>>,
    clock: Box<dyn Fn() -> i64 + Send + Sync>,
    /// How long one wait slice sleeps. The tests set it to zero.
    slice: Duration,
}

impl Budget {
    pub fn new(cache: Arc<dyn CvCache>, policy: BudgetPolicy) -> Self {
        Budget {
            cache,
            policy,
            cancel: Arc::new(AtomicBool::new(false)),
            on_wait: None,
            clock: Box::new(|| chrono::Utc::now().timestamp()),
            slice: Duration::from_secs(1),
        }
    }

    /// Shares the cancel flag of the run, so a waiting request stops
    /// when the user cancels.
    pub fn with_cancel(mut self, cancel: Arc<AtomicBool>) -> Self {
        self.cancel = cancel;
        self
    }

    /// Sets the wait report.
    pub fn with_wait_report(mut self, on_wait: Box<WaitFn>) -> Self {
        self.on_wait = Some(on_wait);
        self
    }

    /// Replaces the clock and the sleep slice. The tests use it.
    pub fn with_clock(
        mut self,
        clock: Box<dyn Fn() -> i64 + Send + Sync>,
        slice: Duration,
    ) -> Self {
        self.clock = clock;
        self.slice = slice;
        self
    }

    pub fn policy(&self) -> BudgetPolicy {
        self.policy
    }

    fn now(&self) -> i64 {
        (self.clock)()
    }

    /// The state of one resource, with no side effect.
    pub fn state(&self, resource: &str) -> BudgetState {
        self.state_at(resource, self.now())
    }

    fn state_at(&self, resource: &str, now: i64) -> BudgetState {
        let since = now - self.policy.window_seconds;
        let used = self.cache.requests_since(resource, since).unwrap_or(0);
        let remaining = self.policy.per_resource - used;
        if remaining > 0 {
            return BudgetState::Ready { remaining };
        }
        // The window frees a slot when its oldest request falls out.
        let oldest = self
            .cache
            .oldest_request_since(resource, since)
            .unwrap_or(None)
            .unwrap_or(now);
        BudgetState::Full {
            resume_at: oldest + self.policy.window_seconds + 1,
        }
    }

    /// The requests left for one resource in the current window.
    pub fn remaining(&self, resource: &str) -> i64 {
        match self.state(resource) {
            BudgetState::Ready { remaining } => remaining,
            BudgetState::Full { .. } => 0,
        }
    }

    /// Waits until the resource has room, then records one request.
    /// Returns `false` when the cancel flag stopped the wait; the
    /// caller must then make no request.
    pub fn acquire(&self, resource: &str) -> bool {
        loop {
            if self.cancel.load(Ordering::Relaxed) {
                return false;
            }
            let now = self.now();
            match self.state_at(resource, now) {
                BudgetState::Ready { .. } => {
                    let _ = self.cache.log_request(resource, now);
                    // The log is pruned lazily, so it cannot grow
                    // without an end.
                    let _ = self
                        .cache
                        .prune_requests(now - self.policy.window_seconds * 2);
                    return true;
                }
                BudgetState::Full { resume_at } => {
                    if let Some(report) = &self.on_wait {
                        report(&WaitNotice {
                            resource: resource.to_string(),
                            resume_at,
                            seconds_left: (resume_at - now).max(0),
                        });
                    }
                    if self.slice.is_zero() {
                        // A test clock moves on its own. One pass is
                        // enough to prove the state.
                        return false;
                    }
                    std::thread::sleep(self.slice);
                }
            }
        }
    }
}

/// The resource name of an API path. Comic Vine counts per resource,
/// so `/issues/` and `/issue/` are different buckets.
pub fn resource_of(path: &str) -> String {
    path.split('/')
        .find(|s| !s.is_empty())
        .unwrap_or("unknown")
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::SqliteCache;
    use std::sync::atomic::AtomicI64;

    fn budget(per_resource: i64) -> (Arc<SqliteCache>, Arc<AtomicI64>, Budget) {
        let cache = Arc::new(SqliteCache::in_memory().expect("cache"));
        let clock = Arc::new(AtomicI64::new(1_000_000));
        let tick = Arc::clone(&clock);
        let b = Budget::new(
            Arc::clone(&cache) as Arc<dyn CvCache>,
            BudgetPolicy {
                per_resource,
                window_seconds: 3600,
            },
        )
        .with_clock(
            Box::new(move || tick.load(Ordering::Relaxed)),
            Duration::ZERO,
        );
        (cache, clock, b)
    }

    #[test]
    fn the_resource_name_comes_from_the_first_path_segment() {
        assert_eq!(resource_of("/issues/"), "issues");
        assert_eq!(resource_of("/issue/"), "issue");
        assert_eq!(resource_of("/volume/4050-771/"), "volume");
        assert_eq!(resource_of("/volumes/"), "volumes");
        assert_eq!(resource_of("/search/"), "search");
        assert_eq!(resource_of(""), "unknown");
    }

    #[test]
    fn the_budget_counts_down_and_then_refuses() {
        let (_cache, _clock, b) = budget(3);
        assert_eq!(b.remaining("issues"), 3);
        assert!(b.acquire("issues"));
        assert_eq!(b.remaining("issues"), 2);
        assert!(b.acquire("issues"));
        assert!(b.acquire("issues"));
        assert_eq!(b.remaining("issues"), 0);
        // The window is full, and the test slice does not sleep.
        assert!(!b.acquire("issues"));
    }

    #[test]
    fn the_budget_is_per_resource() {
        let (_cache, _clock, b) = budget(2);
        assert!(b.acquire("issues"));
        assert!(b.acquire("issues"));
        assert_eq!(b.remaining("issues"), 0);
        // Another resource has its own full budget.
        assert_eq!(b.remaining("volume"), 2);
        assert!(b.acquire("volume"));
    }

    #[test]
    fn the_window_frees_a_slot_when_the_oldest_request_leaves() {
        let (_cache, clock, b) = budget(2);
        assert!(b.acquire("issues"));
        clock.fetch_add(600, Ordering::Relaxed);
        assert!(b.acquire("issues"));
        match b.state("issues") {
            BudgetState::Full { resume_at } => {
                // The first request was at 1_000_000. It leaves the
                // hour window one second after it expires.
                assert_eq!(resume_at, 1_000_000 + 3600 + 1);
            }
            other => panic!("expected Full, got {other:?}"),
        }
        // Move past that point: one slot returns.
        clock.store(1_000_000 + 3601, Ordering::Relaxed);
        assert_eq!(b.remaining("issues"), 1);
        assert!(b.acquire("issues"));
    }

    #[test]
    fn a_wait_is_reported_with_its_resume_time() {
        let (_cache, _clock, _) = budget(1);
        let cache = Arc::new(SqliteCache::in_memory().expect("cache"));
        let seen: Arc<std::sync::Mutex<Vec<WaitNotice>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);
        let b = Budget::new(
            Arc::clone(&cache) as Arc<dyn CvCache>,
            BudgetPolicy {
                per_resource: 1,
                window_seconds: 3600,
            },
        )
        .with_clock(Box::new(|| 1_000_000), Duration::ZERO)
        .with_wait_report(Box::new(move |n| {
            sink.lock().expect("sink").push(n.clone());
        }));

        assert!(b.acquire("issues"));
        assert!(!b.acquire("issues"));
        let notices = seen.lock().expect("sink").clone();
        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].resource, "issues");
        assert_eq!(notices[0].resume_at, 1_000_000 + 3601);
        assert_eq!(notices[0].seconds_left, 3601);
    }

    #[test]
    fn a_cancel_stops_a_wait_and_records_nothing() {
        let cache = Arc::new(SqliteCache::in_memory().expect("cache"));
        let cancel = Arc::new(AtomicBool::new(true));
        let b = Budget::new(
            Arc::clone(&cache) as Arc<dyn CvCache>,
            BudgetPolicy::default(),
        )
        .with_cancel(Arc::clone(&cancel))
        .with_clock(Box::new(|| 1_000_000), Duration::ZERO);

        assert!(!b.acquire("issues"), "a cancelled acquire refuses");
        assert_eq!(cache.requests_since("issues", 0).expect("count"), 0);
    }

    #[test]
    fn the_budget_survives_a_restart() {
        let cache = Arc::new(SqliteCache::in_memory().expect("cache"));
        let policy = BudgetPolicy {
            per_resource: 2,
            window_seconds: 3600,
        };
        {
            let b = Budget::new(Arc::clone(&cache) as Arc<dyn CvCache>, policy)
                .with_clock(Box::new(|| 1_000_000), Duration::ZERO);
            assert!(b.acquire("issues"));
            assert!(b.acquire("issues"));
        }
        // A new Budget over the SAME cache sees the spent window.
        let again = Budget::new(Arc::clone(&cache) as Arc<dyn CvCache>, policy)
            .with_clock(Box::new(|| 1_000_010), Duration::ZERO);
        assert_eq!(again.remaining("issues"), 0);
    }

    #[test]
    fn the_default_ceiling_is_the_user_figure() {
        let p = BudgetPolicy::default();
        assert_eq!(p.per_resource, 200);
        assert_eq!(p.window_seconds, 3600);
    }
}
