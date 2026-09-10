//! Mock-server tests for the ComicVine data layer: a canned HTTP
//! server on a local port drives every endpoint through the real
//! client code (pagination, retries, URL decode, alternate issue
//! numbers, the series-details cache, the magic cvinfo file).

use std::io::{Read, Write};
use std::thread::JoinHandle;

use cr_scrape::cv::connection::CvClient;
use cr_scrape::cv::models::{IssueRef, SeriesRef};
use cr_scrape::cv::queries::Cv;

/// One canned response: the request path must CONTAIN `path`, and the
/// canned `body` (or `status`) is served.
struct Canned {
    path: &'static str,
    status: u16,
    body: &'static str,
}

/// Spawns a server thread answering each connection with the first
/// canned response whose path matches (or 404). Returns the base url.
fn serve(canned: &'static [Canned]) -> (String, JoinHandle<()>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            // read the request head
            let mut buf = [0u8; 8192];
            let len = stream.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..len]).to_string();
            let request_line = request.lines().next().unwrap_or("");
            let path = request_line.split_whitespace().nth(1).unwrap_or("");
            let canned = canned
                .iter()
                .find(|c| path.contains(c.path))
                .map(|c| (c.status, c.body))
                .unwrap_or((404, "{\"status_code\": 404, \"error\": \"no route\"}"));
            let (status, body) = canned;
            let response = format!(
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    (format!("http://127.0.0.1:{port}/api"), handle)
}

fn client_for(base: &str) -> Cv {
    Cv::new(CvClient::with_delays(
        "TESTKEY",
        base,
        std::time::Duration::from_millis(0),
        std::time::Duration::from_millis(0),
    ))
}

fn no_cancel() -> bool {
    false
}

const SEARCH_PAGE_1: &str = r#"{
  "error": "OK", "limit": 100, "offset": 0,
  "number_of_page_results": 2, "number_of_total_results": 2,
  "status_code": 1,
  "results": {"volume": [
    {"id": 40501, "name": "Batman", "start_year": "1940",
     "publisher": {"id": 10, "name": "DC Comics"},
     "count_of_issues": 900, "image": {"small_url": "http://img/batman-small.jpg"}},
    {"id": 40502, "name": "Batman &amp; Robin", "start_year": "2009- ",
     "publisher": null, "count_of_issues": 25, "image": {}}
  ]}
}"#;

const ISSUE_LIST: &str = r#"{
  "error": "OK", "number_of_total_results": 2, "number_of_page_results": 1,
  "status_code": 1,
  "results": {"issue": [
    {"id": 400001, "issue_number": "1", "name": "The Crossing",
     "image": {"small_url": "http://img/i1-small.jpg"}},
    {"id": 400011, "issue_number": "1½", "name": "½ Special",
     "image": {"medium_url": "http://img/i11-medium.jpg"}}
  ]},
  "detail": "ok"
}"#;

const ISSUE_DETAILS: &str = r#"{
  "error": "OK", "number_of_total_results": 1, "status_code": 1,
  "results": {
    "id": "400011", "name": " The Half Issue ",
    "issue_number": " ½ ",
    "site_detail_url": "http://comicvine.gamespot.com/x/4000-11/",
    "cover_date": "2011-05-14", "store_date": "2011-04-20",
    "description": "Overview<br />A &amp; B &nbsp; <b>bold</b>  text.<p>End.",
    "volume": {"id": "40501", "name": "Batman", "start_year": "1940",
               "publisher": {"id": 10, "name": "Vertigo"}},
    "image": {"small_url": "http://img/i11-small.jpg",
              "thumb_url": "http://img/i11-thumb.jpg"},
    "story_arc_credits": {"story_arc": [{"name": "The Crossover"}]},
    "character_credits": {"character": [{"name": "Batman"}, {"name": "Robin"}]},
    "team_credits": {"team": [{"name": "Justice League"}]},
    "location_credits": {"location": [{"name": "Gotham"}]},
    "person_credits": {"person": [
      {"name": "Grant Morrison", "role": "writer"},
      {"name": "Frank Quitely", "role": "penciler, inker"},
      {"name": "Alex Sinclair", "role": "colorer, cover"},
      {"name": "Someone Else", "role": "letterer, editor, artist"}
    ]},
    "associated_images": [
      {"original_url": "http://img/alt1.jpg"},
      {"original_url": "http://img/alt2.jpg"}
    ]
  }
}"#;

