//! Headless probe: the duplicate cleanup (PORT ADDITION, no C#
//! counterpart — ADR-044). Seeds a library with four duplicate groups
//! (CBR/CBZ pair, a conflicting pair, an exact tie, a fileless pair)
//! plus one non-duplicate book, and gates:
//!   A. Views ▸ Show Duplicates narrows the grid to the duplicate
//!      members (8 of 9).
//!   B. `win.select-worst-duplicates` selects the expected worst
//!      copies (the CBR of the clear group, the fileless entry).
//!   C. With the format rule off, the conflict group's smaller CBZ
//!      marks (the ADR-044 score-sum behavior).
//!   D. With every rule off, the command selects nothing.
//!   E. The book context menu row fires the same command.
//!   F. The Preferences Duplicates page opens, its three rows read
//!      the settings, and OK commits them (session + file); the
//!      incoming-path entry reads the empty setting and commits the
//!      typed path the same way.
//!
//! Run: Xvfb + `cargo run -p cr-ui --example duplicates_probe` with
//! an isolated XDG pair (the probe seeds books into the DB it opens).
use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::{CrDateTime, CrGuid};
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{Button, Dialog};

fn find_toplevel(title: &str) -> Option<gtk4::Window> {
    gtk4::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk4::Window>().ok())
        .find(|w| w.title().as_deref().is_some_and(|t| t.starts_with(title)))
}

fn find_button(root: &gtk4::Widget, label: &str) -> Option<Button> {
    let mut stack = vec![root.clone()];
    while let Some(w) = stack.pop() {
        if let Ok(b) = w.clone().downcast::<Button>() {
            if b.label().map(|l| l == label).unwrap_or(false) {
                return Some(b);
            }
        }
        let mut child = w.first_child();
        while let Some(c) = child {
            stack.push(c.clone());
            child = c.next_sibling();
        }
    }
    None
}

/// (series, number, file path, file size, page count) — the physical
/// format comes from the path extension; an empty path is fileless.
fn seed(series: &str, number: &str, path: &str, size: i64, pages: i32) -> ComicBook {
    let mut b = ComicBook {
        id: CrGuid::new_random(),
        file_path: path.into(),
        file_size: size,
        added_time: CrDateTime::now(),
        ..Default::default()
    };
    b.info.series = series.into();
    b.info.number = number.into();
    b.info.page_count = pages;
    b
}

/// The seeded library. `label` is the unique `Title` (the groups are
/// identified by it; the title is NOT a duplicate key).
fn seed_library() -> Vec<ComicBook> {
    let mut books = Vec::new();
    let mut push = |label: &str, series: &str, path: &str, size: i64, pages: i32| {
        let mut b = seed(series, "1", path, size, pages);
        b.info.title = label.into();
        books.push(b);
    };
    // Group 1: the clear case — the CBZ wins on every rule.
    push("g1-keep", "Alpha Probe", "/comics/alpha.cbz", 2000, 20);
    push("g1-worst", "Alpha Probe", "/comics/alpha.cbr", 1000, 10);
    // Group 2: the conflict — the CBR is larger, both lose one rule.
    push("g2-cbz", "Conflict Probe", "/comics/conflict.cbz", 500, 30);
    push("g2-cbr", "Conflict Probe", "/comics/conflict.cbr", 900, 30);
    // Group 3: the exact tie — nothing may mark.
    push("g3-a", "Tie Probe", "/comics/tie-a.cbz", 1000, 20);
    push("g3-b", "Tie Probe", "/comics/tie-b.cbz", 1000, 20);
    // Group 4: the fileless entry ranks worst.
    push("g4-fileless", "Fileless Probe", "", -1, 0);
    push(
        "g4-keep",
        "Fileless Probe",
        "/comics/fileless.cbz",
        1000,
        15,
    );
    // Not a duplicate: never ranks.
    push("solo", "Solo Probe", "/comics/solo.cbz", 100, 5);
    books
}

/// The selected titles (sorted — the selection set order is not
/// stable).
fn selection_titles(shell: &cr_ui::browser::shell::BrowserShell) -> Vec<String> {
    let view = shell.item_view_state();
    let mut titles: Vec<String> = shell
        .state_grid_selection_ids()
        .iter()
        .filter_map(|id| {
            (0..view.len())
                .find(|&i| view.book(i).id == *id)
                .map(|i| format!("{} #{}", view.book(i).info.series, view.book(i).info.title))
        })
        .collect();
    titles.sort();
    titles
}

