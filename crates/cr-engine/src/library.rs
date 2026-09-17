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
use std::sync::mpsc::{self, Receiver};

use cr_core::database::comic_database::{
    open_with_fallback, save, ComicDatabase, DbError, OpenStatus,
};
use cr_core::xml::scalar::CrDateTime;

use crate::queue_manager::QueueManager;
use crate::scanner::{scan_database, ScanItem, ScanResult};
use crate::watch::{Watcher, DEFAULT_DEBOUNCE};

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
    mutation_generation: u64,
    /// The ComicBook queues (`Program.QueueManager`).
    queues: QueueManager,
    /// Live watch over `database.watch_folders` (`Watch = true`).
    watcher: Option<Watcher>,
    /// The in-flight async watcher build (`Watcher::with_debounce`
    /// walks every subdirectory of every watch root — MEASURED 19 s
    /// on a CIFS mount at startup — so the build runs on a worker
    /// thread and the main-thread pump installs through
    /// [`Library::take_watch_folder_rescans`]).
    watcher_rx: Option<Receiver<(u64, std::io::Result<Watcher>)>>,
    /// Last-wins guard: each rebuild bumps the generation; a finished
    /// build installs only when its generation still matches (a
    /// Preferences OK that rebuilds while one build runs drops the
    /// stale result).
    watcher_gen: u64,
}

impl Library {
    /// `DatabaseManager.Open` — opens the database through the full
    /// fallback chain and builds the live watcher from the stored
    /// watch folders (a watcher failure is silent, like the C#
    /// `FileSystemWatcher`).
    pub fn open(file: &Path) -> Result<(Library, OpenStatus), DbError> {
        let (mut library, status) = Self::open_without_watcher(file)?;
        library.rebuild_watcher();
        Ok((library, status))
    }

    /// Opens the database but does not start the live watcher.
    pub fn open_without_watcher(file: &Path) -> Result<(Library, OpenStatus), DbError> {
        let (database, status) = open_with_fallback(file)?;
        crate::trace::trace(format!(
            "library: db open done books={} ({status:?})",
            database.books.len()
        ));
        let lib = Library {
            database,
            file: file.to_path_buf(),
            dirty: false,
            mutation_generation: 0,
            queues: QueueManager::new(),
            watcher: None,
            watcher_rx: None,
            watcher_gen: 0,
        };
        Ok((lib, status))
    }

