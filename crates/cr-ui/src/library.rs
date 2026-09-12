//! The library session — the cr-ui wiring of
//! `cr_engine::library::Library` (the C# `Program.DatabaseManager`
//! static): open at startup, save on exit, the 600 s background save,
//! the reader/book integration (the C# `ComicBookFactory`), and the
//! scan worker (the C# "Book Scanner" low-priority thread).

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::rc::Rc;

use cr_core::database::comic_database::OpenStatus;
use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::{CrDateTime, CrGuid};
use cr_engine::library::Library;
use cr_engine::scanner::{
    create_book, refresh_file_info, refresh_file_info_basic, ScanItem, ScanResult,
};
use glib::ControlFlow;
use gtk4::glib;

/// One queued scan request (the C# scan queue): the items to walk,
/// the per-file limits, and the completion callback. A folder scan
/// carries one recursive item; an explicit-path scan (`scan_files`)
/// carries one file item per path.
struct QueuedScan {
    label: String,
    items: Vec<ScanItem>,
    limits: cr_engine::scanner::ScanLimits,
    done: Box<dyn FnOnce(ScanResult)>,
}

/// The shell's per-batch view refresh (a Weak capture lives inside).
type ScanViewHook = Box<dyn Fn(&[ComicBook])>;

thread_local! {
    static SESSION: RefCell<Option<Rc<RefCell<Library>>>> = const { RefCell::new(None) };
    /// One scan at a time (the C# scan queue): requests arriving
    /// while the worker runs wait here, in arrival order.
    static SCAN_QUEUE: RefCell<Vec<QueuedScan>> = const { RefCell::new(Vec::new()) };
    static SCAN_IN_FLIGHT: RefCell<bool> = const { RefCell::new(false) };
    /// The location the in-flight scan walks (`Scanner.CurrentLocation`
    /// — the Tasks dialog's scan row).
    static SCAN_LOCATION: RefCell<String> = const { RefCell::new(String::new()) };
    /// The in-flight scan's abort flag (`Scanner.Stop`'s volatile
    /// `abortScanning`): set by `abort_scan` (the Tasks abort), read
    /// per walked file by the worker.
    static SCAN_STOP: RefCell<Option<std::sync::Arc<std::sync::atomic::AtomicBool>>> =
        const { RefCell::new(None) };
    /// The in-flight scan's "Skip Current File" flag. `skip_current_
    /// scan_file` sets it; the worker CONSUMES it (one press abandons
    /// one file) and carries on with the next file.
    static SCAN_SKIP: RefCell<Option<std::sync::Arc<std::sync::atomic::AtomicBool>>> =
        const { RefCell::new(None) };
    /// The problems accumulated across the scans of one run (a full
    /// scan queues one request per watch root). Reported once, when the
    /// last queued scan lands.
    static SCAN_PROBLEMS: RefCell<ScanProblemSummary> =
        const { RefCell::new(ScanProblemSummary {
            unreadable: 0,
            mismatched: 0,
            timed_out: 0,
            skipped: 0,
            skipped_known_bad: 0,
        }) };
    /// Book ids removed on the main thread while a scan is in flight
    /// (the remove flow records them): the landing merge drops them
    /// from the worker's storage copy instead of resurrecting them.
    static SCAN_REMOVED_IDS: RefCell<HashSet<CrGuid>> = RefCell::new(HashSet::new());
    /// Book ids mutated on the main thread while a scan is in flight
    /// (edits, reading state, page sizes, write-back results): the
    /// landing merge keeps the DATABASE copy for them (the worker's
    /// storage copy is the pre-scan state).
    static SCAN_TOUCHED_IDS: RefCell<HashSet<CrGuid>> = RefCell::new(HashSet::new());
    /// The per-batch view refresh (the C# scan events update the live
    /// views): the shell installs it, the scan pump fires it when new
    /// books land mid-scan. The slice carries the tick's new books
    /// (the incremental append); an EMPTY slice means the landing
    /// (the full refresh).
    static SCAN_VIEW_HOOK: RefCell<Option<ScanViewHook>> =
        const { RefCell::new(None) };
    /// The user settings (`Program.Settings` static). The GTK code
    /// reaches it through [`settings`].
    static SETTINGS: RefCell<Option<Rc<RefCell<cr_core::settings::Settings>>>> =
        const { RefCell::new(None) };
}

/// Installs the per-batch scan view refresh (the shell does this at
/// creation; the hook must not own the shell — use a Weak capture).
pub fn set_scan_view_hook(hook: Option<ScanViewHook>) {
    SCAN_VIEW_HOOK.with(|cell| *cell.borrow_mut() = hook);
}

fn fire_scan_view_hook(batch: &[ComicBook]) {
    SCAN_VIEW_HOOK.with(|cell| {
        if let Some(hook) = cell.borrow().as_ref() {
            hook(batch);
        }
    });
}

/// Records a book id removed on the main thread while a scan is in
/// flight (the landing merge must not resurrect it from the worker's
/// storage copy).
pub fn record_scan_removal(id: &CrGuid) {
    if is_scanning() {
        SCAN_REMOVED_IDS.with(|s| s.borrow_mut().insert(*id));
    }
}

/// Records a book id the main thread mutated while a scan is in
/// flight (the landing merge keeps the database copy for it).
fn record_scan_touch(id: &CrGuid) {
    if is_scanning() {
        SCAN_TOUCHED_IDS.with(|s| s.borrow_mut().insert(*id));
    }
}

/// Drains the mid-scan side-effect records (the landing merge).
fn take_scan_side_effects() -> (HashSet<CrGuid>, HashSet<CrGuid>) {
    (
        SCAN_REMOVED_IDS.with(|s| std::mem::take(&mut *s.borrow_mut())),
        SCAN_TOUCHED_IDS.with(|s| std::mem::take(&mut *s.borrow_mut())),
    )
}

/// The landing merge: the worker's scanned storage is the master copy
/// (the scanned file-info updates + the new books). What the main
/// thread did WHILE the scan ran is preserved on top: database-only
/// books stay (mid-scan adds), touched ids keep the database copy
/// (mid-scan edits and reads), removed ids drop everywhere. (The C#
/// scans the LIVE collection — one shared storage, no reconciliation;
/// the clone-per-scan split needs this merge.)
fn merge_scan_storage(
    worker: Vec<ComicBook>,
    db_books: &[ComicBook],
    removed: &HashSet<CrGuid>,
    touched: &HashSet<CrGuid>,
) -> Vec<ComicBook> {
    let worker_ids: HashSet<CrGuid> = worker.iter().map(|b| b.id).collect();
    let mut db_wins: HashMap<CrGuid, ComicBook> = HashMap::new();
    let mut preserved: Vec<ComicBook> = Vec::new();
    for b in db_books {
        if removed.contains(&b.id) {
            continue;
        }
        if worker_ids.contains(&b.id) {
            if touched.contains(&b.id) {
                db_wins.insert(b.id, b.clone());
            }
        } else {
            preserved.push(b.clone());
        }
    }
    let mut merged: Vec<ComicBook> = worker
        .into_iter()
        .filter(|b| !removed.contains(&b.id))
        .collect();
    for b in merged.iter_mut() {
        if let Some(db) = db_wins.get(&b.id) {
            *b = db.clone();
        }
    }
    merged.extend(preserved);
    merged
}