/// Flips the duplicate rules in the session settings — all FOUR
/// quality rules (the ADR-044 set; the incoming path is a separate
/// setting the probes do not arm).
fn set_rule(cbr: bool, smaller: bool, fewer: bool, older: bool) {
    let s = cr_ui::library::settings();
    let mut s = s.borrow_mut();
    s.duplicates_cbr_worse_than_cbz = cbr;
    s.duplicates_smaller_file_worse = smaller;
    s.duplicates_fewer_pages_worse = fewer;
    s.duplicates_older_file_worse = older;
    drop(s);
    cr_ui::library::save_settings();
}

fn fail(msg: &str) -> ! {
    eprintln!("FAIL: {msg}");
    std::process::exit(1)
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
        eprintln!("REFUSED: set XDG_DATA_HOME and XDG_CONFIG_HOME to /tmp/opencode/<dir> (an isolated pair)");
        std::process::exit(1);
    }
    let work = std::path::Path::new("/tmp/opencode/duplicates");
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).unwrap();

    let books = seed_library();
    let (mut lib, _) = cr_engine::library::Library::open_at_default_location().unwrap();
    lib.database_mut().books = books;
    lib.save().unwrap();
    cr_ui::library::initialize().expect("session init");
    set_rule(true, true, true, true);

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.duplicates-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let app = app.clone();
        let (window, shell) = cr_ui::browser::shell::BrowserShell::create(&app);
        window.present();
        std::mem::forget(shell.clone());
        let shell = shell.clone();

        // A. The boot grid holds all 9 books; the duplicates toggle
        //     narrows to the 8 duplicate members.
        glib::timeout_add_local(std::time::Duration::from_millis(700), {
            let shell = shell.clone();
            move || {
                let total = shell.state_grid_book_count();
                if total != 9 {
                    fail(&format!("A0 boot grid holds {total} books (expect 9)"));
                }
                shell.state_dispatch("win.duplicates-only");
                let dups = shell.state_grid_book_count();
                let on = shell.state_action_bool("duplicates-only");
                println!("A total={total} duplicates={dups} toggle={on:?} (expect 9/8/Some(true))");
                if dups != 8 || on != Some(true) {
                    fail("A the duplicates view narrowed wrong");
                }
                glib::ControlFlow::Break
            }
        });

        // B. The command selects the worst copies of the four groups.
        glib::timeout_add_local(std::time::Duration::from_millis(1100), {
            let shell = shell.clone();
            move || {
                if !shell.state_dispatch("win.select-worst-duplicates") {
                    fail("B the select-worst-duplicates action did not resolve");
                }
                let sel = selection_titles(&shell);
                let expect: Vec<String> = ["Alpha Probe #g1-worst", "Fileless Probe #g4-fileless"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                println!("B selection={sel:?} (expect {expect:?})");
                if sel != expect {
                    fail("B the worst copies did not select");
                }
                glib::ControlFlow::Break
            }
        });

        // C. The format rule off: the conflict group's smaller CBZ
        //     marks (the score-sum consequence — ADR-044).
        glib::timeout_add_local(std::time::Duration::from_millis(1500), {
            let shell = shell.clone();
            move || {
                set_rule(false, true, true, true);
                shell.state_dispatch("win.select-worst-duplicates");
                let sel = selection_titles(&shell);
                let expect: Vec<String> = [
                    "Alpha Probe #g1-worst",
                    "Conflict Probe #g2-cbz",
                    "Fileless Probe #g4-fileless",
                ]
                .iter()
                .map(|s| s.to_string())
                .collect();
                println!("C selection={sel:?} (expect {expect:?})");
                if sel != expect {
                    fail("C the rule change did not change the ranking");
                }
                glib::ControlFlow::Break
            }
        });

        // D. Every rule off: nothing marks; the selection clears.
        glib::timeout_add_local(std::time::Duration::from_millis(1900), {
            let shell = shell.clone();
            move || {
                set_rule(false, false, false, false);
                shell.state_dispatch("win.select-worst-duplicates");
                let sel = selection_titles(&shell);
                println!("D selection={sel:?} (expect [])");
                if !sel.is_empty() {
                    fail("D an all-tie group marked copies");
                }
                set_rule(true, true, true, true);
                glib::ControlFlow::Break
            }
        });

        // E. The real context menu row fires the same command.
        glib::timeout_add_local(std::time::Duration::from_millis(2300), {
            let shell = shell.clone();
            move || {
                if !shell.state_open_context(300.0, 300.0) {
                    fail("E the book context menu did not open");
                }
                let pop = shell.state_context_popover().expect("the context popover");
                let clicked = pop
                    .child()
                    .as_ref()
                    .and_then(|c| find_button(c, "Select Worst Duplicates"))
                    .map(|b| b.emit_clicked())
                    .is_some();
                if !clicked {
                    fail("E the Select Worst Duplicates row is missing");
                }
                let sel = selection_titles(&shell);
                let expect: Vec<String> = ["Alpha Probe #g1-worst", "Fileless Probe #g4-fileless"]
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                println!("E row-click selection={sel:?} (expect {expect:?})");
                if sel != expect {
                    fail("E the context-menu row fired the wrong command");
                }
                glib::ControlFlow::Break
            }
        });

        // F. The Preferences Duplicates page: the rows read the
        //     settings and OK commits them.
        glib::timeout_add_local(std::time::Duration::from_millis(2800), {
            let window = window.clone();
            move || {
                cr_ui::settings::show_preferences(&window, Some("duplicates"), || {});
                let Some(dialog) = find_toplevel("Preferences") else {
                    fail("F the Preferences dialog did not open");
                };
                let stack = {
                    let mut found = None;
                    let mut walk = vec![dialog.child().expect("dialog child")];
                    while let Some(w) = walk.pop() {
                        if let Ok(s) = w.clone().downcast::<gtk4::Stack>() {
                            found = Some(s);
                            break;
                        }
                        let mut child = w.first_child();
                        while let Some(c) = child {
                            walk.push(c.clone());
                            child = c.next_sibling();
                        }
                    }
                    found.expect("the Preferences stack")
                };
                if stack.visible_child_name().map(|n| n.to_string()).as_deref()
                    != Some("duplicates")
                {
                    fail(&format!(
                        "F the duplicates page did not open (visible {:?})",
                        stack.visible_child_name().map(|n| n.to_string())
                    ));
                }
                let find_check = |label: &str| -> gtk4::CheckButton {
                    let mut found = None;
                    let mut walk = vec![dialog.child().unwrap()];
                    while let Some(w) = walk.pop() {
                        if let Ok(c) = w.clone().downcast::<gtk4::CheckButton>() {
                            if c.label().map(|l| l == label).unwrap_or(false) {
                                found = Some(c);
                                break;
                            }
                        }
                        let mut child = w.first_child();
                        while let Some(ch) = child {
                            walk.push(ch.clone());
                            child = ch.next_sibling();
                        }
                    }
                    found.unwrap_or_else(|| panic!("F the {label} row is missing"))
                };
                let cbr = find_check("CBR copies are worse than CBZ copies");
                let smaller = find_check("Smaller files are worse than larger files");
                let fewer = find_check("Fewer pages are worse than more pages");
                if !(cbr.is_active() && smaller.is_active() && fewer.is_active()) {
                    fail("F the rows do not read the (all-on) settings");
                }
                // The incoming-path entry (ADR-046): it reads the
                // empty setting, and the typed path commits with the
                // checkboxes.
                let find_entry = || -> gtk4::Entry {
                    let mut found = None;
                    let mut walk = vec![dialog.child().unwrap()];
                    while let Some(w) = walk.pop() {
                        if let Ok(e) = w.clone().downcast::<gtk4::Entry>() {
                            if e.placeholder_text()
                                .map(|p| p == "/data/incoming")
                                .unwrap_or(false)
                            {
                                found = Some(e);
                                break;
                            }
                        }
                        let mut child = w.first_child();
                        while let Some(ch) = child {
                            walk.push(ch.clone());
                            child = ch.next_sibling();
                        }
                    }
                    found.unwrap_or_else(|| panic!("F the incoming-path entry is missing"))
                };
                let path_entry = find_entry();
                if !path_entry.text().is_empty() {
                    fail("F the incoming-path entry does not read the empty setting");
                }
                cbr.set_active(false);
                smaller.set_active(false);
                path_entry.set_text("/data/incoming");
                dialog
                    .clone()
                    .downcast::<Dialog>()
                    .expect("the Preferences dialog is a Dialog")
                    .response(gtk4::ResponseType::Ok);
                let s = cr_ui::library::settings().borrow().clone();
                if s.duplicates_cbr_worse_than_cbz || s.duplicates_smaller_file_worse {
                    fail("F the OK commit did not land in the session settings");
                }
                if s.duplicates_incoming_path != "/data/incoming" {
                    fail("F the incoming path did not land in the session settings");
                }
                // The commit is on disk too (the whole-file rewrite).
                let cfg = cr_core::paths::config_file(&cr_core::paths::Paths::new_default());
                let text = std::fs::read_to_string(&cfg).unwrap_or_default();
                if !text.contains("DuplicatesCbrWorseThanCbz = false") {
                    fail("F the config file did not record the commit");
                }
                if !text.contains("DuplicatesIncomingPath = \"/data/incoming\"") {
                    fail("F the config file did not record the incoming path");
                }
                println!("F rows read + commit OK (session + file)");
                set_rule(true, true, true, true);
                {
                    let s = cr_ui::library::settings();
                    s.borrow_mut().duplicates_incoming_path = String::new();
                }
                cr_ui::library::save_settings();
                println!("PROBE COMPLETE");
                app.quit();
                glib::ControlFlow::Break
            }
        });
    });

    app.run();
}
