//! Mock-server gates for the incremental Comic Vine sweep (ADR-038,
//! Phase 15 T3): paging, resume, the cancel flag, and the page cap.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use cr_scrape::cache::sweep::{self, SweepOptions};
use cr_scrape::cache::{CvCache, SqliteCache};
use cr_scrape::cv::connection::CvClient;

/// One canned response. The request path must contain `path`.
struct Canned {
    path: &'static str,
    body: &'static str,
}

/// A server that answers with the first canned response whose path
/// matches. It also counts the requests it served, so a gate can prove
/// that a resume paid for no repeated page.
fn serve(canned: &'static [Canned]) -> (String, Arc<AtomicUsize>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let served = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&served);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 8192];
            let len = stream.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..len]).to_string();
            let path = request
                .lines()
                .next()
                .unwrap_or("")
                .split_whitespace()
                .nth(1)
                .unwrap_or("")
                .to_string();
            counter.fetch_add(1, Ordering::Relaxed);
            let body = canned
                .iter()
                .find(|c| path.contains(c.path))
                .map(|c| c.body)
                .unwrap_or(r#"{"status_code":404,"error":"no route"}"#);
            let response = format!(
                "HTTP/1.1 200 X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    (format!("http://127.0.0.1:{port}/api"), served)
}

fn client(base: &str) -> CvClient {
    CvClient::with_delays(
        "TESTKEY",
        base,
        std::time::Duration::from_millis(0),
        std::time::Duration::from_millis(0),
    )
}

fn options() -> SweepOptions {
    SweepOptions {
        start_date: "2026-08-26".into(),
        end_date: "2026-09-12".into(),
        max_pages: None,
    }
}

/// A page of 100 issues, ids `first..first+100`, all in one volume.
fn page(first: i64, total: i64) -> String {
    let results: Vec<String> = (first..first + 100)
        .map(|id| format!(r#"{{"id":{id},"issue_number":"{id}","volume":{{"id":10}}}}"#))
        .collect();
    format!(
        r#"{{"status_code":1,"number_of_total_results":{total},"number_of_page_results":100,"results":[{}]}}"#,
        results.join(",")
    )
}

// Two full pages then a short one: 250 issues in total.
static PAGE_0: &str = include_str!("testdata/sweep/page0.json");
static PAGE_100: &str = include_str!("testdata/sweep/page100.json");
static PAGE_200: &str = include_str!("testdata/sweep/page200.json");

static THREE_PAGES: &[Canned] = &[
    Canned {
        path: "offset=200",
        body: PAGE_200,
    },
    Canned {
        path: "offset=100",
        body: PAGE_100,
    },
    Canned {
        path: "offset=0",
        body: PAGE_0,
    },
];

#[test]
fn the_fixture_pages_match_the_generator() {
    // The fixtures are large, so this gate proves they still hold the
    // shape the other gates assume.
    assert_eq!(PAGE_0.trim(), page(1, 250).trim());
    assert_eq!(PAGE_100.trim(), page(101, 250).trim());
}

#[test]
fn a_sweep_pages_to_the_end_and_fills_the_skeleton() {
    let (base, served) = serve(THREE_PAGES);
    let cache = SqliteCache::in_memory().expect("cache");
    let cancel = AtomicBool::new(false);
    let mut seen = Vec::new();

    let report = sweep::run(&client(&base), &cache, &options(), &cancel, |p| {
        seen.push((p.offset, p.total))
    })
    .expect("sweep");

    assert!(report.complete);
    assert!(!report.stopped_early);
    assert_eq!(report.pages, 3);
    assert_eq!(report.issues, 250);
    assert_eq!(served.load(Ordering::Relaxed), 3);
    assert_eq!(seen, vec![(100, 250), (200, 250), (250, 250)]);
    assert_eq!(cache.issue_count(10).expect("count"), 250);
    assert!(cache.volume(10).expect("read").is_some());
}

#[test]
fn a_cancelled_sweep_resumes_where_it_stopped() {
    let (base, served) = serve(THREE_PAGES);
    let cache = SqliteCache::in_memory().expect("cache");

    // Stop after the first page.
    let cancel = AtomicBool::new(false);
    let first = sweep::run(&client(&base), &cache, &options(), &cancel, |_| {
        cancel.store(true, Ordering::Relaxed)
    })
    .expect("sweep");
    assert!(first.stopped_early);
    assert!(!first.complete);
    assert_eq!(first.pages, 1);
    assert_eq!(cache.issue_count(10).expect("count"), 100);
    assert_eq!(
        cache.sweep_state().expect("read").expect("present").offset,
        100
    );

    // Resume. It must read pages 2 and 3 only.
    let served_before = served.load(Ordering::Relaxed);
    let go = AtomicBool::new(false);
    let second = sweep::run(&client(&base), &cache, &options(), &go, |_| {}).expect("sweep");
    assert!(second.complete);
    assert_eq!(second.pages, 2, "the first page is not paid for again");
    assert_eq!(served.load(Ordering::Relaxed) - served_before, 2);
    assert_eq!(cache.issue_count(10).expect("count"), 250);
}

#[test]
fn a_complete_sweep_makes_no_request_when_it_runs_again() {
    let (base, served) = serve(THREE_PAGES);
    let cache = SqliteCache::in_memory().expect("cache");
    let cancel = AtomicBool::new(false);
    sweep::run(&client(&base), &cache, &options(), &cancel, |_| {}).expect("sweep");
    let served_before = served.load(Ordering::Relaxed);

    let again = sweep::run(&client(&base), &cache, &options(), &cancel, |_| {}).expect("sweep");
    assert!(again.complete);
    assert_eq!(again.pages, 0);
    assert_eq!(served.load(Ordering::Relaxed), served_before);
}

#[test]
fn a_new_window_starts_at_offset_zero() {
    let (base, _) = serve(THREE_PAGES);
    let cache = SqliteCache::in_memory().expect("cache");
    let cancel = AtomicBool::new(false);
    sweep::run(&client(&base), &cache, &options(), &cancel, |_| {}).expect("sweep");

    let next = SweepOptions {
        start_date: "2026-09-12".into(),
        end_date: "2026-09-30".into(),
        max_pages: None,
    };
    let report = sweep::run(&client(&base), &cache, &next, &cancel, |_| {}).expect("sweep");
    assert_eq!(report.pages, 3, "the new window re-reads from zero");
    assert_eq!(report.state.start_date, "2026-09-12");
}

#[test]
fn the_page_cap_stops_the_run_and_keeps_the_offset() {
    let (base, _) = serve(THREE_PAGES);
    let cache = SqliteCache::in_memory().expect("cache");
    let cancel = AtomicBool::new(false);
    let capped = SweepOptions {
        max_pages: Some(2),
        ..options()
    };

    let report = sweep::run(&client(&base), &cache, &capped, &cancel, |_| {}).expect("sweep");
    assert!(report.stopped_early);
    assert!(!report.complete);
    assert_eq!(report.pages, 2);
    assert_eq!(report.state.offset, 200);
    assert_eq!(cache.issue_count(10).expect("count"), 200);
}

#[test]
fn a_cancel_before_the_first_page_makes_no_request() {
    let (base, served) = serve(THREE_PAGES);
    let cache = SqliteCache::in_memory().expect("cache");
    let cancel = AtomicBool::new(true);

    let report = sweep::run(&client(&base), &cache, &options(), &cancel, |_| {}).expect("sweep");
    assert!(report.stopped_early);
    assert_eq!(report.pages, 0);
    assert_eq!(served.load(Ordering::Relaxed), 0);
}

// --- the request budget at the client chokepoint (ADR-037, T5) ---

#[test]
fn the_budget_stops_the_sweep_and_the_state_survives() {
    use cr_scrape::cache::budget::{Budget, BudgetPolicy};

    let (base, served) = serve(THREE_PAGES);
    let cache = Arc::new(SqliteCache::in_memory().expect("cache"));
    // Two requests per resource per hour, and no sleeping.
    let budget = Arc::new(
        Budget::new(
            Arc::clone(&cache) as Arc<dyn CvCache>,
            BudgetPolicy {
                per_resource: 2,
                window_seconds: 3600,
            },
        )
        .with_clock(Box::new(|| 1_000_000), std::time::Duration::ZERO),
    );
    let mut c = client(&base);
    c.set_budget(Arc::clone(&budget));

    let cancel = AtomicBool::new(false);
    let err = sweep::run(&c, cache.as_ref(), &options(), &cancel, |_| {})
        .expect_err("the third page must be refused");
    assert!(
        matches!(err, cr_scrape::cv::connection::CvError::BudgetSpent(ref r) if r == "issues"),
        "got {err:?}"
    );
    // Two pages went out, and no more.
    assert_eq!(served.load(Ordering::Relaxed), 2);
    assert_eq!(budget.remaining("issues"), 0);
    // The offset of the pages that DID land is stored, so the run
    // resumes when the window frees.
    assert_eq!(
        cache.sweep_state().expect("read").expect("present").offset,
        200
    );
    assert_eq!(cache.issue_count(10).expect("count"), 200);
}

#[test]
fn the_budget_counts_each_resource_on_its_own() {
    use cr_scrape::cache::budget::{Budget, BudgetPolicy};

    let (base, _) = serve(THREE_PAGES);
    let cache = Arc::new(SqliteCache::in_memory().expect("cache"));
    let budget = Arc::new(
        Budget::new(
            Arc::clone(&cache) as Arc<dyn CvCache>,
            BudgetPolicy {
                per_resource: 3,
                window_seconds: 3600,
            },
        )
        .with_clock(Box::new(|| 1_000_000), std::time::Duration::ZERO),
    );
    let mut c = client(&base);
    c.set_budget(Arc::clone(&budget));

    let cancel = AtomicBool::new(false);
    sweep::run(&c, cache.as_ref(), &options(), &cancel, |_| {}).expect("sweep");

    assert_eq!(budget.remaining("issues"), 0);
    // `/volume/` is a different bucket and is untouched.
    assert_eq!(budget.remaining("volume"), 3);
    assert_eq!(c.remaining_budget("/issues/"), Some(0));
    assert_eq!(c.remaining_budget("/volume/4050-771/"), Some(3));
}
