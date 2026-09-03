//! The library session — the cr-ui wiring of
//! `cr_engine::library::Library` (the C# `Program.DatabaseManager`
//! static): open at startup, save on exit, the 600 s background save,
//! and the reader/book integration (the C# `ComicBookFactory`).

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use cr_core::database::comic_database::OpenStatus;
use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::CrDateTime;
use cr_engine::library::Library;
use cr_engine::scanner::{refresh_file_info, ScanResult};

thread_local! {
    static SESSION: RefCell<Option<Rc<RefCell<Library>>>> = const { RefCell::new(None) };
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
/// into the library.
pub fn add_folder_to_library(path: &Path) -> ScanResult {
    let library = session();
    let mut lib = library.borrow_mut();
    lib.scan_file_or_folder(&path.to_string_lossy(), true, false)
}

/// `DatabaseManager.Save` (the exit path): unconditional save.
pub fn save() -> Result<(), cr_core::database::DbError> {
    session().borrow_mut().save()
}

/// `DatabaseManager.SaveInBackground`: saves only when dirty. Returns
/// whether a save ran.
pub fn save_if_dirty() -> Result<bool, cr_core::database::DbError> {
    session().borrow_mut().save_if_dirty()
}

/// Watch-triggered rescans: maps debounced watch events back to the
/// stored watch roots and rescans each one (`remove_missing: false` —
/// vanished files flag as missing, they are not dropped).
pub fn rescan_changed_watch_folders() -> Option<ScanResult> {
    let library = session();
    let mut lib = library.borrow_mut();
    let roots = lib.take_watch_folder_rescans();
    if roots.is_empty() {
        return None;
    }
    let mut combined = ScanResult::default();
    for root in roots {
        let result = lib.scan_file_or_folder(&root, true, false);
        combined.added.extend(result.added);
        combined.updated.extend(result.updated);
        combined.moved.extend(result.moved);
        combined.removed.extend(result.removed);
    }
    Some(combined)
}
