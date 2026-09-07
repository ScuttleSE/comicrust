//! The T3 timing gate (Phase 8): the large-CBL import must stay
//! seconds-fast against a 2500-book library.
//!
//! Shape: 2000 metadata books (series/number/year/volume set — the
//! shadow values come straight from the info) + 500 proposed books
//! (empty info, `enable_proposed` on — the shadow values come from the
//! file-name parse). The items mix exact matches, year/volume-narrowed
//! matches, parse-path matches, and 500 unsolved items that walk the
//! full three-pass relaxation ladder.
//!
//! The chronology `.cbl` (2886 items, git-ignored `tests/testfiles/`)
//! runs against the same synthetic library when the file exists
//! locally; CI skips that part.

use cr_core::database::reading_list::{ReadingListContainer, ReadingListItem};
use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::{CrDateTime, CrGuid};
use cr_engine::reading_list::create_from_reading_list;
use std::time::{Duration, Instant};

/// Ceiling for the whole gate (both runs). The pre-fix storm was
/// minutes; the fixed import is ~1 s (debug). Generous on purpose —
/// the gate only has to separate the two regimes.
const BUDGET: Duration = Duration::from_secs(30);

fn metadata_book(i: usize) -> ComicBook {
    let s = i % 50;
    let k = i / 50;
    let n = k % 20 + 1;
    ComicBook {
        id: CrGuid::new_random(),
        file_path: format!("/comics/Series {:03} {:03} (1970).cbz", s, n),
        info: cr_core::model::comic_info::ComicInfo {
            series: format!("Series {:03}", s),
            number: format!("{}", n),
            volume: (k % 3) as i32 + 1,
            year: 1970 + (k % 40) as i32,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn proposed_book(i: usize) -> ComicBook {
    ComicBook {
        id: CrGuid::new_random(),
        file_path: format!("/comics/Parsed Hero {:03} (1980).cbz", i + 1),
        enable_proposed: true,
        added_time: CrDateTime::min_value(),
        ..Default::default()
    }
}

fn library() -> Vec<ComicBook> {
    (0..2000)
        .map(metadata_book)
        .chain((0..500).map(proposed_book))
        .collect()
}

/// A metadata item: even j carries a year (the year ±1 narrowing
/// pins the candidate), odd j carries a volume instead (the year
/// narrowing stays off, the volume narrowing pins it).
fn metadata_item(j: usize) -> ReadingListItem {
    let s = j % 50;
    let n = j / 50 % 20 + 1;
    ReadingListItem {
        series: format!("Series {:03}", s),
        number: format!("{}", n),
        volume: if j.is_multiple_of(2) {
            -1
        } else {
            ((n - 1) % 3) as i32 + 1
        },
        year: if j.is_multiple_of(2) {
            1970 + (n - 1) as i32
        } else {
            -1
        },
        ..Default::default()
    }
}

fn proposed_item(k: usize) -> ReadingListItem {
    ReadingListItem {
        series: "Parsed Hero".into(),
        number: format!("{}", k + 1),
        volume: -1,
        year: 1980,
        ..Default::default()
    }
}

fn missing_item(j: usize) -> ReadingListItem {
    ReadingListItem {
        series: format!("Missing Series {:03}", j),
        number: "1".into(),
        volume: -1,
        year: -1,
        ..Default::default()
    }
}

fn items() -> Vec<ReadingListItem> {
    (0..1600)
        .map(metadata_item)
        .chain((0..400).map(proposed_item))
        .chain((0..500).map(missing_item))
        .collect()
}

#[test]
fn import_2500_items_against_2500_books_stays_seconds_fast() {
    let lib = library();
    let items = items();

    let t0 = Instant::now();
    let m = create_from_reading_list(&lib, &items);
    let elapsed = t0.elapsed();
    eprintln!("synthetic 2500x2500 import: {elapsed:?}");

    assert_eq!(m.book_ids.len(), 2500);
    // The 500 unsolved items become placeholders.
    assert_eq!(m.new_books.len(), 500);
    assert!(
        elapsed < BUDGET,
        "the CBL import regressed: {elapsed:?} (budget {BUDGET:?})"
    );

    // The chronology fixture (2886 items) against the same synthetic
    // library — runs only where the git-ignored file exists.
    let cbl = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/testfiles/[Spider-Man] 00 - Complete 616 Chronology.cbl");
    if let Ok(bytes) = std::fs::read(&cbl) {
        let container = ReadingListContainer::parse(&bytes).expect("parse the real .cbl");
        let t1 = Instant::now();
        let m2 = create_from_reading_list(&lib, &container.items);
        let elapsed2 = t1.elapsed();
        eprintln!(
            "chronology .cbl ({} items) x 2500 books: {elapsed2:?} ({} placeholders)",
            container.items.len(),
            m2.new_books.len()
        );
        assert_eq!(m2.book_ids.len(), container.items.len());
        assert!(
            elapsed2 < BUDGET,
            "the real-CBL import regressed: {elapsed2:?} (budget {BUDGET:?})"
        );
    } else {
        eprintln!("chronology .cbl not present — the real-file gate is skipped");
    }
}

/// The exact user-reported scenario: the 2886-item chronology `.cbl`
/// against the real-world 255-book library (both git-ignored fixture
/// files; the test skips when either is missing).
#[test]
fn import_real_chronology_cbl_against_the_real_world_library_stays_fast() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cbl = root.join("tests/testfiles/[Spider-Man] 00 - Complete 616 Chronology.cbl");
    let db = root.join("tests/realworld/ComicDb.xml");
    let (Ok(bytes), Ok(_)) = (std::fs::read(&cbl), std::fs::read(&db)) else {
        eprintln!("fixtures not present — the real-world gate is skipped");
        return;
    };
    let container = ReadingListContainer::parse(&bytes).expect("parse the real .cbl");
    let db =
        cr_core::database::comic_database::load(root.join("tests/realworld/ComicDb.xml").as_path())
            .expect("load the real-world DB");
    let books = &db.books;

    let t0 = Instant::now();
    let m = create_from_reading_list(books, &container.items);
    let elapsed = t0.elapsed();
    eprintln!(
        "chronology .cbl ({} items) x {} books: {elapsed:?} ({} placeholders)",
        container.items.len(),
        books.len(),
        m.new_books.len()
    );
    assert_eq!(m.book_ids.len(), container.items.len());
    assert!(
        elapsed < BUDGET,
        "the real-CBL import regressed: {elapsed:?} (budget {BUDGET:?})"
    );
}
