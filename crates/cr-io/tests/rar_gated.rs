//! RAR write-back tests (ADR-030). Gated: they run only when
//! `CR_RAR_TESTS` is set and the RARLAB `rar` binary exists (`7z`
//! too — the read side goes through it). The fixture is built with
//! `rar a` in-test; no RAR data is committed.

use std::process::Command;

use cr_core::model::comic_book::ComicBook;
use cr_io::info::InfoLoadingMethod;
use cr_io::rar::find_rar;
use cr_io::sevenzip::find_7z;
use cr_io::write::store_info_scoped;
use cr_io::ComicProvider;

fn rar_toolchain() -> bool {
    std::env::var_os("CR_RAR_TESTS").is_some() && find_rar().is_some() && find_7z().is_some()
}

#[test]
fn cbr_round_trip_write_and_read() {
    if !rar_toolchain() {
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "comicrust-rar-rt-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let page_dir = dir.join("src");
    std::fs::create_dir_all(&page_dir).unwrap();
    std::fs::write(page_dir.join("cover.jpg"), b"cover-bytes").unwrap();
    std::fs::write(page_dir.join("page1.jpg"), b"page-1").unwrap();

    // Build the fixture with the same binary the writer uses; cwd at
    // the page dir keeps the entry names bare.
    let exe = find_rar().unwrap();
    let archive = dir.join("comic.cbr");
    let out = Command::new(&exe)
        .arg("a")
        .arg("-y")
        .arg(&archive)
        .arg("cover.jpg")
        .arg("page1.jpg")
        .current_dir(&page_dir)
        .output()
        .unwrap();
    assert!(out.status.success(), "rar a fixture failed");

    let provider = ComicProvider::open(&archive).unwrap();
    assert_eq!(provider.page_count(), 2);
    let names: Vec<&str> = provider.pages().iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["cover.jpg", "page1.jpg"]);

    // The write-back: ComicInfo.xml + ComicBook.xml into the archive.
    let mut book = ComicBook::default();
    book.info.series = "RAR Round Trip".into();
    book.info.number = "7".into();
    book.opened_count = 3;
    let changed = store_info_scoped(&provider, &book, true).unwrap();
    assert!(changed);

    // Re-open: pages intact, in-archive metadata wins on Slow.
    let provider = ComicProvider::open(&archive).unwrap();
    assert_eq!(provider.page_count(), 2);
    assert_eq!(provider.read_page(1).unwrap(), b"page-1");
    let info = provider.load_info(InfoLoadingMethod::Slow).unwrap();
    assert_eq!(info.series, "RAR Round Trip");
    assert_eq!(info.number, "7");
    let loaded_book = provider.load_book(InfoLoadingMethod::Slow).unwrap();
    assert_eq!(loaded_book.opened_count, 3);

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn cbr4_fixture_round_trip() {
    // Real-world CBRs are RAR4, which current `rar` (7.x) can create
    // no more — it can only update them. The proof therefore runs on a
    // user-supplied RAR4 file: set CBR_RAR4_FIXTURE=<path> (the file
    // stays out of the repo; the update runs on a copy).
    if !rar_toolchain() {
        return;
    }
    let Some(fixture) = std::env::var_os("CBR_RAR4_FIXTURE").map(std::path::PathBuf::from) else {
        return;
    };
    if !fixture.is_file() {
        return;
    }
    let dir = std::env::temp_dir().join(format!(
        "comicrust-rar4-rt-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let archive = dir.join("comic.cbr");
    std::fs::copy(&fixture, &archive).unwrap();

    let provider = ComicProvider::open(&archive).unwrap();
    let pages_before = provider.page_count();
    assert!(pages_before > 0, "fixture must carry pages");

    let mut book = ComicBook::default();
    book.info.series = "RAR4 Round Trip".into();
    let changed = store_info_scoped(&provider, &book, false).unwrap();
    assert!(changed);

    // The archive keeps its format and its pages; the info reads back.
    let provider = ComicProvider::open(&archive).unwrap();
    assert_eq!(provider.page_count(), pages_before);
    let info = provider.load_info(InfoLoadingMethod::Slow).unwrap();
    assert_eq!(info.series, "RAR4 Round Trip");

    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn cbr_write_without_rar_reports_access_error() {
    // Runs wherever `rar` is absent (CI included): the CBR branch must
    // fail with a clear access error instead of a silent no-op.
    if find_rar().is_some() {
        return;
    }
    let dir = std::env::temp_dir().join("comicrust-rar-error");
    std::fs::create_dir_all(&dir).unwrap();
    let archive = dir.join("comic.cbr");
    std::fs::write(&archive, b"Rar!\x1a\x07\x00").unwrap();
    let provider = ComicProvider::open(&archive).unwrap();
    let book = ComicBook::default();
    let err = store_info_scoped(&provider, &book, true).unwrap_err();
    assert!(err.to_string().contains("rar executable not found"));
    std::fs::remove_dir_all(&dir).ok();
}