/// `Scanner.Stop(clearQueue: true)` — the Tasks dialog's "Abort
/// Scanning": drops the queued scan requests and flags the in-flight
/// scan to stop at the next file. The books found so far stay (the
/// batches already landed; the worker's partial storage merges back).
pub fn abort_scan() {
    SCAN_QUEUE.with(|q| q.borrow_mut().clear());
    SCAN_STOP.with(|cell| {
        if let Some(flag) = cell.borrow().as_ref() {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    });
    crate::trace::trace("scan abort requested");
}

/// "Skip Current File" (PORT ADDITION, user request 2026-09-11 — no
/// C# counterpart): abandon the file the scan is reading right now and
/// continue with the next one. The whole scan keeps running.
///
/// This is the manual escape hatch. It is NOT the normal recovery
/// path: the scanner already bounds every file with its own deadline
/// and marks a file it had to abandon, so an unattended scan never
/// waits for this button.
pub fn skip_current_scan_file() {
    let requested = SCAN_SKIP.with(|cell| {
        if let Some(flag) = cell.borrow().as_ref() {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
            true
        } else {
            false
        }
    });
    crate::trace::trace(format!("scan skip requested (in flight: {requested})"));
}

/// The problems found since the current run of scans began. A full
/// library scan queues one request per watch root, so the counts
/// accumulate and are reported ONCE, when the last queued scan lands.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScanProblemSummary {
    pub unreadable: usize,
    pub mismatched: usize,
    pub timed_out: usize,
    pub skipped: usize,
    pub skipped_known_bad: usize,
}

impl ScanProblemSummary {
    pub fn total(&self) -> usize {
        self.unreadable + self.mismatched + self.timed_out + self.skipped
    }

    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }

    /// The one-paragraph report shown after a scan, and the text a
    /// user can act on with a smart list.
    pub fn message(&self) -> String {
        let mut lines: Vec<String> = Vec::new();
        if self.unreadable > 0 {
            lines.push(format!("{} could not be read", self.unreadable));
        }
        if self.timed_out > 0 {
            lines.push(format!(
                "{} took too long and were abandoned",
                self.timed_out
            ));
        }
        if self.skipped > 0 {
            lines.push(format!("{} were skipped by hand", self.skipped));
        }
        if self.mismatched > 0 {
            lines.push(format!(
                "{} have contents that do not match their file name (these were imported)",
                self.mismatched
            ));
        }
        if self.skipped_known_bad > 0 {
            lines.push(format!(
                "{} were left alone because they failed before and have not changed",
                self.skipped_known_bad
            ));
        }
        lines.join("\n")
    }
}

/// Adds one scan run's problems to the session summary.
fn record_scan_problems(result: &ScanResult) {
    SCAN_PROBLEMS.with(|cell| {
        let mut s = cell.borrow_mut();
        s.unreadable += result.unreadable.len();
        s.mismatched += result.mismatched.len();
        s.timed_out += result.timed_out.len();
        s.skipped += result.skipped.len();
        s.skipped_known_bad += result.skipped_known_bad.len();
    });
}

/// Takes the accumulated summary and resets it. The shell calls this
/// when the last queued scan lands.
pub fn take_scan_problem_summary() -> ScanProblemSummary {
    SCAN_PROBLEMS.with(|cell| std::mem::take(&mut *cell.borrow_mut()))
}

/// Opens the library database at the default location (`Program`'s
/// startup `DatabaseManager.Open`). Returns the `OpenMessage` the C#
/// would show in the attention dialog (None for a plain load).
///
/// Also loads the settings layer: the ONE unified config file
/// `comicrust.toml` (the C# `Settings.Load` + the `IniFile` chain —
/// ADR-033) plus the argv switches for `EngineConfiguration`/
/// `ExtendedSettings` (the `CommandLineParser` boot).
pub fn initialize() -> Result<Option<String>, cr_core::database::DbError> {
    let (library, status) = Library::open_at_default_location()?;
    let message = open_message(status);
    SESSION.with(|cell| {
        *cell.borrow_mut() = Some(Rc::new(RefCell::new(library)));
    });
    initialize_settings();
    Ok(message)
}

/// The settings boot (`Settings.Load` + the ini keys + argv). A
/// missing or corrupt config file falls back to the defaults (the C#
/// catch parity); the first boot seeds the built-in data tables into
/// the file.
fn initialize_settings() {
    let paths = cr_core::paths::Paths::new_default();
    let argv: Vec<String> = std::env::args().skip(1).collect();

    // The unified file: reads `[extended]`/`[engine]`/`[settings]`/
    // `[plugins]`/`[data]` into the session (and seeds the data
    // tables, rewriting the file when it did).
    let loaded = cr_core::settings::unified::load(&cr_core::paths::config_file(&paths));

    let mut engine = cr_core::settings::EngineConfiguration::default();
    engine.load(&loaded.engine);
    cr_core::settings::EngineConfiguration::init_global(engine);

    let mut extended = cr_core::settings::ExtendedSettings::default();
    extended.load(&loaded.extended, &argv);
    // The C# `Program.ExtendedSettings` getter (Program.cs:160-165):
    // a `-restart` boot clears the one-shot arguments so the
    // restarted instance opens nothing.
    if extended.restart {
        extended.files.clear();
        extended.import_list = None;
        extended.install_plugin = None;
    }
    if std::env::var("CR_DEBUG_SL").is_ok() {
        eprintln!(
            "[cache-ov] config={:?} ext-cache-path={:?} argv={argv:?}",
            cr_core::paths::config_file(&paths),
            loaded.extended.get("CachePath")
        );
    }
    cr_core::settings::ExtendedSettings::init_global(extended);

    SETTINGS.with(|cell| {
        *cell.borrow_mut() = Some(Rc::new(RefCell::new(loaded.settings)));
    });
}

/// The user settings (`Program.Settings`). Panics before
/// [`initialize`].
pub fn settings() -> Rc<RefCell<cr_core::settings::Settings>> {
    SETTINGS.with(|cell| cell.borrow().clone().expect("settings not initialized"))
}

/// `Settings.Save(defaultSettingsFile)` (the C# app-exit step):
/// rewrites the whole unified config file — the settings plus the
/// `[extended]`/`[engine]`/`[plugins]`/`[data]` session sections.
pub fn save_settings() {
    let paths = cr_core::paths::Paths::new_default();
    let config_file = cr_core::paths::config_file(&paths);
    let s = settings();
    let _ = cr_core::settings::unified::save_file(&config_file, &s.borrow())
        .inspect_err(|e| eprintln!("saving {config_file:?} failed: {e}"));
}

/// Persists `ExtendedSettings` keys into the `[extended]` section of
/// the unified config (the old ini merge-writer's replacement — the
/// theme toggle and the cache-path preference write through here; a
/// boot applies the keys, argv still wins).
pub fn save_ini_keys(keys: &[(&str, &str)]) {
    cr_core::settings::unified::update_extended_keys(keys);
    save_settings();
}

// ---------- Plugin configs ([plugins.*] of the unified config) ----------

/// The Comic Vine Scraper's section name in the unified config.
pub const SCRAPER_PLUGIN: &str = "comic-vine-scraper";

/// The scraper's stored configuration (the defaults when the section
/// is absent — the C# `load_map` parity). The parsed advanced
/// settings reparse from `advanced_settings` (serde skips them — the
/// old file-load shape).
pub fn scraper_config() -> cr_scrape::config::Configuration {
    let mut config: cr_scrape::config::Configuration =
        cr_core::settings::unified::get_plugin(SCRAPER_PLUGIN).unwrap_or_default();
    let raw = config.advanced_settings.clone();
    config.set_advanced_settings(&raw);
    config
}

