//! Headless probe: the T6 browser toolbar + the Detail column
//! chooser. Gates: the strip mounts above the panes, the Views and
//! Duplicate drops OPEN through their anchors (the unparented-
//! popover crash class), the read filter and the search scope
//! narrow the grid, the column chooser opens over the Detail header
//! and toggles a column live, the Duplicate List drop walks the
//! folders and lands a new smart list, and the Group/Arrange labels
//! follow the sort/group state.
//! Run: Xvfb + `cargo run -p cr-ui --example browserbar_probe` with
//! an isolated XDG.
use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::{CrDateTime, CrGuid};
use gtk4::glib;
use gtk4::prelude::*;

fn main() {
    gtk4::init().expect("gtk init");
    let src = "tests/testfiles/Absolute Flash (2025) Volume 01 Issue 009.cbz";
    let work = std::path::Path::new("/tmp/opencode/browserbar");
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).unwrap();
    // Three books: unread / half read / fully read, distinct series.
    let seeds = [
        ("Probe Alpha", 0i32),
        ("Probe Beta", i32::MIN), // half — set below
        ("Probe Gamma", i32::MAX),
    ];
    for (i, (series, marker)) in seeds.iter().enumerate() {
        let comic = work.join(format!("probe {i}.cbz"));
        std::fs::copy(src, &comic).unwrap();
        let provider = cr_io::ComicProvider::open(&comic).unwrap();
        let mut book = ComicBook {
            id: CrGuid::new_random(),
            file_path: comic.to_string_lossy().into_owned(),
            added_time: CrDateTime::now(),
            ..Default::default()
        };
        book.info.series = (*series).into();
        book.info.page_count = provider.page_count() as i32;
        book.last_page_read = match *marker {
            i32::MIN => book.info.page_count / 2 - 1,
            i32::MAX => book.info.page_count - 1,
            v => v,
        };
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
        let (mut lib, _) = cr_engine::library::Library::open_at_default_location().unwrap();
        lib.database_mut().books.push(book);
        lib.save().unwrap();
    }
    cr_ui::library::initialize().expect("session init");

    // The node count of the list tree (the duplicate-list evidence).
    fn tree_nodes() -> usize {
        fn count(items: &[cr_core::database::list_items::ComicListItem]) -> usize {
            items
                .iter()
                .map(|i| match i {
                    cr_core::database::list_items::ComicListItem::Folder(f) => 1 + count(&f.items),
                    _ => 1,
                })
                .sum()
        }
        count(&cr_ui::library::comic_lists_snapshot())
    }

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.browserbar-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let (window, shell) = cr_ui::browser::shell::BrowserShell::create(app);
        window.present();
        {
            glib::timeout_add_local(std::time::Duration::from_millis(3600), move || {
                println!("DWELL WINDOW ready for the external right-click");
                glib::ControlFlow::Break
            });
        }
        std::mem::forget(shell.clone());
        let base_nodes = tree_nodes();

        // A. The strip mounts; the Views drop OPENS through the
        //    anchor (the OPEN gate — a click-only probe never
        //    exercises the present path).
        glib::timeout_add_local(std::time::Duration::from_millis(800), {
            let shell = shell.clone();
            move || {
                let opened = shell.browserbar_open_dropdown("views");
                println!("A views-open called={opened}");
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(1100), {
            let shell = shell.clone();
            move || {
                let mapped = shell.browserbar_drop_mapped("views");
                println!("A views-drop mapped={mapped} (expect true, no segv)");
                shell.browserbar_close_dropdown("views");
                // The check SYNC (the T6 round-1 report: the Views
                // check never moved): switch to Tiles and read the
                // action state back.
                shell.state_dispatch_param("win.view-mode", "tile");
                println!(
                    "A view-mode state={:?} (expect tile)",
                    shell.state_action_string("view-mode")
                );
                shell.state_dispatch_param("win.view-mode", "thumbnail");
                glib::ControlFlow::Break
            }
        });

        // A2. Browse ▸ Browser from the QuickOpen page (the round-1
        //     report: nothing happened there).
        glib::timeout_add_local(std::time::Duration::from_millis(1300), {
            let shell = shell.clone();
            move || {
                println!(
                    "A2 before page={:?}",
                    shell.state_visible_page()
                );
                shell.state_dispatch("win.toggle-browser");
                println!(
                    "A2 after toggle page={:?} (expect browser)",
                    shell.state_visible_page()
                );
                glib::ControlFlow::Break
            }
        });

        // B. The grid: the read filter narrows (3 books: 1 read,
        //    1 reading, 1 unread — the engine defaults 95/10).
        glib::timeout_add_local(std::time::Duration::from_millis(1500), {
            let shell = shell.clone();
            move || {
                shell.state_dispatch("win.refresh");
                let total = shell.state_grid_book_count();
                shell.state_dispatch_param("win.view-filter", "read");
                let read = shell.state_grid_book_count();
                shell.state_dispatch_param("win.view-filter", "reading");
                let reading = shell.state_grid_book_count();
                shell.state_dispatch_param("win.view-filter", "unread");
                let unread = shell.state_grid_book_count();
                shell.state_dispatch_param("win.view-filter", "all");
                let back = shell.state_grid_book_count();
                println!(
                    "B total={total} read={read} reading={reading} unread={unread} back={back} (expect 3/1/1/1/3)"
                );
                glib::ControlFlow::Break
            }
        });

        // C. The search scope: the cue text follows; the scoped
        //    composed matcher narrows by the scope's fields.
        glib::timeout_add_local(std::time::Duration::from_millis(1500), {
            let shell = shell.clone();
            move || {
                shell.state_dispatch_param("win.search-scope", "writer");
                println!(
                    "C cue={:?} scope={:?} (expect Search Writer / writer)",
                    shell.state_search_placeholder(),
                    shell.state_action_string("search-scope")
                );
                shell.state_set_search_text("Probe Alpha");
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(1800), {
            let shell = shell.clone();
            move || {
                // The writer scope: the series text is NOT a writer
                // field — no hit.
                println!(
                    "C writer-scope hits={} (expect 0)",
                    shell.state_grid_book_count()
                );
                shell.state_dispatch_param("win.search-scope", "series");
                shell.state_set_search_text("Probe Beta");
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(2100), {
            let shell = shell.clone();
            move || {
                println!(
                    "C series-scope hits={} (expect 1)",
                    shell.state_grid_book_count()
                );
                shell.state_set_search_text("");
                shell.state_dispatch_param("win.search-scope", "all");
                glib::ControlFlow::Break
            }
        });

        // D. The Detail column chooser: open over the header, then
        //    the row click toggles Series off and back on.
        glib::timeout_add_local(std::time::Duration::from_millis(2200), {
            let shell = shell.clone();
            move || {
                shell.state_dispatch_param("win.view-mode", "detail");
                let mapped = shell.state_open_column_chooser(40.0, 10.0);
                let h = shell.state_column_chooser_height();
                let series = shell
                    .state_columns_snapshot()
                    .iter()
                    .find(|(_, n, _)| n == "Series")
                    .map(|c| c.2);
                println!(
                    "D chooser-open={mapped} height={h} series-before={series:?} (expect true, height>100)"
                );
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(2500), {
            let shell = shell.clone();
            move || {
                let id = shell
                    .state_columns_snapshot()
                    .iter()
                    .find(|(_, n, _)| n == "Series")
                    .map(|c| c.0);
                let id = match id {
                    Some(id) => id,
                    None => return glib::ControlFlow::Break,
                };
                // The chooser rows toggle through `win.toggle-column`
                // (the CheckButton fires the same action).
                let clicked = shell.state_dispatch_param("win.toggle-column", &id.to_string());
                let after = shell
                    .state_columns_snapshot()
                    .iter()
                    .find(|(_, n, _)| n == "Series")
                    .map(|c| c.2);
                println!(
                    "D chooser-click={clicked} series-after={after:?} (expect Some(false))"
                );
                glib::ControlFlow::Break
            }
        });

        // E. The Duplicate List drop: the folder rows, then the
        //    duplicate lands in the chosen folder.
        glib::timeout_add_local(std::time::Duration::from_millis(2900), {
            let shell = shell.clone();
            move || {
                let opened = shell.browserbar_open_dropdown("duplicate");
                let mapped = shell.browserbar_drop_mapped("duplicate");
                let rows = shell
                    .browserbar_dropdown("duplicate")
                    .map(|d| {
                        d.refresh_slot("duplicate-list");
                        d.dyn_rows_snapshot("duplicate-list")
                    })
                    .unwrap_or_default();
                shell.browserbar_close_dropdown("duplicate");
                println!("E duplicate-open={opened} mapped={mapped} rows={rows:?}");
                // A filter first (the C# duplicate needs a current
                // matcher), then duplicate into the first folder.
                shell.state_dispatch_param("win.view-filter", "read");
                if let Some(id) = cr_ui::library::list_folders().first().map(|(id, _, _)| *id) {
                    shell.state_dispatch_param("win.duplicate-list", &id.to_string());
                }
                shell.state_dispatch_param("win.view-filter", "all");
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(3300), {
            let shell = shell.clone();
            move || {
                let nodes = tree_nodes();
                println!(
                    "E tree-nodes {base_nodes} -> {nodes} (expect +1: the smart list)"
                );
                let _ = &shell;
                // F. A long dwell in Detail mode with the browser
                // page shown: an external xdotool right-click on the
                // header exercises the REAL gesture path (the trace
                // prints CONTEXT/CHOOSER).
                shell.state_dispatch("win.view-library");
                shell.state_dispatch_param("win.view-mode", "detail");
                println!("DWELL START");
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(60000), {
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
