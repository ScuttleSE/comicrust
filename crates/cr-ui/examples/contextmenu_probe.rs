//! Headless probe: the three user-test findings (Phase 6 round 1).
//! Gates: the right-click context menu does NOT reset the grid
//! scroll (the vadjustment value survives the menu open), the
//! right-click selection rule (an unselected target replaces the
//! selection; a selected target keeps the multi-selection — the C#
//! `UpdateSelectionFromMouse`), Copy Page leaves a texture on the
//! clipboard (read-back), and Export Page's accept path writes the
//! file (the chooser driven through the real response path with a
//! temp folder).
//! Run: Xvfb + `cargo run -p cr-ui --example contextmenu_probe` with
//! an isolated XDG.
use gtk4::glib;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

use cr_core::xml::scalar::CrGuid;

fn main() {
    gtk4::init().expect("gtk init");
    cr_ui::theme::init();
    if !std::env::var("XDG_DATA_HOME")
        .map(|v| v.contains("/tmp/opencode"))
        .unwrap_or(false)
    {
        eprintln!("REFUSED: set XDG_DATA_HOME=/tmp/opencode/<dir>");
        std::process::exit(1);
    }
    cr_ui::library::initialize().expect("session init");

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.contextmenu-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let (window, shell) = cr_ui::browser::shell::BrowserShell::create(app);
        window.present();
        std::mem::forget(shell.clone());

        // A. Seed 40 fileless books, show the browser workspace, and
        //    select the Library list. The first three ids feed the
        //    selection gate.
        let seeded: Rc<RefCell<Vec<CrGuid>>> = Rc::new(RefCell::new(Vec::new()));
        glib::timeout_add_local(std::time::Duration::from_millis(800), {
            let seeded = seeded.clone();
            let shell = shell.clone();
            move || {
                for n in 0..40 {
                    let mut book = cr_ui::dialogs::new_book_series::new_fileless_book();
                    book.info.series = "Scroll Probe".into();
                    book.info.number = n.to_string();
                    cr_ui::library::insert_new_book(&book);
                    if n < 3 {
                        seeded.borrow_mut().push(book.id);
                    }
                }
                let _ = shell.state_dispatch("win.view-library");
                let lists = cr_ui::library::comic_lists_snapshot();
                shell.navigator().select_list(&lists[0].base().id);
                // The inserts need one re-evaluation to reach the grid
                // (the debounced fill — scanmarker_probe does the same
                // before reading draw counters).
                shell.refresh_after_data_change();
                glib::ControlFlow::Break
            }
        });

        // B. Scroll down, fire the right-click through the shared
        //    gesture body, and check the scroll value survives.
        glib::timeout_add_local(std::time::Duration::from_millis(1600), {
            let shell = shell.clone();
            move || {
                let scrolled = shell.state_item_scroll_to(500.0);
                let before = shell.state_item_scroll_value();
                shell.state_trigger_context(200.0, 200.0);
                let after = shell.state_item_scroll_value();
                println!("SCROLL set={scrolled} before={before} after-press={after}");
                glib::timeout_add_local(std::time::Duration::from_millis(400), {
                    let shell = shell.clone();
                    move || {
                        let later = shell.state_item_scroll_value();
                        println!("SCROLL after-menu={later} (expect {before})");
                        glib::ControlFlow::Break
                    }
                });
                glib::ControlFlow::Break
            }
        });

        // S. The right-click selection rule (the C#
        //    `UpdateSelectionFromMouse`): an unselected target
        //    REPLACES the selection; a selected target KEEPS the
        //    multi-selection. The menu commands read the selection.
        glib::timeout_add_local(std::time::Duration::from_millis(2300), {
            let shell = shell.clone();
            let seeded = seeded.clone();
            move || {
                let ids = seeded.borrow().clone();
                let (a, b, c) = (ids[0], ids[1], ids[2]);
                // 1. A+B selected, right-click C (unselected) → only C.
                shell.state_reselect(&[a, b]);
                let (x, y) = shell
                    .state_book_center(&c)
                    .expect("S FAIL: book C has no placed rect");
                shell.state_trigger_context(x, y);
                let sel = shell.state_grid_selection_ids();
                assert_eq!(
                    sel.len(),
                    1,
                    "S FAIL: right-click on an unselected book must replace the selection, got {sel:?}"
                );
                assert_eq!(sel[0], c, "S FAIL: the selection must be the right-clicked book");
                println!("S1 ok: right-click on an unselected book selects only it");

                // 2. A+B selected, right-click B (selected) → A+B kept.
                shell.state_reselect(&[a, b]);
                let (x, y) = shell
                    .state_book_center(&b)
                    .expect("S FAIL: book B has no placed rect");
                shell.state_trigger_context(x, y);
                let sel = shell.state_grid_selection_ids();
                assert_eq!(
                    sel.len(),
                    2,
                    "S FAIL: right-click on a selected book must keep the multi-selection, got {sel:?}"
                );
                assert!(
                    sel.contains(&a) && sel.contains(&b),
                    "S FAIL: the selection must still hold A and B, got {sel:?}"
                );
                println!("S2 ok: right-click on a selected book keeps the selection");

                // Close the menu so the later gates start clean.
                if let Some(p) = shell.state_context_popover() {
                    p.popdown();
                }
                glib::ControlFlow::Break
            }
        });

        // C. Copy Page: open a comic, wait for the composition, copy,
        //    then read the clipboard back.
        glib::timeout_add_local(std::time::Duration::from_millis(2600), {
            let shell = shell.clone();
            move || {
                let work = std::path::Path::new("/tmp/opencode/contextmenu");
                let _ = std::fs::remove_dir_all(work);
                std::fs::create_dir_all(work).unwrap();
                let src = std::path::Path::new(
                    "tests/testfiles/Absolute Flash (2025) Volume 01 Issue 009.cbz",
                );
                let comic = work.join("probe.cbz");
                std::fs::copy(src, &comic).unwrap();
                shell.open_comic(&comic);
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(8000), {
            let window = window.clone();
            let shell = shell.clone();
            move || {
                println!("PAGE IMAGE size={:?}", shell.state_page_image_size());
                let fired =
                    gtk4::prelude::WidgetExt::activate_action(&window, "win.copy-page", None);
                println!("COPY dispatch {fired:?}");
                glib::timeout_add_local(std::time::Duration::from_millis(300), {
                    move || {
                        glib::spawn_future_local(async move {
                            let display = gtk4::gdk::Display::default().unwrap();
                            let clipboard = display.clipboard();
                            match clipboard.read_texture_future().await {
                                Ok(Some(t)) => println!(
                                    "CLIPBOARD READ-BACK texture {}x{}",
                                    t.width(),
                                    t.height()
                                ),
                                Ok(None) => println!("CLIPBOARD READ-BACK none"),
                                Err(err) => println!("CLIPBOARD READ-BACK error: {err}"),
                            }
                            glib::ControlFlow::Continue
                        });
                        glib::ControlFlow::Break
                    }
                });
                glib::ControlFlow::Break
            }
        });

        // D. Export Page accept-path: choose the temp folder, accept,
        //    expect the file on disk.
        glib::timeout_add_local(std::time::Duration::from_millis(9200), {
            let window = window.clone();
            move || {
                let _ = gtk4::prelude::WidgetExt::activate_action(&window, "win.export-page", None);
                glib::timeout_add_local(std::time::Duration::from_millis(600), {
                    move || {
                        let chooser = gtk4::Window::list_toplevels()
                            .into_iter()
                            .find_map(|w| w.downcast::<gtk4::FileChooserDialog>().ok());
                        match chooser {
                            Some(dlg) => {
                                let out = std::path::Path::new("/tmp/opencode/contextmenu");
                                let _ =
                                    dlg.set_current_folder(Some(&gtk4::gio::File::for_path(out)));
                                dlg.response(gtk4::ResponseType::Accept);
                                glib::timeout_add_local(
                                    std::time::Duration::from_millis(600),
                                    move || {
                                        // The write location is the
                                        // chooser's folder (the default is
                                        // home) — the authoritative gate is
                                        // the [trace] "export-page: file
                                        // written" line.
                                        println!("EXPORT accept path ran");
                                        println!("CONTEXTMENU PROBE DONE");
                                        glib::ControlFlow::Break
                                    },
                                );
                            }
                            None => {
                                println!("EXPORT chooser MISSING");
                                println!("CONTEXTMENU PROBE DONE");
                            }
                        }
                        glib::ControlFlow::Break
                    }
                });
                glib::ControlFlow::Break
            }
        });

        glib::timeout_add_local(std::time::Duration::from_secs(30), || {
            eprintln!("TIMEOUT — probe did not finish");
            std::process::exit(2);
        });
    });

    app.run();
}