/// Commits the scraper configuration into `[plugins.comic-vine-scraper]`
/// and saves the unified config (the old plugin-local settings.json
/// write — ADR-031's file store is superseded by ADR-033).
pub fn store_scraper_config(config: &cr_scrape::config::Configuration) {
    cr_core::settings::unified::set_plugin(SCRAPER_PLUGIN, config);
    save_settings();
}

/// The Comic Vine disk cache (ADR-037), opened once per process.
///
/// The cache is a file, and the sweep, the warm task, and the scrape
/// worker all use it. `None` means the file could not be opened; the
/// caller must then work without a cache, not fail.
pub fn cv_cache() -> Option<std::sync::Arc<cr_scrape::cache::SqliteCache>> {
    use std::sync::OnceLock;
    static CACHE: OnceLock<Option<std::sync::Arc<cr_scrape::cache::SqliteCache>>> = OnceLock::new();
    CACHE
        .get_or_init(|| {
            let path = cr_scrape::cache::default_cache_path();
            match cr_scrape::cache::SqliteCache::open(&path) {
                Ok(cache) => Some(std::sync::Arc::new(cache)),
                Err(e) => {
                    crate::trace::trace(format!("the Comic Vine cache could not open: {e}"));
                    None
                }
            }
        })
        .clone()
}

