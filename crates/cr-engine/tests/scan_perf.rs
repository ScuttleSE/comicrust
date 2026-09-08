//! T10 timing gate: the library scan at real-file scale (the
//! kickoff's "large-library scan" sweep). Two runs over a tree of
//! real (small) CBZs:
//! 1. the FRESH-library add (one full file-info refresh per new file,
//!    including the provider page-count open — the true add cost),
//! 2. the no-op re-scan (every book exists; the metadata-only walk).
//!
//! The scan runs on the C# scan thread in the app (the UI stays
//! free); this gate bounds the wall time.
//!
//! Budget: 120 s for 1000 fresh adds (debug + real file IO).
use cr_core::database::comic_database::ComicDatabase;
use cr_core::xml::scalar::CrDateTime;
use cr_engine::scanner::{scan_database, ScanItem};
use std::io::Write;
use std::time::{Duration, Instant};

const FILES: usize = 1000;
const BUDGET: Duration = Duration::from_secs(120);

fn build_cbz(path: &std::path::Path) {
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

#[test]
fn scan_of_1000_new_files_stays_seconds() {
    let dir = std::env::temp_dir().join(format!("crscan-perf-{}", std::process::id()));
    // A two-level tree: 10 folders x 100 comics.
    for f in 0..10 {
        let sub = dir.join(format!("shelf{f:02}"));
        std::fs::create_dir_all(&sub).unwrap();
        for i in 0..FILES / 10 {
            build_cbz(&sub.join(format!("book{f:02}{i:03}.cbz")));
        }
    }

    let mut db = ComicDatabase {
        id: cr_core::xml::scalar::CrGuid::new_random(),
        ..Default::default()
    };
    let now = CrDateTime::now();
    let items = [ScanItem {
        location: dir.to_string_lossy().into_owned(),
        all: true,
        remove_missing: false,
        force_refresh_info: false,
    }];

    let t = Instant::now();
    let result = scan_database(&mut db, &items, &now);
    let fresh = t.elapsed();
    eprintln!(
        "fresh scan of {FILES} new files: {fresh:?} ({} added; budget {BUDGET:?})",
        result.added.len()
    );
    assert_eq!(result.added.len(), FILES);
    assert!(fresh < BUDGET, "the fresh scan regressed: {fresh:?}");

    let t = Instant::now();
    let result = scan_database(&mut db, &items, &now);
    let rescan = t.elapsed();
    eprintln!(
        "no-op re-scan of {FILES} files: {rescan:?} ({} updated)",
        result.updated.len()
    );
    assert!(result.added.is_empty());
    assert!(rescan < BUDGET, "the re-scan regressed: {rescan:?}");

    std::fs::remove_dir_all(&dir).ok();
}
