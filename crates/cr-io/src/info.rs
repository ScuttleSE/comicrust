//! In-archive and stored metadata — port of `XmlInfo/*` (provider
//! priority via `XmlInfoProviderFactory.DeserializeAll`), the
//! `ComicProvider.Load` chain, and `NtfsInfoStorage` remapped to
//! Linux xattrs (ADR-006).
//!
//! Load order (`ComicProvider.Load<T>`):
//! 1. stored info — xattrs `user.comicrack.*` (the NTFS ADS
//!    `ComicRackInfo`/`ComicRackBook` streams)
//! 2. sidecar — `<file>.xml` then `<file-without-ext>.xml`
//! 3. in-archive — provider order: ComicInfo.xml (0), MetronInfo.xml
//!    (1, mapped to ComicInfo); ComicBook.xml alone for books
//!
//! `Fast` returns the first hit; `Slow` prefers the in-archive data
//! and falls back to stored info.
//!
//! The `DisableNTFS`/`DisableSidecar` engine options are not ported
//! yet (Phase 0 settings tail); the chain behaves as if both are
//! enabled, which is the default.

use std::path::{Path, PathBuf};

use cr_core::model::comic_book::ComicBook;
use cr_core::model::comic_info::ComicInfo;
use cr_core::xml::XmlReader;

use crate::accessors::accessor_for;
use crate::provider::ComicProvider;

/// `InfoLoadingMethod.cs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InfoLoadingMethod {
    /// First source wins (xattr/sidecar, then in-archive).
    Fast,
    /// In-archive data preferred, stored info as fallback.
    Slow,
}

/// Xattr names for the stored `ComicInfo`/`ComicBook` streams — the
/// `NtfsInfoStorage` ADS stream names under the ADR-006 namespace.
pub const XATTR_COMIC_INFO: &str = "user.comicrack.ComicRackInfo";
pub const XATTR_COMIC_BOOK: &str = "user.comicrack.ComicRackBook";

// --- stored info (xattrs, NtfsInfoStorage port) -------------------------

/// `NtfsInfoStorage.LoadInfo<T>` — xattr first (checked in the caller
/// via `load_stored_*`).
fn read_xattr(file: &Path, name: &str) -> Option<Vec<u8>> {
    // Dangling or unreadable xattrs behave like absent streams.
    xattr::get(file, name).ok().flatten()
}

fn write_xattr(file: &Path, name: &str, data: &[u8]) -> bool {
    xattr::set(file, name, data).is_ok()
}

fn remove_xattr(file: &Path, name: &str) {
    let _ = xattr::remove(file, name);
}

/// `NtfsInfoStorage.StoreInfo` — store ComicInfo bytes, and the
/// ComicBook stream when a full book is given. Skips a write when the
/// stored content is already equal (`IsSameContent`).
pub fn store_stored_info(file: &Path, book: &ComicBook) -> bool {
    let mut success = false;

    let stored_info = load_stored_comic_info(file);
    if !stored_info.is_some_and(|info| info.is_same_content(&book.info, true)) {
        if let Ok(bytes) = book.info.serialize_bytes() {
            if write_xattr(file, XATTR_COMIC_INFO, &bytes) {
                success = true;
            }
        }
    }

    // The C# writes the book stream with the *stripped* serializer
    // (`ComicBook.Serialize`) and reads it with `DeserializeFull`.
    let stored_book = load_stored_comic_book(file);
    if !stored_book.is_some_and(|b| b.is_same_content(book, true)) {
        if let Ok(bytes) = book.serialize_bytes() {
            if write_xattr(file, XATTR_COMIC_BOOK, &bytes) {
                success = true;
            }
        }
    }

    success
}

/// `NtfsInfoStorage.ClearInfo`/`ClearBook`.
pub fn clear_stored_info(file: &Path) {
    remove_xattr(file, XATTR_COMIC_INFO);
}

pub fn clear_stored_book(file: &Path) {
    remove_xattr(file, XATTR_COMIC_BOOK);
}

fn load_stored_comic_info(file: &Path) -> Option<ComicInfo> {
    let bytes = read_xattr(file, XATTR_COMIC_INFO)?;
    parse_comic_info_bytes(&bytes)
}

fn load_stored_comic_book(file: &Path) -> Option<ComicBook> {
    let bytes = read_xattr(file, XATTR_COMIC_BOOK)?;
    parse_comic_book_bytes(&bytes)
}

// --- sidecars (`ComicInfo.LoadFromSidecar`) -----------------------------

/// Sidecar candidates: `<file>.xml`, then the extension swapped for
/// `.xml`.
fn sidecar_paths(file: &Path) -> [Option<PathBuf>; 2] {
    let mut paths: [Option<PathBuf>; 2] = [None, None];
    let mut s = file.to_string_lossy().into_owned();
    s.push_str(".xml");
    paths[0] = Some(PathBuf::from(s));
    if let Some(stem) = file.extension().and_then(|e| e.to_str()) {
        let stem_len = stem.len();
        let mut base = file.to_string_lossy().into_owned();
        let new_len = base.len() - stem_len - 1;
        base.truncate(new_len);
        base.push_str(".xml");
        paths[1] = Some(PathBuf::from(base));
    }
    paths
}

