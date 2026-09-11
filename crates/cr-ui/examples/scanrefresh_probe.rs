//! Headless probe: the scan-land view refresh + the progressive
//! fill + the abort + the mid-scan library liveness (the user
//! reports — File ▸ Scan Book Folders ran the scan but the Library
//! stayed empty; then "populate as we scan" + "what happens if the
//! scan is aborted"; then "the search results blank mid-scan and
//! stay blank until restart"). Gates the REAL paths:
//! A. a fresh library shows 0 books in the grid,
//! B. Scan Book Folders over a seeded watch root lands the scan and
//!    the grid NOW shows the scanned books (the refresh ran),
//! C. a second scan (idempotent re-scan) keeps the count (no dupes),
//! D. a scan over a big folder fills the grid WHILE it walks (the
//!    C# live-storage parity), and Abort Scanning stops the walk —
//!    the books found so far stay (a partial landing, count < total),
//! E. the re-scan after the abort completes the library (the stored
//!    books refresh cheaply — nothing is redone),
//! G. a RE-scan keeps the whole library live mid-scan: the grid, a
//!    smart-list evaluation and the refresh/data-change path (the
//!    debounced select) all evaluate to the FULL stored set — the
//!    search results cannot blank (the worker scans a clone, not a
//!    take),
//! H. the watch-poll dedupe: pending watch events do NOT queue a
//!    rescan while a scan runs; they deliver once it ends,
//! I. a non-Library view (a smart list) does not churn per batch
//!    tick (no per-tick full refresh); it updates once at the
//!    landing,
//! J. what the user does MID-SCAN survives the landing merge: a
//!    removed book stays removed, an edited book keeps the edit.
//! Run: Xvfb + `cargo run -p cr-ui --release --example scanrefresh_probe`
//! with an isolated XDG pair.
use gtk4::glib;
use gtk4::prelude::*;
use std::cell::Cell;
use std::path::Path;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};

use cr_core::database::list_items::ComicListItem;
use cr_core::xml::scalar::CrGuid;

fn write_page_zip(path: &Path) {
    write_page_zip_entries(path, &[]);
}

fn write_page_zip_entries(path: &Path, extra: &[(&str, Vec<u8>)]) {
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
    for (name, data) in extra {
        zip.start_file(
            (*name).to_string(),
            zip::write::SimpleFileOptions::default(),
        )
        .unwrap();
        std::io::Write::write_all(&mut zip, data).unwrap();
    }
    zip.finish().unwrap();
}

