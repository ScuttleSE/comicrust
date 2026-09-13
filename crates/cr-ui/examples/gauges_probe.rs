//! Headless gate: the Library-tree gauges (`LibraryTreeSkin.
//! DrawNodeLabel`/`DrawMarkers` — the number badges after each list
//! name). Seeds an isolated library with known read/unread/new books,
//! boots the real shell, and gates:
//!   A  the startup pass fills Total/Unread/New for the Library root,
//!      a folder (combined from children), and two reading lists
//!   E  the `LibraryGaugesFormat` flag matrix (New off → merges into
//!      the Unread badge)
//!   D  `DisplayLibraryGauges=false` hides every badge
//!   B  a read commit moves a book out of New
//!   C  a delete drops the counters
//! Run with an isolated XDG: XDG_DATA_HOME=/tmp/opencode/... cargo
//! run -p cr-ui --release --example gauges_probe
use cr_core::database::list_items::{ComicListItem, FolderItem, IdListItem, ListItemBase};
use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::{CrDateTime, CrGuid};
use gtk4::glib;
use gtk4::prelude::*;

const GAUGES_ALL: u32 = 0x1007;
const GAUGES_NO_NEW: u32 = 0x1006;

fn seed_book(series: &str, read: Option<(i32, i32)>, added_days_ago: f64) -> ComicBook {
    let mut book = ComicBook {
        id: CrGuid::new_random(),
        file_path: format!("/comics/{series}.cbz"),
        added_time: CrDateTime::now(),
        ..Default::default()
    };
    book.info.series = series.into();
    if let Some((last, count)) = read {
        book.last_page_read = last;
        book.info.page_count = count;
    }
    // Pull AddedTime back by the wanted age (the classification
    // window is 14 days — `IsRecentInDays`).
    book.added_time.naive -=
        chrono::Duration::milliseconds((added_days_ago * 86400.0 * 1000.0) as i64);
    book
}

fn id_list(name: &str, ids: &[CrGuid]) -> ComicListItem {
    ComicListItem::IdList(IdListItem {
        base: ListItemBase {
            id: CrGuid::new_random(),
            name: Some(name.into()),
            ..Default::default()
        },
        book_ids: ids.to_vec(),
    })
}

