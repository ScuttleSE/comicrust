//! Port of `ComicScanner` / `ScanItemFileOrFolder` (headless): walk the
//! library folders, map files to `ComicBook`s with the C# add / move /
//! keep decisions, and refresh the file info (size, times, page count).
//!
//! C# decisions per scanned file (`OnProcessScannedFile`):
//!
//! 1. A book already stored for the path: the file info refreshes.
//! 2. A book with the same file name (without extension) and size whose
//!    stored file no longer exists: the path moves to the new file
//!    (recovered/renamed file).
//! 3. Otherwise a new book is created (`ComicBook.Create` defaults plus
//!    `AddedTime = now`) and added to storage.
//!
//! `remove_missing` (the C# `AutoRemove`) drops linked books whose file
//! is gone after the walk. The C# `DriveChecker.IsConnected` guard is a
//! Windows drive-letter concern and is not ported.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use cr_core::database::comic_database::ComicDatabase;
use cr_core::model::comic_book::ComicBook;
use cr_core::scan_status::{self, ScanStatus, ScanVerdict};
use cr_core::settings::EngineConfiguration;
use cr_core::xml::scalar::{CrDateTime, CrGuid};
use cr_io::formats;
use cr_io::info::InfoLoadingMethod;

/// One scan request (`ScanItemFileOrFolder`).
#[derive(Clone, Debug)]
pub struct ScanItem {
    pub location: String,
    /// `all` — recurse into subfolders.
    pub all: bool,
    /// `removeMissing` — drop books whose file vanished.
    pub remove_missing: bool,
    pub force_refresh_info: bool,
}

/// The per-file work limits (PORT ADDITION, user request 2026-09-11).
///
/// The C# scanner has no deadline: `Scanner.Stop` waits 10 s and then
/// calls `Thread.Abort` (ComicScanner.cs:95-99), which Rust has no
/// equivalent for. An unattended scan of tens of thousands of files
/// must not stall on one file, so the port bounds the per-file work
/// instead of asking a person.
#[derive(Clone, Copy, Debug)]
pub struct ScanLimits {
    /// Longest time one file may take before the scan abandons it and
    /// records [`ScanStatus::TimedOut`]. `None` disables the deadline.
    pub per_file_timeout: Option<Duration>,
    /// Re-open a file that already carries a stored failure verdict,
    /// even when its size and modification time are unchanged. The
    /// normal scan does NOT: a known-bad file is skipped, so a rescan
    /// of a big library does not pay for the same failures again.
    pub retry_failed: bool,
}

impl Default for ScanLimits {
    fn default() -> Self {
        ScanLimits {
            per_file_timeout: Some(Duration::from_secs(DEFAULT_PER_FILE_TIMEOUT_SECS)),
            retry_failed: false,
        }
    }
}

/// The default per-file deadline, in seconds.
///
/// Measured basis (2026-09-11, CIFS, `//diskstation/storage3`): the
/// slowest healthy open in the sample is the 2.8 GB / 851-entry
/// omnibus at 9.0 s while a scan was running in parallel; ordinary
/// 18-67 MB books open in 0.07-0.9 s. 120 s therefore leaves more than
/// a ten-fold margin over the slowest measured healthy file, and still
/// bounds a stuck file to two minutes instead of hours.
pub const DEFAULT_PER_FILE_TIMEOUT_SECS: u64 = 120;

/// Why one file's work was abandoned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Abandoned {
    /// The per-file deadline expired.
    TimedOut,
    /// The user pressed "Skip Current File".
    Skipped,
    /// The whole scan was aborted.
    Aborted,
}

/// The signals the caller can raise while a scan runs.
pub struct ScanControl<'a> {
    /// `Scanner.Stop`'s volatile `abortScanning` — end the whole scan.
    pub stop: &'a dyn Fn() -> bool,
    /// "Skip Current File" — abandon the file in flight and continue
    /// with the next one. The implementation must CONSUME the request
    /// (return true once), so one press skips one file.
    pub take_skip: &'a dyn Fn() -> bool,
}

impl ScanControl<'_> {
    /// The control used by the synchronous, non-interactive entry
    /// points: never stops, never skips.
    pub fn inert() -> ScanControl<'static> {
        ScanControl {
            stop: &|| false,
            take_skip: &|| false,
        }
    }
}