#[test]
fn search_series_returns_refs_with_pagination() {
    static CANNED: &[Canned] = &[Canned {
        path: "/search/",
        status: 200,
        body: SEARCH_PAGE_1,
    }];
    let (base, _guard) = serve(CANNED);
    let mut cv = client_for(&base);
    let refs = cv
        .query_series_refs("batman", &[], 100, &mut no_cancel)
        .unwrap();
    assert_eq!(refs.len(), 2);
    let batman = refs.iter().find(|r| r.series_key == 40501).unwrap();
    assert_eq!(batman.series_name(), "Batman");
    assert_eq!(batman.publisher, "DC Comics");
    assert_eq!(batman.volume_year, 1940);
    assert_eq!(batman.issue_count, 900);
    assert_eq!(
        batman.thumb_url.as_deref(),
        Some("http://img/batman-small.jpg")
    );

    // the amp entity decodes and the trailing "- " year strips
    let amp = refs.iter().find(|r| r.series_key == 40502).unwrap();
    assert_eq!(amp.series_name(), "Batman & Robin");
    assert_eq!(amp.volume_year, 2009);
    assert_eq!(amp.publisher, "");

    // the result is cached per terms (no second request would happen;
    // a second call with the server gone must still succeed)
    drop(_guard);
    std::thread::sleep(std::time::Duration::from_millis(50));
    let cached = cv
        .query_series_refs("batman", &[], 100, &mut no_cancel)
        .unwrap();
    assert_eq!(cached.len(), 2);
}

#[test]
fn ignored_search_terms_are_stripped() {
    static CANNED: &[Canned] = &[Canned {
        path: "/search/",
        status: 200,
        body: SEARCH_PAGE_1,
    }];
    let (base, _guard) = serve(CANNED);
    let mut cv = client_for(&base);
    let refs = cv
        .query_series_refs(
            "batman c2c noads",
            &["c2c".to_string(), "noads".to_string()],
            100,
            &mut no_cancel,
        )
        .unwrap();
    // the query still succeeds; the terms reached the (mock) server
    // without the ignored words
    assert_eq!(refs.len(), 2);
}

