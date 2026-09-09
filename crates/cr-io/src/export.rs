//! The export settings — the `ExportSetting`/`StorageSetting` port
//! (enums verbatim from the C# `IO/*.cs` files; the defaults are the
//! C# `[DefaultValue]`s).

use std::io::Write;
use std::path::{Path, PathBuf};

use cr_core::model::comic_book::ComicBook;
use cr_core::model::comic_info::ComicInfo;

use crate::error::{Error, Result};
use crate::provider::ComicProvider;

/// `ExportTarget`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ExportTarget {
    #[default]
    NewFolder,
    SameAsSource,
    ReplaceSource,
    Ask,
}

/// `ExportNaming`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ExportNaming {
    #[default]
    Filename,
    Caption,
    Custom,
}

/// `StoragePageType` (the C# 64-bit list; Jpeg2000 commented out
/// there too).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StoragePageType {
    #[default]
    Original,
    Jpeg,
    Png,
    Gif,
    Tiff,
    Bmp,
    Djvu,
    Webp,
    Heif,
    Avif,
    JpegXl,
}

/// `StoragePageResize`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum StoragePageResize {
    #[default]
    Original,
    WidthHeight,
    Width,
    Height,
}

/// `DoublePageHandling`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum DoublePageHandling {
    #[default]
    Keep,
    Split,
    Rotate,
    AdaptWidth,
}

/// `ExportImageProcessingSource`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ExportImageProcessingSource {
    #[default]
    Custom,
    FromComic,
}

/// `ExportCompression` (moved from the skeleton; the C# values).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ExportCompression {
    #[default]
    None,
    Medium,
    Strong,
}

/// `ExportSetting` + `StorageSetting` — the fields the export
/// pipeline consumes. Defaults are the C# `[DefaultValue]`s.
#[derive(Clone, Debug)]
pub struct ExportSetting {
    pub target: ExportTarget,
    pub target_folder: String,
    pub delete_original: bool,
    pub add_to_library: bool,
    pub overwrite: bool,
    pub combine: bool,
    pub naming: ExportNaming,
    pub custom_name: String,
    pub custom_naming_start: i32,
    /// The writer format id (2 = CBZ, the C# fallback).
    pub format_id: i32,
    pub comic_compression: ExportCompression,
    pub embed_comic_info: bool,
    pub embed_comic_book: bool,
    pub remove_page_filter: i32,
    pub include_pages: String,
    pub page_type: StoragePageType,
    /// 0..100 (`tbQuality`).
    pub page_compression: i32,
    pub lossless: bool,
    pub page_resize: StoragePageResize,
    pub page_width: i32,
    pub page_height: i32,
    pub dont_enlarge: bool,
    pub double_pages: DoublePageHandling,
    pub ignore_error_pages: bool,
    pub keep_original_image_names: bool,
    pub image_processing_source: ExportImageProcessingSource,
    pub tags_to_append: Option<String>,
    /// `ForcedName` (the sync path; empty normally).
    pub forced_name: String,
}

impl Default for ExportSetting {
    fn default() -> Self {
        ExportSetting {
            target: ExportTarget::NewFolder,
            target_folder: String::new(),
            delete_original: false,
            add_to_library: false,
            overwrite: false,
            combine: false,
            naming: ExportNaming::Filename,
            custom_name: String::new(),
            custom_naming_start: 1,
            format_id: 0,
            comic_compression: ExportCompression::None,
            embed_comic_info: true,
            embed_comic_book: false,
            remove_page_filter: 0,
            include_pages: String::new(),
            page_type: StoragePageType::Original,
            page_compression: 0,
            lossless: false,
            page_resize: StoragePageResize::Original,
            page_width: 0,
            page_height: 0,
            dont_enlarge: false,
            double_pages: DoublePageHandling::Keep,
            ignore_error_pages: false,
            keep_original_image_names: false,
            image_processing_source: ExportImageProcessingSource::Custom,
            tags_to_append: None,
            forced_name: String::new(),
        }
    }
}

