//! Headless probe: the T4 dynamic menu fills. Seeds an isolated
//! library with copies of the test comic, opens two tabs, and walks
//! the fills the way the C# `DropDownOpening` does: Open Books (one
//! row per tab, checked on the current), the row click that switches
//! slots, Recent Books, the bookmark prompt round-trip, the Page
//! Type radio, and the My Rating check.
//! Run: Xvfb + `cargo run -p cr-ui --example dynmenus_probe` with an
//! isolated XDG.
use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::{CrDateTime, CrGuid};
use gtk4::glib;
use gtk4::prelude::*;

fn seed_comic(src: &str, work: &std::path::Path, name: &str) -> ComicBook {
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
    book.info.series = format!("DynProbe {name}");
    book.info.pages = provider
        .pages()
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mut pg = cr_core::model::comic_page_info::ComicPageInfo {
                key: Some(p.name.clone()),
                ..Default::default()
            };
            pg.set_image_index(i as i32);
            pg
        })
        .collect();
    book
}

fn main() {
    gtk4::init().expect("gtk init");
    let src = "tests/testfiles/Absolute Flash (2025) Volume 01 Issue 009.cbz";
    let work = std::path::Path::new("/tmp/opencode/dynmenus");
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).unwrap();

    // Three library books (two opened below).
    let (mut lib, _) = cr_engine::library::Library::open_at_default_location().unwrap();
    for name in ["probe a.cbz", "probe b.cbz", "probe c.cbz"] {
        let book = seed_comic(src, work, name);
        lib.database_mut().books.push(book);
    }
    lib.save().unwrap();
    // The session (loads the isolated settings + the DB).
    cr_ui::library::initialize().expect("session init");

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.dynmenus-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let (window, shell) = cr_ui::browser::shell::BrowserShell::create(app);
        window.present();
        // The probe must keep the shell alive (the thread-local
        // lesson) — every action handler holds Weak<ShellState>.
        std::mem::forget(shell.clone());

        // Open two comics → two reader tabs.
        for name in ["probe a.cbz", "probe b.cbz"] {
            let path = work.join(name);
            shell.open_comic(&path);
        }
        let tabs = shell.state_reader_tab_count();
        println!("TABS {tabs}");

        // 1. The Open Books fill (the File menu, index 0): two rows,
        //    the LAST opened (the current tab) checked.
        glib::timeout_add_local(std::time::Duration::from_millis(1200), {
            let menubar = shell.menubar().clone_handle();
            move || {
                menubar.open_top(0);
                let rows = menubar.dyn_rows_snapshot("open-books");
                println!("OPEN-BOOKS rows={} {:?}", rows.len(), rows);
                let checked = rows.iter().filter(|(_, c, _)| *c).count();
                println!("OPEN-BOOKS checked={checked} (expect 1)");
                glib::ControlFlow::Break
            }
        });

        // 2. The SUBMENU-REMAP gate (the round-1 user finding: the
        //    check stayed stale when revisiting the submenu inside
        //    an already-open menu). Switch to slot 0, refresh the
        //    slot the way the child-popover map does, and expect
        //    the check to move WITHOUT a top-menu reopen.
        glib::timeout_add_local(std::time::Duration::from_millis(1500), {
            let menubar = shell.menubar().clone_handle();
            let shell = shell.clone();
            move || {
                let slot = shell.state_first_open_slot().unwrap_or_default();
                let bare = shell.state_dispatch_param("win.open-tab", &slot.to_string());
                let direct = shell.state_reader_slot();
                println!("SWITCH bare fired={bare:?} ->{direct:?} (expect Some(0))");
                // Pre-refresh: the fill still checks the OLD tab.
                let stale = menubar
                    .dyn_rows_snapshot("open-books")
                    .iter()
                    .filter(|(_, c, _)| *c)
                    .map(|(l, _, _)| l.clone())
                    .collect::<Vec<_>>();
                // The map-hook rebuild (no top reopen).
                menubar.refresh_dyn_slot("open-books");
                let fresh = menubar
                    .dyn_rows_snapshot("open-books")
                    .iter()
                    .filter(|(_, c, _)| *c)
                    .map(|(l, _, _)| l.clone())
                    .collect::<Vec<_>>();
                let moved = fresh != stale && fresh.iter().any(|l| l.contains("a.cbz"));
                println!("REMAP stale={stale:?} fresh={fresh:?} moved={moved}");
                // The row click moves to the OTHER tab (a real move).
                menubar.click_row("win.open-tab::1");
                let after = shell.state_reader_slot();
                println!("SWITCH click ->{after:?} (expect Some(1))");
                glib::ControlFlow::Break
            }
        });

        // 3. The bookmark prompt: activate set-bookmark on the
        //    current tab (probe a), accept the prefilled name, then
        //    the Bookmarks fill carries the entry.
        glib::timeout_add_local(std::time::Duration::from_millis(1900), {
            let window = window.clone();
            move || {
                let _ = gtk4::prelude::WidgetExt::activate_action(&window, "win.set-bookmark", None);
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(2200), {
            let menubar = shell.menubar().clone_handle();
            let shell = shell.clone();
            move || {
                // Accept the prompt (the prefilled "Page N").
                let toplevels = gtk4::Window::list_toplevels();
                for w in toplevels.iter() {
                    if let Some(dlg) = w.downcast_ref::<gtk4::Dialog>() {
                        if dlg.title().as_deref() == Some("Bookmark") {
                            dlg.response(gtk4::ResponseType::Ok);
                        }
                    }
                }
                glib::timeout_add_local(std::time::Duration::from_millis(300), {
                    let menubar = menubar.clone_handle();
                    let shell = shell.clone();
                    move || {
                        menubar.open_top(1);
                        let rows = menubar.dyn_rows_snapshot("bookmarks");
                        println!("BOOKMARKS rows={rows:?}");
                        let page_state = shell.state_current_page_bookmark();
                        println!("BOOKMARK state={page_state:?} (Some = set)");
                        glib::ControlFlow::Break
                    }
                });
                glib::ControlFlow::Break
            }
        });

        // 4. The Page Type radio: the fill checks the current page's
        //    type (Story = 8 by default); then SET Front Cover (1) —
        //    the fill re-checks and the session book carries it.
        glib::timeout_add_local(std::time::Duration::from_millis(2900), {
            let menubar = shell.menubar().clone_handle();
            let shell = shell.clone();
            move || {
                menubar.open_top(1);
                let rows = menubar.dyn_rows_snapshot("page-type");
                let checked: Vec<String> = rows
                    .iter()
                    .filter(|(_, c, _)| *c)
                    .map(|(l, _, _)| l.clone())
                    .collect();
                println!("PAGE-TYPE rows={} checked={checked:?}", rows.len());
                // Set Front Cover on the current page.
                let fired = shell.state_dispatch_param("win.page-type", "1");
                let _ = fired;
                glib::timeout_add_local(std::time::Duration::from_millis(200), {
                    let menubar = menubar.clone_handle();
                    let shell = shell.clone();
                    move || {
                        // Reopen (the fill rebuilds at open — the
                        // C# DropDownOpening shape).
                        menubar.open_top(1);
                        let checked: Vec<String> = menubar
                            .dyn_rows_snapshot("page-type")
                            .iter()
                            .filter(|(_, c, _)| *c)
                            .map(|(l, _, _)| l.clone())
                            .collect();
                        let db_type = shell.state_current_page_type();
                        println!("PAGE-TYPE after set checked={checked:?} db={db_type:?}");
                        glib::ControlFlow::Break
                    }
                });
                glib::ControlFlow::Break
            }
        });

        // 5. My Rating: select the Library list, select the first
        //    book in the grid, rate 4 — the action check flips and
        //    the grid rating updates.
        glib::timeout_add_local(std::time::Duration::from_millis(3300), {
            let shell = shell.clone();
            move || {
                // The navigator fills at startup — select the first
                // list (Library) so the grid has books.
                let items = cr_ui::library::comic_lists_snapshot();
                if let Some(first) = items.first() {
                    let id = first.base().id;
                    shell.navigator().select_list(&id);
                }
                glib::timeout_add_local(std::time::Duration::from_millis(400), {
                    let shell = shell.clone();
                    move || {
                        let books = shell.state_grid_book_count();
                        shell.state_select_first_book();
                        let selected = shell.state_grid_selection_len();
                        let rating_enabled = shell.state_action_enabled("rating-4");
                        let fired = shell.state_dispatch("win.rating-4");
                        let checked = shell.state_rating_checked(4);
                        let rating = shell.state_selected_book_rating();
                        println!(
                            "RATING books={books} selected={selected} enabled={rating_enabled:?} fired={fired:?} checked={checked} rating={rating}"
                        );
                        glib::ControlFlow::Break
                    }
                });
                glib::ControlFlow::Break
            }
        });

        glib::timeout_add_local(std::time::Duration::from_millis(4300), {
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
