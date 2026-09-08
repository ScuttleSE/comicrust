//! The library session — the `DatabaseManager` + `ComicScanner` +
//! `QueueManager` lifecycle: open the database (with the
//! `.restore`/`.bak` fallback chain), save on exit, dirty tracking,
//! file scans, and watch-folder events.
//!
//! C# reference: `ComicRack.Engine/DatabaseManager.cs`,
//! `ComicRack.Engine/ComicScanner.cs` (the C# scan runs on a
//! low-priority worker thread; this port runs one scan synchronously —
//! a 255-book scan is fast and the caller can defer to a thread),
//! and `ComicRack.Engine/QueueManager.cs` (`StartScan`).

use std::path::{Path, PathBuf};

use cr_core::database::comic_database::{
    open_with_fallback, save, ComicDatabase, DbError, OpenStatus,
};
use cr_core::xml::scalar::CrDateTime;

use crate::queue_manager::QueueManager;
use crate::scanner::{scan_database, ScanItem, ScanResult};
use crate::watch::Watcher;

/// `DatabaseBackgroundSaving` default (the C# `ExtendedSettings`).
pub const BACKGROUND_SAVE_INTERVAL_SECS: u64 = 600;

pub struct Library {
    database: ComicDatabase,
    /// The full path to `ComicDb.xml` (the C# `DatabaseFile`, which is
    /// the extension-less `ComicDb` path + `.xml` on save; the C#
    /// `DatabaseManager.Save` accepts an extension-less path and
    /// appends `.xml` — callers here always pass the full file).
    file: PathBuf,
    /// The C# `ComicDatabase.IsDirty` (books mark the container dirty
    /// through events; callers here mark it explicitly).
    dirty: bool,
    /// The ComicBook queues (`Program.QueueManager`).
    queues: QueueManager,
    /// Live watch over `database.watch_folders` (`Watch = true`).
    watcher: Option<Watcher>,
}

impl Library {
    /// `DatabaseManager.Open` — opens the database through the full
    /// fallback chain and builds the live watcher from the stored
    /// watch folders (a watcher failure is silent, like the C#
    /// `FileSystemWatcher`).
    pub fn open(file: &Path) -> Result<(Library, OpenStatus), DbError> {
        let (database, status) = open_with_fallback(file)?;
        let mut lib = Library {
            database,
            file: file.to_path_buf(),
            dirty: false,
            queues: QueueManager::new(),
            watcher: None,
        };
        lib.rebuild_watcher();
        Ok((lib, status))
    }

    /// Opens at the default location (`Paths::new_default` +
    /// `ComicDb/ComicDb.xml`).
    pub fn open_at_default_location() -> Result<(Library, OpenStatus), DbError> {
        let paths = cr_core::paths::Paths::new_default();
        Library::open(&cr_core::paths::database_file(&paths))
    }

    pub fn database(&self) -> &ComicDatabase {
        &self.database
    }

    /// Mutating access — the caller must [`Library::mark_dirty`] for
    /// changes that should reach a save (the C# marks the container
    /// dirty through book events).
    pub fn database_mut(&mut self) -> &mut ComicDatabase {
        &mut self.database
    }