/// `ExportSetting.GetTargetFilePath`: SameAsSource/ReplaceSource
/// keep the source directory; NewFolder uses the setting's folder.
pub fn target_file_path(setting: &ExportSetting, book: &ComicBook) -> PathBuf {
    if setting.target != ExportTarget::NewFolder {
        Path::new(&book.file_path)
            .parent()
            .map(PathBuf::from)
            .unwrap_or_default()
    } else {
        PathBuf::from(&setting.target_folder)
    }
}

/// `FileUtility.MakeValidFilename` (the C# replaces the invalid
/// characters with spaces).
pub fn make_valid_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => ' ',
            c if c.is_control() => ' ',
            c => c,
        })
        .collect()
}

/// `ExportSetting.GetTargetFileName`: the naming template over the
/// book, with the writer format's main extension. `caption` is the
/// book's formatted caption (the Caption naming form).
pub fn target_file_name(
    setting: &ExportSetting,
    book: &ComicBook,
    caption: &str,
    index: usize,
    main_extension: &str,
) -> String {
    if !setting.forced_name.is_empty() {
        return format!("{}{main_extension}", setting.forced_name);
    }
    let text = match setting.naming {
        ExportNaming::Caption => {
            if setting.custom_name.is_empty() {
                caption.to_string()
            } else {
                // `cb.GetFullTitle(CustomName)` — the custom format
                // rendered by the same engine (the caller resolves).
                setting.custom_name.clone()
            }
        }
        ExportNaming::Custom => {
            let mut text = if setting.custom_name.is_empty() {
                file_name_of(&book.file_path)
            } else {
                setting.custom_name.clone()
            };
            let index = index as i32 + setting.custom_naming_start;
            if index > 0 {
                text = format!("{text} ({index})");
            }
            text
        }
        ExportNaming::Filename => file_name_of(&book.file_path),
    };
    format!("{}{main_extension}", make_valid_filename(&text))
}

/// `ExportSetting.GetTargetPath`.
pub fn target_path(
    setting: &ExportSetting,
    book: &ComicBook,
    caption: &str,
    index: usize,
    main_extension: &str,
) -> PathBuf {
    target_file_path(setting, book).join(target_file_name(
        setting,
        book,
        caption,
        index,
        main_extension,
    ))
}

fn file_name_of(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

/// The main extension for a writer format id (the C# `FileFormat`):
/// 2 = CBZ (the fallback), 3 = CBT, 4 = CB7.
pub fn main_extension_for_format(format_id: i32) -> &'static str {
    match format_id {
        3 => ".cbt",
        4 => ".cb7",
        _ => ".cbz",
    }
}

/// The C# `Providers.Writers.GetSourceFormats()` order (sorted by
/// format): the ids the export dialog offers.
pub const EXPORT_FORMATS: [(i32, &str); 3] = [
    (2, "eComic (ZIP)"),
    (3, "eComic (TAR)"),
    (4, "eComic (7Zip)"),
];

/// Exports ONE book per the settings (the sequential core of the
/// C# `ExportComicQueue` page loop; the parallel/spill machinery is
/// deliberately single-threaded here — a local export does not need
/// it). Returns the page count written and the output path (the C#
/// `comicExporter.Export` return, consumed by the
/// `QueueManager.ExportComic` post-export block).
pub fn export_book(
    setting: &ExportSetting,
    book: &ComicBook,
    caption: &str,
    index: usize,
    progress: &dyn Fn(usize, usize),
) -> Result<(usize, PathBuf)> {
    let source = Path::new(&book.file_path);
    let provider = ComicProvider::open(source)?;

    let target = target_path(
        setting,
        book,
        caption,
        index,
        main_extension_for_format(setting.format_id),
    );
    if target.exists() && !setting.overwrite {
        return Err(Error::Access(format!(
            "target exists: {}",
            target.display()
        )));
    }
    if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir)?;
    }

    let info = build_export_info(setting, book);
    let pages = pack_pages(setting, &provider, &info, &target, progress)?;
    Ok((pages, target))
}

