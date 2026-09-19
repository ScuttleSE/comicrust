//! The browser shell — the app's main window (the C# `MainForm`):
//! the navigator pane + ItemView in a paned container with a status
//! bar, the quick search, the view-mode/sort/group/size/columns
//! commands, and the reader docked as a view (the C# reader replaces
//! the browser view; `D` undocks it into its own window).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{gio, Application, ApplicationWindow, Button, Entry, Label, Paned, Stack};

use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::CrGuid;
use cr_engine::image_pool::ImagePool;
use cr_engine::matcher::tree::Matcher;
use cr_scrape::cache::CvCache;

use crate::library;
use crate::reader::display::ImageFitMode;
use crate::reader::page_view::PageLayoutMode;

/// The user settings (`Program.Settings`).
fn cr_ui_settings() -> std::rc::Rc<std::cell::RefCell<cr_core::settings::Settings>> {
    library::settings()
}
use crate::reader_shell::ReaderShell;

struct IncomingOrganizerEffects {
    engine: cr_engine::incoming_transaction::TransactionEngine,
    transaction: std::sync::Mutex<cr_engine::incoming_transaction::IncomingTransaction>,
    working: std::sync::Mutex<(
        cr_core::database::comic_database::ComicDatabase,
        cr_engine::incoming::IncomingCatalog,
    )>,
    previous: std::sync::Mutex<
        Option<(
            cr_core::database::comic_database::ComicDatabase,
            cr_engine::incoming::IncomingCatalog,
        )>,
    >,
    adoption_sources: Option<std::collections::HashSet<String>>,
    captured_epoch: u64,
}

impl IncomingOrganizerEffects {
    fn new(
        transaction: cr_engine::incoming_transaction::IncomingTransaction,
        database: cr_core::database::comic_database::ComicDatabase,
        incoming: cr_engine::incoming::IncomingCatalog,
        adoption_sources: Option<std::collections::HashSet<String>>,
        captured_epoch: u64,
    ) -> Self {
        Self {
            engine: cr_engine::incoming_transaction::TransactionEngine::new(
                &cr_core::paths::Paths::new_default(),
            ),
            transaction: std::sync::Mutex::new(transaction),
            working: std::sync::Mutex::new((database, incoming)),
            previous: std::sync::Mutex::new(None),
            adoption_sources,
            captured_epoch,
        }
    }

    fn update_after_images(&self) -> std::io::Result<()> {
        let working = self.working.lock().unwrap();
        let mut transaction = self.transaction.lock().unwrap();
        transaction.files.comic_database.as_mut().unwrap().after =
            cr_core::database::comic_database::save_bytes(&working.0)
                .map_err(std::io::Error::other)?;
        transaction.files.incoming_catalog.as_mut().unwrap().after = working.1.to_bytes()?;
        Ok(())
    }

    fn prepare_action(
        &self,
        action: cr_engine::incoming_transaction::ExternalFileAction,
    ) -> std::io::Result<()> {
        if cr_engine::incoming_transaction::database_epoch() != self.captured_epoch {
            return Err(std::io::Error::other(
                "The library changed during the Incoming transaction.",
            ));
        }
        let mut transaction = self.transaction.lock().unwrap();
        transaction.external_actions.push(action);
        if self.engine.journal_path().exists() {
            self.engine.update(&transaction)
        } else {
            self.engine.begin(&transaction)
        }
        .map_err(std::io::Error::other)
    }

    fn mark_last(&self, succeeded: bool) -> std::io::Result<()> {
        if !succeeded {
            if let Some(previous) = self.previous.lock().unwrap().take() {
                *self.working.lock().unwrap() = previous;
                self.update_after_images()?;
            }
        } else {
            self.previous.lock().unwrap().take();
        }
        let mut transaction = self.transaction.lock().unwrap();
        if succeeded {
            match transaction.external_actions.last_mut() {
                Some(cr_engine::incoming_transaction::ExternalFileAction::Rename {
                    status,
                    ..
                })
                | Some(cr_engine::incoming_transaction::ExternalFileAction::Delete {
                    status,
                    ..
                }) => *status = cr_engine::incoming_transaction::ExternalActionStatus::Applied,
                None => {}
            }
        } else {
            transaction.external_actions.pop();
        }
        self.engine
            .update(&transaction)
            .map_err(std::io::Error::other)
    }

    fn finish(
        &self,
        guard: &cr_engine::incoming_transaction::MutationGuard,
        database: &cr_core::database::comic_database::ComicDatabase,
        incoming: &cr_engine::incoming::IncomingCatalog,
        auxiliary: Vec<cr_engine::incoming_transaction::FileSnapshot>,
        config: Option<cr_engine::incoming_transaction::FileSnapshot>,
    ) -> Result<u64, String> {
        let mut transaction = self.transaction.lock().unwrap();
        transaction.files.comic_database.as_mut().unwrap().after =
            cr_core::database::comic_database::save_bytes(database)
                .map_err(|error| error.to_string())?;
        transaction.files.incoming_catalog.as_mut().unwrap().after =
            incoming.to_bytes().map_err(|error| error.to_string())?;
        transaction.files.auxiliary = auxiliary;
        transaction.files.config = config;
        if !self.engine.journal_path().exists() {
            self.engine
                .begin(&transaction)
                .map_err(|error| error.to_string())?;
        } else {
            self.engine
                .update(&transaction)
                .map_err(|error| error.to_string())?;
        }
        guard
            .commit_if_epoch(self.captured_epoch, &self.engine, &mut transaction)
            .inspect_err(|error| {
                if matches!(
                    error,
                    cr_engine::incoming_transaction::TransactionError::EpochChanged
                ) {
                    let _ = self.engine.abort_prepared();
                }
            })
            .map_err(|error| error.to_string())
    }
}

impl cr_organize::engine::FilesystemEffects for IncomingOrganizerEffects {
    fn before_rename(&self, source: &str, destination: &str) -> std::io::Result<()> {
        {
            let mut working = self.working.lock().unwrap();
            *self.previous.lock().unwrap() = Some(working.clone());
            let kind = self.transaction.lock().unwrap().kind;
            if kind == cr_engine::incoming_transaction::TransactionKind::Adoption {
                if let Some(index) = working
                    .1
                    .books
                    .iter()
                    .position(|book| book.file_path == source)
                {
                    let mut book = working.1.books.remove(index);
                    book.file_path = destination.into();
                    working
                        .0
                        .books
                        .retain(|value| value.id != book.id && value.file_path != destination);
                    working.0.books.push(book);
                } else if let Some(index) = working
                    .0
                    .books
                    .iter()
                    .position(|book| book.file_path == source)
                {
                    let mut book = working.0.books.remove(index);
                    book.file_path = destination.into();
                    working.1.books.retain(|value| value.id != book.id);
                    working.1.books.push(book);
                }
            } else if kind == cr_engine::incoming_transaction::TransactionKind::Undo
                && self
                    .adoption_sources
                    .as_ref()
                    .is_some_and(|paths| paths.contains(source))
            {
                if let Some(index) = working
                    .0
                    .books
                    .iter()
                    .position(|book| book.file_path == source)
                {
                    let mut book = working.0.books.remove(index);
                    book.file_path = destination.into();
                    working
                        .1
                        .books
                        .retain(|value| value.id != book.id && value.file_path != destination);
                    working.1.books.push(book);
                }
            } else if kind == cr_engine::incoming_transaction::TransactionKind::Undo {
                if let Some(book) = working
                    .0
                    .books
                    .iter_mut()
                    .find(|book| book.file_path == source)
                {
                    book.file_path = destination.into();
                }
            }
        }
        self.update_after_images()?;
        self.prepare_action(
            cr_engine::incoming_transaction::ExternalFileAction::Rename {
                source: source.into(),
                destination: destination.into(),
                status: cr_engine::incoming_transaction::ExternalActionStatus::Pending,
            },
        )
    }

    fn after_rename(
        &self,
        _source: &str,
        _destination: &str,
        succeeded: bool,
    ) -> std::io::Result<()> {
        self.mark_last(succeeded)
    }

    fn before_delete(&self, path: &str) -> std::io::Result<()> {
        {
            let mut working = self.working.lock().unwrap();
            *self.previous.lock().unwrap() = Some(working.clone());
            working.0.books.retain(|book| book.file_path != path);
            working.1.books.retain(|book| book.file_path != path);
        }
        self.update_after_images()?;
        self.prepare_action(
            cr_engine::incoming_transaction::ExternalFileAction::Delete {
                source: path.into(),
                status: cr_engine::incoming_transaction::ExternalActionStatus::Pending,
            },
        )
    }

    fn after_delete(&self, _path: &str, succeeded: bool) -> std::io::Result<()> {
        self.mark_last(succeeded)
    }
}

fn file_snapshot(
    path: std::path::PathBuf,
    after: Vec<u8>,
) -> cr_engine::incoming_transaction::FileSnapshot {
    cr_engine::incoming_transaction::FileSnapshot {
        before: std::fs::read(&path).ok(),
        path,
        after,
        remove_after: false,
    }
}

fn live_snapshot(
    path: std::path::PathBuf,
    bytes: Vec<u8>,
) -> cr_engine::incoming_transaction::FileSnapshot {
    cr_engine::incoming_transaction::FileSnapshot {
        path,
        before: Some(bytes.clone()),
        after: bytes,
        remove_after: false,
    }
}

fn apply_organizer_results(
    database: &mut cr_core::database::comic_database::ComicDatabase,
    applies: &[cr_organize::engine::Apply],
) {
    for apply in applies {
        match apply {
            cr_organize::engine::Apply::Update(book) => {
                if let Some(existing) = database.books.iter_mut().find(|value| value.id == book.id)
                {
                    *existing = book.clone();
                }
            }
            cr_organize::engine::Apply::Insert(book) | cr_organize::engine::Apply::Adopt(book) => {
                database.books.retain(|value| value.id != book.id);
                database.books.push(book.clone());
            }
            cr_organize::engine::Apply::Remove(id) => {
                database.books.retain(|book| book.id != *id);
            }
        }
    }
}

fn failed_run_outcome(error: String) -> crate::dialogs::organize::RunOutcome {
    crate::dialogs::organize::RunOutcome {
        text: error,
        failed_or_skipped: true,
        applies: Vec::new(),
        residual: None,
    }
}

fn prepare_close(sh: &ShellState) {
    let open_files = sh.reader.open_files();
    sh.reader.shutdown();
    let current = *sh.current_list.borrow();
    sh.store_view_config(current);
    let size = sh.quick_view.thumb_height() as i32;
    let previous = cr_ui_settings().borrow().current_workspace.clone();
    let workspace = sh.collect_workspace(previous.as_ref());
    cr_ui_settings().borrow_mut().current_workspace = Some(workspace);
    let settings = cr_ui_settings();
    let mut settings = settings.borrow_mut();
    settings.quick_open_thumbnail_size = size;
    settings.last_open_files = open_files;
    if let Some(folder) = sh.folders_tree.current_folder() {
        settings.last_explorer_folder = folder;
    }
}

fn save_and_finish_close(
    window: gtk4::ApplicationWindow,
    barrier: Rc<RefCell<cr_engine::incoming_transaction::CloseBarrier>>,
) {
    library::save_for_close_async(move |result| match result {
        Ok(true) => {
            library::save_settings();
            if barrier.borrow_mut().coordinator_became_idle() {
                window.close();
            }
        }
        Ok(false) => save_and_finish_close(window, barrier),
        Err(error) => {
            eprintln!("library save failed: {error}");
            barrier.borrow_mut().save_failed();
            show_failure_dialog(
                &window,
                "ComicRust could not close",
                &format!("The library could not be saved. The window remains open.\n\n{error}"),
            );
        }
    });
}

fn flush_lists_and_finish_close(
    window: gtk4::ApplicationWindow,
    barrier: Rc<RefCell<cr_engine::incoming_transaction::CloseBarrier>>,
) {
    library::flush_incoming_lists_for_close(move |result| match result {
        Ok(()) => save_and_finish_close(window, barrier),
        Err(error) => {
            eprintln!("Incoming smart-list save failed: {error}");
            barrier.borrow_mut().save_failed();
            show_failure_dialog(
                &window,
                "ComicRust could not close",
                &format!(
                    "The Incoming smart lists could not be saved. The window remains open.\n\n{error}"
                ),
            );
        }
    });
}

use super::columns::{self, default_columns};
use super::item_view::ItemView;
use super::layout::ItemViewMode;
use super::navigator::Navigator;
use super::pages_view::PagesPanel;
use super::status_bar;
use super::tabstrip::TabId;

/// The browser workspace tabs (the C# `tsbLibrary`/`tsbPages`).
#[derive(Clone, Copy, PartialEq)]
enum Workspace {
    Library,
    /// The Files (Folders) view (`tsbFolders`).
    Folders,
    Pages,
}

/// The search debounce (`UpdateSearch` on text change; large sets
/// re-filter).
const SEARCH_DEBOUNCE_MS: u64 = 300;

/// The thumbnail size range (`Program` limits: 96..512).
const MIN_THUMB: f64 = 96.0;
const MAX_THUMB: f64 = 512.0;
const THUMB_STEP: f64 = 16.0;

/// The clone-able handle (the app's session slot; the closures keep
/// the window alive through the state's widgets).
#[derive(Clone)]
pub struct BrowserShell {
    window: ApplicationWindow,
    state: Rc<ShellState>,
}

struct ShellState {
    window: ApplicationWindow,
    stack: Stack,
    /// The multi-panel status bar (the T8 `statusStrip`).
    status_bar: super::status_bar::StatusBar,
    navigator: Rc<Navigator>,
    item_view: ItemView,
    /// The Files (Folders) view: the filesystem tree + its own grid
    /// (the C# `ComicListFolderFilesBrowser` — a full browser view).
    folders_tree: Rc<super::folder_tree::FolderTree>,
    folders_view: ItemView,
    /// The current folder's display name (the status-bar selection
    /// panel while the Files view shows).
    current_folder_name: RefCell<String>,
    /// The folder-scan generation (stale async results drop — the
    /// reader's stale-payload lesson).
    folder_scan_gen: Cell<u64>,
    quick_view: ItemView,
    pages: PagesPanel,
    /// The full-window Pages workspace page (the probe measures its
    /// allocation — the full-window layout gate).
    pages_page: gtk4::Box,
    /// The navigator pane host — the Sidebar toggle target (the C#
    /// `tbSidebar` collapses the left pane).
    nav_box: gtk4::Box,
    /// The workspace tab strip (`MainView.tabStrip`): Library |
    /// Pages | the comic tabs | `+`, with the reader toolbar docked
    /// at the right end.
    tab_strip: super::tabstrip::TabStrip,
    /// The last browser workspace (the C# `lastBrowser` —
    /// `ShowLast`/ToggleBrowser return to it).
    last_browser: Cell<u8>,
    /// The quick-search entry (the FocusQuickSearch command target).
    search: Entry,
    reader: ReaderShell,
    app: Application,
    /// The current navigator selection (refreshes after mutations).
    current_list: RefCell<Option<CrGuid>>,
    /// The current list's name (the status-bar selection panel; set
    /// on the navigator selection + refreshes).
    current_list_name: RefCell<String>,
    /// The current list's view settings changed since it became
    /// current (ADR-039). Set by every handler that alters the view;
    /// a leave with the flag set writes the live config into the
    /// outgoing list's `<Display><View>` (the C#
    /// `ComicBrowserControl.UpdateViewConfig`). While it is CLEAR the
    /// list keeps whatever it had, so a list with no settings of its
    /// own stays inheriting.
    view_config_dirty: Cell<bool>,
    incoming_list_eval_gen: Rc<Cell<u64>>,
    /// A scroll offset to restore after an in-place refresh rebuilds
    /// the list. A refresh (unlike a list switch) keeps the view
    /// where it was. `set_books` reconfigures the vadjustment and
    /// collapses the value to 0; this holds the pre-refresh offset
    /// across the possible empty-then-populated async refresh pair
    /// and restores it when the populated set lands.
    pending_scroll_restore: Cell<Option<f64>>,
    /// The `win.` action group members by name (the enable-state
    /// sync reaches them here). A RefCell: the map fills while the
    /// state itself already lives in its Rc.
    actions: RefCell<HashMap<&'static str, gio::SimpleAction>>,
    /// The list browsing history (the C# `ILibraryBrowser` back /
    /// forward chain; Previous/Next List + the T6 toolbar buttons).
    list_history: RefCell<Vec<CrGuid>>,
    list_history_pos: Cell<usize>,
    /// The random-book walk state (`OpenNextComic` random mode: no
    /// repeats until the list changed or the cycle wrapped).
    random_list: RefCell<Vec<CrGuid>>,
    random_picked: RefCell<Vec<CrGuid>>,
    /// The main-window menubar (Phase 5.5 T3; the T14
    /// layout persistence and the probes reach it here).
    menubar: super::menubar::MenubarWidget,
    /// The reader toolbar (the T5 `mainToolStrip`).
    toolbar: super::toolbar::ReaderToolbar,
    /// The reader page box (the toolbar's docked parent — the
    /// undock moves the toolbar in and out of it).
    reader_page_box: gtk4::Box,
    /// The browser toolbar (the T6 `ComicBrowserControl.toolStrip`).
    browser_toolbar: super::browser_toolbar::BrowserToolbar,
    /// The Missing Issues scope/Refresh bar (Phase 19): visible only
    /// while the Missing Issues navigator node is selected.
    missing_issues_bar: super::missing_issues_bar::MissingIssuesBar,
    /// The live quick-search text (the composed filter reads it).
    search_text: RefCell<String>,
    /// The composed filter (quick search + the view filters) — the
    /// Duplicate List source (`GetCurrentMatcher`).
    current_filter: RefCell<Option<Matcher>>,
    /// The Detail header column chooser (the C#
    /// `autoHeaderContextMenuStrip`): a FRESH PopoverMenu per open;
    /// the last one is kept for the probe.
    columns_drop: RefCell<Option<gtk4::Popover>>,
    /// The per-column check actions of the chooser ("cols.col<id>",
    /// state = visible — the model items' checkmarks). States refresh
    /// on every open.
    column_actions: RefCell<HashMap<i32, gio::SimpleAction>>,
    /// The book context menu's last popover (the probe's arrow
    /// gate) — the same fresh-per-open shape.
    context_drop: RefCell<Option<gtk4::Popover>>,
    /// The app image pool (the C# `Program.ImagePool`): the Tasks
    /// dialog queue snapshot and the Quick Rating cover load.
    pool: Arc<ImagePool>,
    /// The single Tasks dialog instance (`ShowPendingTasks`
    /// re-presents it).
    tasks_window: RefCell<Option<gtk4::Window>>,
    /// The navigator/item split (the `BrowserSplit` persistence
    /// reads the position, the restore sets it).
    paned: Paned,
}

impl ShellState {
    fn is_incoming_view(&self) -> bool {
        self.current_list
            .borrow()
            .as_ref()
            .is_some_and(super::navigator::is_incoming_scope_id)
    }

    fn selected_incoming_books(&self) -> Vec<ComicBook> {
        if !self.is_incoming_view() {
            return Vec::new();
        }
        library::incoming_books_by_ids(&self.item_view.selection_ids())
    }