    pub fn file(&self) -> &Path {
        &self.file
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// The ComicBook queues (dynamic update / export / read-info /
    /// write-info) — `Program.QueueManager`.
    pub fn queues(&self) -> &QueueManager {
        &self.queues
    }

    /// `DatabaseManager.Save`: saves unconditionally through the
    /// `.bak`-rotation writer and clears the dirty flag. (The C#
    /// swallows save errors — callers here decide.)
    pub fn save(&mut self) -> Result<(), DbError> {
        save(&self.database, &self.file)?;
        self.dirty = false;
        Ok(())
    }

    /// `DatabaseManager.SaveInBackground`: saves only when dirty.
    /// Returns whether a save ran.
    pub fn save_if_dirty(&mut self) -> Result<bool, DbError> {
        if !self.dirty {
            return Ok(false);
        }
        self.save()?;
        Ok(true)
    }

    /// `ComicBookFactory.FindItemByFile` — case-insensitive path
    /// lookup (the Windows path comparison is a load-bearing C#
    /// behavior; the scanner matches the same way).
    pub fn find_book(&self, path: &str) -> Option<&cr_core::model::comic_book::ComicBook> {
        self.database
            .books
            .iter()
            .find(|b| b.file_path.eq_ignore_ascii_case(path))
    }

    pub fn find_book_mut(
        &mut self,
        path: &str,
    ) -> Option<&mut cr_core::model::comic_book::ComicBook> {
        self.database
            .books
            .iter_mut()
            .find(|b| b.file_path.eq_ignore_ascii_case(path))
    }

    /// `ComicScanner.ScanFileOrFolder` — scans `location` (recursing
    /// when `all`) into the database. One synchronous run (the C#
    /// queues scan items onto a low-priority worker thread; the
    /// caller can thread if it ever matters). The result changes mark
    /// the library dirty.
    pub fn scan_file_or_folder(
        &mut self,
        location: &str,
        all: bool,
        remove_missing: bool,
    ) -> ScanResult {
        let items = [ScanItem {
            location: location.to_string(),
            all,
            remove_missing,
            force_refresh_info: false,
        }];
        self.scan_items(&items)
    }

    /// `QueueManager.StartScan(all, remove_missing)` — rescans every
    /// stored watch folder.
    pub fn scan_watch_folders(&mut self, all: bool, remove_missing: bool) -> ScanResult {
        let folders: Vec<String> = self
            .database
            .watch_folders
            .iter()
            .map(|wf| wf.folder.clone())
            .collect();
        let items: Vec<ScanItem> = folders
            .into_iter()
            .map(|folder| ScanItem {
                location: folder,
                all,
                remove_missing,
                force_refresh_info: false,
            })
            .collect();
        self.scan_items(&items)
    }

    /// The debounced file events from the live watcher (each entry is
    /// a changed file/folder path). Watch-triggered rescans map them
    /// back to the watch roots — see
    /// [`Library::take_watch_folder_rescans`].
    pub fn take_watch_events(&mut self) -> Vec<PathBuf> {
        self.watcher
            .as_mut()
            .map(Watcher::take_pending)
            .unwrap_or_default()
    }

    /// Takes the pending watch events and returns the watch roots
    /// they belong to (in stored order; each stored root appears at
    /// most once).
    pub fn take_watch_folder_rescans(&mut self) -> Vec<String> {
        let events = self.take_watch_events();
        if events.is_empty() {
            return Vec::new();
        }
        self.database
            .watch_folders
            .iter()
            .filter(|wf| events.iter().any(|ev| ev.starts_with(&wf.folder)))
            .map(|wf| wf.folder.clone())
            .collect()
    }

    /// Adds a watch folder to the database (`watch` mirrors the C#
    /// `WatchFolder.Watch` flag) and rebuilds the live watcher.
    pub fn add_watch_folder(&mut self, folder: &str, watch: bool) {
        if self
            .database
            .watch_folders
            .iter()
            .any(|wf| wf.folder == folder)
        {
            return;
        }
        self.database
            .watch_folders
            .push(cr_core::database::list_items::WatchFolder {
                folder: folder.to_string(),
                watch,
            });
        self.dirty = true;
        self.rebuild_watcher();
    }

    fn rebuild_watcher(&mut self) {
        self.watcher = Watcher::new(&self.database.watch_folders).ok();
    }

    /// The collapsed Windows-path roots over the three families (the
    /// migration dialog rows; see [`crate::path_migration`]).
    pub fn windows_path_roots(&self) -> Vec<crate::path_migration::PathRoot> {
        crate::path_migration::collect_roots(
            &self.database.books,
            &self.database.watch_folders,
            &self.database.black_list,
        )
    }

    /// Any Windows-style path left in the database?
    pub fn has_windows_paths(&self) -> bool {
        crate::path_migration::has_windows_paths(&self.database)
    }

    /// Applies the user-decided root → target mappings: books,
    /// watch folders, and blacklist rewrite in place, the library
    /// marks dirty, and the watcher rebuilds (the mapped watch
    /// folders are live paths now). The view refresh is the
    /// caller's job.
    pub fn apply_path_migration(
        &mut self,
        mappings: &[crate::path_migration::Mapping],
    ) -> crate::path_migration::ApplyReport {
        let report = crate::path_migration::apply(&mut self.database, mappings);
        if report.changed_anything() {
            self.dirty = true;
            self.rebuild_watcher();
        }
        report
    }

    fn scan_items(&mut self, items: &[ScanItem]) -> ScanResult {
        let now = CrDateTime::now();
        let result = scan_database(&mut self.database, items, &now);
        let changed = !result.added.is_empty()
            || !result.updated.is_empty()
            || !result.moved.is_empty()
            || !result.removed.is_empty();
        if changed {
            self.dirty = true;
        }
        result
    }
}
