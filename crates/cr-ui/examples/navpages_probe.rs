//! Headless probe: the T7 navigator + Pages toolbars. Gates: the
//! navigator buttons fire the SAME ListCommand path as the context
//! menu, the quick-search toggle shows/hides the box and the text
//! filters the tree, Expand/Collapse All flips the whole tree, the
//! Pages Views drop OPENS through its anchor (the unparented-popover
//! crash class), a radio row click switches the grid to Tile and the
//! main click cycles it back, and the action state follows the PANEL
//! (the source of truth).
//! Run: Xvfb + `cargo run -p cr-ui --example navpages_probe` with an
//! isolated XDG (fresh DB → the default list tree).
use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::{CrDateTime, CrGuid};
use gtk4::glib;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

fn main() {
    gtk4::init().expect("gtk init");
    let src = "tests/testfiles/Absolute Flash (2025) Volume 01 Issue 009.cbz";
    let work = std::path::Path::new("/tmp/opencode/navpages");
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).unwrap();
    let comic = work.join("probe a.cbz");
    std::fs::copy(src, &comic).unwrap();
    let provider = cr_io::ComicProvider::open(&comic).unwrap();
    let mut book = ComicBook {
        id: CrGuid::new_random(),
        file_path: comic.to_string_lossy().into_owned(),
        added_time: CrDateTime::now(),
        ..Default::default()
    };
    book.info.page_count = provider.page_count() as i32;
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
    cr_ui::library::initialize().expect("session init");

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.navpages-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let (window, shell) = cr_ui::browser::shell::BrowserShell::create(app);
        window.present();
        // The probe must keep the shell alive (the thread-local
        // lesson) — every action handler holds Weak<ShellState>.
        std::mem::forget(shell.clone());

        // The ListCommand recorder (the toolbar and the context menu
        // share the host callback; the probe records instead of
        // opening dialogs).
        let commands: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        {
            let commands = Rc::clone(&commands);
            shell.navigator().connect_command(move |command, _t| {
                commands.borrow_mut().push(format!("{command:?}"));
            });
        }

        // A. The navigator toolbar buttons fire the ListCommand path.
        glib::timeout_add_local(std::time::Duration::from_millis(800), {
            let shell = shell.clone();
            let commands = Rc::clone(&commands);
            move || {
                let clicked = shell.nav_click_button("new-smart-list");
                let fired = commands.borrow().join(",");
                println!(
                    "A new-smart-list clicked={clicked} fired=[{fired}] (expect NewSmartList)"
                );
                let clicked = shell.nav_click_button("new-folder");
                let fired = commands.borrow().join(",");
                println!("A new-folder clicked={clicked} fired=[{fired}] (expect both commands)");
                glib::ControlFlow::Break
            }
        });

        // B. The quick-search toggle + the tree filter.
        glib::timeout_add_local(std::time::Duration::from_millis(1200), {
            let shell = shell.clone();
            move || {
                let base = shell.nav_row_count();
                shell.nav_click_button("quick-search");
                let shown = shell.nav_search_visible();
                println!("B toggle-on visible={shown} (expect true)");
                shell.nav_set_search_text("zzz-no-match");
                let filtered = shell.nav_row_count();
                println!("B rows {base} -> {filtered} (expect 1: Library always shows)");
                shell.nav_set_search_text("");
                let restored = shell.nav_row_count();
                println!("B rows restored={restored} (expect {base})");
                shell.nav_click_button("quick-search");
                println!(
                    "B toggle-off visible={} (expect false)",
                    shell.nav_search_visible()
                );
                glib::ControlFlow::Break
            }
        });

        // C. Expand/Collapse All: any-expanded → collapse, else
        //    expand (`ExpandCollapseAllNodes`).
        glib::timeout_add_local(std::time::Duration::from_millis(1600), {
            let shell = shell.clone();
            move || {
                shell.nav_click_button("expand-collapse-all");
                let expanded = shell.nav_expanded_count();
                println!("C expand-all expanded={expanded} (expect > 0)");
                shell.nav_click_button("expand-collapse-all");
                let collapsed = shell.nav_expanded_count();
                println!("C collapse-all expanded={collapsed} (expect 0)");
                glib::ControlFlow::Break
            }
        });

        // D. The Pages Views drop: the OPEN gate through the real
        //    anchor, then a radio row click → Tile.
        glib::timeout_add_local(std::time::Duration::from_millis(2000), {
            let shell = shell.clone();
            move || {
                // Open the seeded comic so the Pages tab shows (the
                // anchor must be mapped for the popover to present).
                let path = work.join("probe a.cbz");
                shell.open_comic(&path);
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(2200), {
            let shell = shell.clone();
            move || {
                // The Pages panel is a full-window workspace now —
                // select its tab so the Views anchor maps (the T5
                // lesson: the popover needs a mapped anchor).
                let pages_tab = shell
                    .tabstrip()
                    .tab_visible(&cr_ui::browser::tabstrip::TabId::Pages);
                shell.state_dispatch("win.view-pages");
                println!(
                    "D pages-tab visible={pages_tab} page={:?} (expect true/pages)",
                    shell.state_visible_page()
                );
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(2400), {
            let shell = shell.clone();
            move || {
                let opened = shell.pages_open_views();
                println!("D views-open opened={opened} (expect true, no segv)");
                shell.pages_close_views();
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(2700), {
            let shell = shell.clone();
            move || {
                let clicked = shell.pages_click_view("win.pages-view-mode::tile");
                let mode = shell.pages_mode();
                let state = shell.state_action_string("pages-view-mode");
                println!(
                    "D row-click clicked={clicked} mode={mode:?} state={state:?} (expect tile/tile)"
                );
                glib::ControlFlow::Break
            }
        });

        // E. The main click cycles back to Thumbnail
        //    (`tbbView_ButtonClick`).
        glib::timeout_add_local(std::time::Duration::from_millis(3000), {
            let shell = shell.clone();
            move || {
                shell.pages_click_main();
                let mode = shell.pages_mode();
                let state = shell.state_action_string("pages-view-mode");
                println!("E main-click mode={mode:?} state={state:?} (expect thumbnail/thumbnail)");
                glib::ControlFlow::Break
            }
        });

        // F. The T1 gates: the navigator context menu opens AT the
        //    cursor (view-relative coords) and carries no pointing
        //    arrow (the C# ContextMenuStrip shape). The browser
        //    workspace returns first — steps D/E left the Pages tab
        //    showing, and a hidden tree has no allocation.
        glib::timeout_add_local(std::time::Duration::from_millis(3200), {
            let shell = shell.clone();
            move || {
                shell.state_dispatch("win.view-library");
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(3500), {
            let shell = shell.clone();
            move || {
                // Row 1 (the tree shows Library + the collapsed
                // Smart Lists folder here).
                let (x, y) = (40.0, 20.0);
                shell.navigator().probe_context_menu(x, y);
                match shell.navigator().last_menu_popover() {
                    Some(p) => {
                        let rect = p.pointing_to();
                        println!(
                            "F menu arrow={} rect={rect:?} (expect false / Some(40, 28, 1, 1))",
                            p.has_arrow()
                        );
                    }
                    None => println!("F menu MISSING (no row at {x},{y}?)"),
                }
                glib::ControlFlow::Break
            }
        });

        glib::timeout_add_local(std::time::Duration::from_millis(4000), {
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