    fn compare_incoming(self: &Rc<Self>) {
        let selected = self.selected_incoming_books();
        if selected.is_empty() {
            return;
        }
        let incoming_books = library::incoming_books_snapshot();
        let library_books = library::session().borrow().database().books.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("Compare Incoming".into())
            .spawn(move || {
                let comparisons = crate::dialogs::incoming_compare::build_comparisons(
                    &selected,
                    &incoming_books,
                    &library_books,
                );
                let _ = tx.send(comparisons);
            })
            .expect("spawn Incoming compare worker");
        let window = self.window.clone();
        let pool = Arc::clone(&self.pool);
        let weak_shell = Rc::downgrade(self);
        glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            match rx.try_recv() {
                Ok(comparisons) => {
                    let rules = cr_engine::duplicates::DuplicateRules::from_settings(
                        &library::settings().borrow(),
                    );
                    let weak = weak_shell.clone();
                    let executor: crate::dialogs::incoming_compare::ActionExecutor = Rc::new(
                        move |action, ids, complete| {
                            crate::trace::trace(format!(
                                "compare executor received action={action:?} selected_id={} matched_id={}",
                                ids.selected.to_d_string(),
                                ids.matched.to_d_string()
                            ));
                            let Some(sh) = weak.upgrade() else {
                                complete(Err(
                                    "The browser closed before the action started.".into()
                                ));
                                return;
                            };
                            match action {
                                crate::dialogs::incoming_compare::CompareAction::ReplaceLibraryCopy => {
                                    crate::trace::trace("compare executor dispatching replacement");
                                    let weak = Rc::downgrade(&sh);
                                    library::replace_library_copy_async(ids.selected, ids.matched, move |result| {
                                        let Some(sh) = weak.upgrade() else {
                                            complete(Err("The browser closed before the action finished.".into()));
                                            return;
                                        };
                                        let result = result.and_then(|(database, catalog, committed_epoch, controlled_paths)| {
                                            if cr_engine::incoming_transaction::database_epoch() != committed_epoch {
                                                return Err("The live library changed after the transaction committed. Restart ComicRust to load the saved catalogs.".into());
                                            }
                                            let session = library::session();
                                            let mut session = session.borrow_mut();
                                            session.suppress_watch_paths(controlled_paths);
                                            session.install_persisted_database(database);
                                            drop(session);
                                            library::replace_incoming_catalog(catalog);
                                            sh.refresh_view_from_list();
                                            sh.sync_enabled();
                                            Ok(())
                                        });
                                        complete(result);
                                    });
                                }
                                crate::dialogs::incoming_compare::CompareAction::DeleteSelectedIncomingCopy
                                | crate::dialogs::incoming_compare::CompareAction::DeleteMatchingIncomingCopy => {
                                    let id = if action == crate::dialogs::incoming_compare::CompareAction::DeleteSelectedIncomingCopy { ids.selected } else { ids.matched };
                                    let Some(book) = library::incoming_books_by_ids(&[id]).into_iter().next() else {
                                        complete(Err("The Incoming copy is no longer available.".into()));
                                        return;
                                    };
                                    crate::trace::trace(format!(
                                        "compare executor dispatching discard action={action:?} id={} path='{}'",
                                        id.to_d_string(), book.file_path
                                    ));
                                    let weak = Rc::downgrade(&sh);
                                    library::discard_incoming_async(vec![(id, book.file_path)], false, move |result| {
                                        let Some(sh) = weak.upgrade() else {
                                            complete(Err("The browser closed before the action finished.".into()));
                                            return;
                                        };
                                        let result = result.and_then(|(catalog, outcome, committed_epoch)| {
                                            if outcome.failed != 0 || outcome.removed != 1 {
                                                return Err("The Incoming file could not be moved to trash.".into());
                                            }
                                            if cr_engine::incoming_transaction::database_epoch() != committed_epoch {
                                                return Err("The live library changed after the transaction committed. Restart ComicRust to load the saved catalog.".into());
                                            }
                                            library::session().borrow_mut().suppress_watch_paths(outcome.deleted_paths.clone());
                                            library::replace_incoming_catalog(catalog);
                                            sh.refresh_view_from_list();
                                            sh.sync_enabled();
                                            Ok(())
                                        });
                                        complete(result);
                                    });
                                }
                            }
                        },
                    );
                    crate::dialogs::incoming_compare::show(
                        &window,
                        Arc::clone(&pool),
                        comparisons,
                        rules,
                        executor,
                    );
                    glib::ControlFlow::Break
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    show_failure_dialog(
                        &window,
                        "Compare Incoming",
                        "The compare worker stopped without a result.",
                    );
                    glib::ControlFlow::Break
                }
            }
        });
    }

    fn refresh_missing_issues(self: &Rc<Self>) {
        // The volume-id vote always reads the WHOLE library, never just
        // the chosen scope. See `refresh_missing_issues_async`.
        let library_books = library::session().borrow().database().books.clone();
        let scope_books = match self.missing_issues_bar.chosen_scope() {
            Some(id) => library::evaluate_books(&id)
                .map(|(_, books)| books)
                .unwrap_or_default(),
            None => library_books.clone(),
        };
        library::refresh_missing_issues_async(scope_books, library_books);
        self.missing_issues_bar
            .set_status(&missing_issues_status_text());
        let state = Rc::downgrade(self);
        glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
            let Some(sh) = state.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if library::missing_issues_refresh_active() {
                return glib::ControlFlow::Continue;
            }
            if sh
                .current_list
                .borrow()
                .is_some_and(|id| super::navigator::is_missing_issues_id(&id))
            {
                sh.item_view.set_books(library::missing_issues_snapshot());
            }
            sh.missing_issues_bar
                .set_status(&missing_issues_status_text());
            glib::ControlFlow::Break
        });
    }

    fn find_in_incoming(self: &Rc<Self>) {
        let selected_ids: std::collections::HashSet<CrGuid> =
            self.item_view.selection_ids().into_iter().collect();
        let missing: Vec<ComicBook> = self
            .item_view
            .displayed_books()
            .into_iter()
            .filter(|book| selected_ids.contains(&book.id))
            .collect();
        if missing.is_empty() {
            return;
        }
        let incoming = library::incoming_books_snapshot();
        let matches = cr_engine::incoming::find_missing_incoming_matches(&missing, &incoming);
        let rows = matches
            .into_iter()
            .map(|matched| crate::dialogs::find_incoming::FindIncomingRow {
                missing: missing[matched.missing_index].clone(),
                candidates: matched
                    .incoming_indexes
                    .into_iter()
                    .filter_map(|index| incoming.get(index).cloned())
                    .collect(),
            })
            .collect();
        let weak = Rc::downgrade(self);
        crate::dialogs::find_incoming::show(&self.window, rows, move |selected| {
            let Some(selected) = selected else {
                return;
            };
            if let Some(sh) = weak.upgrade() {
                sh.run_configured_gap_adoption(selected, false, true);
            }
        });
    }

    fn configured_gap_profile(&self) -> Option<cr_organize::profile::Profile> {
        let configured = library::incoming_config().find_in_incoming_profile;
        library::organize_settings()
            .profiles
            .into_iter()
            .find(|profile| {
                profile.mode == cr_organize::profile::MODE_MOVE && profile.name == configured
            })
    }

    /// Runs the shared configured adoption path for Missing Issues and
    /// Incoming > Gap Fills. `confirmed` is true when the match dialog
    /// already supplied the move confirmation.
    fn run_configured_gap_adoption(
        self: &Rc<Self>,
        incoming: Vec<ComicBook>,
        preview: bool,
        confirmed: bool,
    ) {
        if incoming.is_empty() {
            return;
        }
        let Some(profile) = self.configured_gap_profile() else {
            show_report_dialog(
                &self.window,
                "Gap Fill Adoption",
                "Select a Gap Fill adoption Move profile in Preferences > Libraries.",
            );
            return;
        };
        if preview || confirmed {
            self.run_incoming_adoption(incoming, profile, preview, !preview);
            return;
        }

        let dialog = gtk4::MessageDialog::builder()
            .title("Gap Fill Adoption")
            .transient_for(&self.window)
            .modal(true)
            .message_type(gtk4::MessageType::Question)
            .text(format!(
                "Adopt {} selected Gap Fill book(s)?",
                incoming.len()
            ))
            .secondary_text(format!("Library Organizer profile: {}", profile.name))
            .build();
        dialog.add_button("Cancel", gtk4::ResponseType::Cancel);
        dialog.add_button("Adopt", gtk4::ResponseType::Ok);
        let pending = Rc::new(RefCell::new(Some((incoming, profile))));
        let weak = Rc::downgrade(self);
        dialog.connect_response(move |dialog, response| {
            dialog.close();
            if response != gtk4::ResponseType::Ok {
                return;
            }
            let Some((incoming, profile)) = pending.borrow_mut().take() else {
                return;
            };
            if let Some(sh) = weak.upgrade() {
                sh.run_incoming_adoption(incoming, profile, false, true);
            }
        });
        dialog.present();
    }

    fn run_incoming_menu_adoption(self: &Rc<Self>, preview: bool) {
        let selected = self.selected_incoming_books();
        if selected.is_empty() {
            return;
        }
        let gap_fills = self
            .current_list
            .borrow()
            .is_some_and(|id| id == super::navigator::IncomingView::GapFills.id());
        if gap_fills {
            self.run_configured_gap_adoption(selected, preview, false);
        } else {
            self.choose_incoming_profile(preview);
        }
    }

    fn choose_incoming_profile(self: &Rc<Self>, preview: bool) {
        let selected = self.selected_incoming_books();
        if selected.is_empty() {
            return;
        }
        let settings = library::organize_settings();
        let profiles: Vec<_> = settings
            .profiles
            .into_iter()
            .filter(|profile| profile.mode == cr_organize::profile::MODE_MOVE)
            .collect();
        if profiles.is_empty() {
            show_report_dialog(
                &self.window,
                "Incoming Adoption",
                "Create a Library Organizer profile with mode Move first.",
            );
            return;
        }
        let names: Vec<String> = profiles
            .iter()
            .map(|profile| profile.name.clone())
            .collect();
        let last = library::incoming_config().last_organizer_profile;
        let preselected = if names.contains(&last) {
            vec![last]
        } else {
            Vec::new()
        };
        let pending = Rc::new(RefCell::new(Some(selected)));
        let weak = Rc::downgrade(self);
        crate::dialogs::organize::show_profile_selector(
            &self.window,
            &names,
            &preselected,
            move |chosen| {
                let Some(name) = chosen.and_then(|names| names.into_iter().next()) else {
                    return;
                };
                let Some(profile) = profiles
                    .iter()
                    .find(|profile| profile.name == name)
                    .cloned()
                else {
                    return;
                };
                let Some(selected) = pending.borrow_mut().take() else {
                    return;
                };
                if let Some(sh) = weak.upgrade() {
                    sh.run_incoming_adoption(selected, profile, preview, false);
                }
            },
        );
    }

    fn run_incoming_adoption(
        self: &Rc<Self>,
        incoming: Vec<ComicBook>,
        mut profile: cr_organize::profile::Profile,
        preview: bool,
        refresh_missing_after: bool,
    ) {
        if !preview && cr_engine::incoming_transaction::operation_active() {
            show_failure_dialog(
                &self.window,
                "Incoming Adoption",
                "Another Incoming operation is active. Try again after it finishes.",
            );
            return;
        }
        let database_snapshot = library::session().borrow().database().clone();
        let incoming_snapshot = library::incoming_session().borrow().clone();
        let captured_epoch = cr_engine::incoming_transaction::database_epoch();
        let library_books = database_snapshot.books.clone();
        let library_ids: std::collections::HashSet<CrGuid> =
            library_books.iter().map(|book| book.id).collect();
        if incoming.iter().any(|book| library_ids.contains(&book.id)) {
            show_failure_dialog(
                &self.window,
                "Incoming Adoption",
                "A selected Incoming ID already exists in the library.",
            );
            return;
        }
        let chosen_name = profile.name.clone();
        if preview {
            profile.mode = cr_organize::profile::MODE_SIMULATE.to_string();
        }
        let selected_ids: std::collections::HashSet<CrGuid> =
            incoming.iter().map(|book| book.id).collect();
        let pool = Arc::clone(&self.pool);
        let catalog_result = Arc::new(std::sync::Mutex::new(None));
        let worker_result = Arc::clone(&catalog_result);
        let settings = library::settings().borrow().clone();
        let weak = Rc::downgrade(self);
        if !preview && !cr_engine::incoming_transaction::begin_operation() {
            show_failure_dialog(
                &self.window,
                "Incoming Adoption",
                "Another operation is active. Try again after it finishes.",
            );
            return;
        }
        crate::dialogs::organize::show_custom_run(
            &self.window,
            if preview {
                "Preview Adoption"
            } else {
                "Adopt Incoming"
            },
            move |ui, cancel| {
                let _guard =
                    (!preview).then(cr_engine::incoming_transaction::acquire_mutation_guard);
                if !preview && cr_engine::incoming_transaction::database_epoch() != captured_epoch {
                    return failed_run_outcome(
                        "The library changed before adoption started. Try again.".into(),
                    );
                }
                let paths = cr_core::paths::Paths::new_default();
                let database_path = cr_core::paths::database_file(&paths);
                let mut database = database_snapshot;
                let mut catalog = incoming_snapshot;
                let mut books = database.books.clone();
                let selected_start = books.len();
                books.extend(catalog.books.iter().cloned());
                let selected: Vec<usize> = books
                    .iter()
                    .enumerate()
                    .skip(selected_start)
                    .filter(|(_, book)| selected_ids.contains(&book.id))
                    .map(|(index, _)| index)
                    .collect();
                struct Cover(Arc<ImagePool>);
                impl cr_organize::engine::CoverSource for Cover {
                    fn fileless_cover(&self, book: &ComicBook) -> Option<cr_image::Image> {
                        let key = book.custom_thumbnail_key.as_ref()?;
                        let bytes = self.0.read_custom_thumbnail(key)?;
                        cr_image::decode(&bytes).ok()
                    }
                    fn duplicate_cover(&self, _book: &ComicBook) -> Option<Vec<u8>> {
                        None
                    }
                }
                let trash = |path: &str| library::trash_file(path);
                let profiles = [profile];
                let incoming_bytes = match catalog.to_bytes() {
                    Ok(bytes) => bytes,
                    Err(error) => return failed_run_outcome(error.to_string()),
                };
                let database_bytes = match cr_core::database::comic_database::save_bytes(&database)
                {
                    Ok(bytes) => bytes,
                    Err(error) => return failed_run_outcome(error.to_string()),
                };
                let initial = cr_engine::incoming_transaction::IncomingTransaction {
                    kind: cr_engine::incoming_transaction::TransactionKind::Adoption,
                    stage: cr_engine::incoming_transaction::TransactionStage::Prepared,
                    files: cr_engine::incoming_transaction::TransactionFiles {
                        incoming_catalog: Some(live_snapshot(
                            cr_core::paths::incoming_file(&paths),
                            incoming_bytes,
                        )),
                        comic_database: Some(live_snapshot(database_path, database_bytes)),
                        ..Default::default()
                    },
                    external_actions: Vec::new(),
                };
                let effects = IncomingOrganizerEffects::new(
                    initial,
                    database.clone(),
                    catalog.clone(),
                    None,
                    captured_epoch,
                );
                let context = cr_organize::engine::RunContext {
                    books: &books,
                    selected: &selected,
                    profiles: &profiles,
                    move_landing: cr_organize::engine::MoveLanding::InsertPreservingId,
                    trash: &trash,
                    filesystem_effects: (!preview)
                        .then_some(&effects as &dyn cr_organize::engine::FilesystemEffects),
                    cover: &Cover(pool),
                    undo_path: None,
                    cancel,
                };
                let report = cr_organize::engine::organize(context, ui);
                if !preview {
                    let adopted: std::collections::HashSet<CrGuid> = report
                        .applies
                        .iter()
                        .filter_map(|apply| match apply {
                            cr_organize::engine::Apply::Adopt(book) => Some(book.id),
                            _ => None,
                        })
                        .collect();
                    catalog.books.retain(|book| !adopted.contains(&book.id));
                    apply_organizer_results(&mut database, &report.applies);
                    let undo_path = library::organizer_undo_path();
                    let manifest_path = cr_organize::engine::adoption_manifest_path(&undo_path);
                    let manifest = cr_organize::engine::AdoptionManifest::from_adoptions(
                        &report.undo,
                        &report.applies,
                    );
                    let mut config = library::incoming_config();
                    config.last_organizer_profile = chosen_name.clone();
                    let incoming_table = cr_core::settings::unified::serialize_plugin(&config)
                        .ok_or_else(|| {
                            "The Incoming configuration could not be serialized.".to_string()
                        });
                    let result = incoming_table.and_then(|table| {
                        let config_path = cr_core::paths::config_file(&paths);
                        let config_bytes =
                            cr_core::settings::unified::save_bytes_with_plugin_tables(
                                &settings,
                                &[(library::INCOMING_PLUGIN.to_string(), table)],
                            )
                            .map_err(|error| error.to_string())?;
                        let auxiliary = if report.undo.is_empty() {
                            Vec::new()
                        } else {
                            vec![
                                file_snapshot(
                                    undo_path,
                                    report
                                        .undo
                                        .to_bytes(false)
                                        .map_err(|error| error.to_string())?,
                                ),
                                file_snapshot(
                                    manifest_path,
                                    manifest.to_bytes().map_err(|error| error.to_string())?,
                                ),
                            ]
                        };
                        let committed_epoch = effects.finish(
                            _guard.as_ref().unwrap(),
                            &database,
                            &catalog,
                            auxiliary,
                            Some(file_snapshot(config_path, config_bytes)),
                        )?;
                        Ok((database, catalog, config, committed_epoch))
                    });
                    *worker_result.lock().unwrap() = Some(result);
                }
                crate::dialogs::organize::RunOutcome {
                    text: report.text,
                    failed_or_skipped: report.failed_or_skipped,
                    applies: report.applies,
                    residual: None,
                }
            },
            move |outcome| {
                if preview {
                    if let Some(sh) = weak.upgrade() {
                        show_report_dialog(&sh.window, "Preview Adoption", &outcome.text);
                    }
                    return;
                }
                let mut committed = false;
                match catalog_result.lock().unwrap().take() {
                    Some(Ok((database, catalog, config, committed_epoch)))
                        if cr_engine::incoming_transaction::database_epoch() == committed_epoch =>
                    {
                        library::session()
                            .borrow_mut()
                            .install_persisted_database(database);
                        library::replace_incoming_catalog(catalog);
                        cr_core::settings::unified::set_plugin(library::INCOMING_PLUGIN, &config);
                        committed = true;
                    }
                    Some(Ok(_)) => {
                        if let Some(sh) = weak.upgrade() {
                            show_failure_dialog(
                                &sh.window,
                                "Incoming Adoption",
                                "The live library changed after the transaction committed. Restart ComicRust to load the saved catalogs.",
                            );
                        }
                    }
                    Some(Err(error)) => {
                        if let Some(sh) = weak.upgrade() {
                            show_failure_dialog(&sh.window, "Incoming Adoption", &error);
                        }
                    }
                    None => {}
                }
                if let Some(sh) = weak.upgrade() {
                    if committed && refresh_missing_after {
                        sh.refresh_missing_issues();
                    }
                    sh.refresh_view_from_list();
                    sh.sync_enabled();
                }
                cr_engine::incoming_transaction::end_operation();
            },
        );
    }

    fn discard_incoming(self: &Rc<Self>) {
        let selected = self.selected_incoming_books();
        if selected.is_empty() {
            return;
        }
        let confirm = gtk4::MessageDialog::builder()
            .transient_for(&self.window)
            .modal(true)
            .title("Discard Incoming Books")
            .text("Move the selected Incoming files to the trash?")
            .message_type(gtk4::MessageType::Question)
            .buttons(gtk4::ButtonsType::OkCancel)
            .build();
        let permanent = gtk4::CheckButton::with_label("Delete permanently (do not use the trash)");
        if let Some(area) = confirm
            .child()
            .and_downcast::<gtk4::Box>()
            .and_then(|vbox| vbox.first_child().and_downcast::<gtk4::Box>())
        {
            area.append(&permanent);
        }
        let weak = Rc::downgrade(self);
        confirm.connect_response(move |dialog, response| {
            let permanent = permanent.is_active();
            dialog.close();
            if response != gtk4::ResponseType::Ok {
                return;
            }
            let items: Vec<_> = selected
                .iter()
                .map(|book| (book.id, book.file_path.clone()))
                .collect();
            let weak_done = weak.clone();
            library::discard_incoming_async(items, permanent, move |result| {
                if let Some(sh) = weak_done.upgrade() {
                    match result {
                        Ok((catalog, outcome, committed_epoch)) => {
                            if cr_engine::incoming_transaction::database_epoch()
                                == committed_epoch
                            {
                                {
                                    let session = library::session();
                                    session
                                        .borrow_mut()
                                        .suppress_watch_paths(outcome.deleted_paths.clone());
                                }
                                library::replace_incoming_catalog(catalog);
                            } else {
                                show_failure_dialog(
                                    &sh.window,
                                    "Discard Incoming Books",
                                    "The live library changed after the transaction committed. Restart ComicRust to load the saved catalog.",
                                );
                                return;
                            }
                            if outcome.failed > 0 {
                                show_report_dialog(
                                    &sh.window,
                                    "Discard Incoming Books",
                                    &format!("{} file(s) could not be deleted.", outcome.failed),
                                );
                            }
                        }
                        Err(error) => {
                            show_failure_dialog(&sh.window, "Discard Incoming Books", &error)
                        }
                    }
                    sh.refresh_view_from_list();
                    sh.sync_enabled();
                }
            });
        });
        confirm.present();
    }

    fn refresh_incoming_from_comic_vine(self: &Rc<Self>) {
        let selected = self.selected_incoming_books();
        let linked: Vec<_> = selected
            .into_iter()
            .filter(|book| !book.file_path.is_empty())
            .collect();
        if linked.is_empty() {
            show_failure_dialog(
                &self.window,
                "Refresh from Comic Vine",
                "Select one or more linked Incoming books.",
            );
            return;
        }
        let volume_ids = library::selected_incoming_volume_ids(&linked);
        if volume_ids.is_empty() {
            show_failure_dialog(
                &self.window,
                "Refresh from Comic Vine",
                "The selected Incoming series do not name a Comic Vine volume. Scrape a matching book first.",
            );
            return;
        }
        let config = library::scraper_config();
        if !config.has_api_key() {
            show_failure_dialog(
                &self.window,
                "Refresh from Comic Vine",
                "No Comic Vine API key is set. Set it in Preferences ▸ Comic Vine Scraper.",
            );
            return;
        }
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let api_key = config.api_key.clone();
        let budget_config = config.clone();
        let (_, policy, _) = cr_scrape::cache::policies_from(config.advanced());
        self.run_cv_job(
            library::CvJobKind::IncomingRefresh,
            "Refresh from Comic Vine",
            cancel,
            move |progress| {
                let Some(cache) = library::cv_cache() else {
                    return Err("The Comic Vine cache file could not be opened.".to_string());
                };
                let mut client = cr_scrape::cv::connection::CvClient::new(&api_key);
                if let Some(budget) = library::cv_budget(
                    &budget_config,
                    Arc::clone(&cache),
                    Arc::clone(&worker_cancel),
                    Some(wait_reporter(progress.clone())),
                ) {
                    client.set_budget(budget);
                }
                let mut refreshed = 0usize;
                let mut requests = 0usize;
                let mut failures = Vec::new();
                let total = volume_ids.len();
                for (index, volume_id) in volume_ids.into_iter().enumerate() {
                    if worker_cancel.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                    let _ = progress.send(CvProgressMsg::Step {
                        detail: format!("volume {} of {total}", index + 1),
                        done: index as i64,
                        total: total as i64,
                    });
                    match cr_scrape::cache::freshness::issues_of_volume(
                        &client,
                        cache.as_ref(),
                        volume_id,
                        &policy,
                        chrono::Utc::now().timestamp(),
                    ) {
                        Ok((_, report)) => {
                            refreshed += 1;
                            requests += report.requests;
                        }
                        Err(error) => failures.push(format!("Volume {volume_id}: {error}")),
                    }
                }
                Ok((refreshed, requests, failures))
            },
            move |window, result| {
                let (refreshed, requests, failures) = match result {
                    Ok(result) => result,
                    Err(error) => {
                        show_failure_dialog(window, "Refresh from Comic Vine", &error);
                        return;
                    }
                };
                library::refresh_incoming_external_gaps_async();
                if failures.is_empty() {
                    show_report_dialog(
                        window,
                        "Refresh from Comic Vine",
                        &format!("{refreshed} volume(s) refreshed. {requests} request(s) used."),
                    );
                } else {
                    show_failure_dialog(
                        window,
                        "Refresh from Comic Vine",
                        &format!(
                            "{refreshed} volume(s) refreshed. {} volume(s) failed.\n{}",
                            failures.len(),
                            failures.join("\n")
                        ),
                    );
                }
            },
        );
    }

    /// Opens a comic into the docked reader and shows it (the C#
    /// `OpenComic`; the reader tab selects and the workspace swaps).
    fn open_comic(&self, path: &Path) {
        self.open_comic_at(path, false, 0);
    }

    /// The `OpenSupportedFile` open path: `new_slot` forces a fresh
    /// reader tab (the second-instance handoff), `page` (0-based)
    /// opens there instead of the resume position (`books.Open(file,
    /// newSlot, page)`).
    fn open_comic_at(&self, path: &Path, new_slot: bool, page: i32) {
        // The C# `Open(ComicBook)` gate (NavigatorManager.cs): a
        // fileless book (`!IsLinked`) never opens a reader slot.
        if path.as_os_str().is_empty() {
            return;
        }
        match self.reader.open_comic_at(path, new_slot, page) {
            Ok(()) => {
                self.stack.set_visible_child_name("reader");
                self.window.present();
            }
            Err(err) => {
                show_failure_dialog(
                    &self.window,
                    &format!("Cannot open {}", path.to_string_lossy()),
                    &format!("{err:#}"),
                );
            }
        }
    }

    fn show_browser(&self) {
        self.stack.set_visible_child_name("browser");
    }

    /// Selects a browser workspace tab (`ShowView(tsbLibrary)`/
    /// `ShowView(tsbFolders)`/`ShowView(tsbPages)`); the reader
    /// hides behind it.
    fn select_workspace(&self, ws: Workspace) {
        match ws {
            Workspace::Library => {
                self.last_browser.set(0);
                self.stack.set_visible_child_name("browser");
            }
            Workspace::Folders => {
                self.last_browser.set(2);
                self.stack.set_visible_child_name("folders");
            }
            Workspace::Pages => {
                self.last_browser.set(1);
                self.stack.set_visible_child_name("pages");
            }
        }
    }

    /// `ShowLast` — the last browser workspace tab.
    fn select_last_browser(&self) {
        let ws = match self.last_browser.get() {
            1 => Workspace::Pages,
            2 => Workspace::Folders,
            _ => Workspace::Library,
        };
        self.select_workspace(ws);
    }

    /// `OpenBooks_Clicked` / a comic tab click: the slot selects and
    /// the reader workspace shows (`ShowView(i)` → the comic viewer
    /// covers the browser).
    fn activate_slot(&self, slot: usize) {
        self.reader.switch_to_slot(slot);
        // A slot with a book shows the reader; the EMPTY slot hosts
        // the QuickOpen covers (the C# empty-reader overlay shape —
        // the same state the `+` tab lands in).
        if self.reader.has_current_book() {
            self.stack.set_visible_child_name("reader");
        } else {
            self.show_quick_open();
        }
    }

    /// The workspace tab strip click (`Selected`/`CaptionClick`):
    /// selecting another item swaps the workspace; re-clicking the
    /// SELECTED item toggles the browser (the C# `tab_CaptionClick`
    /// → `ToggleBrowser`).
    fn on_tab_select(&self, id: &TabId) {
        let visible = self
            .stack
            .visible_child_name()
            .map(|s| s.to_string())
            .unwrap_or_default();
        match id {
            TabId::Library => {
                if visible == "browser" {
                    self.toggle_browser();
                } else {
                    self.select_workspace(Workspace::Library);
                }
            }
            TabId::Folders => {
                if visible == "folders" {
                    self.toggle_browser();
                } else {
                    self.select_workspace(Workspace::Folders);
                }
            }
            TabId::Pages => {
                if visible == "pages" {
                    self.toggle_browser();
                } else {
                    self.select_workspace(Workspace::Pages);
                }
            }
            TabId::Comic(slot) => {
                // The C# wires CaptionClick only on the WORKSPACE
                // items (`MainView.cs:161-163` — Library/Folders/
                // Pages); comic file tabs never toggle: a re-click on
                // the current comic's tab stays on the page (the C#
                // `ShowView` re-selects the viewer, no toggle).
                self.activate_slot(*slot);
            }
            TabId::Plus => {
                // `OpenBooks.AddSlot` + `CurrentSlot = last`: the new
                // empty slot selects and shows. The C# comic viewer
                // carries the QuickOpen overlay in this state
                // (`readerContainer.Controls.Add(quickOpenView)` —
                // the empty reader area IS QuickOpen when
                // `ShowQuickOpen` and the database has books); the
                // blank reader otherwise.
                self.reader.add_empty_slot();
                self.show_quick_open();
            }
        }
        // The strip state renders from the workspace (the T6
        // lesson) — re-sync after every click.
        self.sync_enabled();
    }

    /// Pushes the open slots into the strip and derives its
    /// selection from the visible workspace (the T6 lesson: state
    /// renders from the source of truth, never from the click).
    fn sync_tabs(&self) {
        let infos = self.reader.tab_infos();
        self.tab_strip.set_tabs(&infos);
        // `tsbPages.Visible = OpenBooks.CurrentBook != null`.
        self.tab_strip
            .set_pages_visible(self.reader.has_current_book());
        // `fileTab.Visible = BrowserDock == Fill && !ReaderUndocked`.
        self.tab_strip
            .set_comic_tabs_visible(!self.reader.is_undocked());
        let selected = match self.stack.visible_child_name().as_deref() {
            Some("browser") => TabId::Library,
            Some("folders") => TabId::Folders,
            Some("pages") => TabId::Pages,
            _ => self
                .reader
                .current_slot_id()
                .map(TabId::Comic)
                .unwrap_or(TabId::Library),
        };
        self.tab_strip.set_selected(&selected);
    }

    /// The QuickOpen display (the C# shows the overlay inside the
    /// EMPTY READER AREA — `UpdateQuickList`): visible when
    /// `ShowQuickOpen` and the database has books, the blank reader
    /// (the empty slot) otherwise.
    fn show_quick_open(&self) {
        if !cr_ui_settings().borrow().show_quick_open {
            self.stack.set_visible_child_name("reader");
            return;
        }
        let lists = library::quick_open_lists();
        let total: usize = lists.iter().map(|(_, b)| b.len()).sum();
        if total == 0 {
            self.stack.set_visible_child_name("reader");
            return;
        }
        let mut books: Vec<ComicBook> = Vec::new();
        for (_, group) in lists {
            books.extend(group);
        }
        self.quick_view.set_books(books);
        self.stack.set_visible_child_name("quickopen");
    }

    /// The scan pump's incremental append: returns false when the view
    /// does not show the Library root (the caller falls back to the
    /// full list refresh — smart lists re-evaluate on it).
    fn append_scan_batch(&self, batch: &[ComicBook]) -> bool {
        let id = *self.current_list.borrow();
        let Some(id) = id else {
            return false;
        };
        if !library::is_library_list(&id) {
            return false;
        }
        let selected = self.item_view.selection_ids();
        self.item_view.append_books(batch.to_vec());
        if !selected.is_empty() {
            self.item_view.reselect(&selected);
        }
        true
    }

    /// The view changed by a user action: the current list now owns
    /// its settings (ADR-039). Every handler that alters the mode,
    /// the item size, the columns, the sort or the grouping calls
    /// this. The write itself happens when the list leaves.
    pub(crate) fn mark_view_config_dirty(&self) {
        self.view_config_dirty.set(true);
    }

    /// Writes the live view into the OUTGOING list's `<Display><View>`
    /// (the C# `ComicBrowserControl.UpdateViewConfig`, `:3355-3390`).
    ///
    /// Gated on the dirty flag on purpose: an untouched list must
    /// keep an ABSENT `<View>` so it goes on inheriting. Without the
    /// gate, merely visiting a list would freeze the current view
    /// onto it.
    fn store_view_config(&self, list: Option<CrGuid>) {
        if !self.view_config_dirty.get() {
            return;
        }
        self.view_config_dirty.set(false);
        let Some(id) = list else {
            return;
        };
        if super::navigator::is_fixed_virtual_id(&id) {
            return;
        }
        let cfg = super::list_view_config::collect(&self.item_view);
        if library::is_incoming_custom_list(&id) {
            let window = self.window.clone();
            library::set_incoming_list_view_config(&id, Some(cfg), move |result| {
                if let Err(error) = result {
                    show_failure_dialog(&window, "Save Incoming Smart List", &error);
                }
            });
        } else {
            library::set_list_view_config(&id, Some(cfg));
        }
    }

    /// Applies the INCOMING list's own settings (the C#
    /// `ComicBrowserControl.RegisterBookList`, `:1926-1954`).
    ///
    /// A list with no `<View>` changes nothing: the browser keeps the
    /// view it shows, which is the C# behavior for a null config.
    fn apply_view_config(&self, list: &CrGuid) {
        if super::navigator::is_missing_issues_id(list) {
            // Forced Detail mode, no thumbnails, and exactly the five
            // columns the report needs (Phase 19) — never the user's
            // per-list configuration.
            self.item_view.configure(|c| c.mode = ItemViewMode::Detail);
            self.item_view
                .set_detail_columns_state(&super::columns::missing_issues_columns_state());
            self.view_config_dirty.set(false);
            return;
        }
        if super::navigator::is_incoming_fixed_id(list) {
            self.view_config_dirty.set(false);
            return;
        }
        let config = if library::is_incoming_custom_list(list) {
            library::incoming_list_view_config(list)
        } else {
            library::list_view_config(list)
        };
        if let Some(cfg) = config {
            super::list_view_config::apply(&self.item_view, &cfg);
        }
        self.view_config_dirty.set(false);
    }

    fn refresh_view_from_list(&self) {
        let id = *self.current_list.borrow();
        if let Some(id) = id {
            if library::is_incoming_custom_list(&id) {
                self.evaluate_incoming_custom_list(id);
                return;
            }
            if let Some((name, books)) = Self::evaluate_source(&id) {
                crate::trace::trace(format!("refresh: evaluate {} books", books.len()));
                // The list name feeds the status panel (a rename
                // shows on the next refresh without a re-select).
                *self.current_list_name.borrow_mut() = name;
                // Hold the pre-refresh scroll offset. A classification
                // change lands as an empty pass then a populated pass;
                // capture once (the first pass), restore when a
                // non-empty set lands.
                if self.pending_scroll_restore.get().is_none() {
                    self.pending_scroll_restore
                        .set(Some(self.item_view.scroll_value()));
                }
                // The C# refresh updates the items in place — the
                // selection survives (the My Rating check reads the
                // selection right after the rating commit).
                let selected = self.item_view.selection_ids();
                let t_set = std::time::Instant::now();
                let empty = books.is_empty();
                self.item_view.set_books(books);
                crate::trace::trace(format!("refresh: set_books {:?}", t_set.elapsed()));
                let t_re = std::time::Instant::now();
                if !selected.is_empty() {
                    self.item_view.reselect(&selected);
                }
                crate::trace::trace(format!("refresh: reselect {:?}", t_re.elapsed()));
                if !empty {
                    if let Some(offset) = self.pending_scroll_restore.take() {
                        self.item_view.set_scroll_value(offset);
                    }
                }
            }
        }
    }

    fn evaluate_source(id: &CrGuid) -> Option<(String, Vec<ComicBook>)> {
        if super::navigator::is_missing_issues_id(id) {
            return Some((
                "Missing Issues".to_string(),
                library::missing_issues_snapshot(),
            ));
        }
        let Some(view) = super::navigator::IncomingView::from_id(id) else {
            return library::evaluate_books(id);
        };
        if view == super::navigator::IncomingView::All {
            return Some((view.name().to_string(), library::incoming_books_snapshot()));
        }
        let snapshot = library::incoming_classification_snapshot();
        let snapshot = snapshot.unwrap_or_else(|| library::IncomingClassificationSnapshot {
            books: library::incoming_books_snapshot(),
            classifications: Vec::new(),
        });
        let books = snapshot
            .classifications
            .into_iter()
            .filter(|classification| match view {
                super::navigator::IncomingView::All => true,
                super::navigator::IncomingView::GapFills => classification.gap_fill,
                super::navigator::IncomingView::Duplicates => classification.duplicate,
                super::navigator::IncomingView::LibraryDuplicates => {
                    classification.library_duplicate
                }
                super::navigator::IncomingView::IncomingDuplicates => {
                    classification.incoming_duplicate
                }
                super::navigator::IncomingView::NewSeries => classification.new_series,
                super::navigator::IncomingView::NeedsReview => classification.needs_review,
            })
            .filter_map(|classification| snapshot.books.get(classification.index).cloned())
            .collect();
        Some((view.name().to_string(), books))
    }

    fn evaluate_incoming_custom_list(&self, id: CrGuid) {
        let generation = self.incoming_list_eval_gen.get().wrapping_add(1);
        self.incoming_list_eval_gen.set(generation);
        if let Some(item) = library::find_incoming_smart_list(&id) {
            *self.current_list_name.borrow_mut() = item.base.name.unwrap_or_default();
        }
        let current_generation = Rc::clone(&self.incoming_list_eval_gen);
        let item_view = self.item_view.clone();
        let window = self.window.clone();
        library::evaluate_incoming_smart_list_async(id, move |result| {
            if current_generation.get() != generation {
                return;
            }
            match result {
                Ok((_name, books)) => {
                    item_view.set_books(books);
                }
                Err(error) => {
                    item_view.set_books(Vec::new());
                    show_failure_dialog(&window, "Invalid Incoming Smart List", &error);
                }
            }
        });
    }

    /// The status-bar panels (`OnUpdateGui`'s strip updates fold
    /// into the same sync the actions ride): the selection info, the
    /// book caption, the page + count, and the thumb slider. The
    /// Files page reads ITS browser (the C# `FindActiveService`
    /// picks the active `IComicBrowser`).
    fn update_status_panels(&self) {
        // The selection info (the C# `SelectionInfo`).
        let page = self.stack.visible_child_name().map(|s| s.to_string());
        let folders_visible = page.as_deref() == Some("folders");
        let (count, total, total_size, selected, selected_size, selected_path) = if folders_visible
        {
            let view = self.folders_view.view_state();
            let selected_ids: Vec<CrGuid> = view.selection_snapshot().into_iter().collect();
            let selected_size: i64 = view
                .books()
                .iter()
                .filter(|b| selected_ids.contains(&b.id))
                .map(|b| b.file_size.max(0))
                .sum();
            let path = if selected_ids.len() == 1 {
                view.books()
                    .iter()
                    .find(|b| b.id == selected_ids[0])
                    .map(|b| b.file_path.clone())
            } else {
                None
            };
            (
                view.len(),
                view.books().len(),
                view.books().iter().map(|b| b.file_size.max(0)).sum(),
                selected_ids.len(),
                selected_size,
                path,
            )
        } else {
            (
                self.item_view.book_count(),
                self.item_view.total_count(),
                self.item_view.visible_size(),
                self.item_view.selection_len(),
                self.item_view.selected_size(),
                if self.item_view.selection_len() == 1 {
                    self.item_view
                        .selection_ids()
                        .first()
                        .and_then(library::book_path)
                } else {
                    None
                },
            )
        };
        let list_name = if folders_visible {
            self.current_folder_name.borrow().clone()
        } else {
            self.current_list_name.borrow().clone()
        };
        // The C# reads the ACTIVE browser service: with the reader
        // or QuickOpen showing, `FindActiveService<IComicBrowser>`
        // returns null and the panel goes EMPTY (the "Ready" text is
        // only the Designer default).
        let browser_visible = page.as_deref() == Some("browser");
        let info = if browser_visible || folders_visible {
            status_bar::selection_info(
                &list_name,
                count,
                total,
                total_size,
                selected,
                selected_size,
                selected_path.as_deref(),
            )
        } else {
            String::new()
        };
        self.status_bar.set_selection_info(&info);

        // The open book (the caption ellipsized in the C# to 60 —
        // the label caps at the same width).
        let caption = self
            .reader
            .tab_infos()
            .into_iter()
            .find(|t| t.current && t.has_book)
            .map(|t| t.caption);
        self.status_bar.set_book(caption.as_deref());

        // The current page + count (the page panel is 1-based;
        // "NA"/"None" without a book).
        let has_book = self.reader.has_current_book();
        let page = if has_book {
            self.reader.current_display_page()
        } else {
            None
        };
        let track = cr_ui_settings().borrow().track_current_page;
        self.status_bar.set_page(page, track);
        let page_count = self.reader.current_book().map(|(_, c)| c).unwrap_or(0);
        self.status_bar
            .set_page_count(&status_bar::page_count_text(page_count));

        // The thumb slider: the browser workspace only (the C#
        // `mainViewContainer.Expanded`), range/value per mode — the
        // ACTIVE browser's view.
        let size = if folders_visible {
            self.folders_view.item_size()
        } else {
            self.item_view.item_size()
        };
        self.status_bar
            .sync_slider(size, browser_visible || folders_visible);
    }
}

impl BrowserShell {
    /// Builds the main window (`MainForm`): the browser view + the
    /// docked reader + the header commands.
    pub fn create(app: &Application) -> (ApplicationWindow, BrowserShell) {
        crate::trace::trace("shell: create start");
        let window = ApplicationWindow::builder()
            .application(app)
            .title("comicrust")
            .default_width(1280)
            .default_height(800)
            .build();
        // F10 = MinimalGui (the C# command). GTK's built-in
        // `handle-menubar-accel` (a CAPTURE-phase F10 shortcut since
        // 4.2) consumes the key to focus a model menubar — our
        // menubar is the custom T3 widget, so the accel never fired
        // (the user report). The window keeps its own F10 meaning.
        window.set_handle_menubar_accel(false);

        // One pool for the whole app (the C# `Program.ImagePool` is
        // global). The `CacheManager` construction: disk caches +
        // memory capacities from the settings, and the cache-event
        // sink that writes decoded page sizes back into the books.
        let pool = Arc::new(ImagePool::with_config(&library::image_pool_config()));
        library::install_cache_events(&pool);
        crate::trace::trace("shell: image pool ready");
        let (reader, reader_widgets) = ReaderShell::new(app, Arc::clone(&pool));
        crate::trace::trace("shell: reader built");
        let navigator = Navigator::new();
        let super::item_view::ItemViewWidgets {
            scroller: item_scroller,
            view: item_view,
        } = ItemView::create(Arc::clone(&pool));
        // The QuickOpen covers (captionless — `HideCaptions`).
        let super::item_view::ItemViewWidgets {
            scroller: quick_scroller,
            view: quick_view,
        } = ItemView::create(Arc::clone(&pool));
        quick_view.configure(|c| c.hide_captions = true);
        // The Files (Folders) view: its own grid over the scanned
        // folder books (the C# `AddExplorerView(null, filesBrowser,
        // tsbFolders)` — a separate browser view per tab).
        let super::item_view::ItemViewWidgets {
            scroller: folders_scroller,
            view: folders_view,
        } = ItemView::create(Arc::clone(&pool));
        let folders_tree = super::folder_tree::FolderTree::create();
        let super::pages_view::PagesPanelWidgets {
            widget: pages_widget,
            panel: pages,
        } = super::pages_view::PagesPanel::create(Arc::clone(&pool), &window);
        crate::trace::trace("shell: views built");

        // The header commands (the handlers wire in `wire`, where
        // the shared state exists). The T6 reorg: Open/Add
        // Folder/Preferences/View/Sort/Group moved into the menubar
        // and the browser toolbar row — the header carries the
        // reader's page display only.
        let header = gtk4::HeaderBar::new();
        // The reader's "Page X of Y" lives in the main window header
        // (the C# main form shows it in the title area).
        header.pack_end(&reader_widgets.subtitle());
        window.set_titlebar(Some(&header));

        // The browser toolbar row (the C# `ComicBrowserControl.
        // toolStrip`): Sidebar, Browse prev/next, Views, Group,
        // Arrange, then the right-aligned Quick Search, List Layouts
        // (disabled), Duplicate List.
        let search = Entry::new();
        let browser_toolbar = super::browser_toolbar::BrowserToolbar::create(&window, &search);

        // The browser page: the navigator pane left, the toolbar +
        // ItemView pane right. The status label moved below the
        // workspace stack (the C# status strip is form-wide).
        let nav_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        nav_box.append(navigator.widget());

        let paned = Paned::new(gtk4::Orientation::Horizontal);
        paned.set_start_child(Some(&nav_box));
        paned.set_shrink_start_child(false);
        paned.set_position(280);
        paned.set_vexpand(true);
        // The status bar sits below the workspace stack (the C#
        // `statusStrip` is form-wide). The panels fill at the first
        // `sync_enabled`.
        let super::status_bar::StatusBarWidgets {
            widget: status_widget,
            bar: status_bar,
        } = super::status_bar::StatusBar::create();
        // The browser toolbar rides the ITEM VIEW pane (the C#
        // toolStrip spans the ComicBrowserControl's list area — the
        // user report: it must start at the left edge of the RIGHT
        // view window, not cover the navigator).
        // The Missing Issues scope/Refresh bar (Phase 19): a second
        // row below the browser toolbar, hidden until that navigator
        // node is selected.
        let missing_issues_bar = super::missing_issues_bar::MissingIssuesBar::create();
        let item_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        item_box.append(browser_toolbar.widget());
        item_box.append(missing_issues_bar.widget());
        item_box.append(&item_scroller);
        paned.set_end_child(Some(&item_box));
        paned.set_shrink_end_child(false);
        let browser_page = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        browser_page.append(&paned);

        // The Files page: the folder tree pane left, the grid right
        // — the SAME side-by-side shape as the Library page (the
        // user report: the grid sat BELOW the paned, a horizontal
        // split).
        let folders_paned = Paned::new(gtk4::Orientation::Horizontal);
        folders_paned.set_start_child(Some(folders_tree.widget()));
        folders_paned.set_shrink_start_child(false);
        folders_paned.set_position(280);
        folders_paned.set_vexpand(true);
        folders_paned.set_end_child(Some(&folders_scroller));
        folders_paned.set_shrink_end_child(false);
        let folders_page = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        folders_page.append(&folders_paned);

        // The quick-open page (the C# reader-area overlay shown when
        // no book is open and `ShowQuickOpen`): the recent lists as
        // captionless covers.
        let quick_page = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        let quick_label = Label::builder()
            .label("Quick Open")
            .halign(gtk4::Align::Start)
            .margin_top(8)
            .margin_start(8)
            .build();
        quick_page.append(&quick_label);
        quick_page.append(&quick_scroller);
        quick_scroller.set_vexpand(true);

        // The workspace stack — the full-window tab contents (the C#
        // `MainView.ShowView`): quick open ⇄ browser ⇄ Pages ⇄
        // reader. The Pages workspace is a full-window tab now (the
        // `ComicPagesView` shape), not a left-panel mini tab. The
        // stack EXPANDS: it owns the window below the bars (the T9
        // user test: the Pages page collapsed to its toolbar
        // without this).
        let stack = Stack::new();
        stack.set_vhomogeneous(false);
        stack.set_hhomogeneous(false);
        stack.set_vexpand(true);
        stack.set_hexpand(true);
        stack.add_named(&quick_page, Some("quickopen"));
        stack.add_named(&browser_page, Some("browser"));
        stack.add_named(&folders_page, Some("folders"));
        let pages_page = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        pages_page.append(&pages_widget);
        stack.add_named(&pages_page, Some("pages"));

        // The menubar (the C# `mainMenuStrip`) rides above the
        // content — the T3 custom bar (GTK4 model menus cannot show
        // the C# menu-item icons).
        let menubar = super::menubar::create_menubar(&window);
        // The workspace tab strip (the T9 `MainView.tabStrip`): the
        // row under the menubar with Library | Pages | the comic tabs
        // | `+`. The reader toolbar (the T5 `mainToolStrip`) docks
        // into its right end (the C# Fill rule:
        // `MainToolStripVisible = false` → the strip lives inside the
        // tab row; Tools/Fullscreen stay reachable from the library
        // view).
        let toolbar = super::toolbar::ReaderToolbar::create(&window);
        let tab_strip = super::tabstrip::TabStrip::create(Arc::clone(&pool));
        tab_strip.host().append(toolbar.widget());
        let reader_page = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        reader_page.append(&reader_widgets.notebook());
        stack.add_named(&reader_page, Some("reader"));
        let content = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        content.append(menubar.widget());
        content.append(tab_strip.widget());
        content.append(&stack);
        content.append(&status_widget);
        window.set_child(Some(&content));
        crate::trace::trace("shell: widgets built");

        let state = Rc::new(ShellState {
            window: window.clone(),
            stack: stack.clone(),
            status_bar,
            navigator: Rc::clone(&navigator),
            item_view,
            folders_tree: Rc::clone(&folders_tree),
            folders_view,
            current_folder_name: RefCell::new(String::new()),
            folder_scan_gen: Cell::new(0),
            quick_view,
            pages,
            pages_page: pages_page.clone(),
            nav_box,
            reader_page_box: tab_strip.host().clone(),
            tab_strip,
            last_browser: Cell::new(0),
            search: search.clone(),
            reader,
            app: app.clone(),
            current_list: RefCell::new(None),
            current_list_name: RefCell::new(String::new()),
            view_config_dirty: Cell::new(false),
            incoming_list_eval_gen: Rc::new(Cell::new(0)),
            pending_scroll_restore: Cell::new(None),
            actions: RefCell::new(HashMap::new()),
            list_history: RefCell::new(Vec::new()),
            list_history_pos: Cell::new(0),
            random_list: RefCell::new(Vec::new()),
            random_picked: RefCell::new(Vec::new()),
            menubar,
            toolbar,
            browser_toolbar,
            missing_issues_bar,
            search_text: RefCell::new(String::new()),
            current_filter: RefCell::new(None),
            // The column chooser builds a FRESH popover per open
            // (the exact shape of the proven book context menu; the
            // last one stays here for the probe).
            columns_drop: RefCell::new(None),
            column_actions: RefCell::new(HashMap::new()),
            context_drop: RefCell::new(None),
            pool,
            tasks_window: RefCell::new(None),
            paned: paned.clone(),
        });
        let shell = BrowserShell {
            window: window.clone(),
            state: Rc::clone(&state),
        };
        shell.wire(&search);
        crate::trace::trace("shell: wired");
        // The persisted workspace restores (the C# `MainForm.Load`
        // applies `Settings.CurrentWorkspace` before the first
        // show). A missing element keeps the defaults.
        if let Some(ws) = cr_ui_settings().borrow().current_workspace.clone() {
            shell.state.apply_workspace(&ws);
        }
        crate::trace::trace("shell: workspace applied");
        (window, shell)
    }

    /// The navigator pane handle (the list-command host).
    pub fn navigator(&self) -> Rc<Navigator> {
        Rc::clone(&self.state.navigator)
    }

    /// The refresh after a direct database mutation outside the
    /// normal view flow (the path-migration apply, the scan lands):
    /// the ItemView re-evaluates, the navigator tree refills, the
    /// action states + status panels re-sync.
    pub fn refresh_after_data_change(&self) {
        let t = std::time::Instant::now();
        self.state.refresh_view_from_list();
        crate::trace::trace(format!("data-change refresh: view {:?}", t.elapsed()));
        self.state
            .navigator
            .refill(&library::comic_lists_snapshot());
        crate::trace::trace(format!("data-change refresh: +navigator {:?}", t.elapsed()));
        self.state.sync_enabled();
        self.state.report_scan_problems();
        crate::trace::trace(format!("data-change refresh: total {:?}", t.elapsed()));
    }

    /// The grid's current view state (display-order checks + probes).
    pub fn item_view_state(&self) -> super::view_state::ViewState {
        self.state.item_view.view_state()
    }

    /// The grid's (sort column, descending, grouper).
    pub fn item_view_sort_summary(&self) -> (Option<String>, bool, Option<&'static str>) {
        self.state.item_view.sort_group_summary()
    }

    /// The main-window menubar (the T3 custom bar; the T14
    /// layout persistence and the probes reach it here).
    pub fn menubar(&self) -> &super::menubar::MenubarWidget {
        &self.state.menubar
    }

    /// The workspace tab strip (the T9 bar; the probes reach it
    /// here).
    pub fn tabstrip(&self) -> super::tabstrip::TabStrip {
        self.state.tab_strip.clone()
    }

    /// The status bar handle (the T8 probe gates the panels).
    pub fn statusbar(&self) -> super::status_bar::StatusBar {
        self.state.status_bar.clone()
    }

    /// The browser grid's current thumb height (the slider resize
    /// gate).
    pub fn state_grid_thumb_height(&self) -> f64 {
        self.state.item_view.thumb_height()
    }

