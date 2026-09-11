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

/// Open a zip archive with a BOUNDED archive-offset resolution.
///
/// `ZipArchive::new` (`ArchiveOffset::Detect`) resolves prepended junk
/// with two backward searches: one for the EOCD and one for the first
/// CDFH. Both are unbounded — when a file carries no usable central
/// directory the EOCD finder walks the WHOLE file backwards in
/// 2045-byte windows. Over CIFS that is a measured ~0.49 MB/s, and the
/// scanner opens each new book three times (page list, ComicInfo,
/// ComicBook), so a 33 MB file cost minutes and a 2.8 GB one cost
/// hours (measured 2026-09-11 on a live scan: thread "Book Scanner"
/// in `folio_wait_bit_common`, file offsets walking backwards).
///
/// This helper does the whole resolution inside ONE tail read:
///
/// 1. read the last 64 KiB + 22 bytes,
/// 2. locate the EOCD there (a valid zip keeps it inside that window,
///    because the trailing comment is at most 64 KiB),
/// 3. derive the archive offset arithmetically, using the ZIP64 record
///    from the same buffer when the zip32 fields are saturated,
/// 4. verify the derived central directory really starts with a CDFH.
///
/// If any step fails, the function returns an error IMMEDIATELY. It
/// never hands the file to the crate's detection, because that is the
/// unbounded path. A file with no central directory is therefore
/// rejected after a few kilobytes instead of after a full-file scan.
fn open_zip_archive(file: File) -> zip::result::ZipResult<zip::ZipArchive<File>> {
    let archive_offset = resolve_archive_offset(&file)?;
    zip::ZipArchive::with_config(
        zip::read::Config {
            archive_offset: zip::read::ArchiveOffset::Known(archive_offset),
        },
        file,
    )
}

/// Zip signatures used by the bounded resolution.
const EOCD_SIG: [u8; 4] = *b"PK\x05\x06";
const EOCD64_SIG: [u8; 4] = *b"PK\x06\x06";
const EOCD64_LOCATOR_SIG: [u8; 4] = *b"PK\x06\x07";
const CDFH_SIG: [u8; 4] = *b"PK\x01\x02";
/// The largest zip trailing comment, so the largest distance from the
/// EOCD to the end of the file.
const MAX_COMMENT: u64 = 65_535;

/// Derives the archive offset (the count of prepended bytes) from the
/// file tail. Every read here is bounded; see [`open_zip_archive`].
///
/// The offset is never trusted from arithmetic alone. Subtracting the
/// directory size and its relative offset from the EOCD position is
/// only correct when the EOCD directly follows the central directory,
/// and that is FALSE for every zip64 file, because the zip64 record
/// (56 bytes) and its locator (20 bytes) sit in between. Measured on
/// the 2.8 GB omnibus: the arithmetic yields 76 for a file whose real
/// offset is 0. So this builds the small set of candidate offsets and
/// verifies each one against the central directory signature, which
/// costs one 4-byte read per candidate.
fn resolve_archive_offset(file: &File) -> zip::result::ZipResult<u64> {
    use std::io::{Read, Seek, SeekFrom};

    let len = file.metadata()?.len();
    if len < 22 {
        return Err(zip::result::ZipError::InvalidArchive(
            "file is too short to hold a zip end-of-central-directory record",
        ));
    }

    // ONE tail read: the EOCD plus the largest possible comment.
    let tail_len = len.min(22 + MAX_COMMENT);
    let tail_start = len - tail_len;
    let mut tail = vec![0u8; tail_len as usize];
    let mut reader = file.try_clone()?;
    reader.seek(SeekFrom::Start(tail_start))?;
    reader.read_exact(&mut tail)?;

    // The EOCD: the LAST occurrence wins, because a comment may
    // contain the signature bytes.
    let eocd_pos = tail.windows(4).rposition(|w| w == EOCD_SIG).ok_or(
        zip::result::ZipError::InvalidArchive(
            "no zip end-of-central-directory record in the file tail",
        ),
    )?;
    if eocd_pos + 22 > tail.len() {
        return Err(zip::result::ZipError::InvalidArchive(
            "truncated zip end-of-central-directory record",
        ));
    }
    let eocd_abs = tail_start + eocd_pos as u64;
    let eocd = &tail[eocd_pos..eocd_pos + 22];
    let mut entries = u16::from_le_bytes([eocd[10], eocd[11]]) as u64;
    let cd_size = u32::from_le_bytes([eocd[12], eocd[13], eocd[14], eocd[15]]) as u64;
    let mut cd_rel = u32::from_le_bytes([eocd[16], eocd[17], eocd[18], eocd[19]]) as u64;

    // Candidate offsets, in order of likelihood. Each one is verified
    // before it is returned.
    let mut candidates: Vec<u64> = vec![0];

    // The zip64 trailer, when present, is authoritative for the counts
    // and gives a second candidate offset. Both records live in the
    // tail buffer that is already in memory.
    if let Some(z64) = read_zip64_trailer(&tail, tail_start, eocd_pos) {
        entries = z64.entries;
        cd_rel = z64.cd_offset;
        candidates.push(z64.archive_offset);
    } else if let Some(arith) = eocd_abs
        .checked_sub(cd_size)
        .and_then(|v| v.checked_sub(cd_rel))
    {
        // Plain zip32 with the EOCD directly after the directory.
        candidates.push(arith);
    }

    // An empty archive has no central directory to verify against.
    if entries == 0 {
        return Ok(0);
    }

    candidates.dedup();
    for candidate in candidates {
        if let Some(pos) = cd_rel.checked_add(candidate) {
            if pos < eocd_abs && verify_cdfh(file, pos)? {
                return Ok(candidate);
            }
        }
    }
    Err(zip::result::ZipError::InvalidArchive(
        "no zip central-directory header at any derived offset",
    ))
}