/// The per-resource request budget over the shared cache, built from
/// the scraper configuration. `None` means no budget: either the cache
/// is off, or its file could not open.
pub fn cv_budget(
    config: &cr_scrape::config::Configuration,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> Option<std::sync::Arc<cr_scrape::cache::budget::Budget>> {
    if !config.advanced().cache_enabled {
        return None;
    }
    let cache = cv_cache()?;
    let (policy, _, _) = cr_scrape::cache::policies_from(config.advanced());
    Some(std::sync::Arc::new(
        cr_scrape::cache::budget::Budget::new(
            cache as std::sync::Arc<dyn cr_scrape::cache::CvCache>,
            policy,
        )
        .with_cancel(cancel),
    ))
}

/// The Comic Vine volume ids the library names, most used first. The
/// warm task spends its budget on these.
pub fn cv_volume_ids(config: &cr_scrape::config::Configuration) -> Vec<i64> {
    use std::collections::BTreeMap;
    let lib = session();
    let l = lib.borrow();
    let mut votes: BTreeMap<i64, usize> = BTreeMap::new();
    for book in &l.database().books {
        let data = cr_scrape::bookdata::BookData::from_book(book, config);
        if let Ok(id) = data.series_key.trim().parse::<i64>() {
            if id > 0 {
                *votes.entry(id).or_default() += 1;
            }
        }
    }
    let mut ids: Vec<(i64, usize)> = votes.into_iter().collect();
    ids.sort_by_key(|&(id, count)| (std::cmp::Reverse(count), id));
    ids.into_iter().map(|(id, _)| id).collect()
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
        custom_thumb_dir: Some(paths.custom_thumbnail_path),
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
                record_scan_touch(&book.id);
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

/// The session when one is initialized (the library-wide key walks
/// tolerate the session-free probes; the C# static would NRE).
pub fn try_session() -> Option<Rc<RefCell<Library>>> {
    SESSION.with(|cell| cell.borrow().clone())
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
        record_scan_touch(&book.id);
        found = Some(book.clone());
    }
    if found.is_some() {
        lib.mark_dirty();
        return found;
    }
    let add_to_library = settings().borrow().add_to_library_on_open;
    if add_to_library && Path::new(path).exists() {
        // `ComicBookFactory.Create(file, AddToStorage, GetFastPageCount)`
        // (ComicBookFactory.cs:79-90): the fresh book goes through
        // `ComicBook.Create` → `RefreshInfoFromFile` — the scan
        // defaults, the file properties, the info chain (ComicInfo.xml
        // etc.) and the page count — then joins the storage.
        let now = CrDateTime::now();
        let book = create_book(path, &now);
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
        record_scan_touch(&book.id);
        lib.mark_dirty();
    }
}

/// `AddFolderToLibrary` — scans the folder (recursively, no removal)
/// into the library on the scan worker. `done` runs on the UI thread
/// with the result.
pub fn add_folder_to_library(path: &Path, done: impl FnOnce(ScanResult) + 'static) {
    scan_async(path.to_string_lossy().into_owned(), done);
}

/// Scans explicit file paths (the context-menu "Rescan Book File(s)"
/// and the navigator "Scan List Contents", ADR-036): ONE request of
/// one-file items, no folder walk. `force_retry` re-opens files that
/// already carry an unchanged failure verdict, for this request only —
/// the global `ScanRetryFailedFiles` keeps its meaning for folder
/// scans. `done` runs on the UI thread with the result; an empty path
/// list completes immediately with an empty result.
pub fn scan_files(
    paths: &[String],
    label: &str,
    force_retry: bool,
    done: impl FnOnce(ScanResult) + 'static,
) {
    let mut limits = scan_limits();
    limits.retry_failed |= force_retry;
    let items: Vec<ScanItem> = distinct_paths(paths.iter().cloned())
        .into_iter()
        .map(|path| ScanItem {
            location: path,
            all: false,
            remove_missing: false,
            force_refresh_info: false,
        })
        .collect();
    if items.is_empty() {
        done(ScanResult::default());
        return;
    }
    queue_scan(QueuedScan {
        label: label.to_string(),
        items,
        limits,
        done: Box::new(done),
    });
}

/// Distinct non-empty paths, first-seen order. A fileless book (empty
/// path) has no file to scan, and a file listed twice must be read
/// once.
fn distinct_paths<I>(paths: I) -> Vec<String>
where
    I: IntoIterator<Item = String>,
{
    let mut out: Vec<String> = Vec::new();
    for path in paths {
        if path.is_empty() || out.contains(&path) {
            continue;
        }
        out.push(path);
    }
    out
}

/// The distinct non-empty file paths of the given ids, in id order
/// (the book menu's "Rescan Book File(s)" target set).
pub fn book_paths_for_ids(ids: &[CrGuid]) -> Vec<String> {
    let lib = session();
    let l = lib.borrow();
    distinct_paths(ids.iter().filter_map(|id| {
        l.database()
            .books
            .iter()
            .find(|b| b.id == *id)
            .map(|b| b.file_path.clone())
    }))
}

/// The distinct non-empty file paths of one list's books, with the
/// list's display name (the navigator's "Scan List Contents"). The
/// evaluation is the same path the browser fills
/// (`evaluate_books`).
pub fn list_book_paths(id: &CrGuid) -> Option<(String, Vec<String>)> {
    let (name, books) = evaluate_books(id)?;
    let paths = distinct_paths(books.iter().map(|b| b.file_path.clone()));
    Some((name, paths))
}

/// The scan worker (the C# `ComicScanner` runs its queue on a
/// dedicated low-priority "Book Scanner" thread; a synchronous scan
/// freezes the UI on real libraries). The worker scans a CLONE of the
/// book storage — the database keeps the FULL library while the scan
/// runs (smart lists, the quick search and every list evaluation stay
/// live mid-scan) — and reports back over std mpsc; a
/// `timeout_add_local` pump merges it (the ADR-019 pattern). New
/// books ride incremental batches to the pump (the C# adds to the
/// live storage per file — the view fills during the walk,
/// `ComicBookCollection.Add` → `OnBookAdded`); the final storage
/// merges at the landing, keeping what the main thread added / edited
/// / removed while the scan ran. Requests arriving mid-scan queue and
/// run in arrival order.
fn scan_async(location: String, done: impl FnOnce(ScanResult) + 'static) {
    queue_scan(QueuedScan {
        label: location.clone(),
        items: vec![ScanItem {
            location,
            all: true,
            remove_missing: false,
            force_refresh_info: false,
        }],
        limits: scan_limits(),
        done: Box::new(done),
    });
}

/// Runs the request now, or queues it behind the in-flight scan (the
/// C# scan queue: requests arriving mid-scan wait, one at a time).
fn queue_scan(q: QueuedScan) {
    let in_flight = SCAN_IN_FLIGHT.with(|cell| *cell.borrow());
    if in_flight {
        SCAN_QUEUE.with(|queue| queue.borrow_mut().push(q));
        return;
    }
    start_scan_worker(q);
}

/// Worker → pump messages: incremental new-book batches and the
/// final merge.
enum ScanWorkerMsg {
    Batch(Vec<ComicBook>),
    Done(Vec<ComicBook>, Box<ScanResult>),
}

/// Books per batch send (the pump appends + refreshes per tick; a
/// book clones once for its batch, so the cost is O(N) total).
const SCAN_BATCH_SIZE: usize = 20;

/// The per-file scan limits from the unified config
/// (`ScanFileTimeoutSeconds`, `ScanRetryFailedFiles`). A timeout of 0
/// disables the deadline.
fn scan_limits() -> cr_engine::scanner::ScanLimits {
    let settings = cr_core::settings::ExtendedSettings::global();
    let seconds = settings.scan_file_timeout_seconds;
    cr_engine::scanner::ScanLimits {
        per_file_timeout: (seconds > 0).then(|| std::time::Duration::from_secs(seconds as u64)),
        retry_failed: settings.scan_retry_failed_files,
    }
}

/// Takes the book storage, runs the scan on a worker thread, and
/// pumps batches + the result back onto the UI thread.
fn start_scan_worker(q: QueuedScan) {
    let location = q.label.clone();
    let library = session();
    let books = {
        let lib = library.borrow_mut();
        SCAN_IN_FLIGHT.with(|cell| *cell.borrow_mut() = true);
        SCAN_LOCATION.with(|cell| *cell.borrow_mut() = q.label.clone());
        // A CLONE, not a take: the database keeps the full library
        // while the scan runs. The take emptied it — a re-scan sends
        // zero batches (only NEW files fire `on_new`), so every list
        // evaluation read an empty library mid-scan and the search
        // results blanked until a restart (user report 2026-09-11).
        // The landing merge reconciles the worker's updates with the
        // mid-scan side effects.
        lib.database().books.clone()
    };
    let items = q.items;
    let limits = q.limits;
    let now = CrDateTime::now();
    let (tx, rx) = std::sync::mpsc::channel::<ScanWorkerMsg>();
    // The per-file progress (the C# `currentLocation` per walked
    // file, ComicScanner.cs:125): the worker ships paths, the pump
    // moves SCAN_LOCATION — the Tasks "Scanning" line tracks the walk.
    let (ptx, prx) = std::sync::mpsc::channel::<String>();
    // The abort flag (`Scanner.Stop`'s volatile `abortScanning`):
    // `abort_scan` sets it, the worker checks per walked file.
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    SCAN_STOP.with(|cell| *cell.borrow_mut() = Some(std::sync::Arc::clone(&stop)));
    // The "Skip Current File" flag: set by the status-bar/Tasks row,
    // CONSUMED by the worker so one press abandons exactly one file.
    let skip = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    SCAN_SKIP.with(|cell| *cell.borrow_mut() = Some(std::sync::Arc::clone(&skip)));
    std::thread::Builder::new()
        .name("Book Scanner".into())
        .spawn(move || {
            let mut storage = books;
            let t = std::time::Instant::now();
            crate::trace::trace(format!("scan start '{location}'"));
            let mut batch: Vec<ComicBook> = Vec::new();
            let stop_fn = || stop.load(std::sync::atomic::Ordering::Relaxed);
            let skip_fn = || {
                skip.swap(false, std::sync::atomic::Ordering::Relaxed)
            };
            let control = cr_engine::scanner::ScanControl {
                stop: &stop_fn,
                take_skip: &skip_fn,
            };
            let result = cr_engine::scanner::scan_sync_with_control(
                &mut storage,
                &items,
                &now,
                &mut |f: &Path| {
                    let _ = ptx.send(f.to_string_lossy().into_owned());
                },
                &control,
                limits,
                &mut |book: &ComicBook| {
                    batch.push(book.clone());
                    if batch.len() >= SCAN_BATCH_SIZE {
                        let _ = tx.send(ScanWorkerMsg::Batch(std::mem::take(&mut batch)));
                    }
                },
            );
            let aborted = stop.load(std::sync::atomic::Ordering::Relaxed);
            if !batch.is_empty() {
                let _ = tx.send(ScanWorkerMsg::Batch(batch));
            }
            crate::trace::trace(format!(
                "scan {} '{location}' in {} ms: added {} updated {} moved {} removed {}",
                if aborted { "aborted" } else { "done" },
                t.elapsed().as_millis(),
                result.added.len(),
                result.updated.len(),
                result.moved.len(),
                result.removed.len()
            ));
            crate::trace::trace(format!(
                "scan problems '{location}': unreadable {} mismatched {} timed out {} skipped {} known-bad skipped {}",
                result.unreadable.len(),
                result.mismatched.len(),
                result.timed_out.len(),
                result.skipped.len(),
                result.skipped_known_bad.len()
            ));
            let _ = tx.send(ScanWorkerMsg::Done(storage, Box::new(result)));
        })
        .expect("spawn Book Scanner");

    // The once-completion callback rides an Option (the pump closure
    // is FnMut — it cannot move `done` out).
    let mut done = Some(q.done);
    let mut seen_files: usize = 0;
    glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        // Drain the progress channel first: the last walked file
        // becomes the live scan location (the Tasks line + the
        // CR_TRACE evidence). Bind the recv result before matching —
        // a `while let` scrutinee borrow lives through the loop body.
        loop {
            let progress = prx.try_recv();
            match progress {
                Ok(path) => {
                    seen_files += 1;
                    SCAN_LOCATION.with(|cell| *cell.borrow_mut() = path);
                }
                Err(std::sync::mpsc::TryRecvError::Empty)
                | Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
            }
        }
        if seen_files > 0 {
            let current = SCAN_LOCATION.with(|cell| cell.borrow().clone());
            crate::trace::trace(format!(
                "scan progress: {seen_files} files, current '{current}'"
            ));
        }
        // Drain the worker messages: batches append to the live database;
        // ONE view refresh per tick after the whole drain (a per-batch
        // refresh is O(N) per batch — O(N²) per tick starved the main
        // loop at 10k books, measured). Done merges the final storage.
        let mut tick_batch: Vec<ComicBook> = Vec::new();
        loop {
            let received = rx.try_recv();
            match received {
                Ok(ScanWorkerMsg::Batch(batch)) => {
                    tick_batch.extend(batch);
                }
                Ok(ScanWorkerMsg::Done(books, result)) => {
                    // The run summary accumulates across the queued
                    // scans; the shell reports it once at the end.
                    record_scan_problems(&result);
                    // Late batches ride the final storage — the landing
                    // merge replaces the appends wholesale.
                    tick_batch.clear();
                    {
                        let mut lib = library.borrow_mut();
                        let (removed, touched) = take_scan_side_effects();
                        let merged =
                            merge_scan_storage(books, &lib.database().books, &removed, &touched);
                        lib.database_mut().books = merged;
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
                    SCAN_STOP.with(|cell| *cell.borrow_mut() = None);
                    SCAN_SKIP.with(|cell| *cell.borrow_mut() = None);
                    fire_scan_view_hook(&[]);
                    // The done callback refreshes against the MERGED
                    // database — it must run BEFORE the next queued
                    // scan takes the storage (a refresh against the
                    // taken, empty book list wipes the view; measured:
                    // "evaluate 0 books" right after "evaluate 10002").
                    if let Some(d) = done.take() {
                        d(*result);
                    }
                    // The queued requests run one at a time. KNOWN DEFECT: `pop`
                    // takes the LAST request, so the queue runs in reverse
                    // arrival order despite the doc comments — the
                    // out-of-scope LIFO fix (phase 14 keeps each new
                    // command at ONE request, so it does not hit this).
                    let next = SCAN_QUEUE.with(|queue| queue.borrow_mut().pop());
                    if let Some(next) = next {
                        start_scan_worker(next);
                    }
                    return ControlFlow::Break;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    SCAN_IN_FLIGHT.with(|cell| *cell.borrow_mut() = false);
                    SCAN_LOCATION.with(|cell| cell.borrow_mut().clear());
                    SCAN_STOP.with(|cell| *cell.borrow_mut() = None);
                    SCAN_SKIP.with(|cell| *cell.borrow_mut() = None);
                    if let Some(d) = done.take() {
                        d(ScanResult::default());
                    }
                    return ControlFlow::Break;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
            }
        }
        if !tick_batch.is_empty() {
            {
                let mut lib = library.borrow_mut();
                lib.database_mut().books.extend(tick_batch.iter().cloned());
            }
            // The session borrow is dropped — the hook may evaluate
            // the library.
            fire_scan_view_hook(&tick_batch);
        }
        ControlFlow::Continue
    });
}

/// Pops the debounced watch roots that need a rescan (the watch poll
/// timer calls this every second). While a scan is in flight the
/// events stay pending — a rescan queued per second behind a running
/// scan stacks a full re-scan storm; the roots deliver once the scan
/// ends.
pub fn take_watch_folder_rescans() -> Vec<String> {
    if is_scanning() {
        return Vec::new();
    }
    session().borrow_mut().take_watch_folder_rescans()
}

/// The collapsed Windows-path roots (the migration dialog rows; the
/// Phase 8 T11 helper).
pub fn windows_path_roots() -> Vec<cr_engine::path_migration::PathRoot> {
    session().borrow().windows_path_roots()
}

/// Any Windows-style path left in the database? (The `win.migrate-paths`
/// enable state.)
pub fn has_windows_paths() -> bool {
    session().borrow().has_windows_paths()
}

/// Applies the root → target mappings (books / watch folders /
/// blacklist rewrite, dirty mark, watcher rebuild). The view refresh
/// is the caller's job.
pub fn apply_path_migration(
    mappings: &[cr_engine::path_migration::Mapping],
) -> cr_engine::path_migration::ApplyReport {
    let t = std::time::Instant::now();
    let report = session().borrow_mut().apply_path_migration(mappings);
    crate::trace::trace(format!(
        "path-migration apply: {} books, {} fileless, {}ms",
        report.books_mapped + report.books_fileless,
        report.books_fileless,
        t.elapsed().as_millis()
    ));
    report
}

/// `DatabaseManager.Save` (the exit path): stops an in-flight scan
/// first — the C# `QueueManager.Dispose` → `Scanner.Dispose` →
/// `Stop(clearQueue: true)` (ComicScanner.cs:228) — then waits for
/// the partial merge (one pump tick) and saves unconditionally.
/// Waiting for a RUNNING scan would hang the exit (the user report);
/// the stop makes the save a consistent full set instead of a
/// mid-scan partial.
pub fn save() -> Result<(), cr_core::database::DbError> {
    if scan_in_flight() {
        abort_scan();
    }
    while scan_in_flight() {
        // The merge happens in the scan pump (a main-loop source) —
        // drive the loop until it runs.
        glib::MainContext::default().iteration(true);
    }
    session().borrow_mut().save()
}

/// `DatabaseManager.SaveInBackground`: saves only when dirty. Runs
/// MID-SCAN too: since ADR-032 the worker scans a CLONE and the
/// main-thread database stays live (the batches appended so far) —
/// the C# background-saves its live collection while the scanner
/// walks. Returns whether a save ran.
pub fn save_if_dirty() -> Result<bool, cr_core::database::DbError> {
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

/// `tvQueries_AfterExpand` / `tvQueries_AfterCollapse` port: sets a
/// navigator folder's persisted `Collapsed` flag
/// (`ComicListItemFolder.Collapsed`, the flag `FillListTree` reads back
/// at the next start). The database goes dirty only on a real change.
/// An id that is not a folder (the Library root, a list) is inert.
pub fn set_folder_collapsed(id: &CrGuid, collapsed: bool) {
    use cr_core::database::list_items::ComicListItem;
    /// True when this subtree changed.
    fn walk(items: &mut [ComicListItem], id: &CrGuid, collapsed: bool) -> bool {
        for item in items.iter_mut() {
            let ComicListItem::Folder(folder) = item else {
                continue;
            };
            if folder.base.id == *id {
                if folder.collapsed == collapsed {
                    return false;
                }
                folder.collapsed = collapsed;
                return true;
            }
            if walk(&mut folder.items, id, collapsed) {
                return true;
            }
        }
        false
    }
    let lib = session();
    let mut l = lib.borrow_mut();
    if walk(&mut l.database_mut().comic_lists, id, collapsed) {
        l.mark_dirty();
    }
}

/// Is this list id the Library root (the all-books list)? The scan
/// batches take the incremental-append path only for it.
pub fn is_library_list(id: &CrGuid) -> bool {
    fn walk(items: &[cr_core::database::list_items::ComicListItem], id: &CrGuid) -> bool {
        items.iter().any(|i| {
            if i.base().id == *id {
                return matches!(i, cr_core::database::list_items::ComicListItem::Library(_));
            }
            match i {
                cr_core::database::list_items::ComicListItem::Folder(folder) => {
                    walk(&folder.items, id)
                }
                _ => false,
            }
        })
    }
    let lib = session();
    let l = lib.borrow();
    walk(&l.database().comic_lists, id)
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
    record_scan_touch(&id);
    l.mark_dirty();
    drop(l);
    schedule_book_file_update(&id);
    true
}

/// `Program.Database.Add` (the `MainForm.AddNewBook` insert path):
/// pushes the new book into the books table and marks the database
/// dirty. Returns false when a book with the id already exists — the
/// book editor commits fire per save point (Apply/OK), so the INSERT
/// must run once and later commits apply instead.
pub fn insert_new_book(book: &ComicBook) -> bool {
    let lib = session();
    let mut l = lib.borrow_mut();
    if l.database().books.iter().any(|b| b.id == book.id) {
        return false;
    }
    l.database_mut().books.push(book.clone());
    l.mark_dirty();
    true
}

// ---------- The file write-back (the C# `WriteComicBookInfoFileQueue`) ----------

/// One completion callback (NOT Send — it stays on the main thread;
/// the worker only ships the result over the channel and the pump
/// runs it here).
type WriteCallback = Box<dyn FnOnce(Result<bool, String>)>;

thread_local! {
    /// The debounced write timers (one per book id; the C# keeps a
    /// 100 ms `Timer` per book so batched property edits write once).
    static WRITE_TIMERS: RefCell<std::collections::HashMap<CrGuid, glib::SourceId>> =
        RefCell::new(std::collections::HashMap::new());
    /// The per-book completion callbacks.
    static WRITE_CALLBACKS: RefCell<std::collections::HashMap<CrGuid, Vec<WriteCallback>>> =
        RefCell::new(std::collections::HashMap::new());
}

/// One queued write: the book CLONE at enqueue time (the worker never
/// touches the session) plus the settings snapshot the writer needs.
struct WriteJob {
    book: ComicBook,
    update_book_files: bool,
}

/// The pending-write queue (the C# `ProcessingQueue` shape: the
/// workers claim inside the lock, a re-request for a pending book
/// REPLACES the queued clone — latest data wins).
static WRITE_QUEUE: std::sync::Mutex<std::collections::VecDeque<WriteJob>> =
    std::sync::Mutex::new(std::collections::VecDeque::new());
static WRITE_CV: std::sync::Condvar = std::sync::Condvar::new();
static WRITE_TX: std::sync::OnceLock<std::sync::mpsc::Sender<(CrGuid, WriteResult)>> =
    std::sync::OnceLock::new();

/// What the worker ships back: the outcome, the post-write book
/// (refreshed file properties, dirty flag cleared), and the info
/// snapshot taken BEFORE the write's file-properties refresh — the
/// pump's re-edit guard compares the slot against THIS (the
/// post-refresh info always differs from the slot's stored times).
struct WriteOutcome {
    written: bool,
    book: ComicBook,
    pristine_info: cr_core::model::comic_info::ComicInfo,
}
type WriteResult = Result<WriteOutcome, String>;

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
            update_book_file_async(&id, always_write, None);
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
/// The writes funnel through the Info Writer worker (the C#
/// `WriteComicBookInfoFileQueue`).
pub fn update_all_book_files() {
    let dirty: Vec<CrGuid> = {
        let lib = session();
        let l = lib.borrow();
        l.database()
            .books
            .iter()
            .filter(|b| b.comic_info_is_dirty)
            .map(|b| b.id)
            .collect()
    };
    for id in dirty {
        update_book_file_async(&id, true, None);
    }
}

/// The enqueue side of [`update_book_file_async`]: the settings
/// gates + the book clone (MAIN thread — the session thread-locals
/// live here), then the job joins the worker queue.
fn enqueue_book_write(id: &CrGuid, always_write: bool, on_done: Option<WriteCallback>) {
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
        if let Some(done) = on_done {
            done(Ok(false));
        }
        return;
    }

    let book = {
        let lib = session();
        let mut l = lib.borrow_mut();
        let Some(book) = l.database_mut().books.iter_mut().find(|b| b.id == *id) else {
            if let Some(done) = on_done {
                done(Ok(false));
            }
            return;
        };
        // Only a dirty book writes (`ComicInfoIsDirty || ...`).
        if !book.comic_info_is_dirty {
            if let Some(done) = on_done {
                done(Ok(false));
            }
            return;
        }
        book.clone()
    };

    if let Some(done) = on_done {
        WRITE_CALLBACKS.with(|c| {
            c.borrow_mut().entry(*id).or_default().push(done);
        });
    }
    ensure_write_worker();
    let mut queue = WRITE_QUEUE.lock().unwrap();
    // The dedup: a re-request for a PENDING book replaces the queued
    // clone (the latest data wins; the callbacks stay).
    if let Some(job) = queue.iter_mut().find(|j| j.book.id == *id) {
        job.book = book;
        job.update_book_files = update_book_files;
        return;
    }
    queue.push_back(WriteJob {
        book,
        update_book_files,
    });
    drop(queue);
    WRITE_CV.notify_one();
}

/// `QueueManager.AddBookToFileUpdate` + `WriteInfoToFileWithCacheUpdate`
/// for one library book, on the INFO WRITER worker: the settings gates
/// run here on the main thread, the metadata write into the file
/// (ComicInfo.xml, plus ComicBook.xml when `UpdateComicBookFiles` is
/// on), the file-properties refresh, and the dirty-flag clear run on
/// the worker (the C# write queue — a synchronous run froze the UI on
/// CB7/CBR books: a full archive rewrite, possibly a `7z`/`rar`
/// subprocess). `on_done` runs on the MAIN thread when the write
/// lands: `Ok(true)` written, `Ok(false)` gated/no-op, `Err` failed.
///
/// The pump applies the refreshed book into the library slot only
/// when the slot was NOT re-edited since the enqueue (the info still
/// matches the clone); a re-edited book keeps its dirty flag and the
/// editor's re-schedule writes it again.
pub fn update_book_file_async(id: &CrGuid, always_write: bool, on_done: Option<WriteCallback>) {
    enqueue_book_write(id, always_write, on_done);
}

/// The pure (Send, session-free) write: provider open + the scoped
/// metadata store + the file-properties refresh + the dirty-flag
/// clear. Returns the written flag, the post-write book, and the
/// info snapshot taken BEFORE the refresh (the pump's re-edit
/// baseline). Runs on the worker; also the unit-test seam.
pub fn run_book_file_write(
    book: &mut ComicBook,
    update_book_files: bool,
) -> Result<(bool, ComicBook, cr_core::model::comic_info::ComicInfo), String> {
    if book.file_path.is_empty() || !Path::new(&book.file_path).exists() {
        return Err(format!("file not found: {}", book.file_path));
    }
    let provider =
        cr_io::ComicProvider::open(Path::new(&book.file_path)).map_err(|e| e.to_string())?;
    let written = cr_io::write::store_info_scoped(&provider, book, update_book_files)
        .map_err(|e| e.to_string())?;
    if written {
        // The pristine snapshot BEFORE `RefreshFileProperties` (the
        // refresh rewrites the file times/size).
        let pristine_info = book.info.clone();
        refresh_file_info(book);
        book.comic_info_is_dirty = false;
        Ok((written, book.clone(), pristine_info))
    } else {
        Ok((false, book.clone(), book.info.clone()))
    }
}

/// Spawns the Info Writer thread + the result pump once (the first
/// enqueue starts them; both live for the process).
fn ensure_write_worker() {
    static STARTED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    if STARTED.set(()).is_err() {
        return;
    }
    let (tx, rx) = std::sync::mpsc::channel::<(CrGuid, WriteResult)>();
    let _ = WRITE_TX.set(tx);
    std::thread::Builder::new()
        .name("Info Writer".into())
        .spawn(move || loop {
            let job = {
                let mut queue = WRITE_QUEUE.lock().unwrap();
                loop {
                    if let Some(job) = queue.pop_front() {
                        break job;
                    }
                    queue = WRITE_CV.wait(queue).unwrap();
                }
            };
            let mut book = job.book;
            let id = book.id;
            let result = match run_book_file_write(&mut book, job.update_book_files) {
                Ok((written, book, pristine_info)) => Ok(WriteOutcome {
                    written,
                    book,
                    pristine_info,
                }),
                Err(e) => Err(e),
            };
            if let Some(tx) = WRITE_TX.get() {
                let _ = tx.send((id, result));
            }
        })
        .expect("spawn Info Writer");

    // The main-thread pump: apply the result into the library slot
    // (with the re-edit guard), run the per-book callbacks.
    let library = session();
    glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        loop {
            // Bind the recv result before matching — a `while let`
            // scrutinee borrow lives through the loop body.
            let received = rx.try_recv();
            let Ok((id, result)) = received else {
                break;
            };
            if let Ok(outcome) = &result {
                if outcome.written {
                    let mut l = library.borrow_mut();
                    if let Some(slot) = l.database_mut().books.iter_mut().find(|b| b.id == id) {
                        // The re-edit guard: the slot's info changed
                        // since the enqueue clone → the write is
                        // stale; keep the dirty flag (the editor's
                        // re-schedule writes it again). The
                        // comparison runs against the PRISTINE
                        // snapshot (the post-refresh times always
                        // differ from the stored ones).
                        let stale = slot.info != outcome.pristine_info;
                        slot.file_modified_time = outcome.book.file_modified_time;
                        slot.file_creation_time = outcome.book.file_creation_time;
                        slot.file_size = outcome.book.file_size;
                        slot.file_is_missing = outcome.book.file_is_missing;
                        if !stale {
                            // Full refresh carry (the newly learned
                            // page count included) — the old inline
                            // path replaced the whole slot.
                            slot.info = outcome.book.info.clone();
                            slot.comic_info_is_dirty = false;
                        }
                        record_scan_touch(&id);
                        l.mark_dirty();
                    }
                }
            }
            // The callback result carries only the written flag / the
            // error (the caller never needs the book).
            let cb_result = result.as_ref().map(|o| o.written).map_err(|e| e.clone());
            let callbacks = WRITE_CALLBACKS.with(|c| c.borrow_mut().remove(&id));
            if let Some(cbs) = callbacks {
                for cb in cbs {
                    cb(cb_result.clone());
                }
            }
        }
        glib::ControlFlow::Continue
    });
}

