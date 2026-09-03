//! The GTK application shell. Phase 3 keeps this minimal: an
//! application window with an Open dialog (or a comic path from the
//! command line) that opens a reader window. D-Bus single instance
//! and the full browser shell arrive in later phases; the app runs
//! `NON_UNIQUE` until then.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{gio, glib};
use gtk4::{
    Application, ApplicationWindow, Button, FileChooserAction, FileChooserNative, FileFilter,
    ResponseType, Window,
};

use cr_core::xml::scalar::CrGuid;

use crate::browser;
use crate::library;
use crate::reader_window::{self, ReaderWindow};
use crate::theme;

pub const APP_ID: &str = "org.comicrust.ComicRust";

/// File-dialog filter extensions (`KnownFileFormats` + folders; the
/// C# open dialog uses the same list from the provider registry).
const OPEN_FILTER_EXTS: &[&str] = &[
    "cbz", "zip", "cbr", "rar", "cb7", "7z", "cbt", "tar", "pdf", "djvu",
];

pub fn run(args: Vec<String>) {
    // gtk4-rs requires explicit init before any object construction
    // (the C# calls Application.EnableVisualStyles at the same point).
    gtk4::init().expect("GTK initialization failed");
    theme::init();
    let app = Application::builder()
        .application_id(APP_ID)
        // NON_UNIQUE: D-Bus single instance is a Phase 7 item.
        // HANDLES_OPEN: GApplication routes positional file arguments
        // to the `open` signal — the C# exe-association behavior.
        .flags(gio::ApplicationFlags::NON_UNIQUE | gio::ApplicationFlags::HANDLES_OPEN)
        .build();
    let _ = args;

    // The library session (`Program.DatabaseManager.Open` at startup).
    match library::initialize() {
        Ok(message) => set_open_message(message),
        Err(err) => set_open_message(Some(format!(
            "There was an error opening the Database:\n{err}"
        ))),
    }

    // `DatabaseBackgroundSaving` (default 600 s): the periodic save
    // while the library is dirty.
    glib::timeout_add_local(
        std::time::Duration::from_secs(cr_engine::library::BACKGROUND_SAVE_INTERVAL_SECS),
        || {
            if let Err(err) = library::save_if_dirty() {
                eprintln!("background save failed: {err}");
            }
            glib::ControlFlow::Continue
        },
    );

    // The watch-folder poll: debounced watch events map back to the
    // stored watch roots and each root rescans on the scan worker
    // (`remove_missing: false` — vanished files flag as missing).
    glib::timeout_add_local(std::time::Duration::from_secs(1), || {
        for root in library::take_watch_folder_rescans() {
            library::add_folder_to_library(Path::new(&root), |_| {});
        }
        glib::ControlFlow::Continue
    });

    app.connect_activate(show_shell);
    app.connect_open(|app, files, _| {
        for file in files {
            if let Some(path) = file.path() {
                open_reader(app, &path);
            }
        }
    });
    app.run();
}