    /// Probe: the browser view mode (the T14 restore gate).
    pub fn state_grid_mode(&self) -> &'static str {
        match self.state.item_view.mode() {
            ItemViewMode::Thumbnail => "thumbnail",
            ItemViewMode::Tile => "tile",
            ItemViewMode::Detail => "detail",
        }
    }

    /// Probe: the "no metadata" tags drawn in the last grid frame
    /// (the metadata-tag gate — settle before reading).
    pub fn state_grid_metadata_badge_draws(&self) -> u32 {
        self.state.item_view.probe_metadata_badge_draws()
    }

    /// Probe: the scan markers ("!" / "≠") drawn in the last grid
    /// frame (settle before reading, like the metadata badge).
    pub fn state_grid_scan_marker_draws(&self) -> u32 {
        self.state.item_view.probe_scan_marker_draws()
    }

    /// Probe: the navigator pane visibility + split (the T14 restore
    /// gate).
    pub fn state_sidebar(&self) -> (bool, i32) {
        (self.state.nav_box.is_visible(), self.state.paned.position())
    }

    /// Probe: the T14 exit snapshot (the collect against the live
    /// widgets; the reader family falls back to the saved one).
    pub fn state_collect_workspace(&self) -> cr_core::settings::workspace::WorkspaceState {
        let prev = cr_ui_settings().borrow().current_workspace.clone();
        self.state.collect_workspace(prev.as_ref())
    }

    /// Probe: the T14 startup restore.
    pub fn state_apply_workspace(&self, ws: &cr_core::settings::workspace::WorkspaceState) {
        self.state.apply_workspace(ws);
    }

    /// Probe: moves the navigator/item split (the T14 collect gate
    /// needs a non-default position to prove the persistence).
    pub fn state_set_paned(&self, position: i32) {
        self.state.paned.set_position(position);
    }

    /// Probe: the Detail column set (id, name, visible) — the T14
    /// restore gate.
    pub fn state_detail_columns(&self) -> Vec<(i32, String, bool)> {
        self.state.item_view.detail_columns_snapshot()
    }

    /// The browser grid's item-size triple (the slider sync gate).
    pub fn state_grid_item_size(&self) -> Option<(f64, f64, f64)> {
        self.state.item_view.item_size()
    }

    /// Dispatches a READER command through the current view (the
    /// page-click path minus the mouse gesture — the ShowBrowser →
    /// ToggleBrowserFromReader gate).
    pub fn state_reader_dispatch(&self, id: &str) {
        self.state.reader.dispatch_current(id);
    }

    /// Probe: the allocated heights of the workspace stack and the
    /// Pages page (the full-window layout gate — the T9 user test
    /// caught the Pages page at its toolbar's height).
    pub fn state_workspace_heights(&self) -> (i32, i32) {
        (self.state.stack.height(), self.state.pages_page.height())
    }

    /// The main window handle.
    pub fn window(&self) -> ApplicationWindow {
        self.window.clone()
    }

    fn wire(&self, search: &Entry) {
        let state = &self.state;

        // The reader docks: the host window drives the fullscreen
        // chrome and the Q exit.
        state.reader.set_host(&self.window);

        // The last reader tab closes → the Library workspace shows
        // again (the C# `Close` → `ShowLibrary`; the strip loses the
        // comic tabs and the Pages tab).
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .reader
                .set_on_last_tab_closed(move || {
                    if let Some(sh) = state.upgrade() {
                        sh.pages.clear_book();
                        // The C# `RebuildBookTabs` tail (MainForm.cs
                        // 3140): the last book closed → `ShowLast()` —
                        // the LAST browser tab returns (Library,
                        // Folders, or Pages — the user report 2026-09-08:
                        // closing a comic opened from the Folders view
                        // must land back on Folders, not QuickOpen).
                        // QuickOpen keeps the `+` empty slot as its
                        // home (the C# shows the QuickOpen overlay in
                        // the empty reader area).
                        sh.select_last_browser();
                        sh.sync_enabled();
                    }
                });
        }

        // The tab set changed (open/close/undock/re-dock/AddSlot) —
        // the strip rebuilds with the sync. The workspace follows
        // the current slot while the reader area shows (the C#
        // comic viewer rebinds on `OpenBooks_CurrentSlotChanged`):
        // a book in the slot → the reader, an empty slot → the
        // QuickOpen covers.
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .reader
                .set_on_tabs_changed(move || {
                    if let Some(sh) = state.upgrade() {
                        let page = sh
                            .stack
                            .visible_child_name()
                            .map(|s| s.to_string())
                            .unwrap_or_default();
                        if page == "reader" || page == "quickopen" {
                            if sh.reader.has_current_book() {
                                sh.stack.set_visible_child_name("reader");
                            } else if !sh.reader.tab_infos().is_empty() {
                                sh.show_quick_open();
                            }
                        }
                        sh.sync_enabled();
                    }
                });
        }

        // A tab closes → the auto Quick Review gate (the C#
        // `OnBookClosing`: AutoShowQuickReview && HasBeenRead &&
        // Rating == 0 — the book leaves, the dialog opens over the
        // library entry).
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .reader
                .set_on_book_closing(move |book| {
                    let auto_show = cr_ui_settings().borrow().auto_show_quick_review;
                    if !crate::dialogs::quick_rating::should_auto_show(book, auto_show) {
                        return;
                    }
                    if let Some(sh) = state.upgrade() {
                        // The rating edits the LIBRARY copy (the
                        // session copy is gone with the closed tab).
                        let Some(current) = library::session()
                            .borrow()
                            .find_book(&book.file_path)
                            .cloned()
                        else {
                            return;
                        };
                        let show_when_read = cr_ui_settings().borrow().auto_show_quick_review;
                        let state2 = Rc::downgrade(&sh);
                        let pool = Arc::clone(&sh.pool);
                        crate::dialogs::quick_rating::show_quick_rating(
                            &sh.window,
                            &current,
                            show_when_read,
                            pool,
                            move |result| {
                                let Some(result) = result else {
                                    return;
                                };
                                cr_ui_settings().borrow_mut().auto_show_quick_review =
                                    result.show_when_read;
                                if let Some(sh) = state2.upgrade() {
                                    sh.set_quick_rating_fields(
                                        &current.id,
                                        result.rating,
                                        &result.review,
                                    );
                                    sh.sync_enabled();
                                }
                            },
                        );
                    }
                });
        }

        // Library-group reader commands (`NextComic`/`PrevComic`/
        // `RandomComic`/`ShowBrowser`) — the C# handlers live on
        // MainForm; the shell owns the list context.
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .reader
                .set_on_library_command(move |id| {
                    if let Some(sh) = state.upgrade() {
                        match id {
                            "NextComic" => sh.open_next_book(1),
                            "PrevComic" => sh.open_next_book(-1),
                            "RandomComic" => sh.open_next_book(0),
                            "ShowBrowser" => {
                                // `ToggleBrowserFromReader`: in Fill
                                // mode (our only mode) the reader's
                                // MouseLeft/Escape command flips
                                // MINIMAL UI, not the browser — the
                                // browser branch runs only with the
                                // MouseSwitchesToFullLibrary
                                // extended setting (MainForm.cs:
                                // 2133-2144).
                                if !cr_core::settings::ExtendedSettings::global()
                                    .mouse_switches_to_full_library
                                {
                                    sh.reader.dispatch_current("ToggleMenu");
                                } else {
                                    sh.toggle_browser();
                                }
                            }
                            _ => {}
                        }
                        sh.sync_enabled();
                    }
                });
        }

        // Undock → the main window reveals the browser (the C#
        // `ReaderUndocked` leaves the main form with its browser);
        // re-dock → the reader page shows again.
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .reader
                .set_on_view_change(move |reader_visible| {
                    if let Some(sh) = state.upgrade() {
                        if reader_visible {
                            sh.stack.set_visible_child_name("reader");
                        } else {
                            sh.select_last_browser();
                        }
                        // Undock/re-dock changes the menubar rule and
                        // hides the comic tabs (the C#
                        // `fileTab.Visible = Fill && !ReaderUndocked`).
                        sh.sync_enabled();
                    }
                });
        }

        // The Pages panel: rebinds on every visible-book change (the
        // C# `Viewer_BookChanged` → `pagesView.Book`; an empty slot
        // clears it), follows the bound book's page turns, and
        // navigates on double-click.
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .reader
                .set_on_book_changed(move || {
                    if let Some(sh) = state.upgrade() {
                        match sh.reader.current_comic_book() {
                            Some(book) => {
                                let page = book.current_page.max(0) as usize;
                                sh.pages.set_book(book);
                                sh.pages.set_current_page(page);
                            }
                            None => sh.pages.clear_book(),
                        }
                        sh.sync_enabled();
                    }
                });
        }
        {
            let state = Rc::downgrade(state);
            state.upgrade().expect("state").reader.set_on_page_change(
                move |page, book_id, last_page_read| {
                    if let Some(sh) = state.upgrade() {
                        sh.pages.set_current_page(page);
                        // The ItemView's read ribbons track the turn
                        // live (the C# ItemView draws the live book
                        // objects; the port's cloned snapshots need
                        // the push). The hook runs INSIDE the
                        // reader-state borrow — no reader access here,
                        // only the passed values.
                        sh.item_view
                            .update_read_state(book_id, page as i32, last_page_read);
                        let track = cr_ui_settings().borrow().track_current_page;
                        sh.status_bar.set_page(Some(page), track);
                    }
                },
            );
        }
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .pages
                .connect_activate(move |page| {
                    if let Some(sh) = state.upgrade() {
                        sh.reader.navigate_current(page);
                        // `ShowComic()`: the double-click reveals the
                        // comic (the reader page wins over the
                        // browser).
                        sh.stack.set_visible_child_name("reader");
                        sh.sync_enabled();
                    }
                });
        }

        // The workspace tab strip: item clicks select the workspace
        // (a re-click on the selected item toggles the browser — the
        // C# `tab_CaptionClick`); the close buttons close the slot.
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .tab_strip
                .connect_select(move |id| {
                    if let Some(sh) = state.upgrade() {
                        sh.on_tab_select(id);
                    }
                });
        }
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .tab_strip
                .connect_close(move |slot| {
                    if let Some(sh) = state.upgrade() {
                        sh.reader.close_slot(slot);
                        sh.sync_enabled();
                    }
                });
        }

        // The Pages tab became visible — reflow with the real
        // allocation (the first show after a hidden binding).
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .stack
                .connect_visible_child_notify(move |_stack| {
                    if let Some(sh) = state.upgrade() {
                        if sh.stack.visible_child_name().as_deref() == Some("pages") {
                            sh.pages.reflow();
                        }
                    }
                });
        }

        // The QuickOpen covers: double-click opens the comic
        // (`QuickOpenBookActivated`).
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .quick_view
                .connect_activate(move |id| {
                    if let Some(sh) = state.upgrade() {
                        if let Some(path) = library::book_path(id) {
                            sh.open_comic(Path::new(&path));
                        }
                    }
                });
        }

        // The navigator selection → the ItemView book set (debounced
        // inside the widget) + the status bar count.
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .navigator
                .connect_selected(move |id, _name| {
                    if let Some(sh) = state.upgrade() {
                        // A real list switch (and the first selection
                        // after boot) carries the per-list view
                        // settings across: the outgoing list keeps
                        // what the user changed, the incoming list
                        // applies its own (ADR-039). A re-select of
                        // the SAME list (a refresh, a boot re-fill)
                        // does neither — it must not overwrite an
                        // unsaved change with itself.
                        let prev = *sh.current_list.borrow();
                        let changing = prev != Some(*id);
                        if changing {
                            sh.incoming_list_eval_gen
                                .set(sh.incoming_list_eval_gen.get().wrapping_add(1));
                            sh.store_view_config(prev);
                        }
                        *sh.current_list.borrow_mut() = Some(*id);
                        if changing {
                            // A list switch resets the view to the top;
                            // drop any held in-place-refresh offset.
                            sh.pending_scroll_restore.set(None);
                        }
                        // The list history (`BrowsePrevious` chain): a
                        // history walk lands on the entry at the walk
                        // position and does not append; a new
                        // selection drops the forward entries.
                        {
                            let mut h = sh.list_history.borrow_mut();
                            let pos = sh.list_history_pos.get();
                            if h.get(pos) != Some(id) {
                                h.truncate(pos + 1);
                                h.push(*id);
                                sh.list_history_pos.set(h.len() - 1);
                            }
                        }
                        let t_eval = std::time::Instant::now();
                        if library::is_incoming_custom_list(id) {
                            sh.evaluate_incoming_custom_list(*id);
                        } else if let Some((name, books)) = ShellState::evaluate_source(id) {
                            // The list name feeds the status-bar
                            // selection panel (`BookList.Name`).
                            *sh.current_list_name.borrow_mut() = name;
                            crate::trace::trace(format!(
                                "nav select: evaluate {} books {:?}",
                                books.len(),
                                t_eval.elapsed()
                            ));
                            let t_set = std::time::Instant::now();
                            sh.item_view.set_books(books);
                            crate::trace::trace(format!(
                                "nav select: set_books {:?}",
                                t_set.elapsed()
                            ));
                        }
                        if super::navigator::IncomingView::from_id(id).is_some() {
                            library::refresh_incoming_classification_async();
                        }
                        if changing {
                            sh.apply_view_config(id);
                        }
                        if super::navigator::is_missing_issues_id(id) {
                            sh.missing_issues_bar
                                .refill_scope(&library::smart_list_scope_options());
                            sh.missing_issues_bar
                                .set_status(&missing_issues_status_text());
                        }
                        sh.missing_issues_bar
                            .widget()
                            .set_visible(super::navigator::is_missing_issues_id(id));
                        sh.sync_enabled();
                    }
                });
        }

        // The navigator Refresh button: refill the tree from the
        // library snapshot and re-evaluate the current list (the
        // C# `FillListTree` + `UpdateBookList` refresh shape).
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .navigator
                .connect_refresh(move || {
                    if let Some(sh) = state.upgrade() {
                        sh.navigator.refill(&library::comic_lists_snapshot());
                        sh.refresh_view_from_list();
                    }
                });
        }

        // The Missing Issues Refresh button (Phase 19): the report's
        // only trigger — it never auto-recomputes. Scopes the pass to
        // the whole library or to the series a chosen smart list
        // selects, then polls until the worker lands and, only if the
        // user is still on this view, replaces the ItemView's rows.
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .missing_issues_bar
                .connect_refresh(move || {
                    if let Some(sh) = state.upgrade() {
                        sh.refresh_missing_issues();
                    }
                });
        }

        // The in-widget view changes (Ctrl+wheel item resize, a
        // column-width drag, a header auto-size): the current list
        // owns its view settings from here on (ADR-039).
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .item_view
                .connect_view_config_changed(move || {
                    if let Some(sh) = state.upgrade() {
                        sh.mark_view_config_dirty();
                    }
                });
        }

        // The selection change feeds the status-bar selection panel
        // + the enable-state (both ride the sync).
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .item_view
                .connect_selection_changed(move |_selected| {
                    if let Some(sh) = state.upgrade() {
                        sh.sync_enabled();
                    }
                });
        }

        // Double-click / Enter → open in the (docked) reader.
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .item_view
                .connect_activate(move |id| {
                    if let Some(sh) = state.upgrade() {
                        let path = if sh.is_incoming_view() {
                            library::incoming_books_by_ids(&[*id])
                                .into_iter()
                                .next()
                                .map(|book| book.file_path)
                        } else {
                            library::book_path(id)
                        };
                        if let Some(path) = path.filter(|path| !path.is_empty()) {
                            sh.open_comic(Path::new(&path));
                        }
                    }
                });
        }

        // The right-click context menu (open / reveal / remove /
        // properties stub).
        {
            state.item_view.connect_context({
                let state = Rc::downgrade(state);
                move |id, x, y| {
                    show_context_menu(&state, id, x, y);
                }
            });
        }

        // Delete opens the Remove from Library flow (the C#
        // `itemView_KeyDown` shortcut, ComicBrowserControl.cs:
        // 1472-1478).
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .item_view
                .connect_remove(move || {
                    if let Some(sh) = state.upgrade() {
                        if sh.is_incoming_view() {
                            sh.discard_incoming();
                        } else {
                            run_remove_books(&Rc::downgrade(&sh));
                        }
                    }
                });
        }

        // The quick search (`UpdateQuickFilter`): the composed filter
        // (scope + the view filters + the text, or a MATCH/NOT
        // query). Debounced.
        {
            let state = Rc::downgrade(state);
            search.connect_changed(move |entry| {
                let text = entry.text().to_string();
                let state = state.clone();
                glib::timeout_add_local(
                    std::time::Duration::from_millis(SEARCH_DEBOUNCE_MS),
                    move || {
                        if let Some(sh) = state.upgrade() {
                            *sh.search_text.borrow_mut() = text.clone();
                            sh.rebuild_filter();
                        }
                        glib::ControlFlow::Break
                    },
                );
            });
        }

        // The Detail header right-click → the column chooser (the
        // C# `autoHeaderContextMenuStrip`).
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .item_view
                .connect_header_context(move |wx, wy| {
                    if let Some(sh) = state.upgrade() {
                        sh.popup_column_chooser(wx, wy);
                    }
                });
        }

        // ----- The Files (Folders) view wiring -----
        // The folder tree selection → the scan → the grid
        // (`tvFolders_AfterSelect` → `FillBooks`; the provider
        // refreshes on the Path change).
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .folders_tree
                .connect_selected(move |path| {
                    let include_sub = library::settings().borrow().explorer_include_sub_folders;
                    scan_folder_async(&state, path.to_string(), include_sub);
                });
        }
        // Include Sub Folders: the setting flips and the current
        // folder rescans (`SwitchIncludeSubFolders` → the provider
        // Refresh).
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .folders_tree
                .connect_include_sub(move |active| {
                    if let Some(sh) = state.upgrade() {
                        library::settings()
                            .borrow_mut()
                            .explorer_include_sub_folders = active;
                        if let Some(path) = sh.folders_tree.current_folder() {
                            scan_folder_async(&state, path, active);
                        }
                    }
                });
        }
        // Add To Favorites (`AddToFavorites`).
        {
            let state = Rc::downgrade(state);
            let Some(btn) = state
                .upgrade()
                .map(|sh| sh.folders_tree.button("add-favorite"))
                .unwrap_or(None)
            else {
                return;
            };
            btn.connect_clicked(move |_| {
                if let Some(sh) = state.upgrade() {
                    super::folder_tree::add_favorite(&sh.folders_tree);
                }
            });
        }
        // Add Folder To Library (`miAddFolderLibrary` →
        // `Scanner.ScanFileOrFolder(CurrentFolder, all: true)`).
        {
            let state = Rc::downgrade(state);
            let Some(btn) = state
                .upgrade()
                .map(|sh| sh.folders_tree.button("add-library"))
                .unwrap_or(None)
            else {
                return;
            };
            btn.connect_clicked(move |_| {
                if let Some(sh) = state.upgrade() {
                    if let Some(path) = sh.folders_tree.current_folder() {
                        let state2 = state.clone();
                        library::add_folder_to_library(Path::new(&path), move |_| {
                            if let Some(sh) = state2.upgrade() {
                                sh.refresh_view_from_list();
                                sh.sync_enabled();
                                sh.report_scan_problems();
                            }
                        });
                    }
                }
            });
        }
        // The Files grid: double-click / Enter opens the comic (the
        // C# books path — the session books carry real file paths).
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .folders_view
                .connect_activate(move |id| {
                    if let Some(sh) = state.upgrade() {
                        let view = sh.folders_view.view_state();
                        if let Some(book) = view.books().iter().find(|b| &b.id == id) {
                            if !book.file_path.is_empty() {
                                sh.open_comic(Path::new(&book.file_path));
                            }
                        }
                    }
                });
        }
        // The Files grid: the selection feeds the status panel.
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .folders_view
                .connect_selection_changed(move |_| {
                    if let Some(sh) = state.upgrade() {
                        sh.sync_enabled();
                    }
                });
        }
        // The Files grid right-click: the folder view's own menu
        // (Open / Reveal / Remove — the `IRemoveBooks` shape: the
        // FILES go to the recycle bin; the port's is-file guard
        // applies, the C# trashes the folder path for folder comics).
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .folders_view
                .connect_context(move |id, x, y| {
                    show_folder_context_menu(&state, id, x, y);
                });
        }

        // The window-activation focus: the browser page grabs the
        // ItemView, the reader page its PageView (the Phase 3
        // dead-first-keypress fix, now per view).
        {
            let state = Rc::downgrade(state);
            self.window
                .connect_notify_local(Some("is-active"), move |win, _| {
                    if !win.is_active() {
                        return;
                    }
                    if let Some(sh) = state.upgrade() {
                        if sh.stack.visible_child_name().as_deref() == Some("reader") {
                            crate::trace::trace("is-active: re-grab reader focus");
                            sh.reader.focus_current();
                        } else {
                            crate::trace::trace("is-active: re-grab item-view focus");
                            sh.item_view.grab_focus();
                        }
                    }
                });
        }

        // Closing the main window: dock the undocked reader back and
        // save (`MainFormFormClosed` → `CleanUp`; the C# exit also
        // stores `Settings.QuickOpenThumbnailSize` and saves the
        // settings file).
        {
            let state = Rc::downgrade(state);
            let barrier = Rc::new(RefCell::new(
                cr_engine::incoming_transaction::CloseBarrier::default(),
            ));
            self.window.connect_close_request(move |window| {
                let decision = barrier.borrow_mut().request();
                if decision == cr_engine::incoming_transaction::CloseDecision::Proceed {
                    return glib::Propagation::Proceed;
                }
                if decision == cr_engine::incoming_transaction::CloseDecision::Stop {
                    return glib::Propagation::Stop;
                }
                library::abort_scan();
                let state = state.clone();
                let barrier = Rc::clone(&barrier);
                let window = window.clone();
                glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
                    if cr_engine::incoming_transaction::operation_active() {
                        return glib::ControlFlow::Continue;
                    }
                    let Some(guard) = cr_engine::incoming_transaction::try_acquire_mutation_guard()
                    else {
                        return glib::ControlFlow::Continue;
                    };
                    if let Some(sh) = state.upgrade() {
                        prepare_close(&sh);
                    }
                    drop(guard);
                    flush_lists_and_finish_close(window.clone(), Rc::clone(&barrier));
                    glib::ControlFlow::Break
                });
                glib::Propagation::Stop
            });
        }

        // The view commands (mode/size/sort/group).
        self.install_actions();

        // The status bar's clicks: the page panel toggles
        // TrackCurrentPage (the C# `tsCurrentPage_Click` — the
        // stateful action flips the setting and re-syncs); the lamps
        // open the Tasks dialog (T13; the disabled stub no-ops).
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .status_bar
                .connect_page_click(move || {
                    if let Some(sh) = state.upgrade() {
                        // The disabled stub would return Err — the
                        // activation is best-effort by design.
                        let _ = gtk4::prelude::WidgetExt::activate_action(
                            &sh.window,
                            "win.track-current-page",
                            None,
                        );
                    }
                });
        }
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .status_bar
                .connect_lamp_click(move || {
                    if let Some(sh) = state.upgrade() {
                        // The Tasks dialog is a disabled stub until
                        // T13 — the activation is best-effort.
                        let _ = gtk4::prelude::WidgetExt::activate_action(
                            &sh.window,
                            "win.tasks",
                            None,
                        );
                    }
                });
        }
        // The scan lamp's "Cancel scan" row (the C# aborts through
        // the Tasks dialog's scan row — `Scanner.Stop(clearQueue)`).
        state.status_bar.connect_cancel_scan(library::abort_scan);
        state
            .status_bar
            .connect_cancel_cv_job(library::abort_cv_job);
        state
            .status_bar
            .connect_cancel_remove(library::abort_remove_books);
        // "Skip current file": abandon the file in flight, keep the
        // scan running (the manual escape hatch; the scanner's own
        // per-file deadline is the unattended path).
        state
            .status_bar
            .connect_skip_scan_file(library::skip_current_scan_file);
        // The slider drag → `SetItemSize` (the C# `TrackBar.Scroll`
        // routes to the ACTIVE browser's view).
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .status_bar
                .connect_slider(move |value| {
                    if let Some(sh) = state.upgrade() {
                        if sh.stack.visible_child_name().as_deref() == Some("folders") {
                            sh.folders_view.set_item_size(value);
                        } else {
                            sh.item_view.set_item_size(value);
                            sh.mark_view_config_dirty();
                        }
                    }
                });
        }
        // The 1 s activity poll (`updateActivityTimer`): the lamps
        // follow the scan/write/export/page activity.
        {
            let state = Rc::downgrade(state);
            status_bar::start_activity_timer(move || {
                if let Some(sh) = state.upgrade() {
                    sh.status_bar.update_lamps(
                        library::is_scanning(),
                        library::writes_pending() > 0,
                        library::export_in_flight(),
                        library::cv_job_active(),
                        sh.pool.is_working(),
                        library::remove_books_in_flight(),
                    );
                    sh.status_bar
                        .set_cv_job_text(library::cv_job().map(|j| j.text()).as_deref());
                }
            });
        }

        // The scan pump's per-batch view refresh: books appear in the
        // Library as the scan walks (the C# scan events update the
        // live view per file — `ComicBookCollection.Add` →
        // `OnBookAdded`). An EMPTY slice = the landing (the full
        // refresh); a batch = the incremental append (the per-book
        // caches survive — a full set_books rebuild every tick was
        // the "glitching" report). A Weak capture — the hook never
        // owns the shell.
        library::set_scan_view_hook(Some(Box::new({
            let state = Rc::downgrade(state);
            move |batch: &[ComicBook]| {
                if let Some(sh) = state.upgrade() {
                    if batch.is_empty() {
                        // The landing (the merge is complete): one full
                        // refresh.
                        sh.refresh_view_from_list();
                    } else {
                        // The incremental append (the Library root only).
                        // A non-Library view deliberately skips the
                        // per-tick full refresh — a re-evaluation every
                        // 100 ms tick was a refresh storm; the landing
                        // hook does the one refresh.
                        sh.append_scan_batch(batch);
                    }
                    sh.sync_enabled();
                }
            }
        })));
        library::set_incoming_gap_view_hook(Some(Box::new({
            let state = Rc::downgrade(state);
            move || {
                if let Some(sh) = state.upgrade() {
                    if sh.is_incoming_view() {
                        sh.refresh_view_from_list();
                    }
                }
            }
        })));
        library::refresh_incoming_external_gaps_async();

        // The gauge row applier: after the gauge pass writes a list's
        // counters, the row label rebuilds in place. A Weak capture —
        // the hook never owns the shell.
        crate::gauges::set_row_hook(Some(Box::new({
            let state = Rc::downgrade(state);
            move |id: &CrGuid| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let Some(item) = library::find_list_item_any(id) else {
                    return;
                };
                let (gauges_on, flags) =
                    super::navigator::gauge_settings_for(&library::settings().borrow());
                let markup = super::navigator::row_markup(
                    item.base().name.as_deref().unwrap_or_default(),
                    item.base(),
                    gauges_on,
                    flags,
                );
                sh.navigator.update_row_label(id, &markup);
            }
        })));

        // The initial fill. The startup view: the BROWSER (the C#
        // `books.OpenCount == 0 && !ShowQuickOpen` shape,
        // MainForm.cs:3140) — the user decision 2026-09-07: the app
        // opens on the Library view, the QuickOpen covers show only
        // through the last-tab-close path (`UpdateQuickList`).
        state.navigator.refill(&library::comic_lists_snapshot());
        // The gauge pass (the startup cache retrieval + recompute —
        // the C# marks lists whose persisted NewBookCountDate is stale
        // and rebuilds their counters on the background queue).
        crate::gauges::invalidate();
        // The Files view boot (the C# ComicListFolderFilesBrowser
        // OnLoad): the tab drops under `DisableFoldersView`, the
        // saved include-sub state + the last folder restore.
        state.tab_strip.set_folders_visible(
            !cr_core::settings::ExtendedSettings::global().disable_folders_view,
        );
        // The include read hoists BEFORE the set — set_active fires
        // the toggled handler synchronously, and the handler borrows
        // the settings mutably (the edition-2021 temporaries lesson).
        let include = cr_ui_settings().borrow().explorer_include_sub_folders;
        state.folders_tree.set_include_sub(include);
        {
            let last = cr_ui_settings().borrow().last_explorer_folder.clone();
            if !last.is_empty() {
                state.folders_tree.drill_to(&last);
            }
        }
        // `UpdateSettings` applies the stored QuickOpen thumbnail size.
        {
            let size = cr_ui_settings().borrow().quick_open_thumbnail_size as f64;
            state.quick_view.configure(|c| c.thumb_height = size);
            let _ = &size;
        }
        state.show_browser();
        // The strip renders its startup state with the sync.
        state.sync_enabled();
    }

    fn install_actions(&self) {
        ShellState::install_commands(&self.state);
        ShellState::install_menubar_keys(&self.state);
        ShellState::install_dyn_fills(&self.state);
    }

    /// Opens a comic into the docked reader (the app's `open_reader`
    /// path).
    pub fn open_comic(&self, path: &Path) {
        self.state.open_comic(path);
    }

    /// The command-line/open-signal open (`OpenSupportedFile`):
    /// `new_slot` forces a fresh reader tab, `page` (0-based) opens
    /// there instead of the resume position.
    pub fn open_comic_page(&self, path: &Path, new_slot: bool, page: i32) {
        self.state.open_comic_at(path, new_slot, page);
    }

    /// The open books' file paths (`books.OpenFiles` — the exit-time
    /// `Settings.LastOpenFiles` source).
    pub fn reader_open_files(&self) -> Vec<String> {
        self.state.reader.open_files()
    }

    /// The open book count (`books.OpenCount` — the reopen-last
    /// gate).
    pub fn reader_open_book_count(&self) -> usize {
        self.state.reader.open_book_count()
    }

    pub fn present(&self) {
        self.window.present();
    }

    // ----- probe accessors (headless gates; not app paths) -----

    /// The open reader tab count.
    pub fn state_reader_tab_count(&self) -> usize {
        self.state.reader.tab_count()
    }

    /// The current reader slot id.
    pub fn state_reader_slot(&self) -> Option<usize> {
        self.state.reader.current_slot_id()
    }

    /// The slot of the FIRST open tab (the Open Books first row).
    pub fn state_first_open_slot(&self) -> Option<usize> {
        self.state.reader.open_tabs().first().map(|(s, _)| *s)
    }

    /// Whether the CURRENT reader page carries a bookmark.
    pub fn state_current_page_bookmark(&self) -> bool {
        self.state.current_page_has_bookmark()
    }

    /// Selects the current view's first book (the rating path).
    pub fn state_select_first_book(&self) {
        let view = self.state.item_view.view_state();
        if let Some(first) = view.books().first() {
            self.state.item_view.select_book(&first.id);
            self.state.sync_enabled();
        }
    }

    /// Probe seam: selects a navigator list through the REAL
    /// selection path (select_list → selection-changed → the debounced
    /// select handler).
    pub fn state_select_list(&self, id: &CrGuid) {
        self.state.navigator.select_list(id);
    }

    /// Probe: whether the Missing Issues scope/Refresh bar is visible
    /// (Phase 19 — shown only while that navigator node is selected).
    pub fn state_missing_issues_bar_visible(&self) -> bool {
        self.state.missing_issues_bar.widget().is_visible()
    }

    /// Probe: picks the Missing Issues scope combo (`None` = Whole
    /// Library, `Some(id)` = the named smart list).
    pub fn state_missing_issues_set_scope(&self, id: Option<CrGuid>) {
        self.state.missing_issues_bar.set_scope(id);
    }

    /// Probe: fires the Missing Issues Refresh button.
    pub fn state_missing_issues_refresh(&self) {
        self.state.missing_issues_bar.click_refresh();
    }

    pub fn state_select_books(&self, ids: &[CrGuid]) {
        self.state.item_view.reselect(ids);
    }

    pub fn state_selection_len(&self) -> usize {
        self.state.item_view.selection_len()
    }

    pub fn state_find_in_incoming(&self) {
        self.state.find_in_incoming();
    }

    pub fn state_run_incoming_adoption(&self, preview: bool) {
        self.state.run_incoming_menu_adoption(preview);
    }

    /// Probe: the Missing Issues bar's status text.
    pub fn state_missing_issues_status(&self) -> String {
        self.state.missing_issues_bar.status_text()
    }

    /// Fires a detailed action on the window (the dispatch path).
    pub fn state_dispatch(&self, action: &str) -> bool {
        gtk4::prelude::WidgetExt::activate_action(&self.window, action, None).is_ok()
    }

    /// Fires an action with an explicit string parameter (bare name
    /// + variant).
    pub fn state_dispatch_param(&self, action: &str, value: &str) -> bool {
        gtk4::prelude::WidgetExt::activate_action(&self.window, action, Some(&value.to_variant()))
            .is_ok()
    }

    /// The state of a rating check action.
    pub fn state_rating_checked(&self, n: u32) -> bool {
        self.state
            .action(&format!("rating-{n}"))
            .and_then(|a| a.state())
            .and_then(|v| v.get::<bool>())
            .unwrap_or(false)
    }

    /// Whether an action is enabled (the probe).
    pub fn state_action_enabled(&self, name: &str) -> bool {
        self.state.action(name).is_some_and(|a| a.is_enabled())
    }

    /// The grid's book count (the probe).
    pub fn state_grid_book_count(&self) -> usize {
        self.state.item_view.book_count()
    }

    /// Probe: the group count + the collapsed count (the grouping
    /// gates).
    pub fn state_grid_groups(&self) -> (usize, usize) {
        let groups = self.state.item_view.group_count();
        let collapsed = self.state.item_view.collapsed_count();
        (groups, collapsed)
    }

    /// Probe: the per-group TRUE counts (the collapsed-header count
    /// gate).
    pub fn state_group_counts(&self) -> Vec<usize> {
        self.state.item_view.group_counts()
    }

    /// Probe: a REAL group-header press through the click path
    /// (the group-collapse crash gate).
    pub fn state_group_press(&self, n: u32, x: f64, y: f64) -> bool {
        self.state.item_view.probe_group_press(n, x, y)
    }

    /// Probe: a REAL grid press through the click path (the n=2
    /// double-click press runs the activate — the open-crash gate).
    pub fn state_item_press(&self, n: u32, x: f64, y: f64) {
        self.state.item_view.probe_press(n, x, y)
    }

    /// Probe: the center of one placed item's rect.
    pub fn state_item_center(&self, display: usize) -> Option<(f64, f64)> {
        self.state.item_view.probe_item_center(display)
    }

    /// Probe: the center of one book's placed rect by id.
    pub fn state_book_center(&self, id: &CrGuid) -> Option<(f64, f64)> {
        self.state.item_view.probe_book_center(id)
    }

    /// Probe: the read state of one book in the grid's copy (the
    /// reader-hook push gate).
    pub fn state_grid_book_read_state(&self, id: &CrGuid) -> Option<(i32, i32)> {
        self.state.item_view.probe_book_read_state(id)
    }

    /// Probe: the recorded arrow zone of one group header.
    pub fn state_group_arrow_zone(&self, group: usize) -> (f64, f64, f64, f64) {
        self.state.item_view.probe_group_arrow_zone(group)
    }

    /// Probe: the grid's selection length.
    pub fn state_grid_selection_len(&self) -> usize {
        self.state.item_view.selection_len()
    }

    /// The grid's selection ids (the probe).
    pub fn state_grid_selection_ids(&self) -> Vec<CrGuid> {
        self.state.item_view.selection_ids()
    }

    /// Sets the grid selection to the ids (the probe; the `reselect`
    /// intersect path).
    pub fn state_reselect(&self, ids: &[CrGuid]) {
        self.state.item_view.reselect(ids);
    }

    /// Opens Incoming Compare through the same handler as the context menu.
    pub fn state_compare_incoming(&self) {
        self.state.compare_incoming();
    }

    /// The Detail column drag, driven through the real paths (the
    /// T5 probe: begin / move / end).
    pub fn state_column_resize_start(&self, id: i32, x: f64) -> bool {
        self.state.item_view.probe_column_resize_start(id, x)
    }

    pub fn state_column_resize_move(&self, x: f64) -> f64 {
        self.state.item_view.probe_column_resize_move(x)
    }

    pub fn state_column_resize_end(&self) -> f64 {
        self.state.item_view.probe_column_resize_end()
    }

    /// The double-click auto-size (the C# `AutoSizeHeader`).
    pub fn state_column_autosize(&self, id: i32) -> f64 {
        self.state.item_view.autosize_column(id)
    }

    /// The persisted Detail column widths (id, visible, width) — the
    /// T5 probe's resize round-trip.
    pub fn state_column_widths(&self) -> Vec<(i32, bool, i32)> {
        self.state.item_view.detail_columns_state()
    }

    /// The visible stack page (the Folders probe's tab gate).
    pub fn state_folders_page(&self) -> Option<String> {
        self.state.stack.visible_child_name().map(|s| s.to_string())
    }

    /// The Files view handle (the probe drives the real tree paths).
    pub fn state_folders_tree(&self) -> Rc<super::folder_tree::FolderTree> {
        Rc::clone(&self.state.folders_tree)
    }

    /// The Files grid's book count.
    pub fn state_folders_book_count(&self) -> usize {
        self.state.folders_view.book_count()
    }

    /// The Files grid's selection length.
    pub fn state_folders_selection_len(&self) -> usize {
        self.state.folders_view.selection_len()
    }

    /// The Files panel's toolbar button by name (the probe's
    /// real-click path).
    pub fn state_folders_button(&self, name: &str) -> Option<Button> {
        self.state.folders_tree.button(name)
    }

    /// The saved favorite folders (the probe).
    pub fn state_favorite_folders(&self) -> Vec<String> {
        library::settings().borrow().favorite_folders.clone()
    }

    /// The workspace tab strip handle (the probe's tab gates).
    pub fn tab_strip_handle(&self) -> super::tabstrip::TabStrip {
        self.state.tab_strip.clone()
    }

    /// The Files grid's view state (the probe's caption reads).
    pub fn state_folders_view_state(&self) -> super::view_state::ViewState {
        self.state.folders_view.view_state()
    }

    /// The Files page's split shape (the probe): (grid's parent is
    /// the paned, orientation is Horizontal = side by side).
    pub fn state_folders_split(&self) -> (bool, bool) {
        let paned = self
            .state
            .folders_view
            .grid_widget()
            .ancestor(gtk4::Paned::static_type());
        let in_paned = paned.is_some();
        let horizontal = paned
            .and_downcast::<gtk4::Paned>()
            .map(|p| p.orientation() == gtk4::Orientation::Horizontal)
            .unwrap_or(false);
        (in_paned, horizontal)
    }

    /// The visible workspace stack page name (the probe).
    pub fn state_stack_page(&self) -> String {
        self.state
            .stack
            .visible_child_name()
            .map(|n| n.to_string())
            .unwrap_or_default()
    }

    /// The current page image's size via `create_page_image` (the
    /// T6 probe).
    pub fn state_page_image_size(&self) -> Option<(i32, i32)> {
        self.state
            .reader
            .current_view()
            .and_then(|v| v.create_page_image())
            .map(|s| (s.width(), s.height()))
    }

    /// Scrolls the book grid (the context-menu probe).
    pub fn state_item_scroll_to(&self, y: f64) -> f64 {
        self.state.item_view.probe_scroll_to(y)
    }

    /// The book grid's live scroll value (the context-menu probe).
    pub fn state_item_scroll_value(&self) -> f64 {
        self.state.item_view.probe_scroll_value()
    }

    /// Fires the right-click hook through the shared gesture body
    /// (the context-menu probe).
    pub fn state_trigger_context(&self, x: f64, y: f64) {
        self.state.item_view.probe_context(x, y);
    }

    /// The post-scan problem summary (the app-level navigator
    /// commands reach it; the shell-internal actions call it
    /// directly).
    pub fn state_report_scan_problems(&self) {
        self.state.report_scan_problems();
    }

    /// "Reset View Settings" (ADR-039): the list drops its own
    /// `<Display><View>` and inherits again. The view on screen does
    /// NOT change — there is nothing to fall back to, and the C#
    /// applies nothing for a null config either.
    pub fn state_reset_list_view_config(&self, id: &CrGuid) -> bool {
        let cleared = if library::is_incoming_custom_list(id) {
            let window = self.window.clone();
            library::set_incoming_list_view_config(id, None, move |result| {
                if let Err(error) = result {
                    show_failure_dialog(&window, "Save Incoming Smart List", &error);
                }
            })
        } else {
            library::set_list_view_config(id, None)
        };
        // The reset must not be undone by the pending-change store
        // when this list leaves.
        if *self.state.current_list.borrow() == Some(*id) {
            self.state.view_config_dirty.set(false);
        }
        cleared
    }

    /// Whether the list carries its own view settings (the context
    /// menu's row gate and the probe).
    pub fn state_has_own_view_config(&self, id: &CrGuid) -> bool {
        if library::is_incoming_custom_list(id) {
            library::incoming_list_view_config(id).is_some()
        } else {
            library::list_view_config(id).is_some()
        }
    }

    /// The selected book's rating in the library (the probe).
    pub fn state_selected_book_rating(&self) -> f32 {
        let ids = self.state.item_view.selection_ids();
        if ids.is_empty() {
            return f32::NAN;
        }
        let lib = library::session();
        let l = lib.borrow();
        l.database()
            .books
            .iter()
            .find(|b| ids.contains(&b.id))
            .map(|b| b.rating)
            .unwrap_or(f32::NAN)
    }

    /// The current reader page's type (the probe).
    pub fn state_current_page_type(&self) -> Option<i16> {
        let book = self.state.reader.current_comic_book()?;
        let display = self.state.reader.current_display_page()?;
        let provider = self.state.reader.provider_index_of_display(display)?;
        book.info.pages.get(provider).map(|p| p.page_type.0 as i16)
    }

    /// The reader toolbar handle (the probe).
    pub fn toolbar_widget(&self) -> gtk4::Widget {
        self.state.toolbar.widget().clone().upcast()
    }

    /// The browser toolbar dropdown by name (the probe).
    pub fn browserbar_dropdown(&self, name: &str) -> Option<crate::browser::menubar::Dropdown> {
        self.state.browser_toolbar.dropdown(name)
    }

    /// Opens a browser toolbar dropdown through its real anchor.
    pub fn browserbar_open_dropdown(&self, name: &str) -> bool {
        self.state.browser_toolbar.open_dropdown(name)
    }

    /// Whether a browser toolbar dropdown's popover is mapped.
    pub fn browserbar_drop_mapped(&self, name: &str) -> bool {
        self.state.browser_toolbar.drop_mapped(name)
    }

    /// Closes one browser toolbar dropdown.
    pub fn browserbar_close_dropdown(&self, name: &str) {
        self.state.browser_toolbar.close_dropdown(name);
    }

    /// A stateful action's string state (the probe).
    pub fn state_action_string(&self, name: &str) -> Option<String> {
        self.state
            .action(name)
            .and_then(|a| a.state())
            .and_then(|v| v.get::<String>())
    }

    /// A stateful action's bool state (the probe).
    pub fn state_action_bool(&self, name: &str) -> Option<bool> {
        self.state
            .action(name)
            .and_then(|a| a.state())
            .and_then(|v| v.get::<bool>())
    }

    /// The search box cue text (the scope probe).
    pub fn state_search_placeholder(&self) -> String {
        self.state
            .search
            .placeholder_text()
            .unwrap_or_default()
            .to_string()
    }

    /// The Detail column snapshot (id, name, visible).
    pub fn state_columns_snapshot(&self) -> Vec<(i32, String, bool)> {
        self.state.item_view.detail_columns_snapshot()
    }

    /// The visible stack page name (the probe).
    pub fn state_visible_page(&self) -> Option<String> {
        self.state.stack.visible_child_name().map(|s| s.to_string())
    }

    /// The Tasks dialog's single-instance visibility (the probe).
    pub fn state_tasks_window_visible(&self) -> bool {
        self.state
            .tasks_window
            .borrow()
            .as_ref()
            .is_some_and(|w| w.is_visible())
    }

    /// The current reader zoom (the Custom Zoom gate).
    pub fn state_current_zoom(&self) -> Option<f32> {
        self.state.reader.current_zoom()
    }

    /// Opens the column chooser through the real hook path (the
    /// probe's OPEN gate).
    pub fn state_open_column_chooser(&self, wx: f64, wy: f64) -> bool {
        self.state.popup_column_chooser(wx, wy);
        self.state
            .columns_drop
            .borrow()
            .as_ref()
            .is_some_and(|p| p.is_mapped())
    }

    /// The book context menu's popover (the probe's arrow gate;
    /// fire `state_trigger_context` first).
    pub fn state_context_popover(&self) -> Option<gtk4::Popover> {
        self.state.context_drop.borrow().clone()
    }

    /// The book context menu OPEN gate: fires the right-click hook
    /// and reports whether the popover mapped.
    pub fn state_open_context(&self, x: f64, y: f64) -> bool {
        self.state_trigger_context(x, y);
        self.state_context_popover().is_some_and(|p| p.is_mapped())
    }

    /// The chooser popover's child natural height (the probe: the
    /// list must be taller than a couple of rows — the "two lines
    /// high" report).
    pub fn state_column_chooser_height(&self) -> i32 {
        self.state
            .columns_drop
            .borrow()
            .as_ref()
            .and_then(|p| p.child())
            .map(|c| c.measure(gtk4::Orientation::Vertical, -1).1)
            .unwrap_or(0)
    }

    /// The rendered chooser row by caption (the probe's REAL-click
    /// path): walks the mapped popover for the `GtkModelButton` whose
    /// label matches AND which is MAPPED — that is the row the user
    /// looks at. Rows on the other (hidden) submenu pages stay out,
    /// because a click cannot reach them.
    pub fn state_column_chooser_row(&self, caption: &str) -> Option<gtk4::Widget> {
        use gtk4::prelude::*;
        let columns_drop = self.state.columns_drop.borrow();
        let popover = columns_drop.as_ref()?;
        fn walk(w: &gtk4::Widget, caption: &str) -> Option<gtk4::Widget> {
            if w.widget_name() == "GtkModelButton" && w.is_mapped() {
                let mut c = w.first_child();
                while let Some(cur) = c {
                    if let Ok(l) = cur.clone().downcast::<gtk4::Label>() {
                        if l.text() == caption {
                            return Some(w.clone());
                        }
                    }
                    c = cur.next_sibling();
                }
            }
            let mut c = w.first_child();
            while let Some(cur) = c {
                if let Some(hit) = walk(&cur, caption) {
                    return Some(hit);
                }
                c = cur.next_sibling();
            }
            None
        }
        popover.child().and_then(|child| walk(&child, caption))
    }

    /// The mapped popover's widget tree dump (the probe's row-walk
    /// debugging seam).
    pub fn state_column_chooser_dump(&self) -> String {
        use gtk4::prelude::*;
        let columns_drop = self.state.columns_drop.borrow();
        let Some(popover) = columns_drop.as_ref() else {
            return "no popover".into();
        };
        fn dump(w: &gtk4::Widget, depth: usize, out: &mut String) {
            for _ in 0..depth {
                out.push_str("  ");
            }
            out.push_str(w.widget_name().as_ref());
            if let Ok(l) = w.clone().downcast::<gtk4::Label>() {
                out.push_str(&format!(" {:?}", l.text()));
            }
            out.push('\n');
            if depth > 8 {
                return;
            }
            let mut c = w.first_child();
            while let Some(cur) = c {
                dump(&cur, depth + 1, out);
                c = cur.next_sibling();
            }
        }
        let mut out = String::new();
        if let Some(child) = popover.child() {
            dump(&child, 0, &mut out);
        }
        out
    }

    /// Switches the open chooser to a submenu page (the
    /// `visible-submenu` property drives the popover's stack) and
    /// counts its menu rows — the probe gate for the EMPTY-submenu
    /// regression (a `custom`-attribute item with a submenu link
    /// never attached its custom page and the model page stayed
    /// empty).
    pub fn state_column_chooser_page_rows(&self, sub: &str) -> usize {
        let columns_drop = self.state.columns_drop.borrow();
        let Some(popover) = columns_drop.as_ref() else {
            return 0;
        };
        popover.set_property("visible-submenu", sub);
        // popover → content → viewport → stack
        let mut w = popover.first_child();
        while let Some(cur) = w {
            if cur.widget_name() == "GtkStack" {
                w = Some(cur);
                break;
            }
            w = cur.first_child();
        }
        let Some(stack) = w else {
            return 0;
        };
        let Ok(stack) = stack.downcast::<gtk4::Stack>() else {
            return 0;
        };
        let Some(page) = stack.child_by_name(sub) else {
            return 0;
        };
        fn walk(w: &gtk4::Widget, count: &mut usize) {
            if w.widget_name() == "GtkModelButton" {
                *count += 1;
            }
            let mut c = w.first_child();
            while let Some(ch) = c {
                walk(&ch, count);
                c = ch.next_sibling();
            }
        }
        let mut count = 0usize;
        walk(&page, &mut count);
        count
    }

    /// The browser toolbar's Group/Arrange label texts (the probe).
    pub fn browserbar_labels(&self) -> (String, String) {
        self.state.browser_toolbar.label_texts()
    }

    // --- T7 probe accessors (the navigator + Pages toolbars) ---

    /// Clicks a navigator toolbar button through the real handler.
    pub fn nav_click_button(&self, name: &str) -> bool {
        self.state.navigator.click_button(name)
    }

    /// Whether the navigator search box shows.
    pub fn nav_search_visible(&self) -> bool {
        self.state.navigator.search_visible()
    }

    /// Sets the navigator search text (the typing path — `set_text`
    /// fires the same changed signal).
    pub fn nav_set_search_text(&self, text: &str) {
        self.state.navigator.set_search_text(text);
    }

    /// The navigator tree row count (the filter evidence).
    pub fn nav_row_count(&self) -> usize {
        self.state.navigator.row_count()
    }

    /// The expanded navigator rows (the expand/collapse-all evidence).
    pub fn nav_expanded_count(&self) -> usize {
        self.state.navigator.expanded_count()
    }

    /// (name, expanded) per row (the probe's debugging seam).
    pub fn nav_expanded_dump(&self) -> Vec<(String, bool)> {
        self.state.navigator.expanded_dump()
    }

    /// The Pages grid mode.
    pub fn pages_mode(&self) -> super::pages_view::PagesMode {
        self.state.pages.mode()
    }

    /// Opens the Pages Views drop through its real anchor (the OPEN
    /// gate).
    pub fn pages_open_views(&self) -> bool {
        self.state.pages.open_views()
    }

    /// Closes the Pages Views drop (the probe cleanup).
    pub fn pages_close_views(&self) {
        self.state.pages.close_views();
    }

    /// Clicks a Pages Views radio row through the real handler.
    pub fn pages_click_view(&self, action: &str) -> bool {
        self.state.pages.click_view(action)
    }

    /// Clicks the Pages Views MAIN part (the mode cycle — the C#
    /// `tbbView_ButtonClick`).
    pub fn pages_click_main(&self) {
        self.state.pages.click_main();
    }

    /// Sets the search text through the composed-filter path (the
    /// probe; the entry typing itself is a user-test matter).
    pub fn state_set_search_text(&self, text: &str) {
        *self.state.search_text.borrow_mut() = text.to_string();
        self.state.rebuild_filter();
    }

    /// The toolbar's zoom state text (the probe).
    pub fn toolbar_zoom_text(&self) -> String {
        self.state.toolbar.zoom_text()
    }

    /// The toolbar's rotation state text (the probe).
    pub fn toolbar_rotate_label(&self) -> String {
        self.state.toolbar.rotate_text()
    }

    /// The toolbar dropdown by name (the probe).
    pub fn toolbar_dropdown(&self, name: &str) -> Option<crate::browser::menubar::Dropdown> {
        self.state.toolbar.dropdown(name)
    }

    /// Opens a toolbar dropdown through its real anchor (the probe).
    pub fn toolbar_open_dropdown(&self, name: &str) -> bool {
        self.state.toolbar.open_dropdown(name)
    }

    /// Closes one toolbar dropdown (the probe).
    pub fn toolbar_close_dropdown(&self, name: &str) {
        self.state.toolbar.close_dropdown(name);
    }

    /// Whether a toolbar dropdown's popover is mapped (the probe).
    pub fn toolbar_drop_mapped(&self, name: &str) -> bool {
        self.state
            .toolbar
            .dropdown(name)
            .is_some_and(|d| d.popover().is_mapped())
    }

    /// The current fit mode as the action name (the probe).
    pub fn reader_current_fit_name(&self) -> Option<&'static str> {
        self.state.reader.current_fit_mode().map(fit_action_name)
    }

    /// Sets a bookmark on a provider page of the CURRENT book,
    /// bypassing the prompt (the probe).
    pub fn state_set_bookmark_silent(&self, provider: usize, name: &str) {
        let book = self.state.reader.edit_current_book(|b| {
            if let Some(p) = b.info.pages.get_mut(provider) {
                p.bookmark = Some(name.to_string());
            }
        });
        if let Some(book) = book {
            library::apply_edited(&book);
        }
    }
}

