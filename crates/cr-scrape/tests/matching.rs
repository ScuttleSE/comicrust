//! The T4 gate: the cover hash, the back-cover strip, the match
//! score, the series filters, and the automatcher end to end over a
//! mock server.

use std::collections::HashSet;
use std::io::{Read, Write};

use cr_core::model::ComicBook;
use cr_image::Image;
use cr_scrape::bookdata::BookData;
use cr_scrape::config::Configuration;
use cr_scrape::cv::connection::CvClient;
use cr_scrape::cv::models::SeriesRef;
use cr_scrape::cv::queries::Cv;
use cr_scrape::matching::automatcher::find_series_ref;
use cr_scrape::matching::imagehash::{hash, similarity};
use cr_scrape::matching::matchscore::MatchScore;
use cr_scrape::matching::{filter_series_refs, strip_back_cover};

// ==========================================================================
// imagehash

#[test]
fn hash_is_stable_for_identical_images() {
    let a = gradient_image(64, 64, |_, y| if y < 32 { 220 } else { 30 });
    let h1 = hash(&a).unwrap();
    assert_eq!(similarity(Some(h1), Some(h1)), 1.0);

    // the inverted pattern is a different hash
    let inverted = gradient_image(64, 64, |_, y| if y < 32 { 30 } else { 220 });
    let h2 = hash(&inverted).unwrap();
    assert!(similarity(Some(h1), Some(h2)) < 1.0);
}

#[test]
fn a_missing_hash_matches_nothing() {
    let h = hash(&gradient_image(8, 8, |_, _| 128)).unwrap();
    assert_eq!(similarity(None, Some(h)), 0.0);
    assert_eq!(similarity(Some(h), None), 0.0);
}

fn gradient_image(w: u32, h: u32, f: impl Fn(u32, u32) -> u8) -> Image {
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let g = f(x, y);
            rgba.extend_from_slice(&[g, g, g, 255]);
        }
    }
    Image::new(w, h, rgba).unwrap()
}

// ==========================================================================
// strip_back_cover

#[test]
fn two_page_ratio_crops_the_right_half() {
    let image = gradient_image(100, 80, |_, _| 90);
    let stripped = strip_back_cover(&image);
    assert_eq!((stripped.width, stripped.height), (50, 80));
    // the kept half is the RIGHT one
    assert_eq!(&stripped.rgba[0..4], &image.rgba[50 * 4..50 * 4 + 4]);

    // a single-page ratio is untouched
    let single = gradient_image(80, 80, |_, _| 10);
    let same = strip_back_cover(&single);
    assert_eq!((same.width, same.height), (80, 80));
}

// ==========================================================================
// matchscore

fn book(filename: &str) -> BookData {
    let book = ComicBook {
        file_path: format!("Comics/{filename}"),
        ..Default::default()
    };
    BookData::from_book(&book, &Configuration::default())
}

#[test]
fn matchscore_hand_traced_totals() {
    let score = MatchScore::new(HashSet::new());
    let book = book("Batman 5 (2011).cbz");

    // Batman (1940, DC Comics, 900 issues): namescore +5, bookscore
    // 100 (count > 100), yearscore 0, recency -(2026-1940)/100
    let series = SeriesRef::new(1, "Batman", 1940, "DC Comics", 900, None).unwrap();
    let expected = 100.0 + 5.0 - ((2026 - 1940) as f64) / 100.0;
    assert!((score.compute(&book, &series, 2026) - expected).abs() < 1e-9);

    // a series that started after the book was published: -500
    let late = SeriesRef::new(2, "Batman", 2015, "DC Comics", 900, None).unwrap();
    let expected = 100.0 + 5.0 - 500.0 - ((2026 - 2015) as f64) / 100.0;
    assert!((score.compute(&book, &late, 2026) - expected).abs() < 1e-9);

    // a mirror publisher is penalized
    let mirror = SeriesRef::new(3, "Batman", 1940, "Panini Comics", 900, None).unwrap();
    let expected = 100.0 + 5.0 - 6.0 - ((2026 - 1940) as f64) / 100.0;
    assert!((score.compute(&book, &mirror, 2026) - expected).abs() < 1e-9);
}

#[test]
fn matchscore_prior_series_boost() {
    let mut prior = HashSet::new();
    prior.insert("1".to_string());
    let score = MatchScore::new(prior);
    let book = book("Batman 5 (2011).cbz");
    let series = SeriesRef::new(1, "Batman", 1940, "DC Comics", 900, None).unwrap();

    let with_prior = score.compute(&book, &series, 2026);
    let without = MatchScore::new(HashSet::new()).compute(&book, &series, 2026);
    assert!((with_prior - without - 7.0).abs() < 1e-9);
}

