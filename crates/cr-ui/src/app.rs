//! The GTK application shell: a UNIQUE GApplication — a second
//! launch forwards its argv to the running instance (focus + file
//! opens, the C# `SingleInstance` handoff) — with the browser shell
//! as the main window.

use std::cell::{Cell, RefCell};
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
use crate::dialogs;
use crate::library;
use crate::theme;

pub const APP_ID: &str = "org.comicrust.ComicRust";

/// File-dialog filter extensions (`KnownFileFormats` + folders; the
/// C# open dialog uses the same list from the provider registry).
pub const OPEN_FILTER_EXTS: &[&str] = &[
    "cbz", "zip", "cbr", "rar", "cb7", "7z", "cbt", "tar", "pdf", "djvu",
];

pub fn run(args: Vec<String>) {
    // The `-waitpid` restart handshake (the C# `Program.Main`
    // waiting step): a freshly spawned binary waits for the dying
    // instance BEFORE it touches GTK or the database.
    wait_for_restart_pid(&args);

    // gtk4-rs requires explicit init before any object construction
    // (the C# calls Application.EnableVisualStyles at the same point).
    gtk4::init().expect("GTK initialization failed");
    theme::init();
    // The build marker: a user trace proves WHICH binary produced it.
    crate::trace::trace(format!("startup build {}", env!("COMICRUST_VERSION")));
    let app = Application::builder()
        .application_id(APP_ID)
        // UNIQUE (the default — NON_UNIQUE is gone): a second launch
        // registers as a remote instance, forwards its argv to the
        // primary's `command-line` handler and exits (the C#
        // `SingleInstance` named-pipe handoff). HANDLES_COMMAND_LINE:
        // the primary's own argv AND every handoff arrive through
        // `command-line` (the C# `StartNew`/`StartLast` pair).
        // HANDLES_OPEN stays for explicit `g_application_open`
        // senders (the file-manager D-Bus route).
        .flags(gio::ApplicationFlags::HANDLES_OPEN | gio::ApplicationFlags::HANDLES_COMMAND_LINE)
        .build();

    // Register before any startup work: a remote (second) instance
    // must NOT open the database — `run` forwards its argv and the
    // process exits (the C# second launch never reaches StartNew).
    if let Err(err) = app.register(None::<&gio::Cancellable>) {
        eprintln!("application registration failed: {err}");
        return;
    }

    if !app.is_remote() {
        // The library session (`Program.DatabaseManager.Open` at
        // startup) — the primary only.
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
    }

    // The command-line entry point IS the boot (the probe evidence:
    // with HANDLES_COMMAND_LINE the gio primary emits `command-line`
    // for its own argv and NEVER `activate`).
    app.connect_command_line(|app, command_line| {
        let argv: Vec<String> = command_line
            .arguments()
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        handle_command_line(app, &argv);
        glib::ExitCode::SUCCESS
    });
    app.connect_open(|app, files, _| {
        let shell = ensure_shell(app);
        for file in files {
            if let Some(path) = file.path() {
                open_supported_file(&shell, &path, false, 0, true);
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
    /// The command-line pipeline already ran (the primary's own boot
    /// is the FIRST call; every later call is a handoff).
    static COMMAND_LINE_SEEN: Cell<bool> = const { Cell::new(false) };
}

fn set_open_message(message: Option<String>) {
    OPEN_MESSAGE.with(|cell| *cell.borrow_mut() = message);
}

/// The `-waitpid <pid>` wait (the C# `Program.cs:1127-1136`,
/// `WaitForExit(30000)`): poll `/proc/<pid>` every 100 ms up to 30 s
/// (the restarted child is not ours to `waitpid`). Linux-only is
/// fine — the port is Linux-native.
fn wait_for_restart_pid(args: &[String]) {
    let Some(pos) = args.iter().position(|a| a.eq_ignore_ascii_case("-waitpid")) else {
        return;
    };
    let Some(pid) = args.get(pos + 1).and_then(|v| v.parse::<u32>().ok()) else {
        return;
    };
    let proc_dir = std::path::PathBuf::from(format!("/proc/{pid}"));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while proc_dir.exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

/// The `command-line` entry point: the primary's own argv (FIRST
/// launch — the gio `StartNew` parse; the boot itself, since
/// `activate` never fires under HANDLES_COMMAND_LINE) and every
/// second-instance handoff (`StartLast`).
fn handle_command_line(app: &Application, argv: &[String]) {
    // The probe evidence: BOTH deliveries carry the program path as
    // element 0 — the parse must never see it (a non-switch argument
    // would land in `files` and the binary would open as a comic).
    let argv = if argv.is_empty() { argv } else { &argv[1..] };
    // The `StartLast` re-parse: a FRESH ExtendedSettings from the
    // argv alone (`Files`/`Page`/`ImportList` are command-line-only).
    let ext = cr_core::settings::ExtendedSettings::from_argv(argv);
    let first = !COMMAND_LINE_SEEN.with(|c| c.get());
    COMMAND_LINE_SEEN.with(|c| c.set(true));

    let shell = ensure_shell(app);
    if first {
        // The boot (the C# `MainForm.Load`): the open message, then
        // the file pipeline (MainForm.cs:1041-1061): EXISTING
        // command-line files (`newSlot: false`, page 0, `fromShell:
        // true`), the `OpenLastFile` session reopen when nothing
        // opened, then the `-il` import (the C# order).
        shell.present();
        if let Some(message) = OPEN_MESSAGE.with(|cell| cell.take()) {
            show_attention_dialog(&shell.window(), &message);
        }
        // The Windows-path migration prompt (Phase 8 T11), BEFORE the
        // file pipeline — a re-homed book opens cleanly.
        maybe_prompt_windows_path_migration(&shell);
        for file in &ext.files {
            let path = Path::new(file);
            if path.is_file() {
                open_supported_file(&shell, path, false, 0, true);
            }
        }
        if shell.reader_open_book_count() == 0 && library::settings().borrow().open_last_file {
            for file in library::settings().borrow().last_open_files.clone() {
                let path = Path::new(&file);
                if path.is_file() {
                    // The C# `AppendNewSlots | NoIncreaseOpenedCount`
                    // reopen (each session book restores as its own
                    // tab); the opened-count bump is not suppressed
                    // (a recorded deviation — a stats field).
                    open_supported_file(&shell, path, false, 0, false);
                }
            }
        }
        // `MainForm.cs:1058-1061` — the `-il` import lands in the
        // Temporary Lists folder (no book opens).
        import_import_list(&shell, &ext.import_list);
    } else {
        // `StartLast` (Program.cs:1063-1093): `RestoreToFront`, the
        // `-il` import first, then the received files with
        // `newSlot: true` and the `-p` page passthrough (1-based
        // here, `page - 1` in the open).
        shell.present();
        import_import_list(&shell, &ext.import_list);
        for file in &ext.files {
            open_supported_file(&shell, Path::new(file), true, ext.page, true);
        }
    }
}

/// The Windows-path migration prompt (Phase 8 T11): any Windows-style
/// path in the three families asks on every boot while one remains
/// (the user decision). The boot branch and the probe share it.
pub fn maybe_prompt_windows_path_migration(shell: &browser::shell::BrowserShell) {
    if !library::has_windows_paths() {
        return;
    }
    let window = shell.window();
    let sh = shell.clone();
    dialogs::path_migration::run(&window, move |report| {
        if report.is_some_and(|r| r.changed_anything()) {
            sh.refresh_after_data_change();
        }
    });
}

/// The `-il` switch (`ImportComicList`): imports the list into the
/// Temporary Lists folder; nothing opens.
fn import_import_list(shell: &browser::shell::BrowserShell, import_list: &Option<String>) {
    let Some(file) = import_list else {
        return;
    };
    let nav = shell.navigator();
    let window = shell.window();
    let path = Path::new(file);
    dialogs::import_list::import_list_file(&window, path, None, &nav, |_| {});
}

/// The `MainForm.OpenSupportedFile` port: the extension gate, the
/// new-slot/page knobs, the `.cbl` import branch and the
/// `HideBrowserIfShellOpen` rule (the reader workspace covers the
/// browser — the open already switches to the reader).
fn open_supported_file(
    shell: &browser::shell::BrowserShell,
    path: &Path,
    new_slot: bool,
    page: i32,
    from_shell: bool,
) {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        // ADR-027: no plugins — the C# opens Preferences on a
        // `.crplugin`.
        "crplugin" => return,
        // The reading-list import (MainForm.cs:2300-2312): import,
        // then open the newest-read book of the list. The
        // HideBrowserIfShellOpen rule does NOT apply here (it lives
        // in the non-`.cbl` branch only).
        "cbl" => {
            let nav = shell.navigator();
            let window = shell.window();
            let shell = shell.clone();
            dialogs::import_list::import_list_file(&window, path, None, &nav, move |id| {
                let Some(id) = id else {
                    return;
                };
                let Some((_, books)) = library::evaluate_books(&id) else {
                    return;
                };
                // The newest-read linked book (`Aggregate` by
                // OpenedTime; a fileless placeholder cannot open —
                // the Phase 6 open gate).
                let mut linked = books.iter().filter(|b| !b.file_path.is_empty());
                let Some(first) = linked.next() else {
                    return;
                };
                let newest = linked.fold(first, |a, b| {
                    if a.opened_time.naive <= b.opened_time.naive {
                        b
                    } else {
                        a
                    }
                });
                let file_path = newest.file_path.clone();
                shell.open_comic_page(Path::new(&file_path), new_slot, 0);
            });
            return;
        }
        _ => {}
    }
    // `books.Open(file, newSlot, Math.Max(0, page - 1))` — the `-p`
    // switch is 1-based; 0 keeps the resume position.
    shell.open_comic_page(path, new_slot, page.max(0).saturating_sub(1));
    if from_shell && cr_core::settings::ExtendedSettings::global().hide_browser_if_shell_open {
        // The C# collapses the browser pane (`BrowserVisible =
        // false`); the docked reader already covers it.
    }
}

/// The shell exists (created on first need — files can arrive
/// through `open`/`command-line` BEFORE any window, and a mapped
/// window holds the app alive).
fn ensure_shell(app: &Application) -> browser::shell::BrowserShell {
    let shell = BROWSER.with(|cell| cell.borrow().as_ref().map(|s| s.clone()));
    match shell {
        Some(shell) => shell,
        None => create_browser(app),
    }
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
        ListCommand::Import => {
            import_list_dialog(parent, nav);
        }
    }
}

/// The `ImportLists` command (ComicListLibraryBrowser.cs:1421-1445):
/// a multi-select `.cbl` chooser; every chosen file imports into the
/// current selection's container (a folder takes it as a child, an
/// item into its parent, none = the top level).
fn import_list_dialog(parent: &ApplicationWindow, nav: &Rc<browser::navigator::Navigator>) {
    let chooser = FileChooserNative::builder()
        .title("Import Reading List")
        .action(FileChooserAction::Open)
        .transient_for(parent)
        .modal(true)
        .select_multiple(true)
        .build();
    // The C# filter: "ComicRack Reading List|*.cbl|Xml File|*.xml|
    // All Files|*.*".
    let cbl = FileFilter::new();
    cbl.set_name(Some("ComicRack Reading List"));
    cbl.add_pattern("*.cbl");
    chooser.add_filter(&cbl);
    let xml = FileFilter::new();
    xml.set_name(Some("Xml File"));
    xml.add_pattern("*.xml");
    chooser.add_filter(&xml);
    let nav2 = Rc::clone(nav);
    let parent = parent.clone();
    chooser.connect_response(move |chooser, response| {
        if response != ResponseType::Accept {
            return;
        }
        let paths: Vec<std::path::PathBuf> = chooser
            .files()
            .snapshot()
            .iter()
            .filter_map(|o| o.downcast_ref::<gio::File>().and_then(|f| f.path()))
            .collect();
        let target = nav2.current_selection().map(|(id, _)| id);
        for path in paths {
            dialogs::import_list::import_list_file(&parent, &path, target, &nav2, |_| {});
        }
    });
    chooser.show();
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
    let shell = ensure_shell(app);
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