/// The zip64 trailer values taken from the tail buffer.
struct Zip64Trailer {
    entries: u64,
    cd_offset: u64,
    archive_offset: u64,
}

/// Reads the zip64 locator (the 20 bytes before the EOCD) and the
/// zip64 end-of-central-directory record it points at. Both are
/// located inside the tail buffer, so this performs no file read.
/// Returns `None` when the file carries no zip64 trailer.
fn read_zip64_trailer(tail: &[u8], tail_start: u64, eocd_pos: usize) -> Option<Zip64Trailer> {
    if eocd_pos < 20 {
        return None;
    }
    let loc = &tail[eocd_pos - 20..eocd_pos];
    if loc[..4] != EOCD64_LOCATOR_SIG {
        return None;
    }
    let eocd64_rel = u64::from_le_bytes([
        loc[8], loc[9], loc[10], loc[11], loc[12], loc[13], loc[14], loc[15],
    ]);

    let eocd64_pos = tail[..eocd_pos - 20]
        .windows(4)
        .rposition(|w| w == EOCD64_SIG)?;
    if eocd64_pos + 56 > tail.len() {
        return None;
    }
    let rec = &tail[eocd64_pos..eocd64_pos + 56];
    let eocd64_abs = tail_start + eocd64_pos as u64;
    Some(Zip64Trailer {
        entries: u64::from_le_bytes([
            rec[32], rec[33], rec[34], rec[35], rec[36], rec[37], rec[38], rec[39],
        ]),
        cd_offset: u64::from_le_bytes([
            rec[48], rec[49], rec[50], rec[51], rec[52], rec[53], rec[54], rec[55],
        ]),
        archive_offset: eocd64_abs.saturating_sub(eocd64_rel),
    })
}

/// Reads the 4 bytes at `pos` and reports whether a central-directory
/// file header starts there. This is the check that proves a derived
/// offset is right before the crate ever touches the file. A short or
/// out-of-range read is a plain `false`, not an error.
fn verify_cdfh(file: &File, pos: u64) -> zip::result::ZipResult<bool> {
    use std::io::{Read, Seek, SeekFrom};

    let mut reader = file.try_clone()?;
    reader.seek(SeekFrom::Start(pos))?;
    let mut sig = [0u8; 4];
    match reader.read_exact(&mut sig) {
        Ok(()) => Ok(sig == CDFH_SIG),
        Err(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof => Ok(false),
        Err(e) => Err(e.into()),
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
        ids::PDF => Some(Box::new(crate::pdf::PdfAccessor)),
        ids::DJVU => Some(Box::new(crate::djvu::DjVuAccessor)),
        ids::FOLDER => Some(Box::new(crate::provider::FolderAccessor)),
        _ => None,
    }
}
