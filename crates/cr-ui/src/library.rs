//! The library session — the cr-ui wiring of
//! `cr_engine::library::Library` (the C# `Program.DatabaseManager`
//! static): open at startup, save on exit, the 600 s background save,
//! the reader/book integration (the C# `ComicBookFactory`), and the
//! scan worker (the C# "Book Scanner" low-priority thread).

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
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
    target: ScanTarget,
    done: Box<dyn FnOnce(ScanResult)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanTarget {
    Library,
    Incoming,
}

/// The shell's per-batch view refresh (a Weak capture lives inside).
type ScanViewHook = Box<dyn Fn(&[ComicBook])>;

thread_local! {
    static SESSION: RefCell<Option<Rc<RefCell<Library>>>> = const { RefCell::new(None) };
    static INCOMING_SESSION: RefCell<Option<Rc<RefCell<cr_engine::incoming::IncomingCatalog>>>> =
        const { RefCell::new(None) };
    static INCOMING_LISTS_SESSION: RefCell<Option<Rc<RefCell<cr_engine::incoming::IncomingLists>>>> =
        const { RefCell::new(None) };
    static INCOMING_LIST_SAVE_QUEUE: RefCell<VecDeque<IncomingListSave>> =
        const { RefCell::new(VecDeque::new()) };
    static INCOMING_LIST_SAVE_ACTIVE: Cell<bool> = const { Cell::new(false) };
    static INCOMING_EXTERNAL_GAPS: RefCell<cr_engine::incoming::ExternalGapCache> =
        RefCell::new(cr_engine::incoming::ExternalGapCache::new());
    static INCOMING_CLASSIFICATION: RefCell<Option<IncomingClassificationSnapshot>> =
        const { RefCell::new(None) };
    static INCOMING_CLASSIFICATION_GENERATION: Cell<u64> = const { Cell::new(0) };
    static INCOMING_CLASSIFICATION_ACTIVE: Cell<bool> = const { Cell::new(false) };
    static INCOMING_GAP_GENERATION: Cell<u64> = const { Cell::new(0) };
    static INCOMING_GAP_REFRESH_ACTIVE: Cell<bool> = const { Cell::new(false) };
    static INCOMING_GAP_VIEW_HOOK: RefCell<Option<Box<dyn Fn()>>> = const { RefCell::new(None) };
    /// The last-computed Missing Issues gap rows (Phase 19): a manual
    /// report, never auto-recomputed. Only the main thread writes this.
    static MISSING_ISSUES_ROWS: RefCell<Vec<ComicBook>> = const { RefCell::new(Vec::new()) };
    static MISSING_ISSUES_GENERATION: Cell<u64> = const { Cell::new(0) };
    static MISSING_ISSUES_ACTIVE: Cell<bool> = const { Cell::new(false) };
    /// Unix seconds of the last completed Missing Issues pass, for the
    /// "last computed" status text.
    static MISSING_ISSUES_LAST_RUN: Cell<Option<i64>> = const { Cell::new(None) };
    /// The wall-clock time the last Missing Issues pass took (T2's
    /// measurement, shown live; recorded once in
    /// `docs/current-status.md` per the phase's named unknown).
    static MISSING_ISSUES_LAST_ELAPSED: Cell<Option<std::time::Duration>> =
        const { Cell::new(None) };
    /// The Comic Vine cache job that runs now (ADR-037, ADR-038): the
    /// MCL import, the incremental sweep, or the warm task. One at a
    /// time, because the sweep and the warm task share one
    /// `sweep_state` row and one request budget.
    ///
    /// A WORKER THREAD MUST NOT WRITE THIS. A thread-local written
    /// from a worker writes that worker's own copy. The worker sends
    /// its progress over an mpsc channel and the main-thread pump
    /// writes here, which is the same shape the scan uses.
    static CV_JOB: RefCell<Option<CvJob>> = const { RefCell::new(None) };
    /// The in-flight cache job's abort flag. `abort_cv_job` sets it;
    /// the worker reads it between pages and between volumes.
    static CV_JOB_CANCEL: RefCell<Option<std::sync::Arc<std::sync::atomic::AtomicBool>>> =
        const { RefCell::new(None) };
    /// One scan at a time (the C# scan queue): requests arriving
    /// while the worker runs wait here, in arrival order.
    static SCAN_QUEUE: RefCell<Vec<QueuedScan>> = const { RefCell::new(Vec::new()) };
    static SCAN_IN_FLIGHT: RefCell<bool> = const { RefCell::new(false) };
    static SCAN_TARGET: Cell<ScanTarget> = const { Cell::new(ScanTarget::Library) };
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
    static SCAN_COMPLETION_ERROR: RefCell<Option<(ScanTarget, String)>> = const { RefCell::new(None) };
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

thread_local! {
    static BACKGROUND_SAVE_IN_FLIGHT: RefCell<bool> = const { RefCell::new(false) };
}

#[derive(Clone)]
pub struct IncomingClassificationSnapshot {
    pub books: Vec<ComicBook>,
    pub classifications: Vec<cr_engine::incoming::IncomingClassification>,
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
    if is_library_scanning() {
        SCAN_REMOVED_IDS.with(|s| s.borrow_mut().insert(*id));
    }
}

/// Records a book id the main thread mutated while a scan is in
/// flight (the landing merge keeps the database copy for it).
fn record_scan_touch(id: &CrGuid) {
    if is_library_scanning() {
        SCAN_TOUCHED_IDS.with(|s| s.borrow_mut().insert(*id));
    }
}

fn is_library_scanning() -> bool {
    is_scanning() && SCAN_TARGET.with(|cell| cell.get() == ScanTarget::Library)
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
/// Which Comic Vine cache job runs (ADR-037, ADR-038).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CvJobKind {
    Import,
    Sweep,
    Warm,
    IncomingRefresh,
    MissingIssuesGap,
    LinkSeriesSearch,
    SeriesMetadataPropagation,
    CacheManager,
}

impl CvJobKind {
    /// The heading the status bar and the Tasks window show.
    pub fn label(self) -> &'static str {
        match self {
            CvJobKind::Import => "Importing an MCL file",
            CvJobKind::Sweep => "Updating the Comic Vine cache",
            CvJobKind::Warm => "Warming the Comic Vine cache",
            CvJobKind::IncomingRefresh => "Refreshing Incoming from Comic Vine",
            CvJobKind::MissingIssuesGap => "Computing the Missing Issues report",
            CvJobKind::LinkSeriesSearch => "Searching Comic Vine for a series",
            CvJobKind::SeriesMetadataPropagation => "Applying cached series metadata",
            CvJobKind::CacheManager => "Updating one Comic Vine series",
        }
    }
}

/// The in-flight Comic Vine cache job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CvJob {
    pub kind: CvJobKind,
    /// The line the Tasks window shows and the lamp tooltip carries.
    pub detail: String,
    /// The work done and the work expected. `total` of zero means the
    /// job cannot say how much is left.
    pub done: i64,
    pub total: i64,
}

impl CvJob {
    /// "Updating the Comic Vine cache — page 12 of 53".
    pub fn text(&self) -> String {
        if self.detail.is_empty() {
            self.kind.label().to_string()
        } else {
            format!("{} \u{2014} {}", self.kind.label(), self.detail)
        }
    }
}

