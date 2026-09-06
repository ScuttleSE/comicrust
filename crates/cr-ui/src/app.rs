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
    Application, ApplicationWindow, FileChooserAction, FileChooserNative, FileFilter, ResponseType,
    Window,
};

use cr_core::xml::scalar::CrGuid;

use crate::browser;
use crate::library;
use crate::theme;

pub const APP_ID: &str = "org.comicrust.ComicRust";

/// File-dialog filter extensions (`KnownFileFormats` + folders; the
/// C# open dialog uses the same list from the provider registry).
pub const OPEN_FILTER_EXTS: &[&str] = &[
    "cbz", "zip", "cbr", "rar", "cb7", "7z", "cbt", "tar", "pdf", "djvu",
];

pub fn run(args: Vec<String>) {
    // gtk4-rs requires explicit init before any object construction
    // (the C# calls Application.EnableVisualStyles at the same point).
    gtk4::init().expect("GTK initialization failed");
    theme::init();
    // The build marker: a user trace proves WHICH binary produced it.
    crate::trace::trace(format!("startup build {}", env!("COMICRUST_VERSION")));
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

    // The theme from the extended settings (`ThemeManager.Initialize(
    // ExtendedSettings.Theme)` parity — the C# `Theme` getter resolves
    // `UseDarkMode` → Dark, `Default` renders light).
    theme::set_dark(
        cr_core::settings::ExtendedSettings::global().effective_theme()
            == cr_core::settings::enums::Themes::Dark,
    );

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

/// The app shell — the browser main window (`MainForm`): the
/// navigator + ItemView, the quick search and view commands, the
/// status bar, and the reader docked as a view. A bare entry dialog
/// still serves the navigator commands (the editors are Phase 5).
fn show_shell(app: &Application) {
    let shell = BROWSER.with(|cell| cell.borrow().as_ref().map(|s| s.clone()));
    let shell = match shell {
        Some(shell) => shell,
        None => create_browser(app),
    };
    if let Some(message) = OPEN_MESSAGE.with(|cell| cell.borrow().clone()) {
        show_attention_dialog(&shell.window(), &message);
    }
    shell.present();
}

/// Creates and registers the browser shell. Files can arrive through
/// the `open` signal BEFORE `activate` (GApplication order); a shell
/// created there keeps the app alive (a mapped window holds it).
fn create_browser(app: &Application) -> browser::shell::BrowserShell {
    let (window, shell) = browser::shell::BrowserShell::create(app);
    {
        let nav = shell.navigator();
        let win_for_cmds = window.clone();
        let nav_for_cmd = Rc::clone(&nav);
        nav.connect_command(move |command, target| {
            run_list_command(&win_for_cmds, &nav_for_cmd, command, target);
        });
    }
    BROWSER.with(|cell| *cell.borrow_mut() = Some(shell.clone()));
    if let Some(message) = OPEN_MESSAGE.with(|cell| cell.borrow().clone()) {
        show_attention_dialog(&window, &message);
    }
    window.present();
    shell
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
            // The C# `NewSmartList`: insert an empty smart list, then
            // open the editor; Cancel removes it.
            let new_id = library::new_smart_list(target.as_ref(), "New Smart List", "");
            match new_id {
                Ok(id) => {
                    nav.refill(&library::comic_lists_snapshot());
                    run_smart_list_editor(parent, nav, Some(id));
                }
                Err(err) => show_attention_dialog(parent, &err),
            }
        }
        ListCommand::Edit => {
            // The C# `EditSmartListItem`/`EditListDialog.Edit`
            // routing: smart lists → the smart-list editor, folders
            // and reading lists → the list editor.
            let Some(id) = target else {
                return;
            };
            let lib = library::session();
            let l = lib.borrow();
            let item = cr_engine::lists::find_list_item(&l.database().comic_lists, &id);
            drop(l);
            match item {
                Some(cr_core::database::list_items::ComicListItem::Smart(_)) => {
                    run_smart_list_editor(parent, nav, Some(id));
                }
                Some(cr_core::database::list_items::ComicListItem::Folder(f)) => {
                    run_list_editor(
                        parent,
                        nav,
                        crate::dialogs::list_editor::ListKind::Folder,
                        &id,
                        f.base.name.as_deref().unwrap_or(""),
                        f.base.description.as_str(),
                        f.base.quick_open,
                        f.combine_mode,
                    );
                }
                Some(cr_core::database::list_items::ComicListItem::IdList(list)) => {
                    run_list_editor(
                        parent,
                        nav,
                        crate::dialogs::list_editor::ListKind::ReadingList,
                        &id,
                        list.base.name.as_deref().unwrap_or(""),
                        list.base.description.as_str(),
                        list.base.quick_open,
                        cr_core::model::enums::ComicFolderCombineMode::Or,
                    );
                }
                _ => {}
            }
        }
        ListCommand::NewList => {
            // The C# `NewList`: the dialog first, then the insert.
            let id = library::new_id_list(target.as_ref(), "New List");
            nav.refill(&library::comic_lists_snapshot());
            let Some(cr_core::database::list_items::ComicListItem::IdList(item)) =
                library::find_list_item_any(&id)
            else {
                return;
            };
            run_list_editor(
                parent,
                nav,
                crate::dialogs::list_editor::ListKind::ReadingList,
                &id,
                item.base.name.as_deref().unwrap_or("New List"),
                item.base.description.as_str(),
                item.base.quick_open,
                cr_core::model::enums::ComicFolderCombineMode::Or,
            );
        }
        ListCommand::NewFolder => {
            // The C# `NewFolder`: the dialog first, then the insert.
            let id = library::new_folder(target.as_ref(), "New Folder");
            nav.refill(&library::comic_lists_snapshot());
            let Some(cr_core::database::list_items::ComicListItem::Folder(folder)) =
                library::find_list_item_any(&id)
            else {
                return;
            };
            run_list_editor(
                parent,
                nav,
                crate::dialogs::list_editor::ListKind::Folder,
                &id,
                folder.base.name.as_deref().unwrap_or("New Folder"),
                folder.base.description.as_str(),
                folder.base.quick_open,
                folder.combine_mode,
            );
        }
        ListCommand::Rename => {
            // Route through the editors (the C# has no separate
            // rename: Edit carries the name).
            let Some(id) = target else {
                return;
            };
            let _ = id;
            run_list_command(parent, nav, browser::navigator::ListCommand::Edit, target);
        }
        ListCommand::Delete => {
            if let Some(id) = target {
                library::remove_list(&id);
                nav.refill(&library::comic_lists_snapshot());
            }
        }
    }
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
pub fn add_folder_dialog(parent: &impl IsA<Window>) {
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

/// Opens a comic path into the session reader window; failures
/// surface as a dialog (the C# shows an error box from
/// `MainForm.OpenComic`).
pub fn open_reader(app: &Application, path: &Path) {
    let shell = BROWSER.with(|cell| cell.borrow().as_ref().map(|s| s.clone()));
    let shell = match shell {
        Some(shell) => shell,
        // The first file can arrive through the `open` signal BEFORE
        // `activate` — the shell must exist (and its window map) now,
        // or the app exits with no mapped window.
        None => create_browser(app),
    };
    shell.open_comic(path);
}

thread_local! {
    static BROWSER: RefCell<Option<browser::shell::BrowserShell>> = const { RefCell::new(None) };
}

/// The smart-list editor flow (the C# `EditSmartListItem`): the
/// editor edits a CLONE; OK commits via `update_smart_list`, Cancel
/// keeps the item as it was (a freshly created one is removed — the
/// C# `NewSmartList` pops the insert when the editor returns false).
fn run_smart_list_editor(
    parent: &ApplicationWindow,
    nav: &Rc<browser::navigator::Navigator>,
    id: Option<CrGuid>,
) {
    let Some(id) = id else {
        return;
    };
    let Some(item) = library::find_smart_list(&id) else {
        return;
    };
    let base_options = library::smart_list_base_options(&id);
    let nav2 = Rc::clone(nav);
    let window2 = parent.clone();
    crate::dialogs::smart_list::show_smart_list_editor(
        parent,
        item,
        base_options,
        move |committed| match committed {
            Some(updated) => {
                if std::env::var("CR_DEBUG_SL").is_ok() {
                    eprintln!(
                        "editor commit: name={:?} matchers={} id={id}",
                        updated.base.name,
                        updated.matchers.len()
                    );
                }
                library::update_smart_list(&id, updated);
                nav2.refill(&library::comic_lists_snapshot());
            }
            None => {
                // A named "New Smart List" with no matchers that the
                // user never committed: the C# pops the fresh insert.
                // Only remove when it is STILL empty (an edit keeps).
                if let Some(item) = library::find_smart_list(&id) {
                    if item.matchers.is_empty()
                        && item.base.name.as_deref() == Some("New Smart List")
                    {
                        library::remove_list(&id);
                    }
                }
                nav2.refill(&library::comic_lists_snapshot());
            } // The window reference keeps the transient parent alive
              // for the dialog lifetime (unused otherwise).
              ,
        },
    );
    let _ = window2;
}

/// The list editor flow for folders and reading lists (the C#
/// `EditListDialog.Edit`): OK applies the fields, Cancel discards
/// (a fresh uncommitted insert with the default name is removed).
#[allow(clippy::too_many_arguments)]
fn run_list_editor(
    parent: &ApplicationWindow,
    nav: &Rc<browser::navigator::Navigator>,
    kind: crate::dialogs::list_editor::ListKind,
    id: &CrGuid,
    name: &str,
    description: &str,
    quick_open: bool,
    combine_mode: cr_core::model::enums::ComicFolderCombineMode,
) {
    let nav2 = Rc::clone(nav);
    let id = *id;
    let name = name.to_string();
    let description = description.to_string();
    // A fresh reading list the user cancels: pop it (computed before
    // the move into the closure).
    let fresh_insert =
        kind == crate::dialogs::list_editor::ListKind::ReadingList && name == "New List";
    crate::dialogs::list_editor::show_list_editor(
        parent,
        kind,
        &name,
        &description,
        quick_open,
        combine_mode,
        move |result| {
            if let Some(result) = result {
                library::update_list_fields(
                    &id,
                    &library::ListEditFields {
                        name: result.name,
                        description: result.description,
                        quick_open: result.quick_open,
                        combine_mode: result.combine_mode,
                    },
                );
            } else if fresh_insert {
                library::remove_list(&id);
            }
            nav2.refill(&library::comic_lists_snapshot());
        },
    );
}