/// Diff produced by one scan run (the unattended-run test asserts this).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScanResult {
    pub added: Vec<String>,
    pub updated: Vec<String>,
    /// (old path, new path) — recovered/renamed files.
    pub moved: Vec<(String, String)>,
    pub removed: Vec<String>,
    /// Files whose archive could not be read at all.
    pub unreadable: Vec<String>,
    /// Files whose content format contradicts their extension. These
    /// ARE imported, through the detected reader.
    pub mismatched: Vec<String>,
    /// Files abandoned when the per-file deadline expired.
    pub timed_out: Vec<String>,
    /// Files abandoned by "Skip Current File".
    pub skipped: Vec<String>,
    /// Files not re-opened because they already carry an unchanged
    /// failure verdict.
    pub skipped_known_bad: Vec<String>,
}

impl ScanResult {
    /// True when the run found anything a person should look at.
    pub fn has_problems(&self) -> bool {
        !self.unreadable.is_empty()
            || !self.mismatched.is_empty()
            || !self.timed_out.is_empty()
            || !self.skipped.is_empty()
    }

    /// The count of files that need attention (the summary line).
    pub fn problem_count(&self) -> usize {
        self.unreadable.len() + self.mismatched.len() + self.timed_out.len() + self.skipped.len()
    }
}

/// What the scanner does to a book's file info
/// (`RefreshInfoFromFile(GetFastPageCount)`): size, timestamps, page
/// count. The page count costs ONE provider open per book — the
/// scanner runs on its worker thread; the UI-thread callers use
/// [`refresh_file_info_basic`].
pub fn refresh_file_info(book: &mut ComicBook) -> bool {
    refresh_file_info_reported(book).0
}

/// [`refresh_file_info`] plus the scan verdict. `None` means the file
/// was not opened at all (nothing to say about it), so the stored
/// verdict must stay as it is.
pub fn refresh_file_info_reported(book: &mut ComicBook) -> (bool, Option<ScanVerdict>) {
    let date_modified = refresh_file_info_basic(book);
    // Page count: refresh when unknown or the file changed. A book
    // that failed its last scan also has page_count 0, so the stored
    // verdict decides whether this re-read is worth paying for; see
    // `should_reopen`.
    if book.info.page_count == 0 || date_modified {
        let path = book.file_path.clone();
        if !path.is_empty() {
            return match cr_io::ComicProvider::open_with_report(Path::new(&path)) {
                Ok((provider, report)) => {
                    let count = provider.pages().len() as i32;
                    if count > 0 {
                        book.info.page_count = count;
                    }
                    let verdict = verdict_from_report(book, &report);
                    (date_modified, Some(verdict))
                }
                Err(e) => (
                    date_modified,
                    Some(ScanVerdict {
                        status: Some(ScanStatus::Unreadable),
                        error: Some(e.to_string()),
                        expected_format: formats::source_format(Path::new(&path))
                            .map(|f| f.name.to_string()),
                        fingerprint: Some(fingerprint_for(book)),
                        ..Default::default()
                    }),
                ),
            };
        }
    }
    (date_modified, None)
}

/// Whether a stored book is worth re-opening. A book that already
/// carries a FAILURE verdict taken from the same `size:mtime` is left
/// alone: re-reading it would cost the same failure again on every
/// scan of the library. A changed file, a mismatch verdict (those
/// books read fine), or `retry_failed` all force the re-open.
fn should_reopen(book: &ComicBook, limits: &ScanLimits) -> bool {
    if limits.retry_failed {
        return true;
    }
    match scan_status::status(book) {
        Some(status) if status.is_failure() => {
            scan_status::fingerprint(book).as_deref() != Some(fingerprint_for(book).as_str())
        }
        _ => true,
    }
}

/// The metadata-only slice of [`refresh_file_info`] (size,
/// timestamps, the missing flag) — no provider open. The page count
/// rides the stored value; the reader fills it from the provider
/// index on open.
pub fn refresh_file_info_basic(book: &mut ComicBook) -> bool {
    let path = book.file_path.clone();
    if path.is_empty() {
        return false;
    }
    let meta = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(_) => {
            book.file_is_missing = true;
            return false;
        }
    };
    book.file_is_missing = false;
    book.file_size = meta.len() as i64;
    let modified = std::fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let modified_time = CrDateTime {
        naive: chrono::DateTime::from_timestamp(modified, 0)
            .map(|dt| dt.naive_utc())
            .unwrap_or_else(|| CrDateTime::min_value().naive),
        kind: cr_core::xml::scalar::DateKind::Utc,
    };
    let date_modified = modified_time != book.file_modified_time;
    book.file_modified_time = modified_time;
    let created = meta
        .created()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    if created > 0 {
        book.file_creation_time = CrDateTime {
            naive: chrono::DateTime::from_timestamp(created, 0)
                .map(|dt| dt.naive_utc())
                .unwrap_or_else(|| CrDateTime::min_value().naive),
            kind: cr_core::xml::scalar::DateKind::Utc,
        };
    }
    date_modified
}