/// Claims the single Comic Vine cache job slot.
///
/// Returns `false` when a job already runs. Two sweeps would race on
/// the one `sweep_state` row, and two jobs would each believe they own
/// the request budget.
pub fn start_cv_job(
    kind: CvJobKind,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> bool {
    if cv_job().is_some() {
        return false;
    }
    CV_JOB.with(|cell| {
        *cell.borrow_mut() = Some(CvJob {
            kind,
            detail: String::new(),
            done: 0,
            total: 0,
        });
    });
    CV_JOB_CANCEL.with(|cell| *cell.borrow_mut() = Some(cancel));
    crate::trace::trace(format!("cv job started: {}", kind.label()));
    true
}

/// Updates the in-flight job. A call after the job ended is dropped.
pub fn set_cv_job_progress(detail: String, done: i64, total: i64) {
    CV_JOB.with(|cell| {
        if let Some(job) = cell.borrow_mut().as_mut() {
            job.detail = detail;
            job.done = done;
            job.total = total;
        }
    });
}

/// Releases the job slot.
pub fn end_cv_job() {
    CV_JOB.with(|cell| *cell.borrow_mut() = None);
    CV_JOB_CANCEL.with(|cell| *cell.borrow_mut() = None);
}

/// The in-flight Comic Vine cache job, or `None`.
pub fn cv_job() -> Option<CvJob> {
    CV_JOB.with(|cell| cell.borrow().clone())
}

/// True while a Comic Vine cache job runs (the status-bar lamp).
pub fn cv_job_active() -> bool {
    CV_JOB.with(|cell| cell.borrow().is_some())
}

/// Asks the in-flight Comic Vine cache job to stop. The work already
/// written to the cache stays, and a sweep keeps the offset it
/// reached, so the next run resumes there.
pub fn abort_cv_job() {
    let requested = CV_JOB_CANCEL.with(|cell| {
        if let Some(flag) = cell.borrow().as_ref() {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
            true
        } else {
            false
        }
    });
    crate::trace::trace(format!("cv job abort requested (in flight: {requested})"));
}

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

/// Takes a worker failure that prevented a scan from landing.
pub fn take_scan_completion_error() -> Option<(ScanTarget, String)> {
    SCAN_COMPLETION_ERROR.with(|cell| cell.borrow_mut().take())
}

/// Opens the library database at the default location (`Program`'s
/// startup `DatabaseManager.Open`). Returns the `OpenMessage` the C#
/// would show in the attention dialog (None for a plain load).
///
/// Also loads the settings layer: the ONE unified config file
/// `comicrust.toml` (the C# `Settings.Load` + the `IniFile` chain —
/// ADR-033) plus the argv switches for `EngineConfiguration`/
/// `ExtendedSettings` (the `CommandLineParser` boot).
pub fn initialize() -> anyhow::Result<Option<String>> {
    let bootstrap = load_bootstrap()?;
    initialize_settings();
    Ok(install_bootstrap(bootstrap))
}

/// Data loaded before the GTK session can be installed.
pub struct BootstrapData {
    library: Library,
    incoming: cr_engine::incoming::IncomingCatalog,
    incoming_lists: cr_engine::incoming::IncomingLists,
    message: Option<String>,
}

/// Recovers transactions and loads both catalogs. Call this on a worker.
pub fn load_bootstrap() -> anyhow::Result<BootstrapData> {
    let paths = cr_core::paths::Paths::new_default();
    let _guard = cr_engine::incoming_transaction::acquire_mutation_guard();
    cr_engine::incoming_transaction::TransactionEngine::new(&paths).recover()?;
    let (library, status) = Library::open_without_watcher(&cr_core::paths::database_file(&paths))?;
    let (incoming, incoming_message) =
        match cr_engine::incoming::IncomingCatalog::load(&paths) {
            Ok(catalog) => (catalog, None),
            Err(error) => (
                cr_engine::incoming::IncomingCatalog::default(),
                Some(format!(
                    "The Incoming catalog could not be opened. Incoming starts empty for this session. The catalog file was not changed.\n\n{error}"
                )),
            ),
        };
    let (incoming_lists, incoming_lists_message) =
        match cr_engine::incoming::IncomingLists::load(&paths) {
            Ok(lists) => (lists, None),
            Err(error) => (
                cr_engine::incoming::IncomingLists::default(),
                Some(format!(
                    "The Incoming smart lists could not be opened. Incoming smart lists start empty for this session. The list file was not changed.\n\n{error}"
                )),
            ),
        };
    let message = [
        open_message(status),
        incoming_message,
        incoming_lists_message,
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join("\n\n");
    Ok(BootstrapData {
        library,
        incoming,
        incoming_lists,
        message: (!message.is_empty()).then_some(message),
    })
}

/// Installs worker-loaded startup data on the GTK thread.
pub fn install_bootstrap(bootstrap: BootstrapData) -> Option<String> {
    let mut library = bootstrap.library;
    library.start_watcher();
    SESSION.with(|cell| {
        *cell.borrow_mut() = Some(Rc::new(RefCell::new(library)));
    });
    INCOMING_SESSION.with(|cell| {
        *cell.borrow_mut() = Some(Rc::new(RefCell::new(bootstrap.incoming)));
    });
    INCOMING_LISTS_SESSION.with(|cell| {
        *cell.borrow_mut() = Some(Rc::new(RefCell::new(bootstrap.incoming_lists)));
    });
    bootstrap.message
}

/// Loads settings after the startup worker completes.
pub fn initialize_bootstrap_settings() {
    initialize_settings();
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

/// The incoming-folder section name in the unified config.
pub const INCOMING_PLUGIN: &str = "incoming";

/// The stored incoming-folder configuration.
pub fn incoming_config() -> cr_engine::incoming::IncomingConfig {
    cr_core::settings::unified::get_plugin(INCOMING_PLUGIN).unwrap_or_default()
}

/// Commits the incoming-folder configuration into `[plugins.incoming]`
/// and saves the unified config.
pub fn store_incoming_config(config: &cr_engine::incoming::IncomingConfig) {
    cr_core::settings::unified::set_plugin(INCOMING_PLUGIN, config);
    save_settings();
}

/// The Library Organizer's section name in the unified config
/// (Phase 17, ADR-033).
pub const ORGANIZER_PLUGIN: &str = "library-organizer";

/// The Library Organizer's stored profiles (the built-in Default
/// profile when the section is absent — the addon's `load_profiles`
/// fallback).
pub fn organize_settings() -> cr_organize::profile::PluginSettings {
    cr_core::settings::unified::get_plugin(ORGANIZER_PLUGIN)
        .unwrap_or_else(cr_organize::profile::PluginSettings::builtin)
}

/// Commits the Library Organizer profiles into
/// `[plugins.library-organizer]` and saves the unified config.
pub fn store_organize_settings(settings: &cr_organize::profile::PluginSettings) {
    cr_core::settings::unified::set_plugin(ORGANIZER_PLUGIN, settings);
    save_settings();
}

/// The organizer's undo log (the addon's `undo.dat`, plugin-local
/// state, not config: ADR-033 keeps the plugin directory for state).
pub fn organizer_undo_path() -> std::path::PathBuf {
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .filter(|v| !v.as_os_str().is_empty())
        .or_else(|| {
            std::env::var_os("HOME").map(|h| {
                let mut p = std::path::PathBuf::from(h);
                p.push(".config");
                p
            })
        })
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    root.join("comicrust")
        .join("plugins")
        .join(ORGANIZER_PLUGIN)
        .join("undo.dat")
}

/// The Comic Vine disk cache (ADR-037), retained after a successful open.
///
/// The cache is a file, and the sweep, the warm task, and the scrape
/// worker all use it. `None` means the file could not be opened; the
/// caller must then work without a cache, not fail.
pub fn cv_cache() -> Option<std::sync::Arc<cr_scrape::cache::SqliteCache>> {
    use std::sync::{Mutex, OnceLock};
    static CACHE: OnceLock<Mutex<Option<std::sync::Arc<cr_scrape::cache::SqliteCache>>>> =
        OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    let mut cached = cache.lock().unwrap();
    if let Some(cache) = cached.as_ref() {
        return Some(std::sync::Arc::clone(cache));
    }
    let path = cr_scrape::cache::default_cache_path();
    match cr_scrape::cache::SqliteCache::open(&path) {
        Ok(cache) => {
            let cache = std::sync::Arc::new(cache);
            *cached = Some(std::sync::Arc::clone(&cache));
            Some(cache)
        }
        Err(error) => {
            crate::trace::trace(format!("the Comic Vine cache could not open: {error}"));
            None
        }
    }
}

/// The per-resource request budget over the shared cache, built from
/// the scraper configuration. `None` means no budget: either the cache
/// is off, or its file could not open.
pub fn cv_budget(
    config: &cr_scrape::config::Configuration,
    cache: std::sync::Arc<cr_scrape::cache::SqliteCache>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    on_wait: Option<Box<cr_scrape::cache::budget::WaitFn>>,
) -> Option<std::sync::Arc<cr_scrape::cache::budget::Budget>> {
    if !config.advanced().cache_enabled {
        return None;
    }
    let (policy, _, _) = cr_scrape::cache::policies_from(config.advanced());
    let mut budget = cr_scrape::cache::budget::Budget::new(
        cache as std::sync::Arc<dyn cr_scrape::cache::CvCache>,
        policy,
    )
    .with_cancel(cancel);
    // Without this the job looks frozen while it waits out the hour.
    if let Some(on_wait) = on_wait {
        budget = budget.with_wait_report(on_wait);
    }
    Some(std::sync::Arc::new(budget))
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
        if cr_engine::incoming_transaction::operation_active() {
            return ControlFlow::Continue;
        }
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

/// How many times [`cache_thumbnails`] has dispatched its enqueue
/// loop to a worker thread. This exists so a gate can assert the
/// DISPATCH itself, not just the lamp widget: `statusbar_probe` gate L
/// drove `update_lamps` with synthetic booleans and so could not see
/// that the real command ran its loop inline on the GTK thread and
/// froze the app. A count that does not rise means the work went back
/// onto the main thread.
static THUMBNAIL_WARMUP_SPAWNS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// The [`THUMBNAIL_WARMUP_SPAWNS`] counter.
pub fn thumbnail_warmup_spawns() -> usize {
    THUMBNAIL_WARMUP_SPAWNS.load(std::sync::atomic::Ordering::SeqCst)
}

/// `MainForm.GenerateFrontCoverCache` — the "Generate Cover
/// Thumbnails" command: one unlimited-queue warm-up job per library
/// book. The worker skips entries already in the thumbnail disk
/// cache, so a repeated command is cheap.
///
/// The enqueue loop runs on a WORKER thread (Rule 9). It used to run
/// inline in the menu action and froze the whole app on a large
/// library, because the per-book key carries the file size and the
/// modified time: `front_cover_thumbnail_key` reaches
/// `ImageKey::from_file` -> `file_stats` -> `std::fs::metadata`, so
/// the loop is one `stat()` syscall per book, plus one contended
/// queue-mutex acquisition per book against the running render
/// workers. While the GTK thread sat in that loop it served no
/// redraws and no timers, so the window stopped painting AND the
/// activity lamp could not appear — its visibility poll is a
/// `glib::timeout_add_local` tick that cannot fire on a blocked main
/// loop. The thumbnails themselves were produced the whole time,
/// which is why the cache directory kept filling during the freeze.
///
/// Only the storage snapshot stays on the main thread, because
/// `session()` is a UI thread-local and a worker must never touch it.
/// That clone is pure memory work with no syscalls, and it mirrors
/// the Book Scanner, which also scans a clone of the book storage
/// (ADR-032).
pub fn cache_thumbnails(pool: &std::sync::Arc<cr_engine::image_pool::ImagePool>) {
    let books: Vec<ComicBook> = session().borrow().database().books.clone();
    let pool = std::sync::Arc::clone(pool);
    let spawned = std::thread::Builder::new()
        .name("Thumbnail Warmup".into())
        .spawn(move || {
            let count = books.len();
            let t = std::time::Instant::now();
            crate::trace::trace(format!("thumbnail warmup queueing {count} books"));
            for book in &books {
                let key = cr_engine::image_pool::front_cover_thumbnail_key(book);
                pool.generate_front_cover_thumbnail(key);
            }
            crate::trace::trace(format!(
                "thumbnail warmup queued {count} books in {:?}",
                t.elapsed()
            ));
        });
    match spawned {
        Ok(_) => {
            THUMBNAIL_WARMUP_SPAWNS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        Err(err) => {
            // Spawning failed, so nothing would ever be queued. Say so
            // rather than fail silently; do NOT fall back to the inline
            // loop, which is the freeze this function exists to avoid.
            crate::trace::trace(format!("thumbnail warmup could not start a thread: {err}"));
        }
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

/// The separate Incoming catalog session. Panics before [`initialize`].
pub fn incoming_session() -> Rc<RefCell<cr_engine::incoming::IncomingCatalog>> {
    INCOMING_SESSION.with(|cell| {
        cell.borrow()
            .clone()
            .expect("incoming session not initialized")
    })
}

/// The separate Incoming smart-list session. It is installed on the GTK thread.
pub fn incoming_lists_session() -> Rc<RefCell<cr_engine::incoming::IncomingLists>> {
    INCOMING_LISTS_SESSION.with(|cell| {
        cell.borrow()
            .clone()
            .expect("incoming list session not initialized")
    })
}

pub fn incoming_lists_snapshot() -> cr_engine::incoming::IncomingLists {
    incoming_lists_session().borrow().clone()
}

pub fn is_incoming_custom_list(id: &CrGuid) -> bool {
    incoming_lists_session()
        .borrow()
        .lists
        .iter()
        .any(|list| list.base.id == *id)
}

type IncomingListSaveDone = Box<dyn FnOnce(Result<(), String>)>;

struct IncomingListSave {
    lists: cr_engine::incoming::IncomingLists,
    done: IncomingListSaveDone,
}

fn queue_incoming_lists_save(
    lists: cr_engine::incoming::IncomingLists,
    done: impl FnOnce(Result<(), String>) + 'static,
) {
    INCOMING_LIST_SAVE_QUEUE.with(|queue| {
        queue.borrow_mut().push_back(IncomingListSave {
            lists,
            done: Box::new(done),
        });
    });
    start_next_incoming_lists_save();
}

/// Queues the current Incoming-list state after all prior list saves.
/// Close uses this as a barrier before it saves the main database.
pub fn flush_incoming_lists_for_close(done: impl FnOnce(Result<(), String>) + 'static) {
    queue_incoming_lists_save(incoming_lists_snapshot(), done);
}

fn start_next_incoming_lists_save() {
    if INCOMING_LIST_SAVE_ACTIVE.with(Cell::get) {
        return;
    }
    let Some(job) = INCOMING_LIST_SAVE_QUEUE.with(|queue| queue.borrow_mut().pop_front()) else {
        return;
    };
    INCOMING_LIST_SAVE_ACTIVE.with(|active| active.set(true));
    let (tx, rx) = std::sync::mpsc::channel();
    let mut done = Some(job.done);
    let lists = job.lists;
    let spawn = std::thread::Builder::new()
        .name("Save Incoming Lists".into())
        .spawn(move || {
            let result = lists
                .save(&cr_core::paths::Paths::new_default())
                .map_err(|error| error.to_string());
            let _ = tx.send(result);
        });
    if let Err(error) = spawn {
        INCOMING_LIST_SAVE_ACTIVE.with(|active| active.set(false));
        if let Some(done) = done.take() {
            done(Err(format!(
                "The Incoming smart-list save worker could not start: {error}"
            )));
        }
        start_next_incoming_lists_save();
        return;
    }
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        match rx.try_recv() {
            Ok(result) => {
                INCOMING_LIST_SAVE_ACTIVE.with(|active| active.set(false));
                if let Some(done) = done.take() {
                    done(result);
                }
                start_next_incoming_lists_save();
                ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                INCOMING_LIST_SAVE_ACTIVE.with(|active| active.set(false));
                start_next_incoming_lists_save();
                ControlFlow::Break
            }
        }
    });
}

pub fn new_incoming_smart_list(
    name: &str,
    done: impl FnOnce(Result<(), String>) + 'static,
) -> CrGuid {
    let session = incoming_lists_session();
    let mut lists = session.borrow_mut();
    let id = lists.add(cr_core::database::list_items::SmartListItem {
        base: cr_core::database::list_items::ListItemBase {
            name: Some(name.to_string()),
            quick_open: false,
            ..Default::default()
        },
        ..Default::default()
    });
    let snapshot = lists.clone();
    drop(lists);
    queue_incoming_lists_save(snapshot, done);
    id
}

pub fn find_incoming_smart_list(
    id: &CrGuid,
) -> Option<cr_core::database::list_items::SmartListItem> {
    incoming_lists_session()
        .borrow()
        .lists
        .iter()
        .find(|list| list.base.id == *id)
        .cloned()
}

pub fn update_incoming_smart_list(
    id: &CrGuid,
    mut item: cr_core::database::list_items::SmartListItem,
    done: impl FnOnce(Result<(), String>) + 'static,
) -> bool {
    let session = incoming_lists_session();
    let mut lists = session.borrow_mut();
    let Some(existing) = lists.lists.iter_mut().find(|list| list.base.id == *id) else {
        return false;
    };
    item.base.id = *id;
    item.base.quick_open = false;
    *existing = item;
    let snapshot = lists.clone();
    drop(lists);
    queue_incoming_lists_save(snapshot, done);
    true
}

pub fn remove_incoming_smart_list(
    id: &CrGuid,
    done: impl FnOnce(Result<(), String>) + 'static,
) -> bool {
    let session = incoming_lists_session();
    let mut lists = session.borrow_mut();
    let old_len = lists.lists.len();
    lists.lists.retain(|list| list.base.id != *id);
    if lists.lists.len() == old_len {
        return false;
    }
    for list in &mut lists.lists {
        if list.base_list_id == *id {
            list.base_list_id = CrGuid::EMPTY;
        }
    }
    let snapshot = lists.clone();
    drop(lists);
    queue_incoming_lists_save(snapshot, done);
    true
}

pub fn incoming_smart_list_base_options(edit_id: &CrGuid) -> Vec<(CrGuid, String)> {
    let lists = incoming_lists_snapshot();
    let mut options = vec![(CrGuid::EMPTY, "Incoming".to_string())];
    options.extend(
        lists
            .lists
            .iter()
            .filter(|list| list.base.id != *edit_id)
            .filter(|candidate| {
                let mut id = candidate.base_list_id;
                let mut seen = HashSet::new();
                while !id.is_empty() && seen.insert(id) {
                    if id == *edit_id {
                        return false;
                    }
                    id = lists
                        .lists
                        .iter()
                        .find(|list| list.base.id == id)
                        .map_or(CrGuid::EMPTY, |list| list.base_list_id);
                }
                true
            })
            .map(|list| (list.base.id, list.base.name.clone().unwrap_or_default())),
    );
    options
}

pub fn incoming_list_view_config(
    id: &CrGuid,
) -> Option<cr_core::database::display_config::ItemViewConfig> {
    find_incoming_smart_list(id)?.base.display?.view
}

pub fn set_incoming_list_view_config(
    id: &CrGuid,
    view: Option<cr_core::database::display_config::ItemViewConfig>,
    done: impl FnOnce(Result<(), String>) + 'static,
) -> bool {
    let session = incoming_lists_session();
    let mut lists = session.borrow_mut();
    let Some(list) = lists.lists.iter_mut().find(|list| list.base.id == *id) else {
        return false;
    };
    let current = list
        .base
        .display
        .as_ref()
        .and_then(|display| display.view.clone());
    if current == view {
        return false;
    }
    match view {
        Some(view) => {
            list.base.display.get_or_insert_with(Default::default).view = Some(view);
        }
        None => list.base.display = None,
    }
    let snapshot = lists.clone();
    drop(lists);
    queue_incoming_lists_save(snapshot, done);
    true
}

pub fn evaluate_incoming_smart_list_async(
    id: CrGuid,
    done: impl FnOnce(Result<(String, Vec<ComicBook>), String>) + 'static,
) {
    let lists = incoming_lists_snapshot();
    let catalog = incoming_session().borrow().clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let spawn = std::thread::Builder::new()
        .name("Evaluate Incoming Smart List".into())
        .spawn(move || {
            let name = lists
                .lists
                .iter()
                .find(|list| list.base.id == id)
                .and_then(|list| list.base.name.clone())
                .unwrap_or_default();
            let result = lists
                .evaluate(id, &catalog)
                .map(|books| (name, books.into_iter().cloned().collect()))
                .map_err(|error| error.to_string());
            let _ = tx.send(result);
        });
    if let Err(error) = spawn {
        done(Err(format!(
            "The Incoming smart-list evaluation worker could not start: {error}"
        )));
        return;
    }
    let mut done = Some(done);
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        match rx.try_recv() {
            Ok(result) => {
                if let Some(done) = done.take() {
                    done(result);
                }
                ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                if let Some(done) = done.take() {
                    done(Err(
                        "The Incoming smart-list evaluation worker stopped.".into()
                    ));
                }
                ControlFlow::Break
            }
        }
    });
}

/// A main-thread snapshot for Incoming views and worker jobs.
pub fn incoming_books_snapshot() -> Vec<ComicBook> {
    incoming_session().borrow().books.clone()
}

/// Returns Incoming books in the requested ID order.
pub fn incoming_books_by_ids(ids: &[CrGuid]) -> Vec<ComicBook> {
    let catalog = incoming_session();
    let catalog = catalog.borrow();
    ids.iter()
        .filter_map(|id| catalog.books.iter().find(|book| book.id == *id).cloned())
        .collect()
}

/// Lands a worker-saved Incoming catalog in the main-thread session.
pub fn replace_incoming_catalog(catalog: cr_engine::incoming::IncomingCatalog) {
    crate::trace::trace(format!(
        "incoming catalog landing books={} epoch_before={}",
        catalog.books.len(),
        cr_engine::incoming_transaction::database_epoch()
    ));
    INCOMING_CLASSIFICATION.with(|snapshot| *snapshot.borrow_mut() = None);
    *incoming_session().borrow_mut() = catalog;
    cr_engine::incoming_transaction::advance_database_epoch();
    INCOMING_GAP_VIEW_HOOK.with(|cell| {
        if let Some(hook) = cell.borrow().as_ref() {
            hook();
        }
    });
    refresh_incoming_classification_async();
    refresh_incoming_external_gaps_async();
}

/// Saves an Incoming snapshot on a worker and returns it for main-thread landing.
pub fn save_incoming_catalog_async(
    catalog: cr_engine::incoming::IncomingCatalog,
    done: impl FnOnce(Result<cr_engine::incoming::IncomingCatalog, String>) + 'static,
) {
    if cr_engine::incoming_transaction::operation_active() {
        done(Err("Another Incoming operation is active.".into()));
        return;
    }
    let epoch = cr_engine::incoming_transaction::database_epoch();
    let (tx, rx) = std::sync::mpsc::channel();
    if !cr_engine::incoming_transaction::begin_operation() {
        done(Err("Another operation is active.".into()));
        return;
    }
    std::thread::Builder::new()
        .name("Save Incoming Catalog".into())
        .spawn(move || {
            let _guard = cr_engine::incoming_transaction::acquire_mutation_guard();
            let paths = cr_core::paths::Paths::new_default();
            let result = (|| -> Result<_, String> {
                let mut transaction = cr_engine::incoming_transaction::IncomingTransaction {
                    kind: cr_engine::incoming_transaction::TransactionKind::Scan,
                    stage: cr_engine::incoming_transaction::TransactionStage::Prepared,
                    files: cr_engine::incoming_transaction::TransactionFiles {
                        incoming_catalog: Some(cr_engine::incoming_transaction::FileSnapshot {
                            path: cr_core::paths::incoming_file(&paths),
                            before: std::fs::read(cr_core::paths::incoming_file(&paths)).ok(),
                            after: catalog.to_bytes().map_err(|error| error.to_string())?,
                            remove_after: false,
                        }),
                        ..Default::default()
                    },
                    external_actions: Vec::new(),
                };
                let engine = cr_engine::incoming_transaction::TransactionEngine::new(&paths);
                engine
                    .begin(&transaction)
                    .map_err(|error| error.to_string())?;
                _guard
                    .commit_if_epoch(epoch, &engine, &mut transaction)
                    .inspect_err(|error| {
                        if matches!(
                            error,
                            cr_engine::incoming_transaction::TransactionError::EpochChanged
                        ) {
                            let _ = engine.abort_prepared();
                        }
                    })
                    .map_err(|error| error.to_string())?;
                Ok(catalog)
            })();
            let _ = tx.send(result);
        })
        .expect("spawn Incoming catalog save");
    let mut done = Some(done);
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        match rx.try_recv() {
            Ok(result) => {
                cr_engine::incoming_transaction::end_operation();
                if let Some(done) = done.take() {
                    done(result);
                }
                ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                cr_engine::incoming_transaction::end_operation();
                if let Some(done) = done.take() {
                    done(Err("The Incoming catalog save worker stopped.".into()));
                }
                ControlFlow::Break
            }
        }
    });
}

/// Saves the Incoming catalog and residual organizer undo state on one worker.
pub fn save_incoming_and_undo_async(
    catalog: cr_engine::incoming::IncomingCatalog,
    undo_path: std::path::PathBuf,
    residual: cr_organize::engine::UndoCollection,
    manifest: Option<cr_organize::engine::AdoptionManifest>,
    done: impl FnOnce(Result<cr_engine::incoming::IncomingCatalog, String>) + 'static,
) {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("Save Incoming Undo".into())
        .spawn(move || {
            let manifest_path = cr_organize::engine::adoption_manifest_path(&undo_path);
            let result = (|| -> Result<_, String> {
                catalog
                    .save(&cr_core::paths::Paths::new_default())
                    .map_err(|error| error.to_string())?;
                if residual.is_empty() {
                    remove_if_present(&undo_path)?;
                    remove_if_present(&manifest_path)?;
                } else {
                    residual
                        .save_for_retry(&undo_path)
                        .map_err(|error| error.to_string())?;
                    if let Some(manifest) = manifest {
                        manifest
                            .save(&manifest_path)
                            .map_err(|error| error.to_string())?;
                    }
                }
                Ok(catalog)
            })();
            let _ = tx.send(result);
        })
        .expect("spawn Incoming undo save");
    let mut done = Some(done);
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        match rx.try_recv() {
            Ok(result) => {
                if let Some(done) = done.take() {
                    done(result);
                }
                ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                if let Some(done) = done.take() {
                    done(Err("The Incoming undo save worker stopped.".into()));
                }
                ControlFlow::Break
            }
        }
    });
}