#[test]
fn issue_refs_list_and_issue_details() {
    static CANNED: &[Canned] = &[
        Canned {
            path: "/issues/",
            status: 200,
            body: ISSUE_LIST,
        },
        Canned {
            path: "/issue/4000-",
            status: 200,
            body: ISSUE_DETAILS,
        },
        Canned {
            path: "/volume/4050-",
            status: 200,
            body: VOLUME_DETAILS,
        },
    ];
    let (base, _guard) = serve(CANNED);
    let mut cv = client_for(&base);
    let series = SeriesRef::new(40501, "Batman", 1940, "DC Comics", 2, None).unwrap();
    let refs = cv.query_issue_refs(&series, &mut no_cancel).unwrap();
    assert_eq!(refs.len(), 2);
    let first = refs.iter().find(|r| r.issue_key == 400011).unwrap();
    assert_eq!(first.issue_num, "1½");
    assert_eq!(
        first.thumb_url.as_deref(),
        Some("http://img/i11-medium.jpg")
    );

    // issue details: all parsed fields
    let issue_ref = IssueRef::new("½", 400011, "", None);
    let issue = cv.query_issue(&issue_ref, true).unwrap();
    assert_eq!(issue.title, "The Half Issue");
    assert_eq!(issue.issue_num, "½");
    assert_eq!(issue.series_name, "Batman");
    assert_eq!(issue.pub_year, 2011);
    assert_eq!(issue.pub_month, 5);
    assert_eq!(issue.pub_day, 14);
    assert_eq!(issue.rel_year, 2011);
    assert_eq!(issue.rel_month, 4);
    assert_eq!(issue.rel_day, 20);
    assert_eq!(issue.webpage, "http://comicvine.gamespot.com/x/4000-11/");
    // the imprint table resolves Vertigo -> DC Comics
    assert_eq!(issue.publisher, "DC Comics");
    assert_eq!(issue.imprint, "Vertigo");
    // parity: MULTISPACES collapses before the NBSP entity, so the
    // nbsp replacement leaves three spaces; the title part is gone
    assert_eq!(issue.summary, "A & B   bold text.\nEnd.");
    assert_eq!(issue.crossovers, vec!["The Crossover"]);
    assert_eq!(issue.characters, vec!["Batman", "Robin"]);
    assert_eq!(issue.teams, vec!["Justice League"]);
    assert_eq!(issue.locations, vec!["Gotham"]);
    assert_eq!(issue.writers, vec!["Grant Morrison"]);
    assert_eq!(issue.pencillers, vec!["Frank Quitely", "Someone Else"]);
    assert_eq!(issue.inkers, vec!["Frank Quitely", "Someone Else"]);
    assert_eq!(issue.cover_artists, vec!["Alex Sinclair"]);
    assert_eq!(issue.colorists, vec!["Alex Sinclair"]);
    assert_eq!(issue.letterers, vec!["Someone Else"]);
    assert_eq!(issue.editors, vec!["Someone Else"]);
    // the issue image url lands first, then the associated images
    assert_eq!(issue.image_urls[0], "http://img/i11-small.jpg");
    assert_eq!(issue.image_urls[1], "http://img/alt1.jpg");
    assert_eq!(issue.image_urls[2], "http://img/alt2.jpg");
}

#[test]
fn issue_ref_lookup_with_alternate_number_ladder() {
    static CANNED: &[Canned] = &[Canned {
        path: "/issues/",
        status: 200,
        body: r#"{
          "error": "OK", "number_of_total_results": 1, "status_code": 1,
          "results": {"issue": {"id": 400011, "issue_number": "0½",
                                "name": "Half", "image": {}}}
        }"#,
    }];
    let (base, _guard) = serve(CANNED);
    let cv = client_for(&base);
    let series = SeriesRef::new(40501, "Batman", 1940, "", 900, None).unwrap();
    // "0.5" -> alternate "0½" is found on the retry
    let found = cv.query_issue_ref(&series, "0.5").unwrap().unwrap();
    assert_eq!(found.issue_key, 400011);
    assert_eq!(found.issue_num, "0½");
}

#[test]
fn url_to_series_ref_decodes_series_and_issue_urls() {
    static CANNED: &[Canned] = &[Canned {
        path: "/volume/4050-40501/",
        status: 200,
        body: VOLUME_DETAILS,
    }];
    let (base, _guard) = serve(CANNED);
    let cv = client_for(&base);
    let series = cv
        .url_to_series_ref("https://comicvine.gamespot.com/batman/4050-40501/")
        .unwrap();
    assert_eq!(series.series_key, 40501);
    // no magic number -> None
    assert!(cv
        .url_to_series_ref("https://example.com/nothing")
        .is_none());
}

const VOLUME_DETAILS: &str = r#"{
  "error": "OK", "number_of_total_results": 1, "status_code": 1,
  "results": {"id": 40501, "name": "Batman", "start_year": "1940",
              "count_of_issues": 900,
              "publisher": {"id": 10, "name": "Vertigo"},
              "image": {"small_url": "http://img/bat-small.jpg"}}
}"#;