fn comic_info_bytes(series: &str) -> Vec<u8> {
    format!(
        "<?xml version=\"1.0\"?>\r\n<ComicInfo xmlns:xsd=\"http://www.w3.org/2001/XMLSchema\" xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\">\r\n  <Series>{series}</Series>\r\n  <Number>1</Number>\r\n</ComicInfo>"
    )
    .into_bytes()
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
    // The probe owns the XDG pair — a stale database from a previous
    // run (the exit save) would fail gate A.
    for dir in [
        std::path::PathBuf::from(std::env::var("XDG_DATA_HOME").expect("XDG_DATA_HOME")),
        std::path::Path::new(&std::env::var("XDG_CONFIG_HOME").expect("XDG_CONFIG_HOME"))
            .to_path_buf(),
    ] {
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
    }
    cr_ui::library::initialize().expect("session init");

    // The scanned folder: two comics (real zips — the scan opens each
    // for the page count).
    let folder = work.join("comics");
    std::fs::create_dir_all(&folder).unwrap();
    // "Scanned A" carries an embedded ComicInfo.xml — gate K proves
    // the scan imports the info chain.
    write_page_zip_entries(
        &folder.join("Scanned A 001.cbz"),
        &[("ComicInfo.xml", comic_info_bytes("Scanned Info Series"))],
    );
    write_page_zip(&folder.join("Scanned B 002.cbz"));

    // The watch-folder root (the Preferences add shape — the REAL
    // API: it pushes AND rebuilds the live watcher, so gate H's
    // events exist) so `win.scan-folders` finds it.
    {
        let lib = cr_ui::library::session();
        lib.borrow_mut()
            .add_watch_folder(&folder.to_string_lossy(), true);
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
            let folder = folder.clone();
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
                    let folder = folder.clone();
                    move || {
                        landed.set(landed.get() + 1);
                        let scanning = cr_ui::library::is_scanning();
                        let count = book_count(&shell);
                        if !scanning && count == 2 {
                            // K. The scan imported the info chain: the
                            //    scanned book carries its embedded
                            //    ComicInfo.xml series (the C#
                            //    RefreshInfoFromFile parity —
                            //    ComicScanner.cs:222). The read runs in
                            //    its own scope — the Ref guard must be
                            //    gone before gate_c dispatches the
                            //    scan (the scan landing borrows the
                            //    session mut).
                            let book_path =
                                folder.join("Scanned A 001.cbz").to_string_lossy().into_owned();
                            let series = {
                                let lib = cr_ui::library::session();
                                let lib = lib.borrow();
                                let book = lib.find_book(&book_path).unwrap_or_else(|| {
                                    panic!("K FAIL: {book_path} not in the library")
                                });
                                book.info.series.clone()
                            };
                            assert_eq!(
                                series, "Scanned Info Series",
                                "K FAIL: the scan did not import the ComicInfo.xml series"
                            );
                            println!("K ok: the scanned book carries its ComicInfo series");
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
    // ApplicationWindow holds it). Gate F closes the window mid-scan:
    // the close-request handler aborts the scan, merges the partial
    // and saves — the run loop then returns and main verifies the
    // saved file.
    let _ = app.run();
    let data = std::env::var("XDG_DATA_HOME").expect("XDG_DATA_HOME");
    let db_path = std::path::Path::new(&data).join("comicrust/ComicDb/ComicDb.xml");
    let db =
        cr_core::database::comic_database::load(&db_path).expect("F: the saved database loads");
    let n = db.books.len();
    assert!(
        n > BIG_COUNT + 2 && n < BIG_COUNT + 2 + EXTRA_COUNT,
        "F FAIL: the exit saved {n} books (expected the partial add: {} < n < {})",
        BIG_COUNT + 2,
        BIG_COUNT + 2 + EXTRA_COUNT
    );
    println!("F ok: the mid-add exit saved {n} books");
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
            gate_d(&shell);
            return glib::ControlFlow::Break;
        }
        if ticks.get() > 60 {
            panic!(
                "C FAIL: the re-scan landed (scanning={scanning}) but the grid shows {count} books (expected 2)"
            );
        }
        glib::ControlFlow::Continue
    });
    let _ = &window;
}

/// D + E. The big-folder gates: the scan fills the grid WHILE it
/// walks; Abort Scanning keeps the partial landing; the re-scan
/// completes.
const BIG_COUNT: usize = 10_000;

