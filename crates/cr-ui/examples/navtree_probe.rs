//! Headless probe: the navigator tree drag-and-drop and the Sort
//! command. Gates: A the drop geometry maps a point to the C# drop
//! style (`SetDropEffects`), B a drop ON a folder makes the item its
//! last child, C a drop on a row puts the item BEFORE that row (the
//! free ordering inside a folder), D the refusals (a folder into its
//! own subtree, the Library root never moves), E the "Sort" menu row
//! appears on a folder only and fires `ListCommand::Sort`, and F the
//! sorted folder keeps folders first and then name order, and the
//! order SURVIVES a save and reload (the order lives in ComicDb.xml).
//! Run: Xvfb + `cargo run -p cr-ui --example navtree_probe` with an
//! isolated XDG.
use cr_core::database::list_items::ComicListItem;
use cr_core::xml::scalar::CrGuid;
use cr_ui::library::ListDrop;
use gtk4::glib;
use gtk4::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

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
        .application_id("org.comicrust.navtree-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let (window, shell) = cr_ui::browser::shell::BrowserShell::create(app);
        window.present();
        std::mem::forget(shell.clone());

        // The commands the navigator menu fires. "Sort" runs the same
        // body `app.rs` runs for `ListCommand::Sort`.
        let commands: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        {
            let commands = Rc::clone(&commands);
            let nav = shell.navigator();
            let nav_for_cmd = Rc::clone(&nav);
            nav.connect_command(move |command, target| {
                commands.borrow_mut().push(format!("{command:?}"));
                if let (cr_ui::browser::navigator::ListCommand::Sort, Some(id)) = (command, target)
                {
                    if cr_ui::library::sort_folder(&id) {
                        nav_for_cmd.refill(&cr_ui::library::comic_lists_snapshot());
                    }
                }
            });
        }

        // The probe tree: one folder plus two root lists. `new_folder`
        // and `new_id_list` with no selection append at the root end.
        let ids: Rc<RefCell<Vec<CrGuid>>> = Rc::new(RefCell::new(Vec::new()));
        glib::timeout_add_local(std::time::Duration::from_millis(800), {
            let shell = shell.clone();
            let ids = ids.clone();
            move || {
                let folder = cr_ui::library::new_folder(None, "Probe Folder");
                let alpha = cr_ui::library::new_id_list(None, "Probe Alpha");
                let beta = cr_ui::library::new_id_list(None, "Probe Beta");
                ids.borrow_mut().extend([folder, alpha, beta]);
                let nav = shell.navigator();
                nav.refill(&cr_ui::library::comic_lists_snapshot());
                nav.click_button("expand-collapse-all");
                glib::ControlFlow::Break
            }
        });

        // A. The drop geometry (`SetDropEffects`).
        glib::timeout_add_local(std::time::Duration::from_millis(1400), {
            let shell = shell.clone();
            let ids = ids.clone();
            move || {
                let nav = shell.navigator();
                let (folder, alpha) = (ids.borrow()[0], ids.borrow()[1]);
                let (folder_centre, folder_top) = nav
                    .probe_row_points(&folder)
                    .expect("A FAIL: the folder row has no allocation");
                let (alpha_centre, _) = nav
                    .probe_row_points(&alpha)
                    .expect("A FAIL: the list row has no allocation");

                let on_folder = nav.drop_target_at(folder_centre.0, folder_centre.1);
                assert_eq!(
                    on_folder,
                    ListDrop::IntoFolder(folder),
                    "A FAIL: the centre of a folder row must drop INTO it"
                );
                let on_folder_edge = nav.drop_target_at(folder_top.0, folder_top.1);
                assert_eq!(
                    on_folder_edge,
                    ListDrop::BeforeItem(folder),
                    "A FAIL: the top 4 px of a folder row must insert before it"
                );
                let on_list = nav.drop_target_at(alpha_centre.0, alpha_centre.1);
                assert_eq!(
                    on_list,
                    ListDrop::BeforeItem(alpha),
                    "A FAIL: a row that is not a folder must insert before it"
                );
                let below_rows = nav.drop_target_at(20.0, 4000.0);
                assert_eq!(
                    below_rows,
                    ListDrop::RootEnd,
                    "A FAIL: empty space must drop at the root end"
                );
                println!("A ok: drop geometry maps to IntoFolder / BeforeItem / RootEnd");
                glib::ControlFlow::Break
            }
        });

        // B. A drop ON the folder takes each list in. Each drop needs
        //    its own main-loop turn: the drop refills the tree, and
        //    `cell_area` reports the new row geometry only after the
        //    view has laid out again.
        for (step, which) in [(1800, 1usize), (2100, 2usize)] {
            let shell = shell.clone();
            let ids = ids.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(step), move || {
                let nav = shell.navigator();
                let folder = ids.borrow()[0];
                let list = ids.borrow()[which];
                let (centre, _) = nav
                    .probe_row_points(&folder)
                    .expect("B FAIL: the folder row has no allocation");
                let target = nav.drop_target_at(centre.0, centre.1);
                println!("B{which} target={target:?} (expect IntoFolder)");
                assert_eq!(
                    target,
                    ListDrop::IntoFolder(folder),
                    "B FAIL: the folder centre must resolve to IntoFolder"
                );
                assert!(
                    nav.probe_drop(&list, centre.0, centre.1),
                    "B FAIL: the drop on the folder did not move the list"
                );
                glib::ControlFlow::Break
            });
        }
        glib::timeout_add_local(std::time::Duration::from_millis(2400), {
            let ids = ids.clone();
            move || {
                let folder = ids.borrow()[0];
                assert_eq!(
                    folder_children(&folder),
                    vec!["Probe Alpha", "Probe Beta"],
                    "B FAIL: the folder must hold both lists in drop order"
                );
                println!("B ok: a drop on a folder appends the list to it");
                glib::ControlFlow::Break
            }
        });

        // C. A drop on the first row reorders inside the folder.
        glib::timeout_add_local(std::time::Duration::from_millis(2700), {
            let shell = shell.clone();
            let ids = ids.clone();
            move || {
                let nav = shell.navigator();
                let (folder, alpha, beta) = (ids.borrow()[0], ids.borrow()[1], ids.borrow()[2]);
                let (alpha_centre, _) = nav
                    .probe_row_points(&alpha)
                    .expect("C FAIL: the Alpha row has no allocation");
                let target = nav.drop_target_at(alpha_centre.0, alpha_centre.1);
                println!("C target={target:?} (expect BeforeItem(Alpha))");
                assert!(
                    nav.probe_drop(&beta, alpha_centre.0, alpha_centre.1),
                    "C FAIL: the reorder drop did nothing"
                );
                assert_eq!(
                    folder_children(&folder),
                    vec!["Probe Beta", "Probe Alpha"],
                    "C FAIL: the drop must put the list BEFORE the target row"
                );
                println!("C ok: a drop on a row reorders inside the folder");
                glib::ControlFlow::Break
            }
        });

        // D. The refusals.
        glib::timeout_add_local(std::time::Duration::from_millis(3000), {
            let shell = shell.clone();
            let ids = ids.clone();
            move || {
                let nav = shell.navigator();
                let (folder, _alpha, beta) = (ids.borrow()[0], ids.borrow()[1], ids.borrow()[2]);
                let before = folder_children(&folder);
                let (beta_centre, _) = nav
                    .probe_row_points(&beta)
                    .expect("D FAIL: the Beta row has no allocation");
                assert!(
                    !nav.probe_drop(&folder, beta_centre.0, beta_centre.1),
                    "D FAIL: a folder must refuse a drop into its own subtree"
                );
                assert_eq!(
                    folder_children(&folder),
                    before,
                    "D FAIL: the refused drop changed the tree"
                );

                // The Library root never moves.
                let root = cr_ui::library::comic_lists_snapshot()
                    .into_iter()
                    .find(|i| matches!(i, ComicListItem::Library(_)))
                    .map(|i| i.base().id)
                    .expect("D FAIL: no Library root in the tree");
                let (folder_centre, _) = nav
                    .probe_row_points(&folder)
                    .expect("D FAIL: the folder row has no allocation");
                assert!(
                    !nav.probe_drop(&root, folder_centre.0, folder_centre.1),
                    "D FAIL: the Library root moved"
                );
                assert!(
                    cr_ui::library::comic_lists_snapshot()
                        .first()
                        .is_some_and(|i| matches!(i, ComicListItem::Library(_))),
                    "D FAIL: the Library root left the first row"
                );
                println!("D ok: subtree and Library-root drops are refused");
                glib::ControlFlow::Break
            }
        });

        // E + F. The Sort row: folder only, and it sorts the folder.
        glib::timeout_add_local(std::time::Duration::from_millis(3400), {
            let shell = shell.clone();
            let ids = ids.clone();
            let commands = Rc::clone(&commands);
            move || {
                let nav = shell.navigator();
                let (folder, alpha) = (ids.borrow()[0], ids.borrow()[1]);

                // A list row has no Sort.
                nav.select_list(&alpha);
                nav.probe_context_menu_for_selection();
                let popover = nav
                    .last_menu_popover()
                    .expect("E FAIL: the list menu did not open");
                let labels = popover_labels(&popover);
                assert!(
                    !labels.iter().any(|l| l == "Sort"),
                    "E FAIL: a list row shows Sort: {labels:?}"
                );
                popover.popdown();

                // The folder row carries it and fires the command.
                nav.select_list(&folder);
                nav.probe_context_menu_for_selection();
                let popover = nav
                    .last_menu_popover()
                    .expect("E FAIL: the folder menu did not open");
                let labels = popover_labels(&popover);
                assert!(
                    labels.iter().any(|l| l == "Sort"),
                    "E FAIL: the folder menu lacks Sort: {labels:?}"
                );
                find_menu_button(&popover, "Sort")
                    .expect("E FAIL: the Sort row is not a button")
                    .emit_clicked();
                assert!(
                    commands.borrow().iter().any(|c| c == "Sort"),
                    "E FAIL: the Sort row did not fire ListCommand::Sort"
                );
                println!("E ok: Sort shows on folders only and fires the command");

                // F. The command sorted the folder: C left the folder
                //    as [Beta, Alpha]; Sort makes it [Alpha, Beta].
                assert_eq!(
                    folder_children(&folder),
                    vec!["Probe Alpha", "Probe Beta"],
                    "F FAIL: Sort did not order the folder by name"
                );
                println!("F ok: the folder sorted by name");
                glib::ControlFlow::Break
            }
        });

        // F2. The order reaches the database file: save, reopen, read.
        glib::timeout_add_local(std::time::Duration::from_millis(3800), {
            let ids = ids.clone();
            let app = app.clone();
            move || {
                let folder = ids.borrow()[0];
                cr_ui::library::save().expect("F2 FAIL: save failed");
                let (lib, _) = cr_engine::library::Library::open_at_default_location()
                    .expect("F2 FAIL: reopen failed");
                let reloaded = child_names(&lib.database().comic_lists, &folder);
                assert_eq!(
                    reloaded,
                    vec!["Probe Alpha", "Probe Beta"],
                    "F2 FAIL: the saved file lost the order"
                );
                println!("F2 ok: the order survives a save and reload");
                println!("NAVTREE PROBE DONE");
                app.quit();
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

/// The child names of one folder in the live session tree.
fn folder_children(folder: &CrGuid) -> Vec<String> {
    child_names(&cr_ui::library::comic_lists_snapshot(), folder)
}

fn child_names(items: &[ComicListItem], folder: &CrGuid) -> Vec<String> {
    for item in items {
        if let ComicListItem::Folder(f) = item {
            if f.base.id == *folder {
                return f
                    .items
                    .iter()
                    .map(|i| i.base().name.clone().unwrap_or_default())
                    .collect();
            }
            let nested = child_names(&f.items, folder);
            if !nested.is_empty() {
                return nested;
            }
        }
    }
    Vec::new()
}

/// The menu row labels of a navigator context popover.
fn popover_labels(popover: &gtk4::Popover) -> Vec<String> {
    let Some(child) = popover.child() else {
        return Vec::new();
    };
    let mut labels = Vec::new();
    let mut next = child.first_child();
    while let Some(widget) = next {
        if let Some(button) = widget.clone().downcast_ref::<gtk4::Button>() {
            if let Some(label) = button.label() {
                labels.push(label.to_string());
            }
        }
        next = widget.next_sibling();
    }
    labels
}

fn find_menu_button(popover: &gtk4::Popover, label: &str) -> Option<gtk4::Button> {
    let child = popover.child()?;
    let mut next = child.first_child();
    while let Some(widget) = next {
        if let Ok(button) = widget.clone().downcast::<gtk4::Button>() {
            if button.label().map(|l| l.to_string()).as_deref() == Some(label) {
                return Some(button);
            }
        }
        next = widget.next_sibling();
    }
    None
}
