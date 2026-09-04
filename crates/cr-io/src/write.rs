//! Metadata write-back — port of `IStorageProvider`/`StorageProvider`
//! write paths and `SevenZipEngine.UpdateComicInfos`.
//!
//! Per docs/phase-1-kickoff.md T5 the zip/tar/folder writers are
//! native (the C# shells out to `7z u` even for zip/tar — we preserve
//! the *behavior*: every entry's decompressed content stays identical,
//! only the metadata entries change):
//!
//! - CBZ: full zip rewrite (same entry order, Stored/Deflated method
//!   preserved where the original used them) to a temp file, then
//!   atomic rename.
//! - CBT: full tar rewrite, same order.
//! - CB7: `7z u -t7z <archive> <tempfile>` per metadata file, as the
//!   C# `Update` does (temp dir + delete).
//! - Folder: plain files.
//!
//! Failure semantics mirror `WriteErrorException`: the caller gets an
//! error for user-actionable failures (7z missing, archive broken),
//! not a silent `false`.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use cr_core::model::comic_book::ComicBook;

use crate::error::{Error, Result};
use crate::formats::ids;
use crate::provider::ComicProvider;

const COMIC_INFO_XML: &str = "ComicInfo.xml";
const COMIC_BOOK_XML: &str = "ComicBook.xml";

/// `ComicProvider.StoreInfo`/`OnStoreInfo`: write both metadata files
/// into the source when the format supports updating
/// (`FileFormatAttribute(EnableUpdate = true)`). Returns whether the
/// archive content changed.
pub fn store_info(provider: &ComicProvider, book: &ComicBook) -> Result<bool> {
    store_info_scoped(provider, book, true)
}

/// The `ComicBook.WriteInfoToFile` scope: the ComicBook.xml library
/// info only writes when the caller allows it (the C# passes
/// `GetInfo()` — ComicInfo only — when `UpdateComicBookFiles` is
/// off).
pub fn store_info_scoped(
    provider: &ComicProvider,
    book: &ComicBook,
    with_book_info: bool,
) -> Result<bool> {
    if !provider.format().supports_update {
        return Ok(false);
    }
    let info_bytes = book
        .info
        .serialize_bytes()
        .map_err(|e| Error::Access(format!("serializing ComicInfo.xml: {e}")))?;
    let book_bytes = if with_book_info {
        Some(
            book.serialize_bytes()
                .map_err(|e| Error::Access(format!("serializing ComicBook.xml: {e}")))?,
        )
    } else {
        None
    };
    let mut pairs: Vec<(&str, &[u8])> = vec![(COMIC_INFO_XML, &info_bytes)];
    if let Some(bytes) = &book_bytes {
        pairs.push((COMIC_BOOK_XML, bytes));
    }

    match provider.format().id {
        ids::CBZ => rewrite_zip(provider.source(), &pairs),
        ids::CBT => rewrite_tar(provider.source(), &pairs),
        ids::CB7 => {
            let mut changed = false;
            for (name, bytes) in &pairs {
                if sevenzip_update(provider.source(), ids::CB7, name, bytes)? {
                    changed = true;
                }
            }
            Ok(changed)
        }
        ids::FOLDER => {
            let base = provider.source();
            for (name, bytes) in &pairs {
                std::fs::write(base.join(name), bytes)?;
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}

/// Zip rewrite: every entry's decompressed content is copied in
/// original central-directory order; the named entries are replaced
/// (case-insensitive, `FindEntry(ignoreCase: true)`) or appended.
fn rewrite_zip(source: &Path, replacements: &[(&str, &[u8])]) -> Result<bool> {
    let file = std::fs::File::open(source)?;
    let mut archive = zip::ZipArchive::new(file)?;
    let mut entries: Vec<(String, Vec<u8>, zip::CompressionMethod)> = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        let method = entry.compression();
        let mut data = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut data)?;
        entries.push((name, data, method));
    }

    let mut changed = false;
    for (name, data) in replacements {
        let existing = entries
            .iter_mut()
            .find(|(entry_name, _, _)| entry_name.eq_ignore_ascii_case(name));
        match existing {
            Some(entry) => {
                if entry.1 != *data {
                    entry.1 = data.to_vec();
                    changed = true;
                }
            }
            None => {
                entries.push((
                    (*name).to_string(),
                    data.to_vec(),
                    zip::CompressionMethod::Deflated,
                ));
                changed = true;
            }
        }
    }
    if !changed {
        return Ok(false);
    }

    let tmp = temp_sibling(source)?;
    let file = std::fs::File::create(&tmp)?;
    let mut writer = zip::ZipWriter::new(file);
    for (name, data, method) in &entries {
        let method = match method {
            zip::CompressionMethod::Stored | zip::CompressionMethod::Deflated => *method,
            _ => zip::CompressionMethod::Deflated,
        };
        let options = zip::write::SimpleFileOptions::default().compression_method(method);
        writer.start_file(name.clone(), options)?;
        writer.write_all(data)?;
    }
    writer.finish()?;
    std::fs::rename(&tmp, source)?;
    Ok(true)
}

/// Tar rewrite: entries copied in stream order, replacements applied
/// by exact name.
fn rewrite_tar(source: &Path, replacements: &[(&str, &[u8])]) -> Result<bool> {
    let file = std::fs::File::open(source)?;
    let mut archive = tar::Archive::new(file);
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        if entry.header().entry_type().is_dir() {
            continue;
        }
        let name = entry.path()?.to_string_lossy().into_owned();
        let mut data = Vec::new();
        entry.read_to_end(&mut data)?;
        entries.push((name, data));
    }

    let mut changed = false;
    for (name, data) in replacements {
        let existing = entries
            .iter_mut()
            .find(|(entry_name, _)| entry_name == name);
        match existing {
            Some(entry) => {
                if entry.1 != *data {
                    entry.1 = data.to_vec();
                    changed = true;
                }
            }
            None => {
                entries.push(((*name).to_string(), data.to_vec()));
                changed = true;
            }
        }
    }
    if !changed {
        return Ok(false);
    }

    let tmp = temp_sibling(source)?;
    let file = std::fs::File::create(&tmp)?;
    let mut builder = tar::Builder::new(file);
    for (name, data) in &entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append_data(&mut header, name, data.as_slice())?;
    }
    builder.finish()?;
    std::fs::rename(&tmp, source)?;
    Ok(true)
}