    /// Starts the live watcher after all startup catalogs are loaded.
    pub fn start_watcher(&mut self) {
        self.rebuild_watcher();
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

    #[track_caller]
    pub fn mark_dirty(&mut self) {
        let caller = std::panic::Location::caller();
        self.dirty = true;
        self.mutation_generation = self.mutation_generation.wrapping_add(1);
        crate::trace::trace(format!(
            "library marked dirty generation={} caller={}:{}",
            self.mutation_generation,
            caller.file(),
            caller.line()
        ));
        crate::incoming_transaction::advance_database_epoch();
    }

    pub fn save_snapshot(&self) -> Option<(ComicDatabase, PathBuf, u64)> {
        self.dirty.then(|| {
            (
                self.database.clone(),
                self.file.clone(),
                self.mutation_generation,
            )
        })
    }

    pub fn persistence_snapshot(&self) -> (ComicDatabase, PathBuf, u64) {
        (
            self.database.clone(),
            self.file.clone(),
            self.mutation_generation,
        )
    }

    pub fn mark_saved(&mut self, generation: u64) {
        if self.mutation_generation == generation {
            self.dirty = false;
        }
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
        // The pump slot: a finished async build installs here, so the
        // existing 1-s timer drives it (no separate timer).
        self.install_pending_watcher();
        let events = self.take_watch_events();
        if events.is_empty() {
            return Vec::new();
        }
        crate::trace::trace(format!(
            "library watcher pending events count={} paths={events:?}",
            events.len()
        ));
        let roots: Vec<String> = self
            .database
            .watch_folders
            .iter()
            .filter(|wf| events.iter().any(|ev| ev.starts_with(&wf.folder)))
            .map(|wf| wf.folder.clone())
            .collect();
        crate::trace::trace(format!(
            "library watcher mapped roots count={} roots={roots:?}",
            roots.len()
        ));
        roots
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
        self.mark_dirty();
        self.rebuild_watcher();
    }

    /// Replaces the whole watch-folder list and rebuilds the live
    /// watcher — the OK-commit of the C# Preferences dialog
    /// (`PreferencesDialog.CopyWatchFoldersToDatabase`), which edits
    /// `lbPaths` in memory and copies into
    /// `Program.Database.WatchFolders` only when the dialog closes
    /// with OK.
    pub fn set_watch_folders(&mut self, folders: Vec<cr_core::database::list_items::WatchFolder>) {
        self.database.watch_folders = folders;
        self.mark_dirty();
        self.rebuild_watcher();
    }

    /// Installs database state that a worker already persisted.
    pub fn install_persisted_database(&mut self, database: ComicDatabase) {
        self.database = database;
        self.dirty = false;
        self.mutation_generation = self.mutation_generation.wrapping_add(1);
        self.rebuild_watcher();
    }

    fn rebuild_watcher(&mut self) {
        // The build walks every subdirectory of every watch root
        // (MEASURED 19 s over CIFS, 0.9 s even warm) — it runs on a
        // worker thread on a CLONE of the folder list; the main
        // thread installs the result through the pump. The previous
        // watcher stays live until the new one lands (last-wins by
        // generation), so a rebuild loses no events.
        self.watcher_gen += 1;
        let gen = self.watcher_gen;
        let folders = self.database.watch_folders.clone();
        let folder_count = folders.len();
        let (tx, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("watcher-build".into())
            .spawn(move || {
                let result = Watcher::with_debounce(&folders, DEFAULT_DEBOUNCE);
                let _ = tx.send((gen, result));
            });
        match spawned {
            Ok(_handle) => {
                crate::trace::trace(format!(
                    "library: watcher build started gen={gen} folders={folder_count}"
                ));
                self.watcher_rx = Some(rx);
            }
            // A failed spawn (effectively OOM) keeps the old watcher;
            // the C# `FileSystemWatcher` failure is silent too.
            Err(err) => eprintln!("watcher build thread failed: {err}"),
        }
    }

    /// Installs a finished async watcher build (the main-thread pump;
    /// called by [`Library::take_watch_folder_rescans`]). Returns
    /// whether a build landed. A failed build clears the watcher (the
    /// old synchronous `.ok()` semantics).
    pub fn install_pending_watcher(&mut self) -> bool {
        let Some(rx) = self.watcher_rx.take() else {
            return false;
        };
        match rx.try_recv() {
            Ok((gen, result)) => {
                if gen != self.watcher_gen {
                    return false;
                }
                match result {
                    Ok(watcher) => {
                        crate::trace::trace(format!("library: watcher installed gen={gen}"));
                        self.watcher = Some(watcher);
                    }
                    Err(err) => {
                        crate::trace::trace(format!(
                            "library: watcher build failed gen={gen}: {err}"
                        ));
                        self.watcher = None;
                    }
                }
                true
            }
            // Still building — keep the receiver for the next tick.
            Err(mpsc::TryRecvError::Empty) => {
                self.watcher_rx = Some(rx);
                false
            }
            // The worker died without sending (a panic): drop the
            // old watcher rather than serve events from a stale
            // generation silently.
            Err(mpsc::TryRecvError::Disconnected) => {
                crate::trace::trace("library: watcher build thread died");
                self.watcher = None;
                true
            }
        }
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
            self.mark_dirty();
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
            self.mark_dirty();
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "comicrust-library-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The async watcher mechanism: `rebuild_watcher` no longer
    /// blocks; the pump installs the finished build, and the live
    /// watcher still delivers events (the watchfolders probe gate E
    /// shape, headless).
    #[test]
    fn watcher_build_is_async_and_installs_through_the_pump() {
        let dir = temp_dir("async");
        let lib_file = dir.join("ComicDb.xml");
        let (mut lib, _) = Library::open(&lib_file).expect("fresh library");

        let watch_root = temp_dir("watch-root");
        lib.add_watch_folder(watch_root.to_str().unwrap(), true);
        // Async: no build is installed synchronously on the call
        // return (the pump slot owns the receiver now).
        assert!(lib.watcher.is_none(), "rebuild must not block");

        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if lib.install_pending_watcher() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(lib.watcher.is_some(), "the pump never installed the build");

        // Events still flow through the installed watcher.
        let changed = watch_root.join("new.cbz");
        std::fs::write(&changed, b"z").unwrap();
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut saw_event = false;
        while Instant::now() < deadline {
            if !lib.take_watch_events().is_empty() {
                saw_event = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(saw_event, "the installed watcher delivered no events");

        // A rebuild with nothing in flight reports no install.
        assert!(!lib.install_pending_watcher());
    }
}
