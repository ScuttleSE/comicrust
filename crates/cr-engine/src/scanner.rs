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
use cr_core::xml::scalar::{CrDateTime, CrGuid};
use cr_io::formats;

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

/// `ComicBook.Create(file, options)`: fresh defaults + the file path,
/// then the info refresh. Public for the Files view's folder book
/// list (`FolderComicListProvider.GetFolderBookList` — the
/// `AddToTemporary` session books; the library scan builds the same
/// shape).
pub fn create_book(file: &str, now: &CrDateTime) -> ComicBook {
    let mut book = ComicBook {
        id: CrGuid::new_random(),
        file_path: file.to_string(),
        ..Default::default()
    };
    book.added_time = *now;
    refresh_file_info(&mut book);
    book
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

/// `FileUtility.GetFiles`: per folder — files first (sorted), then
/// (when recursing) the subfolders; each file rides `progress` then
/// `f` as it is met (the lazy generator shape).
fn walk_files(folder: &Path, all: bool, progress: &mut dyn FnMut(&Path), f: &mut dyn FnMut(&Path)) {
    let (ignore_folder, ignore_sub_folders) = folder_action(folder);
    if ignore_folder {
        return;
    }
    let mut entries: Vec<_> = match std::fs::read_dir(folder) {
        Ok(rd) => rd.filter_map(|e| e.ok()).map(|e| e.path()).collect(),
        Err(_) => return,
    };
    entries.sort();
    for path in &entries {
        if path.is_file() {
            progress(path);
            f(path);
        }
    }
    if all && !ignore_sub_folders {
        for path in &entries {
            if path.is_dir() {
                walk_files(path, all, progress, f);
            }
        }
    }
}

fn file_name_without_extension(path: &str) -> String {
    cr_core::model::comic_name_info::file_name_without_extension(path)
}

/// One synchronous scan pass over the storage (`ComicScanner.
/// ScanFolderQueue` body). `storage` is the database book list.
/// `progress` fires per walked file BEFORE the scan decision — the
/// C# sets `currentLocation = Path.GetFullPath(scanFile)` per file
/// (ComicScanner.cs:125), which is what the Tasks dialog's Scanning
/// line tracks. The walk interleaves with the processing (the C#
/// `FileUtility.GetFiles` is a lazy generator, FileUtility.cs:63) —
/// a huge tree shows progress during the walk, not only after.
pub fn scan_sync_with_progress(
    storage: &mut Vec<ComicBook>,
    items: &[ScanItem],
    now: &CrDateTime,
    progress: &mut dyn FnMut(&Path),
) -> ScanResult {
    let mut result = ScanResult::default();
    for item in items {
        let root = PathBuf::from(&item.location);
        if root.is_file() {
            progress(&root);
            if let Some(file_str) = root.to_str() {
                process_file(storage, &mut result, file_str, item, now);
            }
        } else if root.is_dir() {
            walk_files(&root, item.all, progress, &mut |file: &Path| {
                if let Some(file_str) = file.to_str() {
                    process_file(storage, &mut result, file_str, item, now);
                }
            });
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

/// [`scan_sync_with_progress`] without the per-file callback.
pub fn scan_sync(storage: &mut Vec<ComicBook>, items: &[ScanItem], now: &CrDateTime) -> ScanResult {
    scan_sync_with_progress(storage, items, now, &mut |_| {})
}

/// The per-file scan decision (`OnProcessScannedFile`): refresh an
/// already-stored book, recover a moved file, or add a new book.
fn process_file(
    storage: &mut Vec<ComicBook>,
    result: &mut ScanResult,
    file_str: &str,
    item: &ScanItem,
    now: &CrDateTime,
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
        scan_sync_with_progress(&mut storage, &items, &now, &mut |f| {
            seen.push(f.to_string_lossy().into_owned())
        });
        // Both walked files (pre-filter — the C# currentLocation
        // tracks the walk, not the decisions).
        assert_eq!(seen.len(), 2, "{seen:?}");
        assert!(seen.iter().any(|p| p.ends_with("a.cbz")));
        assert!(seen.iter().any(|p| p.ends_with("notes.txt")));
    }
}
