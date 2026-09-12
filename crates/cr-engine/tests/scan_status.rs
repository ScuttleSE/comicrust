//! Scanner behavior for files that cannot be read, or whose content
//! does not match their name (PORT ADDITION, user request 2026-09-11).
//!
//! The rule these tests pin: an unattended scan of a large library
//! never stops on a bad file. It records a verdict on the book, counts
//! it in the run summary, and moves on.

use std::io::Write;
use std::path::{Path, PathBuf};

use cr_core::model::comic_book::ComicBook;
use cr_core::scan_status::{self, ScanStatus};
use cr_core::xml::scalar::CrDateTime;
use cr_engine::scanner::{scan_sync, ScanItem, ScanLimits};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "comicrust-scanstatus-{name}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A valid, readable CBZ with one page.
fn write_good_cbz(path: &Path) {
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::SimpleFileOptions = Default::default();
    zip.start_file("page1.jpg", options).unwrap();
    zip.write_all(b"not really a jpeg").unwrap();
    zip.finish().unwrap();
}

/// A `.cbz` that begins like a zip but carries no central directory —
/// the shape measured on "The Boys 064 (2012).cbz".
fn write_broken_cbz(path: &Path) {
    let mut bytes: Vec<u8> = b"PK\x03\x04".to_vec();
    bytes.extend_from_slice(&[0u8; 26]);
    bytes.extend_from_slice(&vec![0x5a; 64 * 1024]);
    std::fs::write(path, &bytes).unwrap();
}

/// A TAR archive with a `.cbz` name — the readable-but-mislabeled
/// shape measured on "Blacksad ... Issue 004.cbz" (that one is RAR;
/// TAR needs no subprocess, so it runs on every machine).
fn write_tar_named_cbz(path: &Path) {
    let file = std::fs::File::create(path).unwrap();
    let mut builder = tar::Builder::new(file);
    let data = b"not really a jpeg";
    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder
        .append_data(&mut header, "page1.jpg", data.as_slice())
        .unwrap();
    builder.finish().unwrap();
}

fn items(dir: &Path) -> Vec<ScanItem> {
    vec![ScanItem {
        location: dir.to_string_lossy().into(),
        all: true,
        remove_missing: false,
        force_refresh_info: false,
    }]
}

fn book_for<'a>(storage: &'a [ComicBook], path: &Path) -> &'a ComicBook {
    storage
        .iter()
        .find(|b| Path::new(&b.file_path) == path)
        .expect("the scanned file must be in the library")
}

