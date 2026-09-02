//! Archive accessors — ports of `ZipSharpZipEngine.cs` and
//! `TarSharpZipEngine.cs`. The 7z/RAR subprocess accessor lives in
//! `sevenzip.rs`; PDF/DjVu are separate T1 follow-ups
//! (docs/phase-1-kickoff.md).

use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::error::Result;
use crate::formats::ids;
use crate::provider::{ComicAccessor, ProviderImageInfo};

/// `ZipSharpZipEngine` — CBZ/ZIP.
pub struct ZipAccessor;

impl ComicAccessor for ZipAccessor {
    /// `FileBasedAccessor.IsFormat`: match the "PK" signature. Any
    /// open error returns `true` in the C# (`catch { return true }`)
    /// — the extension check already decided before this runs.
    fn is_format(&self, source: &Path) -> bool {
        let Some(sig) = crate::formats::signature(ids::CBZ) else {
            return true;
        };
        is_signature(source, sig, true)
    }

    /// Every zip entry, central-directory order, with the entry's
    /// native index (`item.ZipFileIndex`).
    fn get_entry_list(&self, source: &Path) -> Result<Vec<ProviderImageInfo>> {
        let file = File::open(source)?;
        let mut archive = zip::ZipArchive::new(file)?;
        let mut list = Vec::with_capacity(archive.len());
        for i in 0..archive.len() {
            let entry = archive.by_index_raw(i)?;
            list.push(ProviderImageInfo::new(
                i,
                entry.name().to_owned(),
                entry.size(),
            ));
        }
        Ok(list)
    }

    /// Read one entry by exact name; failure returns `None` like the
    /// C# `catch { return null }`.
    fn read_byte_image(&self, source: &Path, info: &ProviderImageInfo) -> Option<Vec<u8>> {
        let file = File::open(source).ok()?;
        let mut archive = zip::ZipArchive::new(file).ok()?;
        let mut entry = archive.by_name(&info.name).ok()?;
        let mut data = Vec::with_capacity(info.size.min(64 * 1024 * 1024) as usize);
        entry.read_to_end(&mut data).ok()?;
        Some(data)
    }
}

/// `TarSharpZipEngine` — CBT/TAR.
pub struct TarAccessor;

impl ComicAccessor for TarAccessor {
    /// The tar engine's check is different from the signature ones: it
    /// succeeds when the stream parses one entry, fails otherwise.
    /// (`Entries::next` reports header errors as `Some(Err)`.)
    fn is_format(&self, source: &Path) -> bool {
        let Ok(file) = File::open(source) else {
            return false;
        };
        let mut archive = tar::Archive::new(file);
        archive
            .entries()
            .map(|mut e| matches!(e.next(), Some(Ok(_))))
            .unwrap_or(false)
    }

    /// Non-directory entries in stream order; the index is always 0
    /// (names are the lookup key), exactly as the C# writes it.
    fn get_entry_list(&self, source: &Path) -> Result<Vec<ProviderImageInfo>> {
        let file = File::open(source)?;
        let mut archive = tar::Archive::new(file);
        let mut list = Vec::new();
        for entry in archive.entries()? {
            let entry = entry?;
            if entry.header().entry_type().is_dir() {
                continue;
            }
            let name = entry.path()?.to_string_lossy().into_owned();
            list.push(ProviderImageInfo::new(0, name, entry.header().size()?));
        }
        Ok(list)
    }

    /// Sequential scan until the entry name matches exactly.
    fn read_byte_image(&self, source: &Path, info: &ProviderImageInfo) -> Option<Vec<u8>> {
        let file = File::open(source).ok()?;
        let mut archive = tar::Archive::new(file);
        let mut entries = archive.entries().ok()?;
        for entry in entries.by_ref() {
            let Ok(entry) = entry else { return None };
            let name = entry.path().ok()?.to_string_lossy().into_owned();
            if name == info.name {
                let mut data = Vec::with_capacity(info.size.min(64 * 1024 * 1024) as usize);
                let mut entry = entry;
                entry.read_to_end(&mut data).ok()?;
                return Some(data);
            }
        }
        None
    }
}

/// Shared signature check (`FileBasedAccessor.IsFormat`). `on_error`
/// encodes the C# difference between engines: signature engines
/// return `true` on open errors, the tar engine returns `false`.
pub(crate) fn is_signature(source: &Path, sig: &[u8], on_error: bool) -> bool {
    match File::open(source) {
        Err(_) => on_error,
        Ok(mut file) => {
            let mut head = vec![0u8; sig.len()];
            match file.read(&mut head) {
                Err(_) => on_error,
                Ok(n) => head[..n] == *sig,
            }
        }
    }
}

/// Accessor selection by format id. Formats whose readers are not
/// ported yet return `None` (the factory then reports the source as
/// unsupported).
pub fn accessor_for(format: i32) -> Option<Box<dyn ComicAccessor>> {
    match format {
        ids::CBZ => Some(Box::new(ZipAccessor)),
        ids::CBT => Some(Box::new(TarAccessor)),
        ids::CB7 | ids::CBR | ids::RAR5 => {
            Some(Box::new(crate::sevenzip::SevenZipAccessor::new(format)))
        }
        _ => None,
    }
}