impl ShellState {
    fn action(&self, name: &str) -> Option<gio::SimpleAction> {
        self.actions.borrow().get(name).cloned()
    }

    fn set_action_enabled(&self, name: &str, enabled: bool) {
        if let Some(a) = self.actions.borrow().get(name) {
            a.set_enabled(enabled);
        }
    }

    /// Registers one parameterless action with a `&ShellState`
    /// handler (the `CommandMapper.Add` one-command-one-handler
    /// shape).
    fn add_simple<F: Fn(&Rc<ShellState>) + 'static>(
        self: &Rc<ShellState>,
        group: &gio::SimpleActionGroup,
        name: &'static str,
        f: F,
    ) {
        let action = gio::SimpleAction::new(name, None);
        let state = Rc::downgrade(self);
        action.connect_activate(move |_, _| {
            crate::trace::trace(format!("action {name} activated"));
            if let Some(sh) = state.upgrade() {
                f(&sh);
                // The C# re-syncs the command enable/check states on
                // every menu operation (`CommandMapper` idle update);
                // every action dispatch refreshes ours.
                sh.sync_enabled();
            } else {
                crate::trace::trace(format!("action {name}: shell gone — silent no-op"));
            }
        });
        group.add_action(&action);
        self.actions.borrow_mut().insert(name, action);
    }

    /// The enable-state sync (`CommandMapper` idle update parity):
    /// reader commands need an open book, the edit commands a
    /// selection, Previous/Next List a walkable history. The
    /// radio/check actions take their state from the reader.
    fn sync_enabled(&self) {
        // The reader commands gate on the CURRENT slot's book (the
        // C# `ComicDisplay.Book != null` — an AddSlot slot stays
        // empty).
        let has_book = self.reader.has_current_book();
        let slots = self.reader.tab_count();
        let selected = self.item_view.selection_len();
        let incoming = self.is_incoming_view();
        let mutations_enabled = !cr_engine::incoming_transaction::operation_active();
        let library_selection = selected > 0 && !incoming && mutations_enabled;
        let (can_prev, can_next) = {
            let h = self.list_history.borrow();
            let pos = self.list_history_pos.get();
            (pos > 0, pos + 1 < h.len())
        };
        // Reader commands (`ComicDisplay.Book != null`).
        for name in [
            "close",
            "close-all",
            "first-page",
            "prev-page",
            "next-page",
            "last-page",
            "prev-bookmark",
            "next-bookmark",
            "last-page-read",
            "auto-scroll",
            "double-auto-scroll",
            "show-in-browser",
            "prev-book",
            "next-book",
            "random-book",
            "full-screen",
            "magnifier",
            "minimal-gui",
            "undock-reader",
            "copy-page",
            "export-page",
        ] {
            self.set_action_enabled(name, has_book);
        }
        self.set_action_enabled("prev-tab", slots > 1);
        self.set_action_enabled("next-tab", slots > 1);
        // Selection commands (`GetBookList(Selected)` non-empty).
        for name in [
            "info",
            "rating-0",
            "rating-1",
            "rating-2",
            "rating-3",
            "rating-4",
            "rating-5",
            "quick-rating",
        ] {
            self.set_action_enabled(name, library_selection);
        }
        for name in ["scrape-books", "organize-books", "organize-quick"] {
            self.set_action_enabled(name, library_selection);
        }
        self.set_action_enabled("organize-undo", mutations_enabled);
        for name in ["incoming-adopt", "incoming-discard"] {
            self.set_action_enabled(name, selected > 0 && incoming && mutations_enabled);
        }
        // Bookmark commands (`CanBookmark`/`CanNavigateBookmark`/
        // the current-page bookmark check).
        self.set_action_enabled("set-bookmark", has_book);
        self.set_action_enabled(
            "remove-bookmark",
            has_book && self.current_page_has_bookmark(),
        );
        self.set_action_enabled(
            "prev-bookmark",
            has_book && self.reader.can_navigate_bookmark(-1),
        );
        self.set_action_enabled(
            "next-bookmark",
            has_book && self.reader.can_navigate_bookmark(1),
        );
        // The My Rating check states (`Math.Round(GetRating()) == N`
        // — the selection's COMMON rating, -1 = mixed).
        let common = self.selection_common_rating();
        for (n, name) in [
            (0u32, "rating-0"),
            (1, "rating-1"),
            (2, "rating-2"),
            (3, "rating-3"),
            (4, "rating-4"),
            (5, "rating-5"),
        ] {
            if let Some(a) = self.action(name) {
                let checked = common >= 0.0 && common.round() == n as f32;
                a.set_state(&checked.to_variant());
            }
        }
        self.set_action_enabled("prev-list", can_prev);
        self.set_action_enabled("next-list", can_next);
        // The group expand/collapse command (`() =>
        // itemView.AreGroupsVisible` — a grouper is set).
        self.set_action_enabled("toggle-groups", self.item_view.has_groups());
        // migrate-paths lives only while Windows-style paths remain
        // (the Phase 8 T11 user decision).
        self.set_action_enabled("migrate-paths", library::has_windows_paths());
        // Radio/check state follows the reader (`IsPageFitBest`,
        // `IsPageSingle`, `RightToLeftReading` checks).
        if let Some(fit) = self.reader.current_fit_mode() {
            if let Some(a) = self.action("page-fit") {
                a.set_state(&fit_action_name(fit).to_variant());
            }
        }
        if let Some(layout) = self.reader.current_page_layout() {
            if let Some(a) = self.action("page-layout") {
                a.set_state(&layout_action_name(layout).to_variant());
            }
        }
        if let Some(rtl) = self.reader.current_rtl() {
            if let Some(a) = self.action("right-to-left") {
                a.set_state(&rtl.to_variant());
            }
        }
        // The check states (`CommandMapper` check lambdas):
        // The view-mode radio state follows the VIEW (the source of
        // truth — the T6 report: the Views check never moved).
        if let Some(a) = self.action("view-mode") {
            let name = match self.item_view.mode() {
                ItemViewMode::Thumbnail => "thumbnail",
                ItemViewMode::Tile => "tile",
                ItemViewMode::Detail => "detail",
            };
            a.set_state(&name.to_variant());
        }
        // `() => BrowserVisible`, `() => Program.Settings.AutoScrolling`
        // (the view mirrors it), `() => ComicDisplay.TwoPageNavigation`,
        // MinimalGui / FullScreen / Autorotate. `BrowserVisible` is
        // true on BOTH browser workspaces (the C# browser container
        // holds the Library and Pages views).
        if let Some(a) = self.action("toggle-browser") {
            let visible = matches!(
                self.stack.visible_child_name().as_deref(),
                Some("browser") | Some("pages")
            );
            a.set_state(&visible.to_variant());
        }
        if let Some(v) = self.reader.current_auto_scrolling() {
            if let Some(a) = self.action("auto-scroll") {
                a.set_state(&v.to_variant());
            }
        }
        if let Some(v) = self.reader.current_two_page_navigation() {
            if let Some(a) = self.action("double-auto-scroll") {
                a.set_state(&v.to_variant());
            }
        }
        if let Some(v) = self.reader.current_auto_rotate() {
            if let Some(a) = self.action("auto-rotate") {
                a.set_state(&v.to_variant());
            }
        }
        if let Some(a) = self.action("minimal-gui") {
            a.set_state(&self.reader.is_minimal_gui().to_variant());
        }
        if let Some(a) = self.action("full-screen") {
            a.set_state(&self.reader.is_fullscreen().to_variant());
        }
        // `tbShowMainMenu`: checked while the menu is NOT
        // auto-hidden.
        if let Some(a) = self.action("show-main-menu") {
            let checked = !cr_ui_settings().borrow().auto_hide_main_menu;
            a.set_state(&checked.to_variant());
        }
        // The navigator search toggle: the check = the box visibility
        // (`() => quickSearchPanel.Visible`).
        if let Some(a) = self.action("toggle-navigator-search") {
            a.set_state(&self.navigator.search_visible().to_variant());
        }
        // The Pages grid mode radio (the source of truth is the
        // panel — the main click cycles through the action).
        if let Some(a) = self.action("pages-view-mode") {
            a.set_state(&self.pages.mode().action_name().to_variant());
        }
        // `() => Program.Settings.TrackCurrentPage` — the page-panel
        // lock icon + the menu check derive from the setting.
        if let Some(a) = self.action("track-current-page") {
            a.set_state(&cr_ui_settings().borrow().track_current_page.to_variant());
        }
        // The workspace tab strip (tabs, selection, Pages visibility).
        self.sync_tabs();
        self.update_menubar();
        // The status panels ride the same sync (the `OnUpdateGui`
        // strip updates run in the same idle pass).
        self.update_status_panels();
        self.sync_menubar();
    }

    /// Applies the chrome visibility rules plus the T5 toolbar
    /// visibility. The MENUBAR shows ALWAYS in the normal windowed
    /// state (user decision 2026-09-05: the C# `AutoHideMainMenu`
    /// auto-hide and the Alt-alone reveal are not ported — the C#
    /// rule lives on in `menubar::menubar_visible` + its tests for
    /// the record); MinimalGui/fullscreen chrome still hide it.
    fn update_menubar(&self) {
        let minimal = self.reader.is_minimal_gui();
        let undocked = self.reader.is_undocked();
        let is_comic_viewer = self.stack.visible_child_name().as_deref() == Some("reader");
        let open_books = self.reader.open_book_count();
        let show_no_comic = cr_ui_settings().borrow().show_main_menu_no_comic_open;
        self.menubar.widget().set_visible(!minimal);
        // The tab strip + the status strip ride the Fill-mode `flag4`
        // (`OnGuiVisibilities`: `mainView.TabBarVisible` and
        // `statusStripVisibility.Visible` share it).
        let strip_visible = super::tabstrip::tabstrip_visible(
            minimal,
            undocked,
            is_comic_viewer,
            open_books,
            show_no_comic,
        );
        self.tab_strip.widget().set_visible(strip_visible);
        self.status_bar.widget().set_visible(strip_visible);
        // The toolbar: visible while the reader view shows and
        // MinimalGui is off (the C# `MainToolStripVisible`); the
        // reader-only buttons gate on the current book (`OnUpdateGui`).
        self.toolbar
            .sync_visibility(self.reader.has_current_book(), !minimal);
    }

    /// Pushes the current action states into the menubar rows and
    /// the toolbar dropdowns (check/radio marks + disabled graying +
    /// the hide rules — the custom bars have no model-driven state
    /// rendering).
    fn sync_menubar(&self) {
        let actions = self.actions.borrow();
        // The active-panel emphasis (the C# highlights the
        // miViewLibrary/miViewPages row of the shown workspace — no
        // checkbox on those items).
        let panel = match self.stack.visible_child_name().as_deref() {
            Some("browser") => "library",
            Some("pages") => "pages",
            _ => "",
        };
        // `fileMenu_DropDownOpening`: "Update all Book Files" hides
        // while `AutoUpdateComicsFiles` is on.
        let update_files_visible = !cr_ui_settings().borrow().auto_update_comics_files;
        let resolve = |base: &str| {
            let action = actions.get(base)?;
            let highlight = match base {
                "view-library" => panel == "library",
                "view-pages" => panel == "pages",
                _ => false,
            };
            let visible = match base {
                "update-book-files" => update_files_visible,
                _ => true,
            };
            // The dark-mode check derives from the ExtendedSettings
            // global (the source of truth — the T6 lesson: never
            // trust the click side to have updated the state).
            let state = if base == "dark-mode" {
                Some(
                    (cr_core::settings::ExtendedSettings::global().effective_theme()
                        == cr_core::settings::enums::Themes::Dark)
                        .to_variant(),
                )
            } else {
                action.state()
            };
            Some(super::menubar::ActionState {
                enabled: action.is_enabled(),
                state,
                highlight,
                visible,
            })
        };
        self.menubar.sync(&resolve);
        self.toolbar.sync(&resolve);
        // The Pages panel's Views drop (the T7 mode radios).
        self.pages.sync(&resolve);
        // The browser toolbar (the T6 strip): the enable states +
        // the Group/Arrange label texts (`OnIdle` tbbSort/tbbGroup).
        self.browser_toolbar.sync(&resolve);
        let (sort_col, sort_desc, grouper) = self.item_view.sort_group_summary();
        let sort_label = sort_col.and_then(|p| {
            default_columns()
                .iter()
                .find(|c| c.property == p)
                .map(|c| c.name.to_string())
        });
        self.browser_toolbar
            .sync_labels(sort_label, sort_desc, grouper);
        // The state text/icons (`viewer_PageDisplayModeChanged`).
        self.toolbar.sync_state(
            self.reader.current_zoom(),
            self.reader.current_rotation(),
            self.reader.current_fit_mode(),
            self.reader.current_page_layout(),
            self.reader.current_rtl(),
            self.reader.current_magnifier(),
            self.reader.current_auto_rotate(),
        );
        // The submenu PARENT enables (`OnGuiVisibilities` +
        // `DropDownOpening` rules).
        self.menubar
            .set_sub_enabled("Open Books", self.reader.tab_count() > 0);
        let recent_count = library::recent_books(20)
            .iter()
            .filter(|b| Path::new(&b.file_path).exists())
            .count();
        self.menubar
            .set_sub_enabled("Recent Books", recent_count > 0);
        let has_book = self.reader.has_current_book();
        self.menubar.set_sub_enabled("Page Type", has_book);
        self.menubar.set_sub_enabled("Page Rotation", has_book);
    }

    /// `OpenNextComic(relative)`: the neighbor book in the current
    /// list's view order; `relative == 0` picks a random book without
    /// repeats until the cycle wraps (`lastRandomList`/
    /// `randomSelectedComics` parity). The current book must be part
    /// of the viewed list — the C# resolves the book's own browser
    /// container, we use the active view.
    fn open_next_book(&self, relative: i32) {
        let Some(current) = self.reader.current_comic_book() else {
            return;
        };
        let view = self.item_view.view_state();
        let ids: Vec<CrGuid> = view
            .display_order()
            .iter()
            .map(|&i| view.books()[i].id)
            .collect();
        let Some(pos) = ids.iter().position(|id| *id == current.id) else {
            return;
        };
        let next = if relative == 0 {
            let reset = {
                let mut list = self.random_list.borrow_mut();
                if *list != ids {
                    *list = ids.clone();
                    self.random_picked.borrow_mut().clear();
                }
                let mut picked = self.random_picked.borrow_mut();
                if picked.len() >= ids.len() {
                    picked.clear();
                }
                let remaining: Vec<CrGuid> = ids
                    .iter()
                    .filter(|id| !picked.contains(id))
                    .copied()
                    .collect();
                // `new Random().Next(0, remaining)` — the C# uses an
                // unseeded Random; a time-seeded one matches the
                // behavior class.
                let seed = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos() as i32)
                    .unwrap_or(0);
                let choice =
                    remaining[cr_engine::sort::DotNetRandom::new(seed).next(remaining.len())];
                picked.push(choice);
                choice
            };
            Some(reset)
        } else {
            let idx = pos as i32 + relative;
            if idx >= 0 && (idx as usize) < ids.len() {
                Some(ids[idx as usize])
            } else {
                None
            }
        };
        if let Some(id) = next {
            if let Some(path) = library::book_path(&id) {
                self.open_comic(Path::new(&path));
            }
        }
    }

    /// Walks the list browsing history (Previous/Next List).
    fn browse_history(&self, dir: i32) {
        let next = self.list_history_pos.get() as i64 + dir as i64;
        let id = {
            let h = self.list_history.borrow();
            if next < 0 || next as usize >= h.len() {
                return;
            }
            h[next as usize]
        };
        self.list_history_pos.set(next as usize);
        self.navigator.select_list(&id);
        self.sync_enabled();
    }

    /// `ShowInfo` (Ctrl+I): the editor over the selection — the bulk
    /// editor for several books (`MultipleComicBooksDialog`), the
    /// book editor otherwise.
    fn show_info(self: &Rc<ShellState>) {
        let ids = self.item_view.selection_ids();
        if ids.is_empty() {
            return;
        }
        let books = Self::books_by_ids(&ids);
        if books.is_empty() {
            return;
        }
        if books.len() > 1 {
            self.open_bulk_editor(books);
        } else {
            self.open_editor(books);
        }
    }

    fn books_by_ids(ids: &[CrGuid]) -> Vec<ComicBook> {
        let lib = library::session();
        let l = lib.borrow();
        l.database()
            .books
            .iter()
            .filter(|b| ids.contains(&b.id))
            .cloned()
            .collect()
    }

    /// The editor commit: `apply_edited` (the library replace + the
    /// dirty mark + the debounced file write) and a grid refresh.
    fn editor_commit(self: &Rc<ShellState>) -> crate::dialogs::book_editor::CommitFn {
        let state = Rc::downgrade(self);
        Rc::new(move |edited| {
            library::apply_edited(edited);
            if let Some(sh) = state.upgrade() {
                sh.refresh_view_from_list();
            }
        })
    }

    fn open_editor(self: &Rc<ShellState>, books: Vec<ComicBook>) {
        let commit = self.editor_commit();
        // The editor shares the app pool (the C# `Program.ImagePool`
        // is global — a private pool would re-decode every cover).
        crate::dialogs::book_editor::show(&self.window, books, commit, Arc::clone(&self.pool));
    }

    /// `AddNewBook(showDialog: true)` (MainForm.cs:1879): a fresh
    /// fileless book (no file path, `AddedTime = now`, a new id)
    /// opens the book editor. The commit inserts it into the database
    /// on the first save point (the C# adds after the dialog's OK;
    /// the port inserts idempotently so later Apply commits degrade
    /// to `apply_edited`). Cancel closes without a commit.
    fn open_new_book_editor(self: &Rc<ShellState>) {
        let book = crate::dialogs::new_book_series::new_fileless_book();
        let state = Rc::downgrade(self);
        let commit: crate::dialogs::book_editor::CommitFn = Rc::new(move |edited| {
            let inserted = library::insert_new_book(edited);
            if !inserted {
                library::apply_edited(edited);
            }
            if let Some(sh) = state.upgrade() {
                sh.refresh_view_from_list();
            }
        });
        crate::dialogs::book_editor::show(&self.window, vec![book], commit, Arc::clone(&self.pool));
    }

    /// The NewComics.py port: the dialog creates N = to-from+1
    /// fileless books (`Number = str(n)`, the shared series/volume)
    /// and selects them (the script's `Browser.SelectComics`). A
    /// range over 100 aborts silently — the script's sanity check.
    fn new_book_series(self: &Rc<ShellState>) {
        let state = Rc::downgrade(self);
        crate::dialogs::new_book_series::show(&self.window, move |series, volume, first, last| {
            if last - first > 100 {
                return;
            }
            let mut ids = Vec::with_capacity((last - first + 1).max(0) as usize);
            for n in first..=last {
                let mut book = crate::dialogs::new_book_series::new_fileless_book();
                book.info.series = series.clone();
                book.info.number = n.to_string();
                book.info.volume = volume;
                library::insert_new_book(&book);
                ids.push(book.id);
            }
            if let Some(sh) = state.upgrade() {
                sh.refresh_view_from_list();
                sh.item_view.reselect(&ids);
            }
        });
    }

    /// "Fill Missing Issues" (Phase 15 T7): the Comic Vine cache
    /// skeleton knows every issue of the volume, so the difference
    /// against the issue numbers the library holds becomes fileless
    /// books. Each new book carries the series, the volume, the issue
    /// number, and the Comic Vine issue id, so a later scrape needs no
    /// search.
    ///
    /// A library series names a Comic Vine volume only after a scrape.
    /// With no id the command says so and stops, because a guess from
    /// the series name would pick the wrong volume.
    fn fill_missing_issues(self: &Rc<ShellState>) {
        let ids = self.item_view.selection_ids();
        if ids.is_empty() {
            return;
        }
        let config = library::scraper_config();
        let books: Vec<ComicBook> = {
            let lib = library::session();
            let l = lib.borrow();
            ids.iter()
                .filter_map(|id| l.database().books.iter().find(|b| b.id == *id).cloned())
                .collect()
        };
        if books.is_empty() {
            return;
        }
        // The series of the FIRST selected book decides the target.
        // The C# context commands all read the selection this way.
        let series = books[0].info.series.clone();
        let volume = books[0].info.volume;
        let of_series: Vec<&ComicBook> = books
            .iter()
            .filter(|b| b.info.series.eq_ignore_ascii_case(&series))
            .collect();
        let data: Vec<cr_scrape::bookdata::BookData> = of_series
            .iter()
            .map(|b| cr_scrape::bookdata::BookData::from_book(b, &config))
            .collect();
        let Some(volume_id) =
            cr_scrape::cache::missing::volume_id_of(data.iter().map(|d| d.series_key.clone()))
        else {
            show_failure_dialog(
                &self.window,
                "Fill Missing Issues",
                &format!(
                    "No book of \"{series}\" names a Comic Vine volume. Scrape one book of this series first, then run this command again."
                ),
            );
            return;
        };

        // Every book of this series in the LIBRARY, not only the
        // selection, decides what is owned.
        let owned_numbers: Vec<String> = {
            let lib = library::session();
            let l = lib.borrow();
            l.database()
                .books
                .iter()
                .filter(|b| b.info.series.eq_ignore_ascii_case(&series))
                .map(|b| b.info.number.clone())
                .collect()
        };

        let state = Rc::downgrade(self);
        crate::dialogs::missing_issues::show(
            &self.window,
            crate::dialogs::missing_issues::Request {
                series: series.clone(),
                volume,
                volume_id,
                owned_numbers,
                api_key: config.api_key.clone(),
            },
            Box::new(move |picked| {
                let mut new_ids = Vec::with_capacity(picked.len());
                for issue in picked {
                    let mut book = crate::dialogs::new_book_series::new_fileless_book();
                    book.info.series = series.clone();
                    book.info.number = issue.issue_number.clone();
                    book.info.volume = volume;
                    if let Some(name) = &issue.name {
                        book.info.title = name.clone();
                    }
                    // The Comic Vine issue id, so a later scrape
                    // resolves the book with no search.
                    cr_scrape::bookdata::set_custom_value(
                        &mut book,
                        "comicvine_issue",
                        &issue.issue_id.to_string(),
                    );
                    cr_scrape::bookdata::set_custom_value(
                        &mut book,
                        "comicvine_volume",
                        &volume_id.to_string(),
                    );
                    library::insert_new_book(&book);
                    new_ids.push(book.id);
                }
                if let Some(sh) = state.upgrade() {
                    sh.refresh_view_from_list();
                    sh.item_view.reselect(&new_ids);
                }
            }),
        );
    }

    /// "Link Series from Cache": the clicked book identifies the
    /// series; the candidate set is the CURRENT VIEW
    /// (`item_view.displayed_books()`), not the whole library, so a
    /// smart-list or quick-search scope narrows what gets linked. If
    /// any candidate already votes a Comic Vine volume, that vote is
    /// reused and no network call happens; otherwise one Comic Vine
    /// search finds the volume, the user confirms it
    /// (`dialogs::pick_series`), and every other match comes from the
    /// local cache alone — no further requests.
    fn link_series_from_cache(self: &Rc<ShellState>) {
        let ids = self.item_view.selection_ids();
        let Some(first_id) = ids.first() else {
            return;
        };
        let Some(clicked) = Self::books_by_ids(std::slice::from_ref(first_id))
            .into_iter()
            .next()
        else {
            return;
        };
        let series = clicked.info.series.clone();
        let volume = clicked.info.volume;

        let candidates: Vec<ComicBook> = self
            .item_view
            .displayed_books()
            .into_iter()
            .filter(|b| b.info.series.eq_ignore_ascii_case(&series) && b.info.volume == volume)
            .collect();
        if candidates.is_empty() {
            return;
        }

        let config = library::scraper_config();
        let existing = cr_scrape::cache::missing::volume_id_of(
            candidates
                .iter()
                .map(|b| cr_scrape::bookdata::BookData::from_book(b, &config).series_key),
        );
        if let Some(volume_id) = existing {
            self.link_series_apply(candidates, volume_id);
            return;
        }

        if !config.has_api_key() {
            let window = self.window.clone();
            let state = Rc::downgrade(self);
            crate::settings::preferences::show_preferences(&window, Some("scraper"), move || {
                if let Some(sh) = state.upgrade() {
                    sh.sync_enabled();
                }
            });
            return;
        }

        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let api_key = config.api_key.clone();
        let budget_config = config.clone();
        let search_terms = series.clone();
        let state = Rc::downgrade(self);
        self.run_cv_job(
            library::CvJobKind::LinkSeriesSearch,
            "Link Series from Cache",
            cancel,
            move |progress| -> Result<Vec<cr_scrape::cv::models::SeriesRef>, String> {
                let mut client = cr_scrape::cv::connection::CvClient::new(&api_key);
                if let Some(cache) = library::cv_cache() {
                    if let Some(budget) = library::cv_budget(
                        &budget_config,
                        Arc::clone(&cache),
                        Arc::clone(&worker_cancel),
                        Some(wait_reporter(progress.clone())),
                    ) {
                        client.set_budget(budget);
                    }
                }
                let mut cv = cr_scrape::cv::queries::Cv::new(client);
                cv.query_series_refs(&search_terms, &[], 25, &mut |_done, _total| {
                    worker_cancel.load(std::sync::atomic::Ordering::Relaxed)
                })
                .map_err(|e| e.to_string())
            },
            move |window, result| {
                let refs = match result {
                    Ok(refs) => refs,
                    Err(error) => {
                        show_failure_dialog(window, "Link Series from Cache", &error);
                        return;
                    }
                };
                if refs.is_empty() {
                    show_failure_dialog(
                        window,
                        "Link Series from Cache",
                        &format!("No Comic Vine series matched \"{series}\"."),
                    );
                    return;
                }
                let state = state.clone();
                let candidates = candidates.clone();
                crate::dialogs::pick_series::show(
                    window,
                    &series,
                    &refs,
                    Box::new(move |picked| {
                        if let (Some(picked), Some(sh)) = (picked, state.upgrade()) {
                            sh.link_series_apply(candidates.clone(), picked.series_key);
                        }
                    }),
                );
            },
        );
    }

    /// The apply step of "Link Series from Cache": a single cache-only
    /// read (`CvCache::issues_of_volume`, never the network) plus a
    /// pure local match (`cr_scrape::cache::link::match_series_to_volume`),
    /// then one `library::apply_edited` per matched book — the only
    /// persist primitive that exists (no batch variant), the same shape
    /// the bulk editor's commit loop already uses.
    fn link_series_apply(self: &Rc<ShellState>, mut candidates: Vec<ComicBook>, volume_id: i64) {
        let Some(cache) = library::cv_cache() else {
            show_failure_dialog(
                &self.window,
                "Link Series from Cache",
                "The Comic Vine cache file could not be opened.",
            );
            return;
        };
        let issues = match cache.issues_of_volume(volume_id) {
            Ok(issues) => issues,
            Err(error) => {
                show_failure_dialog(&self.window, "Link Series from Cache", &error.to_string());
                return;
            }
        };
        let (linked, unmatched) =
            cr_scrape::cache::link::match_series_to_volume(&candidates, &issues);
        let issue_by_book: HashMap<CrGuid, i64> =
            linked.iter().map(|l| (l.book_id, l.issue_id)).collect();
        let total = candidates.len();
        let mut linked_count = 0;
        for book in &mut candidates {
            let Some(&issue_id) = issue_by_book.get(&book.id) else {
                continue;
            };
            cr_scrape::bookdata::set_custom_value(book, "comicvine_volume", &volume_id.to_string());
            cr_scrape::bookdata::set_custom_value(book, "comicvine_issue", &issue_id.to_string());
            library::apply_edited(book);
            linked_count += 1;
        }
        self.refresh_view_from_list();
        show_report_dialog(
            &self.window,
            "Link Series from Cache",
            &format!(
                "{linked_count} of {total} book(s) linked. {unmatched} had no matching issue \
                 number in the cache."
            ),
        );
    }

    /// Runs one Comic Vine cache job on a worker thread, and makes it
    /// visible while it runs (ADR-037, ADR-038).
    ///
    /// The job publishes over a channel and the MAIN thread writes the
    /// shared state, because a worker that writes a thread-local
    /// writes its own copy. That is the shape the scan uses.
    ///
    /// The job claims the single cache-job slot. It is released on
    /// every exit, including a failure and a cancel.
    fn run_cv_job<T: Send + 'static>(
        self: &Rc<ShellState>,
        kind: library::CvJobKind,
        heading: &'static str,
        cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
        work: impl FnOnce(std::sync::mpsc::Sender<CvProgressMsg>) -> T + Send + 'static,
        finish: impl Fn(&ApplicationWindow, T) + 'static,
    ) {
        if !library::start_cv_job(kind, cancel) {
            show_failure_dialog(
                &self.window,
                heading,
                "A Comic Vine cache job is already running. Wait for it, or cancel it from the cache lamp in the status bar.",
            );
            return;
        }

        let (result_tx, result_rx) = std::sync::mpsc::channel::<T>();
        let (progress_tx, progress_rx) = std::sync::mpsc::channel::<CvProgressMsg>();
        std::thread::Builder::new()
            .name(format!("Comic Vine {kind:?}"))
            .spawn(move || {
                let outcome = work(progress_tx);
                let _ = result_tx.send(outcome);
            })
            .expect("spawn the Comic Vine cache worker");

        let report_window = self.window.clone();
        let state = Rc::downgrade(self);
        glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
            // Drain the progress first, so the last line the user sees
            // matches the work that landed.
            while let Ok(message) = progress_rx.try_recv() {
                match message {
                    CvProgressMsg::Step {
                        detail,
                        done,
                        total,
                    } => library::set_cv_job_progress(detail, done, total),
                    CvProgressMsg::Waiting {
                        resource,
                        resume_at,
                    } => library::set_cv_job_progress(
                        format!(
                            "the {resource} budget is spent, resuming at {}",
                            local_clock(resume_at)
                        ),
                        0,
                        0,
                    ),
                }
            }
            let outcome = match result_rx.try_recv() {
                Ok(outcome) => outcome,
                Err(std::sync::mpsc::TryRecvError::Empty) => return glib::ControlFlow::Continue,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    library::end_cv_job();
                    show_failure_dialog(
                        &report_window,
                        heading,
                        "The Comic Vine worker stopped without a result.",
                    );
                    return glib::ControlFlow::Break;
                }
            };
            library::end_cv_job();
            if let Some(sh) = state.upgrade() {
                sh.status_bar.update_lamps(
                    library::is_scanning(),
                    library::writes_pending() > 0,
                    library::export_in_flight(),
                    library::cv_job_active(),
                    sh.pool.is_working(),
                    library::remove_books_in_flight(),
                );
            }
            finish(&report_window, outcome);
            glib::ControlFlow::Break
        });
    }

    /// "Import Comic Vine MCL File…" (ADR-038): an `.mcl` snapshot
    /// seeds the cache skeleton with no API request. The read runs on
    /// a worker thread, because a full snapshot is large.
    fn import_cv_mcl(self: &Rc<ShellState>) {
        let Some(cache) = library::cv_cache() else {
            show_failure_dialog(
                &self.window,
                "Import Comic Vine MCL File",
                "The Comic Vine cache file could not be opened.",
            );
            return;
        };
        let state = Rc::downgrade(self);
        open_mcl_dialog(&self.window, move |path| {
            let Some(sh) = state.upgrade() else {
                return;
            };
            let path = path.to_string();
            let cache = std::sync::Arc::clone(&cache);
            let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            sh.run_cv_job(
                library::CvJobKind::Import,
                "Import Comic Vine MCL File",
                cancel,
                move |progress| -> Result<String, String> {
                    let file = std::fs::File::open(&path).map_err(|e| format!("{path}: {e}"))?;
                    let report = cr_scrape::cache::mcl::import_reporting(
                        cache.as_ref(),
                        std::io::BufReader::new(file),
                        |volumes, issues| {
                            let _ = progress.send(CvProgressMsg::Step {
                                detail: format!("{volumes} volumes, {issues} issues"),
                                done: volumes as i64,
                                total: 0,
                            });
                        },
                    )
                    .map_err(|e| e.to_string())?;
                    // The snapshot date becomes the start of the next
                    // incremental sweep (ADR-038).
                    if !report.date.trim().is_empty() {
                        let _ = cache.put_sweep_state(&cr_scrape::cache::SweepState {
                            start_date: report.date.clone(),
                            end_date: report.date.clone(),
                            offset: 0,
                            total: 0,
                            updated_at: chrono::Utc::now().timestamp(),
                        });
                    }
                    Ok(format!(
                        "{} volumes and {} issues loaded (snapshot date {}). {} lines skipped.",
                        report.volumes,
                        report.issues,
                        if report.date.is_empty() {
                            "unknown"
                        } else {
                            &report.date
                        },
                        report.skipped
                    ))
                },
                |window, outcome| match outcome {
                    Ok(text) => show_report_dialog(window, "Import Comic Vine MCL File", &text),
                    Err(reason) => {
                        show_failure_dialog(window, "Import Comic Vine MCL File", &reason)
                    }
                },
            );
        });
    }

    /// "Update Comic Vine Cache" (ADR-038): one paged sweep over the
    /// issues that changed since the last sweep, or since the MCL
    /// snapshot date. It keeps the skeleton current for far fewer
    /// requests than one revalidation per volume.
    fn update_cv_cache(self: &Rc<ShellState>) {
        let config = library::scraper_config();
        if !config.has_api_key() {
            show_failure_dialog(
                &self.window,
                "Update Comic Vine Cache",
                "No Comic Vine API key is set. Set it in Preferences ▸ Comic Vine Scraper.",
            );
            return;
        }
        let Some(cache) = library::cv_cache() else {
            show_failure_dialog(
                &self.window,
                "Update Comic Vine Cache",
                "The Comic Vine cache file could not be opened.",
            );
            return;
        };

        let today = chrono::Local::now().date_naive();
        let stored = cache.sweep_state().ok().flatten();
        // An unfinished sweep of the SAME window resumes. A finished
        // one, or an MCL import, starts a window at its end date.
        let options = match &stored {
            Some(state) if state.total > 0 && state.offset < state.total => {
                cr_scrape::cache::sweep::SweepOptions {
                    start_date: state.start_date.clone(),
                    end_date: state.end_date.clone(),
                    max_pages: None,
                }
            }
            Some(state) if !state.end_date.trim().is_empty() => {
                cr_scrape::cache::sweep::SweepOptions {
                    start_date: state.end_date.clone(),
                    end_date: today.format("%Y-%m-%d").to_string(),
                    max_pages: None,
                }
            }
            _ => {
                show_failure_dialog(
                    &self.window,
                    "Update Comic Vine Cache",
                    "The cache has no starting point. Import an MCL file first, so the sweep knows which date to start from.",
                );
                return;
            }
        };
        if options.start_date == options.end_date {
            show_report_dialog(
                &self.window,
                "Update Comic Vine Cache",
                "The cache is already current for today.",
            );
            return;
        }

        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let window_text = format!("{} to {}", options.start_date, options.end_date);
        let api_key = config.api_key.clone();
        let worker_cancel = std::sync::Arc::clone(&cancel);
        let budget_config = config.clone();
        self.run_cv_job(
            library::CvJobKind::Sweep,
            "Update Comic Vine Cache",
            cancel,
            move |progress| -> Result<cr_scrape::cache::sweep::SweepReport, String> {
                let mut client = cr_scrape::cv::connection::CvClient::new(&api_key);
                if let Some(budget) = library::cv_budget(
                    &budget_config,
                    std::sync::Arc::clone(&cache),
                    std::sync::Arc::clone(&worker_cancel),
                    Some(wait_reporter(progress.clone())),
                ) {
                    client.set_budget(budget);
                }
                cr_scrape::cache::sweep::run(
                    &client,
                    cache.as_ref(),
                    &options,
                    &worker_cancel,
                    |p| {
                        // `offset` counts issues; the page size is 100.
                        // `div_ceil` is unstable for signed integers.
                        let size = cr_scrape::cache::sweep::PAGE_SIZE;
                        let page = p.offset / size;
                        let pages = (p.total + size - 1) / size;
                        let _ = progress.send(CvProgressMsg::Step {
                            detail: format!("page {page} of {pages}"),
                            done: p.offset,
                            total: p.total,
                        });
                    },
                )
                .map_err(|e| e.to_string())
            },
            move |window, outcome| match outcome {
                Ok(report) => {
                    let tail = if report.complete {
                        "The window is complete."
                    } else {
                        "The run stopped early. Run the command again to continue."
                    };
                    show_report_dialog(
                        window,
                        "Update Comic Vine Cache",
                        &format!(
                            "{window_text}: {} pages, {} issues, {} volumes. {tail}",
                            report.pages, report.issues, report.volumes
                        ),
                    );
                }
                Err(reason) => show_failure_dialog(window, "Update Comic Vine Cache", &reason),
            },
        );
    }

    /// "Warm Comic Vine Cache" (ADR-037): spends the request budget
    /// on the volumes the library already names, so a later scrape
    /// reads from the cache. The run is capped and it stops the
    /// moment the budget refuses a request.
    fn warm_cv_cache(self: &Rc<ShellState>) {
        let config = library::scraper_config();
        if !config.has_api_key() {
            show_failure_dialog(
                &self.window,
                "Warm Comic Vine Cache",
                "No Comic Vine API key is set. Set it in Preferences ▸ Comic Vine Scraper.",
            );
            return;
        }
        let Some(cache) = library::cv_cache() else {
            show_failure_dialog(
                &self.window,
                "Warm Comic Vine Cache",
                "The Comic Vine cache file could not be opened.",
            );
            return;
        };
        let volume_ids = library::cv_volume_ids(&config);
        if volume_ids.is_empty() {
            show_failure_dialog(
                &self.window,
                "Warm Comic Vine Cache",
                "No book in the library names a Comic Vine volume. Scrape some books first.",
            );
            return;
        }

        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (_, _, warm_options) = cr_scrape::cache::policies_from(config.advanced());
        let api_key = config.api_key.clone();
        let worker_cancel = std::sync::Arc::clone(&cancel);
        let budget_config = config.clone();
        self.run_cv_job(
            library::CvJobKind::Warm,
            "Warm Comic Vine Cache",
            cancel,
            move |progress| {
                let mut client = cr_scrape::cv::connection::CvClient::new(&api_key);
                if let Some(budget) = library::cv_budget(
                    &budget_config,
                    std::sync::Arc::clone(&cache),
                    std::sync::Arc::clone(&worker_cancel),
                    Some(wait_reporter(progress.clone())),
                ) {
                    client.set_budget(budget);
                }
                cr_scrape::cache::warm::run(
                    &client,
                    cache.as_ref(),
                    &volume_ids,
                    &warm_options,
                    &worker_cancel,
                    |p| {
                        let _ = progress.send(CvProgressMsg::Step {
                            detail: format!(
                                "volume {} of {} ({} requests spent)",
                                p.done, p.total, p.requests
                            ),
                            done: p.done as i64,
                            total: p.total as i64,
                        });
                    },
                )
            },
            |window, report| {
                let stopped = if report.stopped_early {
                    " The run stopped early: the request budget, the cap, or a cancel stopped it."
                } else {
                    ""
                };
                show_report_dialog(
                    window,
                    "Warm Comic Vine Cache",
                    &format!(
                        "{} volumes looked at. {} read, {} already fresh, {} failed. {} requests spent.{stopped}",
                        report.considered,
                        report.warmed,
                        report.already_fresh,
                        report.failed,
                        report.requests
                    ),
                );
            },
        );
    }

    fn open_bulk_editor(self: &Rc<ShellState>, books: Vec<ComicBook>) {
        let commit = self.editor_commit();
        crate::dialogs::bulk_edit::show(&self.window, books, commit);
    }

    /// The Comic Vine Scraper config dialog (`cvs_config`): OK saves
    /// the settings into the unified config's plugin section.
    fn show_scrape_config(self: &Rc<ShellState>) {
        let config = library::scraper_config();
        let state = Rc::downgrade(self);
        crate::dialogs::scrape_config::show_scrape_config(&self.window, &config, move |result| {
            if let Some(config) = result {
                library::store_scraper_config(&config);
                if let Some(sh) = state.upgrade() {
                    sh.sync_enabled();
                }
            }
        });
    }

    /// The Library Organizer main entry (`LibraryOrganizer` hook):
    /// the config dialog, then the run over the selection.
    fn open_organize(self: &Rc<ShellState>) {
        if self.item_view.selection_ids().is_empty() {
            return;
        }
        let settings = library::organize_settings();
        let state = Rc::downgrade(self);
        crate::dialogs::organize_config::show_organize_config(
            &self.window,
            &settings,
            None,
            move |result| {
                if let Some(settings) = result {
                    library::store_organize_settings(&settings);
                    if let Some(sh) = state.upgrade() {
                        sh.launch_organize(&settings);
                    }
                }
            },
        );
    }

    /// The Library Organizer Quick entry (`LibraryOrganizerQuick`):
    /// straight to the profile selector and the run; the addon
    /// warns when the only profile has no base folder.
    fn open_organize_quick(self: &Rc<ShellState>) {
        let settings = library::organize_settings();
        if settings.profiles.len() == 1 && settings.profiles[0].base_folder.is_empty() {
            crate::browser::shell::show_report_dialog(
                &self.window,
                "BaseFolder empty",
                "Library Organizer will not work as expected when the BaseFolder is empty. Run the normal Library Organizer or the Configure dialog first.",
            );
            return;
        }
        self.launch_organize(&settings);
    }

    /// The configure-only entry (`ConfigureLibraryOrganizer` hook).
    fn show_organize_config(self: &Rc<ShellState>) {
        let settings = library::organize_settings();
        let state = Rc::downgrade(self);
        crate::dialogs::organize_config::show_organize_config(
            &self.window,
            &settings,
            None,
            move |result| {
                if let Some(settings) = result {
                    library::store_organize_settings(&settings);
                    if let Some(sh) = state.upgrade() {
                        sh.sync_enabled();
                    }
                }
            },
        );
    }

    /// Resolves the profiles to run (the ProfileSelector when several
    /// exist) and launches the run window over the selection.
    fn launch_organize(self: &Rc<ShellState>, settings: &cr_organize::profile::PluginSettings) {
        if cr_engine::incoming_transaction::operation_active() {
            show_failure_dialog(
                &self.window,
                "Library Organizer",
                "Another operation is active. Try again after it finishes.",
            );
            return;
        }
        let ids = self.item_view.selection_ids();
        if ids.is_empty() {
            return;
        }
        // The FULL snapshot (the series lookups scan the library) and
        // the selected indexes into it.
        let (books, selected) = Self::organize_snapshot(&ids);
        if selected.is_empty() {
            return;
        }
        let names: Vec<String> = settings.profiles.iter().map(|p| p.name.clone()).collect();
        if settings.profiles.len() > 1 {
            let state = Rc::downgrade(self);
            let settings = settings.clone();
            let last_used = settings.last_used.clone();
            type Pending = std::rc::Rc<std::cell::RefCell<Option<(Vec<ComicBook>, Vec<usize>)>>>;
            let pending: Pending =
                std::rc::Rc::new(std::cell::RefCell::new(Some((books, selected))));
            crate::dialogs::organize::show_profile_selector(
                &self.window,
                &names,
                &last_used,
                move |result| {
                    let Some(chosen) = result else {
                        return;
                    };
                    let Some((books, selected)) = pending.borrow_mut().take() else {
                        return;
                    };
                    let profiles: Vec<cr_organize::profile::Profile> = chosen
                        .iter()
                        .filter_map(|name| {
                            settings.profiles.iter().find(|p| &p.name == name).cloned()
                        })
                        .collect();
                    // The chosen names become the last-used set.
                    let mut store = settings.clone();
                    store.last_used = chosen;
                    library::store_organize_settings(&store);
                    if let Some(sh) = state.upgrade() {
                        if !profiles.is_empty() {
                            sh.run_organize(books, selected, profiles);
                        }
                    }
                },
            );
        } else {
            let profiles = settings.profiles.clone();
            if profiles.is_empty() {
                return;
            }
            self.run_organize(books, selected, profiles);
        }
    }

    /// Runs the organizer over `selected` indexes of `books`.
    fn run_organize(
        self: &Rc<ShellState>,
        books: Vec<ComicBook>,
        selected: Vec<usize>,
        profiles: Vec<cr_organize::profile::Profile>,
    ) {
        if cr_engine::incoming_transaction::operation_active() {
            show_failure_dialog(
                &self.window,
                "Library Organizer",
                "Another operation is active. Try again after it finishes.",
            );
            return;
        }
        let undo_path = library::organizer_undo_path();
        let pool = Arc::clone(&self.pool);
        let window = self.window.clone();
        let refresh_state = Rc::downgrade(self);
        let started = crate::dialogs::organize::show_run_dialog(
            &window,
            books,
            selected,
            profiles,
            Some(undo_path),
            Some(pool),
            move |_report| {
                if let Some(sh) = refresh_state.upgrade() {
                    sh.refresh_view_from_list();
                    let weak = Rc::downgrade(&sh);
                    glib::idle_add_local_once(move || {
                        if let Some(sh) = weak.upgrade() {
                            sh.sync_enabled();
                        }
                    });
                }
            },
        );
        if !started {
            show_failure_dialog(
                &self.window,
                "Library Organizer",
                "Another operation is active. Try again after it finishes.",
            );
        } else {
            self.sync_enabled();
        }
    }

    /// The undo command (`LibraryOrganizerUndo` hook): the last run's
    /// undo log; a successful undo deletes the log.
    fn run_organize_undo(self: &Rc<ShellState>) {
        let Some(operation) = cr_engine::incoming_transaction::try_begin_operation() else {
            show_failure_dialog(
                &self.window,
                "Library Organizer - Undo",
                "Another operation is active. Try again after it finishes.",
            );
            return;
        };
        let undo_path = library::organizer_undo_path();
        if !undo_path.exists() {
            crate::browser::shell::show_report_dialog(
                &self.window,
                "Library Organizer - Undo",
                "Nothing to Undo",
            );
            return;
        }
        let collection = crate::dialogs::organize::load_undo_collection(&undo_path);
        if collection.is_empty() {
            crate::browser::shell::show_report_dialog(
                &self.window,
                "Library Organizer - Undo",
                "Error loading Undo file",
            );
            return;
        }
        let settings = library::organize_settings();
        let profiles: std::collections::HashMap<String, cr_organize::profile::Profile> = settings
            .profiles
            .iter()
            .map(|p| (p.name.clone(), p.clone()))
            .collect();
        // The undo snapshot: the books whose current paths the log
        // names (the addon's `get_library_books`).
        let database_snapshot = {
            let lib = library::session();
            let l = lib.borrow();
            l.database().clone()
        };
        let books = database_snapshot.books.clone();
        let incoming_snapshot = library::incoming_session().borrow().clone();
        let captured_epoch = cr_engine::incoming_transaction::database_epoch();
        let pool = Arc::clone(&self.pool);
        let window = self.window.clone();
        let refresh_state = Rc::downgrade(self);
        let path_for_done = undo_path.clone();
        let manifest_path = cr_organize::engine::adoption_manifest_path(&undo_path);
        let manifest = if manifest_path.exists() {
            match cr_organize::engine::AdoptionManifest::load(&manifest_path) {
                Ok(manifest) if manifest.matches(&collection) => Some(manifest),
                Ok(_) => {
                    show_failure_dialog(
                        &self.window,
                        "Library Organizer - Undo",
                        "The adoption manifest does not match undo.dat.",
                    );
                    return;
                }
                Err(error) => {
                    show_failure_dialog(
                        &self.window,
                        "Library Organizer - Undo",
                        &format!("The adoption manifest cannot be read: {error}"),
                    );
                    return;
                }
            }
        } else {
            if cr_organize::engine::missing_manifest_is_unsafe(
                &collection,
                &library::incoming_config().incoming_folders,
            ) {
                show_failure_dialog(
                    &self.window,
                    "Library Organizer - Undo",
                    "The adoption manifest is missing for an Incoming undo entry. No files were changed.",
                );
                return;
            }
            None
        };
        let mut runnable = cr_organize::engine::UndoCollection::default();
        let mut blocked = cr_organize::engine::UndoCollection::default();
        let incoming_config = library::incoming_config();
        for entry in collection.entries() {
            let target = if manifest
                .as_ref()
                .is_some_and(|value| value.entry_for_current(entry.current_path).is_some())
                && !incoming_config.is_incoming_path(entry.undo_path)
            {
                &mut blocked
            } else {
                &mut runnable
            };
            target.append(entry.undo_path, entry.current_path, entry.profile_name);
        }
        if runnable.is_empty() {
            show_failure_dialog(
                &self.window,
                "Library Organizer - Undo",
                "The original Incoming root is not configured. The undo entry remains available.",
            );
            return;
        }
        if manifest.is_none() {
            crate::dialogs::organize::show_undo_dialog_outcome_with_operation(
                &window,
                books,
                runnable,
                profiles,
                Some(pool),
                operation,
                move |outcome, operation| {
                    for apply in &outcome.applies {
                        match apply {
                            cr_organize::engine::Apply::Update(book) => {
                                library::apply_edited_from_organizer(book, operation);
                            }
                            cr_organize::engine::Apply::Insert(book)
                            | cr_organize::engine::Apply::Adopt(book) => {
                                library::insert_new_book_from_organizer(book, operation);
                            }
                            cr_organize::engine::Apply::Remove(id) => {
                                library::remove_book_from_organizer(id, operation);
                            }
                        }
                    }
                    if let Some(sh) = refresh_state.upgrade() {
                        sh.refresh_view_from_list();
                        let weak = Rc::downgrade(&sh);
                        glib::idle_add_local_once(move || {
                            if let Some(sh) = weak.upgrade() {
                                sh.sync_enabled();
                            }
                        });
                    }
                },
            );
            self.sync_enabled();
            return;
        }
        drop(operation);
        let manifest = manifest.unwrap();
        let adoption_sources: std::collections::HashSet<String> = manifest
            .entries
            .iter()
            .map(|entry| entry.current_path.clone())
            .collect();
        let worker_result = Arc::new(std::sync::Mutex::new(None));
        let worker_result_out = Arc::clone(&worker_result);
        let manifest_for_worker = manifest.clone();
        if !cr_engine::incoming_transaction::begin_operation() {
            show_failure_dialog(
                &self.window,
                "Library Organizer - Undo",
                "Another operation is active. Try again after it finishes.",
            );
            return;
        }
        crate::dialogs::organize::show_custom_run(
            &window,
            "Library Organizer - Undo",
            move |ui, cancel| {
                let _guard = cr_engine::incoming_transaction::acquire_mutation_guard();
                if cr_engine::incoming_transaction::database_epoch() != captured_epoch {
                    return failed_run_outcome(
                        "The library changed before undo started. Try again.".into(),
                    );
                }
                let paths = cr_core::paths::Paths::new_default();
                let database_path = cr_core::paths::database_file(&paths);
                let mut database = database_snapshot;
                let mut catalog = incoming_snapshot;
                let initial = cr_engine::incoming_transaction::IncomingTransaction {
                    kind: cr_engine::incoming_transaction::TransactionKind::Undo,
                    stage: cr_engine::incoming_transaction::TransactionStage::Prepared,
                    files: cr_engine::incoming_transaction::TransactionFiles {
                        incoming_catalog: Some(live_snapshot(
                            cr_core::paths::incoming_file(&paths),
                            match catalog.to_bytes() {
                                Ok(bytes) => bytes,
                                Err(error) => return failed_run_outcome(error.to_string()),
                            },
                        )),
                        comic_database: Some(live_snapshot(
                            database_path,
                            match cr_core::database::comic_database::save_bytes(&database) {
                                Ok(bytes) => bytes,
                                Err(error) => return failed_run_outcome(error.to_string()),
                            },
                        )),
                        ..Default::default()
                    },
                    external_actions: Vec::new(),
                };
                let effects = IncomingOrganizerEffects::new(
                    initial,
                    database.clone(),
                    catalog.clone(),
                    Some(adoption_sources),
                    captured_epoch,
                );
                struct Cover(Arc<ImagePool>);
                impl cr_organize::engine::CoverSource for Cover {
                    fn fileless_cover(&self, book: &ComicBook) -> Option<cr_image::Image> {
                        let key = book.custom_thumbnail_key.as_ref()?;
                        let bytes = self.0.read_custom_thumbnail(key)?;
                        cr_image::decode(&bytes).ok()
                    }
                    fn duplicate_cover(&self, _book: &ComicBook) -> Option<Vec<u8>> {
                        None
                    }
                }
                let trash = |path: &str| library::trash_file(path);
                let context = cr_organize::engine::RunContext {
                    books: &database.books,
                    selected: &[],
                    profiles: &[],
                    move_landing: cr_organize::engine::MoveLanding::UpdateExisting,
                    trash: &trash,
                    filesystem_effects: Some(&effects),
                    cover: &Cover(pool),
                    undo_path: None,
                    cancel,
                };
                let report = cr_organize::engine::undo(context, &runnable, &profiles, ui);
                let mut residual = report.residual.clone();
                for entry in blocked.entries() {
                    residual.append(entry.undo_path, entry.current_path, entry.profile_name);
                }
                let successful: std::collections::HashSet<CrGuid> = manifest_for_worker
                    .entries
                    .iter()
                    .filter(|entry| residual.entry(&entry.current_path).is_none())
                    .filter_map(|entry| CrGuid::parse(&entry.id).ok())
                    .collect();
                for apply in &report.applies {
                    match apply {
                        cr_organize::engine::Apply::Update(book)
                            if successful.contains(&book.id) =>
                        {
                            database.books.retain(|value| value.id != book.id);
                            catalog.books.retain(|value| value.id != book.id);
                            catalog.books.push(book.clone());
                        }
                        _ => apply_organizer_results(&mut database, std::slice::from_ref(apply)),
                    }
                }
                let mut residual_manifest = manifest_for_worker;
                residual_manifest.retain_undo(&residual);
                let undo_snapshot = if residual.is_empty() {
                    cr_engine::incoming_transaction::FileSnapshot {
                        path: path_for_done.clone(),
                        before: std::fs::read(&path_for_done).ok(),
                        after: Vec::new(),
                        remove_after: true,
                    }
                } else {
                    file_snapshot(
                        path_for_done.clone(),
                        match residual.to_bytes(true) {
                            Ok(bytes) => bytes,
                            Err(error) => return failed_run_outcome(error.to_string()),
                        },
                    )
                };
                let manifest_path = cr_organize::engine::adoption_manifest_path(&path_for_done);
                let manifest_snapshot = if residual_manifest.entries.is_empty() {
                    cr_engine::incoming_transaction::FileSnapshot {
                        before: std::fs::read(&manifest_path).ok(),
                        path: manifest_path,
                        after: Vec::new(),
                        remove_after: true,
                    }
                } else {
                    file_snapshot(
                        manifest_path,
                        match residual_manifest.to_bytes() {
                            Ok(bytes) => bytes,
                            Err(error) => return failed_run_outcome(error.to_string()),
                        },
                    )
                };
                let result = effects
                    .finish(
                        &_guard,
                        &database,
                        &catalog,
                        vec![undo_snapshot, manifest_snapshot],
                        None,
                    )
                    .map(|committed_epoch| (database, catalog, committed_epoch));
                *worker_result_out.lock().unwrap() = Some(result);
                crate::dialogs::organize::RunOutcome {
                    text: report.text,
                    failed_or_skipped: report.failed_or_skipped,
                    applies: report.applies,
                    residual: Some(residual),
                }
            },
            move |outcome| {
                if let Some(sh) = refresh_state.upgrade() {
                    match worker_result.lock().unwrap().take() {
                        Some(Ok((database, catalog, committed_epoch)))
                            if cr_engine::incoming_transaction::database_epoch()
                                == committed_epoch =>
                        {
                            library::session()
                                .borrow_mut()
                                .install_persisted_database(database);
                            library::replace_incoming_catalog(catalog);
                        }
                        Some(Ok(_)) => show_failure_dialog(
                            &sh.window,
                            "Library Organizer - Undo",
                            "The live library changed after the transaction committed. Restart ComicRust to load the saved catalogs.",
                        ),
                        Some(Err(error)) => {
                            show_failure_dialog(&sh.window, "Library Organizer - Undo", &error)
                        }
                        None => show_failure_dialog(
                            &sh.window,
                            "Library Organizer - Undo",
                            "The undo worker did not return persistence state.",
                        ),
                    }
                    sh.refresh_view_from_list();
                }
                let _ = outcome;
                cr_engine::incoming_transaction::end_operation();
            },
        );
    }

    /// The organize snapshot: the full book storage plus the indexes
    /// of the selection inside it (the engine's series lookups scan
    /// the whole library, like the addon's `GetLibraryBooks`).
    fn organize_snapshot(ids: &[CrGuid]) -> (Vec<ComicBook>, Vec<usize>) {
        let lib = library::session();
        let l = lib.borrow();
        let books: Vec<ComicBook> = l.database().books.clone();
        let selected: Vec<usize> = books
            .iter()
            .enumerate()
            .filter(|(_, b)| ids.contains(&b.id))
            .map(|(i, _)| i)
            .collect();
        (books, selected)
    }

    /// The scrape wizard over the selection (`cvs_scrape`): no API
    /// key opens Preferences on the Comic Vine Scraper page (the key
    /// entry lives there; the C# aborts the scrape when the key is
    /// still missing).
    fn open_scrape(self: &Rc<ShellState>) {
        let ids = self.item_view.selection_ids();
        if ids.is_empty() {
            return;
        }
        let books = Self::books_by_ids(&ids);
        if books.is_empty() {
            return;
        }
        let config = library::scraper_config();
        if !config.has_api_key() {
            let window = self.window.clone();
            let state = Rc::downgrade(self);
            crate::settings::preferences::show_preferences(&window, Some("scraper"), move || {
                if let Some(sh) = state.upgrade() {
                    sh.sync_enabled();
                }
            });
            return;
        }
        let state = Rc::downgrade(self);
        let state2 = Rc::downgrade(self);
        crate::dialogs::scrape::show_scrape_dialog(
            &self.window,
            &config,
            books,
            crate::dialogs::scrape::ScrapeContext {
                base_url: None,
                pool: Some(Arc::clone(&self.pool)),
                cache: library::cv_cache().map(|c| c as Arc<dyn cr_scrape::cache::CvCache>),
            },
            move |_summary| {
                if let Some(sh) = state.upgrade() {
                    sh.refresh_view_from_list();
                }
            },
            move || {
                if let Some(sh) = state2.upgrade() {
                    sh.refresh_view_from_list();
                }
            },
        );
    }

    /// `SetRating(n)` over the selection (the My Rating menu).
    fn set_rating(&self, rating: f32) {
        let ids = self.item_view.selection_ids();
        if ids.is_empty() {
            return;
        }
        for mut book in Self::books_by_ids(&ids) {
            book.rating = rating;
            library::apply_edited(&book);
        }
        self.refresh_view_from_list();
    }

    /// `RatingEditor.GetRating`: the selection's rating when all
    /// selected books agree, else -1 (mixed / empty).
    fn selection_common_rating(&self) -> f32 {
        let ids = self.item_view.selection_ids();
        if ids.is_empty() {
            return -1.0;
        }
        let lib = library::session();
        let l = lib.borrow();
        let mut num = -1.0f32;
        for book in l.database().books.iter().filter(|b| ids.contains(&b.id)) {
            if num == -1.0 {
                num = book.rating;
            } else if num != book.rating {
                return -1.0;
            }
        }
        num
    }

    /// Whether the CURRENT reader page carries a bookmark
    /// (`RemoveBookmarkAvailable`).
    fn current_page_has_bookmark(&self) -> bool {
        let Some(book) = self.reader.current_comic_book() else {
            return false;
        };
        let Some(display) = self.reader.current_display_page() else {
            return false;
        };
        let Some(provider) = self.reader.provider_index_of_display(display) else {
            return false;
        };
        book.info
            .pages
            .get(provider)
            .and_then(|p| p.bookmark.as_deref())
            .is_some_and(|b| !b.is_empty())
    }

    /// The current provider page of the open book (the page-edit
    /// target). `None` without an open comic.
    fn current_provider_page(&self) -> Option<(usize, usize)> {
        let display = self.reader.current_display_page()?;
        let provider = self.reader.provider_index_of_display(display)?;
        Some((display, provider))
    }

    /// Applies an edit to the CURRENT reader book: the session copy
    /// mutates, the library entry replaces (`apply_edited` — the
    /// dirty mark + the gated file write), and the Pages panel
    /// rebinds. `None` without an open comic.
    fn edit_open_book<F: FnOnce(&mut ComicBook)>(&self, f: F) -> Option<ComicBook> {
        let book = self.reader.edit_current_book(f)?;
        library::apply_edited(&book);
        self.pages.set_book(book.clone());
        Some(book)
    }

    /// The dynamic fill provider (the `DropDownOpening` parity):
    /// every menu open rebuilds the dynamic slots from the live
    /// book/tabs state. The check/disabled state is baked here —
    /// the C# also refreshes at `DropDownOpening`, not through the
    /// command states.
    fn install_dyn_fills(self: &Rc<ShellState>) {
        let state = Rc::downgrade(self);
        let fill: crate::browser::menubar::DynFillFn = Rc::new(move |id| match state.upgrade() {
            Some(sh) => sh.dyn_fill(id),
            None => Vec::new(),
        });
        self.menubar.set_dyn_fill(Rc::clone(&fill));
        self.toolbar.set_dyn_fill(fill.clone());
        // The browser toolbar's Duplicate List drop shares it.
        self.browser_toolbar.set_dyn_fill(fill);
        // The toolbar rides into the undocked window (the T5
        // chrome).
        self.reader.set_undock_chrome(
            self.toolbar.widget().clone().upcast(),
            self.reader_page_box.clone(),
        );
    }

    fn dyn_fill(&self, id: &str) -> Vec<super::menubar::DynNode> {
        use super::menubar::{DynItem, DynNode};
        match id {
            // File ▸ Open Books: one row per open tab, checked on
            // the current, Ctrl+Alt+F1..F12 on the first 12.
            "open-books" => {
                let current = self.reader.current_slot_id();
                self.reader
                    .open_tabs()
                    .into_iter()
                    .enumerate()
                    .map(|(i, (slot, caption))| {
                        let accel = if i < 12 {
                            format!("<Control><Alt>F{}", i + 1)
                        } else {
                            String::new()
                        };
                        let detailed = format!("win.open-tab::{slot}");
                        if !accel.is_empty() {
                            self.app.set_accels_for_action(&detailed, &[accel.as_str()]);
                        }
                        DynNode::Item(DynItem {
                            label: caption,
                            action: detailed,
                            accel,
                            icon: "",
                            checked: current == Some(slot),
                            enabled: true,
                        })
                    })
                    .collect()
            }
            // File ▸ Recent Books: numbered file names, existing
            // files only (`RecentFilesMenuOpening`).
            "recent-books" => {
                let mut out = Vec::new();
                let mut n = 0usize;
                for book in library::recent_books(20) {
                    if !Path::new(&book.file_path).exists() {
                        continue;
                    }
                    n += 1;
                    let name = Path::new(&book.file_path)
                        .file_name()
                        .map(|f| f.to_string_lossy().into_owned())
                        .unwrap_or_else(|| book.file_path.clone());
                    out.push(DynNode::Item(DynItem {
                        label: format!("{n} - {name}"),
                        // The raw path rides the detailed name's
                        // value (split_once takes everything after
                        // the first "::" — colons in paths survive).
                        action: format!("win.recent-book::{}", book.file_path),
                        accel: String::new(),
                        icon: "",
                        checked: false,
                        enabled: true,
                    }));
                }
                out
            }
            // Edit ▸ Bookmarks: the per-page list (the C# "bm"
            // items — disabled on the current page).
            "bookmarks" => {
                let Some(book) = self.reader.current_comic_book() else {
                    return Vec::new();
                };
                let Some(current) = self.current_provider_page().map(|(_, provider)| provider)
                else {
                    return Vec::new();
                };
                book.info
                    .pages
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.bookmark.as_deref().is_some_and(|b| !b.is_empty()))
                    .map(|(i, p)| {
                        let name = p.bookmark.clone().unwrap_or_default();
                        DynNode::Item(DynItem {
                            label: format!("{name} (Page {})", i + 1),
                            action: format!("win.open-bookmark::{i}"),
                            accel: String::new(),
                            icon: "",
                            checked: false,
                            enabled: i != current,
                        })
                    })
                    .collect()
            }
            // The TOOLBAR bookmark drops (`tbPrevPage`/
            // `tbNextPage_DropDownOpening` → `UpdateBookmarkMenu(
            // direction)`): the bookmarks BEFORE (-1) / AFTER (+1)
            // the current page; all rows clickable.
            "bookmarks-prev" | "bookmarks-next" => {
                let dir = if id == "bookmarks-prev" { -1 } else { 1 };
                let Some(book) = self.reader.current_comic_book() else {
                    return Vec::new();
                };
                let Some(current) = self.current_provider_page().map(|(_, provider)| provider)
                else {
                    return Vec::new();
                };
                let mut rows: Vec<_> = book
                    .info
                    .pages
                    .iter()
                    .enumerate()
                    .filter(|(i, p)| {
                        p.bookmark.as_deref().is_some_and(|b| !b.is_empty())
                            && if dir < 0 { *i < current } else { *i > current }
                    })
                    .map(|(i, p)| {
                        let name = p.bookmark.clone().unwrap_or_default();
                        DynNode::Item(DynItem {
                            label: format!("{name} (Page {})", i + 1),
                            action: format!("win.open-bookmark::{i}"),
                            accel: String::new(),
                            icon: "",
                            checked: false,
                            enabled: true,
                        })
                    })
                    .collect();
                // The C# reverses for the backward direction (the
                // nearest bookmark first).
                if dir < 0 {
                    rows.reverse();
                }
                rows
            }
            // Edit ▸ Page Type: the enum radio over the CURRENT
            // page (all rows disabled without a book — the C#
            // `pageEditor.IsValid` rule).
            "page-type" => {
                let has_book = !self.reader.is_empty();
                let current = self.current_provider_page().and_then(|(_, provider)| {
                    self.reader
                        .current_comic_book()
                        .and_then(|b| b.info.pages.get(provider).map(|p| p.page_type))
                });
                crate::dialogs::book_editor::PAGE_TYPE_ITEMS
                    .iter()
                    .map(|(label, v)| {
                        DynNode::Item(DynItem {
                            label: (*label).to_string(),
                            action: format!("win.page-type::{}", v.0),
                            accel: String::new(),
                            icon: "",
                            checked: current.is_some_and(|c| c == *v),
                            enabled: has_book,
                        })
                    })
                    .collect()
            }
            // Edit ▸ Page Rotation: the rotation radio with the C#
            // Permanent icons (`EnumMenuUtility` images dict).
            "page-rotation" => {
                let has_book = !self.reader.is_empty();
                let current = self
                    .reader
                    .current_view()
                    .map(|v| v.page_rotation_of(v.current_page()));
                const NONE: cr_core::model::enums::ImageRotation =
                    cr_core::model::enums::ImageRotation::None;
                [
                    ("None", NONE, "Rotate0Permanent"),
                    (
                        "90\u{b0}",
                        cr_core::model::enums::ImageRotation::Rotate90,
                        "Rotate90Permanent",
                    ),
                    (
                        "180\u{b0}",
                        cr_core::model::enums::ImageRotation::Rotate180,
                        "Rotate180Permanent",
                    ),
                    (
                        "270\u{b0}",
                        cr_core::model::enums::ImageRotation::Rotate270,
                        "Rotate270Permanent",
                    ),
                ]
                .into_iter()
                .map(|(label, rot, icon)| {
                    DynNode::Item(DynItem {
                        label: label.to_string(),
                        action: format!(
                            "win.page-rotation::{}",
                            match rot {
                                NONE => "none",
                                cr_core::model::enums::ImageRotation::Rotate90 => "90",
                                cr_core::model::enums::ImageRotation::Rotate180 => "180",
                                cr_core::model::enums::ImageRotation::Rotate270 => "270",
                            }
                        ),
                        accel: String::new(),
                        icon,
                        checked: current.is_some_and(|c| c == rot),
                        enabled: has_book,
                    })
                })
                .collect()
            }
            // The Duplicate List drop (`tbbDuplicateList_
            // DropDownOpening`): every folder of the tree, an indent
            // per child level; an empty tree shows a disabled None.
            "duplicate-list" => {
                let folders = library::list_folders();
                if folders.is_empty() {
                    return vec![DynNode::Item(DynItem {
                        label: "None".into(),
                        action: String::new(),
                        accel: String::new(),
                        icon: "",
                        checked: false,
                        enabled: false,
                    })];
                }
                folders
                    .into_iter()
                    .map(|(id, level, name)| {
                        DynNode::Item(DynItem {
                            label: format!("{}{}", " ".repeat(level * 4), name),
                            action: format!("win.duplicate-list::{}", id),
                            accel: String::new(),
                            icon: "",
                            checked: false,
                            enabled: true,
                        })
                    })
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// `RefreshDisplay` (F5): the tree re-fills and the current list
    /// re-evaluates.
    fn refresh_view(&self) {
        self.navigator.refill(&library::comic_lists_snapshot());
        self.refresh_view_from_list();
    }

    /// `UpdateQuickFilter` + `UpdateSearch`: rebuild the composed
    /// filter (the quick-search text + the view filters + duplicates)
    /// and apply it to the grid. The result stays in
    /// `current_filter` — the Duplicate List source.
    fn rebuild_filter(&self) {
        let text = self.search_text.borrow().clone();
        let state_str = |name: &str, default: &str| -> String {
            self.action(name)
                .and_then(|a| a.state())
                .and_then(|v| v.get::<String>())
                .unwrap_or_else(|| default.to_string())
        };
        let scope = state_str("search-scope", "all");
        let show = state_str("view-filter", "all");
        let ctype = state_str("comic-type", "all");
        let dups = self
            .action("duplicates-only")
            .and_then(|a| a.state())
            .and_then(|v| v.get::<bool>())
            .unwrap_or(false);
        let matcher = compose_quick_filter(&text, &scope, &show, &ctype, dups);
        *self.current_filter.borrow_mut() = matcher.clone();
        self.item_view.set_filter(matcher);
        // The filter changed the visible set — the selection-info
        // panel follows without an action dispatch.
        self.update_status_panels();
    }

    /// The Select Worst Duplicates command (PORT ADDITION, no C#
    /// counterpart — ADR-044): ranks the duplicate groups of the
    /// CURRENT view and selects the worst copies, so the Remove from
    /// Library command deletes them. The rules come from the
    /// Preferences duplicates page.
    fn select_worst_duplicates(&self) {
        let books = self.item_view.displayed_books();
        let refs: Vec<&cr_core::model::comic_book::ComicBook> = books.iter().collect();
        let rules =
            cr_engine::duplicates::DuplicateRules::from_settings(&library::settings().borrow());
        let ids = cr_engine::duplicates::worst_duplicate_ids(&refs, &rules);
        // The command REPLACES the selection: an empty result (no
        // duplicates, or every copy ties) shows as no selection.
        self.item_view.reselect(&ids);
    }

    /// The Detail header column chooser (`CreateHeaderMenu`): a
    /// model-driven `PopoverMenu` built fresh per open — ONE surface
    /// whose pages swap inside it (Wayland-safe; the popover's own
    /// vertical scroller handles the tall pages). The C# T5/T6
    /// `build_dropdown` shape (has_arrow off + child popover
    /// submenus) fails to MAP when parented to the top-level window
    /// on Wayland; a PopoverMenu maps fine.
    ///
    /// GTK contract (gtkmenusectionbox.c, measured): a model item
    /// with a `custom` attribute takes the INLINE-slot branch only
    /// when it carries NO submenu link — an item with both takes the
    /// SUBMENU branch (page content = the linked model) and
    /// `add_child` returns false, leaving the page empty. So the
    /// submenus are REAL model submenus, not custom pages.
    fn popup_column_chooser(self: &Rc<ShellState>, wx: f64, wy: f64) {
        // Release the previous chooser. This popover is built fresh
        // per open and parented to the window, and it is NOT unparented
        // on close (see the note below the model build), so without
        // this every right-click would leave one more popover attached
        // to the window for the rest of the session. The dismissal is
        // safe here because it runs at popup time, never inside a row
        // click.
        if let Some(old) = self.columns_drop.borrow_mut().take() {
            old.popdown();
            old.unparent();
        }
        let snapshot = self.item_view.detail_columns_snapshot();
        // The per-column check actions (one stateful bool per column
        // id — the model items' checkmarks). Created once; the states
        // refresh on every open so an external `win.toggle-column`
        // between opens keeps the checks honest.
        if self.column_actions.borrow().is_empty() {
            let group = gio::SimpleActionGroup::new();
            let mut map = self.column_actions.borrow_mut();
            for (id, _, _) in snapshot.iter().cloned() {
                let action =
                    gio::SimpleAction::new_stateful(&format!("col{id}"), None, &false.to_variant());
                let state = Rc::downgrade(self);
                action.connect_activate(move |a, _| {
                    crate::trace::trace(format!("cols.col{id} activate handler entered"));
                    let Some(sh) = state.upgrade() else {
                        return;
                    };
                    sh.item_view.toggle_column_visible(id);
                    sh.mark_view_config_dirty();
                    let visible = sh
                        .item_view
                        .detail_columns_snapshot()
                        .iter()
                        .find(|c| c.0 == id)
                        .map(|c| c.2)
                        .unwrap_or(false);
                    a.set_state(&visible.to_variant());
                    sh.sync_enabled();
                });
                group.add_action(&action);
                map.insert(id, action);
            }
            self.window.insert_action_group("cols", Some(&group));
        }
        for (id, _, visible) in &snapshot {
            if let Some(a) = self.column_actions.borrow().get(id) {
                a.set_state(&visible.to_variant());
            }
        }
        // The `ContextMenuBuilder.Create(20)` fill: the visible
        // columns at top level, then All + the letter submenus —
        // every row a model item bound to its column action.
        let chooser = columns::chooser_menu(&snapshot);
        let menu = gio::Menu::new();
        let top = gio::Menu::new();
        for entry in &chooser.top {
            top.append_item(&Self::chooser_item(entry));
        }
        menu.append_section(None, &top);
        let all = gio::Menu::new();
        for entry in &chooser.all {
            all.append_item(&Self::chooser_item(entry));
        }
        menu.append_submenu(Some("All"), &all);
        for (label, run) in &chooser.letters {
            let sub = gio::Menu::new();
            for entry in run {
                sub.append_item(&Self::chooser_item(entry));
            }
            menu.append_submenu(Some(label), &sub);
        }
        let popover = gtk4::PopoverMenu::from_model(Some(&menu));
        // No pointing arrow (the C# ContextMenuStrip shape).
        popover.set_has_arrow(false);
        popover.set_parent(&self.window);
        // NO `connect_closed(unparent)` here. MEASURED (GTK 4.22.4,
        // `GTK_DEBUG=actions`): a row click runs the model button's
        // default handler FIRST, which pops the menu down. An unparent
        // inside `closed` then tears the action muxer down, every
        // tracker item logs "action cols.col<id> was removed" and turns
        // `can_activate` off, and the row handler that runs next finds
        // a dead item and activates NOTHING. That was the
        // 2026-09-12 report: the row stayed checked and the column
        // stayed. The previous popover is dismissed at the top of this
        // function instead, which is outside any click.
        let rect = gtk4::gdk::Rectangle::new(wx as i32, wy as i32 + 4, 1, 1);
        popover.set_pointing_to(Some(&rect));
        *self.columns_drop.borrow_mut() = Some(popover.clone().upcast::<gtk4::Popover>());
        popover.popup();
    }

    /// One chooser check row (a model item bound to its column's
    /// stateful `cols.col<id>` action — the checkmark = the action
    /// state).
    fn chooser_item(entry: &columns::ChooserEntry) -> gio::MenuItem {
        let mi = gio::MenuItem::new(Some(&entry.1), None);
        mi.set_action_and_target_value(Some(&format!("cols.col{}", entry.0)), None);
        mi
    }

    /// The Preferences dialog (shared by the header button and the
    /// action): a modal settings clone committed on OK; the open
    /// reader views and the QuickOpen grid re-apply.
    fn show_preferences(self: &Rc<ShellState>) {
        let window = self.window.clone();
        let state = Rc::downgrade(self);
        crate::settings::preferences::show_preferences(&window, None, move || {
            if let Some(sh) = state.upgrade() {
                sh.reader.apply_settings_to_open_views();
                let size = cr_ui_settings().borrow().quick_open_thumbnail_size as f64;
                sh.quick_view.configure(|c| c.thumb_height = size);
                sh.navigator.refill(&library::comic_lists_snapshot());
                sh.refresh_view_from_list();
                library::refresh_incoming_classification_async();
                library::refresh_incoming_external_gaps_async();
                // Settings may move menu-visible state (the
                // update-book-files hide rule reads
                // AutoUpdateComicsFiles) — re-sync now, not on the
                // next unrelated dispatch.
                sh.sync_enabled();
            }
        });
    }

    /// The post-scan problem report (PORT ADDITION, user request
    /// 2026-09-11). It runs when the LAST queued scan lands, so a full
    /// library scan across many watch roots reports once.
    ///
    /// Nothing here interrupts the scan: the scanner already marked
    /// every problem file and carried on. This only tells the user
    /// that the marked books exist, and how to list them.
    fn report_scan_problems(self: &Rc<ShellState>) {
        // More scans are still queued or running: wait for the last.
        if library::is_scanning() {
            return;
        }
        if let Some((target, error)) = library::take_scan_completion_error() {
            let message = match target {
                library::ScanTarget::Incoming => {
                    format!("The Incoming catalog could not be saved.\n\n{error}")
                }
                library::ScanTarget::Library => {
                    format!("The Library scan could not finish.\n\n{error}")
                }
            };
            show_failure_dialog(
                &self.window,
                match target {
                    library::ScanTarget::Incoming => "Incoming Scan",
                    library::ScanTarget::Library => "Library Scan",
                },
                &message,
            );
        }
        let summary = library::take_scan_problem_summary();
        if summary.is_empty() {
            return;
        }
        crate::trace::trace(format!("scan summary: {summary:?}"));
        let dialog = gtk4::MessageDialog::builder()
            .transient_for(&self.window)
            .modal(false)
            .title("comicrust")
            .text(format!(
                "The scan finished. {} book(s) need attention.",
                summary.total()
            ))
            .secondary_text(format!(
                "{}\n\nThese books are in the library and carry a marker on their \
cover. To list them, make a smart list and paste this query:\n\n\
Match [Custom Value] regex \"{}\" \".\"\n\n\
(The matcher has no \"is not empty\" operator. The regex \".\" matches \
any value with at least one character.)",
                summary.message(),
                cr_core::scan_status::STATUS_KEY
            ))
            .message_type(gtk4::MessageType::Warning)
            .buttons(gtk4::ButtonsType::Close)
            .build();
        dialog.connect_response(|dialog, _| dialog.destroy());
        dialog.present();
    }

    /// The Tasks dialog (`ShowPendingTasks`): one instance — an open
    /// dialog re-presents (`taskDialog.Activate()`).
    fn show_tasks(self: &Rc<ShellState>) {
        if let Some(window) = self.tasks_window.borrow().as_ref() {
            window.present();
            return;
        }
        let dialog = crate::dialogs::tasks::show_tasks_dialog(&self.window, Arc::clone(&self.pool));
        *self.tasks_window.borrow_mut() = Some(dialog.window);
    }

    /// The About dialog (`ShowAboutDialog` — the splash image with
    /// the version line).
    fn show_about(self: &Rc<ShellState>) {
        crate::dialogs::about::show_about(&self.window);
    }

    /// Quick Rating and Review over the FIRST selected book (the C#
    /// `GetRatingEditor().QuickRatingAndReview()` →
    /// `books.FirstOrDefault()`); OK applies rating + review and
    /// stores the AutoShowQuickReview setting.
    fn show_quick_rating(self: &Rc<ShellState>) {
        let Some(id) = self.item_view.selection_ids().first().cloned() else {
            return;
        };
        let Some(book) = Self::books_by_ids(&[id]).into_iter().next() else {
            return;
        };
        let show_when_read = cr_ui_settings().borrow().auto_show_quick_review;
        let state = Rc::downgrade(self);
        let pool = Arc::clone(&self.pool);
        crate::dialogs::quick_rating::show_quick_rating(
            &self.window,
            &book,
            show_when_read,
            pool,
            move |result| {
                let Some(result) = result else {
                    return;
                };
                cr_ui_settings().borrow_mut().auto_show_quick_review = result.show_when_read;
                if let Some(sh) = state.upgrade() {
                    sh.set_quick_rating_fields(&book.id, result.rating, &result.review);
                    sh.sync_enabled();
                }
            },
        );
    }

    /// Applies the Quick Rating OK fields to one library book (the
    /// C# writes rating + review inside `QuickRatingDialog.Show` on
    /// OK). Books outside the library are skipped (the port keeps no
    /// session store for them after the tab closes).
    fn set_quick_rating_fields(&self, id: &CrGuid, rating: f32, review: &str) {
        let Some(mut book) = Self::books_by_ids(std::slice::from_ref(id))
            .into_iter()
            .next()
        else {
            return;
        };
        book.rating = rating;
        book.info.review = review.to_string();
        library::apply_edited(&book);
        self.refresh_view_from_list();
    }

    /// `ToggleBrowser`: the reader and the last browser workspace
    /// flip. From the QuickOpen page the browser shows (the user
    /// report: Browse ▸ Browser did nothing there); without an open
    /// book the reader side stays on the browser/QuickOpen.
    fn toggle_browser(&self) {
        let visible = self
            .stack
            .visible_child_name()
            .map(|s| s.to_string())
            .unwrap_or_default();
        match visible.as_str() {
            "reader" | "quickopen" => self.select_last_browser(),
            "browser" | "pages" | "folders" if self.reader.has_current_book() => {
                self.stack.set_visible_child_name("reader");
            }
            _ => {}
        }
    }

    /// Builds the persisted workspace from the live widgets (the
    /// exit path; the C# `MainForm.CleanUp` copies the layout into
    /// `Settings.CurrentWorkspace`). `prev` keeps the reader layout
    /// when no view is open at exit (the display family always reads
    /// the live session copy).
    fn collect_workspace(
        &self,
        prev: Option<&cr_core::settings::workspace::WorkspaceState>,
    ) -> cr_core::settings::workspace::WorkspaceState {
        use crate::workspace::{browser_view_state, display_to_state, fit_name, layout_name};
        let (w, h) = (self.window.width(), self.window.height());
        let (sort_key, descending, grouper) = self.item_view.sort_group_summary();
        // The C# reader layout lives on the workspace whether or not
        // a book is open; without a view the previous save (or the
        // defaults) carries over.
        let prev_reader = prev.map(|p| p.reader.clone());
        let reader = cr_core::settings::workspace::ReaderLayoutState {
            fit: self
                .reader
                .current_fit_mode()
                .map(fit_name)
                .map(|s| s.to_string())
                .or_else(|| prev_reader.as_ref().map(|r| r.fit.clone()))
                .unwrap_or_else(|| "FitWidth".to_string()),
            layout: self
                .reader
                .current_page_layout()
                .map(layout_name)
                .map(|s| s.to_string())
                .or_else(|| prev_reader.as_ref().map(|r| r.layout.clone()))
                .unwrap_or_else(|| "Single".to_string()),
            rotation: self
                .reader
                .current_rotation()
                .or_else(|| prev_reader.as_ref().map(|r| r.rotation))
                .unwrap_or_default(),
            zoom: self
                .reader
                .current_zoom()
                .or_else(|| prev_reader.as_ref().map(|r| r.zoom))
                .unwrap_or(1.0),
            rtl: self
                .reader
                .current_rtl()
                .or_else(|| prev_reader.as_ref().map(|r| r.rtl))
                .unwrap_or(false),
        };
        cr_core::settings::workspace::WorkspaceState {
            width: w,
            height: h,
            maximized: self.window.is_maximized(),
            view: browser_view_state(
                self.nav_box.is_visible(),
                self.paned.position(),
                crate::workspace::BrowserReadouts {
                    mode: self.item_view.mode(),
                    sort_key,
                    descending,
                    grouper,
                    thumb_height: self.item_view.thumb_height(),
                    tile_height: self.item_view.tile_height(),
                    row_height: self.item_view.row_height(),
                    columns: self.item_view.detail_columns_state(),
                },
            ),
            reader,
            display: display_to_state(&crate::reader::page_view::session_display_options()),
        }
    }

    /// Restores the persisted workspace into the widgets (the
    /// startup path; the C# `MainForm.Load` applies
    /// `Settings.CurrentWorkspace`).
    fn apply_workspace(&self, ws: &cr_core::settings::workspace::WorkspaceState) {
        use crate::workspace::{
            display_from_state, fit_from_name, layout_from_name, mode_from_xml, sort_descending,
        };
        self.paned.set_position(ws.view.browser_split);
        self.nav_box.set_visible(ws.view.show_browser);
        if let Some(a) = self.action("sidebar") {
            a.set_state(&ws.view.show_browser.to_variant());
        }
        // Mode first (the item sizes clamp per mode), then the sizes.
        self.item_view
            .configure(|c| c.mode = mode_from_xml(ws.view.mode));
        self.item_view.configure(|c| {
            c.thumb_height = f64::from(ws.view.thumb_height);
            c.tile_size = (
                f64::from(ws.view.tile_height * 2),
                f64::from(ws.view.tile_height),
            );
            // The C# guard (`value.ItemRowHeight >= 8`): an unset
            // height keeps the boot default (font height + 6).
            if ws.view.row_height >= 8 {
                c.row_height = f64::from(ws.view.row_height);
            }
        });
        if let Some(key) = &ws.view.sort_key {
            self.item_view.set_sort_column(key);
        }
        self.item_view
            .set_sort_direction(sort_descending(ws.view.sort_order));
        if let Some(g) = &ws.view.grouper {
            // The registry owns the 'static keys — a stored key only
            // applies when it still exists.
            if let Some((key, _)) = cr_engine::group::groupers()
                .iter()
                .find(|(k, _)| k == &g.as_str())
            {
                self.item_view.set_grouper(Some(key));
            }
        }
        let cols: Vec<(i32, bool, i32)> = ws
            .view
            .columns
            .iter()
            .map(|c| (c.id, c.visible, c.width))
            .collect();
        self.item_view.set_detail_columns_state(&cols);
        if ws.width > 0 && ws.height > 0 {
            self.window.set_default_size(ws.width, ws.height);
        }
        if ws.maximized {
            self.window.maximize();
        }
        // The display family rides the session copy: new views seed
        // from it (the T12 shape).
        crate::reader::page_view::set_session_display_options(display_from_state(&ws.display));
        self.reader
            .set_reader_seed(crate::reader_shell::ReaderSeed {
                fit: Some(fit_from_name(&ws.reader.fit)),
                layout: Some(layout_from_name(&ws.reader.layout)),
                rtl: Some(ws.reader.rtl),
                zoom: Some(ws.reader.zoom),
                rotation: Some(ws.reader.rotation),
            });
    }

    /// The shell command registry (`CommandMapper` parity): every
    /// command a `win.` action, accelerators on the application.
    /// Stub actions stay DISABLED until their feature lands — the
    /// task that lands each one is noted.
    fn install_commands(self: &Rc<ShellState>) {
        let group = gio::SimpleActionGroup::new();
        let state = Rc::downgrade(self);

        // --- Existing view commands (Phase 4) ---
        let mode_action = gio::SimpleAction::new_stateful(
            "view-mode",
            Some(glib::VariantTy::STRING),
            &"thumbnail".to_variant(),
        );
        {
            let state = state.clone();
            mode_action.connect_activate(move |_, value| {
                let Some(state) = state.upgrade() else {
                    return;
                };
                let name = value.and_then(|v| v.get::<String>()).unwrap_or_default();
                let mode = match name.as_str() {
                    "tile" => ItemViewMode::Tile,
                    "detail" => ItemViewMode::Detail,
                    _ => ItemViewMode::Thumbnail,
                };
                state.item_view.configure(|c| c.mode = mode);
                state.mark_view_config_dirty();
                // The check state follows the VIEW (the T6 user
                // report: the Views check never moved — the state
                // was set here but the dropdown rows only re-render
                // on the sync, which this handler never ran).
                state.sync_enabled();
            });
        }
        group.add_action(&mode_action);
        self.actions.borrow_mut().insert("view-mode", mode_action);

        // thumb-size: grow / shrink (the C# Ctrl+wheel steps 16).
        for (name, delta) in [("thumb-bigger", THUMB_STEP), ("thumb-smaller", -THUMB_STEP)] {
            let action = gio::SimpleAction::new(name, None);
            let state = state.clone();
            action.connect_activate(move |_, _| {
                if let Some(sh) = state.upgrade() {
                    let current = sh.item_view.thumb_height();
                    let next = (current + delta).clamp(MIN_THUMB, MAX_THUMB);
                    sh.item_view.configure(|c| c.thumb_height = next);
                    sh.mark_view_config_dirty();
                }
            });
            group.add_action(&action);
        }

        // sort-column (string parameter = the property name; "" =
        // Not Sorted — the Arrange menu's first row). STATEFUL: the
        // check state rides the current sort property.
        let sort_action = gio::SimpleAction::new_stateful(
            "sort-column",
            Some(glib::VariantTy::STRING),
            &"".to_variant(),
        );
        {
            let state = state.clone();
            sort_action.connect_activate(move |action, value| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let name = value.and_then(|v| v.get::<String>()).unwrap_or_default();
                if name.is_empty() {
                    sh.item_view.clear_sort();
                } else {
                    sh.item_view.set_sort_column(&name);
                }
                sh.mark_view_config_dirty();
                action.set_state(&name.to_variant());
                sh.sync_enabled();
            });
        }
        group.add_action(&sort_action);
        self.actions.borrow_mut().insert("sort-column", sort_action);

        // sort-direction toggle.
        let dir_action = gio::SimpleAction::new("sort-direction", None);
        {
            let state = state.clone();
            dir_action.connect_activate(move |_, _| {
                if let Some(sh) = state.upgrade() {
                    sh.item_view.toggle_sort_direction();
                    sh.mark_view_config_dirty();
                }
            });
        }
        group.add_action(&dir_action);

        // group-by (string parameter; "" = none). STATEFUL: the
        // check mark follows the grouper key.
        let group_action = gio::SimpleAction::new_stateful(
            "group-by",
            Some(glib::VariantTy::STRING),
            &"".to_variant(),
        );
        {
            let state = state.clone();
            group_action.connect_activate(move |action, value| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let name = value.and_then(|v| v.get::<String>()).unwrap_or_default();
                let grouper = if name.is_empty() {
                    None
                } else {
                    cr_engine::group::groupers()
                        .iter()
                        .find(|(k, _)| *k == name)
                        .map(|(k, _)| *k)
                };
                sh.item_view.set_grouper(grouper);
                sh.mark_view_config_dirty();
                action.set_state(&name.to_variant());
                sh.sync_enabled();
            });
        }
        group.add_action(&group_action);
        self.actions.borrow_mut().insert("group-by", group_action);

        // The view filters (`ComicBookAllPropertiesMatcher.Create`):
        // the read-state radio, the comic-type toggles, duplicates.
        let view_filter = gio::SimpleAction::new_stateful(
            "view-filter",
            Some(glib::VariantTy::STRING),
            &"all".to_variant(),
        );
        {
            let state = state.clone();
            view_filter.connect_activate(move |action, value| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let name = value.and_then(|v| v.get::<String>()).unwrap_or_default();
                if !matches!(name.as_str(), "all" | "unread" | "reading" | "read") {
                    return;
                }
                action.set_state(&name.to_variant());
                sh.rebuild_filter();
                sh.sync_enabled();
            });
        }
        group.add_action(&view_filter);
        self.actions.borrow_mut().insert("view-filter", view_filter);

        let comic_type = gio::SimpleAction::new_stateful(
            "comic-type",
            Some(glib::VariantTy::STRING),
            &"all".to_variant(),
        );
        {
            let state = state.clone();
            comic_type.connect_activate(move |action, value| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let name = value.and_then(|v| v.get::<String>()).unwrap_or_default();
                if !matches!(name.as_str(), "books" | "fileless") {
                    return;
                }
                // The C# rows toggle: the active row returns to All.
                let current = action
                    .state()
                    .and_then(|v| v.get::<String>())
                    .unwrap_or_default();
                let next = if current == name {
                    "all".to_string()
                } else {
                    name
                };
                action.set_state(&next.to_variant());
                sh.rebuild_filter();
                sh.sync_enabled();
            });
        }
        group.add_action(&comic_type);
        self.actions.borrow_mut().insert("comic-type", comic_type);

        let duplicates =
            gio::SimpleAction::new_stateful("duplicates-only", None, &false.to_variant());
        {
            let state = state.clone();
            duplicates.connect_activate(move |action, _| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let next = !action
                    .state()
                    .and_then(|v| v.get::<bool>())
                    .unwrap_or(false);
                action.set_state(&next.to_variant());
                sh.rebuild_filter();
                sh.sync_enabled();
            });
        }
        group.add_action(&duplicates);
        self.actions
            .borrow_mut()
            .insert("duplicates-only", duplicates);

        // The group expand/collapse command (the C#
        // `ItemView.ToggleGroups` through `miExpandAllGroups` — no
        // check state, enabled iff groups are visible).
        self.add_simple(&group, "toggle-groups", |sh| {
            sh.item_view.toggle_all_groups();
        });

        // PORT ADDITION (no C# counterpart — ADR-044): the duplicate
        // cleanup (no accelerator; the context menu hosts it).
        self.add_simple(&group, "select-worst-duplicates", |sh| {
            sh.select_worst_duplicates();
        });

        // The Quick Search scope radio (the cue text follows).
        let scope = gio::SimpleAction::new_stateful(
            "search-scope",
            Some(glib::VariantTy::STRING),
            &"all".to_variant(),
        );
        {
            let state = state.clone();
            scope.connect_activate(move |action, value| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let name = value.and_then(|v| v.get::<String>()).unwrap_or_default();
                if !super::browser_toolbar::SEARCH_SCOPE_LABELS
                    .iter()
                    .any(|(v, _)| *v == name)
                {
                    return;
                }
                action.set_state(&name.to_variant());
                sh.search.set_placeholder_text(Some(
                    super::browser_toolbar::SEARCH_SCOPE_LABELS
                        .iter()
                        .find(|(v, _)| *v == name)
                        .map(|(_, l)| *l)
                        .unwrap_or("Search All"),
                ));
                sh.sync_enabled();
            });
        }
        group.add_action(&scope);
        self.actions.borrow_mut().insert("search-scope", scope);

        // The Detail column chooser rows (toggle one column).
        let toggle_column = gio::SimpleAction::new("toggle-column", Some(glib::VariantTy::STRING));
        {
            let state = state.clone();
            toggle_column.connect_activate(move |_, value| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let Some(text) = value.and_then(|v| v.get::<String>()) else {
                    return;
                };
                let Ok(id) = text.parse::<i32>() else {
                    return;
                };
                sh.item_view.toggle_column_visible(id);
                sh.sync_enabled();
            });
        }
        group.add_action(&toggle_column);

        // Duplicate List (the folder rows; the parameter = the
        // folder id).
        let duplicate = gio::SimpleAction::new("duplicate-list", Some(glib::VariantTy::STRING));
        {
            let state = state.clone();
            duplicate.connect_activate(move |_, value| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let Some(text) = value.and_then(|v| v.get::<String>()) else {
                    return;
                };
                let Ok(folder) = CrGuid::parse(&text) else {
                    return;
                };
                let Some(source) = *sh.current_list.borrow() else {
                    return;
                };
                let filter = sh.current_filter.borrow().clone();
                let Some(filter) = filter else {
                    return;
                };
                match library::duplicate_smart_list(&source, &folder, &filter) {
                    Ok(_) => sh.refresh_view(),
                    Err(err) => eprintln!("duplicate list failed: {err}"),
                }
            });
        }
        group.add_action(&duplicate);

        // list-layouts — the T14 workspace data lands the menus.
        self.add_disabled(&group, "list-layouts");

        // --- File ---
        self.add_simple(&group, "open-file", |sh| {
            let window = sh.window.clone();
            let state = Rc::downgrade(sh);
            open_file_dialog(&window, move |path| {
                if let Some(sh) = state.upgrade() {
                    sh.open_comic(Path::new(&path));
                }
            });
        });
        self.add_simple(&group, "close", |sh| sh.reader.close_current_tab());
        self.add_simple(&group, "close-all", |sh| sh.reader.close_all_tabs());
        // `OpenBooks.AddSlot` + `CurrentSlot = last`: the new EMPTY
        // slot selects and shows (the ported shape — no QuickOpen
        // overlay in the empty slot; recorded deviation).
        self.add_simple(&group, "new-tab", |sh| {
            sh.reader.add_empty_slot();
            sh.stack.set_visible_child_name("reader");
        });
        // The Open Books rows (`OpenBooks_Clicked`: CurrentSlot = i).
        {
            let open_tab = gio::SimpleAction::new("open-tab", Some(glib::VariantTy::STRING));
            let state = state.clone();
            open_tab.connect_activate(move |_, value| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let Some(text) = value.and_then(|v| v.get::<String>()) else {
                    return;
                };
                let Ok(slot) = text.parse::<usize>() else {
                    return;
                };
                // `OpenBooks_Clicked`: `CurrentSlot = i` — the reader
                // workspace shows.
                sh.activate_slot(slot);
                sh.sync_enabled();
            });
            group.add_action(&open_tab);
        }
        // The Recent Books rows (`OnOpenRecent`: open the path).
        {
            let recent = gio::SimpleAction::new("recent-book", Some(glib::VariantTy::STRING));
            let state = state.clone();
            recent.connect_activate(move |_, value| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let Some(path) = value.and_then(|v| v.get::<String>()) else {
                    return;
                };
                sh.open_comic(Path::new(&path));
                sh.sync_enabled();
            });
            group.add_action(&recent);
        }
        self.add_simple(&group, "add-folder", |sh| {
            let window = sh.window.clone();
            crate::app::add_folder_dialog(&window);
            let state = Rc::downgrade(sh);
            glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
                if let Some(sh) = state.upgrade() {
                    sh.navigator.refill(&library::comic_lists_snapshot());
                }
                glib::ControlFlow::Break
            });
        });
        // migrate-paths — the Windows-path migration dialog (Phase 8
        // T11): the boot prompt, re-runnable from the File menu.
        self.add_simple(&group, "migrate-paths", |sh| {
            if !library::has_windows_paths() {
                return;
            }
            let window = sh.window.clone();
            let state = Rc::clone(sh);
            crate::dialogs::path_migration::run(&window, move |report| {
                if report.is_some_and(|r| r.changed_anything()) {
                    state.navigator.refill(&library::comic_lists_snapshot());
                    state.refresh_view_from_list();
                    state.sync_enabled();
                }
            });
        });
        self.add_simple(&group, "scan-folders", |sh| {
            // `StartFullScan`: re-scan every watch-folder root (the
            // C# `QueueManager.StartScan(all,
            // RemoveMissingFilesOnFullScan)`; the remove-missing flag
            // is not ported — scans flag, never delete).
            let roots: Vec<String> = {
                let lib = library::session();
                let l = lib.borrow();
                l.database()
                    .watch_folders
                    .iter()
                    .map(|w| w.folder.clone())
                    .collect()
            };
            for root in roots {
                if root.is_empty() {
                    continue;
                }
                // The scan lands on the pump — refresh there (the C#
                // scan events update the live view; the port merges
                // the storage once at landing).
                let weak = Rc::downgrade(sh);
                library::add_folder_to_library(Path::new(&root), move |_| {
                    if let Some(sh) = weak.upgrade() {
                        sh.refresh_view_from_list();
                        sh.sync_enabled();
                        sh.report_scan_problems();
                    }
                });
            }
        });
        self.add_simple(&group, "update-book-files", |_| {
            library::update_all_book_files();
        });
        // The Tasks dialog (the C# `ShowPendingTasks`; the lamps and
        // the menu open the same single instance).
        self.add_simple(&group, "tasks", ShellState::show_tasks);
        // The Comic Vine Scraper (Phase 12): the config dialog and
        // the scrape wizard over the selection.
        self.add_simple(&group, "scrape-config", ShellState::show_scrape_config);
        self.add_simple(&group, "scrape-books", ShellState::open_scrape);
        // The Library Organizer (Phase 17): the main entry (config +
        // run), Quick (no config), the configure-only dialog, and the
        // undo command (the addon's Books/Library/ConfigScript/Undo
        // hooks).
        self.add_simple(&group, "organize-books", ShellState::open_organize);
        self.add_simple(&group, "organize-quick", ShellState::open_organize_quick);
        self.add_simple(
            &group,
            "organize-configure",
            ShellState::show_organize_config,
        );
        self.add_simple(&group, "organize-undo", ShellState::run_organize_undo);
        // The Comic Vine disk cache (ADR-037, Phase 15): the MCL
        // seed import and the warm task. The C# plugin had no cache,
        // so it had no such commands.
        self.add_simple(&group, "cv-import-mcl", ShellState::import_cv_mcl);
        self.add_simple(&group, "cv-update", ShellState::update_cv_cache);
        self.add_simple(&group, "cv-warm", ShellState::warm_cv_cache);
        // generate-thumbnails — the C# `CacheThumbnails` queue
        // command: one unlimited-queue warm-up job per library book
        // (the worker skips covers already in the thumbnail disk
        // cache).
        self.add_simple(&group, "generate-thumbnails", |_sh| {
            let pool = Arc::clone(&_sh.pool);
            library::cache_thumbnails(&pool);
        });
        // new-book-entry — the C# `AddNewBook()` (MainForm.cs:1879):
        // a fileless book (no file path) opens the editor; OK inserts
        // it into the database, Cancel discards.
        self.add_simple(&group, "new-book-entry", ShellState::open_new_book_editor);
        // new-book-series — the NewComics.py port ("New fileless Book
        // Series..."): a dialog creates a run of fileless books and
        // selects them (ADR-027 moved the script natively into the
        // app; the C# inserted the script item right after
        // `miNewComic`).
        self.add_simple(&group, "new-book-series", ShellState::new_book_series);
        self.add_simple(&group, "restart", |sh| {
            // `MenuRestart`: save, then re-launch the binary (the C#
            // `Program.Restart` + `Application.Restart`). The
            // workspace snapshot lands first (the restart keeps the
            // layout).
            {
                let prev = cr_ui_settings().borrow().current_workspace.clone();
                let ws = sh.collect_workspace(prev.as_ref());
                cr_ui_settings().borrow_mut().current_workspace = Some(ws);
            }
            if let Err(err) = library::save() {
                eprintln!("library save failed: {err}");
            }
            library::save_settings();
            if let Ok(exe) = std::env::current_exe() {
                // The C# restart handshake (Program.cs:1151-1155):
                // the new process waits for THIS pid to exit before
                // it starts — with unique mode a bare spawn would
                // forward to the dying instance instead of replacing
                // it. `-restart` clears the one-shot file arguments.
                let _ = std::process::Command::new(exe)
                    .args(["-restart", "-waitpid", &std::process::id().to_string()])
                    .spawn();
            }
            sh.app.quit();
        });
        self.add_simple(&group, "quit", |sh| sh.window.close());

        // --- Edit ---
        self.add_simple(&group, "info", ShellState::show_info);
        for (n, name) in [
            (0u32, "rating-0"),
            (1, "rating-1"),
            (2, "rating-2"),
            (3, "rating-3"),
            (4, "rating-4"),
            (5, "rating-5"),
        ] {
            // STATEFUL (the check state — `Math.Round(GetRating())
            // == N`; sync_enabled writes it).
            let action = gio::SimpleAction::new_stateful(name, None, &false.to_variant());
            let state = state.clone();
            action.connect_activate(move |_, _| {
                if let Some(sh) = state.upgrade() {
                    sh.set_rating(n as f32);
                    sh.sync_enabled();
                }
            });
            group.add_action(&action);
            self.actions.borrow_mut().insert(name, action);
        }
        // Quick Rating and Review — the FIRST selected book (the
        // C# `QuickRatingAndReview` passes `books.FirstOrDefault()`).
        self.add_simple(&group, "quick-rating", ShellState::show_quick_rating);
        // Set Bookmark — the name prompt over the CURRENT page
        // (`SetBookmark`: the proposal is the existing bookmark or
        // the page number; an empty entry clears the bookmark).
        self.add_simple(&group, "set-bookmark", |sh| {
            let Some((_, provider)) = sh.current_provider_page() else {
                return;
            };
            let Some(book) = sh.reader.current_comic_book() else {
                return;
            };
            let existing = book
                .info
                .pages
                .get(provider)
                .and_then(|p| p.bookmark.clone())
                .unwrap_or_default();
            let proposal = if existing.is_empty() {
                format!("Page {}", provider + 1)
            } else {
                existing
            };
            let state = Rc::downgrade(sh);
            crate::dialogs::name_prompt::show_name_prompt(
                &sh.window,
                "Bookmark",
                &proposal,
                move |name| {
                    let Some(sh) = state.upgrade() else {
                        return;
                    };
                    sh.edit_open_book(|b| update_bookmark_entry(b, provider, &name));
                    sh.sync_enabled();
                },
            );
        });
        // Remove Bookmark: clears the CURRENT page's bookmark
        // (`UpdateBookmark(page, "")`).
        self.add_simple(&group, "remove-bookmark", |sh| {
            let Some((_, provider)) = sh.current_provider_page() else {
                return;
            };
            sh.edit_open_book(|b| update_bookmark_entry(b, provider, ""));
            sh.sync_enabled();
        });
        // The Bookmarks list rows (`win.open-bookmark::<provider>`).
        {
            let jump = gio::SimpleAction::new("open-bookmark", Some(glib::VariantTy::STRING));
            let state = state.clone();
            jump.connect_activate(move |_, value| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let Some(text) = value.and_then(|v| v.get::<String>()) else {
                    return;
                };
                let Ok(provider) = text.parse::<usize>() else {
                    return;
                };
                let Some(target) = sh.reader.display_of_provider(provider) else {
                    return;
                };
                sh.reader.navigate_current(target);
                sh.sync_enabled();
            });
            group.add_action(&jump);
        }
        self.add_simple(&group, "prev-bookmark", |sh| {
            sh.reader.dispatch_current("MoveToPrevBookmark")
        });
        self.add_simple(&group, "next-bookmark", |sh| {
            sh.reader.dispatch_current("MoveToNextBookmark")
        });
        self.add_simple(&group, "last-page-read", |sh| {
            // `ComicDisplay.DisplayLastPageRead`.
            if let Some(book) = sh.reader.current_comic_book() {
                let page = book.last_page_read.max(0) as usize;
                sh.reader.navigate_current(page);
            }
        });
        // copy-page — `ComicDisplay.CopyPageToClipboard`
        // (ComicDisplay.cs:1441): the current composed page image onto
        // the clipboard (errors are swallowed in the C# too).
        self.add_simple(&group, "copy-page", |sh| {
            crate::trace::trace("copy-page: handler entered");
            let view = sh.reader.current_view();
            let surface = view.and_then(|v| v.create_page_image());
            crate::trace::trace(format!("copy-page: view+surface ok={}", surface.is_some()));
            let Some(surface) = surface else {
                return;
            };
            copy_surface_to_clipboard(&surface);
        });
        // export-page — `ExportCurrentImage` (MainForm.cs:2326 +
        // `ExportImage` 2185): the "Save Page as" dialog, the name
        // "{Caption} - Page {N}", the 5-format filter, the filter
        // index persisted (`LastExportPageFilterIndex`).
        self.add_simple(&group, "export-page", |sh| {
            crate::trace::trace("export-page: handler entered");
            let view = sh.reader.current_view();
            let caption = sh
                .reader
                .current_comic_book()
                .map(|b| cr_engine::display_text::caption(&b))
                .unwrap_or_default();
            let page = sh.reader.current_display_page().map_or(1, |p| p + 1);
            let surface = view.and_then(|v| v.create_page_image());
            crate::trace::trace(format!(
                "export-page: caption={caption:?} page={page} surface={}",
                surface.is_some()
            ));
            export_page_dialog(&sh.window, &caption, page, surface);
        });
        self.add_simple(&group, "refresh", |sh| sh.refresh_view());
        self.add_simple(&group, "preferences", ShellState::show_preferences);

        // --- Browse ---
        // `ToggleBrowser` with the `() => BrowserVisible` check.
        self.add_check(&group, "toggle-browser", true, |sh| sh.toggle_browser());
        self.add_simple(&group, "view-library", |sh| {
            sh.select_workspace(Workspace::Library);
        });
        self.add_simple(&group, "view-pages", |sh| {
            sh.select_workspace(Workspace::Pages);
        });
        {
            let sidebar = gio::SimpleAction::new_stateful("sidebar", None, &true.to_variant());
            let state = state.clone();
            sidebar.connect_activate(move |action, _| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let visible = !sh.nav_box.is_visible();
                sh.nav_box.set_visible(visible);
                action.set_state(&visible.to_variant());
            });
            group.add_action(&sidebar);
            self.actions.borrow_mut().insert("sidebar", sidebar);
        }
        // small-preview — T11 lands the pane.
        self.add_disabled(&group, "small-preview");
        // Dark mode — NO C# command (recorded deviation): the C#
        // theme (`ExtendedSettings.Theme` + the `-dark` switch) is
        // boot-time only. The global is the source of truth; the
        // sync derives the check from it, the ini keys persist it.
        {
            let initial = cr_core::settings::ExtendedSettings::global().effective_theme()
                == cr_core::settings::enums::Themes::Dark;
            let dark = gio::SimpleAction::new_stateful("dark-mode", None, &initial.to_variant());
            {
                let state = state.clone();
                dark.connect_activate(move |action, _| {
                    let Some(sh) = state.upgrade() else {
                        return;
                    };
                    let next = {
                        let extended = cr_core::settings::ExtendedSettings::global();
                        extended.effective_theme() != cr_core::settings::enums::Themes::Dark
                    };
                    {
                        let mut extended = cr_core::settings::ExtendedSettings::global_mut();
                        extended.theme = if next {
                            cr_core::settings::enums::Themes::Dark
                        } else {
                            cr_core::settings::enums::Themes::Default
                        };
                        // The toggle is the explicit intent: the
                        // `-dark` force must not re-darken the next
                        // boot over the stored Theme.
                        extended.use_dark_mode = false;
                    }
                    crate::theme::set_dark(next);
                    action.set_state(&next.to_variant());
                    library::save_ini_keys(&[
                        ("UseDarkMode", "False"),
                        ("Theme", if next { "Dark" } else { "Default" }),
                    ]);
                    sh.sync_enabled();
                });
            }
            group.add_action(&dark);
            self.actions.borrow_mut().insert("dark-mode", dark);
        }
        self.add_simple(&group, "prev-list", |sh| sh.browse_history(-1));
        self.add_simple(&group, "next-list", |sh| sh.browse_history(1));

        // --- Read ---
        self.add_simple(&group, "first-page", |sh| {
            sh.reader.dispatch_current("MoveToFirstPage")
        });
        self.add_simple(&group, "prev-page", |sh| {
            sh.reader.dispatch_current("MoveToPreviousPage")
        });
        self.add_simple(&group, "next-page", |sh| {
            sh.reader.dispatch_current("MoveToNextPage")
        });
        self.add_simple(&group, "last-page", |sh| {
            sh.reader.dispatch_current("MoveToLastPage")
        });
        self.add_simple(&group, "prev-book", |sh| sh.open_next_book(-1));
        self.add_simple(&group, "next-book", |sh| sh.open_next_book(1));
        self.add_simple(&group, "random-book", |sh| sh.open_next_book(0));
        self.add_simple(&group, "show-in-browser", |sh| {
            // `SyncBrowser`: reveal the browser, select the open book.
            if let Some(book) = sh.reader.current_comic_book() {
                sh.show_browser();
                sh.item_view.select_book(&book.id);
            }
        });
        self.add_simple(&group, "prev-tab", |sh| {
            // `OpenBooks.PreviousSlot`: the switch reveals the reader
            // (`ShowView(i)` shows the comic viewer).
            if sh.reader.cycle_slot(-1) {
                sh.stack.set_visible_child_name("reader");
            }
        });
        self.add_simple(&group, "next-tab", |sh| {
            if sh.reader.cycle_slot(1) {
                sh.stack.set_visible_child_name("reader");
            }
        });
        // `() => Program.Settings.AutoScrolling` — the view field
        // mirrors the C# setting (session-only here; the port writes
        // the unified config, ADR-033).
        self.add_check(&group, "auto-scroll", false, |sh| {
            sh.reader.dispatch_current("ToggleAutoScrolling");
        });
        // `() => ComicDisplay.TwoPageNavigation`.
        self.add_check(&group, "double-auto-scroll", true, |sh| {
            sh.reader.dispatch_current("DoublePageAutoScroll");
        });
        {
            // `tsCurrentPage_Click` (`TrackCurrentPage = !…`): the
            // check derives from the SETTING in the sync (the T6
            // source-of-truth rule).
            let track = cr_ui_settings().borrow().track_current_page;
            self.add_check(&group, "track-current-page", track, |_sh| {
                let next = !cr_ui_settings().borrow().track_current_page;
                cr_ui_settings().borrow_mut().track_current_page = next;
            });
            // `tbShowMainMenu` (the Tools menu): CHECKED while the
            // menu is NOT auto-hidden — the C# flips
            // `AutoHideMainMenu` (MainForm.cs:1455-1458).
            let show = gio::SimpleAction::new_stateful(
                "show-main-menu",
                None,
                &(!cr_ui_settings().borrow().auto_hide_main_menu).to_variant(),
            );
            {
                let state = state.clone();
                show.connect_activate(move |action, _| {
                    let Some(sh) = state.upgrade() else {
                        return;
                    };
                    let next = !cr_ui_settings().borrow().auto_hide_main_menu;
                    cr_ui_settings().borrow_mut().auto_hide_main_menu = next;
                    action.set_state(&(!next).to_variant());
                    sh.update_menubar();
                });
            }
            group.add_action(&show);
            self.actions.borrow_mut().insert("show-main-menu", show);
        }

        // --- Display ---
        // display-settings — the `EditWorkspaceDisplaySettings` port
        // (F9): snapshot the current reader view (or the session
        // copy), the dialog edits the copy, OK/Apply push it onto
        // every open view (`SetWorkspaceDisplayOptions`).
        {
            let state = state.clone();
            let action = gio::SimpleAction::new("display-settings", None);
            action.connect_activate(move |_, _| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let window = sh.window.clone();
                let options = sh
                    .reader
                    .current_view()
                    .map(|view| view.display_options())
                    .unwrap_or_else(crate::reader::page_view::session_display_options);
                let reader = sh.reader.clone();
                crate::dialogs::display_settings::show_display_settings(
                    &window,
                    options,
                    move |options| {
                        // The session copy records FIRST (the
                        // workspace write-back shape) so a book
                        // opened later seeds the same options; every
                        // open view re-applies
                        // (`SetWorkspaceDisplayOptions`).
                        crate::reader::page_view::set_session_display_options(options.clone());
                        reader.apply_display_options_all(options);
                    },
                );
            });
            group.add_action(&action);
            self.actions.borrow_mut().insert("display-settings", action);
        }
        let fit_action = gio::SimpleAction::new_stateful(
            "page-fit",
            Some(glib::VariantTy::STRING),
            &"fit-all".to_variant(),
        );
        {
            let state = state.clone();
            fit_action.connect_activate(move |_, value| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let name = value.and_then(|v| v.get::<String>()).unwrap_or_default();
                let id = match name.as_str() {
                    "original" => "Original",
                    "fit-all" => "FitAll",
                    "fit-width" => "FitWidth",
                    "fit-width-adaptive" => "FitWidthAdaptive",
                    "fit-height" => "FitHeight",
                    "fit-best" => "FitBest",
                    _ => return,
                };
                sh.reader.dispatch_current(id);
                sh.sync_enabled();
            });
        }
        group.add_action(&fit_action);
        self.actions.borrow_mut().insert("page-fit", fit_action);

        let layout_action = gio::SimpleAction::new_stateful(
            "page-layout",
            Some(glib::VariantTy::STRING),
            &"single".to_variant(),
        );
        {
            let state = state.clone();
            layout_action.connect_activate(move |_, value| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let name = value.and_then(|v| v.get::<String>()).unwrap_or_default();
                let id = match name.as_str() {
                    "single" => "SinglePage",
                    "double" => "TwoPages",
                    "double-adaptive" => "TwoPagesAdaptive",
                    "continuous" => "Continuous",
                    _ => return,
                };
                sh.reader.dispatch_current(id);
                sh.sync_enabled();
            });
        }
        group.add_action(&layout_action);
        self.actions
            .borrow_mut()
            .insert("page-layout", layout_action);

        let rtl_action =
            gio::SimpleAction::new_stateful("right-to-left", None, &false.to_variant());
        {
            let state = state.clone();
            rtl_action.connect_activate(move |_, _| {
                if let Some(sh) = state.upgrade() {
                    sh.reader.dispatch_current("RightToLeft");
                    sh.sync_enabled();
                }
            });
        }
        group.add_action(&rtl_action);
        self.actions
            .borrow_mut()
            .insert("right-to-left", rtl_action);

        let oversized =
            gio::SimpleAction::new_stateful("only-fit-oversized", None, &false.to_variant());
        {
            let state = state.clone();
            oversized.connect_activate(move |_, _| {
                if let Some(sh) = state.upgrade() {
                    sh.reader.dispatch_current("OnlyFitIfOversized");
                    sh.sync_enabled();
                }
            });
        }
        group.add_action(&oversized);

        self.add_simple(&group, "zoom-in", |sh| sh.reader.dispatch_current("ZoomIn"));
        self.add_simple(&group, "zoom-out", |sh| {
            sh.reader.dispatch_current("ZoomOut")
        });
        // `MainForm.ToggleZoom` (the menu item; the touch binding is
        // a reader-internal no-op).
        self.add_simple(&group, "toggle-zoom", |sh| sh.reader.toggle_zoom_current());
        // The Zoom presets (`ComicDisplay.ImageZoom = v`).
        {
            let zoom = gio::SimpleAction::new("zoom-preset", Some(glib::VariantTy::STRING));
            let state = Rc::downgrade(self);
            zoom.connect_activate(move |_, value| {
                if let Some(sh) = state.upgrade() {
                    let Some(text) = value.and_then(|v| v.get::<String>()) else {
                        return;
                    };
                    let percent: f32 = text.parse().unwrap_or(100.0);
                    sh.reader.zoom_current(percent / 100.0);
                    sh.sync_enabled();
                }
            });
            group.add_action(&zoom);
            self.actions.borrow_mut().insert("zoom-preset", zoom);
        }
        // Custom Zoom (the C# always enables the item — the dialog
        // opens without a book too; OK stores the zoom when a view
        // exists).
        self.add_simple(&group, "zoom-custom", |sh| {
            let zoom = sh.reader.current_zoom().unwrap_or(1.0);
            let state = Rc::downgrade(sh);
            crate::dialogs::zoom::show_zoom_dialog(&sh.window, zoom, move |result| {
                if let (Some(z), Some(sh)) = (result, state.upgrade()) {
                    sh.reader.zoom_current(z);
                    sh.sync_enabled();
                }
            });
        });
        self.add_simple(&group, "rotate-left", |sh| {
            sh.reader.dispatch_current("RotateCC")
        });
        self.add_simple(&group, "rotate-right", |sh| {
            sh.reader.dispatch_current("RotateC")
        });
        self.add_simple(&group, "rotate-0", |sh| {
            sh.reader.dispatch_current("Rotate0")
        });
        self.add_simple(&group, "rotate-90", |sh| {
            sh.reader.dispatch_current("Rotate90")
        });
        self.add_simple(&group, "rotate-180", |sh| {
            sh.reader.dispatch_current("Rotate180")
        });
        self.add_simple(&group, "rotate-270", |sh| {
            sh.reader.dispatch_current("Rotate270")
        });
        // The Page Rotation EDITOR items (`GetPageEditor().Rotation`
        // — the CURRENT PAGE's stored rotation, radio).
        {
            let pr = gio::SimpleAction::new("page-rotation", Some(glib::VariantTy::STRING));
            let state = Rc::downgrade(self);
            pr.connect_activate(move |_, value| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let Some(name) = value.and_then(|v| v.get::<String>()) else {
                    return;
                };
                let rot = match name.as_str() {
                    "none" => cr_core::model::enums::ImageRotation::None,
                    "90" => cr_core::model::enums::ImageRotation::Rotate90,
                    "180" => cr_core::model::enums::ImageRotation::Rotate180,
                    "270" => cr_core::model::enums::ImageRotation::Rotate270,
                    _ => return,
                };
                let Some(view) = sh.reader.current_view() else {
                    return;
                };
                let display = view.current_page();
                view.set_page_rotation_for(display, rot);
                if let Some(provider) = view.provider_index_of(display) {
                    sh.edit_open_book(|b| b.info.update_page_rotation(provider, rot));
                }
                sh.sync_enabled();
            });
            group.add_action(&pr);
        }
        // The Page Type EDITOR items (`GetPageEditor().PageType` —
        // the CURRENT PAGE's type, radio; the parameter is the type
        // VALUE).
        {
            let pt = gio::SimpleAction::new("page-type", Some(glib::VariantTy::STRING));
            let state = Rc::downgrade(self);
            pt.connect_activate(move |_, value| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let Some(name) = value.and_then(|v| v.get::<String>()) else {
                    return;
                };
                let Some((_, pt_value)) = crate::dialogs::book_editor::PAGE_TYPE_ITEMS
                    .iter()
                    .find(|(_, v)| name == v.0.to_string())
                else {
                    return;
                };
                let Some((_, provider)) = sh.current_provider_page() else {
                    return;
                };
                sh.edit_open_book(|b| b.info.update_page_type(provider, *pt_value));
                sh.sync_enabled();
            });
            group.add_action(&pt);
        }
        {
            let auto = gio::SimpleAction::new_stateful("auto-rotate", None, &false.to_variant());
            let state = state.clone();
            auto.connect_activate(move |action, _| {
                if let Some(sh) = state.upgrade() {
                    sh.reader.dispatch_current("AutoRotate");
                    let current = action
                        .state()
                        .and_then(|v| v.get::<bool>())
                        .unwrap_or(false);
                    action.set_state(&(!current).to_variant());
                }
            });
            group.add_action(&auto);
        }
        // MinimalGui / FullScreen checks (the state lives in the
        // reader shell / the root window; sync reads it back).
        self.add_check(&group, "minimal-gui", false, |sh| {
            sh.reader.dispatch_current("ToggleMenu");
        });
        self.add_check(&group, "full-screen", false, |sh| {
            sh.reader.dispatch_current("ToggleFullScreen");
        });
        self.add_simple(&group, "undock-reader", |sh| {
            sh.reader.dispatch_current("ToggleUndockReader")
        });
        self.add_simple(&group, "magnifier", |sh| {
            sh.reader.dispatch_current("ToggleMagnify")
        });

        // --- Help ---
        self.add_simple(&group, "about", ShellState::show_about);

        // --- The mainKeys shell commands ---
        self.add_simple(&group, "focus-search", |sh| {
            sh.search.grab_focus();
        });
        // `tsQuickSearch` (the navigator's own search toggle; the
        // check state = the box visibility — `commands.Add(
        // ToggleQuickSearch, true, () => quickSearchPanel.Visible)`).
        {
            let state = state.clone();
            let action = gio::SimpleAction::new_stateful(
                "toggle-navigator-search",
                None,
                &false.to_variant(),
            );
            action.connect_activate(move |_, _| {
                if let Some(sh) = state.upgrade() {
                    sh.navigator.toggle_search();
                    sh.sync_enabled();
                }
            });
            group.add_action(&action);
            self.actions
                .borrow_mut()
                .insert("toggle-navigator-search", action);
        }

        // --- The Pages panel (the T7 `ComicPagesView.toolStrip`) ---
        // The page-grid mode radios (the Views drop rows).
        let pages_mode = gio::SimpleAction::new_stateful(
            "pages-view-mode",
            Some(glib::VariantTy::STRING),
            &"thumbnail".to_variant(),
        );
        {
            let state = state.clone();
            pages_mode.connect_activate(move |_, value| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let name = value.and_then(|v| v.get::<String>()).unwrap_or_default();
                sh.pages
                    .set_mode(super::pages_view::PagesMode::from_action_name(&name));
                sh.sync_enabled();
            });
        }
        group.add_action(&pages_mode);
        self.actions
            .borrow_mut()
            .insert("pages-view-mode", pages_mode);

        self.window.insert_action_group("win", Some(&group));
        ShellState::register_accels(&self.app);
        ShellState::install_shifted_key_fallback(self);
        self.sync_enabled();
    }

    /// The shifted-symbol key fallback (see
    /// `commands::shifted_symbol_command`): a window key controller
    /// resolves the hardware keycode to its UNSHIFTED keyval and
    /// fires the command the accel table cannot match when Shift
    /// rewrote the symbol (Alt+Shift+4 → '¤', Ctrl+Shift+7 → '/').
    fn install_shifted_key_fallback(self: &Rc<ShellState>) {
        let controller = gtk4::EventControllerKey::new();
        let state = Rc::downgrade(self);
        controller.connect_key_pressed(move |_c, key, keycode, state_bits| {
            let Some(sh) = state.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let mask = gtk4::accelerator_get_default_mod_mask();
            let mods = state_bits & mask;
            let ctrl = mods.contains(gtk4::gdk::ModifierType::CONTROL_MASK);
            let shift = mods.contains(gtk4::gdk::ModifierType::SHIFT_MASK);
            let alt = mods.contains(gtk4::gdk::ModifierType::ALT_MASK);
            // The unshifted keyval: the level-0 mapping of the
            // hardware keycode (group 0 preferred).
            let unshifted = gtk4::prelude::WidgetExt::display(&sh.window)
                .map_keycode(keycode)
                .and_then(|entries| {
                    let pick = |group: Option<i32>| {
                        entries
                            .iter()
                            .find(|(k, _)| k.level() == 0 && group.is_none_or(|g| k.group() == g))
                            .map(|(_, v)| *v)
                    };
                    pick(Some(0)).or_else(|| pick(None))
                });
            let Some(unshifted) = unshifted else {
                return glib::Propagation::Proceed;
            };
            // Only when Shift rewrote the symbol — otherwise the real
            // accelerator handles the combo (never double-fire).
            if unshifted == key {
                return glib::Propagation::Proceed;
            }
            let Some(action) = crate::commands::shifted_symbol_command(ctrl, shift, alt, unshifted)
            else {
                return glib::Propagation::Proceed;
            };
            let _ = gtk4::prelude::WidgetExt::activate_action(
                &sh.window,
                &format!("win.{action}"),
                None,
            );
            sh.sync_enabled();
            glib::Propagation::Stop
        });
        self.window.add_controller(controller);
    }

    /// The menubar wiring (Phase 5.5 T3): the chrome-visibility hook
    /// (fullscreen enter/leave, MinimalGui). The `AutoHideMainMenu`
    /// Alt-alone reveal was REMOVED (the T9 user decision: the
    /// menubar shows always — no reveal shortcut).
    fn install_menubar_keys(self: &Rc<ShellState>) {
        // Chrome changes (fullscreen notify, MinimalGui toggles) →
        // the menubar rule re-evaluates with the sync.
        {
            let state = Rc::downgrade(self);
            self.reader.set_on_chrome_change(move |_visible| {
                if let Some(sh) = state.upgrade() {
                    sh.sync_enabled();
                }
            });
        }
    }

    /// Registers one STATEFUL check action (`CommandMapper.Add(...
    /// checkLambda)` parity): the handler runs, the check state
    /// follows in the next sync (GTK renders stateful-action state
    /// on menu items).
    fn add_check<F: Fn(&Rc<ShellState>) + 'static>(
        self: &Rc<ShellState>,
        group: &gio::SimpleActionGroup,
        name: &'static str,
        initial: bool,
        f: F,
    ) {
        let action = gio::SimpleAction::new_stateful(name, None, &initial.to_variant());
        let state = Rc::downgrade(self);
        action.connect_activate(move |_, _| {
            if let Some(sh) = state.upgrade() {
                f(&sh);
                sh.sync_enabled();
            }
        });
        group.add_action(&action);
        self.actions.borrow_mut().insert(name, action);
    }

    /// A stub action that stays disabled until its feature task
    /// lands (the name is the note).
    fn add_disabled(&self, group: &gio::SimpleActionGroup, name: &'static str) {
        let action = gio::SimpleAction::new(name, None);
        action.set_enabled(false);
        group.add_action(&action);
        self.actions.borrow_mut().insert(name, action);
    }

    /// The accelerator registration (`gtk_application_set_accels_for_
    /// action`); the radio values ride detailed action names.
    fn register_accels(app: &Application) {
        for command in crate::commands::COMMANDS {
            if !command.accels.is_empty() {
                app.set_accels_for_action(&format!("win.{}", command.action), command.accels);
            }
        }
        for (value, accel) in crate::commands::FIT_MODES {
            if !accel.is_empty() {
                app.set_accels_for_action(&format!("win.page-fit::{value}"), &[*accel]);
            }
        }
        for (value, accel) in crate::commands::LAYOUT_MODES {
            if !accel.is_empty() {
                app.set_accels_for_action(&format!("win.page-layout::{value}"), &[*accel]);
            }
        }
    }
}