/// Combine variant: all books merge into ONE archive at
/// `target_path` of the FIRST book (the C#
/// `QueueManager.ExportComic(books, ...)` combine branch; natural
/// page order carries over — pages append). Returns the page count
/// written and the output path.
pub fn export_books_combined(
    setting: &ExportSetting,
    books: &[ComicBook],
    captions: &[String],
    progress: &dyn Fn(usize, usize),
) -> Result<(usize, PathBuf)> {
    let Some(first) = books.first() else {
        return Ok((0, PathBuf::new()));
    };
    let target = target_path(
        setting,
        first,
        captions.first().map(String::as_str).unwrap_or(""),
        0,
        main_extension_for_format(setting.format_id),
    );
    if target.exists() && !setting.overwrite {
        return Err(Error::Access(format!(
            "target exists: {}",
            target.display()
        )));
    }
    if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir)?;
    }

    // The combined ComicInfo merges the first book's info (the C#
    // uses the first book as the container's info source).
    let info = build_export_info(setting, first);
    let tmp = target.with_extension("export");
    let file = std::fs::File::create(&tmp)?;
    let mut writer = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default().compression_method(
        if setting.comic_compression == ExportCompression::None {
            zip::CompressionMethod::Stored
        } else {
            zip::CompressionMethod::Deflated
        },
    );

    let total: usize = books
        .iter()
        .map(|b| b.info.page_count.max(0) as usize)
        .sum();
    let mut done = 0usize;
    for book in books {
        let provider = ComicProvider::open(Path::new(&book.file_path))?;
        for (i, page) in provider.pages().iter().enumerate() {
            let ext = page
                .name
                .rsplit_once('.')
                .map(|(_, e)| format!(".{e}"))
                .unwrap_or_else(|| ".jpg".into());
            let data = provider
                .read_page(i)
                .ok_or_else(|| Error::Access(format!("page {i} failed to load")))?;
            let name = if setting.keep_original_image_names {
                page.name
                    .rsplit('/')
                    .next()
                    .unwrap_or(&page.name)
                    .to_string()
            } else {
                format!("Page{done:0>4}{ext}")
            };
            writer.start_file(name, options)?;
            writer.write_all(data.as_slice())?;
            done += 1;
            progress(done, total);
        }
    }

    let info_bytes = info.serialize_bytes()?;
    writer.start_file("ComicInfo.xml", options)?;
    writer.write_all(info_bytes.as_slice())?;
    writer.finish()?;
    std::fs::rename(&tmp, &target)?;
    Ok((done, target))
}

/// The exported ComicInfo (`ComicExporter.ComicInfo` — the book's
/// info + the tags appended); the post-export block sets this back
/// onto the book.
pub fn build_export_info(setting: &ExportSetting, book: &ComicBook) -> ComicInfo {
    let mut info = book.info.clone();
    if let Some(tags) = &setting.tags_to_append {
        if info.tags.is_empty() {
            info.tags = tags.clone();
        } else {
            info.tags = format!("{}, {}", info.tags, tags);
        }
    }
    info
}