fn main() {
    gtk4::init().expect("gtk init");
    cr_ui::theme::init();
    if !std::env::var("XDG_DATA_HOME")
        .map(|v| v.contains("/tmp/opencode"))
        .unwrap_or(false)
    {
        eprintln!("REFUSED: set XDG_DATA_HOME=/tmp/opencode/<dir> (the probe seeds books into the DB it opens)");
        std::process::exit(1);
    }
    let work = std::path::Path::new("/tmp/opencode/gauges");
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).unwrap();
    // A CRASHED EARLIER RUN would leave a seeded database here (its
    // lists would duplicate the fresh seed and the by-name row lookup
    // would find the stale copy). The seed must start clean.
    let db_dir = std::path::Path::new("/tmp/opencode/gauges-xdg/comicrust");
    if db_dir.exists() {
        eprintln!("removing the leftover database from an earlier run");
        std::fs::remove_dir_all(db_dir).unwrap();
    }

    // Six books: b1 read; b2 + b5 unread-fresh (New); b3, b4, b6
    // unread-old (Unread).
    let b1 = seed_book("g-read", Some((19, 20)), 1.0);
    let b2 = seed_book("g-new", None, 1.0);
    let b3 = seed_book("g-old1", None, 30.0);
    let b4 = seed_book("g-old2", None, 30.0);
    let b5 = seed_book("g-new2", None, 1.0);
    let b6 = seed_book("g-old3", None, 30.0);
    let (b2_id,) = (b2.id,);

    // A = [b1, b2, b3] → total 3, new 1, unread 1
    // B = [b2, b4]      → total 2, new 1, unread 0
    // Folder F(Or)[A, B] → total 4, new 1, unread 1
    // Library root      → total 6, new 1, unread 4
    let a = id_list("gauge A", &[b1.id, b2.id, b3.id]);
    let b = id_list("gauge B", &[b2.id, b4.id]);
    let folder = ComicListItem::Folder(FolderItem {
        base: ListItemBase {
            id: CrGuid::new_random(),
            name: Some("gauge folder".into()),
            ..Default::default()
        },
        items: vec![a, b],
        ..Default::default()
    });

    let (mut lib, _) = cr_engine::library::Library::open_at_default_location().unwrap();
    lib.database_mut().books = vec![b1, b2, b3, b4, b5, b6];
    lib.database_mut().comic_lists.push(folder);
    lib.save().unwrap();
    cr_ui::library::initialize().expect("session init");

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.gauges-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let app = app.clone();
        let (window, shell) = cr_ui::browser::shell::BrowserShell::create(&app);
        window.present();
        std::mem::forget(shell.clone());

        // (name, badge expectations) — every entry: (color-fragment,
        // count) that MUST appear in the row's label; the negative
        // gates assert whole fragments absent.
        fn check_rows(
            shell: &cr_ui::browser::shell::BrowserShell,
            want: &[(&str, &[&str])],
            absent: &[(&str, &[&str])],
        ) -> bool {
            let labels = shell.navigator().row_labels();
            let mut ok = true;
            for (name, fragments) in want {
                let Some((_, label)) = labels.iter().find(|(n, _)| n == name) else {
                    println!("FAIL row {name} absent");
                    ok = false;
                    continue;
                };
                for f in *fragments {
                    if !label.contains(f) {
                        println!("FAIL row {name}: missing {f:?} in {label:?}");
                        ok = false;
                    }
                }
            }
            for (name, fragments) in absent {
                if let Some((_, label)) = labels.iter().find(|(n, _)| n == name) {
                    for f in *fragments {
                        if label.contains(f) {
                            println!("FAIL row {name}: {f:?} must be absent: {label:?}");
                            ok = false;
                        }
                    }
                }
            }
            ok
        }

        /// Waits for one completed refresh run started after
        /// `baseline` (the debounce + per-node ticks + the run-end
        /// counter). `baseline` is captured BEFORE the mutation.
        fn wait_drain(baseline: u64, next: impl FnOnce() -> glib::ControlFlow + 'static) {
            let next = std::cell::RefCell::new(Some(next));
            glib::timeout_add_local(std::time::Duration::from_millis(120), move || {
                if cr_ui::gauges::passes() > baseline {
                    if let Some(n) = next.borrow_mut().take() {
                        n();
                    }
                    glib::ControlFlow::Break
                } else {
                    glib::ControlFlow::Continue
                }
            });
        }

        let green = |n: i32| {
            format!("background=\"#008000\" foreground=\"#ffffff\" size=\"7680\"> {n} </span>")
        };
        let orange = |n: i32| {
            format!("background=\"#ffa500\" foreground=\"#ffffff\" size=\"7680\"> {n} </span>")
        };
        let red = |n: i32| {
            format!("background=\"#ff0000\" foreground=\"#ffffff\" size=\"7680\"> {n} </span>")
        };

        // ---- Gate A: the startup pass (persisted zeros → computed).
        let baseline = cr_ui::gauges::passes();
        glib::timeout_add_local(std::time::Duration::from_millis(600), move || {
            let shell = shell.clone();
            let app = app.clone();
            wait_drain(baseline, move || {
                let ok = check_rows(
                    &shell,
                    &[
                        // b5 is also New: Library = 6 total, 3 unread
                        // (b3, b4, b6), 2 new (b2, b5), 1 read (b1).
                        ("Library", &[&green(6), &orange(3), &red(2)]),
                        // F = A ∪ B = {b1, b2, b3, b4}: 4 total,
                        // unread b3 + b4 = 2, new b2 = 1.
                        ("gauge folder", &[&green(4), &orange(2), &red(1)]),
                        ("gauge A", &[&green(3), &orange(1), &red(1)]),
                        // B = {b2, b4}: 2 total, 1 unread, 1 new.
                        ("gauge B", &[&green(2), &orange(1), &red(1)]),
                    ],
                    &[],
                );
                println!("A badges-after-startup ok={ok}");
                // ---- Gate E: the New flag off merges New into Unread.
                cr_ui::library::settings()
                    .borrow_mut()
                    .library_gauges_format =
                    cr_core::settings::enums::LibraryGauges(GAUGES_NO_NEW as i32);
                shell
                    .navigator()
                    .refill(&cr_ui::library::comic_lists_snapshot());
                let ok_e = check_rows(
                    &shell,
                    &[("Library", &[&green(6), &orange(5)])],
                    &[("Library", &[&red(1)]), ("gauge A", &[&red(1)])],
                );
                println!("E no-new-flag merge ok={ok_e}");
                // ---- Gate D: the master switch hides every badge.
                cr_ui::library::settings()
                    .borrow_mut()
                    .display_library_gauges = false;
                shell
                    .navigator()
                    .refill(&cr_ui::library::comic_lists_snapshot());
                let labels = shell.navigator().row_labels();
                let ok_d = labels
                    .iter()
                    .all(|(_, label)| !label.contains("background="));
                println!("D master-off ok={ok_d}");
                // Restore the defaults for the live-change gates.
                {
                    let settings = cr_ui::library::settings();
                    let mut s = settings.borrow_mut();
                    s.display_library_gauges = true;
                    s.library_gauges_format =
                        cr_core::settings::enums::LibraryGauges(GAUGES_ALL as i32);
                }
                shell
                    .navigator()
                    .refill(&cr_ui::library::comic_lists_snapshot());

                // ---- Gate B: b2 becomes read → out of New. The
                // read math needs the page count on the clone.
                let edited = {
                    let lib = cr_ui::library::session();
                    let l = lib.borrow();
                    let mut book = l
                        .database()
                        .books
                        .iter()
                        .find(|b| b.id == b2_id)
                        .cloned()
                        .unwrap();
                    book.info.page_count = 20;
                    book.last_page_read = 19; // (19+1)*100/20 = 100%
                    book
                };
                let baseline = cr_ui::gauges::passes();
                cr_ui::library::apply_edited(&edited);
                let shell_b = shell.clone();
                wait_drain(baseline, move || {
                    let ok = check_rows(
                        &shell_b,
                        &[
                            ("Library", &[&green(6), &orange(3), &red(1)]),
                            ("gauge folder", &[&green(4), &orange(2)]),
                            ("gauge A", &[&green(3), &orange(1)]),
                            ("gauge B", &[&green(2), &orange(1)]),
                        ],
                        &[
                            ("gauge folder", &[&red(1)]),
                            ("gauge A", &[&red(1)]),
                            ("gauge B", &[&red(1)]),
                        ],
                    );
                    println!("B read-move ok={ok}");

                    // ---- Gate C: deleting b2 drops the counters.
                    let baseline = cr_ui::gauges::passes();
                    cr_ui::library::remove_book(&b2_id);
                    let shell_c = shell.clone();
                    wait_drain(baseline, move || {
                        let ok = check_rows(
                            &shell_c,
                            &[
                                ("Library", &[&green(5), &orange(3), &red(1)]),
                                ("gauge folder", &[&green(3), &orange(2)]),
                                ("gauge A", &[&green(2), &orange(1)]),
                                ("gauge B", &[&green(1), &orange(1)]),
                            ],
                            &[],
                        );
                        println!("C delete-drop ok={ok}");
                        let all = ok;
                        println!("ALL-GATES {}", if all { "ok" } else { "FAIL" });
                        println!("PROBE COMPLETE");
                        app.quit();
                        glib::ControlFlow::Break
                    });
                    glib::ControlFlow::Break
                });
                glib::ControlFlow::Break
            });
            glib::ControlFlow::Break
        });
    });

    app.run();
}
