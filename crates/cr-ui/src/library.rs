//! The library session — the cr-ui wiring of
//! `cr_engine::library::Library` (the C# `Program.DatabaseManager`
//! static): open at startup, save on exit, the 600 s background save,
//! the reader/book integration (the C# `ComicBookFactory`), and the
//! scan worker (the C# "Book Scanner" low-priority thread).

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;

use cr_core::database::comic_database::OpenStatus;
use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::{CrDateTime, CrGuid};
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
    /// The location the in-flight scan walks (`Scanner.CurrentLocation`
    /// — the Tasks dialog's scan row).
    static SCAN_LOCATION: RefCell<String> = const { RefCell::new(String::new()) };
    /// The user settings (`Program.Settings` static). The GTK code
    /// reaches it through [`settings`].
    static SETTINGS: RefCell<Option<Rc<RefCell<cr_core::settings::Settings>>>> =
        const { RefCell::new(None) };
}

/// Opens the library database at the default location (`Program`'s
/// startup `DatabaseManager.Open`). Returns the `OpenMessage` the C#
/// would show in the attention dialog (None for a plain load).
///
/// Also loads the settings layer: `Config.xml` (the C#
/// `Settings.Load(defaultSettingsFile)`) and the `comicrust.ini`
/// chain + argv for `EngineConfiguration`/`ExtendedSettings` (the
/// `IniFile.Default.Register` + `CommandLineParser` boot).
pub fn initialize() -> Result<Option<String>, cr_core::database::DbError> {
    let (library, status) = Library::open_at_default_location()?;
    let message = open_message(status);
    SESSION.with(|cell| {
        *cell.borrow_mut() = Some(Rc::new(RefCell::new(library)));
    });
    initialize_settings();
    Ok(message)
}

/// The settings boot (`Settings.Load` + the ini chain + argv).
/// Unknown/corrupt config files fall back to the defaults (the C#
/// catch parity).
fn initialize_settings() {
    let paths = cr_core::paths::Paths::new_default();
    let settings = cr_core::settings::Settings::load(&cr_core::paths::settings_file(&paths));

    // The ini chain (later files override earlier ones, C#
    // `DefaultIniFile`), plus the command line.
    let chain = cr_core::paths::ini_default_locations(&paths)
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("|");
    let ini = cr_core::settings::IniValues::read_files(&chain);
    let argv: Vec<String> = std::env::args().skip(1).collect();

    let mut engine = cr_core::settings::EngineConfiguration::default();
    engine.load(&ini);
    cr_core::settings::EngineConfiguration::init_global(engine);

    let mut extended = cr_core::settings::ExtendedSettings::default();
    extended.load(&ini, &argv);
    cr_core::settings::ExtendedSettings::init_global(extended);

    SETTINGS.with(|cell| {
        *cell.borrow_mut() = Some(Rc::new(RefCell::new(settings)));
    });
}

/// The user settings (`Program.Settings`). Panics before
/// [`initialize`].
pub fn settings() -> Rc<RefCell<cr_core::settings::Settings>> {
    SETTINGS.with(|cell| cell.borrow().clone().expect("settings not initialized"))
}

/// `Settings.Save(defaultSettingsFile)` (the C# app-exit step).
pub fn save_settings() {
    let paths = cr_core::paths::Paths::new_default();
    let config_file = cr_core::paths::settings_file(&paths);
    let s = settings();
    let _ = s
        .borrow()
        .save(&config_file)
        .inspect_err(|e| eprintln!("saving Config.xml failed: {e}"));
}

/// Persists ini keys into the LAST file of the ini chain (the user
/// location — the chain's override at load). The C# never writes the
/// ini, but it reads `ExtendedSettings` from it at every boot; this
/// is the recorded deviation that lets the runtime theme toggle
/// survive a restart.
pub fn save_ini_keys(keys: &[(&str, &str)]) {
    let paths = cr_core::paths::Paths::new_default();
    let Some(file) = cr_core::paths::ini_default_locations(&paths).pop() else {
        return;
    };
    if let Err(err) = cr_core::settings::ini::merge_write(&file, keys) {
        eprintln!("saving {} failed: {err}", file.display());
    }
}

fn open_message(status: OpenStatus) -> Option<String> {
    status.message().map(str::to_string)
}

// ---------- The image caches (the C# `CacheManager`) ----------