/// `ComicInfo.UpdateBookmark(page, bookmark)`: an empty name clears
/// (the C# writes an empty bookmark; the model stores `None`),
/// a change only when the entry really changes.
fn update_bookmark_entry(book: &mut ComicBook, provider: usize, name: &str) {
    let Some(p) = book.info.pages.get_mut(provider) else {
        return;
    };
    let old = p.bookmark.clone().unwrap_or_default();
    let changed = (!old.is_empty() || !name.is_empty()) && old != name;
    if changed {
        p.bookmark = if name.is_empty() {
            None
        } else {
            Some(name.into())
        };
    }
}

/// `ImageFitMode` → the `win.page-fit` state name.
fn fit_action_name(mode: ImageFitMode) -> &'static str {
    match mode {
        ImageFitMode::Original => "original",
        ImageFitMode::Fit => "fit-all",
        ImageFitMode::FitWidth => "fit-width",
        ImageFitMode::FitWidthAdaptive => "fit-width-adaptive",
        ImageFitMode::FitHeight => "fit-height",
        ImageFitMode::BestFit => "fit-best",
    }
}

/// `PageLayoutMode` → the `win.page-layout` state name.
fn layout_action_name(mode: PageLayoutMode) -> &'static str {
    match mode {
        PageLayoutMode::Single => "single",
        PageLayoutMode::Double => "double",
        PageLayoutMode::DoubleAdaptive => "double-adaptive",
        PageLayoutMode::Continuous => "continuous",
    }
}