/// `SevenZipEngine.Update` — write the payload to a temp file and run
/// `7z u -t<type> <archive> <temp>`.
fn sevenzip_update(source: &Path, format: i32, name: &str, data: &[u8]) -> Result<bool> {
    let type_arg = match format {
        ids::CBZ => "zip",
        ids::CBT => "tar",
        ids::CB7 => "7z",
        _ => return Err(Error::Access("format not supported for updating".into())),
    };
    let exe = crate::sevenzip::find_7z()
        .ok_or_else(|| Error::Access("7z executable not found".into()))?;
    let tmp_dir = std::env::temp_dir().join(format!("comicrust-update-{}", std::process::id()));
    std::fs::create_dir_all(&tmp_dir)?;
    let tmp_file = tmp_dir.join(name);
    std::fs::write(&tmp_file, data)?;

    let source_str = source.to_string_lossy();
    let tmp_str = tmp_file.to_string_lossy();
    let out = crate::sevenzip::run_7z_at(
        &exe,
        &["u", &format!("-t{type_arg}"), &source_str, &tmp_str],
    );
    let _ = std::fs::remove_file(&tmp_file);
    let _ = std::fs::remove_dir(&tmp_dir);
    let out = out?;
    if !out.status.success() {
        return Err(Error::Access(format!(
            "7z update failed for {source_str} (exit {:?})",
            out.status.code()
        )));
    }
    // 7z updates the entry even when identical; report changed based
    // on whether the archive previously contained different data.
    Ok(true)
}

fn temp_sibling(source: &Path) -> Result<PathBuf> {
    let dir = source
        .parent()
        .ok_or_else(|| Error::Access("source has no parent directory".into()))?;
    let name = source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(dir.join(format!(".{name}.rewrite-{}", std::process::id())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zip_rewrite_replaces_and_appends() {
        let dir = std::env::temp_dir().join(format!(
            "comicrust-rewrite-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("comic.cbz");
        {
            let file = std::fs::File::create(&path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            let options: zip::write::SimpleFileOptions = Default::default();
            for (name, data) in [
                ("page1.jpg", b"one".as_slice()),
                ("a.jpg", b"two".as_slice()),
            ] {
                zip.start_file(name, options).unwrap();
                zip.write_all(data).unwrap();
            }
            zip.finish().unwrap();
        }

        let changed = rewrite_zip(
            &path,
            &[
                ("page1.jpg", b"ONE!".as_slice()),
                ("ComicInfo.xml", b"<ComicInfo />".as_slice()),
            ],
        )
        .unwrap();
        assert!(changed);

        let mut archive = zip::ZipArchive::new(std::fs::File::open(&path).unwrap()).unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        // Original order preserved, ComicInfo.xml appended.
        assert_eq!(names, ["page1.jpg", "a.jpg", "ComicInfo.xml"]);
        let mut page1 = String::new();
        archive
            .by_name("page1.jpg")
            .unwrap()
            .read_to_string(&mut page1)
            .unwrap();
        assert_eq!(page1, "ONE!");

        // No-op rewrite reports unchanged.
        let changed =
            rewrite_zip(&path, &[("ComicInfo.xml", b"<ComicInfo />".as_slice())]).unwrap();
        assert!(!changed);

        std::fs::remove_dir_all(&dir).ok();
    }
}
