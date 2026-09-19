use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use cr_scrape::cache::manage::{self, UpdateMode, UpdatePhase};
use cr_scrape::cache::{CvCache, IssueSkeleton, SqliteCache, VolumeRow};
use cr_scrape::cv::connection::CvClient;

struct Canned {
    path: &'static str,
    body: &'static str,
}

fn serve(canned: &'static [Canned]) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind mock server");
    let port = listener.local_addr().expect("server address").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = [0u8; 8192];
            let len = stream.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..len]);
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("");
            let body = canned
                .iter()
                .find(|item| path.contains(item.path))
                .map(|item| item.body)
                .unwrap_or(r#"{"status_code":101,"error":"not found"}"#);
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    format!("http://127.0.0.1:{port}/api")
}

fn client(base: &str) -> CvClient {
    CvClient::with_delays(
        "TEST",
        base,
        std::time::Duration::ZERO,
        std::time::Duration::ZERO,
    )
}

const VOLUME: &str = r#"{
  "status_code":1,
  "results":{
    "id":806,"name":"Example Series","start_year":"2001",
    "count_of_issues":2,"date_last_updated":"2026-09-19 12:00:00",
    "publisher":{"id":7,"name":"Example Press"},
    "description":"Full volume text","character_credits":[{"id":9,"name":"A"}]
  }
}"#;

const ISSUES: &str = r#"{
  "status_code":1,"number_of_total_results":2,
  "results":[
    {"id":92643,"issue_number":"1","volume":{"id":806}},
    {"id":92644,"issue_number":"2","volume":{"id":806}}
  ]
}"#;

const ISSUE_1: &str = r#"{
  "status_code":1,
  "results":{"id":92643,"issue_number":"1","name":"First",
             "cover_date":"2001-01-01","description":"Issue one"}
}"#;

const ISSUE_2: &str = r#"{
  "status_code":1,
  "results":{"id":92644,"issue_number":"2","name":"Second",
             "cover_date":"2001-02-01","description":"Issue two"}
}"#;

static RESPONSES: &[Canned] = &[
    Canned {
        path: "/volume/4050-806/",
        body: VOLUME,
    },
    Canned {
        path: "/issues/",
        body: ISSUES,
    },
    Canned {
        path: "/issue/4000-92643/",
        body: ISSUE_1,
    },
    Canned {
        path: "/issue/4000-92644/",
        body: ISSUE_2,
    },
];

#[test]
fn summary_update_replaces_membership_and_preserves_retained_detail() {
    let cache = SqliteCache::in_memory().expect("cache");
    cache
        .put_volumes(&[VolumeRow {
            volume_id: 806,
            name: Some("Manual".into()),
            last_cover_date: Some("2000-01-01".into()),
            ..Default::default()
        }])
        .expect("old volume");
    cache
        .put_issues(&[
            IssueSkeleton {
                issue_id: 92_643,
                volume_id: 806,
                issue_number: "1".into(),
                cover_date: Some("2000-01-01".into()),
                name: Some("Kept detail".into()),
            },
            IssueSkeleton {
                issue_id: 173_407,
                volume_id: 806,
                issue_number: "old".into(),
                ..Default::default()
            },
        ])
        .expect("old issues");

    let base = serve(RESPONSES);
    let report = manage::update_volume(
        &client(&base),
        &cache,
        806,
        UpdateMode::Summary,
        &AtomicBool::new(false),
        |_| {},
    )
    .expect("summary update");

    assert_eq!(report.issues, 2);
    assert_eq!(report.requests, 2);
    let stored = cache.managed_volume(806).expect("read").expect("volume");
    assert_eq!(stored.volume.name.as_deref(), Some("Example Series"));
    assert_eq!(stored.volume.publisher.as_deref(), Some("Example Press"));
    assert_eq!(stored.volume.start_year, Some(2001));
    assert_eq!(stored.volume.last_cover_date.as_deref(), Some("2000-01-01"));
    assert_eq!(stored.issues.len(), 2);
    assert_eq!(stored.issues[0].name.as_deref(), Some("Kept detail"));
    assert!(stored.detail_json.as_deref().is_some_and(|json| {
        json.contains("Full volume text") && json.contains("character_credits")
    }));
}

#[test]
fn complete_update_keeps_progress_and_resumes() {
    let cache = SqliteCache::in_memory().expect("cache");
    let base = serve(RESPONSES);
    let cancel = AtomicBool::new(false);
    let first = manage::update_volume(
        &client(&base),
        &cache,
        806,
        UpdateMode::Complete,
        &cancel,
        |progress| {
            if progress.phase == UpdatePhase::IssueDetails && progress.done == 1 {
                cancel.store(true, Ordering::Relaxed);
            }
        },
    )
    .expect("partial complete update");

    assert!(first.stopped);
    assert_eq!(first.issue_details, 1);
    assert_eq!(
        cache.pending_issue_details(806).expect("pending"),
        vec![92_644]
    );
    assert!(cache.issue_detail(92_643).expect("detail").is_some());
    assert!(cache.issue_detail(92_644).expect("detail").is_none());

    let resumed = manage::update_volume(
        &client(&base),
        &cache,
        806,
        UpdateMode::Complete,
        &AtomicBool::new(false),
        |_| {},
    )
    .expect("resume complete update");
    assert!(resumed.resumed);
    assert_eq!(resumed.issue_details, 1);
    assert_eq!(resumed.pending_issue_details, 0);
    assert!(cache
        .pending_issue_details(806)
        .expect("pending")
        .is_empty());
    let issues = cache.issues_of_volume(806).expect("issues");
    assert_eq!(issues[0].name.as_deref(), Some("First"));
    assert_eq!(issues[1].name.as_deref(), Some("Second"));
    assert_eq!(
        cache
            .volume(806)
            .expect("volume")
            .unwrap()
            .last_cover_date
            .as_deref(),
        Some("2001-02-01")
    );
}

#[test]
fn manual_metadata_can_create_and_clear_a_volume() {
    let cache = SqliteCache::in_memory().expect("cache");
    cache
        .update_volume_metadata(806, Some("Name"), Some("Publisher"), Some(2001))
        .expect("save");
    let saved = cache.volume(806).expect("read").expect("volume");
    assert_eq!(saved.name.as_deref(), Some("Name"));
    cache
        .update_volume_metadata(806, None, None, None)
        .expect("clear");
    let cleared = cache.volume(806).expect("read").expect("volume");
    assert_eq!(cleared.name, None);
    assert_eq!(cleared.publisher, None);
    assert_eq!(cleared.start_year, None);
}
