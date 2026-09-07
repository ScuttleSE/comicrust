//! The Phase 8 perf gate: the view-side proposed-parse storms. Three
//! regimes the browser hits on every rebuild:
//!
//! 1. sort — the chained comparer over a `PropTable` (was: a full
//!    ComicNameInfo regex parse per side per comparison);
//! 2. group — the grouper registry pass with the precomputed prop
//!    (was: a parse per book);
//! 3. duplicates — `ComicBookDuplicateMatcher` (was: ~10 parses per
//!    book pair in the O(N²) loop).
//!
//! Sizes are fixed so the numbers stay comparable across runs: sort
//! and group over 5000 books, the duplicate matcher over 1000. The
//! budgets separate the parse-storm regime from the scalar regime.

use cr_core::database::list_items::{ComicBookMatcher, ValueMatcher};
use cr_core::model::comic_book::ComicBook;
use cr_core::model::enums::MatcherMode;
use cr_engine::group::{compare_by_column, groupers};
use cr_engine::matcher::book_view;
use cr_engine::matcher::eval::{match_set, MatchContext};
use cr_engine::matcher::tree::Matcher;
use std::time::{Duration, Instant};

const SORT_BUDGET: Duration = Duration::from_secs(15);
const GROUP_BUDGET: Duration = Duration::from_secs(15);
const DUP_BUDGET: Duration = Duration::from_secs(30);

fn book(i: usize, empty_info: bool) -> ComicBook {
    let s = i % 100;
    let n = (i / 100) % 50 + 1;
    let info = if empty_info {
        cr_core::model::comic_info::ComicInfo::default()
    } else {
        cr_core::model::comic_info::ComicInfo {
            series: format!("Series {:03}", s),
            number: format!("{}", n),
            volume: (s % 3) as i32 + 1,
            year: 1970 + (s % 40) as i32,
            ..Default::default()
        }
    };
    ComicBook {
        file_path: format!("/comics/Series {:03} {:03} (1970).cbz", s, n),
        enable_proposed: true,
        info,
        ..Default::default()
    }
}

fn books(count: usize) -> Vec<ComicBook> {
    (0..count).map(|i| book(i, i % 4 == 0)).collect()
}

/// 50 series × 10 numbers → every (series, number, volume, year,
/// format) combo appears twice: the pair loop has real duplicates to
/// find (the plain `books` set is duplicate-free by construction).
fn dup_books(count: usize) -> Vec<ComicBook> {
    (0..count)
        .map(|i| {
            let s = i % 50;
            let n = (i / 50) % 10 + 1;
            let info = if i % 4 == 0 {
                cr_core::model::comic_info::ComicInfo::default()
            } else {
                cr_core::model::comic_info::ComicInfo {
                    series: format!("Series {:03}", s),
                    number: format!("{}", n),
                    volume: (s % 3) as i32 + 1,
                    year: 1970 + (s % 40) as i32,
                    ..Default::default()
                }
            };
            ComicBook {
                file_path: format!("/comics/Series {:03} {:03} (1970).cbz", s, n),
                enable_proposed: true,
                info,
                ..Default::default()
            }
        })
        .collect()
}

#[test]
fn sort_5000_books_by_series_stays_scalar_fast() {
    let lib = books(5000);
    let props = book_view::PropTable::build(&lib);
    // Pre-warm (the gate's subject is the sort, not the lazy parse).
    for (i, b) in lib.iter().enumerate() {
        props.get(i, b);
    }
    let mut order: Vec<usize> = (0..lib.len()).collect();
    let t = Instant::now();
    order.sort_by(|&x, &y| {
        compare_by_column(
            &lib[x],
            &lib[y],
            "Series",
            Some(props.get(x, &lib[x])),
            Some(props.get(y, &lib[y])),
        )
    });
    let elapsed = t.elapsed();
    eprintln!("sort 5000 by Series: {elapsed:?}");
    assert!(elapsed < SORT_BUDGET, "sort regressed: {elapsed:?}");
}

#[test]
fn group_pass_5000_books_stays_scalar_fast() {
    let lib = books(5000);
    let props = book_view::PropTable::build(&lib);
    let grouper = groupers()
        .iter()
        .find(|(k, _)| *k == "Series")
        .map(|(_, g)| *g)
        .expect("Series grouper");
    let t = Instant::now();
    let count = lib
        .iter()
        .enumerate()
        .map(|(i, b)| grouper(b, props.get(i, b)))
        .filter(|i| !i.caption.is_empty())
        .count();
    let elapsed = t.elapsed();
    eprintln!("group pass 5000 by Series: {elapsed:?} ({count} captioned)");
    assert!(count > 0);
    assert!(elapsed < GROUP_BUDGET, "grouping regressed: {elapsed:?}");
}

#[test]
fn duplicate_matcher_1000_books_stays_scalar_fast() {
    let lib = dup_books(1000);
    let items: Vec<&ComicBook> = lib.iter().collect();
    let raw = ComicBookMatcher::Value(ValueMatcher {
        type_name: "ComicBookDuplicateMatcher".into(),
        match_operator: 0,
        ..Default::default()
    });
    let m = Matcher::from_raw(&raw).expect("bind the duplicate matcher");
    let ctx = MatchContext::new(&items);
    let pairs = [(MatcherMode::And, false, &m)];
    let t = Instant::now();
    let result = match_set(&items, &pairs, &ctx);
    let elapsed = t.elapsed();
    eprintln!(
        "duplicates 1000 books: {elapsed:?} ({} in duplicate groups)",
        result.len()
    );
    assert!(
        !result.is_empty(),
        "the synthetic set must contain duplicates"
    );
    assert!(elapsed < DUP_BUDGET, "duplicates regressed: {elapsed:?}");
}
