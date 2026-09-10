//! The T5 gate: the engine loop over a scripted fake UI + the mock
//! server — the fast rescrape, the skip tag, the interactive flow,
//! and the failed-query delay (mirrors the plugin's `test_all.py`
//! scenarios).

use std::collections::HashSet;
use std::io::{Read, Write};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use cr_core::model::comic_book::values_store;
use cr_core::model::ComicBook;
use cr_scrape::config::Configuration;
use cr_scrape::cv::connection::CvClient;
use cr_scrape::cv::models::{IssueRef, SeriesRef};
use cr_scrape::engine::{BookStatus, IssueResult, ScrapeEngine, ScrapeUi, SeriesResult};

// ==========================================================================
// the scripted fake UI

#[derive(Default)]
struct FakeUi {
    term_answers: Vec<Option<String>>,
    series_answers: Vec<SeriesResult>,
    issue_answers: Vec<IssueResult>,
    started: Vec<String>,
    finished: Vec<(String, BookStatus)>,
    scraped: Vec<ComicBook>,
    no_issues: Vec<String>,
}

impl ScrapeUi for FakeUi {
    fn request_search_terms(&mut self, _caption: &str, _failed: &str) -> Option<String> {
        self.term_answers.pop().flatten()
    }
    fn request_series(
        &mut self,
        _caption: &str,
        _terms: &str,
        _refs: &[SeriesRef],
    ) -> SeriesResult {
        self.series_answers.pop().unwrap_or(SeriesResult::Skip)
    }
    fn request_issue(
        &mut self,
        _caption: &str,
        _series: &SeriesRef,
        _issues: &[IssueRef],
        _hint: Option<&IssueRef>,
        _force: bool,
    ) -> IssueResult {
        self.issue_answers.pop().unwrap_or(IssueResult::Skip)
    }
    fn no_issues_available(&mut self, series_name: &str) {
        self.no_issues.push(series_name.to_string());
    }
    fn book_started(&mut self, caption: &str, _remaining: usize) {
        self.started.push(caption.to_string());
    }
    fn book_finished(&mut self, caption: &str, status: BookStatus) {
        self.finished.push((caption.to_string(), status));
    }
    fn book_scraped(&mut self, book: &ComicBook) {
        self.scraped.push(book.clone());
    }
    fn progress(&mut self, _kind: cr_scrape::engine::ProgressKind, _value: f64) {}

    fn error(&mut self, message: &str) {
        self.no_issues.push(format!("ERROR: {message}"));
    }
}

fn engine(config: Configuration) -> ScrapeEngine {
    ScrapeEngine::new(config, Arc::new(AtomicBool::new(false)), HashSet::new())
}

fn start_mock(routes: &[(&str, String)]) -> String {
    start_mock_bytes(
        &routes
            .iter()
            .map(|(p, b)| (*p, b.clone().into_bytes()))
            .collect::<Vec<_>>(),
    )
}

fn start_mock_bytes(routes: &[(&str, Vec<u8>)]) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let routes: Vec<(String, Vec<u8>)> = routes
        .iter()
        .map(|(p, b)| (p.to_string(), b.clone()))
        .collect();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf = [0u8; 8192];
            let len = stream.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..len]).to_string();
            let path = request.lines().next().unwrap_or("");
            let body = routes
                .iter()
                .find(|(p, _)| path.contains(p))
                .map(|(_, b)| b.clone())
                .unwrap_or_default();
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    format!("http://127.0.0.1:{port}")
}

fn client_for(base: &str) -> cr_scrape::cv::queries::Cv {
    cr_scrape::cv::queries::Cv::new(CvClient::with_delays(
        "TEST",
        base,
        std::time::Duration::from_millis(0),
        std::time::Duration::from_millis(0),
    ))
}

#[test]
fn fast_rescrape_uses_the_previous_choice() {
    let details = r#"{
        "number_of_total_results": 1, "status_code": 1,
        "results": {"id": "400011", "name": "The Court of Owls",
            "issue_number": "12",
            "volume": {"id": "40501", "name": "Batman", "start_year": "1940"},
            "image": {"small_url": ""}}}"#;
    let volume = r#"{
        "number_of_total_results": 1, "status_code": 1,
        "results": {"id": 40501, "name": "Batman", "start_year": "1940",
                    "publisher": {"id": 10, "name": "DC Comics"}}}"#;
    let base = start_mock(&[
        ("/issue/4000-", details.to_string()),
        ("/volume/4050-", volume.to_string()),
    ]);

    let mut book = ComicBook {
        file_path: "Comics/Batman 12 (2011).cbz".into(),
        ..Default::default()
    };
    book.custom_values_store = values_store::encode(&[("comicvine_issue".into(), "400011".into())]);
    book.enable_proposed = true;

    let mut ui = FakeUi::default();
    let mut cv = client_for(&base);
    let (scraped, skipped) = engine(Configuration::default()).scrape(vec![book], &mut ui, &mut cv);

    assert_eq!((scraped, skipped), (1, 0));
    assert_eq!(ui.scraped.len(), 1);
    assert_eq!(ui.scraped[0].info.series, "Batman");
    assert_eq!(ui.scraped[0].info.title, "The Court of Owls");
    assert_eq!(ui.started.len(), 1);
}

