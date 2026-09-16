//! Headless probe: the Library Organizer (Phase 17).
//! Gates:
//!   A. the config dialog opens with the built-in Default profile,
//!      Browse opens a folder chooser, and OK commits edits,
//!   B. the plugin settings store round-trips and a new profile's Base
//!      folder reloads when that profile is selected,
//!   C. a MOVE run over a seeded three-book library: files land at
//!      the template layout, the undo log is written, and the report
//!      counts 3 successes,
//!   D. the duplicate dialog fires on an existing destination and
//!      Rename lands the file as ` (1)`,
//!   E. the Multi-Value Selection dialog fires for a series-level
//!      writer token and the selection lands in both file names,
//!   F. the undo run restores the original paths.
//!
//! Run: Xvfb + `cargo run -p cr-ui --example organize_probe` with an
//! isolated XDG pair.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::glib::ControlFlow;
use gtk4::prelude::*;

use cr_core::model::comic_book::ComicBook;
use cr_organize::profile::{PluginSettings, Profile};

fn find_toplevel(title: &str) -> Option<gtk4::Window> {
    gtk4::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk4::Window>().ok())
        .find(|w| w.title().as_deref().is_some_and(|t| t.starts_with(title)))
}

fn walk(widget: &gtk4::Widget, out: &mut Vec<gtk4::Widget>) {
    out.push(widget.clone());
    let mut child = widget.first_child();
    while let Some(c) = child {
        walk(&c, out);
        child = c.next_sibling();
    }
}

fn widgets_of(window: &gtk4::Window) -> Vec<gtk4::Widget> {
    let mut widgets = Vec::new();
    if let Some(child) = window.child() {
        walk(&child, &mut widgets);
    }
    widgets
}

fn find_button(window: &gtk4::Window, label: &str) -> gtk4::Button {
    widgets_of(window)
        .into_iter()
        .filter_map(|w| w.downcast::<gtk4::Button>().ok())
        .find(|b| b.label().as_deref() == Some(label))
        .unwrap_or_else(|| panic!("the button '{label}'"))
}

fn find_entries(window: &gtk4::Window) -> Vec<gtk4::Entry> {
    widgets_of(window)
        .into_iter()
        .filter_map(|w| w.downcast::<gtk4::Entry>().ok())
        .collect()
}

fn find_dropdowns(window: &gtk4::Window) -> Vec<gtk4::DropDown> {
    widgets_of(window)
        .into_iter()
        .filter_map(|w| w.downcast::<gtk4::DropDown>().ok())
        .collect()
}

fn seed_book(dir: &std::path::Path, name: &str, series: &str, number: &str) -> ComicBook {
    let path = dir.join(name);
    std::fs::create_dir_all(dir).expect("seed dir");
    std::fs::write(&path, b"page bytes").expect("write the book file");
    let mut b = ComicBook::default();
    b.info.series = series.into();
    b.info.number = number.into();
    b.info.volume = 1;
    b.info.year = 2012;
    b.enable_proposed = false;
    b.file_path = path.to_string_lossy().into_owned();
    b
}

fn profile_with_base(base: &std::path::Path) -> Profile {
    let mut p = Profile::builtin_default();
    p.base_folder = base.to_string_lossy().into_owned();
    p
}

fn walk_files(dir: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                out.extend(walk_files(&p));
            } else {
                out.push(p.to_string_lossy().into_owned());
            }
        }
    }
    out
}