fn remove_if_present(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

/// Returns the last worker-built Incoming classification snapshot.
pub fn incoming_classification_snapshot() -> Option<IncomingClassificationSnapshot> {
    INCOMING_CLASSIFICATION.with(|snapshot| snapshot.borrow().clone())
}

/// Rebuilds Incoming dynamic-view membership on a worker.
pub fn refresh_incoming_classification_async() {
    let generation = INCOMING_CLASSIFICATION_GENERATION.with(|cell| {
        let next = cell.get().wrapping_add(1);
        cell.set(next);
        next
    });
    let active = INCOMING_CLASSIFICATION_ACTIVE.with(|cell| {
        let active = cell.get();
        if !active {
            cell.set(true);
        }
        active
    });
    if active {
        return;
    }
    let books = incoming_books_snapshot();
    let library = session().borrow().database().books.clone();
    let external_gaps = incoming_external_gap_cache();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("Incoming Classification".into())
        .spawn(move || {
            let classifications =
                cr_engine::incoming::classify_incoming(&books, &library, &external_gaps);
            let _ = tx.send((generation, books, classifications));
        })
        .expect("spawn Incoming classification worker");
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        match rx.try_recv() {
            Ok((completed, books, classifications)) => {
                let current = INCOMING_CLASSIFICATION_GENERATION.with(Cell::get);
                INCOMING_CLASSIFICATION_ACTIVE.with(|cell| cell.set(false));
                if completed == current {
                    INCOMING_CLASSIFICATION.with(|snapshot| {
                        *snapshot.borrow_mut() = Some(IncomingClassificationSnapshot {
                            books,
                            classifications,
                        });
                    });
                    INCOMING_GAP_VIEW_HOOK.with(|cell| {
                        if let Some(hook) = cell.borrow().as_ref() {
                            hook();
                        }
                    });
                } else {
                    refresh_incoming_classification_async();
                }
                ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                INCOMING_CLASSIFICATION_ACTIVE.with(|cell| cell.set(false));
                crate::trace::trace(format!(
                    "Incoming classification worker stopped at generation {generation}"
                ));
                let current = INCOMING_CLASSIFICATION_GENERATION.with(Cell::get);
                if generation != current {
                    refresh_incoming_classification_async();
                }
                ControlFlow::Break
            }
        }
    });
}

/// Supplies the last worker-built cached external gaps to Incoming evaluation.
pub fn incoming_external_gap_cache() -> cr_engine::incoming::ExternalGapCache {
    INCOMING_EXTERNAL_GAPS.with(|cache| cache.borrow().clone())
}

/// Installs the view refresh that runs after a current gap snapshot lands.
pub fn set_incoming_gap_view_hook(hook: Option<Box<dyn Fn()>>) {
    INCOMING_GAP_VIEW_HOOK.with(|cell| *cell.borrow_mut() = hook);
}

fn incoming_volume_ids_for(
    identities: &HashSet<cr_engine::incoming::IncomingIdentity>,
    incoming: &[ComicBook],
    library: &[ComicBook],
) -> HashMap<cr_engine::incoming::IncomingIdentity, i64> {
    // Compute each book's identity and series key once, grouping the
    // series keys by identity. The earlier code recomputed
    // `incoming_identity` for every book once per requested identity,
    // which is O(identities x books) and pinned a core on large
    // libraries. One pass over the books is O(books).
    let mut series_keys_by_identity: HashMap<cr_engine::incoming::IncomingIdentity, Vec<String>> =
        HashMap::new();
    for book in incoming.iter().chain(library) {
        if let Some(identity) = cr_engine::incoming::incoming_identity(book) {
            if identities.contains(&identity) {
                let series_key = cr_scrape::bookdata::series_key_of(book);
                series_keys_by_identity
                    .entry(identity)
                    .or_default()
                    .push(series_key);
            }
        }
    }
    series_keys_by_identity
        .into_iter()
        .filter_map(|(identity, keys)| {
            cr_scrape::cache::missing::volume_id_of(keys).map(|id| (identity, id))
        })
        .collect()
}

/// Returns the distinct Comic Vine volume IDs for selected linked Incoming series.
pub fn selected_incoming_volume_ids(selected: &[ComicBook]) -> Vec<i64> {
    let identities: HashSet<_> = selected
        .iter()
        .filter(|book| !book.file_path.is_empty())
        .filter_map(cr_engine::incoming::incoming_identity)
        .collect();
    let incoming = incoming_books_snapshot();
    let library = session().borrow().database().books.clone();
    let mut ids: Vec<_> = incoming_volume_ids_for(&identities, &incoming, &library)
        .into_values()
        .collect();
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// Checks only the selected records for a direct Comic Vine volume link.
pub fn selected_incoming_has_volume_id(selected: &[ComicBook]) -> bool {
    selected.iter().any(|book| {
        cr_scrape::bookdata::series_key_of(book)
            .trim()
            .parse::<i64>()
            .is_ok_and(|id| id > 0)
    })
}

fn project_incoming_external_gaps(
    cache: &dyn cr_scrape::cache::CvCache,
    incoming: &[ComicBook],
    library: &[ComicBook],
) -> cr_engine::incoming::ExternalGapCache {
    let phase_started = std::time::Instant::now();
    let cache_trace_started = cr_engine::matcher::book_view::proposed_cache_trace_snapshot();
    let identities: HashSet<_> = incoming
        .iter()
        .filter_map(cr_engine::incoming::incoming_identity)
        .collect();
    crate::trace::trace(format!(
        "gap refresh thread={} phase=identities elapsed={:?} identities={}",
        cr_core::trace::thread_label(),
        phase_started.elapsed(),
        identities.len()
    ));
    cr_engine::matcher::book_view::trace_proposed_cache_delta(
        "gap-refresh identities",
        cache_trace_started,
    );
    let phase_started = std::time::Instant::now();
    let cache_trace_started = cr_engine::matcher::book_view::proposed_cache_trace_snapshot();
    let volume_ids = incoming_volume_ids_for(&identities, incoming, library);
    crate::trace::trace(format!(
        "gap refresh thread={} phase=volume_ids elapsed={:?} identities={} volumes={}",
        cr_core::trace::thread_label(),
        phase_started.elapsed(),
        identities.len(),
        volume_ids.len()
    ));
    cr_engine::matcher::book_view::trace_proposed_cache_delta(
        "gap-refresh volume-ids",
        cache_trace_started,
    );
    // One pass over `library`, grouping owned issue numbers by identity,
    // instead of rescanning the whole library once per identity below
    // (the same O(identities x books) shape `incoming_volume_ids_for`
    // fixes just above).
    let phase_started = std::time::Instant::now();
    let cache_trace_started = cr_engine::matcher::book_view::proposed_cache_trace_snapshot();
    let mut owned_by_identity: HashMap<cr_engine::incoming::IncomingIdentity, Vec<String>> =
        HashMap::new();
    for book in library {
        if let Some(identity) = cr_engine::incoming::incoming_identity(book) {
            owned_by_identity
                .entry(identity)
                .or_default()
                .push(book.info.number.clone());
        }
    }
    crate::trace::trace(format!(
        "gap refresh thread={} phase=owned_by_identity elapsed={:?} identities={}",
        cr_core::trace::thread_label(),
        phase_started.elapsed(),
        owned_by_identity.len()
    ));
    cr_engine::matcher::book_view::trace_proposed_cache_delta(
        "gap-refresh owned-by-identity",
        cache_trace_started,
    );
    let phase_started = std::time::Instant::now();
    let cache_trace_started = cr_engine::matcher::book_view::proposed_cache_trace_snapshot();
    let mut result = cr_engine::incoming::ExternalGapCache::new();
    for (identity, volume_id) in volume_ids {
        let Ok(issues) = cache.issues_of_volume(volume_id) else {
            continue;
        };
        let owned = owned_by_identity
            .get(&identity)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let gaps: Vec<_> = cr_scrape::cache::missing::missing_issues(&issues, owned)
            .into_iter()
            .filter_map(|issue| cr_engine::incoming::IssueNumber::parse(&issue.issue_number))
            .collect();
        if !gaps.is_empty() {
            result.insert(identity, gaps);
        }
    }
    crate::trace::trace(format!(
        "gap refresh thread={} phase=cache_and_missing elapsed={:?} gap_identities={}",
        cr_core::trace::thread_label(),
        phase_started.elapsed(),
        result.len()
    ));
    cr_engine::matcher::book_view::trace_proposed_cache_delta(
        "gap-refresh cache-and-missing",
        cache_trace_started,
    );
    result
}

/// Refreshes the offline Incoming gap projection without blocking GTK.
#[track_caller]
pub fn refresh_incoming_external_gaps_async() {
    let caller = std::panic::Location::caller();
    let generation = INCOMING_GAP_GENERATION.with(|cell| {
        let next = cell.get().wrapping_add(1);
        cell.set(next);
        next
    });
    let active = INCOMING_GAP_REFRESH_ACTIVE.with(|cell| {
        let active = cell.get();
        if !active {
            cell.set(true);
        }
        active
    });
    crate::trace::trace(format!(
        "gap refresh call generation={generation} active_before={active} caller={}:{}",
        caller.file(),
        caller.line()
    ));
    if active {
        return;
    }
    let snapshot_started = std::time::Instant::now();
    let incoming = incoming_books_snapshot();
    let library = session().borrow().database().books.clone();
    crate::trace::trace(format!(
        "gap refresh phase=snapshots elapsed={:?} incoming={} library={}",
        snapshot_started.elapsed(),
        incoming.len(),
        library.len()
    ));
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("Incoming Comic Vine Gaps".into())
        .spawn(move || {
            let cache = cv_cache();
            let gaps = cache
                .as_deref()
                .map(|cache| project_incoming_external_gaps(cache, &incoming, &library))
                .unwrap_or_default();
            let _ = tx.send((generation, gaps));
        })
        .expect("spawn Incoming Comic Vine gap worker");
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        match rx.try_recv() {
            Ok((completed, gaps)) => {
                let current = INCOMING_GAP_GENERATION.with(Cell::get);
                INCOMING_GAP_REFRESH_ACTIVE.with(|cell| cell.set(false));
                crate::trace::trace(format!(
                    "gap refresh done completed={completed} current={current} respawn={}",
                    completed != current
                ));
                if completed == current {
                    INCOMING_EXTERNAL_GAPS.with(|cache| *cache.borrow_mut() = gaps);
                    refresh_incoming_classification_async();
                } else {
                    refresh_incoming_external_gaps_async();
                }
                ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                INCOMING_GAP_REFRESH_ACTIVE.with(|cell| cell.set(false));
                crate::trace::trace(format!(
                    "Incoming Comic Vine gap worker stopped at generation {generation}"
                ));
                let current = INCOMING_GAP_GENERATION.with(Cell::get);
                if generation != current {
                    refresh_incoming_external_gaps_async();
                }
                ControlFlow::Break
            }
        }
    });
}

/// Builds one synthetic, in-memory-only `ComicBook` per missing issue
/// (Phase 19): the same fields "Fill Missing Issues"
/// (`crates/cr-ui/src/browser/shell.rs`) writes into a real fileless
/// book, minus the insert. This view never calls `insert_new_book`.
fn missing_issues_rows(
    cache: &dyn cr_scrape::cache::CvCache,
    scope_books: &[ComicBook],
    library_books: &[ComicBook],
) -> Vec<ComicBook> {
    cr_scrape::cache::missing::missing_issues_of_library(cache, scope_books, library_books)
        .into_iter()
        .flat_map(|gap| {
            gap.missing.into_iter().map(move |issue| {
                let mut book = crate::dialogs::new_book_series::new_fileless_book();
                book.info.series = gap.series.clone();
                book.info.number = issue.issue_number.clone();
                book.info.volume = gap.volume;
                if let Some(name) = &issue.name {
                    book.info.title = name.clone();
                }
                book.info.year =
                    cr_scrape::cache::missing::year_of_cover_date(issue.cover_date.as_deref())
                        .unwrap_or(-1);
                cr_scrape::bookdata::set_custom_value(
                    &mut book,
                    "comicvine_issue",
                    &issue.issue_id.to_string(),
                );
                cr_scrape::bookdata::set_custom_value(
                    &mut book,
                    "comicvine_volume",
                    &gap.volume_id.to_string(),
                );
                book
            })
        })
        .collect()
}

/// The rows of the last completed Missing Issues pass (Phase 19), or
/// empty before the first refresh. Manual refresh only; never
/// auto-recomputed.
pub fn missing_issues_snapshot() -> Vec<ComicBook> {
    MISSING_ISSUES_ROWS.with(|cell| cell.borrow().clone())
}