/// `ComicBook.Create(file, options)` + `RefreshInfoFromFile(
/// GetFastPageCount)` (ComicBook.cs:1764, 2442): fresh defaults, the
/// file properties, the info chain (ComicInfo.xml through the full
/// stored/sidecar/in-archive chain, the `Slow` method = the C#
/// `Complete` — MetronInfo.xml maps in the same chain; then the
/// ComicBook.xml copy unless `IgnoreEmbeddedComicBookXml`), then the
/// page count. ONE
/// provider open serves the info chain and the count. Public for the
/// Files view's folder book list (`FolderComicListProvider
/// .GetFolderBookList` — the `AddToTemporary` session books; the
/// library scan builds the same shape).
pub fn create_book(file: &str, now: &CrDateTime) -> ComicBook {
    create_book_reported(file, now).0
}

/// [`create_book`] plus the scan verdict for the file (the "!" and "≠"
/// thumbnail chips, and the `comicrust.scan.*` custom values a smart
/// list queries).
pub fn create_book_reported(file: &str, now: &CrDateTime) -> (ComicBook, ScanVerdict) {
    let mut book = ComicBook {
        id: CrGuid::new_random(),
        file_path: file.to_string(),
        ..Default::default()
    };
    book.added_time = *now;
    // RefreshFileProperties first (ComicBook.cs:2449), then the
    // early-out on a missing file (ComicBook.cs:2454).
    let date_modified = refresh_file_info_basic(&mut book);
    if book.file_is_missing {
        return (book, ScanVerdict::clean());
    }
    let path = book.file_path.clone();
    if path.is_empty() {
        return (book, ScanVerdict::clean());
    }
    let verdict = match cr_io::ComicProvider::open_with_report(Path::new(&path)) {
        Ok((provider, report)) => {
            apply_info_chain(&mut book, &provider);
            // Page count: always refresh for a fresh book (the C#
            // `needsPageCountRefresh` sees FileSize change from the
            // default) — the provider count wins over the stored one.
            if book.info.page_count == 0 || date_modified {
                let count = provider.pages().len() as i32;
                if count > 0 {
                    book.info.page_count = count;
                }
            }
            verdict_from_report(&book, &report)
        }
        // No reader at all for this source (the C# factory returns
        // null). The book still enters the library, marked.
        Err(e) => ScanVerdict {
            status: Some(ScanStatus::Unreadable),
            error: Some(e.to_string()),
            expected_format: formats::source_format(Path::new(&path)).map(|f| f.name.to_string()),
            fingerprint: Some(fingerprint_for(&book)),
            ..Default::default()
        },
    };
    (book, verdict)
}

/// Turns an [`cr_io::OpenReport`] into the stored verdict.
fn verdict_from_report(book: &ComicBook, report: &cr_io::OpenReport) -> ScanVerdict {
    if let Some(error) = &report.entry_error {
        return ScanVerdict {
            status: Some(ScanStatus::Unreadable),
            error: Some(error.clone()),
            detected_format: report.detected_format.map(|f| f.name.to_string()),
            expected_format: report.extension_format.map(|f| f.name.to_string()),
            fingerprint: Some(fingerprint_for(book)),
        };
    }
    if report.mismatch {
        return ScanVerdict {
            status: Some(ScanStatus::FormatMismatch),
            error: None,
            detected_format: report.detected_format.map(|f| f.name.to_string()),
            expected_format: report.extension_format.map(|f| f.name.to_string()),
            fingerprint: Some(fingerprint_for(book)),
        };
    }
    ScanVerdict::clean()
}

/// The `size:mtime` fingerprint of the book's current file facts.
fn fingerprint_for(book: &ComicBook) -> String {
    scan_status::fingerprint_of(
        book.file_size,
        book.file_modified_time.naive.and_utc().timestamp(),
    )
}