fn gate_d(shell: &cr_ui::browser::shell::BrowserShell) {
    // The big folder: zero-byte .cbz files (the comic check is the
    // extension; the provider open fails fast — the walk cost is the
    // gate's subject, not the decode).
    let big = std::path::Path::new("/tmp/opencode/scanrefresh/big");
    let _ = std::fs::remove_dir_all(big);
    std::fs::create_dir_all(big).unwrap();
    for i in 0..BIG_COUNT {
        std::fs::write(big.join(format!("bulk{i:05}.cbz")), b"").unwrap();
    }
    {
        let lib = cr_ui::library::session();
        lib.borrow_mut()
            .add_watch_folder(&big.to_string_lossy(), true);
    }
    let window = shell.window();
    let _ = gtk4::prelude::WidgetExt::activate_action(&window, "win.scan-folders", None);
    let ticks = Rc::new(Cell::new(0u32));
    glib::timeout_add_local(std::time::Duration::from_millis(25), {
        let shell = shell.clone();
        move || {
            ticks.set(ticks.get() + 1);
            let scanning = cr_ui::library::is_scanning();
            let count = shell.state_grid_book_count();
            if scanning && count >= 5 {
                // The grid filled WHILE the scan walks — the progressive
                // fill works. Abort now (the Tasks "Abort Scanning").
                println!("D ok: grid filled mid-scan ({count} books, scanning) — aborting");
                cr_ui::library::abort_scan();
                gate_d_wait(shell.clone(), 0);
                return glib::ControlFlow::Break;
            }
            if !scanning {
                panic!(
                "D FAIL: the scan finished before the abort could fire (count {count}) — the fixture is too small"
            );
            }
            if ticks.get() > 2000 {
                panic!("D FAIL: no mid-scan fill observed (count {count}, still scanning)");
            }
            glib::ControlFlow::Continue
        }
    });
}

/// D tail: the abort lands — the partial count stays (< BIG_COUNT).
fn gate_d_wait(shell: cr_ui::browser::shell::BrowserShell, ticks: u32) -> glib::ControlFlow {
    let scanning = cr_ui::library::is_scanning();
    let count = shell.state_grid_book_count();
    if !scanning {
        assert!(
            count > 0 && count < BIG_COUNT + 2,
            "D FAIL: after the abort the grid holds {count} books (expected a partial landing, 0 < count < {})",
            BIG_COUNT + 2
        );
        println!("D ok: abort kept {count} of {} books", BIG_COUNT + 2);
        gate_e(&shell);
    } else if ticks > 200 {
        panic!("D FAIL: the scan never landed after the abort (count {count})");
    } else {
        let shell2 = shell.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            gate_d_wait(shell2.clone(), ticks + 1)
        });
    }
    glib::ControlFlow::Break
}

/// E: the re-scan after the abort completes the library.
fn gate_e(shell: &cr_ui::browser::shell::BrowserShell) {
    let window = shell.window();
    let _ = gtk4::prelude::WidgetExt::activate_action(&window, "win.scan-folders", None);
    let ticks = Rc::new(Cell::new(0u32));
    glib::timeout_add_local(std::time::Duration::from_millis(50), {
        let shell = shell.clone();
        move || {
            ticks.set(ticks.get() + 1);
            let scanning = cr_ui::library::is_scanning();
            let count = shell.state_grid_book_count();
            if !scanning && count == BIG_COUNT + 2 {
                println!(
                    "E ok: the re-scan completed the library ({} books)",
                    BIG_COUNT + 2
                );
                gate_g(&shell);
                return glib::ControlFlow::Break;
            }
            if ticks.get() > 400 {
                panic!(
                    "E FAIL: the re-scan after the abort landed (scanning={scanning}) but the grid shows {count} books (expected {})",
                    BIG_COUNT + 2
                );
            }
            glib::ControlFlow::Continue
        }
    });
}

/// G + H. A RE-scan keeps the whole library live mid-scan (the worker
/// scans a CLONE — the take left an empty database for the whole
/// scan, so every evaluation read 0 books and the search results
/// blanked until restart; the user report). Pending watch events do
/// NOT queue a rescan while a scan runs; they deliver once it ends.
const COMICS_DIR: &str = "/tmp/opencode/scanrefresh/comics";

fn find_library_root(items: &[ComicListItem]) -> Option<CrGuid> {
    items.iter().find_map(|i| match i {
        ComicListItem::Library(_) => Some(i.base().id),
        _ => None,
    })
}

fn find_named_list(items: &[ComicListItem], name: &str) -> Option<CrGuid> {
    for i in items {
        if let ComicListItem::Folder(f) = i {
            if let Some(id) = find_named_list(&f.items, name) {
                return Some(id);
            }
        } else if i.base().name.as_deref() == Some(name) {
            return Some(i.base().id);
        }
    }
    None
}