/// `ShellFile.DeleteFile` parity — the recycle bin via `gio trash`
/// (ADR-006). The Phase 7 incident guards: an EMPTY path or a
/// non-file never reaches the trash (gio resolves "" to the
/// process's CWD).
fn trash_path(path: &str) -> bool {
    if path.is_empty() || !Path::new(path).is_file() {
        return false;
    }
    std::process::Command::new("gio")
        .args(["trash", path])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// The `QueueManager.ExportComic` post-export block
/// (QueueManager.cs:455-508) for ONE export group: `group[0]` is the
/// group's key book (`kcb`), the rest are combine sources, and
/// `out_path` is the single output file. Replace-source re-points the
/// book at the output and trashes the leftover sources;
/// delete-original trashes the sources; add-to-library creates a book
/// for the output. The dirty flags clear only when the book's own
/// file was replaced (the export embedded the info).
pub fn export_post_process(
    setting: &cr_io::export::ExportSetting,
    group: &[ComicBook],
    out_path: &Path,
) -> Result<(), String> {
    export_post_process_with(setting, group, out_path, &trash_path)
}

/// The surgery with the trash step injected (the gio call is
/// environment-dependent — tmpfs and USB sticks refuse to trash; the
/// failure then only skips that source's removal and is reported).
pub fn export_post_process_with(
    setting: &cr_io::export::ExportSetting,
    group: &[ComicBook],
    out_path: &Path,
    trash: &dyn Fn(&str) -> bool,
) -> Result<(), String> {
    use cr_io::export::{ExportImageProcessingSource, ExportTarget};

    let Some(kcb) = group.first() else {
        return Ok(());
    };
    let out_str = out_path.to_string_lossy().into_owned();
    let is_local = !kcb.file_path.is_empty();
    let replace_source = setting.target == ExportTarget::ReplaceSource;
    let is_local_and_replace = is_local && replace_source;
    let was_replaced = is_local_and_replace || kcb.file_path == out_str;
    // Captured from the pre-export book in the C# (`clearDirtyInfoFlag`).
    let clear_dirty_info = setting.embed_comic_info && kcb.comic_info_is_dirty;
    let clear_dirty_book = setting.embed_comic_book && kcb.comic_book_is_dirty;
    let exported = cr_io::export::build_export_info(setting, kcb);
    // The local group members, minus the output itself (the C#
    // filters `p != outPath` before both branches).
    let sources: Vec<String> = group
        .iter()
        .filter(|b| !b.file_path.is_empty() && b.file_path != out_str)
        .map(|b| b.file_path.clone())
        .collect();

    let mut errors: Vec<String> = Vec::new();
    let lib = session();
    let mut l = lib.borrow_mut();

    // The C# `Database.Books.Remove(item)` — every book pointing at
    // the removed file leaves the database. The removed ids are
    // recorded while a scan runs (the landing merge must not
    // resurrect them from the worker's storage copy).
    let remove_by_path = |l: &mut Library, file: &str| {
        let removed: Vec<CrGuid> = l
            .database()
            .books
            .iter()
            .filter(|b| b.file_path == file)
            .map(|b| b.id)
            .collect();
        l.database_mut().books.retain(|b| b.file_path != file);
        for id in &removed {
            record_scan_removal(id);
        }
    };
    // Trash + drop; a failed trash keeps the book (data-safe — the
    // C# `ShellFile.DeleteFile` throw skips the removal too).
    let remove_source = |l: &mut Library, file: &str, errors: &mut Vec<String>| {
        if trash(file) {
            remove_by_path(l, file);
        } else {
            errors.push(format!("could not trash the original file: {file}"));
        }
    };

    if is_local_and_replace {
        let mut book = kcb.clone();
        book.file_path = out_str.clone();
        refresh_file_info_basic(&mut book);
        book.set_info(&exported, false, true);
        if setting.image_processing_source == ExportImageProcessingSource::FromComic {
            book.color_adjustment = cr_core::model::bitmap_adjustment::BitmapAdjustment::default();
        }
        if clear_dirty_info {
            book.comic_info_is_dirty = false;
        }
        if clear_dirty_book {
            book.comic_book_is_dirty = false;
        }
        // The C# re-points `kcb.FilePath` (QueueManager.cs:471)
        // BEFORE the source-delete loop, so the by-path removal
        // cannot hit the key book — the write-back goes first here
        // for the same reason.
        if let Some(slot) = l.database_mut().books.iter_mut().find(|b| b.id == kcb.id) {
            *slot = book;
            record_scan_touch(&kcb.id);
            l.mark_dirty();
        }
        for source in &sources {
            remove_source(&mut l, source, &mut errors);
        }
    } else {
        if setting.delete_original && is_local {
            for source in &sources {
                remove_source(&mut l, source, &mut errors);
            }
        }
        if setting.add_to_library || replace_source {
            let mut book = ComicBook {
                file_path: out_str.clone(),
                added_time: CrDateTime::now(),
                ..ComicBook::default()
            };
            book.set_info(&exported, false, true);
            l.database_mut().books.push(book);
            l.mark_dirty();
        }
        if was_replaced {
            // The same-path overwrite case: the export embedded the
            // info, so the dirty flags clear on the kept book.
            if let Some(slot) = l.database_mut().books.iter_mut().find(|b| b.id == kcb.id) {
                let mut changed = false;
                if clear_dirty_info && slot.comic_info_is_dirty {
                    slot.comic_info_is_dirty = false;
                    changed = true;
                }
                if clear_dirty_book && slot.comic_book_is_dirty {
                    slot.comic_book_is_dirty = false;
                    changed = true;
                }
                if changed {
                    record_scan_touch(&kcb.id);
                    l.mark_dirty();
                }
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Removes one book from the library by id (the context-menu
/// command; the file on disk is untouched).
pub fn remove_book(id: &CrGuid) {
    let lib = session();
    let mut l = lib.borrow_mut();
    let before = l.database().books.len();
    l.database_mut().books.retain(|b| b.id != *id);
    if l.database().books.len() != before {
        record_scan_removal(id);
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

/// The `ImportList(fc, file)` landing for a target selection: a
/// folder takes the item as its last child, an item appends to ITS
/// parent container, no target appends at the top level (the C#
/// `GetNodeComicListCollection` + `Collection.Add` shape; an unknown
/// target falls back to the top level). Returns the item's id.
pub fn import_list_item(
    target: Option<&CrGuid>,
    item: cr_core::database::list_items::ComicListItem,
) -> CrGuid {
    let id = item.base().id;
    let lib = session();
    let mut l = lib.borrow_mut();
    let lists = &mut l.database_mut().comic_lists;
    match target {
        Some(id) => {
            if let Some(folder) = find_folder_mut(lists, id) {
                folder.items.push(item);
            } else if let Some(container) = find_container(lists, id) {
                container.push(item);
            } else {
                lists.push(item);
            }
        }
        None => lists.push(item),
    }
    l.mark_dirty();
    id
}

/// The `ImportList(file)` landing without a target: the
/// `Library.TemporaryFolder.Items` (find-or-create, appended last).
/// Returns the id of the inserted item.
pub fn import_temporary_item(item: cr_core::database::list_items::ComicListItem) -> CrGuid {
    let id = item.base().id;
    let lib = session();
    let mut l = lib.borrow_mut();
    let temp = l.database_mut().temporary_folder();
    temp.push(item);
    l.mark_dirty();
    id
}

/// The `Library.Books.AddRange(newBooks)` parity: appends the books
/// (the imported missing placeholders) and marks the database dirty.
pub fn add_books(books: Vec<cr_core::model::comic_book::ComicBook>) {
    if books.is_empty() {
        return;
    }
    let lib = session();
    let mut l = lib.borrow_mut();
    l.database_mut().books.extend(books);
    l.mark_dirty();
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
    /// unified-config block joins when the settings schema grows the
    /// export lists).
    static LAST_EXPORT: RefCell<Option<cr_io::export::ExportSetting>> = const { RefCell::new(None) };
}

pub fn remember_export_setting(setting: cr_io::export::ExportSetting) {
    LAST_EXPORT.with(|c| *c.borrow_mut() = Some(setting));
}

pub fn last_export_setting() -> Option<cr_io::export::ExportSetting> {
    LAST_EXPORT.with(|c| c.borrow().clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cr_core::model::comic_book::ComicBook;

    fn book(path: &str) -> ComicBook {
        ComicBook {
            file_path: path.to_string(),
            id: CrGuid::new_random(),
            ..ComicBook::default()
        }
    }

    fn ids(books: &[ComicBook]) -> Vec<String> {
        let mut v: Vec<String> = books.iter().map(|b| b.file_path.clone()).collect();
        v.sort();
        v
    }

    #[test]
    fn merge_without_side_effects_takes_the_worker_storage() {
        let a = book("a.cbz");
        let b = book("b.cbz");
        let worker = vec![a.clone(), b.clone()];
        let db = vec![a, b];
        let merged = merge_scan_storage(worker, &db, &HashSet::new(), &HashSet::new());
        assert_eq!(ids(&merged), vec!["a.cbz", "b.cbz"]);
    }

    #[test]
    fn merge_keeps_books_added_mid_scan() {
        // The worker's clone predates the mid-scan add: the database
        // book survives.
        let a = book("a.cbz");
        let worker = vec![a.clone()];
        let db = vec![a, book("added-mid-scan.cbz")];
        let merged = merge_scan_storage(worker, &db, &HashSet::new(), &HashSet::new());
        assert_eq!(ids(&merged), vec!["a.cbz", "added-mid-scan.cbz"]);
    }

    #[test]
    fn merge_keeps_the_database_copy_for_touched_books() {
        // Edited on the main thread while the scan ran: the database
        // copy (the edit) wins over the worker's pre-scan copy; the
        // untouched book keeps the worker's scanned copy.
        let a = book("a.cbz");
        let b = book("b.cbz");
        let mut edited = a.clone();
        edited.info.title = "Edited Mid-Scan".into();
        let touched: HashSet<CrGuid> = [a.id].into();
        let worker = vec![a, b.clone()];
        let db = vec![edited, b];
        let merged = merge_scan_storage(worker, &db, &HashSet::new(), &touched);
        assert_eq!(ids(&merged), vec!["a.cbz", "b.cbz"]);
        assert_eq!(merged[0].info.title, "Edited Mid-Scan");
    }

    #[test]
    fn merge_drops_removed_books_everywhere() {
        // Removed on the main thread mid-scan: the database copy is
        // gone already and the worker's copy must not resurrect it.
        let a = book("a.cbz");
        let removed_book = book("removed-mid-scan.cbz");
        let removed: HashSet<CrGuid> = [removed_book.id].into();
        let worker = vec![a.clone(), removed_book];
        let db = vec![a];
        let merged = merge_scan_storage(worker, &db, &removed, &HashSet::new());
        assert_eq!(ids(&merged), vec!["a.cbz"]);
    }

    #[test]
    fn merge_pump_added_books_do_not_duplicate() {
        // The pump appended the new book to the database mid-scan AND
        // the worker pushed it into its storage: one copy survives.
        let a = book("a.cbz");
        let pumped = book("pumped.cbz");
        let worker = vec![a.clone(), pumped.clone()];
        let db = vec![a, pumped];
        let merged = merge_scan_storage(worker, &db, &HashSet::new(), &HashSet::new());
        assert_eq!(ids(&merged), vec!["a.cbz", "pumped.cbz"]);
    }

    #[test]
    fn merge_touched_pump_added_book_keeps_the_database_copy() {
        // Added by the pump mid-scan, then edited on the main thread:
        // the database copy (the edit) wins over the worker's.
        let a = book("a.cbz");
        let pumped = book("pumped.cbz");
        let mut edited = pumped.clone();
        edited.info.title = "Edited After Pump".into();
        let touched: HashSet<CrGuid> = [pumped.id].into();
        let worker = vec![a.clone(), pumped];
        let db = vec![a, edited];
        let merged = merge_scan_storage(worker, &db, &HashSet::new(), &touched);
        assert_eq!(merged[1].info.title, "Edited After Pump");
    }

    #[test]
    fn distinct_paths_drop_empties_and_duplicates() {
        // The explicit-scan target set: a fileless book (empty path)
        // has no file, and a file listed twice must be read once.
        let paths = distinct_paths([
            "a.cbz".to_string(),
            String::new(),
            "a.cbz".to_string(),
            "b.cbz".to_string(),
        ]);
        assert_eq!(paths, vec!["a.cbz".to_string(), "b.cbz".to_string()]);
    }
}