/// The `ImagePool` construction from the session (`CacheManager`
/// ctor parity): the `SystemPaths` cache folders and the caching
/// settings (`ThumbCacheEnabled/PageCacheEnabled/...MB`,
/// `MemoryThumbCacheSizeMB`, `MemoryPageCacheCount`). The pools get
/// the defaults where the C# uses its own constants (thumb memory
/// item cap 8192 in the engine's `MEMORY_THUMBNAIL_CACHE_SIZE`).
pub fn image_pool_config() -> cr_engine::image_pool::ImagePoolConfig {
    use cr_engine::image_pool::ImagePoolConfig;
    let paths = cr_core::paths::Paths::new_default();
    let s = settings();
    let s = s.borrow();
    let clamp_page = s.memory_page_cache_count.clamp(1, 100) as usize;
    ImagePoolConfig {
        page_cache_dir: Some(paths.image_cache_path),
        thumb_cache_dir: Some(paths.thumbnail_cache_path),
        page_cache_size_mb: s.page_cache_size_mb.max(0) as u64,
        thumb_cache_size_mb: s.thumb_cache_size_mb.max(0) as u64,
        page_cache_enabled: s.page_cache_enabled,
        thumb_cache_enabled: s.thumb_cache_enabled,
        page_memory_count: clamp_page,
        thumb_memory_bytes: s.memory_thumb_cache_size_mb.max(0) as usize * 1024 * 1024,
    }
}

/// Wires `PageCached`/`ThumbnailCached` into the pool and drains the
/// event queue onto the UI thread (the C# `CacheManager` handlers
/// fire inline; the port bridges the worker completion over mpsc —
/// the ADR-019 shape). Every event writes the decoded pixel size
/// into the book with that location (`UpdateComicBookPageData`:
/// `TranslateImageIndexToPage(key.Index)` then `UpdatePageSize`),
/// marking the library dirty only on an actual change.
pub fn install_cache_events(pool: &std::sync::Arc<cr_engine::image_pool::ImagePool>) {
    let (tx, rx) = std::sync::mpsc::channel::<cr_engine::image_pool::CacheEvent>();
    pool.set_event_tx(cr_engine::image_pool::CacheEventTx::new(tx));
    let library = session();
    glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
        // Drain every pending event per tick (the warm-up emits
        // thousands; one per tick would lag the write-back behind
        // for hours).
        loop {
            let received = rx.try_recv();
            let Ok(event) = received else {
                break;
            };
            let (location, index, w, h) = match event {
                cr_engine::image_pool::CacheEvent::PageCached {
                    location,
                    index,
                    width,
                    height,
                } => (location, index, width, height),
                cr_engine::image_pool::CacheEvent::ThumbnailCached {
                    location,
                    index,
                    width,
                    height,
                } => (location, index, width, height),
            };
            let mut l = library.borrow_mut();
            let Some(book) = l
                .database_mut()
                .books
                .iter_mut()
                .find(|b| b.file_path == location)
            else {
                continue;
            };
            let page = book.info.translate_image_index_to_page(index as i32);
            if book.info.update_page_size(page, w as i32, h as i32) {
                l.mark_dirty();
            }
        }
        ControlFlow::Continue
    });
}

/// `MainForm.GenerateFrontCoverCache` — the "Generate Cover
/// Thumbnails" command: one unlimited-queue warm-up job per library
/// book. The worker skips entries already in the thumbnail disk
/// cache, so a repeated command is cheap.
pub fn cache_thumbnails(pool: &std::sync::Arc<cr_engine::image_pool::ImagePool>) {
    let books: Vec<ComicBook> = session().borrow().database().books.clone();
    for book in &books {
        let key = cr_engine::image_pool::front_cover_thumbnail_key(book);
        pool.generate_front_cover_thumbnail(key);
    }
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

/// `ComicBookFactory.Create` on open: a comic that is in the library
/// reuses the stored book — file-info refresh (`RefreshInfoFromFile`),
/// the open stamps (`OnBookOpened` + the navigator `Opened`), and a
/// dirty mark. Returns `None` for comics outside the library — they
/// stay temporary session books whose reading state is not persisted
/// (C# `AddToTemporary` parity), UNLESS `Settings.AddToLibraryOnOpen`
/// makes the create use `CreateBookOption.AddToStorage` (a new book
/// with `AddedTime = now` joins the database).
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
        return found;
    }
    let add_to_library = settings().borrow().add_to_library_on_open;
    if add_to_library && Path::new(path).exists() {
        // `ComicBookFactory.Create(file, AddToStorage)`: the new book
        // carries the scan defaults and joins the storage.
        let mut book = ComicBook {
            file_path: path.to_string(),
            added_time: CrDateTime::now(),
            ..ComicBook::default()
        };
        refresh_file_info(&mut book);
        lib.database_mut().books.push(book.clone());
        lib.mark_dirty();
        return Some(book);
    }
    None
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
        SCAN_LOCATION.with(|cell| *cell.borrow_mut() = location.clone());
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
                SCAN_LOCATION.with(|cell| cell.borrow_mut().clear());
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
                SCAN_LOCATION.with(|cell| cell.borrow_mut().clear());
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

/// The status-bar scan lamp (`Program.Scanner.IsScanning` — the
/// worker holds the book storage until the pump merges it back).
pub fn is_scanning() -> bool {
    scan_in_flight()
}

/// The location the in-flight scan walks (`Scanner.CurrentLocation`;
/// empty when idle).
pub fn scan_location() -> String {
    SCAN_LOCATION.with(|cell| cell.borrow().clone())
}