fn gate_g(shell: &cr_ui::browser::shell::BrowserShell) {
    let comics = Path::new(COMICS_DIR);
    // Two NEW files → watch events, delivered well before the scan
    // starts (the dedupe guard must HOLD them mid-scan, not drop
    // them).
    write_page_zip(&comics.join("Scanned C 003.cbz"));
    write_page_zip(&comics.join("Scanned D 004.cbz"));
    glib::timeout_add_local(std::time::Duration::from_millis(800), {
        let shell = shell.clone();
        move || {
            let snapshot = cr_ui::library::comic_lists_snapshot();
            let never_read =
                find_named_list(&snapshot, "Never Read").expect("the default Never Read list");
            let (_, pre_books) = cr_ui::library::evaluate_books(&never_read).expect("evaluate");
            let pre = pre_books.len();
            assert_eq!(
                pre,
                shell.state_grid_book_count(),
                "G FAIL: the Library grid and the Never Read evaluation disagree pre-scan"
            );
            let window = shell.window();
            let _ = gtk4::prelude::WidgetExt::activate_action(&window, "win.scan-folders", None);
            let ticks = Rc::new(Cell::new(0u32));
            let checked = Rc::new(Cell::new(false));
            glib::timeout_add_local(std::time::Duration::from_millis(25), {
                let shell = shell.clone();
                let ticks = ticks.clone();
                let checked = checked.clone();
                move || {
                    ticks.set(ticks.get() + 1);
                    let scanning = cr_ui::library::is_scanning();
                    let count = shell.state_grid_book_count();
                    if scanning {
                        if !checked.get() {
                            checked.set(true);
                            // H: the pending watch events do NOT queue
                            // a rescan while this scan runs.
                            let rescans = cr_ui::library::take_watch_folder_rescans();
                            assert!(
                                rescans.is_empty(),
                                "H FAIL: a rescan queued mid-scan ({rescans:?})"
                            );
                            // G: the whole library is live mid-scan —
                            // the smart-list evaluation sees the FULL
                            // stored set (the take read 0 here).
                            let (_, books) =
                                cr_ui::library::evaluate_books(&never_read).expect("evaluate");
                            assert!(
                                books.len() >= pre,
                                "G FAIL: the mid-scan evaluation holds {} books (expected >= {pre}) — the database was emptied by the scan",
                                books.len()
                            );
                            // G: exercise the REAL refresh path (the
                            // watch-poll landing shape: refresh + the
                            // debounced navigator select).
                            shell.refresh_after_data_change();
                            println!(
                                "G: mid-scan checks ok (grid {count}, evaluation {})",
                                books.len()
                            );
                        }
                        // The non-blank invariant through the debounced
                        // select and the append batches.
                        assert!(
                            count >= pre,
                            "G FAIL: the grid dropped to {count} books mid-scan (expected >= {pre}) — the view blanked"
                        );
                        if ticks.get() > 2000 {
                            panic!("G FAIL: the re-scan never landed (count={count})");
                        }
                        glib::ControlFlow::Continue
                    } else {
                        assert_eq!(
                            count,
                            pre + 2,
                            "G FAIL: the landing holds {count} books (expected {})",
                            pre + 2
                        );
                        // H: the events held mid-scan deliver now.
                        let rescans = cr_ui::library::take_watch_folder_rescans();
                        assert!(
                            rescans.len() == 1 && rescans[0] == COMICS_DIR,
                            "H FAIL: the held events did not deliver after the scan ({rescans:?})"
                        );
                        println!("G ok: the library stayed live mid-scan; H ok: the rescan was held, then delivered");
                        gate_i(&shell, &never_read);
                        glib::ControlFlow::Break
                    }
                }
            });
            glib::ControlFlow::Break
        }
    });
}