// The `DatabaseManager.OpenMessage` — captured at startup, shown
// with the shell (the C# shows it in an attention box on the main
// form; a dialog built before the GApplication `startup` signal
// warns "New application windows must be added...").
thread_local! {
    static OPEN_MESSAGE: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn set_open_message(message: Option<String>) {
    OPEN_MESSAGE.with(|cell| *cell.borrow_mut() = message);
}

/// The app shell — the future browser window (Phase 4 T5 replaces
/// the placeholder center pane with the ItemView): the list navigator
/// on the left, a placeholder that shows the evaluated list on the
/// right, and the file commands in the header.
fn show_shell(app: &Application) {
    let win = ApplicationWindow::builder()
        .application(app)
        .title("comicrust")
        .default_width(1100)
        .default_height(700)
        .build();
    if let Some(message) = OPEN_MESSAGE.with(|cell| cell.borrow().clone()) {
        show_attention_dialog(&win, &message);
    }

    let header = gtk4::HeaderBar::new();
    let open = Button::with_label("Open…");
    let win_clone = win.clone();
    open.connect_clicked(move |_| open_file_dialog(&win_clone));
    header.pack_start(&open);

    // `AddFolderToLibrary` (the browser command).
    let add_folder = Button::with_label("Add Folder to Library…");
    let win_for_click = win.clone();
    add_folder.connect_clicked(move |_| {
        add_folder_dialog(&win_for_click);
    });
    header.pack_start(&add_folder);
    win.set_titlebar(Some(&header));

    // Navigator pane + placeholder (the ItemView arrives in T3).
    let paned = gtk4::Paned::new(gtk4::Orientation::Horizontal);
    let navigator = browser::navigator::Navigator::new();
    paned.set_start_child(Some(navigator.widget()));
    paned.set_shrink_start_child(false);
    paned.set_position(280);

    let placeholder = gtk4::Label::new(None);
    placeholder.set_valign(gtk4::Align::Center);
    placeholder.set_halign(gtk4::Align::Center);
    placeholder.set_hexpand(true);
    placeholder.set_vexpand(true);
    paned.set_end_child(Some(&placeholder));
    paned.set_shrink_end_child(false);
    win.set_child(Some(&paned));

    // Selection → evaluate the list (debounced inside the widget);
    // the placeholder shows what the browser would display.
    navigator.connect_selected(move |_id, name| {
        let result = library::evaluate_list(_id);
        let text = match result {
            Some((list_name, _ids, count)) => {
                format!("{list_name}\n{count} book(s)")
            }
            None => String::new(),
        };
        placeholder.set_text(&text);
        let _ = name;
    });

    // Context-menu commands (the C# navigator commands, dialogs are
    // Phase 5 — bare entry dialogs here).
    {
        let nav = navigator.clone();
        let win_for_cmds = win.clone();
        navigator.connect_command(move |command, target| {
            run_list_command(&win_for_cmds, &nav, command, target);
        });
    }

    // Fill the tree from the session DB.
    navigator.refill(&library::comic_lists_snapshot());

    // The launcher is the app's main window until the browser lands:
    // closing it is app exit — save the library
    // (`MainFormFormClosed` → `CleanUp` parity). Without this, a scan
    // or reading session that never opened a reader window would be
    // discarded.
    win.connect_close_request(|_| {
        if let Err(err) = library::save() {
            eprintln!("library save failed: {err}");
        }
        glib::Propagation::Proceed
    });

    win.present();
}

/// The navigator context-menu commands (`NewSmartList`, `NewFolder`,
/// `RenameNode`, `RemoveListOrFolder`). Small entry dialogs instead of
/// the Phase 5 editors.
fn run_list_command(
    parent: &ApplicationWindow,
    nav: &Rc<browser::navigator::Navigator>,
    command: browser::navigator::ListCommand,
    target: Option<CrGuid>,
) {
    use browser::navigator::ListCommand;
    match command {
        ListCommand::NewSmartList => {
            let dialog = entry_dialog(parent, "New Smart List", "Name", "New Smart List");
            if let Some(name) = dialog {
                // The C# query form: `Match` + `[matcher name]`
                // operator "value" (the editor UI is Phase 5; the C#
                // SmartListDialog generates this text via
                // `ComicSmartListItem.ToString()`).
                let query_dialog = entry_dialog(
                    parent,
                    "New Smart List",
                    "Match query  (Match [Name] contains \"text\")",
                    "Match [Series] contains \"Batman\"",
                );
                let query = query_dialog.unwrap_or_default();
                if let Err(err) = library::new_smart_list(target.as_ref(), &name, &query) {
                    show_attention_dialog(parent, &format!("Bad query: {err}"));
                }
                nav.refill(&library::comic_lists_snapshot());
            }
        }
        ListCommand::NewFolder => {
            if let Some(name) = entry_dialog(parent, "New Folder", "Name", "New Folder") {
                library::new_folder(target.as_ref(), &name);
                nav.refill(&library::comic_lists_snapshot());
            }
        }
        ListCommand::Rename => {
            if let Some(id) = target {
                if let Some(name) = entry_dialog(parent, "Rename", "Name", "") {
                    library::rename_list(&id, &name);
                    nav.refill(&library::comic_lists_snapshot());
                }
            }
        }
        ListCommand::Delete => {
            if let Some(id) = target {
                library::remove_list(&id);
                nav.refill(&library::comic_lists_snapshot());
            }
        }
    }
}

/// A one-field prompt. Returns the entered text, or `None` when the
/// dialog was cancelled. (The C# inline label edit lands with the
/// Phase 5 dialogs; this is the T2 stand-in.)
fn entry_dialog(
    parent: &ApplicationWindow,
    title: &str,
    label: &str,
    initial: &str,
) -> Option<String> {
    let dialog = gtk4::Dialog::builder()
        .title(title)
        .transient_for(parent)
        .modal(true)
        .build();
    dialog.add_button("Cancel", gtk4::ResponseType::Cancel);
    dialog.add_button("OK", gtk4::ResponseType::Ok);
    dialog.set_default_response(gtk4::ResponseType::Ok);
    let content = dialog.content_area();
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.set_spacing(6);
    let label = gtk4::Label::with_mnemonic(label);
    content.append(&label);
    let entry = gtk4::Entry::new();
    entry.set_text(initial);
    entry.set_activates_default(true);
    content.append(&entry);
    // The response closure cannot return out — park the result in a
    // cell the wait loop reads back.
    let result: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
    let r = result.clone();
    dialog.connect_response(move |d, response| {
        if response == gtk4::ResponseType::Ok {
            *r.borrow_mut() = Some(entry.text().into());
        }
        d.destroy();
    });
    dialog.show();
    // Nested iteration until the dialog closes (a modal prompt).
    while dialog.is_visible() {
        glib::MainContext::default().iteration(true);
    }
    let out = result.borrow().clone();
    out
}

pub fn open_file_dialog(parent: &impl IsA<Window>) {
    // FileChooserNative (GTK 4.0-era API): the runner's GTK version is
    // unknown, so the shell avoids version-gated widgets.
    let chooser = FileChooserNative::builder()
        .title("Open Comic")
        .action(FileChooserAction::Open)
        .transient_for(parent)
        .modal(true)
        .build();
    let filter = FileFilter::new();
    filter.set_name(Some("Comic files"));
    for ext in OPEN_FILTER_EXTS {
        filter.add_pattern(&format!("*.{ext}"));
    }
    chooser.add_filter(&filter);
    let parent_window: Window = parent.clone().upcast();
    chooser.connect_response(move |chooser, response| {
        if response != ResponseType::Accept {
            return;
        }
        let Some(path) = chooser.file().and_then(|f| f.path()) else {
            return;
        };
        let Some(app) = parent_window.application() else {
            return;
        };
        if let Some(app) = app.downcast_ref::<Application>() {
            open_reader(app, &path);
            // The shell is only a launcher until the Phase 4 browser
            // lands: a successful open hands the session to the
            // reader window and the shell goes away.
            parent_window.close();
        }
    });
    chooser.show();
}

/// `AddFolderToLibrary`: folder chooser → a recursive scan into the
/// library on the scan worker (the UI stays responsive; the C# shows
/// the scan progress in the status strip). The result dialog reports
/// the scan.
fn add_folder_dialog(parent: &impl IsA<Window>) {
    let chooser = FileChooserNative::builder()
        .title("Add Folder to Library")
        .action(FileChooserAction::SelectFolder)
        .transient_for(parent)
        .modal(true)
        .build();
    let window: Window = parent.clone().upcast();
    chooser.connect_response(move |chooser, response| {
        if response != ResponseType::Accept {
            return;
        }
        let Some(path) = chooser.file().and_then(|f| f.path()) else {
            return;
        };
        let window = window.clone();
        let path_display = path.display().to_string();
        library::add_folder_to_library(&path, move |result| {
            // The scan result counts every diff kind: a re-link
            // (`moved` — the same-name+size recovery) is a success,
            // not "no books found".
            let mut parts = Vec::new();
            if !result.added.is_empty() {
                parts.push(format!("{} added", result.added.len()));
            }
            if !result.updated.is_empty() {
                parts.push(format!("{} updated", result.updated.len()));
            }
            if !result.moved.is_empty() {
                parts.push(format!("{} re-linked", result.moved.len()));
            }
            if !result.removed.is_empty() {
                parts.push(format!("{} removed", result.removed.len()));
            }
            let message = if parts.is_empty() {
                format!("No books found in {path_display}")
            } else {
                parts.join(", ")
            };
            show_info_dialog(&window, &message);
        });
    });
    chooser.show();
}

/// The scan result (an info box; the C# browser refreshes its list
/// instead).
fn show_info_dialog(parent: &impl IsA<Window>, message: &str) {
    let dialog = gtk4::MessageDialog::builder()
        .transient_for(parent)
        .modal(true)
        .title("comicrust")
        .text(message)
        .message_type(gtk4::MessageType::Info)
        .buttons(gtk4::ButtonsType::Close)
        .build();
    dialog.connect_response(|dialog, _| dialog.destroy());
    dialog.present();
}

/// The C# shows `DatabaseManager.OpenMessage` in an attention box on
/// the main form (restored-from-backup, fresh database).
fn show_attention_dialog(parent: &impl IsA<Window>, message: &str) {
    let dialog = gtk4::MessageDialog::builder()
        .transient_for(parent)
        .modal(true)
        .title("comicrust")
        .text(message)
        .message_type(gtk4::MessageType::Warning)
        .buttons(gtk4::ButtonsType::Close)
        .build();
    dialog.connect_response(|dialog, _| dialog.destroy());
    dialog.present();
}

// The session reader window (the C# single main form): every open
// — command line, `open` signal, or the launcher dialog — becomes a
// tab in this window.
thread_local! {
    static READER: RefCell<Option<reader_window::ReaderWindow>> =
        const { RefCell::new(None) };
}

/// Opens a comic path into the session reader window; failures
/// surface as a dialog (the C# shows an error box from
/// `MainForm.OpenComic`).
pub fn open_reader(app: &Application, path: &Path) {
    let result = READER.with(|cell| {
        let mut reader = cell.borrow_mut();
        match reader.as_mut() {
            Some(win) => win.open_comic(path),
            None => ReaderWindow::open(app, path).map(|win| {
                win.present();
                *reader = Some(win);
            }),
        }
    });
    if let Err(err) = result {
        show_error_dialog(app, &path.to_string_lossy(), &format!("{err:#}"));
    }
}

fn show_error_dialog(app: &Application, title: &str, message: &str) {
    let dialog = gtk4::MessageDialog::builder()
        .application(app)
        .title("comicrust")
        .text(format!("Cannot open {title}"))
        .secondary_text(message.to_string())
        .message_type(gtk4::MessageType::Error)
        .buttons(gtk4::ButtonsType::Close)
        .build();
    dialog.connect_response(|dialog, _| dialog.destroy());
    dialog.present();
}
