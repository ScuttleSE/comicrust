//! Headless probe: the Phase 6 fileless-book features (ADR-027).
//! Gates: the idempotent library insert (`insert_new_book` — the
//! editor commits fire per save point), the "New fileless Book
//! Series…" dialog (the OK-enable rule, the created run of fileless
//! books, the selection), the >100 sanity abort, the "New fileless
//! Book Entry…" editor opening and Cancel adding nothing, the
//! FilelessMarker icon load, and the reader-open gate (an empty path
//! never opens a slot — the C# `NavigatorManager.Open` `IsLinked`
//! rule).
//! Run: Xvfb + `cargo run -p cr-ui --example newbook_probe` with an
//! isolated XDG (the probe inserts books into the DB it opens).
use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::CrDateTime;
use gtk4::glib;
use gtk4::prelude::*;

fn find_dialog(title: &str) -> Option<gtk4::Dialog> {
    gtk4::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk4::Dialog>().ok())
        .find(|w| w.title().as_deref().is_some_and(|t| t.starts_with(title)))
}

/// Full widget-tree walk (first_child/next_sibling — no container
/// type assumptions).
fn walk(widget: &gtk4::Widget, out: &mut Vec<gtk4::Widget>) {
    out.push(widget.clone());
    let mut child = widget.first_child();
    while let Some(c) = child {
        walk(&c, out);
        child = c.next_sibling();
    }
}

fn dialog_entries(dialog: &gtk4::Dialog) -> Vec<gtk4::Entry> {
    let mut widgets = Vec::new();
    if let Some(child) = dialog.child() {
        walk(&child, &mut widgets);
    }
    widgets
        .into_iter()
        .filter_map(|w| w.downcast::<gtk4::Entry>().ok())
        .collect()
}

fn ok_enabled(dlg: &gtk4::Dialog) -> bool {
    dlg.widget_for_response(gtk4::ResponseType::Ok)
        .map(|b| b.is_sensitive() && b.is_visible())
        .unwrap_or(false)
}

fn library_books() -> Vec<ComicBook> {
    let lib = cr_ui::library::session();
    let l = lib.borrow();
    l.database().books.clone()
}

fn dispatch(window: &impl IsA<gtk4::Window>, action: &str) {
    let _ = gtk4::prelude::WidgetExt::activate_action(window.upcast_ref(), action, None);
}