/// I. A non-Library view (the Never Read smart list) does not churn
/// per batch tick: the count holds while the scan runs (no per-tick
/// full refresh, no appends on a non-Library view) and updates ONCE
/// at the landing.
fn gate_i(shell: &cr_ui::browser::shell::BrowserShell, never_read: &CrGuid) {
    let comics = Path::new(COMICS_DIR);
    for i in 0..50 {
        write_page_zip(&comics.join(format!("Bulk{i:03}.cbz")));
    }
    let (_, pre_books) = cr_ui::library::evaluate_books(never_read).expect("evaluate");
    let pre = pre_books.len();
    // Switch through the REAL selection path (the debounced select).
    shell.state_select_list(never_read);
    glib::timeout_add_local(std::time::Duration::from_millis(400), {
        let shell = shell.clone();
        move || {
            assert_eq!(
                shell.state_grid_book_count(),
                pre,
                "I FAIL: the Never Read view did not fill"
            );
            let window = shell.window();
            let _ = gtk4::prelude::WidgetExt::activate_action(&window, "win.scan-folders", None);
            let ticks = Rc::new(Cell::new(0u32));
            glib::timeout_add_local(std::time::Duration::from_millis(25), {
                let shell = shell.clone();
                let ticks = ticks.clone();
                move || {
                    ticks.set(ticks.get() + 1);
                    let scanning = cr_ui::library::is_scanning();
                    let count = shell.state_grid_book_count();
                    if scanning {
                        // The scan runs one leg per watch root: a leg
                        // LANDING legitimately refreshes this view
                        // once (a step to the post-landing total). A
                        // per-tick refresh would churn through
                        // INTERMEDIATE values (20 books per batch) —
                        // only the two stable totals may appear.
                        assert!(
                            count == pre || count == pre + 50,
                            "I FAIL: the non-Library view churned mid-scan ({count} books, expected {pre} or {}) — a per-tick refresh ran",
                            pre + 50
                        );
                        if ticks.get() > 2000 {
                            panic!("I FAIL: the scan never landed");
                        }
                        glib::ControlFlow::Continue
                    } else {
                        assert_eq!(
                            count,
                            pre + 50,
                            "I FAIL: the landing holds {count} books (expected {})",
                            pre + 50
                        );
                        println!(
                            "I ok: the smart-list view held {pre} mid-scan and landed at {count}"
                        );
                        gate_j(&shell);
                        glib::ControlFlow::Break
                    }
                }
            });
            glib::ControlFlow::Break
        }
    });
}

