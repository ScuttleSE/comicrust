//! RAR write-back through the RARLAB `rar` command line (ADR-030).
//!
//! Reads stay on the `7z` subprocess (`sevenzip.rs`). Writes need a
//! RAR *compressor*, which does not exist as free software: 7-Zip and
//! libarchive decode only, unrar is extract-only and GPL-incompatible.
//! The C# never writes into RAR archives at all (`CbrComicProvider`
//! carries no `EnableUpdate`); this module is a recorded beyond-parity
//! addition. The binary is never bundled or linked — when absent, the
//! caller degrades (`ComicProvider::store_info` falls back to the
//! xattr stream; the app write path reports the error).

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Error, Result};

/// Locates the RARLAB `rar` console executable. `unrar` never counts —
/// it cannot create or modify archives.
pub fn find_rar() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("CR_RAR") {
        let p = PathBuf::from(path);
        if p.is_file() {
            return Some(p);
        }
    }
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let p = dir.join("rar");
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

/// `rar a <archive> <files>` with cwd = the files' directory, so the
/// entries land at the archive root under their bare names (the same
/// entry shape the 7z update path produces). `-y` answers any
/// remaining query, and stdin is null — a password-protected target
/// then fails its prompt immediately (measured: exit 12 "Read error
/// in the file stdin") instead of hanging the caller.
pub fn add_files(source: &Path, files: &[&Path]) -> Result<()> {
    let exe = find_rar().ok_or_else(|| {
        Error::Access(
            "rar executable not found (install the RARLAB rar CLI, or set CR_RAR)".to_string(),
        )
    })?;
    if files.is_empty() {
        return Err(Error::Access("no payload files for the rar update".into()));
    }
    // The subprocess runs inside the staging directory; a relative
    // archive path would resolve against it.
    let source_abs = std::fs::canonicalize(source)
        .map_err(|e| Error::Access(format!("resolving {source:?} for the rar update: {e}")))?;
    let dir = files[0]
        .parent()
        .ok_or_else(|| Error::Access("payload file has no parent directory".into()))?;
    let names: Vec<String> = files
        .iter()
        .map(|f| {
            f.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .ok_or_else(|| Error::Access(format!("bad payload path {f:?}")))
        })
        .collect::<Result<_>>()?;
    let source_str = source_abs.to_string_lossy();
    let out = Command::new(&exe)
        .arg("a")
        .arg("-y")
        .arg(&*source_str)
        .args(&names)
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| Error::Access(format!("running rar failed: {e}")))?;
    if !out.status.success() {
        return Err(Error::Access(format!(
            "rar update failed for {source_str} (exit {:?}): {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_rar_reports_access_error() {
        // Runs wherever `rar` is absent (CI included); on hosts with
        // rar installed there is nothing to assert.
        if find_rar().is_some() {
            return;
        }
        let dir = std::env::temp_dir().join(format!(
            "comicrust-rar-missing-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let payload = dir.join("ComicInfo.xml");
        std::fs::write(&payload, b"<ComicInfo />").unwrap();
        let target = dir.join("comic.cbr");
        std::fs::write(&target, b"Rar!\x1a\x07\x00").unwrap();
        let err = add_files(&target, &[payload.as_path()]).unwrap_err();
        assert!(err.to_string().contains("rar executable not found"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