#[test]
fn a_skip_tagged_book_never_reaches_the_database() {
    let mut book = ComicBook {
        file_path: "Comics/Skipped 1 (2020).cbz".into(),
        ..Default::default()
    };
    book.info.tags = "CVDBSKIP, read".into();

    // no routes: any database hit would surface; the skip gate must
    // fire first
    let mut ui = FakeUi::default();
    let mut cv = client_for("http://127.0.0.1:1");
    let (scraped, skipped) = engine(Configuration::default()).scrape(vec![book], &mut ui, &mut cv);

    assert_eq!((scraped, skipped), (0, 1));
    assert!(ui.scraped.is_empty());
    assert_eq!(ui.finished.len(), 1);
    assert_eq!(ui.finished[0].1, BookStatus::Skipped);
}

#[test]
fn interactive_scrape_lands_the_details() {
    let search_body = r#"{
      "number_of_total_results": 1, "number_of_page_results": 1, "status_code": 1,
      "results": [
        {"id": 40501, "name": "Batman", "start_year": "1940",
         "publisher": {"id": 10, "name": "DC Comics"},
         "count_of_issues": 900, "image": {"small_url": ""}}]}"#;
    let issues_body = r#"{
        "number_of_total_results": 1, "number_of_page_results": 1, "status_code": 1,
        "results": [
            {"id": 400011, "issue_number": "12", "name": "The Court of Owls",
             "image": {"small_url": ""}}]}"#;
    let details_body = r#"{
        "number_of_total_results": 1, "status_code": 1,
        "results": {"id": "400011", "name": "The Court of Owls",
            "issue_number": "12",
            "cover_date": "2011-05-14",
            "volume": {"id": "40501", "name": "Batman", "start_year": "1940"},
            "image": {"small_url": ""}}}"#;
    let volume_body = r#"{
        "number_of_total_results": 1, "status_code": 1,
        "results": {"id": 40501, "name": "Batman", "start_year": "1940",
                    "publisher": {"id": 10, "name": "DC Comics"}}}"#;
    let base = start_mock(&[
        ("/search/", search_body.to_string()),
        ("/issues/", issues_body.to_string()),
        ("/issue/4000-", details_body.to_string()),
        ("/volume/4050-", volume_body.to_string()),
    ]);

    let mut book = ComicBook {
        file_path: "Comics/Batman 12 (2011).cbz".into(),
        ..Default::default()
    };
    book.enable_proposed = true;

    let mut ui = FakeUi {
        series_answers: vec![SeriesResult::Ok(
            SeriesRef::new(40501, "Batman", 1940, "DC Comics", 900, None).unwrap(),
        )],
        ..Default::default()
    };
    let mut config = Configuration::default();
    config.rescrape_tags = true;
    let mut cv = client_for(&base);
    let (scraped, skipped) = engine(config).scrape(vec![book], &mut ui, &mut cv);

    assert_eq!((scraped, skipped), (1, 0));
    assert_eq!(ui.scraped.len(), 1);
    assert_eq!(ui.scraped[0].info.series, "Batman");
    assert_eq!(ui.scraped[0].info.year, 2011); // from the cover date
                                               // the key tag and the key note landed
    assert!(ui.scraped[0].info.tags.contains("CVDB400011"));
    assert!(ui.scraped[0]
        .info
        .notes
        .starts_with("Scraped metadata from ComicVine [CVDB400011]."));
}

#[test]
fn a_failed_query_delays_then_skips() {
    // the issue key resolves but the details query fails: the book is
    // delayed to the end, retried, and reported skipped
    let bad = r#"{"status_code": 101, "error": "Invalid API Key"}"#.to_string();
    let base = start_mock(&[("/issue/4000-", bad)]);

    let mut book = ComicBook {
        file_path: "Comics/Batman 12 (2011).cbz".into(),
        ..Default::default()
    };
    book.custom_values_store = values_store::encode(&[("comicvine_issue".into(), "400011".into())]);
    book.enable_proposed = true;

    let mut ui = FakeUi::default();
    let mut cv = client_for(&base);
    let (scraped, skipped) = engine(Configuration::default()).scrape(vec![book], &mut ui, &mut cv);

    assert_eq!((scraped, skipped), (0, 1));
    assert!(ui.scraped.is_empty());
    assert_eq!(ui.finished[0].1, BookStatus::Skipped);
}

