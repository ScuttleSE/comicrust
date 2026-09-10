//! Headless probe: the scan-land view refresh (the user report —
//! File ▸ Scan Book Folders ran the scan, the Tasks dialog showed the
//! Scanning line, but the Library stayed empty: the scan's done
//! callback never refreshed the view). Gates the REAL paths:
//! A. a fresh library shows 0 books in the grid,
//! B. Scan Book Folders over a seeded watch root lands the scan and
//!    the grid NOW shows the scanned books (the refresh ran),
//! C. a second scan (idempotent re-scan) keeps the count (no dupes,
//!    the refresh still runs).
//! Run: Xvfb + `cargo run -p cr-ui --release --example scanrefresh_probe`
//! with an isolated XDG pair.
use gtk4::glib;
use gtk4::prelude::*;
use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;

fn write_page_zip(path: &Path) {
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file("00000.jpg", zip::write::SimpleFileOptions::default())
        .unwrap();
    // A 1x1 JPEG placeholder (the decoder accepts it as a page).
    std::io::Write::write_all(
        &mut zip,
        &[
            0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x43, 0x00, 0x08, 0x06, 0x06, 0x07, 0x06, 0x05, 0x08,
            0x07, 0x07, 0x09, 0x09, 0x08, 0x0A, 0x0C, 0x14, 0x0D, 0x0C, 0x0B, 0x0B, 0x0C, 0x19,
            0x12, 0x13, 0x0F, 0x14, 0x1D, 0x1A, 0x1F, 0x1E, 0x1D, 0x1A, 0x1C, 0x1C, 0x20, 0x24,
            0x2E, 0x27, 0x20, 0x22, 0x2C, 0x23, 0x1C, 0x1E, 0x23, 0x27, 0x29, 0x2B, 0x2E, 0x27,
            0x2C, 0x2A, 0x2D, 0x2F, 0x29, 0x2B, 0xFF, 0xD9,
        ],
    )
    .unwrap();
    zip.finish().unwrap();
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
    let work = std::path::Path::new("/tmp/opencode/scanrefresh");
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).unwrap();
    cr_ui::library::initialize().expect("session init");

    // The scanned folder: two comics (real zips — the scan opens each
    // for the page count).
    let folder = work.join("comics");
    std::fs::create_dir_all(&folder).unwrap();
    write_page_zip(&folder.join("Scanned A 001.cbz"));
    write_page_zip(&folder.join("Scanned B 002.cbz"));

    // The watch-folder root (the Preferences add shape) so
    // `win.scan-folders` finds it.
    {
        let lib = cr_ui::library::session();
        let mut l = lib.borrow_mut();
        l.database_mut()
            .watch_folders
            .push(cr_core::database::list_items::WatchFolder {
                folder: folder.to_string_lossy().into_owned(),
                watch: true,
            });
        l.mark_dirty();
    }

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.scanrefresh-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let app = app.clone();
        let (window, shell) = cr_ui::browser::shell::BrowserShell::create(&app);
        window.present();
        std::mem::forget(shell.clone());

        let book_count = |shell: &cr_ui::browser::shell::BrowserShell| shell.state_grid_book_count();

        // Settle past the boot fill's 200 ms debounced Library
        // evaluation first — it must already have fired when the scan
        // starts, or the boot race fakes a refresh (measured: an
        // immediate dispatch passed against the unfixed code).
        glib::timeout_add_local(std::time::Duration::from_millis(600), {
            let shell = shell.clone();
            move || {
                // A. The fresh library grid is empty (past the boot
                //    evaluation — no pending boot fill can set books).
                assert_eq!(
                    book_count(&shell),
                    0,
                    "the fresh library grid must start empty"
                );
                println!("A ok: fresh grid 0 books");

                // B. Scan Book Folders → the scan lands → the grid
                //    refreshes.
                let window = shell.window();
                let _ = gtk4::prelude::WidgetExt::activate_action(
                    &window,
                    "win.scan-folders",
                    None,
                );
                let landed = Rc::new(Cell::new(0u32));
                glib::timeout_add_local(std::time::Duration::from_millis(100), {
                    let shell = shell.clone();
                    let landed = landed.clone();
                    move || {
                        landed.set(landed.get() + 1);
                        let scanning = cr_ui::library::is_scanning();
                        let count = book_count(&shell);
                        if !scanning && count == 2 {
                            println!("B ok: scan landed, grid shows {count} books");
                            gate_c(shell.clone(), landed.get());
                            return glib::ControlFlow::Break;
                        }
                        if landed.get() > 60 {
                            panic!(
                                "B FAIL: the scan landed (scanning={scanning}) but the grid shows {count} books (expected 2) — the scan-land refresh did not run"
                            );
                        }
                        glib::ControlFlow::Continue
                    }
                });
                glib::ControlFlow::Break
            }
        });
    });

    // The app runs until the gates finish (the last gate exits the
    // process — a plain Window probe shape would exit early, the
    // ApplicationWindow holds it).
    let _ = app.run();
    unreachable!("the probe exits through the final gate");
}

/// C. The re-scan (the same command again): the count stays 2 and the
/// refresh still runs (no duplicates).
fn gate_c(shell: cr_ui::browser::shell::BrowserShell, _landed: u32) {
    let window = shell.window();
    let _ = gtk4::prelude::WidgetExt::activate_action(&window, "win.scan-folders", None);
    let ticks = Rc::new(Cell::new(0u32));
    glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        ticks.set(ticks.get() + 1);
        let scanning = cr_ui::library::is_scanning();
        let count = shell.state_grid_book_count();
        if !scanning && count == 2 {
            println!("C ok: re-scan keeps 2 books");
            std::process::exit(0);
        }
        if ticks.get() > 60 {
            panic!(
                "C FAIL: the re-scan landed (scanning={scanning}) but the grid shows {count} books (expected 2)"
            );
        }
        glib::ControlFlow::Continue
    });
}