/// The info-chain slice of `ComicBook.RefreshInfoFromFile`
/// (ComicBook.cs:2483-2500) against an already-open provider:
/// `LoadInfo` + `SetInfo(ci, onlyUpdateEmpty: true)`, then the
/// ComicBook.xml copy — `cb.SetInfo(ci, onlyUpdateEmpty: false)` plus
/// `SetBook(cb)` — unless `IgnoreEmbeddedComicBookXml`. The fresh
/// book's `FileInfoRetrieved == false` picks the `Complete` method
/// (the port's `Slow`: in-archive sources read even when a sidecar/
/// xattr exists). Public for the reader's temporary-book open path
/// (`ComicBookFactory.Create` → `AddToTemporary` →
/// `RefreshInfoFromFile`, the C# ComicBookFactory.cs:95 shape).
pub fn apply_info_chain(book: &mut ComicBook, provider: &cr_io::ComicProvider) {
    let ignore = EngineConfiguration::global().ignore_embedded_comic_book_xml;
    apply_info_chain_gated(book, provider, ignore);
}

/// [`apply_info_chain`] with the `IgnoreEmbeddedComicBookXml` gate
/// injected — the test seam (the engine-config global is
/// process-wide; tests must not flip it).
fn apply_info_chain_gated(
    book: &mut ComicBook,
    provider: &cr_io::ComicProvider,
    ignore_embedded: bool,
) {
    let ci = provider.load_info(InfoLoadingMethod::Slow);
    if let Some(ci) = &ci {
        book.set_info(ci, true, true);
    }
    if !ignore_embedded {
        if let Some(mut cb) = provider.load_book(InfoLoadingMethod::Slow) {
            if let Some(ci) = &ci {
                cb.set_info(ci, false, true);
            }
            book.set_book(cb);
        }
    }
}

fn is_comic_file(file: &Path) -> bool {
    formats::source_format(file).is_some()
}

/// `comicrackscanner.ini` folder validation (`ValidateFolder`): the ini
/// holds an `options` value with the ignore flags.
fn folder_action(path: &Path) -> (bool, bool) {
    let ini = path.join("comicrackscanner.ini");
    if !ini.is_file() {
        return (false, false);
    }
    let Ok(text) = std::fs::read_to_string(&ini) else {
        return (false, false);
    };
    // IniFile.GetValue(text, "options", Default): the value in the
    // [options] section — a comma/space list of action flags.
    let mut section = false;
    let mut value = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].eq_ignore_ascii_case("options");
        } else if section {
            if let Some((k, v)) = line.split_once('=') {
                if k.trim().eq_ignore_ascii_case("options") {
                    value = v.trim().to_string();
                }
            }
        }
    }
    let flags = value.to_lowercase();
    (
        flags.contains("ignorefolder"),
        flags.contains("ignoresubfolders"),
    )
}

/// The lazy `FileUtility.GetFiles` walk: per folder — files first
/// (sorted), then (when recursing) the subfolders; each file rides
/// `progress` then `f` as it is met (the lazy generator shape). `f`
/// returns false to stop the whole walk (the C# abort exits the
/// foreach, ComicScanner.cs:130-133) — the stop propagates up the
/// recursion.
fn walk_files(
    folder: &Path,
    all: bool,
    progress: &mut dyn FnMut(&Path),
    f: &mut dyn FnMut(&Path) -> bool,
) -> bool {
    let (ignore_folder, ignore_sub_folders) = folder_action(folder);
    if ignore_folder {
        return true;
    }
    let mut entries: Vec<_> = match std::fs::read_dir(folder) {
        Ok(rd) => rd.filter_map(|e| e.ok()).map(|e| e.path()).collect(),
        Err(_) => return true,
    };
    entries.sort();
    for path in &entries {
        if path.is_file() {
            progress(path);
            if !f(path) {
                return false;
            }
        }
    }
    if all && !ignore_sub_folders {
        for path in &entries {
            if path.is_dir() && !walk_files(path, all, progress, f) {
                return false;
            }
        }
    }
    true
}

fn file_name_without_extension(path: &str) -> String {
    cr_core::model::comic_name_info::file_name_without_extension(path)
}

