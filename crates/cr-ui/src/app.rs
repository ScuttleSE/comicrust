//! The GTK application shell. Phase 3 keeps this minimal: an
//! application window with an Open dialog (or a comic path from the
//! command line) that opens a reader window. D-Bus single instance
//! and the full browser shell arrive in later phases; the app runs
//! `NON_UNIQUE` until then.

use std::cell::RefCell;
use std::path::Path;

use gtk4::gio;
use gtk4::prelude::*;
use gtk4::{
    Application, ApplicationWindow, Button, FileChooserAction, FileChooserNative, FileFilter,
    ResponseType, Window,
};

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

/// The empty shell (no browser yet — Phase 4): a bare window with an
/// Open button so the app is usable.
fn show_shell(app: &Application) {
    let win = ApplicationWindow::builder()
        .application(app)
        .title("comicrust")
        .default_width(480)
        .default_height(240)
        .build();
    let button = Button::with_label("Open…");
    button.set_margin_top(24);
    button.set_margin_bottom(24);
    button.set_margin_start(24);
    button.set_margin_end(24);
    let win_clone = win.clone();
    button.connect_clicked(move |_| open_file_dialog(&win_clone));
    win.set_child(Some(&button));
    win.present();
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
