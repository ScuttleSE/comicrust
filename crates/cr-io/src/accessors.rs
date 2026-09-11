//! Archive accessors — ports of `ZipSharpZipEngine.cs` and
//! `TarSharpZipEngine.cs`. The 7z/RAR subprocess accessor lives in
//! `sevenzip.rs`; PDF/DjVu are separate T1 follow-ups
//! (docs/archive/phases/phase-1.md).

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
        let mut archive = open_zip_archive(file)?;
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
        let mut archive = open_zip_archive(file).ok()?;
        let mut entry = archive.by_name(&info.name).ok()?;
        let mut data = Vec::with_capacity(info.size.min(64 * 1024 * 1024) as usize);
        entry.read_to_end(&mut data).ok()?;
        Some(data)
    }

    /// `ZipSharpZipEngine.Read`: case-insensitive full-name entry
    /// search (`zipFile.FindEntry(s, ignoreCase: true)`).
    fn read_info_file(&self, source: &Path, filename: &str) -> Option<Vec<u8>> {
        let file = File::open(source).ok()?;
        let mut archive = open_zip_archive(file).ok()?;
        let index = (0..archive.len()).find(|i| {
            archive
                .by_index_raw(*i)
                .map(|f| f.name().eq_ignore_ascii_case(filename))
                .unwrap_or(false)
        })?;
        let mut entry = archive.by_index(index).ok()?;
        let mut data = Vec::new();
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

    /// `TarSharpZipEngine.Read`: match the entry *basename*
    /// case-insensitively (`Path.GetFileName` comparison).
    fn read_info_file(&self, source: &Path, filename: &str) -> Option<Vec<u8>> {
        let file = File::open(source).ok()?;
        let mut archive = tar::Archive::new(file);
        let mut entries = archive.entries().ok()?;
        for entry in entries.by_ref() {
            let Ok(entry) = entry else { return None };
            let name = entry.path().ok()?.to_string_lossy().into_owned();
            let basename = name.rsplit('/').next().unwrap_or(&name);
            if basename.eq_ignore_ascii_case(filename) {
                let mut data = Vec::new();
                let mut entry = entry;
                entry.read_to_end(&mut data).ok()?;
                return Some(data);
            }
        }
        None
    }
}

/// Open a zip archive with a cheap archive-offset resolution.
///
/// `ZipArchive::new` (`ArchiveOffset::Detect`) handles prepended junk by
/// searching BACKWARDS from the EOCD for the first CDFH in 2045-byte
/// windows. On a big archive over CIFS (~4 ms per seek+read round trip)
/// that crawl costs hours per file (measured: a 2.85 GB omnibus with a
/// 512-byte prepend). This helper reads the file tail ONCE, locates the
/// EOCD, and derives the archive offset arithmetically
/// (`eocd_offset - cd_size - relative_cd_offset`); the crate's mandatory
/// CDFH guess then hits on the FIRST read. Any parse surprise falls back
/// to the crate's own detection.
fn open_zip_archive(file: File) -> zip::result::ZipResult<zip::ZipArchive<File>> {
    use std::io::{Seek, SeekFrom};

    const EOCD_SIG: [u8; 4] = [0x50, 0x4b, 0x05, 0x06];
    const MAX_COMMENT: u64 = 65_535;

    let detect = || zip::ZipArchive::new(file.try_clone()?);
    let len = match file.metadata() {
        Ok(m) => m.len(),
        Err(_) => return detect(),
    };
    if len < 22 {
        return detect();
    }

    // One tail read: EOCD (22) + the maximum comment.
    let tail_len = (len).min(22 + MAX_COMMENT);
    let tail_start = len - tail_len;
    let mut tail = vec![0u8; tail_len as usize];
    let mut file2 = match file.try_clone() {
        Ok(f) => f,
        Err(_) => return detect(),
    };
    if file2.seek(SeekFrom::Start(tail_start)).is_err() {
        return detect();
    }
    if std::io::Read::read_exact(&mut file2, &mut tail).is_err() {
        return detect();
    }

    // EOCD: scan back for the signature (last occurrence wins — the
    // comment may contain the bytes).
    let Some(eocd_pos) = tail.windows(4).rposition(|w| w == EOCD_SIG) else {
        return detect();
    };
    let eocd = eocd_pos + tail_start as usize;
    if eocd + 22 > len as usize {
        return detect();
    }
    let b = &tail[eocd_pos..eocd_pos + 22];
    let cd_size = u32::from_le_bytes([b[12], b[13], b[14], b[15]]) as u64;
    let cd_rel = u32::from_le_bytes([b[16], b[17], b[18], b[19]]) as u64;

    // Derive the archive offset from the EOCD position. If the numbers
    // disagree (or the derivation underflows), let the crate detect.
    let eocd_off = eocd as u64;
    if cd_size == 0 || cd_rel == 0 {
        return detect();
    }
    let Some(archive_offset) = eocd_off
        .checked_sub(cd_size)
        .and_then(|v| v.checked_sub(cd_rel))
    else {
        return detect();
    };
    if archive_offset == 0 {
        // No prepend — the plain open is already fast.
        return detect();
    }

    zip::ZipArchive::with_config(
        zip::read::Config {
            archive_offset: zip::read::ArchiveOffset::Known(archive_offset),
        },
        file,
    )
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
        ids::PDF => Some(Box::new(crate::pdf::PdfAccessor)),
        ids::DJVU => Some(Box::new(crate::djvu::DjVuAccessor)),
        ids::FOLDER => Some(Box::new(crate::provider::FolderAccessor)),
        _ => None,
    }
}