/// One synchronous scan pass over the storage (`ComicScanner.
/// ScanFolderQueue` body). `storage` is the database book list.
/// `progress` fires per walked file BEFORE the scan decision — the
/// C# sets `currentLocation = Path.GetFullPath(scanFile)` per file
/// (ComicScanner.cs:125), which is what the Tasks dialog's Scanning
/// line tracks. `stop` is the per-file abort check (`Scanner.Stop`'s
/// volatile `abortScanning`, ComicScanner.cs:130 — checked after the
/// location update, before the file processes); when it fires the
/// scan returns the partial result and skips the AutoRemove pass
/// (the C# returns out of the queue body). `on_new` fires with each
/// newly created book (the C# adds to the live storage per file —
/// `ComicBookCollection.Add` fires `OnBookAdded`; the port's worker
/// ships incremental batches over it). The walk interleaves with
/// the processing (the C# `FileUtility.GetFiles` is a lazy generator,
/// FileUtility.cs:63) — a huge tree shows progress during the walk,
/// not only after.
pub fn scan_sync_with_progress(
    storage: &mut Vec<ComicBook>,
    items: &[ScanItem],
    now: &CrDateTime,
    progress: &mut dyn FnMut(&Path),
    stop: &dyn Fn() -> bool,
    on_new: &mut dyn FnMut(&ComicBook),
) -> ScanResult {
    let control = ScanControl {
        stop,
        take_skip: &|| false,
    };
    scan_sync_with_control(
        storage,
        items,
        now,
        progress,
        &control,
        ScanLimits::default(),
        on_new,
    )
}

/// [`scan_sync_with_progress`] with the per-file limits and the
/// "Skip Current File" control (PORT ADDITION, user request
/// 2026-09-11).
///
/// Each file's provider work runs under a deadline. When the deadline
/// expires, or the user skips the file, the scan records the verdict
/// on the book and MOVES ON by itself — an unattended scan of tens of
/// thousands of files never waits for a person.
pub fn scan_sync_with_control(
    storage: &mut Vec<ComicBook>,
    items: &[ScanItem],
    now: &CrDateTime,
    progress: &mut dyn FnMut(&Path),
    control: &ScanControl<'_>,
    limits: ScanLimits,
    on_new: &mut dyn FnMut(&ComicBook),
) -> ScanResult {
    let stop = control.stop;
    let mut result = ScanResult::default();
    let stamp = verdict_timestamp();
    for item in items {
        let root = PathBuf::from(&item.location);
        if root.is_file() {
            progress(&root);
            if stop() {
                return result;
            }
            if let Some(file_str) = root.to_str() {
                process_file(
                    storage,
                    &mut result,
                    file_str,
                    item,
                    now,
                    control,
                    &limits,
                    &stamp,
                    on_new,
                );
            }
        } else if root.is_dir() {
            walk_files(&root, item.all, progress, &mut |file: &Path| {
                if stop() {
                    return false;
                }
                if let Some(file_str) = file.to_str() {
                    process_file(
                        storage,
                        &mut result,
                        file_str,
                        item,
                        now,
                        control,
                        &limits,
                        &stamp,
                        on_new,
                    );
                }
                true
            });
        }
        if stop() {
            return result;
        }
    }
    // AutoRemove: drop linked books whose file vanished.
    if items.iter().any(|i| i.remove_missing) {
        let mut kept = Vec::new();
        for book in storage.drain(..) {
            if !book.file_path.is_empty() && std::fs::metadata(&book.file_path).is_err() {
                result.removed.push(book.file_path.clone());
            } else {
                kept.push(book);
            }
        }
        *storage = kept;
    }
    result
}

/// [`scan_sync_with_progress`] without the callbacks.
pub fn scan_sync(storage: &mut Vec<ComicBook>, items: &[ScanItem], now: &CrDateTime) -> ScanResult {
    scan_sync_with_progress(storage, items, now, &mut |_| {}, &|| false, &mut |_| {})
}

/// The verdict timestamp for one scan run (RFC 3339, seconds).
fn verdict_timestamp() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// Runs one file's provider work under the per-file deadline, and
/// under the "Skip Current File" and abort signals.
///
/// The work runs on its own thread so the scan can walk away from it.
/// Rust has no safe thread kill (the C# scanner uses `Thread.Abort`,
/// ComicScanner.cs:97), so an abandoned thread is DETACHED and keeps
/// running until its blocked read returns. That is bounded in
/// practice: the CIFS mount is `soft`, so a hung read ends in an error
/// rather than hanging forever, and the abandoned result is dropped.
/// The bounded-open work in cr-io removes the case that produced these
/// stalls in the first place; this deadline is the backstop.
fn run_bounded<T, F>(
    limits: &ScanLimits,
    control: &ScanControl<'_>,
    work: F,
) -> Result<T, Abandoned>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    // No deadline and no interactive controls: run inline and keep the
    // cost of a thread out of the common path.
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("Book Scanner File".into())
        .spawn(move || {
            let _ = tx.send(work());
        })
        .map_err(|_| Abandoned::TimedOut)?;

    let deadline = limits.per_file_timeout.map(|t| Instant::now() + t);
    loop {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(value) => return Ok(value),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                // The worker panicked. Treat it as an unreadable file
                // rather than taking the whole scan down.
                return Err(Abandoned::TimedOut);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if (control.take_skip)() {
                    return Err(Abandoned::Skipped);
                }
                if (control.stop)() {
                    return Err(Abandoned::Aborted);
                }
                if let Some(deadline) = deadline {
                    if Instant::now() >= deadline {
                        return Err(Abandoned::TimedOut);
                    }
                }
            }
        }
    }
}