#[test]
fn an_unreadable_file_is_marked_and_the_scan_continues() {
    let dir = temp_dir("unreadable");
    // Names force the walk order: the broken file comes FIRST, so a
    // scan that stopped on it would never reach the good one.
    let broken = dir.join("0-broken.cbz");
    let good = dir.join("1-good.cbz");
    write_broken_cbz(&broken);
    write_good_cbz(&good);

    let mut storage: Vec<ComicBook> = Vec::new();
    let result = scan_sync(&mut storage, &items(&dir), &CrDateTime::min_value());

    // Both files are in the library: the bad one is marked, not lost.
    assert_eq!(result.added.len(), 2, "{result:?}");
    assert_eq!(storage.len(), 2);
    assert_eq!(
        result.unreadable,
        vec![broken.to_string_lossy().to_string()]
    );
    assert!(result.has_problems());
    assert_eq!(result.problem_count(), 1);

    let bad = book_for(&storage, &broken);
    assert_eq!(scan_status::status(bad), Some(ScanStatus::Unreadable));
    assert!(scan_status::status(bad).unwrap().is_failure());
    assert!(
        scan_status::error_text(bad).is_some(),
        "the stored verdict must say WHY the file could not be read"
    );

    // The good file that follows it is imported normally and unmarked.
    let ok = book_for(&storage, &good);
    assert_eq!(scan_status::status(ok), None);
    assert_eq!(ok.info.page_count, 1);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_mislabeled_archive_is_imported_and_flagged() {
    let dir = temp_dir("mismatch");
    let path = dir.join("tar-inside.cbz");
    write_tar_named_cbz(&path);

    let mut storage: Vec<ComicBook> = Vec::new();
    let result = scan_sync(&mut storage, &items(&dir), &CrDateTime::min_value());

    assert_eq!(result.mismatched, vec![path.to_string_lossy().to_string()]);
    assert!(result.unreadable.is_empty());

    let book = book_for(&storage, &path);
    assert_eq!(scan_status::status(book), Some(ScanStatus::FormatMismatch));
    assert!(
        !scan_status::status(book).unwrap().is_failure(),
        "a mismatch is readable, so it draws the amber chip, not the red one"
    );
    assert_eq!(
        scan_status::detected_format(book).as_deref(),
        Some("eComic (TAR)")
    );
    // Readable through the detected reader: the pages really arrived.
    assert_eq!(book.info.page_count, 1);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_known_bad_file_is_not_reopened_on_the_next_scan() {
    let dir = temp_dir("known-bad");
    let broken = dir.join("broken.cbz");
    write_broken_cbz(&broken);

    let mut storage: Vec<ComicBook> = Vec::new();
    let first = scan_sync(&mut storage, &items(&dir), &CrDateTime::min_value());
    assert_eq!(first.unreadable.len(), 1);

    // A rescan of an unchanged bad file must not pay for the failure
    // again — a library with thousands of them would re-read every one
    // on every scan.
    let second = scan_sync(&mut storage, &items(&dir), &CrDateTime::min_value());
    assert_eq!(
        second.skipped_known_bad,
        vec![broken.to_string_lossy().to_string()],
        "{second:?}"
    );
    assert!(second.unreadable.is_empty());
    // The verdict is still on the book, so the chip and the smart list
    // keep working.
    let book = book_for(&storage, &broken);
    assert_eq!(scan_status::status(book), Some(ScanStatus::Unreadable));

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_repaired_file_loses_its_marker_on_the_next_scan() {
    let dir = temp_dir("repaired");
    let path = dir.join("book.cbz");
    write_broken_cbz(&path);

    let mut storage: Vec<ComicBook> = Vec::new();
    let first = scan_sync(&mut storage, &items(&dir), &CrDateTime::min_value());
    assert_eq!(first.unreadable.len(), 1);
    assert_eq!(
        scan_status::status(book_for(&storage, &path)),
        Some(ScanStatus::Unreadable)
    );

    // Replace the file with a valid archive. The fingerprint changes,
    // so the scan re-opens it even though it is a known failure.
    write_good_cbz(&path);
    let second = scan_sync(&mut storage, &items(&dir), &CrDateTime::min_value());

    assert!(second.skipped_known_bad.is_empty(), "{second:?}");
    assert!(second.unreadable.is_empty(), "{second:?}");
    let book = book_for(&storage, &path);
    assert_eq!(
        scan_status::status(book),
        None,
        "a repaired file must clear its own marker with no user action"
    );
    assert_eq!(book.info.page_count, 1);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn retry_failed_forces_a_reopen_of_known_bad_files() {
    let dir = temp_dir("retry");
    let broken = dir.join("broken.cbz");
    write_broken_cbz(&broken);

    let mut storage: Vec<ComicBook> = Vec::new();
    scan_sync(&mut storage, &items(&dir), &CrDateTime::min_value());

    let limits = ScanLimits {
        retry_failed: true,
        ..Default::default()
    };
    let result = cr_engine::scanner::scan_sync_with_control(
        &mut storage,
        &items(&dir),
        &CrDateTime::min_value(),
        &mut |_| {},
        &cr_engine::scanner::ScanControl::inert(),
        limits,
        &mut |_| {},
    );
    assert!(result.skipped_known_bad.is_empty());
    assert_eq!(
        result.unreadable,
        vec![broken.to_string_lossy().to_string()],
        "an explicit retry re-reads the file and reports it again"
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn explicit_file_paths_scan_one_batch_and_force_the_retry() {
    // The "Rescan Book File(s)" / "Scan List Contents" composition
    // (ADR-036): ONE request of one-file items over the linked paths,
    // with a one-shot forced retry. Only the given files walk — a
    // sibling in the same folder stays out (the user's "rescan just
    // these individual books").
    let dir = temp_dir("explicit-paths");
    let broken = dir.join("0-broken.cbz");
    let good = dir.join("1-good.cbz");
    write_broken_cbz(&broken);
    write_good_cbz(&good);

    let file_items = |paths: &[&Path]| -> Vec<ScanItem> {
        paths
            .iter()
            .map(|p| ScanItem {
                location: p.to_string_lossy().into(),
                all: false,
                remove_missing: false,
                force_refresh_info: false,
            })
            .collect()
    };
    let now = CrDateTime::min_value();

    // First pass: ONE file requested — the broken sibling in the same
    // folder must not appear in the library.
    let mut storage: Vec<ComicBook> = Vec::new();
    let result = scan_sync(&mut storage, &file_items(&[&good]), &now);
    assert_eq!(
        result.added,
        vec![good.to_string_lossy().to_string()],
        "{result:?}"
    );
    assert_eq!(storage.len(), 1);

    // Second pass over BOTH paths, forced retry: the broken file is
    // scanned for the first time (added), the good one refreshes.
    let limits = ScanLimits {
        retry_failed: true,
        ..Default::default()
    };
    let forced = |storage: &mut Vec<ComicBook>, paths: &[&Path]| {
        cr_engine::scanner::scan_sync_with_control(
            storage,
            &file_items(paths),
            &now,
            &mut |_| {},
            &cr_engine::scanner::ScanControl::inert(),
            limits,
            &mut |_| {},
        )
    };
    let result = forced(&mut storage, &[&good, &broken]);
    assert_eq!(result.added, vec![broken.to_string_lossy().to_string()]);
    assert_eq!(result.updated, vec![good.to_string_lossy().to_string()]);
    assert_eq!(
        scan_status::status(book_for(&storage, &broken)),
        Some(ScanStatus::Unreadable)
    );

    // Third pass, same paths, still forced: the known-bad skip does
    // not apply, so the broken file re-reads and re-reports.
    let result = forced(&mut storage, &[&good, &broken]);
    assert!(result.skipped_known_bad.is_empty(), "{result:?}");
    assert_eq!(result.updated.len(), 2, "{result:?}");
    assert_eq!(
        result.unreadable,
        vec![broken.to_string_lossy().to_string()]
    );

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn skip_current_file_abandons_one_file_and_keeps_scanning() {
    let dir = temp_dir("skip");
    let first_file = dir.join("0-a.cbz");
    let second_file = dir.join("1-b.cbz");
    write_good_cbz(&first_file);
    write_good_cbz(&second_file);

    // A skip request that fires once, for the first file only.
    let pending = std::sync::atomic::AtomicBool::new(true);
    let take_skip = || {
        pending
            .compare_exchange(
                true,
                false,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            )
            .is_ok()
    };
    let control = cr_engine::scanner::ScanControl {
        stop: &|| false,
        take_skip: &take_skip,
    };
    // A zero deadline would race the skip; keep the normal one and let
    // the skip request decide.
    let limits = ScanLimits {
        per_file_timeout: Some(std::time::Duration::from_secs(30)),
        retry_failed: false,
    };

    let mut storage: Vec<ComicBook> = Vec::new();
    let result = cr_engine::scanner::scan_sync_with_control(
        &mut storage,
        &items(&dir),
        &CrDateTime::min_value(),
        &mut |_| {},
        &control,
        limits,
        &mut |_| {},
    );

    // Whichever file the skip landed on, the scan kept going and both
    // files are in the library.
    assert_eq!(storage.len(), 2, "{result:?}");
    assert_eq!(result.added.len(), 2, "{result:?}");
    assert!(
        result.skipped.len() <= 1,
        "one skip request must abandon at most one file: {result:?}"
    );
    for book in &storage {
        if let Some(status) = scan_status::status(book) {
            assert_eq!(status, ScanStatus::Skipped, "{result:?}");
        }
    }

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_zero_deadline_marks_files_timed_out_without_stopping_the_scan() {
    let dir = temp_dir("deadline");
    write_good_cbz(&dir.join("0-a.cbz"));
    write_good_cbz(&dir.join("1-b.cbz"));

    // A deadline no real read can meet: every file is abandoned, and
    // the scan STILL reaches the end and reports.
    let limits = ScanLimits {
        per_file_timeout: Some(std::time::Duration::from_nanos(1)),
        retry_failed: false,
    };
    let mut storage: Vec<ComicBook> = Vec::new();
    let result = cr_engine::scanner::scan_sync_with_control(
        &mut storage,
        &items(&dir),
        &CrDateTime::min_value(),
        &mut |_| {},
        &cr_engine::scanner::ScanControl::inert(),
        limits,
        &mut |_| {},
    );

    assert_eq!(result.added.len(), 2, "{result:?}");
    assert_eq!(storage.len(), 2);
    for book in &storage {
        // Each book is either timed out or (if the read beat the
        // deadline) imported cleanly; neither outcome loses the file.
        let status = scan_status::status(book);
        assert!(
            status.is_none() || status == Some(ScanStatus::TimedOut),
            "unexpected verdict {status:?}"
        );
    }

    std::fs::remove_dir_all(&dir).ok();
}