fn watchdog(loop_: &gtk4::glib::MainLoop) {
    let watchdog = loop_.clone();
    gtk4::glib::timeout_add_local(std::time::Duration::from_secs(30), move || {
        watchdog.quit();
        ControlFlow::Break
    });
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
    cr_ui::library::initialize().expect("session");

    let host = gtk4::Window::new();
    host.set_title(Some("organize-probe-host"));
    host.present();

    let work = std::env::temp_dir().join(format!("organize-probe-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&work);
    std::fs::create_dir_all(&work).expect("work dir");
    let undo_path = cr_ui::library::organizer_undo_path();
    let _ = std::fs::remove_file(&undo_path);

    // ------------------------------------------------------------------
    // Gates A + B: the config dialog and the plugin settings store.
    // ------------------------------------------------------------------
    let committed: Rc<RefCell<Vec<PluginSettings>>> = Rc::new(RefCell::new(Vec::new()));
    let settings = PluginSettings::builtin();
    {
        let committed = Rc::clone(&committed);
        cr_ui::dialogs::organize_config::show_organize_config(&host, &settings, None, move |r| {
            if let Some(s) = r {
                committed.borrow_mut().push(s);
            }
        });
    }
    let Some(config_window) = find_toplevel("Configure Library Organizer") else {
        eprintln!("FAIL A: the config dialog did not open");
        std::process::exit(1);
    };
    let entries = find_entries(&config_window);
    let file_entry = entries
        .iter()
        .find(|e| e.text().contains("<number2>"))
        .unwrap_or_else(|| panic!("FAIL A: the file template entry is missing"))
        .clone();
    let base_entry = entries
        .iter()
        .find(|e| e.text().is_empty())
        .unwrap_or_else(|| panic!("FAIL A: the Base folder entry is missing"))
        .clone();
    println!("GATE A OK: the config dialog opens with the built-in profile");

    find_button(&config_window, "Browse…").emit_clicked();
    while gtk4::glib::MainContext::default().pending() {
        gtk4::glib::MainContext::default().iteration(false);
    }
    let browse_failed = match find_toplevel("Choose the base folder") {
        Some(folder_chooser) => {
            folder_chooser
                .downcast::<gtk4::Dialog>()
                .expect("folder chooser dialog")
                .response(gtk4::ResponseType::Cancel);
            false
        }
        None => {
            eprintln!("FAIL A: Browse did not open the Base folder chooser");
            true
        }
    };

    // Create a profile and edit fields: every edit must reach the store.
    find_button(&config_window, "New").emit_clicked();
    let base = work.join("configured-base").to_string_lossy().into_owned();
    base_entry.set_text(&base);
    file_entry.set_text("{<series>}{ #<number2>} edited");
    config_window
        .clone()
        .downcast::<gtk4::Dialog>()
        .expect("dialog")
        .response(gtk4::ResponseType::Ok);
    while committed.borrow().is_empty() {
        gtk4::glib::MainContext::default().iteration(true);
    }
    {
        let store = committed.borrow()[0].clone();
        if store.profiles[1].file_template != "{<series>}{ #<number2>} edited" {
            eprintln!("FAIL B: the selected profile did not receive the template edit");
            std::process::exit(1);
        }
        if store.profiles.len() != 2 || store.profiles[1].base_folder != base {
            eprintln!(
                "FAIL B: the new profile lost the Base folder: {:?}",
                store.profiles.get(1).map(|p| &p.base_folder)
            );
            std::process::exit(1);
        }
        if store.profiles[1].folder_template.is_empty() {
            eprintln!("FAIL B: the committed profile lost the folder template");
            std::process::exit(1);
        }
    }
    let store = committed.borrow()[0].clone();
    cr_ui::library::store_organize_settings(&store);
    let back = cr_ui::library::organize_settings();
    if back != store {
        eprintln!("FAIL B: the settings round trip changed the organizer profiles");
        std::process::exit(1);
    }
    let reloaded: Rc<RefCell<Vec<PluginSettings>>> = Rc::new(RefCell::new(Vec::new()));
    {
        let reloaded = Rc::clone(&reloaded);
        cr_ui::dialogs::organize_config::show_organize_config(&host, &back, None, move |r| {
            if let Some(s) = r {
                reloaded.borrow_mut().push(s);
            }
        });
    }
    let Some(reloaded_window) = find_toplevel("Configure Library Organizer") else {
        eprintln!("FAIL B: the config dialog did not reopen");
        std::process::exit(1);
    };
    let profile_drop = find_dropdowns(&reloaded_window)
        .into_iter()
        .next()
        .expect("FAIL B: the profile selector is missing");
    profile_drop.set_selected(1);
    while gtk4::glib::MainContext::default().pending() {
        gtk4::glib::MainContext::default().iteration(false);
    }
    let reload_failed = !find_entries(&reloaded_window)
        .iter()
        .any(|entry| entry.text() == base);
    if reload_failed {
        eprintln!("FAIL B: the selected profile did not reload its Base folder");
    }
    reloaded_window
        .downcast::<gtk4::Dialog>()
        .expect("reloaded config dialog")
        .response(gtk4::ResponseType::Cancel);
    if browse_failed || reload_failed {
        std::process::exit(1);
    }
    println!("GATE B OK: settings and selected-profile fields round-trip");

    // ------------------------------------------------------------------
    // Gate C: the MOVE run over three seeded books.
    // ------------------------------------------------------------------
    let src = work.join("src");
    std::fs::create_dir_all(&src).unwrap();
    let books: Vec<ComicBook> = ["B 001.cbz", "B 002.cbz", "B 003.cbz"]
        .iter()
        .enumerate()
        .map(|(n, name)| seed_book(&src, name, "Probe Series", &format!("{}", n + 1)))
        .collect();
    let probe_profile = profile_with_base(&work.join("dst"));
    let loop_c = gtk4::glib::MainLoop::new(None, false);
    let quit_c = loop_c.clone();
    let reports: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    {
        let reports = Rc::clone(&reports);
        let quit = quit_c.clone();
        cr_ui::dialogs::organize::show_run_dialog(
            &host,
            books,
            vec![0, 1, 2],
            vec![probe_profile.clone()],
            Some(undo_path.clone()),
            None,
            move |text| {
                reports.borrow_mut().push(text.to_string());
                quit.quit();
            },
        );
    }
    watchdog(&loop_c);
    loop_c.run();

    let dest_dir = work.join("dst").join("Probe Series (2012)");
    if !dest_dir.exists() {
        eprintln!(
            "FAIL C: the destination folder is missing; report={:?}",
            reports.borrow()
        );
        std::process::exit(1);
    }
    let moved = std::fs::read_dir(&dest_dir).expect("dest dir").count();
    if moved != 3 {
        eprintln!("FAIL C: expected 3 moved files in {dest_dir:?}, got {moved}");
        std::process::exit(1);
    }
    let undo = cr_ui::dialogs::organize::load_undo_collection(&undo_path);
    if undo.len() != 3 {
        eprintln!(
            "FAIL C: the undo log holds {} entries, expected 3",
            undo.len()
        );
        std::process::exit(1);
    }
    let report = reports.borrow()[0].clone();
    if !report.contains("Successfully moved: 3") {
        eprintln!("FAIL C: the report lost the successes: {report:?}");
        std::process::exit(1);
    }
    println!("GATE C OK: the move run reorganizes the files and writes the undo log");

    // ------------------------------------------------------------------
    // Gate D: the duplicate dialog + Rename.
    // ------------------------------------------------------------------
    let src2_book = seed_book(&src, "B 004.cbz", "Batman", "5");
    let existing = work
        .join("dst")
        .join("Batman (2012)")
        .join("Batman Vol.1 #05 (2012).cbz");
    std::fs::create_dir_all(existing.parent().unwrap()).expect("dest dir");
    std::fs::write(&existing, b"already here").expect("write the destination");
    let dup_profile = profile_with_base(existing.parent().unwrap().parent().unwrap());
    let loop_d = gtk4::glib::MainLoop::new(None, false);
    let quit_d = loop_d.clone();
    // Answer Rename the moment the duplicate dialog maps.
    gtk4::glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
        if let Some(win) = find_toplevel("Library Organizer — Duplicate") {
            find_button(&win, "Rename").emit_clicked();
            return ControlFlow::Break;
        }
        ControlFlow::Continue
    });
    {
        let reports = Rc::clone(&reports);
        let quit = quit_d.clone();
        cr_ui::dialogs::organize::show_run_dialog(
            &host,
            vec![src2_book],
            vec![0],
            vec![dup_profile],
            None,
            None,
            move |text| {
                reports.borrow_mut().push(text.to_string());
                quit.quit();
            },
        );
    }
    watchdog(&loop_d);
    loop_d.run();
    let renamed = work
        .join("dst")
        .join("Batman (2012)")
        .join("Batman Vol.1 #05 (2012) (1).cbz");
    if !renamed.exists() {
        let tree: Vec<String> = walk_files(&work);
        eprintln!(
            "FAIL D: the renamed duplicate is missing; reports={:?} tree={tree:?}",
            reports.borrow()
        );
        std::process::exit(1);
    }
    if !existing.exists() {
        eprintln!("FAIL D: the rename flow must keep the existing file");
        std::process::exit(1);
    }
    println!("GATE D OK: the duplicate dialog rename lands the file as ' (1)'");

    // ------------------------------------------------------------------
    // Gate E: the Multi-Value Selection dialog for a series-level
    // writer token.
    // ------------------------------------------------------------------
    let src3 = work.join("src3");
    std::fs::create_dir_all(&src3).unwrap();
    let mut w1 = seed_book(&src3, "W 001.cbz", "Multi", "1");
    w1.info.writer = "Ann Author, Bob Writer".into();
    let mut w2 = seed_book(&src3, "W 002.cbz", "Multi", "2");
    w2.info.writer = w1.info.writer.clone();
    let mut mv_profile = profile_with_base(&work.join("dst3"));
    mv_profile.file_template = "{<series>}{ #<number2>}{ <writer(, )(series)>}".into();
    let loop_e = gtk4::glib::MainLoop::new(None, false);
    let quit_e = loop_e.clone();
    // Answer the multi-value dialog: check the first value, OK.
    gtk4::glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
        if let Some(win) = find_toplevel("Choose which Writers") {
            let checks = widgets_of(&win)
                .into_iter()
                .filter_map(|w| w.downcast::<gtk4::CheckButton>().ok())
                .collect::<Vec<_>>();
            if let Some(first) = checks.first() {
                first.set_active(true);
            }
            find_button(&win, "OK").emit_clicked();
            return ControlFlow::Break;
        }
        ControlFlow::Continue
    });
    {
        let reports = Rc::clone(&reports);
        let quit = quit_e.clone();
        cr_ui::dialogs::organize::show_run_dialog(
            &host,
            vec![w1, w2],
            vec![0, 1],
            vec![mv_profile.clone()],
            None,
            None,
            move |text| {
                reports.borrow_mut().push(text.to_string());
                quit.quit();
            },
        );
    }
    watchdog(&loop_e);
    loop_e.run();
    let dir3 = work.join("dst3").join("Multi (2012)");
    if !dir3.exists() {
        let tops: Vec<String> = gtk4::Window::list_toplevels()
            .into_iter()
            .filter_map(|w| w.downcast::<gtk4::Window>().ok())
            .filter_map(|w| w.title().map(|t| t.to_string()))
            .collect();
        eprintln!(
            "FAIL E: no folder; reports={:?} toplevels={tops:?}",
            reports.borrow()
        );
        std::process::exit(1);
    }
    let moved3: Vec<String> = std::fs::read_dir(&dir3)
        .unwrap_or_else(|e| panic!("FAIL E: the multi-value run produced no folder: {e}"))
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    if moved3.len() != 2 {
        eprintln!("FAIL E: expected 2 renamed books, got {moved3:?}");
        std::process::exit(1);
    }
    for name in &moved3 {
        if !name.contains("Ann Author") {
            eprintln!(
                "FAIL E: the series-level selection must land in every file name: {moved3:?}"
            );
            std::process::exit(1);
        }
    }
    println!("GATE E OK: the multi-value selection lands in every file of the series");

    // ------------------------------------------------------------------
    // Gate F: the undo run restores the original paths.
    // ------------------------------------------------------------------
    let undo_books: Vec<ComicBook> = undo
        .current_paths
        .iter()
        .map(|path| {
            let mut b = ComicBook::default();
            b.info.series = "Probe Series".into();
            b.enable_proposed = false;
            b.file_path = path.clone();
            b
        })
        .collect();
    let undo_profiles: std::collections::HashMap<String, Profile> =
        std::iter::once(("Default".to_string(), probe_profile.clone())).collect();
    let loop_f = gtk4::glib::MainLoop::new(None, false);
    let quit_f = loop_f.clone();
    let undo_reports: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
    {
        let reports = Rc::clone(&undo_reports);
        let quit = quit_f.clone();
        let collection = cr_ui::dialogs::organize::load_undo_collection(&undo_path);
        cr_ui::dialogs::organize::show_undo_dialog(
            &host,
            undo_books,
            collection,
            undo_profiles,
            None,
            move |text| {
                reports.borrow_mut().push(text.to_string());
                quit.quit();
            },
        );
    }
    watchdog(&loop_f);
    loop_f.run();
    let undo_report = undo_reports.borrow()[0].clone();
    if !undo_report.contains("Successfully moved: 3") {
        eprintln!("FAIL F: the undo run lost books: {undo_report:?}");
        std::process::exit(1);
    }
    let original = std::path::PathBuf::from(&undo.undo_paths[0]);
    if !original.exists() {
        eprintln!(
            "FAIL F: the first book did not return to {}",
            undo.undo_paths[0]
        );
        std::process::exit(1);
    }
    println!("GATE F OK: the undo run restores the original paths");

    let _ = std::fs::remove_dir_all(&work);
    println!("PROBE DONE");
    std::process::exit(0);
}