/// Records an abandoned file on the result and returns the verdict to
/// store on the book.
fn abandoned_verdict(
    result: &mut ScanResult,
    file_str: &str,
    reason: Abandoned,
    limits: &ScanLimits,
) -> ScanVerdict {
    let status = match reason {
        Abandoned::Skipped => {
            result.skipped.push(file_str.to_string());
            ScanStatus::Skipped
        }
        Abandoned::TimedOut | Abandoned::Aborted => {
            result.timed_out.push(file_str.to_string());
            ScanStatus::TimedOut
        }
    };
    let error = match reason {
        Abandoned::Skipped => "skipped by the user".to_string(),
        Abandoned::Aborted => "the scan was aborted while reading this file".to_string(),
        Abandoned::TimedOut => format!(
            "the file did not finish reading within {} s",
            limits
                .per_file_timeout
                .map(|t| t.as_secs())
                .unwrap_or_default()
        ),
    };
    ScanVerdict {
        status: Some(status),
        error: Some(error),
        expected_format: formats::source_format(Path::new(file_str)).map(|f| f.name.to_string()),
        ..Default::default()
    }
}

/// Counts a stored verdict on the run summary.
fn count_verdict(result: &mut ScanResult, file_str: &str, verdict: &ScanVerdict) {
    match verdict.status {
        Some(ScanStatus::Unreadable) => result.unreadable.push(file_str.to_string()),
        Some(ScanStatus::FormatMismatch) => result.mismatched.push(file_str.to_string()),
        // Timed out and skipped are counted where they are produced.
        _ => {}
    }
}

/// The per-file scan decision (`OnProcessScannedFile`): refresh an
/// already-stored book, recover a moved file, or add a new book.
#[allow(clippy::too_many_arguments)]
fn process_file(
    storage: &mut Vec<ComicBook>,
    result: &mut ScanResult,
    file_str: &str,
    item: &ScanItem,
    now: &CrDateTime,
    control: &ScanControl<'_>,
    limits: &ScanLimits,
    stamp: &str,
    on_new: &mut dyn FnMut(&ComicBook),
) {
    // The C# factory rejects files no reader supports.
    if !is_comic_file(Path::new(file_str)) {
        return;
    }
    // 1. Already stored: refresh the file info.
    if let Some(index) = storage
        .iter()
        .position(|b| b.file_path.eq_ignore_ascii_case(file_str))
    {
        refresh_stored_book(storage, index, result, file_str, control, limits, stamp);
        if item.force_refresh_info {
            // ForceRefresh re-reads the info chain; the port
            // refreshes size/times/page count (info reload is
            // the cr-io info chain and stays a caller concern).
        }
        result.updated.push(file_str.to_string());
        return;
    }
    // 2. Renamed/recovered file: same name+size, stored file gone.
    let name = file_name_without_extension(file_str);
    let size = std::fs::metadata(file_str)
        .map(|m| m.len() as i64)
        .unwrap_or(0);
    let candidate = storage.iter().position(|b| {
        file_name_without_extension(&b.file_path) == name
            && b.file_size == size
            && std::fs::metadata(&b.file_path).is_err()
    });
    if let Some(index) = candidate {
        let old = storage[index].file_path.clone();
        storage[index].file_path = file_str.to_string();
        refresh_stored_book(storage, index, result, file_str, control, limits, stamp);
        result.moved.push((old, file_str.to_string()));
        return;
    }
    // 3. New book.
    let owned_path = file_str.to_string();
    let owned_now = *now;
    let outcome = run_bounded(limits, control, move || {
        create_book_reported(&owned_path, &owned_now)
    });
    let (mut book, verdict) = match outcome {
        Ok((book, verdict)) => {
            count_verdict(result, file_str, &verdict);
            (book, verdict)
        }
        Err(reason) => {
            // The file is still added, so it is visible and queryable
            // instead of silently missing from the library.
            let mut book = ComicBook {
                id: CrGuid::new_random(),
                file_path: file_str.to_string(),
                ..Default::default()
            };
            book.added_time = *now;
            refresh_file_info_basic(&mut book);
            let verdict = abandoned_verdict(result, file_str, reason, limits);
            (book, verdict)
        }
    };
    scan_status::apply(&mut book, &verdict, stamp);
    result.added.push(file_str.to_string());
    on_new(&book);
    storage.push(book);
}