/// Unix seconds of the last completed Missing Issues pass, or `None`
/// before the first refresh.
pub fn missing_issues_last_run() -> Option<i64> {
    MISSING_ISSUES_LAST_RUN.with(Cell::get)
}

/// The wall-clock time the last completed Missing Issues pass took, or
/// `None` before the first refresh.
pub fn missing_issues_last_elapsed() -> Option<std::time::Duration> {
    MISSING_ISSUES_LAST_ELAPSED.with(Cell::get)
}

/// True while a Missing Issues gap pass runs.
pub fn missing_issues_refresh_active() -> bool {
    MISSING_ISSUES_ACTIVE.with(Cell::get)
}

/// Runs the Missing Issues gap pass (Phase 19) over `scope_books` on a
/// worker thread and publishes the result on the main thread. `scope_books`
/// is either the whole library or the books a chosen smart list selects
/// (T4 resolves which, on the main thread, before calling this), and
/// decides which series to report on and which numbers count as owned.
/// `library_books` is always the whole library: it decides which Comic
/// Vine volume each series maps to, independent of the chosen scope, so
/// that scoping to a subset that happens to exclude a series' linked
/// copies does not make the whole series silently vanish from the
/// report (see `cr_scrape::cache::missing::missing_issues_of_library`).
/// Reads the local cache skeleton only — never the network (locked
/// decision 2).
///
/// A call while a pass is already running is dropped: this is a manual,
/// button-triggered report, not a reactive refresh, so simple
/// re-entrancy guarding is enough (contrast
/// `refresh_incoming_external_gaps_async`, which coalesces many rapid
/// automatic callers).
pub fn refresh_missing_issues_async(scope_books: Vec<ComicBook>, library_books: Vec<ComicBook>) {
    if MISSING_ISSUES_ACTIVE.with(Cell::get) {
        return;
    }
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    if !start_cv_job(CvJobKind::MissingIssuesGap, cancel) {
        return;
    }
    MISSING_ISSUES_ACTIVE.with(|cell| cell.set(true));
    let generation = MISSING_ISSUES_GENERATION.with(|cell| {
        let next = cell.get().wrapping_add(1);
        cell.set(next);
        next
    });
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("Missing Issues Gap Pass".into())
        .spawn(move || {
            let cache = cv_cache();
            let started = std::time::Instant::now();
            let rows = cache
                .as_deref()
                .map(|cache| missing_issues_rows(cache, &scope_books, &library_books))
                .unwrap_or_default();
            let _ = tx.send((generation, rows, started.elapsed()));
        })
        .expect("spawn Missing Issues gap worker");
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        match rx.try_recv() {
            Ok((completed, rows, elapsed)) => {
                MISSING_ISSUES_ACTIVE.with(|cell| cell.set(false));
                end_cv_job();
                if completed == MISSING_ISSUES_GENERATION.with(Cell::get) {
                    let count = rows.len();
                    MISSING_ISSUES_ROWS.with(|cell| *cell.borrow_mut() = rows);
                    MISSING_ISSUES_LAST_RUN
                        .with(|cell| cell.set(Some(chrono::Utc::now().timestamp())));
                    MISSING_ISSUES_LAST_ELAPSED.with(|cell| cell.set(Some(elapsed)));
                    crate::trace::trace(format!(
                        "missing issues gap pass: {count} rows in {elapsed:?}"
                    ));
                }
                ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                MISSING_ISSUES_ACTIVE.with(|cell| cell.set(false));
                end_cv_job();
                ControlFlow::Break
            }
        }
    });
}

/// The smart lists in the list tree (recursing into folders), for the
/// Missing Issues scope picker (T4). Unlike `smart_list_base_options`,
/// this excludes the Library and Folder entries themselves — only a
/// smart list can narrow the gap pass's input series.
pub fn smart_list_scope_options() -> Vec<(CrGuid, String)> {
    fn walk(
        items: &[cr_core::database::list_items::ComicListItem],
        out: &mut Vec<(CrGuid, String)>,
    ) {
        for i in items {
            match i {
                cr_core::database::list_items::ComicListItem::Smart(s) => {
                    out.push((s.base.id, s.base.name.clone().unwrap_or_default()));
                }
                cr_core::database::list_items::ComicListItem::Folder(f) => walk(&f.items, out),
                cr_core::database::list_items::ComicListItem::Library(_)
                | cr_core::database::list_items::ComicListItem::IdList(_) => {}
            }
        }
    }
    let lib = session();
    let l = lib.borrow();
    let mut out = Vec::new();
    walk(&l.database().comic_lists, &mut out);
    out
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
    if cr_engine::incoming_transaction::operation_active() {
        return None;
    }
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
        crate::gauges::invalidate();
        return Some(book);
    }
    None
}

/// The page-turn write-back (`TrackCurrentPage` mirroring into the
/// library book; `ComicBookNavigator.CurrentPage` setter parity —
/// `set_current_page` carries the `LastPageRead` high-water mark).
/// Temporary books have no library entry and are skipped.
pub fn record_page_change(path: &str, page: i32) {
    if cr_engine::incoming_transaction::operation_active() {
        return;
    }
    let library = session();
    let mut lib = library.borrow_mut();
    if let Some(book) = lib.find_book_mut(path) {
        book.set_current_page(page);
        record_scan_touch(&book.id);
        lib.mark_dirty();
        // LastPageRead moved — the New/Unread classification changes.
        crate::gauges::invalidate();
    }
}

/// `AddFolderToLibrary` — scans the folder recursively on the scan
/// worker. Configured Incoming roots go to the Incoming catalog. All
/// other roots go to the library. `done` runs on the UI thread.
pub fn add_folder_to_library(path: &Path, done: impl FnOnce(ScanResult) + 'static) {
    let location = path.to_string_lossy().into_owned();
    let target = scan_target_for_path(&location, &incoming_config());
    scan_async(location, target, done);
}

