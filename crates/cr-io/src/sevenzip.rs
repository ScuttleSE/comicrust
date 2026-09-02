//! 7z/RAR subprocess accessor — the Linux stand-in for
//! `SevenZipEngine.cs` (which uses the bundled `7z.exe`/COM `7z.dll`).
//! Per ADR-007 we never link unrar; archives CB7, CBR (RAR4), and
//! RAR5 are read through the `7z` command line, mirroring the C#
//! exe mode:
//!
//! - listing: `7z l -slt <archive>` — `Path`/`Size` pairs per entry
//!   block (the C# parses the same fields with a regex)
//! - reading: `7z e -so <archive> <name>` — stdout, exit code 0 means
//!   success (otherwise `None`, like the C# `return null`)
//! - format check: the format's leading-byte signature
//!   (`FileBasedAccessor.IsFormat`; open errors report `true`)
//!
//! The binary is discovered once: `CR_SEVENZIP` env override, then
//! `7z`, `7za`, `7zz` on `PATH`. Operations spawn one process each,
//! as the C# exe mode does; pooling is a possible later optimization.
//!
//! Known parity gap (subprocess policy in docs/phase-1-kickoff.md):
//! 7z cannot *create* RAR archives, so RAR fixtures must come from
//! real files and those tests stay gated behind `CR_FORMAT_TESTS`.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::error::{Error, Result};
use crate::formats;
use crate::provider::{ComicAccessor, ProviderImageInfo};

/// Locates the 7z console executable.
fn find_7z() -> Option<PathBuf> {
    if let Ok(path) = std::env::var("CR_SEVENZIP") {
        let p = PathBuf::from(path);
        if p.is_file() {
            return Some(p);
        }
    }
    let candidates = ["7z", "7za", "7zz"];
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        for candidate in candidates {
            let p = dir.join(candidate);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

fn run_7z(args: &[&str]) -> Result<std::process::Output> {
    let exe = find_7z().ok_or_else(|| {
        Error::Access("7z executable not found (install p7zip, or set CR_SEVENZIP)".to_string())
    })?;
    Command::new(exe)
        .args(args)
        .output()
        .map_err(|e| Error::Access(format!("running 7z failed: {e}")))
}

/// Reads CB7, CBR (RAR4), and RAR5 through the 7z subprocess.
pub struct SevenZipAccessor {
    format: i32,
}

impl SevenZipAccessor {
    pub fn new(format: i32) -> Self {
        SevenZipAccessor { format }
    }
}

impl ComicAccessor for SevenZipAccessor {
    /// `FileBasedAccessor.IsFormat` — signature match; any open error
    /// returns `true` (the extension check decided before this runs).
    fn is_format(&self, source: &Path) -> bool {
        match formats::signature(self.format) {
            Some(sig) => crate::accessors::is_signature(source, sig, true),
            None => true,
        }
    }

    /// `SevenZipEngine.GetEntryList` exe mode: every listing block's
    /// Path and Size. Index is always 0 in exe mode; reads go by name.
    fn get_entry_list(&self, source: &Path) -> Result<Vec<ProviderImageInfo>> {
        let source_str = source.to_string_lossy();
        let out = run_7z(&["l", "-slt", &source_str])?;
        if !out.status.success() {
            return Err(Error::Access(format!(
                "7z listing failed for {source_str} (exit {:?})",
                out.status.code()
            )));
        }
        let text = String::from_utf8_lossy(&out.stdout);
        Ok(parse_listing(&text))
    }

    /// `SevenZipEngine.GetFileData(source, file)` exe mode.
    fn read_byte_image(&self, source: &Path, info: &ProviderImageInfo) -> Option<Vec<u8>> {
        let source_str = source.to_string_lossy();
        let out = run_7z(&["e", "-so", "-y", &source_str, &info.name]).ok()?;
        if out.status.success() {
            Some(out.stdout)
        } else {
            None
        }
    }
}

/// Parses `7z l -slt` output: blank-line separated blocks, each with
/// `Path = <name>` and `Size = <n>` (the fields the C# regex grabs).
/// `Packed Size` does not match the `Size = ` prefix.
fn parse_listing(text: &str) -> Vec<ProviderImageInfo> {
    let mut entries = Vec::new();
    let mut current_path: Option<String> = None;
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            current_path = None;
            continue;
        }
        if let Some(value) = line.strip_prefix("Path = ") {
            current_path = Some(value.to_string());
        } else if let Some(value) = line.strip_prefix("Size = ") {
            if let Some(name) = current_path.take() {
                let size = value.trim().parse().unwrap_or(0);
                entries.push(ProviderImageInfo::new(0, name, size));
            }
        }
    }
    entries
}

#[cfg(test)]
mod tests {
    use super::parse_listing;
    use crate::formats::ids;

    #[test]
    fn listing_parse() {
        let text = "\
Date Time Attr Size Compressed Name
------------------- ----- ------------ ------------ ------------
2024-01-01 00:00:00 ..... 123 100 page1.jpg
2024-01-01 00:00:00 D.... 0 0 pages
------------------- ----- ------------ ------------ ------------

Path = page1.jpg
Folder = -
Size = 123
Packed Size = 100

Path = pages
Folder = +
Size = 0

Path = ComicInfo.xml
Folder = -
Size = 456
";
        let entries = parse_listing(text);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].name, "page1.jpg");
        assert_eq!(entries[0].size, 123);
        assert_eq!(entries[1].name, "pages");
        assert_eq!(entries[1].size, 0);
        assert_eq!(entries[2].name, "ComicInfo.xml");
        assert_eq!(entries[2].size, 456);
    }

    #[test]
    fn accessor_selection_includes_subprocess_formats() {
        assert!(crate::accessors::accessor_for(ids::CB7).is_some());
        assert!(crate::accessors::accessor_for(ids::CBR).is_some());
        assert!(crate::accessors::accessor_for(ids::RAR5).is_some());
    }
}