thread_local! {
    /// The export lamp (`QueueManager.IsInComicConversion` parity
    /// point): the C# export funnels through a background queue; the
    /// port runs synchronously in the export dialog, so the flag is
    /// only readable between page-progress callbacks (recorded
    /// deviation — the dialog drives the lamp through the same
    /// accessor).
    static EXPORT_IN_FLIGHT: Cell<bool> = const { Cell::new(false) };
}

/// The status-bar export lamp.
pub fn export_in_flight() -> bool {
    EXPORT_IN_FLIGHT.with(|cell| cell.get())
}

/// The export dialog sets this around its run.
pub fn set_export_active(active: bool) {
    EXPORT_IN_FLIGHT.with(|cell| cell.set(active));
}

/// The status-bar file-write lamp (`QueueManager.IsInComicFileUpdate`
/// parity point): the pending debounced write timers.
pub fn writes_pending() -> usize {
    WRITE_TIMERS.with(|cell| cell.borrow().len())
}

/// The file paths of the books with a pending write (the Tasks
/// dialog's "Write Info" rows — `WriteComicBookInfoFileQueue.
/// PendingItemInfos` parity: the C# formats the book caption, the
/// port shows the file path).
pub fn pending_write_files() -> Vec<String> {
    let ids: Vec<CrGuid> = WRITE_TIMERS.with(|cell| cell.borrow().keys().cloned().collect());
    let lib = session();
    let l = lib.borrow();
    ids.iter()
        .filter_map(|id| {
            l.database()
                .books
                .iter()
                .find(|b| b.id == *id)
                .map(|b| b.file_path.clone())
        })
        .collect()
}

/// The Tasks dialog's "Abort all User Tasks" write half (the C#
/// `WriteComicBookInfoFileQueue.Clear`): drops every pending debounced
/// write. A timer that already fired is untouched (its entry left the
/// map when the callback ran).
pub fn clear_pending_writes() {
    WRITE_TIMERS.with(|cell| {
        for (_, source) in cell.borrow_mut().drain() {
            source.remove();
        }
    });
}

// ---------- The list navigator (Phase 4 T2) ----------

/// A clone of the ComicLists tree for the navigator widget.
pub fn comic_lists_snapshot() -> Vec<cr_core::database::list_items::ComicListItem> {
    session().borrow().database().comic_lists.clone()
}

/// Evaluates one tree node: (name, book ids, count).
pub fn evaluate_list(id: &CrGuid) -> Option<(String, Vec<CrGuid>, usize)> {
    let lib = session();
    let l = lib.borrow();
    let item = cr_engine::lists::find_list_item(&l.database().comic_lists, id)?;
    let books = cr_engine::lists::evaluate_list(&item, l.database());
    let name = item.base().name.clone().unwrap_or_default();
    Some((name, books.iter().map(|b| b.id).collect(), books.len()))
}

/// Inserts a list item after the selection (the C#
/// `GetCurrentNodeComicListCollection` + `IndexOf(current) + 1`): a
/// selected folder takes the item as its first child, a selected
/// list inserts after it in its container, nothing selected appends
/// at the root.
pub fn insert_list_item(
    after: Option<&CrGuid>,
    item: cr_core::database::list_items::ComicListItem,
) {
    let lib = session();
    let mut l = lib.borrow_mut();
    let lists = &mut l.database_mut().comic_lists;
    match after {
        Some(id) => {
            // A selected folder takes the new item as its first child.
            if let Some(folder) = find_folder_mut(lists, id) {
                folder.items.insert(0, item);
            } else if let Some(container) = find_container(lists, id) {
                let pos = container
                    .iter()
                    .position(|i| i.base().id == *id)
                    .map_or(container.len(), |p| p + 1);
                container.insert(pos, item);
            }
        }
        None => lists.push(item),
    }
    l.mark_dirty();
}

/// `NewSmartList`: name + a hand-written `Match` query (the editor
/// dialog is Phase 5; the query parses through the Phase 2 matcher
/// language). An empty query matches every book (the C# default).
pub fn new_smart_list(after: Option<&CrGuid>, name: &str, query: &str) -> Result<CrGuid, String> {
    let matchers = if query.trim().is_empty() {
        Vec::new()
    } else {
        let mut t = cr_engine::tokenizer::Tokenizer::new(query);
        let group =
            cr_engine::matcher::query::parse_group_query(&mut t).map_err(|e| format!("{e}"))?;
        group
            .matchers
            .iter()
            .map(cr_engine::matcher::tree::Matcher::to_raw)
            .collect()
    };
    let id = CrGuid::new_random();
    let item = cr_core::database::list_items::ComicListItem::Smart(
        cr_core::database::list_items::SmartListItem {
            base: cr_core::database::list_items::ListItemBase {
                id,
                name: Some(name.to_string()),
                ..Default::default()
            },
            matchers,
            matcher_mode: cr_core::model::enums::MatcherMode::And,
            ..Default::default()
        },
    );
    insert_list_item(after, item);
    Ok(id)
}