/// Selects the catalog for a folder or watcher root.
pub fn scan_target_for_path(
    path: &str,
    config: &cr_engine::incoming::IncomingConfig,
) -> ScanTarget {
    let components = |value: &str| {
        value
            .split(['/', '\\'])
            .filter(|part| !part.is_empty())
            .map(str::to_ascii_lowercase)
            .collect::<Vec<_>>()
    };
    let path_components = components(path);
    let is_root = config
        .incoming_folders
        .iter()
        .any(|root| !path_components.is_empty() && path_components == components(root));
    if is_root || config.is_incoming_path(path) {
        ScanTarget::Incoming
    } else {
        ScanTarget::Library
    }
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
        target: ScanTarget::Library,
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
fn scan_async(location: String, target: ScanTarget, done: impl FnOnce(ScanResult) + 'static) {
    queue_scan(QueuedScan {
        label: location.clone(),
        items: vec![ScanItem {
            location,
            all: true,
            remove_missing: false,
            force_refresh_info: false,
        }],
        limits: scan_limits(),
        target,
        done: Box::new(done),
    });
}

/// Runs the request now, or queues it behind the in-flight scan (the
/// C# scan queue: requests arriving mid-scan wait, one at a time).
fn queue_scan(q: QueuedScan) {
    let in_flight = SCAN_IN_FLIGHT.with(|cell| *cell.borrow());
    crate::trace::trace(format!(
        "scan request target={:?} location='{}' in_flight={in_flight} operation_active={} epoch={}",
        q.target,
        q.label,
        cr_engine::incoming_transaction::operation_active(),
        cr_engine::incoming_transaction::database_epoch()
    ));
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
    Done(Vec<ComicBook>, Box<ScanResult>, std::sync::mpsc::Sender<()>),
    Failed(String),
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
    let incoming = incoming_session();
    SCAN_IN_FLIGHT.with(|cell| *cell.borrow_mut() = true);
    SCAN_TARGET.with(|cell| cell.set(q.target));
    SCAN_LOCATION.with(|cell| *cell.borrow_mut() = q.label.clone());
    let books = match q.target {
        ScanTarget::Library => {
            let lib = library.borrow();
            // A CLONE, not a take: the database keeps the full library
            // while the scan runs. The take emptied it — a re-scan sends
            // zero batches (only NEW files fire `on_new`), so every list
            // evaluation read an empty library mid-scan and the search
            // results blanked until a restart (user report 2026-09-11).
            // The landing merge reconciles the worker's updates with the
            // mid-scan side effects.
            lib.database().books.clone()
        }
        ScanTarget::Incoming => incoming.borrow().books.clone(),
    };
    let scan_epoch = cr_engine::incoming_transaction::database_epoch();
    crate::trace::trace(format!(
        "scan worker admitted target={:?} location='{}' epoch={scan_epoch} operation_active={}",
        q.target,
        q.label,
        cr_engine::incoming_transaction::operation_active()
    ));
    if q.target == ScanTarget::Incoming && !cr_engine::incoming_transaction::begin_operation() {
        SCAN_IN_FLIGHT.with(|cell| *cell.borrow_mut() = false);
        SCAN_LOCATION.with(|cell| cell.borrow_mut().clear());
        (q.done)(ScanResult::default());
        return;
    }
    let target = q.target;
    let incoming_paths = cr_core::paths::Paths::new_default();
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
            crate::trace::trace(format!(
                "scan worker waiting for mutation guard target={target:?} location='{location}' epoch={scan_epoch}"
            ));
            let _mutation_guard = cr_engine::incoming_transaction::acquire_mutation_guard();
            let current_epoch = cr_engine::incoming_transaction::database_epoch();
            if current_epoch != scan_epoch {
                crate::trace::trace(format!(
                    "scan worker rejected before start target={target:?} location='{location}' captured_epoch={scan_epoch} current_epoch={current_epoch}"
                ));
                let _ = tx.send(ScanWorkerMsg::Failed(
                    "The library changed before the scan started. Run the scan again.".into(),
                ));
                return;
            }
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
                    if target == ScanTarget::Library {
                        batch.push(book.clone());
                        if batch.len() >= SCAN_BATCH_SIZE {
                            let _ = tx.send(ScanWorkerMsg::Batch(std::mem::take(&mut batch)));
                        }
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
            if target == ScanTarget::Incoming {
                let catalog = cr_engine::incoming::IncomingCatalog {
                    books: storage.clone(),
                };
                let result = (|| -> Result<(), String> {
                    let path = cr_core::paths::incoming_file(&incoming_paths);
                    let incoming_before = std::fs::read(&path).ok();
                    let mut transaction = cr_engine::incoming_transaction::IncomingTransaction {
                        kind: cr_engine::incoming_transaction::TransactionKind::Scan,
                        stage: cr_engine::incoming_transaction::TransactionStage::Prepared,
                        files: cr_engine::incoming_transaction::TransactionFiles {
                            incoming_catalog: Some(
                                cr_engine::incoming_transaction::FileSnapshot {
                                    before: incoming_before,
                                    after: catalog
                                        .to_bytes()
                                        .map_err(|error| error.to_string())?,
                                    path,
                                    remove_after: false,
                                },
                            ),
                            ..Default::default()
                        },
                        external_actions: Vec::new(),
                    };
                    let engine = cr_engine::incoming_transaction::TransactionEngine::new(
                        &incoming_paths,
                    );
                    engine
                        .begin(&transaction)
                        .map_err(|error| error.to_string())?;
                    if cr_engine::incoming_transaction::database_epoch() != scan_epoch {
                        engine
                            .abort_prepared()
                            .map_err(|error| error.to_string())?;
                        return Err(
                            "The catalogs changed during the Incoming scan. Run the scan again."
                                .into(),
                        );
                    }
                    _mutation_guard
                        .commit_if_epoch(scan_epoch, &engine, &mut transaction)
                        .inspect_err(|error| {
                            if matches!(
                                error,
                                cr_engine::incoming_transaction::TransactionError::EpochChanged
                            ) {
                                let _ = engine.abort_prepared();
                            }
                        })
                        .map(|_| ())
                        .map_err(|error| error.to_string())
                })();
                if let Err(error) = result {
                    let _ = tx.send(ScanWorkerMsg::Failed(error));
                    return;
                }
            }
            let (landed_tx, landed_rx) = std::sync::mpsc::channel();
            if tx
                .send(ScanWorkerMsg::Done(storage, Box::new(result), landed_tx))
                .is_ok()
            {
                let _ = landed_rx.recv();
            }
        })
        .expect("spawn Book Scanner");

    // The once-completion callback rides an Option (the pump closure
    // is FnMut — it cannot move `done` out).
    let mut done = Some(q.done);
    let mut seen_files: usize = 0;
    glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        if target == ScanTarget::Library && cr_engine::incoming_transaction::operation_active() {
            return ControlFlow::Continue;
        }
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
                Ok(ScanWorkerMsg::Done(books, result, landed)) => {
                    // The run summary accumulates across the queued
                    // scans; the shell reports it once at the end.
                    record_scan_problems(&result);
                    // Late batches ride the final storage — the landing
                    // merge replaces the appends wholesale.
                    tick_batch.clear();
                    let data_changed = !result.added.is_empty()
                        || !result.updated.is_empty()
                        || !result.moved.is_empty()
                        || !result.removed.is_empty();
                    match target {
                        ScanTarget::Library => {
                            let mut lib = library.borrow_mut();
                            let (removed, touched) = take_scan_side_effects();
                            let merged = merge_scan_storage(
                                books,
                                &lib.database().books,
                                &removed,
                                &touched,
                            );
                            lib.database_mut().books = merged;
                            if data_changed {
                                lib.mark_dirty();
                                crate::gauges::invalidate();
                            }
                        }
                        ScanTarget::Incoming => {
                            replace_incoming_catalog(cr_engine::incoming::IncomingCatalog { books })
                        }
                    }
                    if target == ScanTarget::Incoming {
                        cr_engine::incoming_transaction::end_operation();
                    }
                    let _ = landed.send(());
                    if data_changed {
                        refresh_incoming_external_gaps_async();
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
                Ok(ScanWorkerMsg::Failed(error)) => {
                    eprintln!("incoming scan save failed: {error}");
                    SCAN_COMPLETION_ERROR.with(|cell| *cell.borrow_mut() = Some((target, error)));
                    SCAN_IN_FLIGHT.with(|cell| *cell.borrow_mut() = false);
                    SCAN_LOCATION.with(|cell| cell.borrow_mut().clear());
                    SCAN_STOP.with(|cell| *cell.borrow_mut() = None);
                    SCAN_SKIP.with(|cell| *cell.borrow_mut() = None);
                    if target == ScanTarget::Incoming {
                        cr_engine::incoming_transaction::end_operation();
                    }
                    if let Some(d) = done.take() {
                        d(ScanResult::default());
                    }
                    let next = SCAN_QUEUE.with(|queue| queue.borrow_mut().pop());
                    if let Some(next) = next {
                        start_scan_worker(next);
                    }
                    return ControlFlow::Break;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    let error = "The scan worker stopped without a result.".to_string();
                    crate::trace::trace(&error);
                    SCAN_COMPLETION_ERROR.with(|cell| *cell.borrow_mut() = Some((target, error)));
                    SCAN_IN_FLIGHT.with(|cell| *cell.borrow_mut() = false);
                    SCAN_LOCATION.with(|cell| cell.borrow_mut().clear());
                    SCAN_STOP.with(|cell| *cell.borrow_mut() = None);
                    SCAN_SKIP.with(|cell| *cell.borrow_mut() = None);
                    if target == ScanTarget::Incoming {
                        cr_engine::incoming_transaction::end_operation();
                    }
                    if let Some(d) = done.take() {
                        d(ScanResult::default());
                    }
                    let next = SCAN_QUEUE.with(|queue| queue.borrow_mut().pop());
                    if let Some(next) = next {
                        start_scan_worker(next);
                    }
                    return ControlFlow::Break;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
            }
        }
        if !tick_batch.is_empty() {
            debug_assert_eq!(target, ScanTarget::Library);
            {
                let mut lib = library.borrow_mut();
                lib.database_mut().books.extend(tick_batch.iter().cloned());
                crate::gauges::invalidate();
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
    if watcher_rescans_deferred(
        is_scanning(),
        cr_engine::incoming_transaction::operation_active(),
    ) {
        return Vec::new();
    }
    let roots = session().borrow_mut().take_watch_folder_rescans();
    if !roots.is_empty() {
        crate::trace::trace(format!(
            "watcher delivered rescans count={} roots={roots:?} operation_active={} epoch={}",
            roots.len(),
            cr_engine::incoming_transaction::operation_active(),
            cr_engine::incoming_transaction::database_epoch()
        ));
    }
    roots
}

fn watcher_rescans_deferred(scanning: bool, operation_active: bool) -> bool {
    scanning || operation_active
}

/// The collapsed Windows-path roots (the migration dialog rows; the
/// Phase 8 T11 helper).
pub fn windows_path_roots() -> Vec<cr_engine::path_migration::PathRoot> {
    session().borrow().windows_path_roots()
}

// ---------- The bulk remove-books job ----------
//
// The file deletion of a large Remove Books / Move to Recycle Bin ran
// INLINE on the GTK main thread — MEASURED 22.9 s for 792 unlinks over
// the CIFS mount, with the UI frozen the whole time (Rule 9). The job
// moves the per-file work to a named worker and lands the book
// removals in batches through a main-thread pump (the ADR-019 shape
// the scan uses). Cancel: the Tasks row and the status-bar lamp.

/// The outcome of one bulk remove job.
#[derive(Clone, Debug, Default)]
pub struct RemoveBooksOutcome {
    pub removed: usize,
    pub failed: usize,
    pub canceled: bool,
    pub removed_ids: Vec<CrGuid>,
    /// The Incoming file paths this job deleted. The UI suppresses the
    /// watcher events for these paths so a discard does not trigger a
    /// full self-scan of the Incoming root.
    pub deleted_paths: Vec<std::path::PathBuf>,
}

/// Worker → pump messages: landing batches of (id, file-deletion-ok)
/// and the end mark (canceled).
enum RemoveWorkerMsg {
    Batch(Vec<(CrGuid, bool)>),
    Done(bool),
}

fn delete_path(path: &str, permanent: bool) -> bool {
    if path.is_empty() {
        return true;
    }
    let path = Path::new(path);
    if !path.is_file() {
        return !path.exists();
    }
    let started = std::time::Instant::now();
    crate::trace::trace(format!(
        "file delete start permanent={permanent} path='{}'",
        path.display()
    ));
    let command_status = if permanent {
        let _ = std::fs::remove_file(path);
        None
    } else {
        std::process::Command::new("gio")
            .args(["trash", &path.to_string_lossy()])
            .status()
            .ok()
            .and_then(|status| status.code())
    };
    let removed = !path.exists();
    crate::trace::trace(format!(
        "file delete finish permanent={permanent} removed={removed} command_status={command_status:?} elapsed_ms={} path='{}'",
        started.elapsed().as_millis(),
        path.display()
    ));
    removed
}

pub fn trash_file(path: &str) -> bool {
    delete_path(path, false)
}

pub fn discard_incoming_async(
    items: Vec<(CrGuid, String)>,
    permanent: bool,
    done: impl FnOnce(
            Result<
                (
                    cr_engine::incoming::IncomingCatalog,
                    RemoveBooksOutcome,
                    u64,
                ),
                String,
            >,
        ) + 'static,
) {
    let request_started = std::time::Instant::now();
    let mut catalog = incoming_session().borrow().clone();
    let captured_epoch = cr_engine::incoming_transaction::database_epoch();
    crate::trace::trace(format!(
        "discard requested items={} permanent={permanent} catalog_books={} epoch={captured_epoch} request_elapsed_ms={}",
        items.len(),
        catalog.books.len(),
        request_started.elapsed().as_millis()
    ));
    let (tx, rx) = std::sync::mpsc::channel();
    if !cr_engine::incoming_transaction::begin_operation() {
        done(Err("Another operation is active.".into()));
        return;
    }
    std::thread::Builder::new()
        .name("Discard Incoming".into())
        .spawn(move || {
            let result = (|| -> Result<_, String> {
                let worker_started = std::time::Instant::now();
                let _guard = cr_engine::incoming_transaction::acquire_mutation_guard();
                if cr_engine::incoming_transaction::database_epoch() != captured_epoch {
                    return Err("The library changed before the discard started. Try again.".into());
                }
                let paths = cr_core::paths::Paths::new_default();
                let catalog_path = cr_core::paths::incoming_file(&paths);
                let engine = cr_engine::incoming_transaction::TransactionEngine::new(&paths);
                let serialize_started = std::time::Instant::now();
                // One serialization: the unchanged catalog is both the
                // `before` snapshot and the initial `after` image.
                let before = catalog.to_bytes().map_err(|error| error.to_string())?;
                crate::trace::trace(format!(
                    "discard worker initial serialization bytes={} elapsed_ms={}",
                    before.len(),
                    serialize_started.elapsed().as_millis()
                ));
                let mut transaction = cr_engine::incoming_transaction::IncomingTransaction {
                    kind: cr_engine::incoming_transaction::TransactionKind::Discard,
                    stage: cr_engine::incoming_transaction::TransactionStage::Prepared,
                    files: cr_engine::incoming_transaction::TransactionFiles {
                        incoming_catalog: Some(cr_engine::incoming_transaction::FileSnapshot {
                            after: before.clone(),
                            before: Some(before),
                            path: catalog_path,
                            remove_after: false,
                        }),
                        ..Default::default()
                    },
                    external_actions: Vec::new(),
                };
                engine
                    .begin(&transaction)
                    .map_err(|error| error.to_string())?;
                crate::trace::trace(format!(
                    "discard journal prepared elapsed_ms={}",
                    worker_started.elapsed().as_millis()
                ));
                let mut outcome = RemoveBooksOutcome::default();
                for (id, path) in items {
                    transaction.external_actions.push(
                        cr_engine::incoming_transaction::ExternalFileAction::Delete {
                            source: path.clone().into(),
                            status: cr_engine::incoming_transaction::ExternalActionStatus::Pending,
                        },
                    );
                    engine
                        .update(&transaction)
                        .map_err(|error| error.to_string())?;
                    crate::trace::trace(format!(
                        "discard delete prepared id={} worker_elapsed_ms={}",
                        id.to_d_string(),
                        worker_started.elapsed().as_millis()
                    ));
                    if delete_path(&path, permanent) {
                        if let Some(cr_engine::incoming_transaction::ExternalFileAction::Delete {
                            status,
                            ..
                        }) = transaction.external_actions.last_mut()
                        {
                            *status =
                                cr_engine::incoming_transaction::ExternalActionStatus::Applied;
                        }
                        catalog.books.retain(|book| book.id != id);
                        let serialize_started = std::time::Instant::now();
                        transaction.files.incoming_catalog.as_mut().unwrap().after =
                            catalog.to_bytes().map_err(|error| error.to_string())?;
                        crate::trace::trace(format!(
                            "discard final catalog serialization books={} bytes={} elapsed_ms={}",
                            catalog.books.len(),
                            transaction.files.incoming_catalog.as_ref().unwrap().after.len(),
                            serialize_started.elapsed().as_millis()
                        ));
                        outcome.removed += 1;
                        outcome.removed_ids.push(id);
                        outcome.deleted_paths.push(path.clone().into());
                    } else {
                        transaction.external_actions.pop();
                        outcome.failed += 1;
                    }
                    engine
                        .update(&transaction)
                        .map_err(|error| error.to_string())?;
                }
                let committed_epoch = _guard
                    .commit_if_epoch(captured_epoch, &engine, &mut transaction)
                    .map_err(|error| error.to_string())?;
                crate::trace::trace(format!(
                    "discard commit complete committed_epoch={committed_epoch} worker_elapsed_ms={}",
                    worker_started.elapsed().as_millis()
                ));
                Ok((catalog, outcome, committed_epoch))
            })();
            let _ = tx.send(result);
        })
        .expect("spawn Incoming discard");
    let mut done = Some(done);
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        match rx.try_recv() {
            Ok(result) => {
                cr_engine::incoming_transaction::end_operation();
                if let Some(done) = done.take() {
                    done(result);
                }
                ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                cr_engine::incoming_transaction::end_operation();
                if let Some(done) = done.take() {
                    done(Err("The Incoming discard worker stopped.".into()));
                }
                ControlFlow::Break
            }
        }
    });
}

pub fn replace_library_copy_async(
    incoming_id: CrGuid,
    library_id: CrGuid,
    done: impl FnOnce(
            Result<
                (
                    cr_core::database::comic_database::ComicDatabase,
                    cr_engine::incoming::IncomingCatalog,
                    u64,
                    Vec<std::path::PathBuf>,
                ),
                String,
            >,
        ) + 'static,
) {
    let database_snapshot = session().borrow().database().clone();
    let incoming_snapshot = incoming_session().borrow().clone();
    let captured_epoch = cr_engine::incoming_transaction::database_epoch();
    crate::trace::trace(format!(
        "replacement requested incoming_id={} library_id={} epoch={captured_epoch}",
        incoming_id.to_d_string(),
        library_id.to_d_string()
    ));
    let Some(operation) = cr_engine::incoming_transaction::try_begin_operation() else {
        done(Err("Another operation is active.".into()));
        return;
    };
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("Replace Library Copy".into())
        .spawn(move || {
            let result = (|| -> Result<_, String> {
                crate::trace::trace(format!(
                    "replacement waiting for mutation guard epoch={captured_epoch}"
                ));
                let guard = cr_engine::incoming_transaction::acquire_mutation_guard();
                if cr_engine::incoming_transaction::database_epoch() != captured_epoch {
                    return Err("The library changed before the replacement started. Try again.".into());
                }
                let mut database = database_snapshot;
                let mut catalog = incoming_snapshot;
                let incoming = catalog
                    .books
                    .iter()
                    .find(|book| book.id == incoming_id)
                    .cloned()
                    .ok_or_else(|| "The Incoming copy is no longer available.".to_string())?;
                let library_index = database
                    .books
                    .iter()
                    .position(|book| book.id == library_id)
                    .ok_or_else(|| "The Library copy is no longer available.".to_string())?;
                let old_library = std::path::PathBuf::from(&database.books[library_index].file_path);
                let source = std::path::PathBuf::from(&incoming.file_path);
                let extension = source
                    .extension()
                    .ok_or_else(|| "The Incoming copy has no file extension.".to_string())?;
                let destination = old_library.with_extension(extension);
                let file_name = destination
                    .file_name()
                    .and_then(|name| name.to_str())
                    .ok_or_else(|| "The replacement destination has no valid file name.".to_string())?;
                let staging = destination.with_file_name(format!(
                    ".{file_name}.{}.incoming-stage",
                    incoming_id.to_d_string()
                ));

                let mut replacement = database.books[library_index].clone();
                replacement.file_path = incoming.file_path.clone();
                replacement.info.page_count = 0;
                let (_, verdict) = cr_engine::scanner::refresh_file_info_reported(&mut replacement);
                if let Some(verdict) = verdict {
                    cr_core::scan_status::apply(
                        &mut replacement,
                        &verdict,
                        &chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                    );
                }
                replacement.file_path = destination.to_string_lossy().into_owned();
                database.books[library_index] = replacement;
                catalog.books.retain(|book| book.id != incoming_id);

                let paths = cr_core::paths::Paths::new_default();
                let database_path = cr_core::paths::database_file(&paths);
                let incoming_path = cr_core::paths::incoming_file(&paths);
                let mut transaction = cr_engine::incoming_transaction::ReplacementTransaction::prepare(
                    source,
                    staging,
                    destination,
                    old_library,
                    cr_engine::incoming_transaction::FileSnapshot {
                        before: std::fs::read(&database_path).ok(),
                        path: database_path,
                        after: cr_core::database::comic_database::save_bytes(&database)
                            .map_err(|error| error.to_string())?,
                        remove_after: false,
                    },
                    cr_engine::incoming_transaction::FileSnapshot {
                        before: std::fs::read(&incoming_path).ok(),
                        path: incoming_path,
                        after: catalog.to_bytes().map_err(|error| error.to_string())?,
                        remove_after: false,
                    },
                )
                .map_err(|error| error.to_string())?;
                let engine = cr_engine::incoming_transaction::TransactionEngine::new(&paths);
                engine
                    .begin_replacement(&transaction)
                    .map_err(|error| error.to_string())?;
                crate::trace::trace(format!(
                    "replacement journal prepared epoch={captured_epoch} source='{}' destination='{}'",
                    transaction.source.display(),
                    transaction.destination.display()
                ));
                if cr_engine::incoming_transaction::database_epoch() != captured_epoch {
                    return Err("The library changed before the replacement committed. Restart ComicRust to recover the pending transaction.".into());
                }
                engine
                    .commit_replacement(&mut transaction)
                    .map_err(|error| error.to_string())?;
                crate::trace::trace(format!(
                    "replacement durable commit complete epoch_before_commit={captured_epoch}"
                ));
                let committed_epoch = cr_engine::incoming_transaction::commit_database_epoch();
                let controlled_paths = vec![
                    transaction.source.clone(),
                    transaction.staging.clone(),
                    transaction.destination.clone(),
                    transaction.old_library.clone(),
                ];
                drop(guard);
                Ok((database, catalog, committed_epoch, controlled_paths))
            })();
            let _ = tx.send((operation, result));
        })
        .expect("spawn Library replacement");
    let mut done = Some(done);
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        match rx.try_recv() {
            Ok((operation, result)) => {
                crate::trace::trace(format!(
                    "replacement worker landed success={} epoch={}",
                    result.is_ok(),
                    cr_engine::incoming_transaction::database_epoch()
                ));
                operation.finish(|_| {
                    if let Some(done) = done.take() {
                        done(result);
                    }
                });
                ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                if let Some(done) = done.take() {
                    done(Err("The Library replacement worker stopped.".into()));
                }
                ControlFlow::Break
            }
        }
    });
}

/// Books per batch send (the pump lands each batch in ONE retain —
/// a per-book borrow_mut/retain was the O(N) per-book removal).
const REMOVE_BATCH_SIZE: usize = 25;

thread_local! {
    /// One bulk remove job at a time. The WORKER never writes this —
    /// the main-thread pump owns it.
    static REMOVE_IN_FLIGHT: RefCell<bool> = const { RefCell::new(false) };
    /// The job's abort flag: `abort_remove_books` sets it, the worker
    /// checks it between books.
    static REMOVE_STOP: RefCell<Option<std::sync::Arc<std::sync::atomic::AtomicBool>>> =
        const { RefCell::new(None) };
    /// (processed, total) for the Tasks row and the lamp.
    static REMOVE_PROGRESS: RefCell<(usize, usize)> = const { RefCell::new((0, 0)) };
}

/// Whether a bulk remove/delete job runs now (the lamp + the Tasks row).
pub fn remove_books_in_flight() -> bool {
    REMOVE_IN_FLIGHT.with(|cell| *cell.borrow())
}