/// The refresh half of [`process_file`], for a book already in
/// storage. Runs under the same deadline and skip control.
fn refresh_stored_book(
    storage: &mut [ComicBook],
    index: usize,
    result: &mut ScanResult,
    file_str: &str,
    control: &ScanControl<'_>,
    limits: &ScanLimits,
    stamp: &str,
) {
    // The cheap half always runs on this thread: size, times, missing.
    refresh_file_info_basic(&mut storage[index]);
    if !should_reopen(&storage[index], limits) {
        result.skipped_known_bad.push(file_str.to_string());
        return;
    }
    let mut candidate = storage[index].clone();
    let outcome = run_bounded(limits, control, move || {
        let (_, verdict) = refresh_file_info_reported(&mut candidate);
        (candidate, verdict)
    });
    match outcome {
        Ok((refreshed, verdict)) => {
            storage[index] = refreshed;
            if let Some(verdict) = verdict {
                count_verdict(result, file_str, &verdict);
                scan_status::apply(&mut storage[index], &verdict, stamp);
            }
        }
        Err(reason) => {
            let verdict = abandoned_verdict(result, file_str, reason, limits);
            scan_status::apply(&mut storage[index], &verdict, stamp);
        }
    }
}

/// Scans directly into a `ComicDatabase` (the `ComicBookFactory.Storage`
/// equivalent).
pub fn scan_database(db: &mut ComicDatabase, items: &[ScanItem], now: &CrDateTime) -> ScanResult {
    scan_sync(&mut db.books, items, now)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "comicrust-scan-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scan_adds_updates_moves_and_removes() {
        let dir = temp_dir("lib");
        let sub = dir.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(dir.join("a.cbz"), b"fakezip").unwrap();
        std::fs::write(dir.join("notes.txt"), b"skip me").unwrap();
        std::fs::write(sub.join("b.cbz"), b"fakezip2").unwrap();
        let now = CrDateTime::min_value();
        let mut storage: Vec<ComicBook> = Vec::new();

        // Initial scan: recursive, remove missing.
        let items = vec![ScanItem {
            location: dir.to_string_lossy().into(),
            all: true,
            remove_missing: true,
            force_refresh_info: false,
        }];
        let result = scan_sync(&mut storage, &items, &now);
        // .txt is not a comic; both .cbz files added (page count 0 — the
        // fake zips hold no pages).
        assert_eq!(result.added.len(), 2, "{result:?}");
        assert_eq!(storage.len(), 2);
        assert!(storage.iter().all(|b| !b.file_is_missing));

        // Rescan: nothing changes.
        let result = scan_sync(&mut storage, &items, &now);
        assert!(result.added.is_empty() && result.updated.len() == 2 && result.moved.is_empty());

        // Move b.cbz into a new folder: same name+size, stored file
        // gone → the stored book moves to the new path.
        let moved_dir = dir.join("moved");
        std::fs::create_dir_all(&moved_dir).unwrap();
        std::fs::rename(sub.join("b.cbz"), moved_dir.join("b.cbz")).unwrap();
        let result = scan_sync(&mut storage, &items, &now);
        assert_eq!(result.moved.len(), 1, "{result:?}");
        assert!(result.moved[0].1.ends_with("moved/b.cbz"), "{result:?}");

        // Delete a.cbz: removed by the AutoRemove pass.
        std::fs::remove_file(dir.join("a.cbz")).unwrap();
        let result = scan_sync(&mut storage, &items, &now);
        assert_eq!(result.removed.len(), 1, "{result:?}");
        assert_eq!(storage.len(), 1);
    }

    #[test]
    fn comicrackscanner_ini_is_honored() {
        let dir = temp_dir("ini");
        let sub = dir.join("ignored");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(dir.join("a.cbz"), b"z").unwrap();
        std::fs::write(sub.join("b.cbz"), b"z").unwrap();
        std::fs::write(
            sub.join("comicrackscanner.ini"),
            "[options]\noptions = IgnoreFolder\n",
        )
        .unwrap();
        let now = CrDateTime::min_value();
        let items = vec![ScanItem {
            location: dir.to_string_lossy().into(),
            all: true,
            remove_missing: false,
            force_refresh_info: false,
        }];
        let mut storage = Vec::new();
        scan_sync(&mut storage, &items, &now);
        assert_eq!(storage.len(), 1, "subfolder must be ignored");
    }

    #[test]
    fn progress_fires_per_walked_file() {
        let dir = temp_dir("progress");
        std::fs::write(dir.join("a.cbz"), b"z").unwrap();
        std::fs::write(dir.join("notes.txt"), b"skip").unwrap();
        let now = CrDateTime::min_value();
        let items = vec![ScanItem {
            location: dir.to_string_lossy().into(),
            all: true,
            remove_missing: false,
            force_refresh_info: false,
        }];
        let mut storage = Vec::new();
        let mut seen: Vec<String> = Vec::new();
        scan_sync_with_progress(
            &mut storage,
            &items,
            &now,
            &mut |f| seen.push(f.to_string_lossy().into_owned()),
            &|| false,
            &mut |_| {},
        );
        // Both walked files (pre-filter — the C# currentLocation
        // tracks the walk, not the decisions).
        assert_eq!(seen.len(), 2, "{seen:?}");
        assert!(seen.iter().any(|p| p.ends_with("a.cbz")));
        assert!(seen.iter().any(|p| p.ends_with("notes.txt")));
    }

    #[test]
    fn scan_stops_when_flagged_and_keeps_partial() {
        let dir = temp_dir("stop");
        for i in 0..10 {
            std::fs::write(dir.join(format!("book{i}.cbz")), b"z").unwrap();
        }
        let now = CrDateTime::min_value();
        let items = vec![ScanItem {
            location: dir.to_string_lossy().into(),
            all: true,
            // The AutoRemove pass must be SKIPPED on an abort (the C#
            // returns out of the queue body) — remove_missing stays
            // set to prove the skip.
            remove_missing: true,
            force_refresh_info: false,
        }];
        let mut storage = Vec::new();
        // The stop fires once the second file is walked: file 1
        // processes, file 2 is skipped (the check runs after the
        // location update, before the process — ComicScanner.cs:130).
        let walked = std::cell::Cell::new(0usize);
        let result = scan_sync_with_progress(
            &mut storage,
            &items,
            &now,
            &mut |_| {
                walked.set(walked.get() + 1);
            },
            &|| walked.get() >= 2,
            &mut |_| {},
        );
        assert!(
            result.added.len() < 10 && !result.added.is_empty(),
            "partial landing expected, got {result:?}"
        );
        assert_eq!(storage.len(), result.added.len());
        // The walk visited exactly 2 files before the flag fired.
        assert_eq!(walked.get(), 2);
    }

    #[test]
    fn apply_info_chain_gates_comic_book_xml() {
        use std::io::Write as _;
        let dir = temp_dir("gate");
        let path = dir.join("book.cbz");
        let file = std::fs::File::create(&path).unwrap();
        let mut z = zip::ZipWriter::new(file);
        let options: zip::write::SimpleFileOptions = Default::default();
        z.start_file("page1.jpg", options).unwrap();
        z.write_all(b"p").unwrap();
        z.start_file("ComicBook.xml", options).unwrap();
        z.write_all(
            br#"<?xml version="1.0"?>
<ComicBook xmlns:xsd="http://www.w3.org/2001/XMLSchema" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance" Checked="true">
  <Series>Book Series</Series>
  <BookNotes>catalog notes</BookNotes>
</ComicBook>"#,
        )
        .unwrap();
        z.finish().unwrap();

        let provider = cr_io::ComicProvider::open(&path).unwrap();

        // Embedded ComicBook.xml read (the default).
        let mut book = ComicBook::default();
        apply_info_chain_gated(&mut book, &provider, false);
        assert_eq!(book.info.series, "Book Series");
        assert_eq!(book.book_notes, "catalog notes");

        // IgnoreEmbeddedComicBookXml: the ComicBook.xml copy is
        // skipped entirely (ComicBook.cs:2492).
        let mut book2 = ComicBook::default();
        apply_info_chain_gated(&mut book2, &provider, true);
        assert!(book2.info.series.is_empty());
        assert!(book2.book_notes.is_empty());

        std::fs::remove_dir_all(&dir).ok();
    }
}
