//! Headless probe: the scan markers (the port addition — no C#
//! counterpart; user request 2026-09-11).
//!
//! Gates:
//!
//! - A. A real scan of a folder that holds one good file, one
//!   unreadable file, and one mislabeled file imports ALL THREE and
//!   never stops.
//! - B. The two problem books carry their stored verdicts, so a smart
//!   list can find them after the scan.
//! - C. The grid draws exactly two scan markers.
//! - D. Repairing the broken file clears its marker on a rescan.
//!
//! Run: Xvfb + `cargo run --release -p cr-ui --example scanmarker_probe`
//! with an isolated XDG pair.
use std::io::Write;

use cr_core::scan_status::{self, ScanStatus};
use gtk4::glib;
use gtk4::prelude::*;

/// A valid single-page CBZ.
fn write_good_cbz(path: &std::path::Path) {
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file("00000.jpg", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(&[
        0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x43, 0x00, 0x08, 0x06, 0x06, 0x07, 0x06, 0x05, 0x08, 0x07,
        0x07, 0x09, 0x09, 0x08, 0x0A, 0x0C, 0x14, 0x0D, 0x0C, 0x0B, 0x0B, 0x0C, 0x19, 0x12, 0x13,
        0x0F, 0x14, 0x1D, 0x1A, 0x1F, 0x1E, 0x1D, 0x1A, 0x1C, 0x1C, 0x20, 0x24, 0x2E, 0x27, 0x20,
        0x22, 0x2C, 0x23, 0x1C, 0x1E, 0x23, 0x27, 0x29, 0x2B, 0x2E, 0x27, 0x2C, 0x2A, 0x2D, 0x2F,
        0x29, 0x2B, 0xFF, 0xD9,
    ])
    .unwrap();
    zip.finish().unwrap();
}

/// A `.cbz` that starts like a zip and has no central directory — the
/// shape measured on "The Boys 064 (2012).cbz".
fn write_broken_cbz(path: &std::path::Path) {
    let mut bytes: Vec<u8> = b"PK\x03\x04".to_vec();
    bytes.extend_from_slice(&[0u8; 26]);
    bytes.extend_from_slice(&vec![0x5a; 256 * 1024]);
    std::fs::write(path, &bytes).unwrap();
}

/// A TAR archive with a `.cbz` name — readable, but not the format the
/// name claims (the "Blacksad" shape, without needing a subprocess).
fn write_tar_named_cbz(path: &std::path::Path) {
    let file = std::fs::File::create(path).unwrap();
    let mut builder = tar::Builder::new(file);
    let data = [
        0xFFu8, 0xD8, 0xFF, 0xDB, 0x00, 0x43, 0x00, 0x08, 0x06, 0x06, 0x07, 0x06, 0x05, 0x08, 0x07,
        0x07, 0x09, 0x09, 0x08, 0x0A, 0x0C, 0x14, 0x0D, 0x0C, 0x0B, 0x0B, 0x0C, 0x19, 0x12, 0x13,
        0x0F, 0x14, 0x1D, 0x1A, 0x1F, 0x1E, 0x1D, 0x1A, 0x1C, 0x1C, 0x20, 0x24, 0x2E, 0x27, 0x20,
        0x22, 0x2C, 0x23, 0x1C, 0x1E, 0x23, 0x27, 0x29, 0x2B, 0x2E, 0x27, 0x2C, 0x2A, 0x2D, 0x2F,
        0x29, 0x2B, 0xFF, 0xD9,
    ];
    let mut header = tar::Header::new_gnu();
    header.set_size(data.len() as u64);
    header.set_mode(0o644);
    header.set_cksum();
    builder
        .append_data(&mut header, "00000.jpg", data.as_slice())
        .unwrap();
    builder.finish().unwrap();
}

fn book_status(path: &std::path::Path) -> Option<ScanStatus> {
    let lib = cr_ui::library::session();
    let lib = lib.borrow();
    lib.find_book(&path.to_string_lossy())
        .and_then(scan_status::status)
}

fn main() {
    gtk4::init().expect("gtk init");
    cr_ui::theme::init();
    if !std::env::var("XDG_DATA_HOME")
        .map(|v| v.contains("/tmp/opencode"))
        .unwrap_or(false)
        || !std::env::var("XDG_CONFIG_HOME")
            .map(|v| v.contains("/tmp/opencode"))
            .unwrap_or(false)
    {
        eprintln!("REFUSED: set XDG_DATA_HOME=/tmp/opencode/<dir> AND XDG_CONFIG_HOME=/tmp/opencode/<dir> (the probe writes the database)");
        std::process::exit(1);
    }
    let work = std::path::Path::new("/tmp/opencode/scanmarker");
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).unwrap();

    // The walk is sorted, so the BROKEN file is met first. A scan that
    // stopped on it would never reach the other two.
    let broken = work.join("0 Broken 001.cbz");
    let mislabeled = work.join("1 Mislabeled 001.cbz");
    let good = work.join("2 Good 001.cbz");
    write_broken_cbz(&broken);
    write_tar_named_cbz(&mislabeled);
    write_good_cbz(&good);

    cr_ui::library::initialize().expect("session init");

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.scanmarker-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let (window, shell) = cr_ui::browser::shell::BrowserShell::create(app);
        window.present();
        std::mem::forget(shell.clone());
        let (broken, mislabeled, good) = (broken.clone(), mislabeled.clone(), good.clone());

        cr_ui::library::add_folder_to_library(work, move |result| {
            // A. Every file landed, and the scan reported the two
            //    problems instead of stopping on them.
            assert_eq!(
                result.added.len(),
                3,
                "A FAIL: expected 3 books: {result:?}"
            );
            assert_eq!(result.unreadable.len(), 1, "A FAIL: {result:?}");
            assert_eq!(result.mismatched.len(), 1, "A FAIL: {result:?}");
            println!(
                "A ok: the scan imported all 3 files and flagged 2 ({} unreadable, {} mismatched)",
                result.unreadable.len(),
                result.mismatched.len()
            );

            // B. The verdicts are ON the books, so a smart list finds
            //    them after the scan.
            assert_eq!(
                book_status(&broken),
                Some(ScanStatus::Unreadable),
                "B FAIL: the broken file must carry the Unreadable verdict"
            );
            assert_eq!(
                book_status(&mislabeled),
                Some(ScanStatus::FormatMismatch),
                "B FAIL: the mislabeled file must carry the Format mismatch verdict"
            );
            assert_eq!(
                book_status(&good),
                None,
                "B FAIL: the good file must carry no verdict"
            );
            println!("B ok: the stored verdicts are queryable after the scan");

            let shell2 = shell.clone();
            let broken2 = broken.clone();
            shell.refresh_after_data_change();
            // Settle past the debounced evaluation and the first thumb
            // decode before reading the draw counters.
            glib::timeout_add_local(std::time::Duration::from_millis(1200), move || {
                // C. The grid draws one chip per problem book.
                let drawn = shell2.state_grid_scan_marker_draws();
                assert_eq!(
                    drawn, 2,
                    "C FAIL: expected 2 scan markers drawn, got {drawn}"
                );
                println!("C ok: the grid draws {drawn} scan markers");

                // D. Repair the broken file: the next scan clears the
                //    marker with no user action.
                write_good_cbz(&broken2);
                let shell3 = shell2.clone();
                let broken3 = broken2.clone();
                cr_ui::library::add_folder_to_library(work, move |result| {
                    assert!(result.unreadable.is_empty(), "D FAIL: {result:?}");
                    assert_eq!(
                        book_status(&broken3),
                        None,
                        "D FAIL: a repaired file must clear its own marker"
                    );
                    shell3.refresh_after_data_change();
                    glib::timeout_add_local(std::time::Duration::from_millis(1200), move || {
                        let drawn = shell3.state_grid_scan_marker_draws();
                        assert_eq!(
                            drawn, 1,
                            "D FAIL: only the mislabeled book should still be marked, got {drawn}"
                        );
                        println!("D ok: the repaired book cleared its marker ({drawn} left)");
                        println!("PROBE DONE");
                        std::process::exit(0);
                    });
                });
                glib::ControlFlow::Break
            });
        });
        let _ = window;
    });

    let _ = app.run();
}