#[test]
fn filter_series_refs_by_publisher_year_and_threshold() {
    let mk = |key: i64, publisher: &str, year: i32, count: i32| {
        SeriesRef::new(key, "S", year, publisher, count, None).unwrap()
    };
    let ignored: std::collections::BTreeSet<String> =
        ["marvel italia".to_string()].into_iter().collect();
    let refs = vec![
        mk(1, "DC Comics", 1940, 10),
        mk(2, "Marvel Italia", 1940, 10),
        mk(3, "DC Comics", 1800, 10),  // starts before 1900
        mk(4, "DC Comics", 1950, 500), // never filtered (count >= 500)
    ];
    let kept = filter_series_refs(refs, &ignored, 1900, 2026, 500);
    let keys: Vec<i64> = kept.iter().map(|r| r.series_key).collect();
    assert_eq!(keys, vec![1, 4]);
}

// ==========================================================================
// the automatcher end to end


fn start_mock(series_count: usize) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let base = format!("http://127.0.0.1:{port}");
    let cover_url = format!("{base}/cover.png");

    // the search result: `series_count` volumes, all with the same cover
    let mut volumes = Vec::new();
    for i in 0..series_count {
        volumes.push(format!(
            r#"{{"id": {}, "name": "Batman{}", "start_year": "1940",
               "publisher": {{"id": 10, "name": "DC Comics"}},
               "count_of_issues": 900, "image": {{"small_url": "{}"}}}}"#,
            40501 + i,
            i,
            cover_url
        ));
    }
    let search_body = format!(
        r#"{{"number_of_total_results": {count}, "number_of_page_results": {count}, "status_code": 1,
            "results": {{"volume": [{vol}]}}}}"#,
        count = series_count,
        vol = volumes.join(",")
    );
    let issues_body = format!(
        r#"{{"number_of_total_results": 1, "number_of_page_results": 1, "status_code": 1,
            "results": {{"issue": [{{"id": 400011, "issue_number": "5",
                                     "name": "Fifth", "image": {{"small_url": "{url}"}}}}]}}}}"#,
        url = cover_url
    );
    let details_body = format!(
        r#"{{"number_of_total_results": 1, "status_code": 1,
            "results": {{"id": "400011", "name": "Fifth", "issue_number": "5",
                "volume": {{"id": "40501", "name": "Batman", "start_year": "1940"}},
                "image": {{"small_url": "{url}"}}}}}}"#,
        url = cover_url
    );
    let png = cover_png();

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf = [0u8; 8192];
            let len = stream.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..len]).to_string();
            let path = request.lines().next().unwrap_or("");
            let body: Vec<u8> = if path.contains("/cover") {
                png.clone()
            } else if path.contains("/issue/4000-") {
                details_body.as_bytes().to_vec()
            } else if path.contains("/issues/") {
                issues_body.as_bytes().to_vec()
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
    base
}

fn mock_client(base: &str) -> Cv {
    Cv::new(CvClient::with_delays(
        "TEST",
        base,
        std::time::Duration::from_millis(0),
        std::time::Duration::from_millis(0),
    ))
}

fn cover_png() -> Vec<u8> {
    let mut img = image::GrayImage::new(8, 8);
    for y in 0..8u32 {
        for x in 0..8u32 {
            img.put_pixel(
                x,
                y,
                if y < 4 {
                    image::Luma([220])
                } else {
                    image::Luma([30])
                },
            );
        }
    }
    let dyn_img = image::DynamicImage::ImageLuma8(img);
    let mut out = Vec::new();
    dyn_img
        .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
        .unwrap();
    out
}

#[test]
fn automatcher_identifies_a_non_first_issue() {
    let base = start_mock(1);
    let mut cv = mock_client(&base);
    let page0 = cr_image::decode(&cover_png()).unwrap();
    let book = book("Batman 5 (2011).cbz");

    let found = find_series_ref(
        &book,
        &Configuration::default(),
        &mut cv,
        &MatchScore::new(HashSet::new()),
        2026,
        Some(&page0),
        &mut || false,
    )
    .unwrap();
    assert!(found.is_some(), "the automatcher must identify the series");
    assert_eq!(found.unwrap().series_key, 40501);
}

#[test]
fn automatcher_bails_on_nearly_identical_first_issue_covers() {
    let base = start_mock(2);
    let mut cv = mock_client(&base);
    let page0 = cr_image::decode(&cover_png()).unwrap();
    let book = book("Batman 1 (2011).cbz");

    let found = find_series_ref(
        &book,
        &Configuration::default(),
        &mut cv,
        &MatchScore::new(HashSet::new()),
        2026,
        Some(&page0),
        &mut || false,
    )
    .unwrap();
    assert!(
        found.is_none(),
        "near-identical first-issue covers must bail"
    );
}

#[test]
fn automatcher_rejects_a_dissimilar_cover() {
    let base = start_mock(1);
    let mut cv = mock_client(&base);
    // a book whose cover does NOT match the remote art (all mid-gray
    // vs the half-black cover)
    let page0 = gradient_image(64, 64, |_, _| 128);
    let book = book("Batman 5 (2011).cbz");

    let found = find_series_ref(
        &book,
        &Configuration::default(),
        &mut cv,
        &MatchScore::new(HashSet::new()),
        2026,
        Some(&page0),
        &mut || false,
    )
    .unwrap();
    assert!(found.is_none(), "a dissimilar cover must not match");
}