/// The folder scan on a worker thread (the C# wraps the provider
/// refresh in `AutomaticProgressDialog` with a Cancel button; the
/// port keeps the UI free instead — the ADR-019 worker pattern).
/// Each request bumps the generation; a stale result drops on
/// arrival, so a fast folder switch never lands an older scan.
fn scan_folder_async(state: &std::rc::Weak<ShellState>, path: String, include_sub: bool) {
    let state = std::rc::Weak::clone(state);
    let Some(sh) = state.upgrade() else {
        return;
    };
    sh.folder_scan_gen.set(sh.folder_scan_gen.get() + 1);
    let gen = sh.folder_scan_gen.get();
    let (tx, rx) = std::sync::mpsc::channel::<(u64, String, Vec<ComicBook>)>();
    std::thread::Builder::new()
        .name("Folder Scan".into())
        .spawn(move || {
            let name = std::path::Path::new(&path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_else(|| "File System".into());
            let books =
                super::folder_tree::folder_book_list(std::path::Path::new(&path), include_sub);
            let _ = tx.send((gen, name, books));
        })
        .expect("spawn Folder Scan");
    glib::timeout_add_local(std::time::Duration::from_millis(40), move || {
        let Some(sh) = state.upgrade() else {
            return glib::ControlFlow::Break;
        };
        // Bind the recv result before matching (the scrutinee-borrow
        // lesson).
        let received = rx.try_recv();
        match received {
            Ok((g, name, books)) => {
                if g == sh.folder_scan_gen.get() {
                    *sh.current_folder_name.borrow_mut() = name;
                    sh.folders_view.set_books(books);
                    sh.sync_enabled();
                }
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => glib::ControlFlow::Break,
        }
    });
}

/// The book context menu (the C# `contextMenuItems`). Every command
/// reads `item_view.selection_ids()`: the right-click already applied
/// the C# `UpdateSelectionFromMouse` rule in `emit_context`, so the
/// selection IS the target set (an unselected target replaced the
/// selection; a selected target kept the multi-selection).
/// The Remove from Library flow — the C# remove flow asks: remove
/// from the list only, or from the Library, and whether to move the
/// files to the trash. The permanent-delete option (no trash) is the
/// ADR-045 port addition. The context menu and the Delete key both
/// land here.
fn run_remove_books(state: &std::rc::Weak<ShellState>) {
    let Some(sh) = state.upgrade() else {
        return;
    };
    let ids = sh.item_view.selection_ids();
    if ids.is_empty() {
        return;
    }
    let count = ids.len();
    let confirm = gtk4::MessageDialog::builder()
        .transient_for(&sh.window)
        .modal(true)
        .title("Remove Books")
        .text(format!("Remove {count} book(s) from the current list?"))
        .message_type(gtk4::MessageType::Question)
        .buttons(gtk4::ButtonsType::OkCancel)
        .build();
    let also_files = gtk4::CheckButton::with_label("Also delete the files");
    let permanent = gtk4::CheckButton::with_label("Delete permanently (do not use the trash)");
    permanent.set_sensitive(false);
    also_files
        .bind_property("active", &permanent, "sensitive")
        .sync_create()
        .build();
    // The MessageDialog message_area is a Box; reach
    // it through the child hierarchy.
    let area = confirm
        .child()
        .and_downcast::<gtk4::Box>()
        .and_then(|vbox| vbox.first_child().and_downcast::<gtk4::Box>());
    if let Some(area) = area {
        area.append(&also_files);
        area.append(&permanent);
    }
    let refresh_state = state.clone();
    let ids_for_ok = ids.clone();
    confirm.connect_response(move |dlg, resp| {
        let remove_files = also_files.is_active();
        let permanent_delete = permanent.is_active();
        dlg.destroy();
        if resp != gtk4::ResponseType::Ok {
            return;
        }
        let Some(sh) = refresh_state.upgrade() else {
            return;
        };
        // The delete rides the "Remove Books" worker (the ADR-019
        // shape): the per-file unlink/gio-trash ran INLINE here and
        // froze the UI — MEASURED 22.9 s for 792 unlinks over CIFS.
        // The pump lands the book removals in batches; the completion
        // callback refreshes ONCE and reports the failed deletes.
        if library::remove_books_in_flight() {
            crate::trace::trace("remove-books: a bulk delete is already running — ignored");
            return;
        }
        let sh_w = std::rc::Rc::downgrade(&sh);
        library::remove_books_async(
            ids_for_ok.clone(),
            remove_files,
            permanent_delete,
            true,
            move |outcome| {
                crate::trace::trace(format!(
                    "remove-books: job landed — removed {} failed {} canceled {}",
                    outcome.removed, outcome.failed, outcome.canceled
                ));
                if let Some(sh) = sh_w.upgrade() {
                    // The C# `FailedDeleteBooks` message.
                    if outcome.failed > 0 {
                        let err = gtk4::MessageDialog::builder()
                            .transient_for(&sh.window)
                            .modal(true)
                            .title("comicrust")
                            .text("Some files could not be deleted (maybe they are in use)!")
                            .message_type(gtk4::MessageType::Info)
                            .buttons(gtk4::ButtonsType::Ok)
                            .build();
                        err.connect_response(|d, _| d.close());
                        err.present();
                    }
                    if outcome.removed > 0 {
                        sh.refresh_view_from_list();
                    }
                }
            },
        );
    });
    confirm.present();
}

fn show_context_menu(state: &std::rc::Weak<ShellState>, target: Option<CrGuid>, x: f64, y: f64) {
    let Some(shell) = state.upgrade() else {
        return;
    };
    if shell.item_view.selection_len() == 0 {
        return;
    }
    let popover = gtk4::Popover::new();
    // No pointing arrow (the C# ContextMenuStrip shape).
    popover.set_has_arrow(false);
    let box_ = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    box_.set_margin_top(4);
    box_.set_margin_bottom(4);
    box_.set_margin_start(4);
    box_.set_margin_end(4);

    let window = shell.window.clone();
    let incoming_view = shell.is_incoming_view();
    let missing_issues_view = shell
        .current_list
        .borrow()
        .is_some_and(|id| super::navigator::is_missing_issues_id(&id));
    let mutations_enabled = !cr_engine::incoming_transaction::operation_active();
    let add_item = |box_: &gtk4::Box, label: &str, action: &'static str| {
        let popover = popover.clone();
        let state = state.clone();
        let window = window.clone();
        let button = crate::widgets::menu_item_button(label);
        button.connect_clicked(move |_| {
            popover.popdown();
            let Some(sh) = state.upgrade() else {
                return;
            };
            match action {
                "incoming-compare" => sh.compare_incoming(),
                "incoming-preview" => sh.run_incoming_menu_adoption(true),
                "incoming-adopt" => sh.run_incoming_menu_adoption(false),
                "incoming-discard" => sh.discard_incoming(),
                "incoming-cv-refresh" => sh.refresh_incoming_from_comic_vine(),
                "find-in-incoming" => sh.find_in_incoming(),
                "open" => {
                    if let Some(id) = target {
                        if let Some(path) = library::book_path(&id) {
                            sh.open_comic(Path::new(&path));
                        }
                    }
                }
                "reveal" => {
                    if let Some(id) = target {
                        if let Some(path) = library::book_path(&id) {
                            // The C# `IsLinked` gate: a fileless book
                            // has no folder to reveal (an empty path
                            // would resolve to the app's working
                            // directory).
                            if !path.is_empty() {
                                let _ = std::process::Command::new("xdg-open")
                                    .arg(Path::new(&path).parent().unwrap_or(Path::new("/")))
                                    .spawn();
                            }
                        }
                    }
                }
                "edit" => {
                    // The bulk editor over the selection (the C#
                    // `MultipleComicBooksDialog`).
                    let ids = sh.item_view.selection_ids();
                    if ids.is_empty() {
                        return;
                    }
                    let books = ShellState::books_by_ids(&ids);
                    if books.is_empty() {
                        return;
                    }
                    sh.open_bulk_editor(books);
                }
                "update-file" => {
                    // The manual write (the C# `AddBookToFileUpdate(cb,
                    // alwaysWrite: true)`): the UpdateComicFiles gate
                    // still applies. The writes ride the Info Writer
                    // worker; the batch collector reports when the
                    // last write lands (the C# queue parity — the UI
                    // never blocks on an archive rewrite).
                    let ids = sh.item_view.selection_ids();
                    if ids.is_empty() {
                        return;
                    }
                    let total = ids.len();
                    let remaining = std::rc::Rc::new(std::cell::Cell::new(total));
                    let written = std::rc::Rc::new(std::cell::Cell::new(0usize));
                    let errors = std::rc::Rc::new(std::cell::RefCell::new(Vec::<String>::new()));
                    let sh_w = std::rc::Rc::downgrade(&sh);
                    for id in &ids {
                        let remaining = std::rc::Rc::clone(&remaining);
                        let written = std::rc::Rc::clone(&written);
                        let errors = std::rc::Rc::clone(&errors);
                        let sh_w = sh_w.clone();
                        library::update_book_file_async(
                            id,
                            true,
                            Some(Box::new(move |result| {
                                match result {
                                    Ok(true) => written.set(written.get() + 1),
                                    Ok(false) => {}
                                    Err(e) => errors.borrow_mut().push(e),
                                }
                                if remaining.get() == 1 {
                                    // The last write of the batch.
                                    if let Some(sh) = sh_w.upgrade() {
                                        let errs = errors.borrow();
                                        if let Some(last) = errs.last() {
                                            show_failure_dialog(
                                                &sh.window,
                                                "Update Book Files",
                                                &format!(
                                                    "{}/{} written. Last error: {last}",
                                                    written.get(),
                                                    total
                                                ),
                                            );
                                        }
                                        sh.refresh_view_from_list();
                                    }
                                }
                                remaining.set(remaining.get() - 1);
                            })),
                        );
                    }
                }
                "rescan" => {
                    // The "Rescan Book File(s)" command (ADR-036): ONE
                    // explicit request over the selected books' linked
                    // files, with a one-shot forced retry — a known-bad
                    // unchanged file re-reads (that is the point of the
                    // command; the C# Refresh parity is the C# scan
                    // without the port's known-bad skip).
                    let paths = library::book_paths_for_ids(&sh.item_view.selection_ids());
                    if paths.is_empty() {
                        return;
                    }
                    let weak = std::rc::Rc::downgrade(&sh);
                    library::scan_files(&paths, "selected book(s)", true, move |_| {
                        if let Some(sh) = weak.upgrade() {
                            sh.refresh_view_from_list();
                            sh.sync_enabled();
                            sh.report_scan_problems();
                        }
                    });
                }
                "fill-missing" => {
                    // "Fill Missing Issues" (Phase 15 T7): the Comic
                    // Vine cache knows every issue of the volume, so
                    // the gap against the owned numbers becomes
                    // fileless books.
                    sh.fill_missing_issues();
                }
                "link-series-from-cache" => {
                    // "Link Series from Cache": one Comic Vine search
                    // (skipped if a vote already exists), then a local
                    // cache-only match links the rest of the CURRENT
                    // VIEW's copies of the series.
                    sh.link_series_from_cache();
                }
                "export" => {
                    // The export dialog over the selection (the C#
                    // `ConvertComic`).
                    let ids = sh.item_view.selection_ids();
                    if ids.is_empty() {
                        return;
                    }
                    let books: Vec<cr_core::model::comic_book::ComicBook> = {
                        let lib = library::session();
                        let l = lib.borrow();
                        l.database()
                            .books
                            .iter()
                            .filter(|b| ids.contains(&b.id))
                            .cloned()
                            .collect()
                    };
                    if books.is_empty() {
                        return;
                    }
                    let captions: Vec<String> =
                        books.iter().map(cr_engine::display_text::caption).collect();
                    // `Program.Settings.CurrentExportSetting` —
                    // session-only here (the persistence joins when
                    // the settings schema carries the export block).
                    let session_default = library::last_export_setting().unwrap_or_default();
                    let refresh_state = state.clone();
                    crate::dialogs::export::show_export_dialog(
                        &window,
                        books,
                        captions,
                        session_default,
                        move |result| {
                            if let Some(r) = result {
                                library::remember_export_setting(r.setting);
                                if let Some(sh) = refresh_state.upgrade() {
                                    sh.refresh_view_from_list();
                                }
                            }
                        },
                    );
                }
                "scrape" => {
                    // The Comic Vine Scraper wizard over the selection.
                    sh.open_scrape();
                }
                "organize" => {
                    // The Library Organizer: config then run.
                    sh.open_organize();
                }
                "organize-quick" => {
                    // The Library Organizer Quick: no config step.
                    sh.open_organize_quick();
                }
                "remove" => {
                    run_remove_books(&state);
                }
                "properties" => {
                    // The selection — the C# opens the dialog over the
                    // selected books (prev/next when > 1).
                    let ids = sh.item_view.selection_ids();
                    if ids.is_empty() {
                        return;
                    }
                    // List order for the prev/next walk.
                    let books = ShellState::books_by_ids(&ids);
                    if books.is_empty() {
                        return;
                    }
                    sh.open_editor(books);
                }
                "select-worst-duplicates" => {
                    // PORT ADDITION (ADR-044): rank the duplicate
                    // groups of the current view, select the worst.
                    sh.select_worst_duplicates();
                }
                _ => {}
            }
        });
        box_.append(&button);
        button
    };
    if missing_issues_view {
        let config = library::incoming_config();
        let has_profile = library::organize_settings().profiles.iter().any(|profile| {
            profile.mode == cr_organize::profile::MODE_MOVE
                && profile.name == config.find_in_incoming_profile
        });
        add_item(&box_, "Find in Incoming", "find-in-incoming")
            .set_sensitive(has_profile && mutations_enabled);
    } else if incoming_view {
        add_item(&box_, "Compare", "incoming-compare");
        let current = *shell.current_list.borrow();
        if current.is_some_and(show_select_worst_for_incoming_view) {
            add_item(&box_, "Select Worst Duplicates", "select-worst-duplicates");
        }
        let has_move_profile = library::organize_settings()
            .profiles
            .iter()
            .any(|profile| profile.mode == cr_organize::profile::MODE_MOVE);
        let gap_fills = shell
            .current_list
            .borrow()
            .is_some_and(|id| id == super::navigator::IncomingView::GapFills.id());
        let has_gap_profile = shell.configured_gap_profile().is_some();
        let adoption_enabled = if gap_fills {
            has_gap_profile
        } else {
            has_move_profile
        };
        add_item(&box_, "Preview Adoption", "incoming-preview").set_sensitive(adoption_enabled);
        add_item(&box_, "Adopt", "incoming-adopt")
            .set_sensitive(adoption_enabled && mutations_enabled);
        add_item(&box_, "Discard", "incoming-discard").set_sensitive(mutations_enabled);
        let selected = shell.selected_incoming_books();
        let can_refresh = library::scraper_config().has_api_key()
            && library::selected_incoming_has_volume_id(&selected);
        add_item(&box_, "Refresh from Comic Vine", "incoming-cv-refresh")
            .set_sensitive(can_refresh);
    } else {
        add_item(&box_, "Open", "open");
        add_item(&box_, "Reveal in File Manager", "reveal");
        add_item(&box_, "Edit…", "edit");
        add_item(&box_, "Update Book File(s)", "update-file");
        add_item(&box_, "Rescan Book File(s)", "rescan");
        add_item(&box_, "Export…", "export");
        add_item(&box_, "Scrape from Comic Vine…", "scrape");
        add_item(&box_, "Library Organizer…", "organize");
        add_item(&box_, "Library Organizer (Quick)", "organize-quick");
        add_item(&box_, "Fill Missing Issues…", "fill-missing");
        add_item(&box_, "Link Series from Cache…", "link-series-from-cache");
        add_item(&box_, "Select Worst Duplicates", "select-worst-duplicates");
        add_item(&box_, "Remove from Library", "remove");
        add_item(&box_, "Properties…", "properties");
    }
    popover.set_child(Some(&box_));
    popover.set_parent(&window);
    popover.connect_closed(|p| p.unparent());
    let rect = gtk4::gdk::Rectangle::new(x as i32, y as i32 + 8, 1, 1);
    popover.set_pointing_to(Some(&rect));
    if let Some(sh) = state.upgrade() {
        *sh.context_drop.borrow_mut() = Some(popover.clone());
    }
    crate::trace::trace(format!(
        "context: popup at ({x}, {y}) — scroll before popup {}",
        state
            .upgrade()
            .map(|sh| sh.item_view.probe_scroll_value())
            .unwrap_or(-1.0)
    ));
    popover.popup();
}

fn show_select_worst_for_incoming_view(id: CrGuid) -> bool {
    id == super::navigator::IncomingView::Duplicates.id()
        || id == super::navigator::IncomingView::IncomingDuplicates.id()
}

/// The Files view's context menu (`ItemContextMenuStrip` → the
/// `FolderComicListProvider.RemoveBooks` shape): Open, Reveal, and
/// Move to Recycle Bin. The remove asks the C# question ("Are you
/// sure you want to move these files to the Recycle Bin?") with the
/// "Additionally remove the books from the Library" option
/// (`RemoveFilesfromDatabase`); the port's is-file guard applies
/// (the Phase 7 audit — the C# `ShellFile.DeleteFile` would trash a
/// FOLDER path for folder comics).
fn show_folder_context_menu(
    state: &std::rc::Weak<ShellState>,
    target: Option<CrGuid>,
    x: f64,
    y: f64,
) {
    let popover = gtk4::Popover::new();
    popover.set_has_arrow(false);
    let box_ = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    box_.set_margin_top(4);
    box_.set_margin_bottom(4);
    box_.set_margin_start(4);
    box_.set_margin_end(4);
    let window = state
        .upgrade()
        .map(|sh| sh.window.clone())
        .expect("shell alive while the menu opens");
    // The selected folder book paths (the right-click already applied
    // the C# selection rule in `emit_context` — the selection IS the
    // target set).
    let target_paths = |sh: &ShellState| -> Vec<(CrGuid, String)> {
        let view = sh.folders_view.view_state();
        let ids: Vec<CrGuid> = view.selection_snapshot().into_iter().collect();
        view.books()
            .iter()
            .filter(|b| ids.contains(&b.id))
            .map(|b| (b.id, b.file_path.clone()))
            .collect()
    };
    let add_item = |box_: &gtk4::Box, label: &str, action: &'static str| {
        let popover = popover.clone();
        let state = state.clone();
        let window = window.clone();
        let button = crate::widgets::menu_item_button(label);
        button.connect_clicked(move |_| {
            popover.popdown();
            let Some(sh) = state.upgrade() else {
                return;
            };
            match action {
                "open" => {
                    if let Some(id) = target {
                        let view = sh.folders_view.view_state();
                        if let Some(book) = view.books().iter().find(|b| b.id == id) {
                            sh.open_comic(Path::new(&book.file_path));
                        }
                    }
                }
                "reveal" => {
                    for (_, path) in target_paths(&sh) {
                        if !path.is_empty() {
                            let _ = std::process::Command::new("xdg-open")
                                .arg(Path::new(&path).parent().unwrap_or(Path::new("/")))
                                .spawn();
                            break;
                        }
                    }
                }
                "remove" => {
                    // The C# `RemoveBooks(ask: true)`.
                    let paths = target_paths(&sh);
                    if paths.is_empty() {
                        return;
                    }
                    let confirm = gtk4::MessageDialog::builder()
                        .transient_for(&window)
                        .modal(true)
                        .title("Remove Books")
                        .text("Are you sure you want to move these files to the Recycle Bin?")
                        .message_type(gtk4::MessageType::Question)
                        .buttons(gtk4::ButtonsType::OkCancel)
                        .build();
                    let also_library = gtk4::CheckButton::with_label(
                        "Additionally remove the books from the Library (all information not stored in the files will be lost)",
                    );
                    also_library.set_active(library::settings().borrow().remove_files_from_database);
                    // ADR-045 (the port addition): the C# folder flow
                    // deletes through the shell (recycle bin only).
                    let permanent = gtk4::CheckButton::with_label(
                        "Delete permanently (do not use the trash)",
                    );
                    let area = confirm
                        .child()
                        .and_downcast::<gtk4::Box>()
                        .and_then(|vbox| vbox.first_child().and_downcast::<gtk4::Box>());
                    if let Some(area) = area {
                        area.append(&also_library);
                        area.append(&permanent);
                    }
                    let refresh_state = state.clone();
                    confirm.connect_response(move |dlg, resp| {
                        let remove_from_library = also_library.is_active();
                        let permanent_delete = permanent.is_active();
                        dlg.destroy();
                        if resp != gtk4::ResponseType::Ok {
                            return;
                        }
                        let Some(sh) = refresh_state.upgrade() else {
                            return;
                        };
                        library::settings().borrow_mut().remove_files_from_database =
                            remove_from_library;
                        // The file deletion rides the "Remove Books"
                        // worker (the same MEASURED main-thread freeze
                        // as the browser flow); the removal, the
                        // failure dialog and the rescan stay here and
                        // run from the completion callback.
                        if library::remove_books_in_flight() {
                            crate::trace::trace("remove-books: a bulk delete is already running — ignored");
                            return;
                        }
                        let file_paths: Vec<String> =
                            paths.iter().map(|(_, p)| p.clone()).collect();
                        let sh_w = std::rc::Rc::downgrade(&sh);
                        let remove_from_library_cb = remove_from_library;
                        let paths_cb = paths.clone();
                        let refresh_cb = refresh_state.clone();
                        library::delete_files_async(file_paths, permanent_delete, move |outcome| {
                            let Some(sh) = sh_w.upgrade() else {
                                return;
                            };
                            let deleted = outcome.failed == 0;
                            if remove_from_library_cb
                                && !outcome.canceled
                                && !cr_engine::incoming_transaction::operation_active()
                            {
                                // `Program.Database.Books.RemoveRange(books)`
                                // — the library books at the same paths.
                                let lib = library::session();
                                let removed: Vec<CrGuid> = {
                                    let l = lib.borrow();
                                    l.database()
                                        .books
                                        .iter()
                                        .filter(|b| {
                                            paths_cb.iter().any(|(_, p)| *p == b.file_path)
                                        })
                                        .map(|b| b.id)
                                        .collect()
                                };
                                let mut l = lib.borrow_mut();
                                let before = l.database().books.len();
                                l.database_mut().books.retain(|b| {
                                    !paths_cb
                                        .iter()
                                        .any(|(_, p)| *p == b.file_path)
                                });
                                if l.database().books.len() != before {
                                    // A mid-scan removal must survive the
                                    // scan's landing merge.
                                    for id in &removed {
                                        library::record_scan_removal(id);
                                    }
                                    l.mark_dirty();
                                    crate::gauges::invalidate();
                                }
                            }
                            // The failed-delete message (the C#
                            // `FailedDeleteBooks`).
                            if !deleted {
                                let err = gtk4::MessageDialog::builder()
                                    .transient_for(&sh.window)
                                    .modal(true)
                                    .title("comicrust")
                                    .text("Some books could not be deleted (maybe they are in use)!")
                                    .message_type(gtk4::MessageType::Info)
                                    .buttons(gtk4::ButtonsType::Ok)
                                    .build();
                                err.connect_response(|d, _| d.close());
                                err.present();
                            }
                            // The provider refreshes on the Path change
                            // (the C# `BookListChanged`) — rescan on the
                            // worker (the same async path).
                            if let Some(folder) = sh.folders_tree.current_folder() {
                                let include_sub =
                                    library::settings().borrow().explorer_include_sub_folders;
                                let weak = refresh_cb.clone();
                                scan_folder_async(&weak, folder, include_sub);
                            }
                            sh.sync_enabled();
                        });
                    });
                    confirm.present();
                }
                _ => {}
            }
        });
        box_.append(&button);
    };
    add_item(&box_, "Open", "open");
    add_item(&box_, "Reveal in File Manager", "reveal");
    add_item(&box_, "Move to Recycle Bin", "remove");
    popover.set_child(Some(&box_));
    popover.set_parent(&window);
    popover.connect_closed(|p| p.unparent());
    let rect = gtk4::gdk::Rectangle::new(x as i32, y as i32 + 8, 1, 1);
    popover.set_pointing_to(Some(&rect));
    popover.popup();
}

