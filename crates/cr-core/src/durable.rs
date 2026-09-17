//! Durable same-directory file replacement primitives.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Replaces `path` after the new bytes and the parent directory reach storage.
pub fn durable_replace(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();

    loop {
        let sequence = TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(".{name}.{}.{}.tmp", std::process::id(), sequence));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        match options.open(&temporary) {
            Ok(mut file) => {
                let result = (|| {
                    file.write_all(bytes)?;
                    file.sync_all()?;
                    drop(file);
                    std::fs::rename(&temporary, path)?;
                    File::open(parent)?.sync_all()
                })();
                if result.is_err() {
                    let _ = std::fs::remove_file(&temporary);
                }
                return result;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
}

/// Removes `path` and syncs its parent. A missing path is successful.
pub fn durable_remove(path: &Path) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    match std::fs::remove_file(path) {
        Ok(()) => File::open(parent)?.sync_all(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}
