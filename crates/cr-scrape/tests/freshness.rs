//! Mock-server gates for the freshness rule (ADR-037, Phase 15 T4).
//!
//! The gates measure REQUESTS, because the whole point of the rule is
//! what it does not ask for.

use std::io::{Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use cr_scrape::cache::freshness::{self, FreshnessPolicy, Verdict};
use cr_scrape::cache::{CvCache, IssueSkeleton, SqliteCache, VolumeRow};
use cr_scrape::cv::connection::CvClient;

/// 2020-01-01T00:00:00Z.
const NOW: i64 = 1_577_836_800;

/// A server whose answers the test can change between calls. It
/// records every path it served.
struct Server {
    base: String,
    paths: Arc<Mutex<Vec<String>>>,
    count: Arc<AtomicUsize>,
    body: Arc<Mutex<Vec<(String, String)>>>,
}

impl Server {
    fn start() -> Server {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let paths = Arc::new(Mutex::new(Vec::new()));
        let count = Arc::new(AtomicUsize::new(0));
        let body: Arc<Mutex<Vec<(String, String)>>> = Arc::new(Mutex::new(Vec::new()));
        let (p, c, b) = (Arc::clone(&paths), Arc::clone(&count), Arc::clone(&body));
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
                c.fetch_add(1, Ordering::Relaxed);
                let reply = b
                    .lock()
                    .expect("body")
                    .iter()
                    .find(|(m, _)| path.contains(m.as_str()))
                    .map(|(_, v)| v.clone())
                    .unwrap_or_else(|| r#"{"status_code":404,"error":"no route"}"#.to_string());
                p.lock().expect("paths").push(path);
                let response = format!(
                    "HTTP/1.1 200 X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    reply.len(),
                    reply
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        Server {
            base: format!("http://127.0.0.1:{port}/api"),
            paths,
            count,
            body,
        }
    }

    fn reply(&self, matches: &str, body: String) {
        self.body
            .lock()
            .expect("body")
            .push((matches.to_string(), body));
    }

    fn clear_replies(&self) {
        self.body.lock().expect("body").clear();
    }

    fn requests(&self) -> usize {
        self.count.load(Ordering::Relaxed)
    }

    fn paths(&self) -> Vec<String> {
        self.paths.lock().expect("paths").clone()
    }

    fn client(&self) -> CvClient {
        CvClient::with_delays(
            "TESTKEY",
            &self.base,
            std::time::Duration::from_millis(0),
            std::time::Duration::from_millis(0),
        )
    }
}

fn volume_body(count: i32, updated: &str) -> String {
    format!(
        r#"{{"status_code":1,"results":{{"id":771,"name":"Blacksad","count_of_issues":{count},"date_last_updated":"{updated}","start_year":"2010","publisher":{{"id":1,"name":"Dark Horse"}}}}}}"#
    )
}

fn issues_body(first: i64, n: i64, total: i64, cover: &str) -> String {
    let rows: Vec<String> = (first..first + n)
        .map(|id| {
            format!(
                r#"{{"id":{id},"issue_number":"{id}","name":"Issue {id}","cover_date":"{cover}"}}"#
            )
        })
        .collect();
    format!(
        r#"{{"status_code":1,"number_of_total_results":{total},"number_of_page_results":{n},"results":[{}]}}"#,
        rows.join(",")
    )
}

/// A volume the cache holds as OPEN: its last issue is recent, and
/// its last check was two days ago. `fetched_at` merges with MAX, so
/// an open volume must be seeded open — a later write cannot move the
/// time backwards.
fn seed_open(cache: &SqliteCache) {
    cache
        .put_volumes(&[VolumeRow {
            volume_id: 771,
            count_of_issues: Some(3),
            date_last_updated: Some("2013-06-02 00:00:00".into()),
            last_cover_date: Some("2019-12-01".into()),
            fetched_at: NOW - 2 * 86_400,
            ..Default::default()
        }])
        .expect("write");
    cache
        .put_issues(
            &(1..=3)
                .map(|id| IssueSkeleton {
                    issue_id: id,
                    volume_id: 771,
                    issue_number: id.to_string(),
                    cover_date: Some("2019-12-01".into()),
                    name: None,
                })
                .collect::<Vec<_>>(),
        )
        .expect("write");
}

/// A volume that the cache already holds as closed.
fn seed_closed(cache: &SqliteCache) {
    cache
        .put_volumes(&[VolumeRow {
            volume_id: 771,
            count_of_issues: Some(3),
            last_cover_date: Some("2013-06-01".into()),
            fetched_at: NOW - 10,
            ..Default::default()
        }])
        .expect("write");
    cache
        .put_issues(
            &(1..=3)
                .map(|id| IssueSkeleton {
                    issue_id: id,
                    volume_id: 771,
                    issue_number: id.to_string(),
                    cover_date: Some("2013-06-01".into()),
                    name: None,
                })
                .collect::<Vec<_>>(),
        )
        .expect("write");
}

#[test]
fn a_closed_volume_makes_zero_requests() {
    let server = Server::start();
    let cache = SqliteCache::in_memory().expect("cache");
    seed_closed(&cache);

    let (issues, report) = freshness::issues_of_volume(
        &server.client(),
        &cache,
        771,
        &FreshnessPolicy::default(),
        NOW,
    )
    .expect("read");

    assert_eq!(report.verdict_was, Some(Verdict::Fresh));
    assert_eq!(report.requests, 0);
    assert_eq!(server.requests(), 0);
    assert_eq!(issues.len(), 3);
}

#[test]
fn an_unchanged_open_volume_makes_one_request() {
    let server = Server::start();
    server.reply("/volume/4050-771", volume_body(3, "2013-06-02 00:00:00"));
    let cache = SqliteCache::in_memory().expect("cache");
    seed_open(&cache);

    let (issues, report) = freshness::issues_of_volume(
        &server.client(),
        &cache,
        771,
        &FreshnessPolicy::default(),
        NOW,
    )
    .expect("read");

    assert_eq!(report.verdict_was, Some(Verdict::Revalidate));
    assert!(!report.repaged, "the probe found no change");
    assert_eq!(report.requests, 1);
    assert_eq!(server.requests(), 1);
    assert!(server.paths()[0].contains("/volume/4050-771"));
    assert_eq!(issues.len(), 3);
}

#[test]
fn a_changed_open_volume_repages_its_issue_list() {
    let server = Server::start();
    // The API now counts 4 issues, and its update date moved.
    server.reply("/volume/4050-771", volume_body(4, "2020-01-01 00:00:00"));
    server.reply("/issues/", issues_body(1, 4, 4, "2019-12-01"));
    let cache = SqliteCache::in_memory().expect("cache");
    seed_open(&cache);

    let (issues, report) = freshness::issues_of_volume(
        &server.client(),
        &cache,
        771,
        &FreshnessPolicy::default(),
        NOW,
    )
    .expect("read");

    assert_eq!(report.verdict_was, Some(Verdict::Revalidate));
    assert!(report.repaged);
    // One probe plus one page.
    assert_eq!(report.requests, 2);
    assert_eq!(issues.len(), 4);
    assert_eq!(cache.issue_count(771).expect("count"), 4);
    let v = cache.volume(771).expect("read").expect("present");
    assert_eq!(v.count_of_issues, Some(4));
    assert_eq!(v.last_cover_date.as_deref(), Some("2019-12-01"));
    assert_eq!(v.fetched_at, NOW);
    assert_eq!(v.name.as_deref(), Some("Blacksad"));
    assert_eq!(v.publisher.as_deref(), Some("Dark Horse"));
    assert_eq!(v.start_year, Some(2010));
}

#[test]
fn an_unknown_volume_pages_the_whole_issue_list() {
    let server = Server::start();
    server.reply("/volume/4050-771", volume_body(150, "2020-01-01 00:00:00"));
    server.reply("offset=100", issues_body(101, 50, 150, "2019-12-01"));
    server.reply("offset=0", issues_body(1, 100, 150, "2011-01-01"));
    let cache = SqliteCache::in_memory().expect("cache");

    let (issues, report) = freshness::issues_of_volume(
        &server.client(),
        &cache,
        771,
        &FreshnessPolicy::default(),
        NOW,
    )
    .expect("read");

    assert_eq!(report.verdict_was, Some(Verdict::Fetch));
    // One probe plus two pages.
    assert_eq!(report.requests, 3);
    assert_eq!(issues.len(), 150);
    assert_eq!(cache.issue_count(771).expect("count"), 150);
    // The latest cover date over the whole list, not the last page
    // read.
    let v = cache.volume(771).expect("read").expect("present");
    assert_eq!(v.last_cover_date.as_deref(), Some("2019-12-01"));

    // The volume is now closed, so a second call asks for nothing.
    server.clear_replies();
    let before = server.requests();
    let (again, second) = freshness::issues_of_volume(
        &server.client(),
        &cache,
        771,
        &FreshnessPolicy::default(),
        NOW,
    )
    .expect("read");
    assert_eq!(second.verdict_was, Some(Verdict::Fresh));
    assert_eq!(server.requests(), before);
    assert_eq!(again.len(), 150);
}

#[test]
fn an_mcl_seeded_volume_still_fetches_its_detail() {
    let server = Server::start();
    server.reply("/volume/4050-771", volume_body(3, "2020-01-01 00:00:00"));
    server.reply("/issues/", issues_body(1, 3, 3, "2013-06-01"));
    let cache = SqliteCache::in_memory().expect("cache");
    // An MCL import fills the skeleton and no API field.
    cr_scrape::cache::mcl::import(&cache, "Missing;2026-08-26\n771;1,2,3;1,2,3,\n".as_bytes())
        .expect("import");
    assert_eq!(cache.issue_count(771).expect("count"), 3);

    let (_, report) = freshness::issues_of_volume(
        &server.client(),
        &cache,
        771,
        &FreshnessPolicy::default(),
        NOW,
    )
    .expect("read");
    assert_eq!(report.verdict_was, Some(Verdict::Fetch));
    assert_eq!(report.requests, 2);
}

// --- the warm task (ADR-037, Phase 15 T6) ---

#[test]
fn the_warm_task_skips_what_is_already_fresh() {
    use cr_scrape::cache::warm::{self, WarmOptions};
    use std::sync::atomic::AtomicBool;

    let server = Server::start();
    let cache = SqliteCache::in_memory().expect("cache");
    seed_closed(&cache);
    let cancel = AtomicBool::new(false);

    let report = warm::run(
        &server.client(),
        &cache,
        &[771],
        &WarmOptions::default(),
        &cancel,
        |_| {},
    );
    assert_eq!(report.considered, 1);
    assert_eq!(report.already_fresh, 1);
    assert_eq!(report.warmed, 0);
    assert_eq!(report.requests, 0);
    assert_eq!(server.requests(), 0);
    assert!(!report.stopped_early);
}

#[test]
fn the_warm_task_fills_an_unknown_volume() {
    use cr_scrape::cache::warm::{self, WarmOptions};
    use std::sync::atomic::AtomicBool;

    let server = Server::start();
    server.reply("/volume/4050-771", volume_body(3, "2020-01-01 00:00:00"));
    server.reply("/issues/", issues_body(1, 3, 3, "2013-06-01"));
    let cache = SqliteCache::in_memory().expect("cache");
    let cancel = AtomicBool::new(false);

    let mut seen = Vec::new();
    let report = warm::run(
        &server.client(),
        &cache,
        &[771],
        &WarmOptions::default(),
        &cancel,
        |p| seen.push((p.volume_id, p.done, p.total)),
    );
    assert_eq!(report.warmed, 1);
    // One probe plus one page.
    assert_eq!(report.requests, 2);
    assert_eq!(seen, vec![(771, 1, 1)]);
    assert_eq!(cache.issue_count(771).expect("count"), 3);
}

#[test]
fn the_request_cap_stops_the_warm_task() {
    use cr_scrape::cache::warm::{self, WarmOptions};
    use std::sync::atomic::AtomicBool;

    let server = Server::start();
    server.reply("/volume/4050-771", volume_body(3, "2020-01-01 00:00:00"));
    server.reply("/issues/", issues_body(1, 3, 3, "2013-06-01"));
    let cache = SqliteCache::in_memory().expect("cache");
    let cancel = AtomicBool::new(false);

    let report = warm::run(
        &server.client(),
        &cache,
        &[771, 772, 773],
        &WarmOptions {
            max_requests: Some(1),
            ..WarmOptions::default()
        },
        &cancel,
        |_| {},
    );
    // The first volume is allowed to finish; the cap stops the next.
    assert_eq!(report.considered, 1);
    assert!(report.stopped_early);
    assert_eq!(report.stopped_at, Some(772));
}

#[test]
fn a_failed_volume_does_not_stop_the_warm_task() {
    use cr_scrape::cache::warm::{self, WarmOptions};
    use std::sync::atomic::AtomicBool;

    let server = Server::start();
    // Volume 771 answers; volume 772 has no route.
    server.reply("/volume/4050-771", volume_body(3, "2020-01-01 00:00:00"));
    server.reply("/issues/", issues_body(1, 3, 3, "2013-06-01"));
    let cache = SqliteCache::in_memory().expect("cache");
    let cancel = AtomicBool::new(false);

    let report = warm::run(
        &server.client(),
        &cache,
        &[772, 771],
        &WarmOptions::default(),
        &cancel,
        |_| {},
    );
    assert_eq!(report.failed, 1);
    assert_eq!(report.warmed, 1);
    assert!(!report.stopped_early);
    assert_eq!(cache.issue_count(771).expect("count"), 3);
}

#[test]
fn a_spent_budget_stops_the_warm_task() {
    use cr_scrape::cache::budget::{Budget, BudgetPolicy};
    use cr_scrape::cache::warm::{self, WarmOptions};
    use cr_scrape::cache::CvCache;
    use std::sync::atomic::AtomicBool;

    let server = Server::start();
    server.reply("/volume/4050-", volume_body(3, "2020-01-01 00:00:00"));
    server.reply("/issues/", issues_body(1, 3, 3, "2013-06-01"));
    let cache = Arc::new(SqliteCache::in_memory().expect("cache"));
    // One `/volume/` request per hour, so the second volume is
    // refused at its probe.
    let budget = Arc::new(
        Budget::new(
            Arc::clone(&cache) as Arc<dyn CvCache>,
            BudgetPolicy {
                per_resource: 1,
                window_seconds: 3600,
            },
        )
        .with_clock(Box::new(|| 1_000_000), std::time::Duration::ZERO),
    );
    let mut client = server.client();
    client.set_budget(budget);
    let cancel = AtomicBool::new(false);

    let report = warm::run(
        &client,
        cache.as_ref(),
        &[771, 772, 773],
        &WarmOptions::default(),
        &cancel,
        |_| {},
    );
    assert_eq!(report.warmed, 1);
    assert!(report.stopped_early);
    assert_eq!(report.stopped_at, Some(772));
}
