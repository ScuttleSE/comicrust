//! The library session — the cr-ui wiring of
//! `cr_engine::library::Library` (the C# `Program.DatabaseManager`
//! static): open at startup, save on exit, the 600 s background save,
//! the reader/book integration (the C# `ComicBookFactory`), and the
//! scan worker (the C# "Book Scanner" low-priority thread).

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use cr_core::database::comic_database::OpenStatus;
use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::CrDateTime;
use cr_engine::library::Library;
use cr_engine::scanner::{refresh_file_info, scan_sync, ScanItem, ScanResult};
use glib::ControlFlow;
use gtk4::glib;

/// One queued scan request: the location and the completion callback.
type QueuedScan = (String, Box<dyn FnOnce(ScanResult)>);

thread_local! {
    static SESSION: RefCell<Option<Rc<RefCell<Library>>>> = const { RefCell::new(None) };
    /// One scan at a time (the C# scan queue): requests arriving
    /// while the worker runs wait here, in arrival order.
    static SCAN_QUEUE: RefCell<Vec<QueuedScan>> = const { RefCell::new(Vec::new()) };
    static SCAN_IN_FLIGHT: RefCell<bool> = const { RefCell::new(false) };
}

/// Opens the library database at the default location (`Program`'s
/// startup `DatabaseManager.Open`). Returns the `OpenMessage` the C#
/// would show in the attention dialog (None for a plain load).
pub fn initialize() -> Result<Option<String>, cr_core::database::DbError> {
    let (library, status) = Library::open_at_default_location()?;
    let message = open_message(status);
    SESSION.with(|cell| {
        *cell.borrow_mut() = Some(Rc::new(RefCell::new(library)));
    });
    Ok(message)
}

fn open_message(status: OpenStatus) -> Option<String> {
    status.message().map(str::to_string)
}

/// The session (`Program.DatabaseManager`). Panics before
/// [`initialize`] — the C# static would NRE the same way.
pub fn session() -> Rc<RefCell<Library>> {
    SESSION.with(|cell| {
        cell.borrow()
            .clone()
            .expect("library session not initialized")
    })
}

/// The database path shown in diagnostics.
pub fn database_file() -> std::path::PathBuf {
    session().borrow().file().to_path_buf()
}

/// `ComicBookFactory.Create` on open (`AddToLibraryOnOpen` is false,
/// the C# default): a comic that is in the library reuses the stored
/// book — file-info refresh (`RefreshInfoFromFile`), the open stamps
/// (`OnBookOpened` + the navigator `Opened`), and a dirty mark. The
/// returned clone seeds the reader session. Returns `None` for comics
/// outside the library — they stay temporary session books whose
/// reading state is not persisted (C# `AddToTemporary` parity).
pub fn open_book(path: &str) -> Option<ComicBook> {
    let library = session();
    let mut lib = library.borrow_mut();
    let mut found = None;
    if let Some(book) = lib.find_book_mut(path) {
        refresh_file_info(book);
        book.opened_time = CrDateTime::now();
        book.opened_count += 1;
        book.new_pages = 0;
        found = Some(book.clone());
    }
    if found.is_some() {
        lib.mark_dirty();
    }
    found
}

/// The page-turn write-back (`TrackCurrentPage` mirroring into the
/// library book; `ComicBookNavigator.CurrentPage` setter parity —
/// `set_current_page` carries the `LastPageRead` high-water mark).
/// Temporary books have no library entry and are skipped.
pub fn record_page_change(path: &str, page: i32) {
    let library = session();
    let mut lib = library.borrow_mut();
    if let Some(book) = lib.find_book_mut(path) {
        book.set_current_page(page);
        lib.mark_dirty();
    }
}

/// `AddFolderToLibrary` — scans the folder (recursively, no removal)
/// into the library on the scan worker. `done` runs on the UI thread
/// with the result.
pub fn add_folder_to_library(path: &Path, done: impl FnOnce(ScanResult) + 'static) {
    scan_async(path.to_string_lossy().into_owned(), done);
}

