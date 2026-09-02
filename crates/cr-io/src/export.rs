//! Export pipeline skeleton — port of `ExportSetting.cs`,
//! `ExportImageContainer.cs`, and the `PackedStorageProvider` page
//! loop (docs/phase-1-kickoff.md T5: "the dialogs come in Phase 5").
//!
//! The C# export runs pages through `GetImages` (resize/convert per
//! `ExportSetting`) and packs them with `PackedStorageProvider`
//! (natural page order, compression level from `ExportCompression`).
//! This skeleton covers the container and the sequential packing;
//! the parallel `Parallel.For`, streaming `PageResult.Store()` spill,
//! and progress events stay open until the export dialogs land.

use std::io::Write;
use std::path::Path;

use cr_core::model::comic_info::ComicInfo;

use crate::error::{Error, Result};
use crate::provider::ComicProvider;

/// `ExportImageContainer` — one exported page payload.
#[derive(Clone, Debug)]
pub struct ExportImageContainer {
    pub data: Vec<u8>,
    /// C# `NeedsToConvert` — true when the data still needs a
    /// conversion pass before it is final.
    pub needs_to_convert: bool,
}

/// `ExportCompression`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportCompression {
    Store,
    Medium,
    Strong,
}

/// Compression level mapping from `CbzStorageProvider.OnCreateFile`.
pub fn compression_level(compression: ExportCompression) -> i64 {
    match compression {
        ExportCompression::Store => 0,
        ExportCompression::Medium => 5,
        ExportCompression::Strong => 9,
    }
}

/// Exports the pages of `source` in provider page order into a new
/// CBZ at `target`, writing `info` as ComicInfo.xml. The source bytes
/// pass through untouched (the `GetImages` resize/convert stack is
/// Phase 5 dialog work).
pub fn export_pages_to_cbz(
    source: &Path,
    target: &Path,
    info: &ComicInfo,
    compression: ExportCompression,
) -> Result<usize> {
    let provider = ComicProvider::open(source)?;
    let tmp = target.with_extension("cbz.export");
    let file = std::fs::File::create(&tmp)?;
    let mut writer = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default().compression_method(
        if compression == ExportCompression::Store {
            zip::CompressionMethod::Stored
        } else {
            zip::CompressionMethod::Deflated
        },
    );

    for (index, page) in provider.pages().iter().enumerate() {
        let ext = page
            .name
            .rsplit_once('.')
            .map(|(_, e)| format!(".{e}"))
            .unwrap_or_else(|| ".jpg".into());
        let data = provider
            .read_page(index)
            .ok_or_else(|| Error::Access(format!("page {index} failed to load")))?;
        writer.start_file(format!("Page{index:0>4}{ext}"), options)?;
        writer.write_all(&data)?;
    }

    let info_bytes = info.serialize_bytes()?;
    writer.start_file("ComicInfo.xml", options)?;
    writer.write_all(&info_bytes)?;
    writer.finish()?;
    std::fs::rename(&tmp, target)?;
    Ok(provider.page_count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compression_levels_match_the_c_sharp_switch() {
        assert_eq!(compression_level(ExportCompression::Store), 0);
        assert_eq!(compression_level(ExportCompression::Medium), 5);
        assert_eq!(compression_level(ExportCompression::Strong), 9);
    }
}
