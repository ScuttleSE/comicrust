//! Watch folders (`Database/WatchFolder.cs` + the file-watching part of
//! the C# library): inotify via the `notify` crate, debounced into
//! scanner runs.
//!
//! The C# library polls `WatchFolderCollection` on a timer and calls
//! `QueueManager.StartScan` when a watched folder changes; the port
//! maps the `WatchFolder.Watch` flag onto a live `notify` watcher and
//! collects change events with a debounce window. The consumer polls
//! [`Watcher::take_pending`] (or spawns its own thread) and feeds the
//! affected folders into a scan.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::Duration;

use notify::{RecursiveMode, Watcher as _};

use cr_core::database::list_items::WatchFolder as DbWatchFolder;

/// Debounce default: events inside the window collapse into one
/// notification (the C# uses a 1 s poll timer).
pub const DEFAULT_DEBOUNCE: Duration = Duration::from_secs(1);

/// A live watcher over the library's watch folders.
pub struct Watcher {
    receiver: Receiver<PathBuf>,
    _watcher: notify::RecommendedWatcher,
    debounce: Duration,
    pending: Vec<PathBuf>,
    last_flush: std::time::Instant,
}

impl Watcher {
    /// Watches every `folder` with `watch == true`, recursively (the
    /// C# watch folder semantics always include subfolders).
    pub fn new(folders: &[DbWatchFolder]) -> std::io::Result<Watcher> {
        Self::with_debounce(folders, DEFAULT_DEBOUNCE)
    }

    pub fn with_debounce(
        folders: &[DbWatchFolder],
        debounce: Duration,
    ) -> std::io::Result<Watcher> {
        let (tx, receiver) = std::sync::mpsc::channel();
        let mut watcher =
            notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
                if let Ok(event) = res {
                    for path in event.paths {
                        // Only forward events that touch a real path.
                        if path.exists() || event.kind.is_remove() {
                            let _ = tx.send(path);
                        }
                    }
                }
            })
            .map_err(std::io::Error::other)?;
        for folder in folders {
            if folder.watch && !folder.folder.is_empty() {
                // Missing folders are skipped (the C# scanner tolerates
                // disconnected paths too).
                let _ = watcher.watch(Path::new(&folder.folder), RecursiveMode::Recursive);
            }
        }
        Ok(Watcher {
            receiver,
            _watcher: watcher,
            debounce,
            pending: Vec::new(),
            last_flush: std::time::Instant::now(),
        })
    }

    /// Collects events and returns the affected folders once the
    /// debounce window has passed. Returns an empty vec while events
    /// keep arriving inside the window.
    pub fn take_pending(&mut self) -> Vec<PathBuf> {
        while let Ok(path) = self.receiver.try_recv() {
            self.pending.push(path);
        }
        if self.pending.is_empty() {
            return Vec::new();
        }
        if self.last_flush.elapsed() < self.debounce {
            return Vec::new();
        }
        self.last_flush = std::time::Instant::now();
        std::mem::take(&mut self.pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "comicrust-watch-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn watch_folder_triggers_on_file_change() {
        let dir = temp_dir("w");
        let folders = vec![DbWatchFolder {
            folder: dir.to_string_lossy().into_owned(),
            watch: true,
        }];
        let mut watcher =
            Watcher::with_debounce(&folders, Duration::from_millis(50)).expect("watcher");
        // Unwatched folder: no events.
        let unwatched = temp_dir("u");
        std::fs::write(unwatched.join("x.cbz"), b"z").unwrap();

        std::fs::write(dir.join("new.cbz"), b"z").unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut saw_event = false;
        while std::time::Instant::now() < deadline {
            let pending = watcher.take_pending();
            if !pending.is_empty() {
                saw_event = true;
                assert!(pending
                    .iter()
                    .any(|p| p.to_string_lossy().contains("new.cbz")));
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(saw_event, "watch folder produced no events");
    }
}