/// (processed, total) of the running job, or None when idle.
pub fn remove_books_progress() -> Option<(usize, usize)> {
    if !remove_books_in_flight() {
        return None;
    }
    let (done, total) = REMOVE_PROGRESS.with(|cell| *cell.borrow());
    Some((done, total))
}

/// The Tasks row's live line while a job runs.
pub fn remove_job_line() -> Option<String> {
    let (done, total) = remove_books_progress()?;
    Some(format!("Deleting books — {done} of {total}"))
}

/// Aborts the running job (the Tasks abort, the lamp cancel): the
/// worker stops BEFORE the next file; books whose files it already
/// deleted still leave the library (a kept book would point at a
/// deleted file).
pub fn abort_remove_books() {
    if let Some(stop) = REMOVE_STOP.with(|cell| cell.borrow().clone()) {
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Starts the bulk job: delete the books' files (when asked) on the
/// worker and drop the books from the database as their deletions
/// succeed. `land_ids` = false runs the SAME file deletion without
/// database removals (the Files-view flow removes by path itself in
/// its own completion callback — the `RemoveRange` shape).
///
/// One job at a time: a request while one runs is refused (the
/// outcome arrives with `canceled = true`).
pub fn remove_books_async(
    ids: Vec<CrGuid>,
    delete_files: bool,
    permanent: bool,
    land_ids: bool,
    done: impl FnOnce(RemoveBooksOutcome) + 'static,
) {
    if remove_books_in_flight() {
        crate::trace::trace("remove-books: job already running — request refused");
        done(RemoveBooksOutcome {
            canceled: true,
            ..Default::default()
        });
        return;
    }
    // One pass for the paths (a per-id linear find was ~900 × 73k
    // compares — small next to the unlinks, but free to fix here).
    let paths: HashMap<CrGuid, String> = {
        let lib = session();
        let l = lib.borrow();
        l.database()
            .books
            .iter()
            .map(|b| (b.id, b.file_path.clone()))
            .collect()
    };
    let items: Vec<(CrGuid, String)> = ids
        .into_iter()
        .map(|id| {
            let path = paths.get(&id).cloned().unwrap_or_default();
            (id, path)
        })
        .collect();
    remove_items_async(items, delete_files, permanent, land_ids, done);
}

/// Deletes explicit `(id, path)` items. Callers outside the main library use
/// the returned IDs to land only successful file operations.
pub fn delete_items_async(
    items: Vec<(CrGuid, String)>,
    permanent: bool,
    done: impl FnOnce(RemoveBooksOutcome) + 'static,
) {
    remove_items_async(items, true, permanent, false, done);
}

fn remove_items_async(
    items: Vec<(CrGuid, String)>,
    delete_files: bool,
    permanent: bool,
    land_ids: bool,
    done: impl FnOnce(RemoveBooksOutcome) + 'static,
) {
    if remove_books_in_flight() || cr_engine::incoming_transaction::operation_active() {
        done(RemoveBooksOutcome {
            canceled: true,
            ..Default::default()
        });
        return;
    }
    let total = items.len();
    let (tx, rx) = std::sync::mpsc::channel::<RemoveWorkerMsg>();
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    REMOVE_IN_FLIGHT.with(|cell| *cell.borrow_mut() = true);
    REMOVE_STOP.with(|cell| *cell.borrow_mut() = Some(std::sync::Arc::clone(&stop)));
    REMOVE_PROGRESS.with(|cell| *cell.borrow_mut() = (0, total));
    crate::trace::trace(format!(
        "remove-books: job started {total} books (delete_files={delete_files} permanent={permanent} land_ids={land_ids})"
    ));
    std::thread::Builder::new()
        .name("Remove Books".into())
        .spawn(move || {
            let t_files = std::time::Instant::now();
            let mut t_file_time = std::time::Duration::ZERO;
            let (mut n_trash, mut n_unlink) = (0usize, 0usize);
            let mut canceled = false;
            let mut batch: Vec<(CrGuid, bool)> = Vec::new();
            for (id, path) in items {
                if stop.load(std::sync::atomic::Ordering::Relaxed) {
                    canceled = true;
                    break;
                }
                let mut ok = true;
                if delete_files && !path.is_empty() {
                    if Path::new(&path).is_file() {
                        let t0 = std::time::Instant::now();
                        if permanent {
                            n_unlink += 1;
                        } else {
                            n_trash += 1;
                        }
                        t_file_time += t0.elapsed();
                    }
                    ok = delete_path(&path, permanent);
                }
                batch.push((id, ok));
                if batch.len() >= REMOVE_BATCH_SIZE {
                    let _ = tx.send(RemoveWorkerMsg::Batch(std::mem::take(&mut batch)));
                }
            }
            if !batch.is_empty() {
                let _ = tx.send(RemoveWorkerMsg::Batch(batch));
            }
            crate::trace::trace(format!(
                "remove-books: files stage done in {:?} — trash {n_trash} unlink {n_unlink} file-time {t_file_time:?}",
                t_files.elapsed()
            ));
            let _ = tx.send(RemoveWorkerMsg::Done(canceled));
        })
        .expect("spawn Remove Books");

    let mut done = Some(done);
    let mut removed_total: usize = 0;
    let mut failed_total: usize = 0;
    let mut removed_ids = Vec::new();
    let land = land_ids;
    glib::timeout_add_local(std::time::Duration::from_millis(100), move || {
        if land && cr_engine::incoming_transaction::operation_active() {
            return glib::ControlFlow::Continue;
        }
        let mut tick_removed: HashSet<CrGuid> = HashSet::new();
        let mut end: Option<bool> = None;
        loop {
            match rx.try_recv() {
                Ok(RemoveWorkerMsg::Batch(batch)) => {
                    REMOVE_PROGRESS.with(|cell| {
                        let (d, t) = *cell.borrow();
                        *cell.borrow_mut() = (d + batch.len(), t);
                    });
                    for (id, ok) in batch {
                        if ok {
                            removed_ids.push(id);
                            removed_total += 1;
                            if land {
                                tick_removed.insert(id);
                            }
                        } else {
                            failed_total += 1;
                        }
                    }
                }
                Ok(RemoveWorkerMsg::Done(canceled)) => {
                    end = Some(canceled);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    // The worker died (a panic): end the job so the
                    // lamp clears and the UI is not stuck in-flight.
                    end = Some(false);
                    break;
                }
            }
        }
        if !tick_removed.is_empty() {
            {
                let lib = session();
                let mut l = lib.borrow_mut();
                // A mid-scan removal must survive the scan's landing
                // merge (recorded on the MAIN thread — here).
                for id in &tick_removed {
                    record_scan_removal(id);
                }
                let before = l.database().books.len();
                l.database_mut()
                    .books
                    .retain(|b| !tick_removed.contains(&b.id));
                if l.database().books.len() != before {
                    l.mark_dirty();
                    crate::gauges::invalidate();
                }
            }
            crate::trace::trace(format!(
                "remove-books: {removed_total} removed, {failed_total} failed, {} processed",
                REMOVE_PROGRESS.with(|cell| cell.borrow().0)
            ));
        }
        let Some(canceled) = end else {
            return ControlFlow::Continue;
        };
        REMOVE_IN_FLIGHT.with(|cell| *cell.borrow_mut() = false);
        REMOVE_STOP.with(|cell| *cell.borrow_mut() = None);
        REMOVE_PROGRESS.with(|cell| *cell.borrow_mut() = (0, 0));
        if let Some(d) = done.take() {
            d(RemoveBooksOutcome {
                removed: removed_total,
                failed: failed_total,
                canceled,
                removed_ids: std::mem::take(&mut removed_ids),
                deleted_paths: Vec::new(),
            });
        }
        ControlFlow::Break
    });
}

/// The Files-view file deletion (the "Move to Recycle Bin" flow): the
/// SAME worker shape, no database landings — the flow's own completion
/// callback does its path-based `RemoveRange` and the folder rescan.
pub fn delete_files_async(
    paths: Vec<String>,
    permanent: bool,
    done: impl FnOnce(RemoveBooksOutcome) + 'static,
) {
    let items = paths
        .into_iter()
        .enumerate()
        .map(|(index, path)| {
            let id =
                CrGuid::parse(&format!("00000000-0000-0000-0000-{index:012}")).unwrap_or_default();
            (id, path)
        })
        .collect();
    delete_items_async(items, permanent, done);
}

#[cfg(test)]
mod delete_path_tests {
    use super::delete_path;

    #[test]
    fn permanent_delete_uses_the_supplied_path() {
        let path = std::env::temp_dir().join(format!(
            "comicrust-delete-path-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, b"test").unwrap();

        assert!(delete_path(&path.to_string_lossy(), true));
        assert!(!path.exists());
    }

    #[test]
    fn missing_path_is_a_success_and_directory_is_not_deleted() {
        let missing = std::env::temp_dir().join("comicrust-delete-path-missing");
        assert!(delete_path(&missing.to_string_lossy(), true));
        assert!(!delete_path(&std::env::temp_dir().to_string_lossy(), true));
    }
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
    if cr_engine::incoming_transaction::operation_active() {
        return cr_engine::path_migration::ApplyReport::default();
    }
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
    let _guard = cr_engine::incoming_transaction::acquire_mutation_guard();
    session().borrow_mut().save()
}

/// Saves while the caller holds the mutation guard.
pub fn save_with_mutation_guard(
    guard: &cr_engine::incoming_transaction::MutationGuard,
) -> Result<(), cr_core::database::DbError> {
    debug_assert!(guard.is_held());
    session().borrow_mut().save()
}

/// Saves the current live database on a worker after the coordinator becomes idle.
pub fn save_for_close_async(done: impl FnOnce(Result<bool, String>) + 'static) {
    let (database, path, generation) = session().borrow().persistence_snapshot();
    let epoch = cr_engine::incoming_transaction::database_epoch();
    let (tx, rx) = std::sync::mpsc::channel();
    if !cr_engine::incoming_transaction::begin_operation() {
        done(Ok(false));
        return;
    }
    std::thread::Builder::new()
        .name("Database Close Save".into())
        .spawn(move || {
            let _guard = cr_engine::incoming_transaction::acquire_mutation_guard();
            let result = if cr_engine::incoming_transaction::database_epoch() != epoch {
                Ok(None)
            } else {
                cr_core::database::comic_database::save(&database, &path)
                    .map(|()| Some(generation))
                    .map_err(|error| error.to_string())
            };
            let _ = tx.send(result);
        })
        .expect("spawn database close save");
    let mut done = Some(done);
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        let result = match rx.try_recv() {
            Ok(Ok(Some(generation))) => {
                let epoch_unchanged = cr_engine::incoming_transaction::database_epoch() == epoch;
                session().borrow_mut().mark_saved(generation);
                Ok(epoch_unchanged && !session().borrow().is_dirty())
            }
            Ok(Ok(None)) => Ok(false),
            Ok(Err(error)) => Err(error),
            Err(std::sync::mpsc::TryRecvError::Empty) => return ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Err("The database close-save worker stopped.".into())
            }
        };
        cr_engine::incoming_transaction::end_operation();
        if let Some(done) = done.take() {
            done(result);
        }
        ControlFlow::Break
    });
}

/// `DatabaseManager.SaveInBackground`: saves only when dirty. Runs
/// MID-SCAN too: since ADR-032 the worker scans a CLONE and the
/// main-thread database stays live (the batches appended so far) —
/// the C# background-saves its live collection while the scanner
/// walks. Returns whether a save ran.
pub fn save_if_dirty() -> Result<bool, cr_core::database::DbError> {
    let t = std::time::Instant::now();
    let _guard = cr_engine::incoming_transaction::acquire_mutation_guard();
    let saved = session().borrow_mut().save_if_dirty();
    if saved.as_ref().is_ok_and(|ran| *ran) {
        crate::trace::trace(format!("save_if_dirty: saved {:?}", t.elapsed()));
    }
    saved
}

/// Saves a dirty database snapshot on a worker and clears dirty state only for that generation.
pub fn save_if_dirty_async(done: impl FnOnce(Result<bool, String>) + 'static) {
    if BACKGROUND_SAVE_IN_FLIGHT.with(|value| *value.borrow()) {
        done(Ok(false));
        return;
    }
    let snapshot = session().borrow().save_snapshot();
    let Some((database, path, generation)) = snapshot else {
        done(Ok(false));
        return;
    };
    BACKGROUND_SAVE_IN_FLIGHT.with(|value| *value.borrow_mut() = true);
    let epoch = cr_engine::incoming_transaction::database_epoch();
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("Database Background Save".into())
        .spawn(move || {
            let _guard = cr_engine::incoming_transaction::acquire_mutation_guard();
            if cr_engine::incoming_transaction::database_epoch() != epoch {
                let _ = tx.send(Ok(None));
                return;
            }
            let result = cr_core::database::comic_database::save(&database, &path)
                .map(|()| Some(generation))
                .map_err(|error| error.to_string());
            let _ = tx.send(result);
        })
        .expect("spawn database background save");
    let mut done = Some(done);
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        match rx.try_recv() {
            Ok(Ok(generation)) => {
                if let Some(generation) = generation {
                    session().borrow_mut().mark_saved(generation);
                }
                BACKGROUND_SAVE_IN_FLIGHT.with(|value| *value.borrow_mut() = false);
                if let Some(done) = done.take() {
                    done(Ok(generation.is_some()));
                }
                ControlFlow::Break
            }
            Ok(Err(error)) => {
                BACKGROUND_SAVE_IN_FLIGHT.with(|value| *value.borrow_mut() = false);
                if let Some(done) = done.take() {
                    done(Err(error));
                }
                ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                BACKGROUND_SAVE_IN_FLIGHT.with(|value| *value.borrow_mut() = false);
                if let Some(done) = done.take() {
                    done(Err("The database save worker stopped.".into()));
                }
                ControlFlow::Break
            }
        }
    });
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

/// Writes the gauge counters into one session list item (the C#
/// `CommitCache` counter write: BookCount / NewBookCount /
/// UnreadBookCount, and NewBookCountDate = the pass snapshot) and
/// marks the database dirty so they persist into ComicDb.xml.
pub fn store_list_gauges(id: &CrGuid, gauges: cr_engine::gauges::Gauges, now: CrDateTime) -> bool {
    if cr_engine::incoming_transaction::operation_active() {
        return false;
    }
    let lib = session();
    let mut l = lib.borrow_mut();
    let Some(base) = find_base_mut(&mut l.database_mut().comic_lists, id) else {
        return false;
    };
    base.book_count = gauges.total;
    base.new_book_count = gauges.new;
    base.unread_book_count = gauges.unread;
    base.new_book_count_date = now;
    l.mark_dirty();
    true
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
    if cr_engine::incoming_transaction::operation_active() {
        return;
    }
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
    // A folder's membership may have changed — recompute its gauges.
    crate::gauges::invalidate();
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
    if cr_engine::incoming_transaction::operation_active() {
        return Err("An Incoming operation is active.".into());
    }
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

/// The list's own view settings (`ComicListItem.Display.View`).
/// `None` = the list has no settings of its own and inherits.
pub fn list_view_config(id: &CrGuid) -> Option<cr_core::database::display_config::ItemViewConfig> {
    let lib = session();
    let mut l = lib.borrow_mut();
    let lists = &mut l.database_mut().comic_lists;
    find_base_mut(lists, id)?.display.as_ref()?.view.clone()
}

/// Writes the list's own view settings (the C#
/// `ComicBrowserControl.UpdateViewConfig`, `:3355-3390`). `None`
/// clears them, which returns the list to inheriting.
///
/// The C# `Display` getter is never null, so a list with no `<Display>`
/// yet gets a default one rather than losing the sibling fields
/// (`ShowComicType`, `QuickSearch`, and the rest).
pub fn set_list_view_config(
    id: &CrGuid,
    view: Option<cr_core::database::display_config::ItemViewConfig>,
) -> bool {
    if cr_engine::incoming_transaction::operation_active() {
        return false;
    }
    let lib = session();
    let mut l = lib.borrow_mut();
    let lists = &mut l.database_mut().comic_lists;
    let Some(base) = find_base_mut(lists, id) else {
        return false;
    };
    let display = base.display.get_or_insert_with(Default::default);
    if display.view == view {
        return false;
    }
    display.view = view;
    l.mark_dirty();
    true
}

/// Rename (the C# `AfterLabelEdit` → `comicListItem.Name = label`).
pub fn rename_list(id: &CrGuid, name: &str) {
    if cr_engine::incoming_transaction::operation_active() {
        return;
    }
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
    if cr_engine::incoming_transaction::operation_active() {
        return;
    }
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
    if cr_engine::incoming_transaction::operation_active() {
        return;
    }
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
        // The parent folder lost a child — its gauges change.
        crate::gauges::invalidate();
    }
}

