//! T1 acceptance (Phase 4): the library session lifecycle.
//!
//! - A fresh database carries the C# default list tree
//!   (`ComicDatabase.CreateNew` on the fallback path).
//! - The real-world DB: open → unmutated re-save is byte-identical;
//!   a scan adds books; reading-state mutations round-trip through a
//!   save; all other books stay byte-identical; the "Never Read"
//!   ground truth flips as the user reads.
//! - Watch events map back to watch roots and drive a rescan.

use cr_core::database::comic_database::load;
use cr_core::database::comic_database::ComicDatabase;
use cr_core::database::list_items::{ComicListItem, SmartListItem};
use cr_engine::library::Library;
use cr_engine::scanner::refresh_file_info;
use cr_engine::smart_list::evaluate_smart_list;

const REALWORLD_DB: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/realworld/ComicDb.xml"
);

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "comicrust-library-{name}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn smart_lists(db: &ComicDatabase) -> Vec<&SmartListItem> {
    fn walk<'a>(items: &'a [ComicListItem], out: &mut Vec<&'a SmartListItem>) {
        for item in items {
            match item {
                ComicListItem::Smart(s) => out.push(s),
                ComicListItem::Folder(f) => walk(&f.items, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&db.comic_lists, &mut out);
    out
}

/// Evaluates one saved list by name and returns the matched books'
/// file paths (owned — the call sites assert counts and paths).
fn evaluate(db: &ComicDatabase, name: &str) -> Vec<String> {
    let list = smart_lists(db)
        .into_iter()
        .find(|l| l.base.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("list {name:?} missing"));
    let books: Vec<&cr_core::model::comic_book::ComicBook> = db.books.iter().collect();
    evaluate_smart_list(list, &books, None)
        .into_iter()
        .map(|b| b.file_path.clone())
        .collect()
}

#[test]
fn fresh_database_has_the_default_list_tree() {
    let dir = temp_dir("fresh");
    let file = dir.join("ComicDb.xml");
    let (mut lib, status) = Library::open(&file).expect("open fresh");
    assert_eq!(status, cr_core::database::OpenStatus::FreshEmpty);
    // The C# fallback uses `ComicDatabase.CreateNew()` — the default
    // list tree, not a bare database.
    assert!(
        !lib.database().comic_lists.is_empty(),
        "fresh DB must carry the default list tree"
    );
    // And it saves back with that tree.
    lib.save().expect("save fresh");
    let reloaded = load(&file).expect("reload fresh");
    assert_eq!(reloaded.comic_lists.len(), lib.database().comic_lists.len());
}

#[test]
fn library_session_lifecycle_round_trips_reading_state() {
    let dir = temp_dir("session");
    let file = dir.join("ComicDb.xml");
    std::fs::copy(REALWORLD_DB, &file).unwrap();

    // 1. Open: the fixture loads as-is.
    let (mut lib, status) = Library::open(&file).expect("open real-world DB");
    assert_eq!(status, cr_core::database::OpenStatus::Loaded);
    assert_eq!(lib.database().books.len(), 255);

    // 2. Unmutated re-save is byte-identical (the save gate).
    let fixture_bytes = std::fs::read(REALWORLD_DB).unwrap();
    lib.save().expect("save unmutated");
    let saved = std::fs::read(&file).unwrap();
    assert_eq!(
        saved, fixture_bytes,
        "unmutated save must be byte-identical"
    );

    // 3. "Never Read" = all books before any reading (the Phase 2
    // ground truth).
    let before = evaluate(lib.database(), "Never Read");
    assert_eq!(before.len(), 255);
    assert!(evaluate(lib.database(), "Read").is_empty());

    // 4. Reading-state mutations: one book resumes at page 3, one
    // book is read to its last page. The resume book needs enough
    // pages that page 3 keeps its ReadPercentage at or below the
    // "Never Read" threshold (10): (3+1)*100/count <= 10 → count >= 40.
    let candidates: Vec<String> = lib
        .database()
        .books
        .iter()
        .filter(|b| b.info.page_count > 40)
        .map(|b| b.file_path.clone())
        .take(2)
        .collect();
    assert_eq!(candidates.len(), 2, "fixture must have two long books");
    {
        let db = lib.database_mut();
        let resume_book = db
            .books
            .iter_mut()
            .find(|b| b.file_path == candidates[0])
            .unwrap();
        resume_book.set_current_page(3);
        let read_book = db
            .books
            .iter_mut()
            .find(|b| b.file_path == candidates[1])
            .unwrap();
        let last = read_book.info.page_count - 1;
        read_book.set_current_page(last);
    }
    lib.mark_dirty();

    // The list ground truth flipped: Never Read lost one, Read
    // gained one (the read book's ReadPercentage is 100).
    let never_read = evaluate(lib.database(), "Never Read");
    assert_eq!(never_read.len(), 254, "Never Read must lose the read book");
    let read = evaluate(lib.database(), "Read");
    assert_eq!(read.len(), 1, "Read must gain the read book");
    assert_eq!(read[0], candidates[1]);
    let read_book = lib
        .database()
        .books
        .iter()
        .find(|b| b.file_path == candidates[1])
        .unwrap();
    assert_eq!(read_book.last_page_read, read_book.info.page_count - 1);

    // 5. Save → reload: the reading state round-trips.
    lib.save().expect("save mutated");
    let saved_bytes = std::fs::read(&file).unwrap();
    let reloaded = load(&file).expect("reload mutated");
    let resume = reloaded
        .books
        .iter()
        .find(|b| b.file_path == candidates[0])
        .unwrap();
    assert_eq!(resume.current_page, 3, "resume position must round-trip");
    assert_eq!(resume.last_page_read, 3, "high-water mark must round-trip");
    let read = reloaded
        .books
        .iter()
        .find(|b| b.file_path == candidates[1])
        .unwrap();
    assert_eq!(read.last_page_read, read.info.page_count - 1);

    // 6. Exactly the two mutated books changed; every other book is
    // byte-identical to the fixture (full-form serialization).
    let fixture = load(REALWORLD_DB.as_ref()).expect("load fixture for comparison");
    let mut changed = 0;
    for (old, new) in fixture.books.iter().zip(reloaded.books.iter()) {
        let same = old.serialize_full_bytes().unwrap() == new.serialize_full_bytes().unwrap();
        if !same {
            changed += 1;
            assert!(
                old.file_path == candidates[0] || old.file_path == candidates[1],
                "only the mutated books may change (unexpected: {})",
                old.file_path
            );
        }
    }
    assert_eq!(changed, 2);

    // 7. Re-save of the mutated state is byte-stable.
    lib.save().expect("re-save mutated");
    assert_eq!(
        std::fs::read(&file).unwrap(),
        saved_bytes,
        "re-save of mutated state must be byte-identical"
    );

    // 8. A fresh session on the saved file: `find_book` by path finds
    // the mutated book with its state (the reader resume path).
    let (lib2, _) = Library::open(&file).expect("reopen");
    let found = lib2.find_book(&candidates[0]).expect("find by path");
    assert_eq!(found.current_page, 3);
    assert_eq!(found.info.page_count, resume.info.page_count);
}

#[test]
fn library_scan_adds_books_from_a_folder() {
    let dir = temp_dir("scan");
    let file = dir.join("ComicDb.xml");
    let (mut lib, status) = Library::open(&file).expect("open fresh");
    assert_eq!(status, cr_core::database::OpenStatus::FreshEmpty);

    // A folder with two fake comics and one ignored file.
    let folder = dir.join("comics");
    let sub = folder.join("sub");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(folder.join("a.cbz"), b"fakezip").unwrap();
    std::fs::write(sub.join("b.cbz"), b"fakezip2").unwrap();
    std::fs::write(folder.join("notes.txt"), b"skip").unwrap();

    let result = lib.scan_file_or_folder(&folder.to_string_lossy(), true, false);
    assert_eq!(result.added.len(), 2, "{result:?}");
    assert!(lib.is_dirty());
    lib.save().expect("save scanned");

    // A fresh session sees the added books.
    let (mut lib2, _) = Library::open(&file).expect("reopen scanned");
    assert_eq!(lib2.database().books.len(), 2);
    assert!(
        lib2.find_book(&sub.join("b.cbz").to_string_lossy())
            .is_some(),
        "recursive scan must reach subfolders"
    );

    // File-info refresh flags a vanished file as missing.
    std::fs::remove_file(folder.join("a.cbz")).unwrap();
    {
        let db = lib2.database_mut();
        let book = db
            .books
            .iter_mut()
            .find(|b| b.file_path.ends_with("a.cbz"))
            .unwrap();
        refresh_file_info(book);
        assert!(book.file_is_missing);
    }
}

#[test]
fn watch_events_rescan_the_stored_watch_folders() {
    let dir = temp_dir("watch");
    let file = dir.join("ComicDb.xml");
    let (mut lib, _) = Library::open(&file).expect("open fresh");

    let folder = dir.join("watched");
    std::fs::create_dir_all(&folder).unwrap();
    lib.add_watch_folder(&folder.to_string_lossy(), true);
    // Duplicate adds are rejected.
    lib.add_watch_folder(&folder.to_string_lossy(), true);
    assert_eq!(lib.database().watch_folders.len(), 1);

    // Touch a file inside the watched folder; the debounced events
    // must map back to the watch root, and the rescan adds the book.
    std::fs::write(folder.join("new.cbz"), b"fakezip").unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let mut roots = Vec::new();
    while std::time::Instant::now() < deadline {
        roots = lib.take_watch_folder_rescans();
        if !roots.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    assert!(!roots.is_empty(), "watch events produced no rescan roots");
    assert!(roots
        .iter()
        .any(|r| r.ends_with(folder.file_name().unwrap().to_str().unwrap())));

    let result = lib.scan_file_or_folder(&roots[0], true, false);
    assert_eq!(result.added.len(), 1, "{result:?}");
    assert_eq!(lib.database().books.len(), 1);
}
