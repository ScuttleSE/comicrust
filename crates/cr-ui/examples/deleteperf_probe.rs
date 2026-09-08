//! Headless timing probe for the T4 fileless-book delete hang (the
//! user report: ~200 empty books, delete, ~1 min hang). Reproduces
//! the user's scenario when the fixtures exist: the real-world
//! 255-book ComicDb + the chronology `.cbl` imported through the
//! REAL flow (the missing-books question answered with Add
//! missing), then ~250 fileless placeholders selected and removed
//! through the real context menu, the confirm dialog, the per-id
//! loop + refresh. Falls back to the synthetic scenario (55 metadata
//! books and 200 fileless) without the fixtures. The gate: the whole
//! flow stays in seconds, and the grid/DB counts drop by the
//! selection.
//!
//! Run: Xvfb + `cargo run -p cr-ui --release --example deleteperf_probe`
//! with an isolated XDG (the probe seeds books into the DB it opens).
use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::{CrDateTime, CrGuid};
use gtk4::prelude::*;
use gtk4::{glib, Button, Dialog};
use std::time::Instant;

const CHRONOLOGY_CBL: &str = "tests/testfiles/[Spider-Man] 00 - Complete 616 Chronology.cbl";
const REALWORLD_DB: &str = "tests/realworld/ComicDb.xml";

fn find_toplevel(title: &str) -> Option<gtk4::Window> {
    gtk4::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk4::Window>().ok())
        .find(|w| w.title().as_deref().is_some_and(|t| t.starts_with(title)))
}

fn walk(widget: &gtk4::Widget, out: &mut Vec<gtk4::Widget>) {
    out.push(widget.clone());
    let mut child = widget.first_child();
    while let Some(c) = child {
        walk(&c, out);
        child = c.next_sibling();
    }
}

/// The "Remove from Library" button inside the context popover.
fn find_button(root: &gtk4::Widget, label: &str) -> Option<Button> {
    let mut all = Vec::new();
    walk(root, &mut all);
    all.into_iter()
        .filter_map(|w| w.downcast::<Button>().ok())
        .find(|b| {
            b.label()
                .or_else(|| b.child().and_downcast::<gtk4::Label>().map(|l| l.text()))
                .is_some_and(|t| t.contains(label))
        })
}

fn db_book_count() -> usize {
    let lib = cr_ui::library::session();
    let l = lib.borrow();
    l.database().books.len()
}

