//! T10 timing gate: list evaluation at 10k-book scale (the kickoff's
//! "list evaluation on 10k+ synthetic books"). Four shapes:
//! the Library root (all books), a folder tree, a smart list with a
//! Series matcher (the matcher pipeline), and an id list with every
//! book id (the stored-order walk).
//!
//! Budgets are generous on purpose — the gate only has to separate
//! "ms-scale" from a regression that the user would feel.
use cr_core::database::comic_database::ComicDatabase;
use cr_core::database::list_items::{
    ComicBookMatcher, ComicListItem, FolderItem, IdListItem, LibraryListItem, ListItemBase,
    SmartListItem, ValueMatcher,
};
use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::{CrDateTime, CrGuid};
use cr_engine::lists::evaluate_list;
use std::time::{Duration, Instant};

const BOOKS: usize = 10_000;
const BUDGET: Duration = Duration::from_secs(15);

fn make_db() -> ComicDatabase {
    let mut db = ComicDatabase {
        id: CrGuid::new_random(),
        ..Default::default()
    };
    for i in 0..BOOKS {
        let mut b = ComicBook {
            id: CrGuid::new_random(),
            file_path: format!("/comics/book{i:05}.cbz"),
            added_time: CrDateTime::now(),
            ..Default::default()
        };
        b.info.series = format!("Series {:04}", i % 500);
        b.info.number = format!("{}", 1 + i / 500);
        b.info.page_count = 24;
        b.info.writer = "John Writer; Jane Penciller".into();
        db.books.push(b);
    }
    db
}

#[test]
fn library_root_all_books() {
    let db = make_db();
    let library = ComicListItem::Library(LibraryListItem {
        base: ListItemBase {
            id: CrGuid::new_random(),
            name: Some("Library".into()),
            ..Default::default()
        },
    });
    let t = Instant::now();
    let books = evaluate_list(&library, &db);
    let elapsed = t.elapsed();
    eprintln!(
        "Library root x {BOOKS} books: {elapsed:?} ({} books)",
        books.len()
    );
    assert_eq!(books.len(), BOOKS);
    assert!(
        elapsed < BUDGET,
        "Library evaluation regressed: {elapsed:?}"
    );
}

#[test]
fn folder_tree_with_children() {
    let db = make_db();
    let library = ComicListItem::Library(LibraryListItem {
        base: ListItemBase {
            id: CrGuid::new_random(),
            name: Some("Library".into()),
            ..Default::default()
        },
    });
    let smart = ComicListItem::Smart(SmartListItem {
        base: ListItemBase {
            id: CrGuid::new_random(),
            name: Some("Series 0001".into()),
            ..Default::default()
        },
        matchers: vec![ComicBookMatcher::Value(ValueMatcher {
            type_name: "ComicBookSeriesMatcher".into(),
            match_operator: 1, // contains
            match_value: "Series 0001".into(),
            ..Default::default()
        })],
        ..Default::default()
    });
    // Warm the Proposed cache (the C# parses once per book session —
    // this gate targets the folder UNION cost, not the one-time
    // parse class the smart-list test owns).
    let _ = evaluate_list(&smart, &db);
    // A folder of folders (Or), mixing the Library root and the
    // smart list — the union dedup walks every child's set.
    let folder = ComicListItem::Folder(FolderItem {
        base: ListItemBase {
            id: CrGuid::new_random(),
            name: Some("Folder".into()),
            ..Default::default()
        },
        items: vec![library, smart],
        ..Default::default()
    });
    let t = Instant::now();
    let books = evaluate_list(&folder, &db);
    let elapsed = t.elapsed();
    eprintln!(
        "Folder tree x {BOOKS} books: {elapsed:?} ({} books)",
        books.len()
    );
    assert_eq!(books.len(), BOOKS);
    assert!(elapsed < BUDGET, "Folder evaluation regressed: {elapsed:?}");
}

#[test]
fn smart_list_series_matcher() {
    let db = make_db();
    let smart = ComicListItem::Smart(SmartListItem {
        base: ListItemBase {
            id: CrGuid::new_random(),
            name: Some("Series 0001".into()),
            ..Default::default()
        },
        matchers: vec![ComicBookMatcher::Value(ValueMatcher {
            type_name: "ComicBookSeriesMatcher".into(),
            match_operator: 1, // contains
            match_value: "Series 0001".into(),
            ..Default::default()
        })],
        ..Default::default()
    });
    // Stage split for the record (run FIRST — the budget assert below
    // aborts the test when it trips).
    let library: Vec<&ComicBook> = db.books.iter().collect();
    let t = Instant::now();
    let ctx = cr_engine::matcher::eval::MatchContext::new(&library);
    eprintln!("  MatchContext::new: {:?}", t.elapsed());
    let smart_item = match &smart {
        ComicListItem::Smart(s) => s,
        _ => unreachable!(),
    };
    let bound = cr_engine::smart_list::bind_matcher(&smart_item.matchers[0]).expect("binds");
    let t = Instant::now();
    let matched = cr_engine::matcher::eval::match_set(
        &library,
        &[(cr_core::model::enums::MatcherMode::And, false, &bound)],
        &ctx,
    );
    eprintln!(
        "  match_set (cold cache): {:?} ({} books)",
        t.elapsed(),
        matched.len()
    );

    // The C# `Proposed` parity: the FIRST evaluation pays one parse
    // per parse-needy book (a one-time class); every later evaluation
    // is served from the process-wide cache. The gate: the second run
    // is ms-scale.
    let t = Instant::now();
    let first = evaluate_list(&smart, &db);
    eprintln!("Smart list FIRST eval (cold cache): {:?}", t.elapsed());
    assert_eq!(first.len(), BOOKS / 500);

    let t = Instant::now();
    let books = evaluate_list(&smart, &db);
    let elapsed = t.elapsed();
    eprintln!(
        "Smart list (Series contains) re-eval x {BOOKS} books: {elapsed:?} ({} books)",
        books.len()
    );
    assert_eq!(books.len(), BOOKS / 500);
    assert!(
        elapsed < BUDGET,
        "Smart-list re-evaluation regressed: {elapsed:?}"
    );
}

#[test]
fn id_list_every_book() {
    let db = make_db();
    let list = ComicListItem::IdList(IdListItem {
        base: ListItemBase {
            id: CrGuid::new_random(),
            name: Some("All".into()),
            ..Default::default()
        },
        book_ids: db.books.iter().map(|b| b.id).collect(),
    });
    let t = Instant::now();
    let books = evaluate_list(&list, &db);
    let elapsed = t.elapsed();
    eprintln!("Id list ({BOOKS} ids): {elapsed:?} ({} books)", books.len());
    assert_eq!(books.len(), BOOKS);
    assert!(
        elapsed < BUDGET,
        "Id-list evaluation regressed: {elapsed:?}"
    );
}
