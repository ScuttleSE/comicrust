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

    let report =
        update::run(&client(&base), &cache, ENDPOINTS, None, &cancel, |_| {}).expect("run");

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

/// A full first page of 100 publishers with a reported total above the
/// page, so the run has a second page to fetch.
const BIG_PUBLISHERS: &[Canned] = &[Canned {
    path: "/publishers/",
    body: r#"{"status_code":1,"number_of_total_results":250,"number_of_page_results":100,
             "results":[
             {"id":1,"name":"P1"},{"id":2,"name":"P2"},{"id":3,"name":"P3"},{"id":4,"name":"P4"},
             {"id":5,"name":"P5"},{"id":6,"name":"P6"},{"id":7,"name":"P7"},{"id":8,"name":"P8"},
             {"id":9,"name":"P9"},{"id":10,"name":"P10"},{"id":11,"name":"P11"},{"id":12,"name":"P12"},
             {"id":13,"name":"P13"},{"id":14,"name":"P14"},{"id":15,"name":"P15"},{"id":16,"name":"P16"},
             {"id":17,"name":"P17"},{"id":18,"name":"P18"},{"id":19,"name":"P19"},{"id":20,"name":"P20"},
             {"id":21,"name":"P21"},{"id":22,"name":"P22"},{"id":23,"name":"P23"},{"id":24,"name":"P24"},
             {"id":25,"name":"P25"},{"id":26,"name":"P26"},{"id":27,"name":"P27"},{"id":28,"name":"P28"},
             {"id":29,"name":"P29"},{"id":30,"name":"P30"},{"id":31,"name":"P31"},{"id":32,"name":"P32"},
             {"id":33,"name":"P33"},{"id":34,"name":"P34"},{"id":35,"name":"P35"},{"id":36,"name":"P36"},
             {"id":37,"name":"P37"},{"id":38,"name":"P38"},{"id":39,"name":"P39"},{"id":40,"name":"P40"},
             {"id":41,"name":"P41"},{"id":42,"name":"P42"},{"id":43,"name":"P43"},{"id":44,"name":"P44"},
             {"id":45,"name":"P45"},{"id":46,"name":"P46"},{"id":47,"name":"P47"},{"id":48,"name":"P48"},
             {"id":49,"name":"P49"},{"id":50,"name":"P50"},{"id":51,"name":"P51"},{"id":52,"name":"P52"},
             {"id":53,"name":"P53"},{"id":54,"name":"P54"},{"id":55,"name":"P55"},{"id":56,"name":"P56"},
             {"id":57,"name":"P57"},{"id":58,"name":"P58"},{"id":59,"name":"P59"},{"id":60,"name":"P60"},
             {"id":61,"name":"P61"},{"id":62,"name":"P62"},{"id":63,"name":"P63"},{"id":64,"name":"P64"},
             {"id":65,"name":"P65"},{"id":66,"name":"P66"},{"id":67,"name":"P67"},{"id":68,"name":"P68"},
             {"id":69,"name":"P69"},{"id":70,"name":"P70"},{"id":71,"name":"P71"},{"id":72,"name":"P72"},
             {"id":73,"name":"P73"},{"id":74,"name":"P74"},{"id":75,"name":"P75"},{"id":76,"name":"P76"},
             {"id":77,"name":"P77"},{"id":78,"name":"P78"},{"id":79,"name":"P79"},{"id":80,"name":"P80"},
             {"id":81,"name":"P81"},{"id":82,"name":"P82"},{"id":83,"name":"P83"},{"id":84,"name":"P84"},
             {"id":85,"name":"P85"},{"id":86,"name":"P86"},{"id":87,"name":"P87"},{"id":88,"name":"P88"},
             {"id":89,"name":"P89"},{"id":90,"name":"P90"},{"id":91,"name":"P91"},{"id":92,"name":"P92"},
             {"id":93,"name":"P93"},{"id":94,"name":"P94"},{"id":95,"name":"P95"},{"id":96,"name":"P96"},
             {"id":97,"name":"P97"},{"id":98,"name":"P98"},{"id":99,"name":"P99"},{"id":100,"name":"P100"}
             ]}"#,
}];

#[test]
fn a_page_cap_stops_the_endpoint_and_holds_the_watermark() {
    let (base, served) = serve(BIG_PUBLISHERS);
    let cache = SqliteCache::in_memory().expect("cache");
    let cancel = AtomicBool::new(false);

    // Cap at one page: the endpoint fetches page one, then stops before
    // page two because the reported total (250) exceeds the offset.
    let report = update::run(
        &client(&base),
        &cache,
        &["publishers"],
        Some(1),
        &cancel,
        |_| {},
    )
    .expect("run");

    let ep = &report.endpoints[0];
    assert_eq!(ep.pages, 1);
    assert!(ep.capped, "the cap should mark the endpoint capped");
    assert!(!ep.complete);
    assert_eq!(served.load(Ordering::Relaxed), 1);

    // The watermark did NOT advance to today; the resume offset holds
    // page two, so the next run continues.
    let state = cache
        .sync_state("publishers")
        .expect("query")
        .expect("watermark");
    assert!(state.last_sync.as_str() < "2026-01-01");
    assert!(state.resume_state.unwrap().contains("100"));
}

#[test]
fn preflight_reports_the_changed_count_per_endpoint() {
    let (base, served) = serve(CANNED);
    let cache = SqliteCache::in_memory().expect("cache");
    let estimates = update::preflight(&client(&base), &cache, ENDPOINTS).expect("preflight");
    assert_eq!(estimates.len(), 4);
    // Each canned page reports number_of_total_results = 1.
    assert!(estimates.iter().all(|e| e.changed == 1));
    // One cheap request per endpoint.
    assert_eq!(served.load(Ordering::Relaxed), 4);
}