fn main() {
    gtk4::init().expect("gtk init");
    cr_ui::theme::init();
    if !std::env::var("XDG_DATA_HOME")
        .map(|v| v.contains("/tmp/opencode"))
        .unwrap_or(false)
    {
        eprintln!("REFUSED: set XDG_DATA_HOME=/tmp/opencode/<dir> (the probe inserts books into the DB it opens)");
        std::process::exit(1);
    }
    cr_ui::library::initialize().expect("session init");

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.newbook-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let (window, shell) = cr_ui::browser::shell::BrowserShell::create(app);
        window.present();
        // The probe must keep the shell alive (the thread-local
        // lesson) — every action handler holds Weak<ShellState>.
        std::mem::forget(shell.clone());

        // A. The insert contract: the same id inserts once.
        glib::timeout_add_local(std::time::Duration::from_millis(800), {
            let shell = shell.clone();
            move || {
                let _ = &shell;
                let before = library_books().len();
                let book = cr_ui::dialogs::new_book_series::new_fileless_book();
                let first = cr_ui::library::insert_new_book(&book);
                let second = cr_ui::library::insert_new_book(&book);
                let delta = library_books().len() - before;
                println!(
                    "INSERT first={first} second={second} delta={delta} (expect true/false/1)"
                );
                glib::ControlFlow::Break
            }
        });

        // B. The series dialog: OK-disabled on empty input.
        glib::timeout_add_local(std::time::Duration::from_millis(1000), {
            let window = window.clone();
            move || {
                dispatch(&window, "win.new-book-series");
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(1300), {
            move || {
                let Some(dlg) = find_dialog("New fileless Book Series") else {
                    println!("SERIES DIALOG MISSING");
                    return glib::ControlFlow::Break;
                };
                let entries = dialog_entries(&dlg);
                let empty_ok = ok_enabled(&dlg);
                entries[0].set_text("Probe Series");
                entries[1].set_text("3");
                entries[2].set_text("5");
                entries[3].set_text("7");
                println!(
                    "SERIES OPEN entries={} ok-enabled-empty={empty_ok} ok-enabled-filled={}",
                    entries.len(),
                    ok_enabled(&dlg)
                );
                dlg.response(gtk4::ResponseType::Ok);
                glib::ControlFlow::Break
            }
        });
        // C. The created run: three fileless books + the selection.
        glib::timeout_add_local(std::time::Duration::from_millis(1700), {
            let shell = shell.clone();
            move || {
                let books = library_books();
                let created: Vec<ComicBook> = books
                    .iter()
                    .filter(|b| b.info.series == "Probe Series")
                    .cloned()
                    .collect();
                println!("SERIES CREATED {}", created.len());
                for b in &created {
                    println!(
                        "BOOK number={} volume={} path-empty={} added={}",
                        b.info.number,
                        b.info.volume,
                        b.file_path.is_empty(),
                        b.added_time.naive > CrDateTime::min_value().naive
                    );
                }
                let selected = shell.state_grid_selection_ids();
                let matches = created.iter().filter(|b| selected.contains(&b.id)).count();
                println!(
                    "SELECTED {} MATCH {matches}/{} (expect match 3/3)",
                    selected.len(),
                    created.len()
                );
                glib::ControlFlow::Break
            }
        });

        // D. The >100 sanity abort (silent).
        glib::timeout_add_local(std::time::Duration::from_millis(1900), {
            let window = window.clone();
            move || {
                dispatch(&window, "win.new-book-series");
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(2200), {
            move || {
                let Some(dlg) = find_dialog("New fileless Book Series") else {
                    println!("SERIES DIALOG 2 MISSING");
                    return glib::ControlFlow::Break;
                };
                let entries = dialog_entries(&dlg);
                entries[0].set_text("Too Many");
                entries[1].set_text("1");
                entries[2].set_text("1");
                entries[3].set_text("150");
                let mark = library_books().len();
                dlg.response(gtk4::ResponseType::Ok);
                glib::timeout_add_local(std::time::Duration::from_millis(300), move || {
                    let delta = library_books().len() - mark;
                    println!("ABORT delta={delta} (expect 0)");
                    glib::ControlFlow::Break
                });
                glib::ControlFlow::Break
            }
        });

        // E. The single-entry editor opens; Cancel adds nothing.
        glib::timeout_add_local(std::time::Duration::from_millis(2800), {
            let window = window.clone();
            move || {
                dispatch(&window, "win.new-book-entry");
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(3300), {
            move || {
                let Some(dlg) = find_dialog("Book") else {
                    println!("EDITOR MISSING");
                    return glib::ControlFlow::Break;
                };
                let mark = library_books().len();
                dlg.response(gtk4::ResponseType::Cancel);
                glib::timeout_add_local(std::time::Duration::from_millis(300), move || {
                    let delta = library_books().len() - mark;
                    println!("EDITOR CANCEL delta={delta} (expect 0)");
                    glib::ControlFlow::Break
                });
                glib::ControlFlow::Break
            }
        });

        // F. The marker icon, the reader-open gate, the fileless
        //    books in the Library evaluation.
        glib::timeout_add_local(std::time::Duration::from_millis(3900), {
            let shell = shell.clone();
            move || {
                let marker = cr_ui::icon::image_for_name("FilelessMarker");
                println!("MARKER LOADED {}", marker.is_some());

                let stack_before = shell.state_stack_page();
                shell.open_comic(std::path::Path::new(""));
                println!(
                    "OPEN GATE stayed={} page-before={stack_before}",
                    shell.state_stack_page() == stack_before
                );

                let fileless = library_books()
                    .iter()
                    .filter(|b| b.file_path.is_empty())
                    .count();
                println!("FILELESS count={fileless}");
                println!("NEWBOOK PROBE DONE");
                glib::ControlFlow::Break
            }
        });

        glib::timeout_add_local(std::time::Duration::from_secs(12), || {
            eprintln!("TIMEOUT — probe did not finish");
            std::process::exit(2);
        });
    });

    app.run();
}