/// `ComicBookAllPropertiesMatcher.Create` for the quick search: a
/// contains over the All field set; MATCH/NOT text parses as a full
/// query (`UpdateQuickFilter`).
/// One registered value matcher as raw XML data (the C#
/// `new ComicBookXMatcher { ... }` object initializers).
fn raw_value_matcher(
    type_name: &str,
    op: i32,
    value: &str,
    value2: &str,
    not: bool,
    option: Option<&str>,
) -> cr_core::database::list_items::ComicBookMatcher {
    cr_core::database::list_items::ComicBookMatcher::Value(
        cr_core::database::list_items::ValueMatcher {
            type_name: type_name.into(),
            not,
            match_operator: op,
            match_value: value.into(),
            match_value_2: value2.into(),
            option: option.map(Into::into),
            ..Default::default()
        },
    )
}

/// `ComicBookAllPropertiesMatcher.Create(text, 3, option, show,
/// comic)` + the `ShowOnlyDuplicates` extra of `GetCurrentMatcher` —
/// the composed browser filter (`UpdateQuickFilter` parity):
///
/// - a MATCH/NOT text parses as a full query, but ONLY for the All
///   scope (the C# `UpdateQuickFilter` gate); the view filters do
///   NOT apply to a parsed query (they live in the Create path).
/// - otherwise: [read-state filter][comic-type filter][text
///   matcher] as an And group (each part only when active).
/// - duplicates-only adds a `ComicBookDuplicateMatcher` on top
///   (always applied — it is a GetCurrentMatcher member, not a
///   quickFilter member).
/// - all inactive + no text → no filter (the C# `Create` returns
///   null).
fn compose_quick_filter(
    text: &str,
    scope: &str,
    show: &str,
    ctype: &str,
    dups: bool,
) -> Option<Matcher> {
    use cr_core::model::enums::MatcherMode;
    let engine = cr_core::settings::EngineConfiguration::global();
    let read_at = engine.is_read_completion_percentage.to_string();
    let not_read_at = engine.is_not_read_completion_percentage.to_string();
    let reading_from = (engine.is_not_read_completion_percentage + 1).to_string();
    let reading_to = (engine.is_read_completion_percentage - 1).to_string();

    let mut quick: Option<Matcher> = None;
    let trimmed = text.trim();
    let upper = trimmed.to_ascii_uppercase();
    if scope == "all" && (upper.starts_with("NOT") || upper.starts_with("MATCH")) {
        let mut t = cr_engine::tokenizer::Tokenizer::new(text);
        if let Ok(group) = cr_engine::matcher::query::parse_group_query(&mut t) {
            quick = Some(Matcher::Group(group));
        }
    }
    if quick.is_none() {
        let mut list: Vec<cr_core::database::list_items::ComicBookMatcher> = Vec::new();
        match show {
            "read" => list.push(raw_value_matcher(
                "ComicBookReadPercentageMatcher",
                cr_engine::matcher::spec::ops::NUM_GREATER as i32,
                &read_at,
                "",
                false,
                None,
            )),
            "reading" => list.push(raw_value_matcher(
                "ComicBookReadPercentageMatcher",
                cr_engine::matcher::spec::ops::NUM_IN_RANGE as i32,
                &reading_from,
                &reading_to,
                false,
                None,
            )),
            "unread" => list.push(raw_value_matcher(
                "ComicBookReadPercentageMatcher",
                cr_engine::matcher::spec::ops::NUM_LESSER as i32,
                &not_read_at,
                "",
                false,
                None,
            )),
            _ => {}
        }
        match ctype {
            "books" => list.push(raw_value_matcher(
                "ComicBookFileMatcher",
                cr_engine::matcher::spec::ops::STR_EQUALS as i32,
                "",
                "",
                true,
                None,
            )),
            "fileless" => list.push(raw_value_matcher(
                "ComicBookFileMatcher",
                cr_engine::matcher::spec::ops::STR_EQUALS as i32,
                "",
                "",
                false,
                None,
            )),
            _ => {}
        }
        if !trimmed.is_empty() {
            // The C# `Create` passes operator 3 (ContainsAll) and the
            // RAW (untrimmed) search text. The option carries the C#
            // enum name (the action value is the lowercase id).
            let option = match scope {
                "series" => "Series",
                "writer" => "Writer",
                "artists" => "Artists",
                "descriptive" => "Descriptive",
                "catalog" => "Catalog",
                "file" => "File",
                _ => "All",
            };
            list.push(raw_value_matcher(
                "ComicBookAllPropertiesMatcher",
                cr_engine::matcher::spec::ops::STR_CONTAINS_ALL as i32,
                text,
                "",
                false,
                Some(option),
            ));
        }
        quick = match list.len() {
            0 => None,
            1 => Matcher::from_raw(&list[0]),
            _ => {
                let raws = list.iter().filter_map(Matcher::from_raw).collect();
                Some(Matcher::Group(cr_engine::matcher::tree::GroupMatcher {
                    matchers: raws,
                    matcher_mode: MatcherMode::And,
                    ..Default::default()
                }))
            }
        };
    }
    let mut parts: Vec<Matcher> = Vec::new();
    if let Some(q) = quick {
        parts.push(q);
    }
    if dups {
        if let Some(d) = Matcher::from_raw(&raw_value_matcher(
            "ComicBookDuplicateMatcher",
            0,
            "",
            "",
            false,
            None,
        )) {
            parts.push(d);
        }
    }
    match parts.len() {
        0 => None,
        1 => {
            // The C# wraps quickFilter in the GetCurrentMatcher
            // group, whose Match applies a child's `Not`
            // (`ComicBookValueMatcher.Match` ignores it). A bare Not
            // value matcher as the single part (e.g. Show only Books)
            // needs that wrapper — the set evaluator skips the root
            // matcher's own Not.
            let part = parts.pop().unwrap();
            if matches!(part, Matcher::Value(ref v) if v.not) {
                Some(Matcher::Group(cr_engine::matcher::tree::GroupMatcher {
                    matcher_mode: MatcherMode::And,
                    matchers: vec![part],
                    ..Default::default()
                }))
            } else {
                Some(part)
            }
        }
        _ => Some(Matcher::Group(cr_engine::matcher::tree::GroupMatcher {
            matcher_mode: MatcherMode::And,
            matchers: parts,
            ..Default::default()
        })),
    }
}