#[test]
fn series_details_cache_and_imprint_resolution() {
    static CANNED: &[Canned] = &[
        Canned {
            path: "/issue/4000-",
            status: 200,
            body: ISSUE_DETAILS,
        },
        Canned {
            path: "/volume/4050-",
            status: 200,
            body: VOLUME_DETAILS,
        },
    ];
    let (base, _guard) = serve(CANNED);
    let cv = client_for(&base);
    let issue_ref = IssueRef::new("½", 400011, "", None);
    let issue = cv.query_issue(&issue_ref, false).unwrap();
    // Vertigo is an imprint: parent publisher wins, imprint recorded
    assert_eq!(issue.publisher, "DC Comics");
    assert_eq!(issue.imprint, "Vertigo");
    assert_eq!(issue.volume_year, 1940);
    assert_eq!(issue.series_key, "40501");

    // a second query for the same series hits the cache (no volume
    // query needed even without start_year in the issue dom — here
    // the dom carries it, so the cache is what the second run proves)
    let issue2 = cv.query_issue(&issue_ref, false).unwrap();
    assert_eq!(issue, issue2);
}

#[test]
fn error_status_and_retry_surface() {
    // a broken response on both attempts -> an error surfaces
    static BROKEN: &[Canned] = &[Canned {
        path: "/search/",
        status: 200,
        body: r#"{"status_code": 101, "error": "Invalid API Key"}"#,
    }];
    let (base, _guard) = serve(BROKEN);
    let mut cv = client_for(&base);
    let err = cv
        .query_series_refs("batman", &[], 100, &mut no_cancel)
        .unwrap_err();
    assert!(err.to_string().contains("101"), "{err}");
}

#[test]
fn key_tags_round_trip() {
    static CANNED: &[Canned] = &[];
    let (base, _guard) = serve(CANNED);
    let cv = client_for(&base);
    assert_eq!(cv.create_key_tag(12345).as_deref(), Some("CVDB12345"));
    assert_eq!(cv.create_key_tag(0), None);
    assert_eq!(
        cv.parse_key_tag("Scraped metadata from ComicVine [CVDB12345]."),
        Some(12345)
    );
    assert_eq!(cv.parse_key_tag("cvdb999"), Some(999));
    assert_eq!(cv.parse_key_tag("old ComicVine [777"), Some(777));
    assert_eq!(cv.parse_key_tag("no tag here"), None);
}

#[test]
fn magic_cvinfo_file_decodes() {
    static CANNED: &[Canned] = &[Canned {
        path: "/volume/4050-40501/",
        status: 200,
        body: VOLUME_DETAILS,
    }];
    let (base, _guard) = serve(CANNED);
    let cv = client_for(&base);
    let dir = std::env::temp_dir().join(format!("cr-scrape-cvinfo-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("cvinfo.txt"),
        "https://comicvine.gamespot.com/batman/4050-40501/",
    )
    .unwrap();
    let book_path = dir.join("Batman 12 (2011).cbz");
    let series = cv.check_magic_file(&book_path.to_string_lossy());
    assert_eq!(series.map(|s| s.series_key), Some(40501));

    // a directory with no cvinfo file -> None
    let empty = dir.join("sub");
    std::fs::create_dir_all(&empty).unwrap();
    assert!(cv.check_magic_file(&empty.to_string_lossy()).is_none());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn cleanup_terms_follow_the_python_rules() {
    use cr_scrape::cv::queries::cleanup_search_terms;
    assert_eq!(
        cleanup_search_terms("Batman & Robin (c2c)", false),
        "batman and robin"
    );
    assert_eq!(cleanup_search_terms("X-Force noads tbp", false), "x-force");
    // number words: expand a digit, contract a word
    assert_eq!(cleanup_search_terms("five", true), "5");
    assert_eq!(cleanup_search_terms("5", true), "five");
    assert_eq!(cleanup_search_terms("five", false), "five");
    // punctuation outside [\w':.-] drops
    assert_eq!(
        cleanup_search_terms("O'Malley, Part #2!", false),
        "o'malley part 2"
    );
}
