//! Phase 19 T2 timing gate: the Missing Issues gap pass at a scale
//! comparable to the 21,599-book real library named in
//! `docs/current-status.md` (the O(S x N) idle-CPU incident this
//! phase's locked decision 5 exists to avoid repeating).
//!
//! This is a SYNTHETIC benchmark, not a real-library measurement — no
//! CIFS-scale library is available in this environment. It exists to
//! catch a reintroduced per-series full-library scan (which would turn
//! this from milliseconds into minutes), not to state the real-library
//! wall-clock time as fact.

use cr_core::model::comic_book::ComicBook;
use cr_scrape::cache::missing::missing_issues_of_library;
use cr_scrape::cache::{CvCache, IssueSkeleton, SqliteCache};
use std::time::{Duration, Instant};

const BOOKS: usize = 22_000;
const SERIES: i64 = 2_000;
const ISSUES_PER_VOLUME: i64 = 60;
const BUDGET: Duration = Duration::from_secs(10);

fn cache_and_books() -> (SqliteCache, Vec<ComicBook>) {
    let cache = SqliteCache::in_memory().expect("in-memory cache opens");
    let mut issues = Vec::new();
    for volume_id in 1..=SERIES {
        for number in 1..=ISSUES_PER_VOLUME {
            issues.push(IssueSkeleton {
                issue_id: volume_id * 1000 + number,
                volume_id,
                issue_number: number.to_string(),
                ..Default::default()
            });
        }
    }
    cache.put_issues(&issues).expect("seed issue skeletons");

    let mut books = Vec::with_capacity(BOOKS);
    for i in 0..BOOKS {
        let volume_id = 1 + (i as i64 % SERIES);
        // Every third issue of the volume is missing from the library,
        // so the pass has real gap rows to build, not just misses.
        let owned_number = 1 + (i as i64 % ISSUES_PER_VOLUME);
        let mut book = ComicBook {
            id: cr_core::xml::scalar::CrGuid::new_random(),
            file_path: format!("/comics/book{i:05}.cbz"),
            ..Default::default()
        };
        book.info.series = format!("Series {volume_id:04}");
        book.info.volume = 2020;
        book.info.number = owned_number.to_string();
        cr_scrape::bookdata::set_custom_value(
            &mut book,
            "comicvine_volume",
            &volume_id.to_string(),
        );
        books.push(book);
    }
    (cache, books)
}

#[test]
fn a_full_library_pass_completes_well_inside_budget() {
    let (cache, books) = cache_and_books();
    let t = Instant::now();
    let gaps = missing_issues_of_library(&cache, &books, &books);
    let elapsed = t.elapsed();
    eprintln!(
        "Missing Issues gap pass x {BOOKS} books / {SERIES} series: {elapsed:?} ({} series with gaps)",
        gaps.len()
    );
    assert!(!gaps.is_empty(), "the synthetic fixture must produce gaps");
    assert!(
        elapsed < BUDGET,
        "gap pass took {elapsed:?}, over the {BUDGET:?} budget — check for a \
         reintroduced per-series full-library scan (locked decision 5)"
    );
}