/// Where a dragged navigator row lands (`tvQueries_DragDrop`,
/// `ComicListLibraryBrowser.cs:1045`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListDrop {
    /// Dropped ON a folder row: the item becomes the folder's LAST
    /// child (the C# `((ComicListItemFolder)dropNode.Tag).Items.Add`).
    IntoFolder(CrGuid),
    /// The C# "separator" style: the item lands BEFORE this row inside
    /// that row's own container. It applies when the pointer is in the
    /// top 4 px of a row, and always when the row is not a folder.
    BeforeItem(CrGuid),
    /// Dropped on empty space: the item moves to the end of the root
    /// list (the C# `dropNode == null` branch).
    RootEnd,
}

/// Moves one list or folder inside the ComicLists tree — the internal
/// drag of `tvQueries_DragDrop`. Returns whether the tree changed.
///
/// Refused: the Library root (it never drags), a drop on the item
/// itself, and a drop into the item's own subtree (the C# refuses
/// these at `SetDropEffects` time through the recursive `Nodes.Find`,
/// `FormUtility.cs:399`).
pub fn move_list_item(id: &CrGuid, drop: &ListDrop) -> bool {
    if cr_engine::incoming_transaction::operation_active() {
        return false;
    }
    let lib = session();
    let mut l = lib.borrow_mut();
    let moved = apply_list_move(&mut l.database_mut().comic_lists, id, drop);
    if moved {
        l.mark_dirty();
        // The containers on both ends changed membership.
        crate::gauges::invalidate();
    }
    moved
}

/// `SortList` (`ComicListLibraryBrowser.cs:1162`): sorts ONE folder's
/// items — folders first, then the rest by name with the
/// `ZeroesFirst | IgnoreArticles | IgnoreCase` comparer. The C# enables
/// the command only for a folder row, so the root list never sorts.
pub fn sort_folder(id: &CrGuid) -> bool {
    if cr_engine::incoming_transaction::operation_active() {
        return false;
    }
    let lib = session();
    let mut l = lib.borrow_mut();
    let Some(folder) = find_folder_mut(&mut l.database_mut().comic_lists, id) else {
        return false;
    };
    sort_list_items(&mut folder.items);
    l.mark_dirty();
    true
}

/// The pure body of [`move_list_item`], over the tree itself.
fn apply_list_move(
    items: &mut Vec<cr_core::database::list_items::ComicListItem>,
    id: &CrGuid,
    drop: &ListDrop,
) -> bool {
    use cr_core::database::list_items::ComicListItem;

    // The Library root never moves (`tvQueries_ItemDrag` clears the
    // drag node for it).
    let Some(source) = find_list_item_in(items, id) else {
        return false;
    };
    if matches!(source, ComicListItem::Library(_)) {
        return false;
    }
    // A drop on the item itself, or anywhere inside its own subtree,
    // would build a cycle.
    let target = match drop {
        ListDrop::IntoFolder(t) | ListDrop::BeforeItem(t) => Some(*t),
        ListDrop::RootEnd => None,
    };
    if let Some(target) = target {
        if target == *id || contains_item(source, &target) {
            return false;
        }
    }

    // Take the item out first; every destination index is then read
    // from the tree as it stands, which is what the C# achieves with
    // its `sourceIndex < dropIndex` decrement (:1076).
    let Some(container) = find_container(items, id) else {
        return false;
    };
    let Some(position) = container.iter().position(|i| i.base().id == *id) else {
        return false;
    };
    let item = container.remove(position);
    let restore = |items: &mut Vec<ComicListItem>, item: ComicListItem| {
        // The destination vanished: put the item back where it was.
        if let Some(container) = find_container(items, id) {
            let at = position.min(container.len());
            container.insert(at, item);
        } else {
            items.push(item);
        }
    };

    match drop {
        ListDrop::RootEnd => {
            items.push(item);
            true
        }
        ListDrop::IntoFolder(folder_id) => match find_folder_mut(items, folder_id) {
            Some(folder) => {
                folder.items.push(item);
                true
            }
            None => {
                restore(items, item);
                false
            }
        },
        ListDrop::BeforeItem(target_id) => {
            let Some(container) = find_container(items, target_id) else {
                restore(items, item);
                return false;
            };
            let Some(at) = container.iter().position(|i| i.base().id == *target_id) else {
                restore(items, item);
                return false;
            };
            // Deviation from the C#, which inserts above the Library
            // root when the drop lands in its top 4 px and paints the
            // list above it. The root stays first here.
            let at = if matches!(container[at], ComicListItem::Library(_)) {
                at + 1
            } else {
                at
            };
            container.insert(at, item);
            true
        }
    }
}

/// The `SortList` comparator: folders before everything else, then the
/// name compare the C# passes (`:1167-1173`).
fn sort_list_items(items: &mut [cr_core::database::list_items::ComicListItem]) {
    use cr_core::database::list_items::ComicListItem;
    items.sort_by(|a, b| {
        let folder_a = matches!(a, ComicListItem::Folder(_));
        let folder_b = matches!(b, ComicListItem::Folder(_));
        match folder_b.cmp(&folder_a) {
            std::cmp::Ordering::Equal => {
                cr_io::extended_compare::extended_compare_zeroes_first_articles_case(
                    a.base().name.as_deref().unwrap_or(""),
                    b.base().name.as_deref().unwrap_or(""),
                )
            }
            other => other,
        }
    });
}

/// Whether `id` is `item` itself or sits anywhere under it.
fn contains_item(item: &cr_core::database::list_items::ComicListItem, id: &CrGuid) -> bool {
    if item.base().id == *id {
        return true;
    }
    match item {
        cr_core::database::list_items::ComicListItem::Folder(f) => {
            f.items.iter().any(|child| contains_item(child, id))
        }
        _ => false,
    }
}

/// Depth-first borrow of one item in a tree.
fn find_list_item_in<'a>(
    items: &'a [cr_core::database::list_items::ComicListItem],
    id: &CrGuid,
) -> Option<&'a cr_core::database::list_items::ComicListItem> {
    for item in items {
        if item.base().id == *id {
            return Some(item);
        }
        if let cr_core::database::list_items::ComicListItem::Folder(f) = item {
            if let Some(found) = find_list_item_in(&f.items, id) {
                return Some(found);
            }
        }
    }
    None
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
    if cr_engine::incoming_transaction::operation_active() {
        return false;
    }
    apply_edited_inner(edited)
}

/// Applies a group of edited books in one database pass. This is the
/// bulk form of [`apply_edited`]. It invalidates derived views once and
/// schedules one file update for each changed book.
pub fn apply_edited_many(edited: Vec<ComicBook>) -> usize {
    if edited.is_empty() || cr_engine::incoming_transaction::operation_active() {
        return 0;
    }

    let mut by_id: HashMap<CrGuid, ComicBook> =
        edited.into_iter().map(|book| (book.id, book)).collect();
    let lib = session();
    let mut l = lib.borrow_mut();
    let mut changed = Vec::new();
    for slot in &mut l.database_mut().books {
        let Some(mut replacement) = by_id.remove(&slot.id) else {
            continue;
        };
        replacement.comic_info_is_dirty = true;
        changed.push(replacement.id);
        *slot = replacement;
    }
    if !changed.is_empty() {
        l.mark_dirty();
    }
    drop(l);

    for id in &changed {
        record_scan_touch(id);
        schedule_book_file_update(id);
    }
    if !changed.is_empty() {
        crate::gauges::invalidate();
        refresh_incoming_external_gaps_async();
    }
    changed.len()
}

/// Applies an organizer result while its exclusive operation is still active.
pub(crate) fn apply_edited_from_organizer(
    edited: &ComicBook,
    _operation: &cr_engine::incoming_transaction::ActiveOperation,
) -> bool {
    apply_edited_inner(edited)
}

fn apply_edited_inner(edited: &ComicBook) -> bool {
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
    // The commit may carry AddedTime / read progress — reclassify.
    crate::gauges::invalidate();
    refresh_incoming_external_gaps_async();
    schedule_book_file_update(&id);
    true
}

/// `Program.Database.Add` (the `MainForm.AddNewBook` insert path):
/// pushes the new book into the books table and marks the database
/// dirty. Returns false when a book with the id already exists — the
/// book editor commits fire per save point (Apply/OK), so the INSERT
/// must run once and later commits apply instead.
pub fn insert_new_book(book: &ComicBook) -> bool {
    if cr_engine::incoming_transaction::operation_active() {
        return false;
    }
    insert_new_book_inner(book)
}

pub(crate) fn insert_new_book_from_organizer(
    book: &ComicBook,
    _operation: &cr_engine::incoming_transaction::ActiveOperation,
) -> bool {
    insert_new_book_inner(book)
}