/// `NewFolder` — created in the selection's container.
pub fn new_folder(after: Option<&CrGuid>, name: &str) -> CrGuid {
    let id = CrGuid::new_random();
    let item = cr_core::database::list_items::ComicListItem::Folder(
        cr_core::database::list_items::FolderItem {
            base: cr_core::database::list_items::ListItemBase {
                id,
                name: Some(name.to_string()),
                ..Default::default()
            },
            ..Default::default()
        },
    );
    insert_list_item(after, item);
    id
}

/// The `ComicListItemFolder` nodes of the tree in order:
/// (folder id, child level, display name) — the Duplicate List
/// dropdown (`tbbDuplicateList_DropDownOpening`).
pub fn list_folders() -> Vec<(CrGuid, usize, String)> {
    fn walk(
        items: &[cr_core::database::list_items::ComicListItem],
        level: usize,
        out: &mut Vec<(CrGuid, usize, String)>,
    ) {
        for item in items {
            if let cr_core::database::list_items::ComicListItem::Folder(f) = item {
                out.push((f.base.id, level, f.base.name.clone().unwrap_or_default()));
                walk(&f.items, level + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(&comic_lists_snapshot(), 0, &mut out);
    out
}

/// `DuplicateList(clif)`: a new smart list from the CURRENT filter
/// (`GetCurrentMatcher` — quick search + the view filters), named
/// from the matcher values (joined `/`) or the current list name,
/// numbered against the existing siblings (`NumberedString`), added
/// to `folder`. `Ok(None)` = the C# early return: no filter or no
/// source list. The C# None-folder target adds to the special
/// TemporaryFolder — unreachable from the toolbar menu (the None row
/// is only a disabled placeholder).
pub fn duplicate_smart_list(
    source_id: &CrGuid,
    folder: &CrGuid,
    filter: &cr_engine::matcher::tree::Matcher,
) -> Result<CrGuid, String> {
    // The matcher values become the name (the C# joins the
    // `MatchValue` texts of every value matcher in the tree).
    fn collect_values(m: &cr_engine::matcher::tree::Matcher, out: &mut Vec<String>) {
        match m {
            cr_engine::matcher::tree::Matcher::Group(g) => {
                for inner in &g.matchers {
                    collect_values(inner, out);
                }
            }
            cr_engine::matcher::tree::Matcher::Value(v) => {
                let text = v.value.trim();
                if !text.is_empty() {
                    out.push(text.to_string());
                }
            }
        }
    }
    let mut values = Vec::new();
    collect_values(filter, &mut values);

    let lib = session();
    let l = lib.borrow();
    // The source list's name (the numbering fallback base).
    let source_name = cr_engine::lists::find_list_item(&l.database().comic_lists, source_id)
        .and_then(|i| i.base().name.clone())
        .unwrap_or_default();
    let mut name = values.join("/");
    if name.is_empty() {
        name = cr_engine::text::strip_number(&source_name);
    }
    // The numbering: MaxNumber over the lists that strip to the same
    // name (`NumberedString.Format(name, MaxNumber(...))`).
    let names: Vec<String> = l
        .database()
        .comic_lists
        .iter()
        .filter(|i| {
            cr_engine::text::strip_number(&i.base().name.clone().unwrap_or_default()) == name
        })
        .map(|i| i.base().name.clone().unwrap_or_default())
        .collect();
    let number = cr_engine::text::max_number(names.iter().map(|s| s.as_str()));
    let name = cr_engine::text::format_numbered(&name, number);
    drop(l);

    // The C# clones the CHILD matchers of the current matcher group
    // into the new item (not the wrapper group).
    let matchers: Vec<cr_core::database::list_items::ComicBookMatcher> = match filter {
        cr_engine::matcher::tree::Matcher::Group(g) => {
            g.matchers.iter().map(|m| m.to_raw()).collect()
        }
        m => vec![m.to_raw()],
    };

    let id = CrGuid::new_random();
    let item = cr_core::database::list_items::ComicListItem::Smart(
        cr_core::database::list_items::SmartListItem {
            base: cr_core::database::list_items::ListItemBase {
                id,
                name: Some(name),
                ..Default::default()
            },
            matchers,
            matcher_mode: cr_core::model::enums::MatcherMode::And,
            base_list_id: *source_id,
            ..Default::default()
        },
    );
    // The C# `clif.Items.Add` appends to the chosen folder.
    {
        let lib = session();
        let mut l = lib.borrow_mut();
        if let Some(folder) = find_folder_mut(&mut l.database_mut().comic_lists, folder) {
            folder.items.push(item);
            l.mark_dirty();
            return Ok(id);
        }
        Err("duplicate list target folder not found".into())
    }
}

/// Rename (the C# `AfterLabelEdit` → `comicListItem.Name = label`).
pub fn rename_list(id: &CrGuid, name: &str) {
    let lib = session();
    let mut l = lib.borrow_mut();
    let lists = &mut l.database_mut().comic_lists;
    if let Some(base) = find_base_mut(lists, id) {
        base.name = Some(name.to_string());
        l.mark_dirty();
    }
}

/// `RemoveListOrFolder` — the Library root is protected.
pub fn remove_list(id: &CrGuid) {
    let lib = session();
    let mut l = lib.borrow_mut();
    let lists = &mut l.database_mut().comic_lists;
    if let Some(container) = find_container(lists, id) {
        let is_library = container.iter().any(|i| {
            i.base().id == *id
                && matches!(i, cr_core::database::list_items::ComicListItem::Library(_))
        });
        if is_library {
            return;
        }
        container.retain(|i| i.base().id != *id);
        l.mark_dirty();
    }
}

fn find_container<'a>(
    items: &'a mut Vec<cr_core::database::list_items::ComicListItem>,
    id: &CrGuid,
) -> Option<&'a mut Vec<cr_core::database::list_items::ComicListItem>> {
    if items.iter().any(|i| i.base().id == *id) {
        return Some(items);
    }
    for item in items.iter_mut() {
        if let cr_core::database::list_items::ComicListItem::Folder(f) = item {
            if let Some(c) = find_container(&mut f.items, id) {
                return Some(c);
            }
        }
    }
    None
}

fn find_folder_mut<'a>(
    items: &'a mut [cr_core::database::list_items::ComicListItem],
    id: &CrGuid,
) -> Option<&'a mut cr_core::database::list_items::FolderItem> {
    for item in items.iter_mut() {
        if let cr_core::database::list_items::ComicListItem::Folder(f) = item {
            if f.base.id == *id {
                return Some(f);
            }
            if let Some(found) = find_folder_mut(&mut f.items, id) {
                return Some(found);
            }
        }
    }
    None
}

