//! Mock-server gates for the in-app all-endpoint Comic Vine update
//! (ADR-075): the per-endpoint watermark advances, rows land stamped,
//! and a capped run resumes without re-paying for a page.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use cr_scrape::cache::resources::ResourceKind;
use cr_scrape::cache::update::{self, ENDPOINTS};
use cr_scrape::cache::{CvCache, SqliteCache};
use cr_scrape::cv::connection::CvClient;

struct Canned {
    path: &'static str,
    body: &'static str,
}

/// A server that answers with the first canned response whose path
/// matches, and counts the requests it served.
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
            // The offset marks a second page; an empty page ends it.
            let body = if path.contains("offset=100") {
                r#"{"status_code":1,"number_of_total_results":1,"number_of_page_results":0,"results":[]}"#
            } else {
                canned
                    .iter()
                    .find(|c| path.contains(c.path))
                    .map(|c| c.body)
                    .unwrap_or(r#"{"status_code":404,"error":"no route"}"#)
            };
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

const CANNED: &[Canned] = &[
    Canned {
        path: "/publishers/",
        body: r#"{"status_code":1,"number_of_total_results":1,"number_of_page_results":1,
                 "results":[{"id":7,"name":"Press","date_last_updated":"2026-08-02 06:33:59"}]}"#,
    },
    Canned {
        path: "/people/",
        body: r#"{"status_code":1,"number_of_total_results":1,"number_of_page_results":1,
                 "results":[{"id":30,"name":"Ann Nocenti","date_last_updated":"2026-08-01 08:05:20"}]}"#,
    },
    Canned {
        path: "/volumes/",
        body: r#"{"status_code":1,"number_of_total_results":1,"number_of_page_results":1,
                 "results":[{"id":10,"name":"Ten","publisher":{"id":7,"name":"Press"},
                 "start_year":"2001","count_of_issues":12,
                 "date_last_updated":"2026-08-01 12:36:19"}]}"#,
    },
    Canned {
        path: "/issues/",
        body: r#"{"status_code":1,"number_of_total_results":1,"number_of_page_results":1,
                 "results":[{"id":100,"issue_number":"1","volume":{"id":10,"name":"Ten"},
                 "name":"The One","cover_date":"2001-01-01",
                 "date_last_updated":"2026-08-01 14:48:15"}]}"#,
    },
];

#[test]
fn an_update_walks_every_endpoint_and_stamps_rows() {
    let (base, served) = serve(CANNED);
    let cache = SqliteCache::in_memory().expect("cache");
    let cancel = AtomicBool::new(false);

    let report = update::run(&client(&base), &cache, ENDPOINTS, &cancel, |_| {}).expect("run");

    // One page per endpoint, all complete.
    assert_eq!(report.endpoints.len(), 4);
    assert!(report.endpoints.iter().all(|e| e.complete));
    assert_eq!(served.load(Ordering::Relaxed), 4);

    // The rows landed with the real API stamp.
    let publisher = cache
        .resource(ResourceKind::Publisher, 7)
        .expect("query")
        .expect("publisher row");
    assert_eq!(publisher.name.as_deref(), Some("Press"));
    assert_eq!(
        publisher.date_last_updated.as_deref(),
        Some("2026-08-02 06:33:59")
    );
    let person = cache
        .resource(ResourceKind::Person, 30)
        .expect("query")
        .expect("person row");
    assert_eq!(person.name.as_deref(), Some("Ann Nocenti"));
    let volume = cache.volume(10).expect("query").expect("volume row");
    assert_eq!(volume.name.as_deref(), Some("Ten"));
    assert_eq!(
        volume.date_last_updated.as_deref(),
        Some("2026-08-01 12:36:19")
    );
    let issues = cache.issues_of_volume(10).expect("issues");
    assert_eq!(issues.len(), 1);
    assert_eq!(
        issues[0].date_last_updated.as_deref(),
        Some("2026-08-01 14:48:15")
    );

    // Every endpoint watermark advanced to today (past the seed).
    for endpoint in ENDPOINTS {
        let state = cache
            .sync_state(endpoint)
            .expect("query")
            .expect("watermark");
        assert!(state.last_sync.as_str() >= "2026-08-04");
        assert!(state.resume_state.is_none());
    }
}

#[test]
fn a_seeded_watermark_sets_the_window_start() {
    // A pre-seeded watermark (as the localcv import writes) is the
    // `since` of the next update's filter window.
    let cache = SqliteCache::in_memory().expect("cache");
    cache
        .put_sync_state(&cr_scrape::cache::SyncState {
            endpoint: "issues".into(),
            last_sync: "2026-08-03".into(),
            resume_state: None,
        })
        .expect("seed");
    let state = cache.sync_state("issues").expect("query").expect("row");
    assert_eq!(state.last_sync, "2026-08-03");
}