/// The scan worker (the C# `ComicScanner` runs its queue on a
/// dedicated low-priority "Book Scanner" thread; a synchronous scan
/// freezes the UI on real libraries). The book storage moves to the
/// worker and back over std mpsc; a `timeout_add_local` pump merges
/// it (the ADR-019 pattern). While a scan runs, the database holds no
/// books — lookups degrade to temporary books for the duration.
/// Requests arriving mid-scan queue and run in arrival order.
fn scan_async(location: String, done: impl FnOnce(ScanResult) + 'static) {
    let in_flight = SCAN_IN_FLIGHT.with(|cell| *cell.borrow());
    if in_flight {
        SCAN_QUEUE.with(|q| {
            q.borrow_mut().push((location, Box::new(done)));
        });
        return;
    }
    start_scan_worker(location, done);
}

/// Takes the book storage, runs `scan_sync` on a worker thread, and
/// pumps the result back onto the UI thread.
fn start_scan_worker(location: String, done: impl FnOnce(ScanResult) + 'static) {
    let library = session();
    let books = {
        let mut lib = library.borrow_mut();
        SCAN_IN_FLIGHT.with(|cell| *cell.borrow_mut() = true);
        std::mem::take(&mut lib.database_mut().books)
    };
    let items = [ScanItem {
        location,
        all: true,
        remove_missing: false,
        force_refresh_info: false,
    }];
    let now = CrDateTime::now();
    let (tx, rx) = std::sync::mpsc::channel::<(Vec<ComicBook>, ScanResult)>();
    std::thread::Builder::new()
        .name("Book Scanner".into())
        .spawn(move || {
            let mut storage = books;
            let result = scan_sync(&mut storage, &items, &now);
            let _ = tx.send((storage, result));
        })
        .expect("spawn Book Scanner");

    // The once-completion callback rides an Option (the pump closure
    // is FnMut — it cannot move `done` out).
    let mut done = Some(done);
    glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        // Bind the recv result before matching — a `while let`
        // scrutinee borrow lives through the loop body.
        let received = rx.try_recv();
        match received {
            Ok((books, result)) => {
                {
                    let mut lib = library.borrow_mut();
                    lib.database_mut().books = books;
                    let changed = !result.added.is_empty()
                        || !result.updated.is_empty()
                        || !result.moved.is_empty()
                        || !result.removed.is_empty();
                    if changed {
                        lib.mark_dirty();
                    }
                }
                SCAN_IN_FLIGHT.with(|cell| *cell.borrow_mut() = false);
                // The queued requests run one at a time, in arrival
                // order (the C# scan queue).
                let next = SCAN_QUEUE.with(|q| q.borrow_mut().pop());
                if let Some((location, done_next)) = next {
                    start_scan_worker(location, done_next);
                }
                if let Some(d) = done.take() {
                    d(result);
                }
                ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                SCAN_IN_FLIGHT.with(|cell| *cell.borrow_mut() = false);
                if let Some(d) = done.take() {
                    d(ScanResult::default());
                }
                ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => ControlFlow::Continue,
        }
    });
}

/// Pops the debounced watch roots that need a rescan (the watch poll
/// timer calls this every second).
pub fn take_watch_folder_rescans() -> Vec<String> {
    session().borrow_mut().take_watch_folder_rescans()
}

/// `DatabaseManager.Save` (the exit path): waits for an in-flight
/// scan to merge back first (the C# `Scanner.Stop`/join runs before
/// `DatabaseManager.Dispose` → `Save` — saving mid-scan would write
/// the taken, empty book list), then saves unconditionally.
pub fn save() -> Result<(), cr_core::database::DbError> {
    while scan_in_flight() {
        // The merge happens in the scan pump (a main-loop source) —
        // drive the loop until it runs.
        glib::MainContext::default().iteration(true);
    }
    session().borrow_mut().save()
}

/// `DatabaseManager.SaveInBackground`: saves only when dirty. A scan
/// in flight skips this tick (the book storage is on the worker).
/// Returns whether a save ran.
pub fn save_if_dirty() -> Result<bool, cr_core::database::DbError> {
    if scan_in_flight() {
        return Ok(false);
    }
    session().borrow_mut().save_if_dirty()
}

fn scan_in_flight() -> bool {
    SCAN_IN_FLIGHT.with(|cell| *cell.borrow())
}