fn find_base_mut<'a>(
    items: &'a mut [cr_core::database::list_items::ComicListItem],
    id: &CrGuid,
) -> Option<&'a mut cr_core::database::list_items::ListItemBase> {
    for item in items.iter_mut() {
        if item.base().id == *id {
            return Some(item.base_mut());
        }
        if let cr_core::database::list_items::ComicListItem::Folder(f) = item {
            if let Some(b) = find_base_mut(&mut f.items, id) {
                return Some(b);
            }
        }
    }
    None
}

/// Evaluates one tree node to its book set, cloned for the ItemView
/// (the browser shell shares handles in T5).
pub fn evaluate_books(id: &CrGuid) -> Option<(String, Vec<ComicBook>)> {
    let lib = session();
    let l = lib.borrow();
    let item = cr_engine::lists::find_list_item(&l.database().comic_lists, id)?;
    let books = cr_engine::lists::evaluate_list(&item, l.database());
    let name = item.base().name.clone().unwrap_or_default();
    Some((name, books.into_iter().cloned().collect()))
}

/// Applies an edited book (the book editor's commit callback): the
/// library entry with the same id is REPLACED (the C# edits the live
/// object; the clone round-trip through the dialog is the port's
/// shape), the database marks dirty, and the book's file info is
/// marked stale (`ComicInfoIsDirty` — the C#
/// `WatchedBookHasChanged` fires on every property edit). The
/// auto-update timer then decides about the file write. Returns
/// false when the book is not in the library (a temporary session
/// book).
pub fn apply_edited(edited: &ComicBook) -> bool {
    let lib = session();
    let mut l = lib.borrow_mut();
    let Some(slot) = l
        .database_mut()
        .books
        .iter_mut()
        .find(|b| b.id == edited.id)
    else {
        return false;
    };
    let mut edited = edited.clone();
    edited.comic_info_is_dirty = true;
    let id = edited.id;
    *slot = edited;
    l.mark_dirty();
    drop(l);
    schedule_book_file_update(&id);
    true
}

// ---------- The file write-back (the C# `QueueManager.AddBookToFileUpdate`) ----------

thread_local! {
    /// The debounced write timers (one per book id; the C# keeps a
    /// 100 ms `Timer` per book so batched property edits write once).
    static WRITE_TIMERS: RefCell<std::collections::HashMap<CrGuid, glib::SourceId>> =
        RefCell::new(std::collections::HashMap::new());
}

/// Schedules the (debounced) automatic file write for one book — the
/// `AutoUpdateComicsFiles` path. The gates run again when the timer
/// fires.
pub fn schedule_book_file_update(id: &CrGuid) {
    schedule_book_write(id, false);
}

/// The debounced write timer (`AddBookToFileUpdate` debounces 100 ms);
/// `always_write` is the manual command path (it bypasses the
/// `AutoUpdateComicsFiles` gate, not the dirty flag).
fn schedule_book_write(id: &CrGuid, always_write: bool) {
    WRITE_TIMERS.with(|cell| {
        let mut timers = cell.borrow_mut();
        if let Some(old) = timers.remove(id) {
            old.remove();
        }
        let id = *id;
        let source = glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
            WRITE_TIMERS.with(|c| {
                c.borrow_mut().remove(&id);
            });
            let _ = update_book_file(&id, always_write);
            glib::ControlFlow::Break
        });
        timers.insert(id, source);
    });
}

