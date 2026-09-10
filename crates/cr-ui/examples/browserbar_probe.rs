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
                // The T1 gate: the drop rows render no `&` and the
                // popover carries no pointing arrow.
                if let Some(d) = shell.browserbar_dropdown("views") {
                    let amps = d.row_labels().iter().filter(|l| l.contains('&')).count();
                    println!(
                        "A views amps={amps} arrow={} (expect 0 / false)",
                        d.popover().has_arrow()
                    );
                } else {
                    println!("A views drop MISSING");
                }
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

        // D2. Grouping (the user-reported gap): the Views drop
        //     carries "Collapse/Expand all Groups"; headers show in
        //     EVERY mode while a grouper is set, the action
        //     collapses/expands all, and the command disables
        //     without a grouper.
        glib::timeout_add_local(std::time::Duration::from_millis(2700), {
            let shell = shell.clone();
            move || {
                shell.state_dispatch_param("win.view-mode", "detail");
                let ungrouped = shell.state_grid_groups();
                let enabled_ungrouped = shell.state_action_enabled("toggle-groups");
                shell.state_dispatch_param("win.group-by", "Series");
                let (groups, collapsed0) = shell.state_grid_groups();
                let enabled = shell.state_action_enabled("toggle-groups");
                shell.state_dispatch("win.toggle-groups");
                let (_, collapsed_all) = shell.state_grid_groups();
                shell.state_dispatch("win.toggle-groups");
                let (_, collapsed_back) = shell.state_grid_groups();
                shell.state_dispatch_param("win.group-by", "");
                let ungrouped_again = shell.state_grid_groups();
                println!(
                    "D2 ungrouped={ungrouped:?} enabled-ungrouped={enabled_ungrouped} groups={groups} collapsed0={collapsed0} enabled={enabled} collapsed-all={collapsed_all} collapsed-back={collapsed_back} ungrouped-again={ungrouped_again:?} (expect (1,0)/false/3/0/true/3/0/(1,0) — the ungrouped view keeps ONE empty-caption bucket, headers stay hidden; Alpha/Beta/Gamma are three series)"
                );
                shell.state_dispatch_param("win.view-mode", "thumbnail");
                glib::ControlFlow::Break
            }
        });

        // D3. The group-header CLICK paths through the REAL press
        //     handler (the group-by-series crash: the double-click
        //     borrow_mut collided with the scrutinee borrow). The
        //     arrow press toggles ONE group; the double-click (n=2)
        //     expands/collapses ALL; the label selects the group's
        //     items.
        // D3. The group-header CLICK paths through the REAL press
        //     handler (the group-by-series crash: the double-click
        //     borrow_mut collided with the scrutinee borrow). One
        //     press per 150 ms tick — the draw between presses
        //     re-records the arrow zones (the real app always has
        //     frames between presses). Gates: the label select, the
        //     single-click collapse/expand of ONE group, both
        //     double-click directions (the C# net: every group takes
        //     the OPPOSITE of the clicked header's ORIGINAL state),
        //     and the TRUE counts on collapsed headers.
        {
            let shell = shell.clone();
            let step = std::rc::Rc::new(std::cell::Cell::new(0u32));
            let log: std::rc::Rc<std::cell::RefCell<Vec<String>>> =
                std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
            let zone: std::rc::Rc<std::cell::Cell<(f64, f64, f64, f64)>> =
                std::rc::Rc::new(std::cell::Cell::new((0.0, 0.0, 0.0, 0.0)));
            let print_log = log.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(2800), {
                let shell = shell.clone();
                move || {
                    shell.state_dispatch_param("win.group-by", "Series");
                    glib::ControlFlow::Break
                }
            });
            glib::timeout_add_local(std::time::Duration::from_millis(3300), move || {
                glib::timeout_add_local(
                    std::time::Duration::from_millis(150),
                    {
                        let shell = shell.clone();
                        let step = step.clone();
                        let log = log.clone();
                        let zone = zone.clone();
                        move || {
                            let s = step.get();
                            step.set(s + 1);
                            let (ax, ay, aw, ah) = shell.state_group_arrow_zone(0);
                            if s == 0 {
                                if aw <= 0.0 {
                                    log.borrow_mut().push(
                                        "arrow-zone not recorded (draw pending) — INCONCLUSIVE"
                                            .into(),
                                    );
                                    return glib::ControlFlow::Break;
                                }
                                zone.set((ax, ay, aw, ah));
                                return glib::ControlFlow::Continue;
                            }
                            let (ax, ay, aw, ah) = zone.get();
                            let cx = ax + aw / 2.0;
                            let cy = ay + ah / 2.0;
                            let lx = ax + aw + 30.0;
                            match s {
                                1 => {
                                    // The label zone while EXPANDED:
                                    // selects the group's items (group
                                    // 0 = 1 book).
                                    let hit = shell.state_group_press(1, lx, cy);
                                    log.borrow_mut().push(format!(
                                        "label hit={hit} selected={}",
                                        shell.state_grid_selection_len()
                                    ));
                                }
                                2 => {
                                    let hit = shell.state_group_press(1, cx, cy);
                                    log.borrow_mut().push(format!(
                                        "arrow1 hit={hit} collapsed={} (expect 1)",
                                        shell.state_grid_groups().1
                                    ));
                                }
                                3 => {
                                    shell.state_group_press(1, cx, cy);
                                    log.borrow_mut().push(format!(
                                        "arrow1 again collapsed={} (expect 0 — expand just one)",
                                        shell.state_grid_groups().1
                                    ));
                                }
                                4 => {
                                    shell.state_group_press(1, cx, cy);
                                }
                                5 => {
                                    shell.state_group_press(2, cx, cy);
                                    log.borrow_mut().push(format!(
                                        "dbl on EXPANDED collapsed={} counts={:?} (expect 3 + the true counts)",
                                        shell.state_grid_groups().1,
                                        shell.state_group_counts()
                                    ));
                                }
                                6 => {
                                    shell.state_group_press(1, cx, cy);
                                }
                                7 => {
                                    shell.state_group_press(2, cx, cy);
                                    log.borrow_mut().push(format!(
                                        "dbl on COLLAPSED collapsed={} (expect 0)",
                                        shell.state_grid_groups().1
                                    ));
                                    shell.state_dispatch_param("win.group-by", "");
                                    log.borrow_mut().push("DONE".into());
                                    return glib::ControlFlow::Break;
                                }
                                _ => return glib::ControlFlow::Break,
                            }
                            glib::ControlFlow::Continue
                        }
                    },
                );
                glib::ControlFlow::Break
            });
            glib::timeout_add_local(std::time::Duration::from_millis(5400), {
                let log = print_log;
                move || {
                    for line in log.borrow().iter() {
                        println!("D3 {line}");
                    }
                    glib::ControlFlow::Break
                }
            });
        }

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
                println!("E tree-nodes {base_nodes} -> {nodes} (expect +1: the smart list)");
                let _ = &shell;
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(5700), {
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
