//! Timing gate for the Phase 8 T11 apply path. The user freeze
//! report: after OK the app froze "until all the comics were loaded"
//! — the apply called the FULL file-info refresh per found book, and
//! its page-count branch opened EVERY archive inline on the UI thread
//! (the stored Windows-era mtime always differs from the copied
//! file's, so the open always fired). The fix: the apply uses the
//! metadata-only refresh (`refresh_file_info_basic`) — the file
//! content is the one the DB describes, only the path changed, and
//! the stored page count stays valid.
//!
//! The test maps N real zip archives and asserts the stored page
//! counts survive (nothing re-opened) inside a budget the full
//! refresh would blow; it also TIMES the full refresh on the same
//! books for the record.
//!
//! Budget: 10 s (debug profile, 120 real archives).
use cr_core::database::comic_database::ComicDatabase;
use cr_core::database::list_items::WatchFolder;
use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::CrDateTime;
use cr_engine::path_migration::{apply, Mapping};
use std::io::Write;
use std::time::{Duration, Instant};

const BOOKS: usize = 120;
const BUDGET: Duration = Duration::from_secs(10);

fn build_zip(path: &std::path::Path) {
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    for p in ["00000.jpg", "00001.jpg"] {
        zip.start_file(p, options).unwrap();
        zip.write_all(&[0u8; 64]).unwrap();
    }
    zip.finish().unwrap();
}

/// The migrated shape: a known page count from the Windows DB and a
/// stale stored mtime (the copy refreshed the file times).
fn make_db(comics: &std::path::Path) -> ComicDatabase {
    let mut db = ComicDatabase {
        id: cr_core::xml::scalar::CrGuid::new_random(),
        ..Default::default()
    };
    for i in 0..BOOKS {
        let mut b = ComicBook {
            file_path: format!("C:\\Comics\\book{i:03}.cbz"),
            ..Default::default()
        };
        b.info.page_count = 24;
        b.file_modified_time = CrDateTime::min_value();
        db.books.push(b);
    }
    db.watch_folders = vec![WatchFolder {
        folder: "C:\\Comics".into(),
        watch: false,
    }];
    let _ = comics;
    db
}

#[test]
fn apply_maps_real_archives_without_reopening_them() {
    let dir = std::env::temp_dir().join(format!("crpathmig-perf-{}", std::process::id()));
    let comics = dir.join("Comics");
    std::fs::create_dir_all(&comics).unwrap();
    for i in 0..BOOKS {
        build_zip(&comics.join(format!("book{i:03}.cbz")));
    }

    // The FIXED path (the apply): metadata-only per book.
    let mut db = make_db(&comics);
    let t = Instant::now();
    let report = apply(
        &mut db,
        &[Mapping {
            windows_root: "C:\\Comics".into(),
            linux_target: comics.to_string_lossy().into_owned(),
        }],
    );
    let light = t.elapsed();
    eprintln!("apply (light refresh) {BOOKS} real archives: {light:?} (budget {BUDGET:?})");
    assert_eq!(report.books_mapped, BOOKS);
    assert_eq!(report.books_fileless, 0);
    // The stored page count survived — nothing re-opened an archive.
    assert!(
        db.books.iter().all(|b| b.info.page_count == 24),
        "the stored page count must ride the migration (no re-open)"
    );
    assert!(
        light < BUDGET,
        "the apply must stay metadata-cheap: {light:?}"
    );

    // The OLD path for the record: the full refresh per book (one
    // provider open each).
    let mut db2 = make_db(&comics);
    let t = Instant::now();
    for (i, b) in db2.books.iter_mut().enumerate() {
        b.file_path = comics
            .join(format!("book{i:03}.cbz"))
            .to_string_lossy()
            .into_owned();
        cr_engine::scanner::refresh_file_info(b);
    }
    let full = t.elapsed();
    eprintln!(
        "full refresh (one provider open per book) {BOOKS} archives: {full:?} — the freeze the fix removes"
    );

    std::fs::remove_dir_all(&dir).ok();
}