/// The page packing for ONE book: Original = byte pass-through with
/// the original names; Jpeg/Png/WebP convert via cr-image (q from
/// page_compression); the exotic formats report unsupported (the
/// Phase 1 codec gaps).
fn pack_pages(
    setting: &ExportSetting,
    provider: &ComicProvider,
    info: &ComicInfo,
    target: &Path,
    progress: &dyn Fn(usize, usize),
) -> Result<usize> {
    let tmp = target.with_extension("export");
    match setting.format_id {
        4 => {
            return Err(Error::Access(
                "CB7 export needs the 7z subprocess writer (not ported yet)".into(),
            ));
        }
        3 => {
            // CBT: the same page loop, tar container.
            return pack_tar(setting, provider, info, target, progress);
        }
        _ => {}
    }

    let file = std::fs::File::create(&tmp)?;
    let mut writer = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default().compression_method(
        if setting.comic_compression == ExportCompression::None {
            zip::CompressionMethod::Stored
        } else {
            zip::CompressionMethod::Deflated
        },
    );

    let count = provider.page_count();
    for (i, page) in provider.pages().iter().enumerate() {
        let data = provider
            .read_page(i)
            .ok_or_else(|| Error::Access(format!("page {i} failed to load")))?;
        let original_ext = page
            .name
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        let (name_bytes, out_ext) = match setting.page_type {
            StoragePageType::Original => (data, original_ext),
            StoragePageType::Jpeg => (
                convert_page(&data, "jpeg", setting.page_compression),
                "jpg".into(),
            ),
            StoragePageType::Png => (convert_page(&data, "png", 0), "png".into()),
            StoragePageType::Webp => (
                convert_page(&data, "webp", setting.page_compression),
                "webp".into(),
            ),
            other => {
                return Err(Error::Access(format!(
                    "page format {other:?} not supported yet"
                )));
            }
        };
        let name = if setting.keep_original_image_names
            || setting.page_type == StoragePageType::Original
        {
            page.name
                .rsplit('/')
                .next()
                .unwrap_or(&page.name)
                .to_string()
        } else {
            format!("Page{i:0>4}.{out_ext}")
        };
        writer.start_file(name, options)?;
        writer.write_all(name_bytes.as_slice())?;
        progress(i + 1, count);
    }

    if setting.embed_comic_info {
        let info_bytes = info.serialize_bytes()?;
        writer.start_file("ComicInfo.xml", options)?;
        writer.write_all(info_bytes.as_slice())?;
    }
    writer.finish()?;
    std::fs::rename(&tmp, target)?;
    Ok(count)
}

fn pack_tar(
    setting: &ExportSetting,
    provider: &ComicProvider,
    info: &ComicInfo,
    target: &Path,
    progress: &dyn Fn(usize, usize),
) -> Result<usize> {
    let tmp = target.with_extension("export");
    let file = std::fs::File::create(&tmp)?;
    let mut builder = tar::Builder::new(file);
    let count = provider.page_count();
    for (i, page) in provider.pages().iter().enumerate() {
        let data = provider
            .read_page(i)
            .ok_or_else(|| Error::Access(format!("page {i} failed to load")))?;
        let name = if setting.keep_original_image_names {
            page.name
                .rsplit('/')
                .next()
                .unwrap_or(&page.name)
                .to_string()
        } else {
            let ext = page
                .name
                .rsplit_once('.')
                .map(|(_, e)| format!(".{e}"))
                .unwrap_or_else(|| ".jpg".into());
            format!("Page{i:0>4}{ext}")
        };
        let mut header = tar::Header::new_gnu();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        builder.append_data(&mut header, name, data.as_slice())?;
        progress(i + 1, count);
    }
    let info_bytes = info.serialize_bytes()?;
    let mut header = tar::Header::new_gnu();
    header.set_size(info_bytes.len() as u64);
    header.set_mode(0o644);
    builder.append_data(&mut header, "ComicInfo.xml", info_bytes.as_slice())?;
    builder.finish()?;
    drop(builder);
    std::fs::rename(&tmp, target)?;
    Ok(count)
}

/// The page conversion. cr-image carries the JPEG encoder only —
/// PNG/WebP requests re-encode to JPEG with the quality applied
/// (documented deviation; the encoders land with the codec work).
fn convert_page(data: &[u8], format: &str, quality: i32) -> Vec<u8> {
    match cr_image::decode::decode(data) {
        Ok(image) => {
            if format == "jpeg" || format == "png" || format == "webp" {
                cr_image::encode_jpeg(&image, quality.clamp(0, 100) as u8)
                    .unwrap_or_else(|_| data.to_vec())
            } else {
                data.to_vec()
            }
        }
        Err(_) => data.to_vec(),
    }
}

/// The writers registry subset (`Providers.Writers`): the formats
/// the export can write, by id. Used by the dialog combo.
pub fn export_writers() -> &'static [(i32, &'static str)] {
    &EXPORT_FORMATS
}

/// `Providers.Writers.GetSourceFormats` fallback (the C# picks CBZ
/// when the format id does not resolve).
pub fn writer_format_name(format_id: i32, book_format: &str) -> String {
    EXPORT_FORMATS
        .iter()
        .find(|(id, _)| *id == format_id)
        .map(|(_, n)| n.to_string())
        .unwrap_or_else(|| format!("eComic ({book_format})"))
}