/// `UpdateComics` (`MainForm`): the manual "Update all Book Files"
/// pass — `AddBookToFileUpdate(cb, alwaysWrite: true)` for every
/// book. The dirty flag STILL gates (the C# gate order: IsLinked,
/// FileInfoRetrieved, UpdateComicFiles, AutoUpdate || alwaysWrite,
/// then `ComicInfoIsDirty || (UpdateComicBookFiles &&
/// ComicBookIsDirty)`) — only the "Files to update" books write.
/// The writes drain one per main-loop tick so the UI keeps drawing
/// (the C# funnels them through the WriteComicBookInfoFileQueue).
pub fn update_all_book_files() {
    let dirty: std::collections::VecDeque<CrGuid> = {
        let lib = session();
        let l = lib.borrow();
        l.database()
            .books
            .iter()
            .filter(|b| b.comic_info_is_dirty)
            .map(|b| b.id)
            .collect()
    };
    if dirty.is_empty() {
        return;
    }
    let mut queue = dirty;
    glib::timeout_add_local(std::time::Duration::from_millis(0), move || {
        match queue.pop_front() {
            Some(id) => {
                let _ = update_book_file(&id, true);
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        }
    });
}

/// `QueueManager.AddBookToFileUpdate` + `WriteInfoToFileWithCacheUpdate`
/// for one library book: the settings gates, the metadata write into
/// the file (ComicInfo.xml, plus ComicBook.xml when
/// `UpdateComicBookFiles` is on), the file-properties refresh, and
/// the dirty-flag clear. Returns whether a write happened.
///
/// `always_write` is the manual command path (the C#
/// `AddBookToFileUpdate(cb, alwaysWrite: true)`): it bypasses the
/// `AutoUpdateComicsFiles` setting but still honors
/// `UpdateComicFiles`.
pub fn update_book_file(id: &CrGuid, always_write: bool) -> Result<bool, String> {
    let settings = settings();
    let (update_files, auto_update, update_book_files) = {
        let s = settings.borrow();
        (
            s.update_comic_files,
            s.auto_update_comics_files,
            s.update_comic_book_files,
        )
    };
    // `AddBookToFileUpdate` gates.
    if !update_files || !(auto_update || always_write) {
        return Ok(false);
    }

    let mut book = {
        let lib = session();
        let mut l = lib.borrow_mut();
        let Some(book) = l.database_mut().books.iter_mut().find(|b| b.id == *id) else {
            return Ok(false);
        };
        // Only a dirty book writes (`ComicInfoIsDirty || ...`).
        if !book.comic_info_is_dirty {
            return Ok(false);
        }
        book.clone()
    };
    if !Path::new(&book.file_path).exists() {
        return Err(format!("file not found: {}", book.file_path));
    }

    let provider =
        cr_io::ComicProvider::open(Path::new(&book.file_path)).map_err(|e| e.to_string())?;
    let written = cr_io::write::store_info_scoped(&provider, &book, update_book_files)
        .map_err(|e| e.to_string())?;
    if written {
        // `RefreshFileProperties` + the dirty-flag clear.
        crate::library::refresh_file_info(&mut book);
        book.comic_info_is_dirty = false;
        let lib = session();
        let mut l = lib.borrow_mut();
        if let Some(slot) = l.database_mut().books.iter_mut().find(|b| b.id == *id) {
            *slot = book;
            l.mark_dirty();
        }
    }
    Ok(written)
}

/// Removes one book from the library by id (the context-menu
/// command; the file on disk is untouched).
pub fn remove_book(id: &CrGuid) {
    let lib = session();
    let mut l = lib.borrow_mut();
    let before = l.database().books.len();
    l.database_mut().books.retain(|b| b.id != *id);
    if l.database().books.len() != before {
        l.mark_dirty();
    }
}

/// The file path of one library book (the ItemView activate path).
pub fn book_path(id: &CrGuid) -> Option<String> {
    let lib = session();
    let l = lib.borrow();
    l.database()
        .books
        .iter()
        .find(|b| b.id == *id)
        .map(|b| b.file_path.clone())
}

/// `ComicDatabase.GetRecentFiles(count)`: the books ordered by
/// `OpenedTime` (newest first), `count` entries — the File ▸ Recent
/// Books fill (`Settings.RecentFileCount` = 20; `OpenedTime` is
/// NULL-ordered last in the C# LINQ — CrDateTime::min_value sorts
/// the same way here).
pub fn recent_books(count: usize) -> Vec<ComicBook> {
    let lib = session();
    let l = lib.borrow();
    let mut books: Vec<ComicBook> = l.database().books.clone();
    books.sort_by_key(|b| std::cmp::Reverse(b.opened_time.naive));
    books.truncate(count);
    books
}

/// The QuickOpen lists (`FillWithQuickOpenBooks`): the three built-in
/// lists — Reading (ReadPercentage in 10..95), Recently Read
/// (OpenedTime in the last 14 days), Recently Added (AddedTime in
/// the last 14 days) — deduped by book id across groups, sorted by
/// OpenedTime desc (tie: AddedTime desc), 10 per group.
pub fn quick_open_lists() -> Vec<(String, Vec<ComicBook>)> {
    use cr_core::database::list_items::{
        ComicBookMatcher, ComicListItem, ListItemBase, SmartListItem, ValueMatcher,
    };
    use cr_core::xml::scalar::CrGuid;

    let matcher = |type_name: &str, v1: &str, v2: &str| {
        ComicBookMatcher::Value(ValueMatcher {
            type_name: type_name.into(),
            match_operator: 3,
            match_value: v1.into(),
            match_value_2: v2.into(),
            ..Default::default()
        })
    };
    // The engine-configuration values (the C# fills the built-in
    // lists from `EngineConfiguration.Default`).
    let engine = cr_core::settings::EngineConfiguration::global();
    let (recent, read_at, not_read_at) = (
        engine.is_recent_in_days.to_string(),
        engine.is_read_completion_percentage.to_string(),
        engine.is_not_read_completion_percentage.to_string(),
    );
    let groups: Vec<(&str, ComicBookMatcher)> = vec![
        (
            "Reading",
            matcher("ComicBookReadPercentageMatcher", &not_read_at, &read_at),
        ),
        (
            "Recently Read",
            matcher("ComicBookOpenedMatcher", &recent, ""),
        ),
        (
            "Recently Added",
            matcher("ComicBookAddedMatcher", &recent, ""),
        ),
    ];

    let lib = session();
    let l = lib.borrow();
    let mut seen: Vec<CrGuid> = Vec::new();
    let mut out: Vec<(String, Vec<ComicBook>)> = Vec::new();
    for (name, m) in groups {
        let item = ComicListItem::Smart(SmartListItem {
            base: ListItemBase {
                id: CrGuid::new_random(),
                name: Some(name.into()),
                ..Default::default()
            },
            matchers: vec![m],
            ..Default::default()
        });
        let matched = cr_engine::lists::evaluate_list(&item, l.database());
        let mut books: Vec<ComicBook> = matched
            .into_iter()
            .filter(|b| {
                if seen.contains(&b.id) {
                    return false;
                }
                seen.push(b.id);
                true
            })
            .cloned()
            .collect();
        // OpenedTime desc, tie → AddedTime desc.
        books.sort_by(|a, b| {
            b.opened_time
                .naive
                .cmp(&a.opened_time.naive)
                .then_with(|| b.added_time.naive.cmp(&a.added_time.naive))
        });
        books.truncate(cr_core::settings::ExtendedSettings::global().quick_open_list_size as usize);
        out.push((name.to_string(), books));
    }
    out
}

// ---------- The smart-list editor (Phase 5 T3) ----------

/// The item for the editor (a clone; the editor edits the clone and
/// commits it).
pub fn find_smart_list(id: &CrGuid) -> Option<cr_core::database::list_items::SmartListItem> {
    let lib = session();
    let l = lib.borrow();
    match cr_engine::lists::find_list_item(&l.database().comic_lists, id)? {
        cr_core::database::list_items::ComicListItem::Smart(s) => Some(s.clone()),
        _ => None,
    }
}

/// `ComicSmartListItem.SetList`: replaces the smart-list item's
/// model fields (position + id stay; the extra values move over).
pub fn update_smart_list(id: &CrGuid, item: cr_core::database::list_items::SmartListItem) -> bool {
    let lib = session();
    let mut l = lib.borrow_mut();
    fn apply(
        items: &mut [cr_core::database::list_items::ComicListItem],
        id: &CrGuid,
        new: &cr_core::database::list_items::SmartListItem,
    ) -> bool {
        for i in items.iter_mut() {
            match i {
                cr_core::database::list_items::ComicListItem::Smart(s) if s.base.id == *id => {
                    let mut next = new.clone();
                    next.base.id = s.base.id;
                    next.base.book_count = s.base.book_count;
                    next.base.new_book_count = s.base.new_book_count;
                    next.base.unread_book_count = s.base.unread_book_count;
                    next.base.cache_storage = s.base.cache_storage.clone();
                    *s = next;
                    return true;
                }
                cr_core::database::list_items::ComicListItem::Folder(f) => {
                    if apply(&mut f.items, id, new) {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }
    let changed = apply(&mut l.database_mut().comic_lists, id, &item);
    if std::env::var("CR_DEBUG_SL").is_ok() {
        eprintln!("update_smart_list id={id} changed={changed}");
    }
    if changed {
        l.mark_dirty();
    }
    changed
}

/// The base-list combo options: every list item (Library = the empty
/// Guid) whose subtree does not reference the edited id (the C#
/// `RecursionTest`).
pub fn smart_list_base_options(edit_id: &CrGuid) -> Vec<(CrGuid, String)> {
    /// Whether the base chain starting at `start` reaches `edit_id`
    /// (the recursion guard the C# applies as `RecursionTest`).
    fn chain_references(
        l: &std::rc::Rc<std::cell::RefCell<Library>>,
        start: &CrGuid,
        edit_id: &CrGuid,
    ) -> bool {
        if start == edit_id {
            return true;
        }
        let Some(item) =
            cr_engine::lists::find_list_item(&l.borrow().database().comic_lists, start)
        else {
            return false;
        };
        if let cr_core::database::list_items::ComicListItem::Smart(s) = &item {
            if !s.base_list_id.is_empty() {
                return chain_references(l, &s.base_list_id, edit_id);
            }
        }
        false
    }
    let lib = session();
    let l = lib.borrow();
    let mut out = Vec::new();
    fn walk(
        items: &[cr_core::database::list_items::ComicListItem],
        edit_id: &CrGuid,
        out: &mut Vec<(CrGuid, String)>,
        lib: &std::rc::Rc<std::cell::RefCell<Library>>,
    ) {
        for i in items {
            match i {
                cr_core::database::list_items::ComicListItem::Library(_) => {
                    out.push((CrGuid::EMPTY, "Library".to_string()));
                }
                cr_core::database::list_items::ComicListItem::Smart(s) => {
                    let name = s.base.name.clone().unwrap_or_default();
                    // Skip candidates whose base chain would recurse
                    // through the edited list (the C# RecursionTest).
                    if s.base.id != *edit_id && !chain_references(lib, &s.base_list_id, edit_id) {
                        out.push((s.base.id, name));
                    }
                }
                cr_core::database::list_items::ComicListItem::Folder(f) => {
                    let name = f.base.name.clone().unwrap_or_default();
                    if f.base.id != *edit_id {
                        out.push((f.base.id, format!("{name} (folder)")));
                    }
                    walk(&f.items, edit_id, out, lib);
                }
                cr_core::database::list_items::ComicListItem::IdList(_) => {}
            }
        }
    }
    walk(&l.database().comic_lists, edit_id, &mut out, &lib);
    out
}

/// `NewList`: inserts an empty reading list (the `ComicIdListItem`).
/// Returns the new id.
pub fn new_id_list(after: Option<&CrGuid>, name: &str) -> CrGuid {
    let id = CrGuid::new_random();
    let item = cr_core::database::list_items::ComicListItem::IdList(
        cr_core::database::list_items::IdListItem {
            base: cr_core::database::list_items::ListItemBase {
                id,
                name: Some(name.to_string()),
                ..Default::default()
            },
            book_ids: Vec::new(),
        },
    );
    insert_list_item(after, item);
    id
}

/// `EditListDialog.Edit` result for one item: the fields the dialog
/// edits (the rest of the item stays).
pub struct ListEditFields {
    pub name: String,
    pub description: String,
    pub quick_open: bool,
    pub combine_mode: Option<cr_core::model::enums::ComicFolderCombineMode>,
}

/// Applies [`ListEditFields`] to a folder or reading list (the C#
/// `EditListDialog.Edit` write-back; `SetList` parity for the base
/// fields). Returns false when the id is not a folder/id list.
pub fn update_list_fields(id: &CrGuid, fields: &ListEditFields) -> bool {
    let lib = session();
    let mut l = lib.borrow_mut();
    fn apply(
        items: &mut [cr_core::database::list_items::ComicListItem],
        id: &CrGuid,
        f: &ListEditFields,
    ) -> bool {
        for i in items.iter_mut() {
            match i {
                cr_core::database::list_items::ComicListItem::Folder(folder)
                    if folder.base.id == *id =>
                {
                    folder.base.name = Some(f.name.clone());
                    folder.base.description = f.description.clone();
                    if let Some(mode) = f.combine_mode {
                        folder.combine_mode = mode;
                    }
                    return true;
                }
                cr_core::database::list_items::ComicListItem::IdList(list)
                    if list.base.id == *id =>
                {
                    list.base.name = Some(f.name.clone());
                    list.base.description = f.description.clone();
                    list.base.quick_open = f.quick_open;
                    return true;
                }
                cr_core::database::list_items::ComicListItem::Folder(folder) => {
                    if apply(&mut folder.items, id, f) {
                        return true;
                    }
                }
                _ => {}
            }
        }
        false
    }
    let changed = apply(&mut l.database_mut().comic_lists, id, fields);
    if changed {
        l.mark_dirty();
    }
    changed
}

/// The item clone for ANY kind (the editor routing reads it).
pub fn find_list_item_any(id: &CrGuid) -> Option<cr_core::database::list_items::ComicListItem> {
    let lib = session();
    let l = lib.borrow();
    cr_engine::lists::find_list_item(&l.database().comic_lists, id)
}

thread_local! {
    /// `Program.Settings.CurrentExportSetting` (session-only; the
    /// Config.xml block joins when the settings schema grows the
    /// export lists).
    static LAST_EXPORT: RefCell<Option<cr_io::export::ExportSetting>> = const { RefCell::new(None) };
}

pub fn remember_export_setting(setting: cr_io::export::ExportSetting) {
    LAST_EXPORT.with(|c| *c.borrow_mut() = Some(setting));
}

pub fn last_export_setting() -> Option<cr_io::export::ExportSetting> {
    LAST_EXPORT.with(|c| c.borrow().clone())
}