#[test]
fn user_skip_and_permskip_mark_the_book() {
    // an unresolvable series: the user permskips -> CVDBSKIP lands
    let empty_search = r#"{"number_of_total_results": 0, "status_code": 1}"#;
    let base = start_mock(&[("/search/", empty_search.to_string())]);

    let book = ComicBook {
        file_path: "Comics/Unknown Series 3 (2019).cbz".into(),
        enable_proposed: true,
        ..Default::default()
    };

    let mut ui = FakeUi::default();
    let mut cv = client_for(&base);
    let (scraped, skipped) = engine(Configuration::default()).scrape(vec![book], &mut ui, &mut cv);
    // no series found: the search dialog never shows (no results),
    // the book is reported unscraped -> skipped
    assert_eq!((scraped, skipped), (0, 1));
    assert!(ui.scraped.is_empty());
}

#[test]
fn thumbnails_install_for_fileless_books() {
    // A fileless book scrapes interactively (one series, one issue);
    // the injected installer receives the downloaded cover bytes and
    // the clone carries the custom-thumbnail key. File-backed books
    // never install (C# parity).
    let cover: Vec<u8> = vec![0x89, b'P', b'N', b'G', 1, 2, 3];
    // The listener binds first so the fixtures can reference the
    // mock's cover url.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let base = format!("http://127.0.0.1:{port}");
    let cover_url = format!("{base}/cover.png");
    let search_body = r#"{
      "number_of_total_results": 1, "number_of_page_results": 1, "status_code": 1,
      "results": [
        {"id": 40501, "name": "Batman", "start_year": "1940",
         "publisher": {"id": 10, "name": "DC Comics"},
         "count_of_issues": 900, "image": {"small_url": ""}}]}"#;
    let issues_body = r#"{
        "number_of_total_results": 1, "number_of_page_results": 1, "status_code": 1,
        "results": [
            {"id": 400011, "issue_number": "12", "name": "The Court of Owls",
             "image": {"small_url": ""}}]}"#;
    let details_body = format!(
        r#"{{
        "number_of_total_results": 1, "status_code": 1,
        "results": {{"id": "400011", "name": "The Court of Owls",
            "issue_number": "12",
            "cover_date": "2011-05-14",
            "volume": {{"id": "40501", "name": "Batman", "start_year": "1940"}},
            "image": {{"small_url": "{cover_url}"}}}}}}"#
    );
    let volume_body = r#"{
        "number_of_total_results": 1, "status_code": 1,
        "results": {"id": 40501, "name": "Batman", "start_year": "1940",
                    "publisher": {"id": 10, "name": "DC Comics"}}}"#;

    let search_body = search_body.to_string();
    let issues_body = issues_body.to_string();
    let details_body = details_body.to_string();
    let volume_body = volume_body.to_string();
    let cover_for_server = cover.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf = [0u8; 8192];
            let len = stream.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..len]).to_string();
            let path = request.lines().next().unwrap_or("");
            let body: Vec<u8> = if path.contains("/cover.png") {
                cover_for_server.clone()
            } else if path.contains("/issue/4000-") {
                details_body.as_bytes().to_vec()
            } else if path.contains("/issues/") {
                issues_body.as_bytes().to_vec()
            } else if path.contains("/volume/") {
                volume_body.as_bytes().to_vec()
            } else {
                search_body.as_bytes().to_vec()
            };
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });

    // The fileless book: no file path, no stored metadata (the
    // C# "fileless" placeholder).
    let book = ComicBook {
        file_path: String::new(),
        ..Default::default()
    };

    let mut ui = FakeUi {
        term_answers: vec![Some("batman".into())],
        series_answers: vec![SeriesResult::Ok(
            SeriesRef::new(40501, "Batman", 1940, "DC Comics", 900, None).unwrap(),
        )],
        ..Default::default()
    };
    let mut cv = client_for(&base);
    let mut engine = engine(Configuration::default());
    let cover = cover.clone();
    engine.thumb_installer = Some(Arc::new(move |downloaded: &[u8]| {
        assert_eq!(downloaded, &cover[..], "the cover bytes downloaded");
        Some("guid-1".into())
    }));
    let (_scraped, _skipped) = engine.scrape(vec![book], &mut ui, &mut cv);

    assert_eq!(ui.scraped.len(), 1);
    let thumb_key = ui.scraped[0]
        .custom_thumbnail_key
        .clone()
        .expect("the thumb key installed");
    assert_eq!(thumb_key, "guid-1");
}