/// The page-export filter table (`ExportImage`, MainForm.cs:2190):
/// JPEG | BMP | PNG | GIF | TIFF, filter index 1-based.
const PAGE_EXPORT_FILTERS: &[(&str, &[&str], cr_image::decode::ImageFormat)] = &[
    (
        "JPEG Image",
        &["jpg", "jpeg"],
        cr_image::decode::ImageFormat::Jpeg,
    ),
    (
        "Windows Bitmap Image",
        &["bmp"],
        cr_image::decode::ImageFormat::Bmp,
    ),
    ("PNG Image", &["png"], cr_image::decode::ImageFormat::Png),
    ("GIF Image", &["gif"], cr_image::decode::ImageFormat::Gif),
    ("TIFF Image", &["tif"], cr_image::decode::ImageFormat::Tiff),
];

/// ARGB (premultiplied, cairo stride) → the RGBA currency
/// (`Bitmap.SaveImage` consumes the un-premultiplied form).
/// ARGB (premultiplied, cairo stride) → the RGBA currency
/// (`Bitmap.SaveImage` consumes the un-premultiplied form). Reads via
/// `with_data` — `data()` demands exclusive access (surface refcount
/// 1) and always fails while the caller holds a reference.
fn surface_to_image(surface: &gtk4::cairo::ImageSurface) -> Option<cr_image::Image> {
    surface.flush();
    let width = surface.width() as u32;
    let height = surface.height() as u32;
    let stride = surface.stride() as usize;
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    let read = surface.with_data(|data| {
        for y in 0..height as usize {
            let row = &data[y * stride..y * stride + width as usize * 4];
            for px in row.as_chunks::<4>().0 {
                let a = u32::from(px[3]);
                if a == 0 {
                    rgba.extend_from_slice(&[0, 0, 0, 0]);
                } else {
                    // Un-premultiply (identity for opaque pages).
                    let un = |v: u8| ((u32::from(v) * 255) / a) as u8;
                    rgba.extend_from_slice(&[un(px[2]), un(px[1]), un(px[0]), px[3]]);
                }
            }
        }
    });
    if let Err(err) = read {
        crate::trace::trace(format!("surface_to_image: with_data failed: {err}"));
        return None;
    }
    Some(cr_image::Image {
        width,
        height,
        rgba,
    })
}

/// `Clipboard.SetImage` (ComicDisplay.cs:1441): the composed page
/// image as a texture on the default clipboard.
fn copy_surface_to_clipboard(surface: &gtk4::cairo::ImageSurface) {
    let Some(image) = surface_to_image(surface) else {
        crate::trace::trace("copy-page: surface_to_image returned None");
        return;
    };
    crate::trace::trace(format!(
        "copy-page: page image {}x{}",
        image.width, image.height
    ));
    let Ok(png) = cr_image::decode::encode_image(&image, cr_image::decode::ImageFormat::Png) else {
        crate::trace::trace("copy-page: png encode failed");
        return;
    };
    crate::trace::trace(format!("copy-page: png {} bytes", png.len()));
    let Some(display) = gdk::Display::default() else {
        crate::trace::trace("copy-page: no default display");
        return;
    };
    let provider = gdk::ContentProvider::for_bytes("image/png", &gdk::glib::Bytes::from_owned(png));
    match display.clipboard().set_content(Some(&provider)) {
        Ok(()) => crate::trace::trace("copy-page: clipboard set_content OK"),
        Err(err) => crate::trace::trace(format!("copy-page: set_content failed: {err}")),
    }
}

/// `ExportImage` (MainForm.cs:2185): the "Save Page as" native save
/// dialog. The initial name is "{Caption} - Page {N}" with the saved
/// filter index's extension (the C# `AddExtension`); the accepted
/// path gets the extension when missing, and the chosen filter index
/// persists in the settings.
fn export_page_dialog(
    window: &ApplicationWindow,
    caption: &str,
    page: usize,
    surface: Option<gtk4::cairo::ImageSurface>,
) {
    let Some(surface) = surface else {
        crate::trace::trace("export-page: no page image, dialog skipped");
        return;
    };
    let chooser = gtk4::FileChooserNative::builder()
        .title("Save Page as")
        .action(gtk4::FileChooserAction::Save)
        .transient_for(window)
        .modal(true)
        .build();
    let mut filter_handles: Vec<gtk4::FileFilter> = Vec::new();
    for (name, exts, _) in PAGE_EXPORT_FILTERS {
        let filter = gtk4::FileFilter::new();
        filter.set_name(Some(name));
        for ext in *exts {
            filter.add_pattern(&format!("*.{ext}"));
        }
        chooser.add_filter(&filter);
        filter_handles.push(filter);
    }
    let saved_index = cr_ui_settings().borrow().last_export_page_filter_index;
    let initial = saved_index.clamp(1, PAGE_EXPORT_FILTERS.len() as i32) as usize;
    // Select the saved one (1-based, the C# FilterIndex).
    chooser.set_filter(&filter_handles[initial - 1]);
    let (_, initial_exts, _) = PAGE_EXPORT_FILTERS[initial - 1];
    let name = format!(
        "{} - Page {}.{}",
        cr_io::export::make_valid_filename(caption),
        page,
        initial_exts[0]
    );
    chooser.set_current_name(&name);
    crate::trace::trace(format!("export-page: chooser shown, initial name {name:?}"));
    let window_for_errors = window.clone();
    chooser.connect_response(move |chooser, response| {
        crate::trace::trace(format!(
            "export-page: response {response:?} (accept={:?})",
            gtk4::ResponseType::Accept
        ));
        let _ = &filter_handles;
        if response != gtk4::ResponseType::Accept {
            return;
        }
        let Some(path) = chooser.file().and_then(|f| f.path()) else {
            crate::trace::trace("export-page: response carried no file path");
            return;
        };
        // The chosen filter: position in the kept handle list.
        let selected = chooser.filter();
        let chosen = filter_handles
            .iter()
            .position(|f| selected.as_ref().is_some_and(|sel| sel == f))
            .map_or(initial, |i| i + 1);
        let (_, exts, format) = PAGE_EXPORT_FILTERS[chosen - 1];
        // `AddExtension`: append the filter's extension when missing.
        let mut path = path;
        if path.extension().is_none() {
            path.set_extension(exts[0]);
        }
        crate::trace::trace(format!("export-page: writing {path:?} (format {chosen})"));
        cr_ui_settings().borrow_mut().last_export_page_filter_index = chosen as i32;
        let Some(image) = surface_to_image(&surface) else {
            crate::trace::trace("export-page: surface_to_image returned None");
            return;
        };
        match cr_image::decode::encode_image(&image, format) {
            Ok(bytes) => {
                crate::trace::trace(format!("export-page: encoded {} bytes", bytes.len()));
                if let Err(err) = std::fs::write(&path, bytes) {
                    // `CouldNotSaveImage` parity — an error dialog.
                    crate::trace::trace(format!("export-page: write failed: {err}"));
                    show_failure_dialog(
                        &window_for_errors,
                        &format!("Cannot save {}", path.to_string_lossy()),
                        &err.to_string(),
                    );
                } else {
                    crate::trace::trace("export-page: file written");
                }
            }
            Err(err) => {
                crate::trace::trace(format!("export-page: encode failed: {err}"));
                show_failure_dialog(
                    &window_for_errors,
                    &format!("Cannot save {}", path.to_string_lossy()),
                    &err.to_string(),
                );
            }
        }
    });
    chooser.show();
}

fn open_file_dialog(window: &ApplicationWindow, on_open: impl Fn(&str) + 'static) {
    let chooser = gtk4::FileChooserNative::builder()
        .title("Open Comic")
        .action(gtk4::FileChooserAction::Open)
        .transient_for(window)
        .modal(true)
        .build();
    let filter = gtk4::FileFilter::new();
    filter.set_name(Some("Comic files"));
    for ext in crate::app::OPEN_FILTER_EXTS {
        filter.add_pattern(&format!("*.{ext}"));
    }
    chooser.add_filter(&filter);
    chooser.connect_response(move |chooser, response| {
        if response != gtk4::ResponseType::Accept {
            return;
        }
        if let Some(path) = chooser.file().and_then(|f| f.path()) {
            on_open(&path.to_string_lossy());
        }
    });
    chooser.show();
}

/// Opens the `.mcl` file chooser (ADR-038).
fn open_mcl_dialog(window: &ApplicationWindow, on_open: impl Fn(&str) + 'static) {
    let chooser = gtk4::FileChooserNative::builder()
        .title("Import Comic Vine MCL File")
        .action(gtk4::FileChooserAction::Open)
        .transient_for(window)
        .modal(true)
        .build();
    let filter = gtk4::FileFilter::new();
    filter.set_name(Some("Comic Vine MCL files"));
    filter.add_pattern("*.mcl");
    chooser.add_filter(&filter);
    chooser.connect_response(move |chooser, response| {
        if response != gtk4::ResponseType::Accept {
            return;
        }
        if let Some(path) = chooser.file().and_then(|f| f.path()) {
            on_open(&path.to_string_lossy());
        }
    });
    chooser.show();
}

/// What a Comic Vine cache worker tells the main thread.
enum CvProgressMsg {
    Step {
        detail: String,
        done: i64,
        total: i64,
    },
    /// The per-resource budget is spent (ADR-037). Without this line
    /// the job looks frozen for up to an hour.
    Waiting { resource: String, resume_at: i64 },
}

/// A budget wait report that forwards to the job's progress channel.
/// It runs on the WORKER thread, so it only sends.
fn wait_reporter(
    progress: std::sync::mpsc::Sender<CvProgressMsg>,
) -> Box<cr_scrape::cache::budget::WaitFn> {
    Box::new(move |notice| {
        let _ = progress.send(CvProgressMsg::Waiting {
            resource: notice.resource.clone(),
            resume_at: notice.resume_at,
        });
    })
}

/// The Missing Issues bar's status text (Phase 19): the row count and
/// timing of the last completed pass, or a running/not-yet-run state.
fn missing_issues_status_text() -> String {
    if library::missing_issues_refresh_active() {
        return "Running…".to_string();
    }
    let Some(last_run) = library::missing_issues_last_run() else {
        return "Not yet run".to_string();
    };
    let count = library::missing_issues_snapshot().len();
    let elapsed = library::missing_issues_last_elapsed()
        .map(|d| format!(" ({:.1}s)", d.as_secs_f64()))
        .unwrap_or_default();
    format!(
        "{count} missing issue{} — last computed at {}{elapsed}",
        if count == 1 { "" } else { "s" },
        local_clock(last_run)
    )
}

/// Unix seconds as a local wall-clock time, for "resuming at 14:32".
fn local_clock(unix_seconds: i64) -> String {
    chrono::DateTime::from_timestamp(unix_seconds, 0)
        .map(|t| {
            chrono::DateTime::<chrono::Local>::from(t)
                .format("%H:%M")
                .to_string()
        })
        .unwrap_or_else(|| "later".to_string())
}

/// A report a command leaves when it finishes.
///
/// `heading` is shown as written. The dialog is transient over the
/// window and modal: an `application`-parented dialog gets no parent
/// hint, so the window manager is free to put it BEHIND the main
/// window, which is what it did.
pub(crate) fn show_report_dialog(parent: &impl IsA<gtk4::Window>, heading: &str, message: &str) {
    message_dialog(parent, heading, message, gtk4::MessageType::Info);
}

/// A failure a command leaves when it stops.
///
/// `heading` is shown as written. It used to be forced through
/// `format!("Cannot open {title}")`, which was wrong for every caller
/// that was not opening a file.
fn show_failure_dialog(parent: &impl IsA<gtk4::Window>, heading: &str, message: &str) {
    message_dialog(parent, heading, message, gtk4::MessageType::Error);
}

fn message_dialog(
    parent: &impl IsA<gtk4::Window>,
    heading: &str,
    message: &str,
    kind: gtk4::MessageType,
) {
    let dialog = gtk4::MessageDialog::builder()
        .transient_for(parent)
        .modal(true)
        .title("comicrust")
        .text(heading.to_string())
        .secondary_text(message.to_string())
        .message_type(kind)
        .buttons(gtk4::ButtonsType::Close)
        .build();
    dialog.connect_response(|dialog, _| dialog.destroy());
    dialog.present();
}

#[cfg(test)]
mod tests {
    use super::*;
    use cr_core::xml::scalar::CrGuid;

    fn book(series: &str, writer: &str, read_pct: f32, path: &str) -> ComicBook {
        let mut b = ComicBook {
            id: CrGuid::new_random(),
            file_path: path.into(),
            ..Default::default()
        };
        b.info.series = series.into();
        b.info.writer = writer.into();
        if read_pct > 0.0 {
            b.info.page_count = 100;
            b.last_page_read = ((read_pct / 100.0) * 99.0).round() as i32;
        }
        b
    }

    fn eval(matcher: &Matcher, books: &[ComicBook]) -> Vec<usize> {
        let items: Vec<&ComicBook> = books.iter().collect();
        let ctx = cr_engine::matcher::eval::MatchContext::new(&items);
        let pairs = [(cr_core::model::enums::MatcherMode::And, false, matcher)];
        cr_engine::matcher::eval::match_set(&items, &pairs, &ctx)
            .iter()
            .filter_map(|b| books.iter().position(|x| x.id == b.id))
            .collect()
    }

    /// The quick search text contains (all fields) — the Create path
    /// with operator 3 (ContainsAll).
    #[test]
    fn compose_matches_by_text() {
        let books = [
            book("Batman", "Frank Miller", 0.0, "/a.cbz"),
            book("Spider-Man", "Stan Lee", 0.0, "/b.cbz"),
        ];
        let m = compose_quick_filter("batman", "all", "all", "all", false).unwrap();
        let hit = eval(&m, &books);
        assert_eq!(hit, vec![0]);
        // The Writer scope narrows to the writer field.
        let m = compose_quick_filter("stan", "writer", "all", "all", false).unwrap();
        assert_eq!(eval(&m, &books), vec![1]);
        // A different scope misses.
        let m = compose_quick_filter("stan", "series", "all", "all", false).unwrap();
        assert!(eval(&m, &books).is_empty());
    }

    /// The read-state filter (the engine defaults: read >= 95,
    /// unread < 10, reading 11..=94).
    #[test]
    fn compose_applies_the_read_filter() {
        let books = [
            book("a", "w", 100.0, "/a.cbz"),
            book("b", "w", 50.0, "/b.cbz"),
            book("c", "w", 0.0, "/c.cbz"),
        ];
        let m = compose_quick_filter("", "all", "read", "all", false).unwrap();
        assert_eq!(eval(&m, &books), vec![0]);
        let m = compose_quick_filter("", "all", "reading", "all", false).unwrap();
        assert_eq!(eval(&m, &books), vec![1]);
        let m = compose_quick_filter("", "all", "unread", "all", false).unwrap();
        assert_eq!(eval(&m, &books), vec![2]);
        // No filters, no text → no matcher at all (the C# null).
        assert!(compose_quick_filter("", "all", "all", "all", false).is_none());
    }

    /// The comic-type filter: a file path = Books, empty = fileless.
    #[test]
    fn compose_applies_the_comic_type_filter() {
        let books = [book("a", "w", 0.0, "/a.cbz"), book("b", "w", 0.0, "")];
        let m = compose_quick_filter("", "all", "all", "books", false).unwrap();
        assert_eq!(eval(&m, &books), vec![0]);
        let m = compose_quick_filter("", "all", "all", "fileless", false).unwrap();
        assert_eq!(eval(&m, &books), vec![1]);
    }

    /// Duplicates-only rides on top of everything (set-based).
    #[test]
    fn compose_applies_duplicates() {
        let books = [
            book("a", "w", 0.0, "/a.cbz"),
            book("a", "w", 0.0, "/b.cbz"),
            book("c", "w", 0.0, "/c.cbz"),
        ];
        let m = compose_quick_filter("", "all", "all", "all", true).unwrap();
        let hit = eval(&m, &books);
        assert!(hit.contains(&0) && hit.contains(&1) && !hit.contains(&2));
    }

    /// A MATCH query parses only for the All scope, and then the
    /// view filters do NOT apply (the C# UpdateQuickFilter order).
    #[test]
    fn compose_match_query_only_for_all_scope() {
        let books = [
            book("Batman", "w", 100.0, "/a.cbz"),
            book("Superman", "w", 0.0, "/b.cbz"),
        ];
        let m = compose_quick_filter(
            "MATCH [Series] contains \"Batman\"",
            "all",
            "all",
            "all",
            false,
        );
        assert_eq!(eval(m.as_ref().unwrap(), &books), vec![0]);
        // A non-All scope keeps the AllProperties path (the query
        // text searches the scoped fields — no hit on "MATCH ...").
        let m = compose_quick_filter(
            "MATCH [Series] contains \"Batman\"",
            "series",
            "all",
            "all",
            false,
        );
        assert!(eval(m.as_ref().unwrap(), &books).is_empty());
    }

    #[test]
    fn select_worst_is_available_for_incoming_only_duplicate_sets() {
        use crate::browser::navigator::IncomingView;

        assert!(show_select_worst_for_incoming_view(
            IncomingView::Duplicates.id()
        ));
        assert!(show_select_worst_for_incoming_view(
            IncomingView::IncomingDuplicates.id()
        ));
        assert!(!show_select_worst_for_incoming_view(
            IncomingView::LibraryDuplicates.id()
        ));
        assert!(!show_select_worst_for_incoming_view(IncomingView::All.id()));
    }
}
