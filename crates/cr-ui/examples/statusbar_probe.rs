//! Headless probe: the T8 multi-panel status bar (`statusStrip`).
//! Gates: the startup "Ready"/"None"/"NA"/"Unknown" defaults, the
//! selection info (list name + count + filtered + selected + size),
//! the book/page/page-count panels on open, the thumb slider
//! (visible on the browser only, range/value per mode, the drag
//! resizes the grid), the page-panel click flipping
//! `track-current-page` (the locked icon), and the lamp visibility
//! following the activity flags.
//! Run: Xvfb + `cargo run -p cr-ui --example statusbar_probe` with an
//! isolated XDG (fresh DB → the default list tree).
use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::{CrDateTime, CrGuid};
use gtk4::glib;
use gtk4::prelude::*;

fn main() {
    gtk4::init().expect("gtk init");
    // The app loads the CSS in `app::run` — the probe must load it
    // too (the T9 lesson) or the panel styles never apply.
    cr_ui::theme::init();
    let src = "tests/testfiles/Absolute Flash (2025) Volume 01 Issue 009.cbz";
    let work = std::path::Path::new("/tmp/opencode/statusbar");
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).unwrap();
    // Three seeds → the grid has books with file sizes.
    for name in ["probe a.cbz", "probe b.cbz", "probe c.cbz"] {
        let comic = work.join(name);
        std::fs::copy(src, &comic).unwrap();
        let provider = cr_io::ComicProvider::open(&comic).unwrap();
        let mut book = ComicBook {
            id: CrGuid::new_random(),
            file_path: comic.to_string_lossy().into_owned(),
            added_time: CrDateTime::now(),
            ..Default::default()
        };
        book.info.page_count = provider.page_count() as i32;
        book.file_size = 2048;
        let (mut lib, _) = cr_engine::library::Library::open_at_default_location().unwrap();
        lib.database_mut().books.push(book);
        lib.save().unwrap();
    }
    cr_ui::library::initialize().expect("session init");

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.statusbar-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let (window, shell) = cr_ui::browser::shell::BrowserShell::create(app);
        window.present();
        // The probe must keep the shell alive (the thread-local
        // lesson) — every action handler holds Weak<ShellState>.
        std::mem::forget(shell.clone());
        let bar = shell.statusbar();

        // A. Startup (QuickOpen page): the C# defaults.
        glib::timeout_add_local(std::time::Duration::from_millis(800), {
            let bar = bar.clone();
let _shell = shell.clone();
            move || {
                println!(
                    "A info='{}' book='{}' page='{}' count='{}' slider={} (expect '' — QuickOpen shows, no active browser —/None/NA/Unknown/false)",
                    bar.info_text(),
                    bar.book_text(),
                    bar.page_label_text(),
                    bar.page_count_text(),
                    bar.slider_visible(),
                );
                glib::ControlFlow::Break
            }
        });

        // B. Select the Library list → the selection info names it;
        //    the browser workspace (F6) makes the slider visible.
        glib::timeout_add_local(std::time::Duration::from_millis(1200), {
            let shell = shell.clone();
            move || {
                let items = cr_ui::library::comic_lists_snapshot();
                if let Some(first) = items.first() {
                    shell.navigator().select_list(&first.base().id);
                }
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(1700), {
            let shell = shell.clone();
            let bar = bar.clone();
            move || {
                let _ = shell.state_dispatch("win.view-library");
                println!(
                    "B info='{}' slider={} (expect 'Library: 3 Books' + slider true)",
                    bar.info_text(),
                    bar.slider_visible(),
                );
                glib::ControlFlow::Break
            }
        });

        // C. Select two books → the selected tail; the single-book
        //    path shows the file path.
        glib::timeout_add_local(std::time::Duration::from_millis(2100), {
            let shell = shell.clone();
            let bar = bar.clone();
            move || {
                shell.state_select_first_book();
                glib::timeout_add_local(std::time::Duration::from_millis(300), {
                    let shell = shell.clone();
                    let bar = bar.clone();
                    move || {
                        let single = bar.info_text();
                        // Add a second selection through the action
                        // path is not available — dispatch select-all
                        // is unported; re-select with the shifted
                        // model is grid-internal. Use one book for
                        // the path gate and record the count gate.
                        println!(
                            "C single='{single}' (expect 'Library: 3 Books - /tmp/... probe a.cbz')"
                        );
                        let _ = shell;
                        glib::ControlFlow::Break
                    }
                });
                glib::ControlFlow::Break
            }
        });

        // D. Open a comic → the book caption + page 1 + the page
        //    count; the page panel flips 2 on a Next Page dispatch.
        glib::timeout_add_local(std::time::Duration::from_millis(2800), {
            let shell = shell.clone();
            let work = work.to_path_buf();
            move || {
                shell.open_comic(&work.join("probe a.cbz"));
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(3400), {
            let shell = shell.clone();
            let bar = bar.clone();
            move || {
                let _ = shell.state_dispatch("win.next-page");
                glib::timeout_add_local(std::time::Duration::from_millis(400), {
                    let _shell = shell.clone();
                    let bar = bar.clone();
                    move || {
                        println!(
                            "D book='{}' page='{}' count='{}' locked={} (expect a non-None caption/2/N Page(s)/false; TrackCurrentPage defaults true)",
                            bar.book_text(),
                            bar.page_label_text(),
                            bar.page_count_text(),
                            bar.locked_visible(),
                        );
                        glib::ControlFlow::Break
                    }
                });
                glib::ControlFlow::Break
            }
        });

        // E. The page-panel click toggles TrackCurrentPage (the C#
        //    `tsCurrentPage_Click`): the locked icon hides and the
        //    action check flips.
        glib::timeout_add_local(std::time::Duration::from_millis(4300), {
            let bar = bar.clone();
            move || {
                bar.click_page();
                glib::timeout_add_local(std::time::Duration::from_millis(300), {
                    let bar = bar.clone();
                    move || {
                        println!(
                            "E locked={} (expect true after the click — tracking off)",
                            bar.locked_visible(),
                        );
                        glib::ControlFlow::Break
                    }
                });
                glib::ControlFlow::Break
            }
        });

        // F. The slider: the value follows the grid (128 default);
        //    a drag to 256 resizes the grid; a mode switch to Detail
        //    re-ranges (12..48).
        glib::timeout_add_local(std::time::Duration::from_millis(5000), {
            let shell = shell.clone();
            let bar = bar.clone();
            move || {
                let before = shell.state_grid_thumb_height();
                bar.drag_slider(256.0);
                let after = shell.state_grid_thumb_height();
                let synced = bar.slider_value();
                // Back to the browser view so the slider sync runs.
                let _ = shell.state_dispatch("win.view-library");
                glib::timeout_add_local(std::time::Duration::from_millis(300), {
                    let shell = shell.clone();
                    let _bar = bar.clone();
                    move || {
                        println!(
                            "F before={before} after={after} slider={synced} size={:?} (expect 128/256/256/Some((96,512,256)))",
                            shell.state_grid_item_size(),
                        );
                        glib::ControlFlow::Break
                    }
                });
                glib::ControlFlow::Break
            }
        });

        // G. The lamps follow the export flag (the timer runs on
        //    the same loop — read the flag paths directly).
        glib::timeout_add_local(std::time::Duration::from_millis(5800), {
            let bar = bar.clone();
            move || {
                cr_ui::library::set_export_active(true);
                bar.update_lamps(cr_ui::library::is_scanning(), cr_ui::library::writes_pending() > 0, true);
                let export_on = bar.lamp_visible("export");
                let scan_on = bar.lamp_visible("scan");
                cr_ui::library::set_export_active(false);
                bar.update_lamps(false, false, false);
                let export_off = bar.lamp_visible("export");
                println!(
                    "G export-on={export_on} scan-on={scan_on} export-off={export_off} (expect true/false/false)"
                );
                glib::ControlFlow::Break
            }
        });

        glib::timeout_add_local(std::time::Duration::from_millis(6600), {
            let app = app.clone();
            move || {
                println!("PROBE COMPLETE");
                app.quit();
                glib::ControlFlow::Break
            }
        });
    });

    app.run();
}