fn insert_new_book_inner(book: &ComicBook) -> bool {
    let lib = session();
    let mut l = lib.borrow_mut();
    if l.database().books.iter().any(|b| b.id == book.id) {
        return false;
    }
    l.database_mut().books.push(book.clone());
    l.mark_dirty();
    crate::gauges::invalidate();
    drop(l);
    refresh_incoming_external_gaps_async();
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
        if cr_engine::incoming_transaction::operation_active() {
            return glib::ControlFlow::Continue;
        }
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
    if cr_engine::incoming_transaction::operation_active() {
        return Err("An Incoming operation is active.".into());
    }
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
        // Books left/entered the library (the source removals and the
        // added/kept export output) — reclassify.
        crate::gauges::invalidate();
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

/// Removes one book from the library by id (the context-menu
/// command; the file on disk is untouched).
pub fn remove_book(id: &CrGuid) {
    if cr_engine::incoming_transaction::operation_active() {
        return;
    }
    remove_book_inner(id);
}

pub(crate) fn remove_book_from_organizer(
    id: &CrGuid,
    _operation: &cr_engine::incoming_transaction::ActiveOperation,
) {
    remove_book_inner(id);
}

fn remove_book_inner(id: &CrGuid) {
    let lib = session();
    let mut l = lib.borrow_mut();
    let before = l.database().books.len();
    l.database_mut().books.retain(|b| b.id != *id);
    let changed = l.database().books.len() != before;
    if changed {
        record_scan_removal(id);
        l.mark_dirty();
        crate::gauges::invalidate();
    }
    drop(l);
    if changed {
        refresh_incoming_external_gaps_async();
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
    if cr_engine::incoming_transaction::operation_active() {
        return false;
    }
    let t = std::time::Instant::now();
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
        // The matcher set changed — the list's membership changes.
        crate::gauges::invalidate();
    }
    crate::trace::trace(format!(
        "update_smart_list id={id} changed={changed} {:?}",
        t.elapsed()
    ));
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
    if cr_engine::incoming_transaction::operation_active() {
        return id;
    }
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
    if cr_engine::incoming_transaction::operation_active() {
        return id;
    }
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
    if books.is_empty() || cr_engine::incoming_transaction::operation_active() {
        return;
    }
    let lib = session();
    let mut l = lib.borrow_mut();
    l.database_mut().books.extend(books);
    l.mark_dirty();
    crate::gauges::invalidate();
    drop(l);
    refresh_incoming_external_gaps_async();
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
    if cr_engine::incoming_transaction::operation_active() {
        return false;
    }
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
        // A folder's combine mode may have changed — its set changes.
        crate::gauges::invalidate();
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
    use cr_scrape::cache::CvCache;

    #[test]
    fn watcher_rescans_wait_for_scans_and_incoming_operations() {
        assert!(!watcher_rescans_deferred(false, false));
        assert!(watcher_rescans_deferred(true, false));
        assert!(watcher_rescans_deferred(false, true));
        assert!(watcher_rescans_deferred(true, true));
    }

    fn book(path: &str) -> ComicBook {
        ComicBook {
            file_path: path.to_string(),
            id: CrGuid::new_random(),
            ..ComicBook::default()
        }
    }

    fn series_book(path: &str, number: &str, volume_id: Option<i64>) -> ComicBook {
        let mut book = book(path);
        book.info.series = "Example Series".into();
        book.info.volume = 2020;
        book.info.number = number.into();
        book.info.language_iso = "en".into();
        if let Some(volume_id) = volume_id {
            cr_scrape::bookdata::set_custom_value(
                &mut book,
                "comicvine_volume",
                &volume_id.to_string(),
            );
        }
        book
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

    #[test]
    fn cached_issue_skeletons_project_to_incoming_gaps_without_fetching() {
        let cache = cr_scrape::cache::SqliteCache::in_memory().expect("cache");
        cache
            .put_issues(&[
                cr_scrape::cache::IssueSkeleton {
                    issue_id: 1,
                    volume_id: 771,
                    issue_number: "1".into(),
                    ..Default::default()
                },
                cr_scrape::cache::IssueSkeleton {
                    issue_id: 2,
                    volume_id: 771,
                    issue_number: "2".into(),
                    ..Default::default()
                },
                cr_scrape::cache::IssueSkeleton {
                    issue_id: 3,
                    volume_id: 771,
                    issue_number: "Special".into(),
                    ..Default::default()
                },
            ])
            .expect("seed issues");
        let incoming = vec![series_book("incoming-2.cbz", "2", None)];
        let library = vec![series_book("owned-1.cbz", "1", Some(771))];

        let gaps = project_incoming_external_gaps(&cache, &incoming, &library);

        let identity = cr_engine::incoming::incoming_identity(&incoming[0]).expect("identity");
        let values: Vec<f32> = gaps[&identity]
            .iter()
            .map(|number| number.value())
            .collect();
        assert_eq!(values, vec![2.0]);
    }

    /// Regression test for a bug where the owned-issue-number lookup
    /// rescanned the whole library once per identity, which could let
    /// one series' owned numbers leak into another series' gap set.
    #[test]
    fn project_incoming_external_gaps_does_not_cross_contaminate_identities() {
        fn other_series_book(path: &str, number: &str, volume_id: Option<i64>) -> ComicBook {
            let mut book = book(path);
            book.info.series = "Other Series".into();
            book.info.volume = 1999;
            book.info.number = number.into();
            book.info.language_iso = "en".into();
            if let Some(volume_id) = volume_id {
                cr_scrape::bookdata::set_custom_value(
                    &mut book,
                    "comicvine_volume",
                    &volume_id.to_string(),
                );
            }
            book
        }

        let cache = cr_scrape::cache::SqliteCache::in_memory().expect("cache");
        cache
            .put_issues(&[
                cr_scrape::cache::IssueSkeleton {
                    issue_id: 1,
                    volume_id: 771,
                    issue_number: "1".into(),
                    ..Default::default()
                },
                cr_scrape::cache::IssueSkeleton {
                    issue_id: 2,
                    volume_id: 771,
                    issue_number: "2".into(),
                    ..Default::default()
                },
                cr_scrape::cache::IssueSkeleton {
                    issue_id: 3,
                    volume_id: 900,
                    issue_number: "1".into(),
                    ..Default::default()
                },
                cr_scrape::cache::IssueSkeleton {
                    issue_id: 4,
                    volume_id: 900,
                    issue_number: "2".into(),
                    ..Default::default()
                },
            ])
            .expect("seed issues");
        // Two distinct series, each owning issue "1" in the library and
        // missing issue "2". A grouping bug (e.g. an unqualified lookup
        // or wrong key) would let "Example Series" owning "1" hide
        // "Other Series"'s gap on "1", or vice versa.
        let incoming = vec![
            series_book("incoming-example.cbz", "2", None),
            other_series_book("incoming-other.cbz", "2", None),
        ];
        let library = vec![
            series_book("owned-example-1.cbz", "1", Some(771)),
            other_series_book("owned-other-1.cbz", "1", Some(900)),
        ];

        let gaps = project_incoming_external_gaps(&cache, &incoming, &library);

        let example_identity =
            cr_engine::incoming::incoming_identity(&incoming[0]).expect("identity");
        let other_identity =
            cr_engine::incoming::incoming_identity(&incoming[1]).expect("identity");
        assert_ne!(example_identity, other_identity);

        let example_values: Vec<f32> = gaps[&example_identity]
            .iter()
            .map(|number| number.value())
            .collect();
        assert_eq!(example_values, vec![2.0]);

        let other_values: Vec<f32> = gaps[&other_identity]
            .iter()
            .map(|number| number.value())
            .collect();
        assert_eq!(other_values, vec![2.0]);
    }

    /// Phase 19-style timing gate at a scale comparable to the
    /// 21,599-book real library named in `docs/current-status.md` (the
    /// O(identities x books) idle-CPU incident this test exists to avoid
    /// repeating in `project_incoming_external_gaps`'s owned-number
    /// lookup). Synthetic, not a real-library measurement.
    #[test]
    fn project_incoming_external_gaps_completes_well_inside_budget() {
        const SERIES: i64 = 2_000;
        const BOOKS_PER_SERIES: i64 = 10;
        const ISSUES_PER_VOLUME: i64 = 15;
        let budget = std::time::Duration::from_secs(10);

        let cache = cr_scrape::cache::SqliteCache::in_memory().expect("cache");
        let mut cv_issues = Vec::new();
        for volume_id in 1..=SERIES {
            for number in 1..=ISSUES_PER_VOLUME {
                cv_issues.push(cr_scrape::cache::IssueSkeleton {
                    issue_id: volume_id * 1000 + number,
                    volume_id,
                    issue_number: number.to_string(),
                    ..Default::default()
                });
            }
        }
        cache.put_issues(&cv_issues).expect("seed issue skeletons");

        let mut incoming = Vec::with_capacity(SERIES as usize);
        let mut library = Vec::with_capacity((SERIES * BOOKS_PER_SERIES) as usize);
        for volume_id in 1..=SERIES {
            let mut wanted = book(&format!("/incoming/series{volume_id:04}.cbz"));
            wanted.info.series = format!("Series {volume_id:04}");
            wanted.info.volume = 2020;
            wanted.info.number = (ISSUES_PER_VOLUME + 1).to_string();
            wanted.info.language_iso = "en".into();
            incoming.push(wanted);

            for number in 1..=BOOKS_PER_SERIES {
                let mut owned = book(&format!("/library/series{volume_id:04}#{number:02}.cbz"));
                owned.info.series = format!("Series {volume_id:04}");
                owned.info.volume = 2020;
                owned.info.number = number.to_string();
                owned.info.language_iso = "en".into();
                cr_scrape::bookdata::set_custom_value(
                    &mut owned,
                    "comicvine_volume",
                    &volume_id.to_string(),
                );
                library.push(owned);
            }
        }

        let t = std::time::Instant::now();
        let gaps = project_incoming_external_gaps(&cache, &incoming, &library);
        let elapsed = t.elapsed();
        eprintln!(
            "Incoming external gap pass x {} library books / {SERIES} series: {elapsed:?} \
             ({} series with gaps)",
            library.len(),
            gaps.len()
        );
        assert_eq!(
            gaps.len(),
            SERIES as usize,
            "every series owns fewer issues than the cache has, so every identity should report a gap"
        );
        assert!(
            elapsed < budget,
            "gap pass took {elapsed:?}, over the {budget:?} budget — check for a \
             reintroduced per-identity full-library scan in project_incoming_external_gaps"
        );
    }

    #[test]
    fn folder_scan_routing_is_component_aware() {
        let config = cr_engine::incoming::IncomingConfig {
            incoming_folders: vec!["/comics/incoming".into()],
            ..Default::default()
        };

        assert_eq!(
            scan_target_for_path("/COMICS/incoming/", &config),
            ScanTarget::Incoming
        );
        assert_eq!(
            scan_target_for_path("/comics/incoming/series", &config),
            ScanTarget::Incoming
        );
        assert_eq!(
            scan_target_for_path("/comics/incoming-old", &config),
            ScanTarget::Library
        );
    }
}

#[cfg(test)]
mod cv_job_tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    fn flag() -> Arc<AtomicBool> {
        Arc::new(AtomicBool::new(false))
    }

    #[test]
    fn the_job_slot_holds_one_job_at_a_time() {
        end_cv_job();
        assert!(!cv_job_active());
        assert!(start_cv_job(CvJobKind::Sweep, flag()));
        assert!(cv_job_active());
        // A second job is refused. Two sweeps would race on the one
        // sweep_state row and each would think it owned the budget.
        assert!(!start_cv_job(CvJobKind::Warm, flag()));
        assert_eq!(cv_job().map(|j| j.kind), Some(CvJobKind::Sweep));
        end_cv_job();
        assert!(!cv_job_active());
        // The slot is free again.
        assert!(start_cv_job(CvJobKind::Warm, flag()));
        end_cv_job();
    }

    #[test]
    fn the_job_text_reads_as_a_sentence() {
        end_cv_job();
        assert!(start_cv_job(CvJobKind::Sweep, flag()));
        // No detail yet: the heading alone.
        assert_eq!(
            cv_job().map(|j| j.text()),
            Some("Updating the Comic Vine cache".to_string())
        );
        set_cv_job_progress("page 3 of 53".into(), 300, 5300);
        let job = cv_job().expect("a job runs");
        assert_eq!(
            job.text(),
            "Updating the Comic Vine cache \u{2014} page 3 of 53"
        );
        assert_eq!(job.done, 300);
        assert_eq!(job.total, 5300);
        end_cv_job();
    }

    #[test]
    fn progress_after_the_job_ended_is_dropped() {
        end_cv_job();
        set_cv_job_progress("ghost".into(), 1, 2);
        assert_eq!(cv_job(), None);
    }

    #[test]
    fn the_abort_reaches_the_worker_flag() {
        end_cv_job();
        let cancel = flag();
        assert!(start_cv_job(CvJobKind::Warm, Arc::clone(&cancel)));
        assert!(!cancel.load(Ordering::Relaxed));
        abort_cv_job();
        assert!(cancel.load(Ordering::Relaxed), "the worker can see it");
        end_cv_job();
    }

    #[test]
    fn an_abort_with_no_job_is_harmless() {
        end_cv_job();
        abort_cv_job();
        assert!(!cv_job_active());
    }

    #[test]
    fn every_kind_has_a_label() {
        for kind in [CvJobKind::Import, CvJobKind::Sweep, CvJobKind::Warm] {
            assert!(!kind.label().is_empty());
        }
    }
}

#[cfg(test)]
mod list_tree_tests {
    use super::*;
    use cr_core::database::list_items::{
        ComicListItem, FolderItem, IdListItem, LibraryListItem, ListItemBase, SmartListItem,
    };

    fn base(name: &str) -> ListItemBase {
        ListItemBase {
            id: CrGuid::new_random(),
            name: Some(name.to_string()),
            ..Default::default()
        }
    }

    fn list(name: &str) -> ComicListItem {
        ComicListItem::IdList(IdListItem {
            base: base(name),
            ..Default::default()
        })
    }

    fn smart(name: &str) -> ComicListItem {
        ComicListItem::Smart(SmartListItem {
            base: base(name),
            ..Default::default()
        })
    }

    fn folder(name: &str, items: Vec<ComicListItem>) -> ComicListItem {
        ComicListItem::Folder(FolderItem {
            base: base(name),
            items,
            ..Default::default()
        })
    }

    fn library_root() -> ComicListItem {
        ComicListItem::Library(LibraryListItem {
            base: base("Library"),
        })
    }

    /// The names of one container, in order.
    fn names(items: &[ComicListItem]) -> Vec<String> {
        items
            .iter()
            .map(|i| i.base().name.clone().unwrap_or_default())
            .collect()
    }

    fn id_of(items: &[ComicListItem], name: &str) -> CrGuid {
        find_named(items, name).expect("the test item exists")
    }

    fn find_named(items: &[ComicListItem], name: &str) -> Option<CrGuid> {
        for item in items {
            if item.base().name.as_deref() == Some(name) {
                return Some(item.base().id);
            }
            if let ComicListItem::Folder(f) = item {
                if let Some(found) = find_named(&f.items, name) {
                    return Some(found);
                }
            }
        }
        None
    }

    fn folder_items<'a>(items: &'a [ComicListItem], name: &str) -> &'a [ComicListItem] {
        for item in items {
            if let ComicListItem::Folder(f) = item {
                if f.base.name.as_deref() == Some(name) {
                    return &f.items;
                }
                let nested = folder_items(&f.items, name);
                if !nested.is_empty() {
                    return nested;
                }
            }
        }
        &[]
    }

    #[test]
    fn drop_on_a_folder_appends_the_item_to_it() {
        let mut tree = vec![
            library_root(),
            folder("Marvel", vec![list("Avengers")]),
            list("Batman"),
        ];
        let moved = id_of(&tree, "Batman");
        let target = id_of(&tree, "Marvel");
        assert!(apply_list_move(
            &mut tree,
            &moved,
            &ListDrop::IntoFolder(target)
        ));
        assert_eq!(names(&tree), vec!["Library", "Marvel"]);
        assert_eq!(
            names(folder_items(&tree, "Marvel")),
            vec!["Avengers", "Batman"]
        );
    }

    #[test]
    fn drop_before_a_row_reorders_inside_one_container() {
        // The user's ask: free ordering inside a folder.
        let mut tree = vec![folder(
            "Reading",
            vec![list("Alpha"), list("Beta"), list("Gamma")],
        )];
        let moved = id_of(&tree, "Gamma");
        let target = id_of(&tree, "Alpha");
        assert!(apply_list_move(
            &mut tree,
            &moved,
            &ListDrop::BeforeItem(target)
        ));
        assert_eq!(
            names(folder_items(&tree, "Reading")),
            vec!["Gamma", "Alpha", "Beta"]
        );
    }

    #[test]
    fn a_downward_move_lands_before_the_target_row() {
        // The C# decrements its drop index for this case (:1076); the
        // port removes first, so the fresh index already accounts for
        // it.
        let mut tree = vec![list("Alpha"), list("Beta"), list("Gamma")];
        let moved = id_of(&tree, "Alpha");
        let target = id_of(&tree, "Gamma");
        assert!(apply_list_move(
            &mut tree,
            &moved,
            &ListDrop::BeforeItem(target)
        ));
        assert_eq!(names(&tree), vec!["Beta", "Alpha", "Gamma"]);
    }

    #[test]
    fn drop_before_a_row_moves_across_containers() {
        let mut tree = vec![
            folder("Marvel", vec![list("Avengers"), list("X-Men")]),
            list("Batman"),
        ];
        let moved = id_of(&tree, "Batman");
        let target = id_of(&tree, "X-Men");
        assert!(apply_list_move(
            &mut tree,
            &moved,
            &ListDrop::BeforeItem(target)
        ));
        assert_eq!(names(&tree), vec!["Marvel"]);
        assert_eq!(
            names(folder_items(&tree, "Marvel")),
            vec!["Avengers", "Batman", "X-Men"]
        );
    }

    #[test]
    fn a_drop_on_empty_space_moves_to_the_root_end() {
        let mut tree = vec![
            library_root(),
            folder("Marvel", vec![list("Avengers")]),
            list("Batman"),
        ];
        let moved = id_of(&tree, "Avengers");
        assert!(apply_list_move(&mut tree, &moved, &ListDrop::RootEnd));
        assert_eq!(
            names(&tree),
            vec!["Library", "Marvel", "Batman", "Avengers"]
        );
        assert!(folder_items(&tree, "Marvel").is_empty());
    }

    #[test]
    fn a_drop_before_the_library_root_keeps_the_root_first() {
        // The stated deviation: the C# would insert above the root.
        let mut tree = vec![library_root(), list("Batman")];
        let moved = id_of(&tree, "Batman");
        let root = id_of(&tree, "Library");
        assert!(apply_list_move(
            &mut tree,
            &moved,
            &ListDrop::BeforeItem(root)
        ));
        assert_eq!(names(&tree), vec!["Library", "Batman"]);
    }

    #[test]
    fn the_library_root_never_moves() {
        let mut tree = vec![library_root(), folder("Marvel", vec![])];
        let root = id_of(&tree, "Library");
        let target = id_of(&tree, "Marvel");
        assert!(!apply_list_move(
            &mut tree,
            &root,
            &ListDrop::IntoFolder(target)
        ));
        assert_eq!(names(&tree), vec!["Library", "Marvel"]);
    }

    #[test]
    fn a_folder_refuses_a_drop_into_itself_or_its_subtree() {
        let mut tree = vec![folder(
            "Outer",
            vec![folder("Inner", vec![list("Deep")]), list("Alpha")],
        )];
        let outer = id_of(&tree, "Outer");
        let inner = id_of(&tree, "Inner");
        let deep = id_of(&tree, "Deep");
        // Into its own child folder.
        assert!(!apply_list_move(
            &mut tree,
            &outer,
            &ListDrop::IntoFolder(inner)
        ));
        // Before a row inside its own subtree.
        assert!(!apply_list_move(
            &mut tree,
            &outer,
            &ListDrop::BeforeItem(deep)
        ));
        // On itself.
        assert!(!apply_list_move(
            &mut tree,
            &outer,
            &ListDrop::IntoFolder(outer)
        ));
        assert_eq!(names(&tree), vec!["Outer"]);
        assert_eq!(names(folder_items(&tree, "Outer")), vec!["Inner", "Alpha"]);
    }

    #[test]
    fn sort_puts_folders_first_then_names_without_articles() {
        let mut items = vec![
            list("The Zebra"),
            smart("apple"),
            folder("Zulu", vec![]),
            list("Bravo"),
            folder("Alpha", vec![]),
        ];
        sort_list_items(&mut items);
        assert_eq!(
            names(&items),
            vec!["Alpha", "Zulu", "apple", "Bravo", "The Zebra"]
        );
    }

    #[test]
    fn sort_uses_the_zeroes_first_number_rule() {
        let mut items = vec![list("9 - Nine"), list("010 - Ten")];
        sort_list_items(&mut items);
        assert_eq!(names(&items), vec!["010 - Ten", "9 - Nine"]);
    }
}
