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
    // The probe SEEDS books into whatever DB it opens — refuse a
    // real home (the accidental-run lesson: run with an isolated
    // XDG_DATA_HOME or the user's library gets probe entries).
    if !std::env::var("XDG_DATA_HOME")
        .map(|v| v.contains("/tmp/opencode"))
        .unwrap_or(false)
    {
        eprintln!("REFUSED: set XDG_DATA_HOME=/tmp/opencode/<dir> (the probe seeds books into the DB it opens)");
        std::process::exit(1);
    }
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

        // D2. The live read ribbons: the page turn reaches the
        //     ItemView's book copy through the page-change hook (the
        //     stale-green-ribbon fix: the view's cloned snapshot
        //     follows the session book without a list refresh).
        glib::timeout_add_local(std::time::Duration::from_millis(4000), {
            let shell = shell.clone();
            move || {
                let state = shell.item_view_state();
                let opened = state
                    .books()
                    .iter()
                    .find(|b| b.file_path.ends_with("probe a.cbz"))
                    .map(|b| (b.current_page, b.last_page_read));
                println!(
                    "D2 view-copy read-state={opened:?} (expect Some((1, 1)) — the turn reached the grid)"
                );
                assert_eq!(
                    opened,
                    Some((1, 1)),
                    "the page turn never reached the ItemView's book copy (stale read ribbons)"
                );
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
                bar.update_lamps(cr_ui::library::is_scanning(), cr_ui::library::writes_pending() > 0, true, false);
                let export_on = bar.lamp_visible("export");
                let scan_on = bar.lamp_visible("scan");
                cr_ui::library::set_export_active(false);
                bar.update_lamps(false, false, false, false);
                let export_off = bar.lamp_visible("export");
                println!(
                    "G export-on={export_on} scan-on={scan_on} export-off={export_off} (expect true/false/false)"
                );
                glib::ControlFlow::Break
            }
        });

        // J. The scan lamp: the bundled ScanAnimation frames load,
        //    the frame timer runs ONLY while the lamp shows, and the
        //    click opens the Cancel-scan menu whose row fires the
        //    abort hook (the user-requested menu; the C# lamp opens
        //    Tasks and the abort lives in its scan row).
        glib::timeout_add_local(std::time::Duration::from_millis(7300), {
            let bar = bar.clone();
            move || {
                let frames = bar.scan_frame_count();
                bar.update_lamps(true, false, false, false);
                let visible_on = bar.lamp_visible("scan");
                let anim_on = bar.scan_anim_running();
                bar.update_lamps(false, false, false, false);
                let anim_off = bar.scan_anim_running();
                println!(
                    "J frames={frames} scan-on={visible_on} anim-on={anim_on} anim-off={anim_off} (expect 4/true/true/false)"
                );
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(7700), {
            let bar = bar.clone();
            move || {
                // The lamp must SHOW for the popover to map (an
                // invisible parent cannot host a mapped popover).
                bar.update_lamps(true, false, false, false);
                let fired = std::rc::Rc::new(std::cell::Cell::new(false));
                let flag = fired.clone();
                bar.connect_cancel_scan(move || flag.set(true));
                bar.click_scan_lamp();
                // +250 keeps the reads clear of the 1 s activity
                // poll (it re-hides an idle scan lamp on the second
                // marks and unmaps the popover with it).
                glib::timeout_add_local(std::time::Duration::from_millis(250), {
                    let bar = bar.clone();
                    let fired = fired.clone();
                    move || {
                        let menu_open = bar.cancel_menu_visible();
                        bar.click_cancel_scan();
                        glib::timeout_add_local(std::time::Duration::from_millis(150), {
                            let bar = bar.clone();
                            let fired = fired.clone();
                            move || {
                                println!(
                                    "J2 menu-open={menu_open} menu-closed={} cancel-fired={} (expect true/true/true)",
                                    !bar.cancel_menu_visible(),
                                    fired.get(),
                                );
                                glib::ControlFlow::Break
                            }
                        });
                        glib::ControlFlow::Break
                    }
                });
                glib::ControlFlow::Break
            }
        });

        // K. The Comic Vine cache lamp (ADR-037, ADR-038): it
        //    follows the job slot, its tooltip carries the live job
        //    line, and its menu row fires the abort hook. NO C# item.
        glib::timeout_add_local(std::time::Duration::from_millis(8600), {
            let bar = bar.clone();
            move || {
                let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                let claimed = cr_ui::library::start_cv_job(
                    cr_ui::library::CvJobKind::Sweep,
                    std::sync::Arc::clone(&cancel),
                );
                // A second job must be refused while one runs.
                let second = cr_ui::library::start_cv_job(
                    cr_ui::library::CvJobKind::Warm,
                    std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                );
                cr_ui::library::set_cv_job_progress("page 3 of 53".into(), 300, 5300);
                bar.update_lamps(false, false, false, cr_ui::library::cv_job_active());
                bar.set_cv_job_text(
                    cr_ui::library::cv_job().map(|j| j.text()).as_deref(),
                );
                let lamp_on = bar.lamp_visible("cv");
                let tooltip = bar.cv_job_tooltip().unwrap_or_default();
                println!(
                    "K claimed={claimed} second-refused={} lamp-on={lamp_on} tooltip={tooltip:?} (expect true/true/true/\"...page 3 of 53\")",
                    !second
                );

                // The menu row reaches the worker's cancel flag.
                let fired = std::rc::Rc::new(std::cell::Cell::new(false));
                let flag = fired.clone();
                bar.connect_cancel_cv_job(move || {
                    cr_ui::library::abort_cv_job();
                    flag.set(true);
                });
                bar.click_cv_lamp();
                glib::timeout_add_local(std::time::Duration::from_millis(250), {
                    let bar = bar.clone();
                    let cancel = std::sync::Arc::clone(&cancel);
                    let fired = fired.clone();
                    move || {
                        let menu_open = bar.cv_menu_visible();
                        bar.click_cancel_cv_job();
                        glib::timeout_add_local(std::time::Duration::from_millis(150), {
                            let bar = bar.clone();
                            let cancel = std::sync::Arc::clone(&cancel);
                            let fired = fired.clone();
                            move || {
                                let worker_sees_it =
                                    cancel.load(std::sync::atomic::Ordering::Relaxed);
                                cr_ui::library::end_cv_job();
                                bar.update_lamps(
                                    false,
                                    false,
                                    false,
                                    cr_ui::library::cv_job_active(),
                                );
                                println!(
                                    "K2 menu-open={menu_open} menu-closed={} cancel-fired={} worker-flag={worker_sees_it} lamp-off={} (expect true/true/true/true/true)",
                                    !bar.cv_menu_visible(),
                                    fired.get(),
                                    !bar.lamp_visible("cv"),
                                );
                                glib::ControlFlow::Break
                            }
                        });
                        glib::ControlFlow::Break
                    }
                });
                glib::ControlFlow::Break
            }
        });

        // I. The boot-configure re-entrancy gate (the 2026-09-06
        //    boot crash): a persisted workspace size applied, then
        //    a list selection — the selection notify syncs the
        //    slider to the new size, the value_changed handler
        //    re-enters set_item_size. Before the notify-borrow fix
        //    that was a RefCell double-borrow abort.
        glib::timeout_add_local(std::time::Duration::from_millis(8000), {
            let shell = shell.clone();
            move || {
                let mut ws = shell.state_collect_workspace();
                ws.view.mode = cr_core::model::enums::ItemViewMode::Detail;
                ws.view.row_height = 32;
                shell.state_apply_workspace(&ws);
                shell.navigator().select_list(
                    &cr_ui::library::comic_lists_snapshot()[0].base().id,
                );
                glib::timeout_add_local(std::time::Duration::from_millis(500), {
                    let shell = shell.clone();
                    move || {
                        let size = shell.state_grid_item_size();
                        let mode = shell.state_grid_mode();
                        println!("I boot-configure mode={mode} size={size:?} (expect Detail row 32 — no RefCell abort)");
                        glib::ControlFlow::Break
                    }
                });
                glib::ControlFlow::Break
            }
        });

        // H. MinimalGui (F10's action): the menubar hides; the tab
        //    strip + the status bar ride the C# `flag4` formula —
        //    on the BROWSER workspace they stay visible even in
        //    MinimalGui (`!IsComicViewer` term); restore after a
        //    re-dispatch.
        glib::timeout_add_local(std::time::Duration::from_millis(6200), {
            let shell = shell.clone();
            let bar = bar.clone();
            move || {
                let _ = shell.state_dispatch("win.minimal-gui");
                let minimal_menubar = shell.menubar().widget().is_visible();
                let minimal_status = bar.widget().is_visible();
                let minimal_state = shell.state_action_bool("minimal-gui");
                let _ = shell.state_dispatch("win.minimal-gui");
                let back_menubar = shell.menubar().widget().is_visible();
                let back_status = bar.widget().is_visible();
                println!(
                    "H minimal: menubar={minimal_menubar} status={minimal_status} state={minimal_state:?} | restored: menubar={back_menubar} status={back_status} (expect false/true/Some(true)/true/true)"
                );
                glib::ControlFlow::Break
            }
        });

        // I. The F10 ACCEL (the user report: F10 did nothing —
        //    GTK's capture-phase `handle-menubar-accel` consumed the
        //    key). XTEST the key into the focused window and check
        //    the menubar hides (env-gated: needs xdotool + focus).
        if std::env::var("CR_F10_KEYS").is_ok() {
            glib::timeout_add_local(std::time::Duration::from_millis(6600), {
                let shell = shell.clone();
                let bar = bar.clone();
                move || {
                    let _ = std::process::Command::new("xdotool")
                        .args(["search", "--name", "comicrust"])
                        .args(["windowfocus", "--sync"])
                        .args(["key", "F10"])
                        .status();
                    glib::timeout_add_local(std::time::Duration::from_millis(600), {
                        let shell = shell.clone();
                        let _bar = bar.clone();
                        move || {
                            let menubar = shell.menubar().widget().is_visible();
                            // A known-good accel through the SAME
                            // injection path (F8 = view-pages):
                            // if F6 lands, the path works and an
                            // F10 miss is the code; if F6 also
                            // misses, the injection is the artifact.
                            let page_before = shell.state_visible_page();
                            let _ = std::process::Command::new("xdotool")
                                .args(["search", "--name", "comicrust"])
                                .args(["windowfocus", "--sync"])
                                .args(["key", "F8"])
                                .status();
                            glib::timeout_add_local(std::time::Duration::from_millis(600), {
                                let shell = shell.clone();
                                move || {
                                    let page_after = shell.state_visible_page();
                                    let delivered = page_before != page_after;
                                    if delivered {
                                        println!(
                                            "I F10 accel: menubar={menubar} (F8 control delivered — this run is evidence)"
                                        );
                                    } else {
                                        println!(
                                            "I F10 accel: menubar={menubar} | F8 control UNDELIVERED — the Xvfb injection path is dead, inconclusive (the user test decides)"
                                        );
                                    }
                                    glib::ControlFlow::Break
                                }
                            });
                            glib::ControlFlow::Break
                        }
                    });
                    glib::ControlFlow::Break
                }
            });
        }

        glib::timeout_add_local(std::time::Duration::from_millis(10500), {
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