fn db_fileless_ids() -> Vec<CrGuid> {
    let lib = cr_ui::library::session();
    let l = lib.borrow();
    l.database()
        .books
        .iter()
        .filter(|b| b.file_path.is_empty())
        .map(|b| b.id)
        .collect()
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
        eprintln!("REFUSED: set XDG_DATA_HOME=/tmp/opencode/<dir> AND XDG_CONFIG_HOME=/tmp/opencode/<dir> (the probe seeds books and can write Config.xml)");
        std::process::exit(1);
    }
    let work = std::path::Path::new("/tmp/opencode/deleteperf");
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).unwrap();

    // The real-world 255-book database when present (the user's
    // library scale): copy it to the XDG library location before
    // the session opens.
    let real_db = std::path::Path::new(REALWORLD_DB);
    let has_real_db = real_db.exists();
    if has_real_db {
        let xdg = std::env::var("XDG_DATA_HOME").unwrap();
        let db_dir = std::path::Path::new(&xdg).join("comicrust").join("ComicDb");
        std::fs::create_dir_all(&db_dir).unwrap();
        std::fs::copy(real_db, db_dir.join("ComicDb.xml")).unwrap();
    }
    cr_ui::library::initialize().expect("session init");

    let synthetic = !has_real_db;
    let cbl = std::path::Path::new(CHRONOLOGY_CBL);
    let has_cbl = cbl.exists();
    let t = Instant::now();
    if synthetic {
        // Fallback: 55 metadata books + 200 fileless books.
        for i in 0..55 {
            let mut b = ComicBook {
                id: CrGuid::new_random(),
                file_path: format!("/comics/real-{i}.cbz"),
                added_time: CrDateTime::now(),
                ..Default::default()
            };
            b.info.series = format!("Keep Series {}", i % 7);
            b.info.number = format!("{}", 1 + i);
            b.info.page_count = 24;
            b.info.writer = "John Writer; Jane Penciller".into();
            assert!(cr_ui::library::insert_new_book(&b));
        }
        for n in 0..200 {
            let mut b = cr_ui::dialogs::new_book_series::new_fileless_book();
            b.info.series = "Delete Probe Series".into();
            b.info.number = n.to_string();
            assert!(cr_ui::library::insert_new_book(&b));
        }
    }
    println!(
        "seed: db={} real-db={has_real_db} chronology={has_cbl} {} ms",
        db_book_count(),
        t.elapsed().as_millis()
    );

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.deleteperf-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let app = app.clone();
        let (window, shell) = cr_ui::browser::shell::BrowserShell::create(&app);
        window.present();
        std::mem::forget(shell.clone());

        if has_cbl {
            // The real import flow; the missing-books question gets
            // the Add-missing answer (the user's flow).
            glib::timeout_add_local(std::time::Duration::from_millis(600), {
                let window = window.clone();
                let shell = shell.clone();
                let cbl = cbl.to_path_buf();
                move || {
                    let nav = shell.navigator();
                    cr_ui::dialogs::import_list::import_list_file(
                        &window,
                        &cbl,
                        None,
                        &nav,
                        |_| {},
                    );
                    glib::ControlFlow::Break
                }
            });
            glib::timeout_add_local(std::time::Duration::from_millis(1400), {
                move || {
                    if let Some(dlg) =
                        find_toplevel("Import").and_then(|w| w.downcast::<Dialog>().ok())
                    {
                        dlg.response(gtk4::ResponseType::Apply);
                        println!("import question answered: add missing");
                    } else {
                        println!("import question absent (all solved?)");
                    }
                    glib::ControlFlow::Break
                }
            });
        }

        // A. Select the fileless (placeholder) books — the user's
        //    ~200-tagged selection, capped at 250.
        glib::timeout_add_local(std::time::Duration::from_millis(2600), {
            let app = app.clone();
            let shell = shell.clone();
            move || {
                // Stage breakdown of the coming refresh (the same
                // pieces refresh_view_from_list runs).
                if let Some((id, _name)) = shell.navigator().current_selection() {
                    let t = Instant::now();
                    let books = cr_ui::library::evaluate_books(&id)
                        .map(|(_, b)| b)
                        .unwrap_or_default();
                    println!(
                        "A0 evaluate: {} ms — {} books",
                        t.elapsed().as_millis(),
                        books.len()
                    );
                }
                let ids: Vec<CrGuid> = db_fileless_ids().into_iter().take(250).collect();
                let t = Instant::now();
                shell.state_reselect(&ids);
                println!(
                    "A select n={} {} ms",
                    shell.state_grid_selection_len(),
                    t.elapsed().as_millis()
                );
                if shell.state_grid_selection_len() == 0 {
                    eprintln!("A FAILED: nothing selected");
                    app.quit();
                }
                glib::ControlFlow::Break
            }
        });

        // B. Open the real context menu.
        glib::timeout_add_local(std::time::Duration::from_millis(3000), {
            let shell = shell.clone();
            move || {
                let t = Instant::now();
                let opened = shell.state_open_context(300.0, 300.0);
                println!("B menu open={opened} {} ms", t.elapsed().as_millis());
                glib::ControlFlow::Break
            }
        });

        // C. Click "Remove from Library" (the confirm dialog opens).
        glib::timeout_add_local(std::time::Duration::from_millis(3400), {
            let shell = shell.clone();
            move || {
                let t = Instant::now();
                let pop = shell.state_context_popover();
                let clicked = pop
                    .as_ref()
                    .and_then(|p| p.child())
                    .and_then(|c| find_button(&c, "Remove from Library"))
                    .map(|b| b.emit_clicked())
                    .is_some();
                println!("C remove click={clicked} {} ms", t.elapsed().as_millis());
                glib::ControlFlow::Break
            }
        });

        // D. Answer the confirm dialog with OK — the response closure
        //    runs the per-id loop + refresh synchronously; the wall
        //    time here IS the user-visible hang.
        glib::timeout_add_local(std::time::Duration::from_millis(3900), {
            let app = app.clone();
            let shell = shell.clone();
            move || {
                let Some(dlg) =
                    find_toplevel("Remove Books").and_then(|w| w.downcast::<Dialog>().ok())
                else {
                    eprintln!("D confirm dialog MISSING");
                    app.quit();
                    return glib::ControlFlow::Break;
                };
                let t = Instant::now();
                dlg.response(gtk4::ResponseType::Ok);
                println!(
                    "D response (remove+refresh) {} ms — grid now {} books, db {} books",
                    t.elapsed().as_millis(),
                    shell.state_grid_book_count(),
                    db_book_count(),
                );
                glib::ControlFlow::Break
            }
        });

        // E. Settle (the follow-up draw frames), then quit — the
        //    quit timing carries the close-request DB save.
        glib::timeout_add_local(std::time::Duration::from_millis(9000), {
            let app = app.clone();
            let shell = shell.clone();
            move || {
                println!(
                    "E settled grid {} db {}",
                    shell.state_grid_book_count(),
                    db_book_count()
                );
                let t = Instant::now();
                app.quit();
                println!("E quit+save {} ms", t.elapsed().as_millis());
                println!("PROBE COMPLETE");
                glib::ControlFlow::Break
            }
        });

        glib::timeout_add_local(std::time::Duration::from_secs(120), {
            let app = app.clone();
            move || {
                eprintln!("TIMEOUT — probe did not finish");
                app.quit();
                glib::ControlFlow::Break
            }
        });
    });

    app.run();
}