#[cfg(test)]
mod setting_tests {
    use super::*;

    #[test]
    fn target_paths_follow_the_naming_template() {
        let setting = ExportSetting {
            target_folder: "/out".into(),
            naming: ExportNaming::Filename,
            ..ExportSetting::default()
        };
        let book = ComicBook {
            file_path: "/comics/Batman 001.cbz".into(),
            ..ComicBook::default()
        };

        // Filename naming: the source stem + the writer extension.
        assert_eq!(
            target_path(&setting, &book, "", 0, ".cbz"),
            PathBuf::from("/out/Batman 001.cbz")
        );

        // Caption naming: the formatted caption.
        let setting = ExportSetting {
            naming: ExportNaming::Caption,
            ..setting.clone()
        };
        assert_eq!(
            target_path(&setting, &book, "Batman Vol.1 #1", 0, ".cbz"),
            PathBuf::from("/out/Batman Vol.1 #1.cbz")
        );

        // Custom naming: the custom text + the (start+index) suffix.
        let setting = ExportSetting {
            naming: ExportNaming::Custom,
            custom_name: "Export".into(),
            custom_naming_start: 1,
            ..setting.clone()
        };
        assert_eq!(
            target_path(&setting, &book, "", 0, ".cbz"),
            PathBuf::from("/out/Export (1).cbz")
        );
    }

    #[test]
    fn same_as_source_keeps_the_source_directory() {
        let setting = ExportSetting {
            target: ExportTarget::SameAsSource,
            ..ExportSetting::default()
        };
        let book = ComicBook {
            file_path: "/comics/Batman 001.cbz".into(),
            ..ComicBook::default()
        };
        assert_eq!(target_file_path(&setting, &book), PathBuf::from("/comics"));
    }

    #[test]
    fn invalid_filename_characters_become_spaces() {
        assert_eq!(
            make_valid_filename("a<b>c:d\"e/f\\g|h?i*j"),
            "a b c d e f g h i j"
        );
    }
}

#[cfg(test)]
mod engine_tests {
    use super::*;
    use std::io::Write as IoWrite;

    /// Builds a tiny source CBZ with two JPEG pages.
    fn make_source(dir: &std::path::Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        let file = std::fs::File::create(&path).unwrap();
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for i in 0..2 {
            let image = cr_image::Image::new(4, 4, vec![128; 4 * 4 * 4]).unwrap();
            let jpeg = cr_image::encode_jpeg(&image, 75).unwrap();
            writer.start_file(format!("{i:0>4}.jpg"), options).unwrap();
            writer.write_all(jpeg.as_slice()).unwrap();
        }
        writer.finish().unwrap();
        path
    }

    #[test]
    fn export_book_copies_pages_and_writes_info() {
        use std::io::Read;
        let dir = std::env::temp_dir().join(format!(
            "comicrust-export-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let out_dir = dir.join("out");
        std::fs::create_dir_all(&out_dir).unwrap();
        let source = make_source(&dir, "src.cbz");

        let book = ComicBook {
            file_path: source.to_string_lossy().into_owned(),
            info: ComicInfo {
                page_count: 2,
                series: "Export Probe".into(),
                ..ComicInfo::default()
            },
            ..ComicBook::default()
        };

        let setting = ExportSetting {
            target_folder: out_dir.to_string_lossy().into_owned(),
            ..ExportSetting::default()
        };
        let (pages, out_path) =
            export_book(&setting, &book, "probe caption", 0, &|_, _| {}).expect("export");
        assert_eq!(pages, 2);

        let target = out_dir.join("src.cbz");
        assert_eq!(out_path, target, "the output path is reported");
        assert!(target.exists(), "the exported file exists");
        let file = std::fs::File::open(&target).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(names.contains(&"0000.jpg".to_string()), "{names:?}");
        assert!(names.contains(&"ComicInfo.xml".to_string()));
        let mut entry = archive.by_name("ComicInfo.xml").unwrap();
        let mut info = Vec::new();
        entry.read_to_end(&mut info).unwrap();
        let info = String::from_utf8(info).unwrap();
        assert!(info.contains("Export Probe"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
