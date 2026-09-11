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

use cr_core::database::comic_database::ComicDatabase;
use cr_core::model::comic_book::ComicBook;
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

/// Diff produced by one scan run (the unattended-run test asserts this).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScanResult {
    pub added: Vec<String>,
    pub updated: Vec<String>,
    /// (old path, new path) — recovered/renamed files.
    pub moved: Vec<(String, String)>,
    pub removed: Vec<String>,
}

/// What the scanner does to a book's file info
/// (`RefreshInfoFromFile(GetFastPageCount)`): size, timestamps, page
/// count. The page count costs ONE provider open per book — the
/// scanner runs on its worker thread; the UI-thread callers use
/// [`refresh_file_info_basic`].
pub fn refresh_file_info(book: &mut ComicBook) -> bool {
    let date_modified = refresh_file_info_basic(book);
    // Page count: refresh when unknown or the file changed.
    if book.info.page_count == 0 || date_modified {
        let path = book.file_path.clone();
        if !path.is_empty() {
            if let Ok(provider) = cr_io::ComicProvider::open(Path::new(&path)) {
                let count = provider.pages().len() as i32;
                if count > 0 {
                    book.info.page_count = count;
                }
            }
        }
    }
    date_modified
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
        return book;
    }
    let path = book.file_path.clone();
    if path.is_empty() {
        return book;
    }
    if let Ok(provider) = cr_io::ComicProvider::open(Path::new(&path)) {
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
    }
    book
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
    let mut result = ScanResult::default();
    for item in items {
        let root = PathBuf::from(&item.location);
        if root.is_file() {
            progress(&root);
            if stop() {
                return result;
            }
            if let Some(file_str) = root.to_str() {
                process_file(storage, &mut result, file_str, item, now, on_new);
            }
        } else if root.is_dir() {
            walk_files(&root, item.all, progress, &mut |file: &Path| {
                if stop() {
                    return false;
                }
                if let Some(file_str) = file.to_str() {
                    process_file(storage, &mut result, file_str, item, now, on_new);
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

/// The per-file scan decision (`OnProcessScannedFile`): refresh an
/// already-stored book, recover a moved file, or add a new book.
fn process_file(
    storage: &mut Vec<ComicBook>,
    result: &mut ScanResult,
    file_str: &str,
    item: &ScanItem,
    now: &CrDateTime,
    on_new: &mut dyn FnMut(&ComicBook),
) {
    // The C# factory rejects files no reader supports.
    if !is_comic_file(Path::new(file_str)) {
        return;
    }
    // 1. Already stored: refresh the file info.
    if let Some(book) = storage
        .iter_mut()
        .find(|b| b.file_path.eq_ignore_ascii_case(file_str))
    {
        refresh_file_info(book);
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
    let candidate = storage.iter_mut().find(|b| {
        file_name_without_extension(&b.file_path) == name
            && b.file_size == size
            && !std::fs::metadata(&b.file_path).is_ok()
    });
    if let Some(book) = candidate {
        let old = book.file_path.clone();
        book.file_path = file_str.to_string();
        refresh_file_info(book);
        result.moved.push((old, file_str.to_string()));
        return;
    }
    // 3. New book.
    let book = create_book(file_str, now);
    result.added.push(file_str.to_string());
    on_new(&book);
    storage.push(book);
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