fn load_sidecar_comic_info(file: &Path) -> Option<ComicInfo> {
    for path in sidecar_paths(file).into_iter().flatten() {
        if let Ok(bytes) = std::fs::read(&path) {
            if let Some(info) = parse_comic_info_bytes(&bytes) {
                return Some(info);
            }
        }
    }
    None
}

fn load_sidecar_comic_book(file: &Path) -> Option<ComicBook> {
    for path in sidecar_paths(file).into_iter().flatten() {
        if let Ok(bytes) = std::fs::read(&path) {
            if let Some(book) = parse_comic_book_bytes(&bytes) {
                return Some(book);
            }
        }
    }
    None
}

// --- parsing helpers ----------------------------------------------------

fn parse_comic_info_bytes(bytes: &[u8]) -> Option<ComicInfo> {
    let mut cursor = std::io::Cursor::new(bytes);
    let mut reader = XmlReader::new(&mut cursor);
    cr_core::model::comic_info::parse_root(&mut reader).ok()
}

fn parse_comic_book_bytes(bytes: &[u8]) -> Option<ComicBook> {
    let mut cursor = std::io::Cursor::new(bytes);
    let mut reader = XmlReader::new(&mut cursor);
    ComicBook::parse_root(&mut reader).ok()
}

// --- in-archive providers (XmlInfoProviderFactory) ----------------------

/// In-archive `ComicInfo` sources in `XmlInfoFile` order: ComicInfo.xml
/// (order 0), MetronInfo.xml (order 1, mapped). `DeserializeAll`
/// returns the first that parses.
fn in_archive_comic_info(provider: &ComicProvider) -> Option<ComicInfo> {
    for name in ["ComicInfo.xml", "MetronInfo.xml"] {
        if let Some(bytes) = provider.read_info_file(name) {
            if name == "MetronInfo.xml" {
                if let Some(metron) = parse_metron_bytes(&bytes) {
                    return Some(metron.to_comic_info());
                }
            } else if let Some(info) = parse_comic_info_bytes(&bytes) {
                return Some(info);
            }
        }
    }
    None
}

/// In-archive `ComicBook` sources: ComicBook.xml only.
fn in_archive_comic_book(provider: &ComicProvider) -> Option<ComicBook> {
    let bytes = provider.read_info_file("ComicBook.xml")?;
    parse_comic_book_bytes(&bytes)
}

fn parse_metron_bytes(bytes: &[u8]) -> Option<cr_core::model::metron_info::MetronInfo> {
    let mut cursor = std::io::Cursor::new(bytes);
    let mut reader = XmlReader::new(&mut cursor);
    cr_core::model::metron_info::MetronInfo::parse_root(&mut reader).ok()
}

// --- public chain (ComicProvider.Load / IInfoStorage) -------------------

impl ComicProvider {
    /// Raw named-entry access for the info chain.
    pub fn read_info_file(&self, filename: &str) -> Option<Vec<u8>> {
        let accessor = accessor_for(self.format().id)?;
        accessor.read_info_file(self.source(), filename)
    }

    /// `ComicProvider.LoadInfo(method)` — the full stored + sidecar +
    /// in-archive chain for `ComicInfo`.
    pub fn load_info(&self, method: InfoLoadingMethod) -> Option<ComicInfo> {
        let stored = load_stored_comic_info(self.source())
            .or_else(|| load_sidecar_comic_info(self.source()));
        if stored.is_some() && method == InfoLoadingMethod::Fast {
            return stored;
        }
        in_archive_comic_info(self).or(stored)
    }

    /// `ComicProvider.LoadBook(method)` — same chain for `ComicBook`.
    pub fn load_book(&self, method: InfoLoadingMethod) -> Option<ComicBook> {
        let stored = load_stored_comic_book(self.source())
            .or_else(|| load_sidecar_comic_book(self.source()));
        if stored.is_some() && method == InfoLoadingMethod::Fast {
            return stored;
        }
        in_archive_comic_book(self).or(stored)
    }

    /// `ComicProvider.StoreInfo` — in-archive write-back when the
    /// format supports updating (`UpdateEnabled`), then the stored
    /// xattr streams (`NtfsInfoStorage.StoreInfo`). Returns whether
    /// anything was written.
    pub fn store_info(&self, book: &ComicBook) -> bool {
        let mut written = false;
        if self.format().supports_update {
            match crate::write::store_info(self, book) {
                Ok(w) => written = w || written,
                // `OnStoreInfo` maps WriteErrorException to an error
                // event; without event plumbing we keep the store
                // going (the C# returns false on failure).
                Err(e) => eprintln!("archive store failed: {e}"),
            }
        }
        if store_stored_info(self.source(), book) {
            written = true;
        }
        written
    }
}