/// J. What the user does MID-SCAN survives the landing merge: a
/// removed book stays removed, an edited book keeps the edit (the
/// wholesale replace resurrected both).
fn gate_j(shell: &cr_ui::browser::shell::BrowserShell) {
    let root_id = find_library_root(&cr_ui::library::comic_lists_snapshot())
        .expect("the Library root exists");
    // Back to the Library view (F's count expectations ride on it).
    shell.state_select_list(&root_id);
    glib::timeout_add_local(std::time::Duration::from_millis(400), {
        let shell = shell.clone();
        move || {
            let (_, books) = cr_ui::library::evaluate_books(&root_id).expect("evaluate");
            let remove_id = books[0].id;
            let edit_id = books[1].id;
            let window = shell.window();
            let _ = gtk4::prelude::WidgetExt::activate_action(&window, "win.scan-folders", None);
            let ticks = Rc::new(Cell::new(0u32));
            let acted = Rc::new(Cell::new(false));
            glib::timeout_add_local(std::time::Duration::from_millis(25), {
                let shell = shell.clone();
                let ticks = ticks.clone();
                let acted = acted.clone();
                move || {
                    ticks.set(ticks.get() + 1);
                    let scanning = cr_ui::library::is_scanning();
                    if scanning && !acted.get() {
                        acted.set(true);
                        // Mid-scan: remove one book, edit another,
                        // abort the scan.
                        cr_ui::library::remove_book(&remove_id);
                        let (_, books) =
                            cr_ui::library::evaluate_books(&root_id).expect("evaluate");
                        let mut edited = books
                            .iter()
                            .find(|b| b.id == edit_id)
                            .expect("the edit target")
                            .clone();
                        edited.info.title = "JEdit Survives".into();
                        assert!(cr_ui::library::apply_edited(&edited));
                        cr_ui::library::abort_scan();
                        return glib::ControlFlow::Continue;
                    }
                    if !scanning {
                        let count = shell.state_grid_book_count();
                        let (_, books) =
                            cr_ui::library::evaluate_books(&root_id).expect("evaluate");
                        assert!(
                            !books.iter().any(|b| b.id == remove_id),
                            "J FAIL: the removed book was resurrected by the landing merge"
                        );
                        assert_eq!(
                            books
                                .iter()
                                .find(|b| b.id == edit_id)
                                .map(|b| b.info.title.clone())
                                .as_deref(),
                            Some("JEdit Survives"),
                            "J FAIL: the mid-scan edit was lost at the landing merge"
                        );
                        assert_eq!(
                            count,
                            books.len(),
                            "J FAIL: the grid ({count}) and the library ({}) disagree",
                            books.len()
                        );
                        println!(
                            "J ok: the mid-scan remove + edit survived the merge ({count} books)"
                        );
                        gate_f(&shell);
                        return glib::ControlFlow::Break;
                    }
                    if ticks.get() > 400 {
                        panic!("J FAIL: the abort never landed");
                    }
                    glib::ControlFlow::Continue
                }
            });
            glib::ControlFlow::Break
        }
    });
}

/// F: quitting MID-SCAN saves the partial library (the C#
/// Stop-before-Save order). A FRESH folder (5000 REAL one-page zips —
/// the provider open stretches the walk past a poll tick; zero-byte
/// files scan between two ticks) starts landing; closing the window
/// mid-add aborts the scan, merges the partial and writes the DB;
/// the app run loop exits and `main` verifies the saved file. The
/// trigger rides the LIVE count base (the gates before F grew the
/// library — a fixed threshold would fire before the adds land).
const EXTRA_COUNT: usize = 5_000;
static F_BASE: AtomicUsize = AtomicUsize::new(0);

fn gate_f(shell: &cr_ui::browser::shell::BrowserShell) {
    let extra = std::path::Path::new("/tmp/opencode/scanrefresh/big2");
    let _ = std::fs::remove_dir_all(extra);
    std::fs::create_dir_all(extra).unwrap();
    for i in 0..EXTRA_COUNT {
        write_page_zip(&extra.join(format!("more{i:05}.cbz")));
    }
    {
        let lib = cr_ui::library::session();
        lib.borrow_mut()
            .add_watch_folder(&extra.to_string_lossy(), true);
    }
    let base = shell.state_grid_book_count();
    F_BASE.store(base, Ordering::Relaxed);
    let window = shell.window();
    let _ = gtk4::prelude::WidgetExt::activate_action(&window, "win.scan-folders", None);
    let ticks = Rc::new(Cell::new(0u32));
    glib::timeout_add_local(std::time::Duration::from_millis(25), {
        let shell = shell.clone();
        move || {
            ticks.set(ticks.get() + 1);
            let scanning = cr_ui::library::is_scanning();
            let count = shell.state_grid_book_count();
            // New books are landing — close while the fresh scan adds.
            if scanning && count >= base + 22 {
                println!("F: mid-add ({count} books, base {base}) — closing the window (the graceful exit)");
                shell.window().close();
                return glib::ControlFlow::Break;
            }
            if !scanning {
                panic!(
                    "F FAIL: the scan finished before the close could land mid-add (count {count}, base {base})"
                );
            }
            if ticks.get() > 2000 {
                panic!("F FAIL: no mid-add fill observed (count {count}, base {base})");
            }
            glib::ControlFlow::Continue
        }
    });
}
