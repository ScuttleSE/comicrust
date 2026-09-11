//! Provider framework — port of `IComicAccessor.cs`,
//! `ProviderImageInfo.cs`, `ArchiveComicProvider.cs` (the page
//! enumeration and ordering semantics), and `ComicProvider.cs` (the
//! supported-image filter).

use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};
use crate::extended_compare::extended_compare_ignore_case;
use crate::formats::{self, FileFormat};
use crate::hash;

/// Port of `ProviderImageInfo` — one candidate page inside a source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderImageInfo {
    /// The accessor's native entry index (zip: central-directory
    /// order; tar: always 0, names are the key — as in the C#).
    pub index: usize,
    pub name: String,
    pub size: u64,
}

impl ProviderImageInfo {
    pub fn new(index: usize, name: impl Into<String>, size: u64) -> Self {
        ProviderImageInfo {
            index,
            name: name.into(),
            size,
        }
    }
}

/// Port of `IComicAccessor` — the per-format archive access layer.
/// Info read/write methods land with Phase 1 T2.
pub trait ComicAccessor {
    /// `IComicAccessor.IsFormat` — fast signature check.
    fn is_format(&self, source: &Path) -> bool;

    /// `IComicAccessor.GetEntryList` — every candidate entry, in
    /// accessor-native order; the provider filters and sorts.
    fn get_entry_list(&self, source: &Path) -> Result<Vec<ProviderImageInfo>>;

    /// `IComicAccessor.ReadByteImage` — decompressed bytes of one
    /// entry, looked up by name; `None` on any failure (the C#
    /// engines swallow exceptions into `null`).
    fn read_byte_image(&self, source: &Path, info: &ProviderImageInfo) -> Option<Vec<u8>>;

    /// `IComicAccessor.ReadInfo` data access — raw bytes of a named
    /// entry (the `DeserializeAll` delegate). Name matching follows
    /// the C# engines: zip uses a case-insensitive full-name search,
    /// tar matches the entry basename case-insensitively, 7z passes
    /// the exact name to `e -so`. PDF/DjVu return `None` (their C#
    /// ReadInfo is null).
    fn read_info_file(&self, source: &Path, filename: &str) -> Option<Vec<u8>>;
}

/// The image extensions `ComicProvider.supportedTypes` accepts, in
/// source order. Stored with the leading dot the C# comparisons use
/// (`fileExt == "." + ext`).
const SUPPORTED_TYPES: &[&str] = &[
    ".jpg", ".jpeg", ".jif", ".jiff", ".gif", ".png", ".tif", ".tiff", ".bmp", ".djvu", ".webp",
    ".heic", ".heif", ".avif", ".jp2", ".j2k", ".jxl",
];

/// Port of `ComicProvider.IsSupportedImage` + `IsImageThumbnailFolder`.
fn is_supported_image(name: &str) -> bool {
    // C# checks for ".DS_Store\" and "__MACOSX\" substrings —
    // backslashes included, so plain ".DS_Store" files still pass.
    if name.contains(".DS_Store\\") || name.contains("__MACOSX\\") {
        return false;
    }
    let Some(ext) = formats::path_extension(Path::new(name)) else {
        return false;
    };
    SUPPORTED_TYPES.iter().any(|t| ext.eq_ignore_ascii_case(t))
}

/// The pseudo format for directory comics (`KnownFileFormats.FOLDER`,
/// "Image Folder" in the C# writer registry). Matched by
/// `ComicProvider::open` for directories, not by extension.
pub const FOLDER_FORMAT: FileFormat = FileFormat {
    name: "Image Folder",
    id: 100,
    extensions: &[],
    supports_update: false,
    dynamic: false,
};

/// What [`ComicProvider::open_with_report`] found out about a source
/// while opening it. The scanner turns this into the per-book scan
/// status (the red "!" and amber "≠" thumbnail chips).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OpenReport {
    /// The format the FILE NAME claimed, when the extension is known.
    pub extension_format: Option<&'static FileFormat>,
    /// The format the file CONTENT proved, when a signature matched.
    pub detected_format: Option<&'static FileFormat>,
    /// The content format contradicts the extension, and the content
    /// won (a RAR archive named `.cbz`, for example).
    pub mismatch: bool,
    /// The entry list could not be read. The file is in the library but
    /// unreadable; the text is the accessor's own message.
    pub entry_error: Option<String>,
}

impl OpenReport {
    /// True when the source opened with no complaint at all.
    pub fn is_clean(&self) -> bool {
        !self.mismatch && self.entry_error.is_none()
    }
}

/// Port of `ArchiveComicProvider` + `ImageProvider` page semantics,
/// driven by a `ComicAccessor`. Page index = position in the filtered,
/// naturally sorted list — the C# `foundImageList`.
pub struct ComicProvider {
    source: PathBuf,
    format: &'static FileFormat,
    pages: Vec<ProviderImageInfo>,
}
impl ComicProvider {
    /// Provider factory + open: pick the format by extension
    /// (`ProviderFactory.CreateSourceProvider`), build the accessor,
    /// and parse the page list (`ImageProvider.Open` + `OnParse`).
    ///
    /// Directories route to the folder accessor. The C# has no folder
    /// *reader* provider class (folder comics flow through the
    /// library's file-system lists); this gives the same observable
    /// page behavior under the `FOLDER` format id (100).
    ///
    /// PDF bypasses the archive page logic entirely
    /// (`PdfComicProvider` extends `ComicProvider`, not
    /// `ArchiveComicProvider`): the accessor's page list is used as-is.
    pub fn open(source: &Path) -> Result<ComicProvider> {
        Self::open_with_report(source).map(|(p, _)| p)
    }

    /// [`ComicProvider::open`] plus the [`OpenReport`] the scanner
    /// records.
    ///
    /// The extension picks the candidate format, exactly as the C#
    /// `ProviderFactory.GetSourceProviderType` does. Then the
    /// accessor's own `IsFormat` runs, and a failure sends the source
    /// through content detection — the port's form of
    /// `ImageProviderFactory.CreateSourceProvider`, which asks the
    /// other providers for a `FastFormatCheck` hit
    /// (ImageProviderFactory.cs:18-27). A RAR archive named `.cbz`
    /// therefore reads through the RAR accessor instead of driving the
    /// zip reader across the whole file.
    pub fn open_with_report(source: &Path) -> Result<(ComicProvider, OpenReport)> {
        let mut report = OpenReport::default();
        let (format, accessor): (&'static FileFormat, Box<dyn ComicAccessor>) = if source.is_dir() {
            (&FOLDER_FORMAT, Box::new(FolderAccessor))
        } else {
            let by_extension = formats::source_format(source)
                .ok_or_else(|| Error::UnsupportedFormat(source.to_path_buf()))?;
            report.extension_format = Some(by_extension);
            let accessor = crate::accessors::accessor_for(by_extension.id)
                .ok_or_else(|| Error::UnsupportedFormat(source.to_path_buf()))?;

            // `FastFormatCheck`: when the claimed reader recognizes the
            // content, keep it. Formats with no signature (tar) answer
            // from their own parse attempt.
            if accessor.is_format(source) {
                (by_extension, accessor)
            } else {
                match formats::detect_format(source) {
                    Some(id) if id != by_extension.id => {
                        let detected = formats::format_by_id(id);
                        report.detected_format = detected;
                        match detected
                            .and_then(|f| crate::accessors::accessor_for(f.id).map(|a| (f, a)))
                        {
                            Some((f, a)) => {
                                report.mismatch = true;
                                (f, a)
                            }
                            // Known signature, no ported reader: keep
                            // the claimed one and let it report.
                            None => (by_extension, accessor),
                        }
                    }
                    // No signature matched, or it agrees with the
                    // extension after all: keep the claimed reader.
                    _ => (by_extension, accessor),
                }
            }
        };
        let mut provider = ComicProvider {
            source: source.to_path_buf(),
            format,
            pages: Vec::new(),
        };
        report.entry_error = provider.parse(
            &*accessor,
            format.id == formats::ids::PDF || format.id == formats::ids::DJVU,
        );
        Ok((provider, report))
    }
    pub fn format(&self) -> &FileFormat {
        self.format
    }

    pub fn source(&self) -> &Path {
        &self.source
    }

    pub fn pages(&self) -> &[ProviderImageInfo] {
        &self.pages
    }

    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// `ArchiveComicProvider.GetFile` + `OnRetrieveSourceByteImage`,
    /// or `PdfComicProvider.OnRetrieveSourceByteImage`, including the
    /// normalize-to-JPEG conversion chain
    /// (`ImageProvider.RetrieveSourceByteImage`): WebP/JXL/HEIF/J2K
    /// page bytes become JPEG. Decoding itself is cr-image work; the
    /// DjVu/WebP conversions of the C# live in `cr_image::normalize_to_jpeg`.
    pub fn read_page(&self, index: usize) -> Option<Vec<u8>> {
        let info = self.pages.get(index)?;
        let accessor = if self.format.id == formats::ids::FOLDER {
            Box::new(FolderAccessor) as Box<dyn ComicAccessor>
        } else {
            crate::accessors::accessor_for(self.format.id)?
        };
        let raw = accessor.read_byte_image(&self.source, info)?;
        Some(cr_image::normalize_to_jpeg(&raw).unwrap_or(raw))
    }

    /// `ArchiveComicProvider.CreateHash` — the archive's cache key.
    /// PDF and DjVu override it with a SHA-1 of the whole file
    /// (`PdfComicProvider.CreateHash` / `DjvuComicProvider.CreateHash`).
    pub fn create_hash(&self) -> String {
        if self.format.id == formats::ids::PDF || self.format.id == formats::ids::DJVU {
            hash::file_hash(&self.source)
        } else {
            hash::create_hash_from_image_list(&self.pages)
        }
    }

    /// `ArchiveComicProvider.OnParse` / `PdfComicProvider.OnParse` /
    /// `DjvuComicProvider.OnParse`: archive sources take the
    /// accessor's entry list, keep supported images, and sort by
    /// natural order (`ExtendedStringComparer`, IgnoreCase); PDF and
    /// DjVu sources take the page list as-is.
    ///
    /// The C# swallows a broken archive into an empty page list. The
    /// port keeps that behavior (the page list stays empty) and ALSO
    /// returns the message, so the scanner can mark the book
    /// unreadable instead of storing a silent zero-page entry.
    fn parse(&mut self, accessor: &dyn ComicAccessor, raw_page_list: bool) -> Option<String> {
        let (entries, error) = match accessor.get_entry_list(&self.source) {
            Ok(entries) => (entries, None),
            Err(e) => (Vec::new(), Some(e.to_string())),
        };
        if raw_page_list {
            self.pages = entries;
            return error;
        }
        let mut list = entries
            .into_iter()
            .filter(|ii| is_supported_image(&ii.name))
            .collect::<Vec<_>>();
        list.sort_by(|a, b| extended_compare_ignore_case(&a.name, &b.name));
        self.pages = list;
        error
    }
}

/// Directory (folder-of-images) provider. The C# registers the folder
/// format for writing (`FolderStorageProvider`, "Image Folder") and
/// reads folders through the file-system lists; this accessor gives
/// Phase 1 the same read behavior: recursive enumeration, the same
/// supported-image filter, the same natural sort.
pub struct FolderAccessor;

impl ComicAccessor for FolderAccessor {
    fn is_format(&self, source: &Path) -> bool {
        source.is_dir()
    }

    fn get_entry_list(&self, source: &Path) -> Result<Vec<ProviderImageInfo>> {
        let mut entries = Vec::new();
        collect_files(source, source, 0, &mut entries)?;
        Ok(entries)
    }

    fn read_byte_image(&self, source: &Path, info: &ProviderImageInfo) -> Option<Vec<u8>> {
        fs::read(source.join(&info.name)).ok()
    }

    fn read_info_file(&self, source: &Path, filename: &str) -> Option<Vec<u8>> {
        // Case-insensitive directory scan (the C# folder flow matches
        // file names without case sensitivity on Windows).
        let wanted = filename.to_ascii_lowercase();
        for entry in fs::read_dir(source).ok()? {
            let entry = entry.ok()?;
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.to_ascii_lowercase() == wanted {
                return fs::read(entry.path()).ok();
            }
        }
        None
    }
}

/// Depth-first walk mirroring `Directory.GetFiles(...,
/// AllDirectories)`: files before subdirectories, alphabetical within
/// each directory. The final natural sort reorders everything anyway.
fn collect_files(
    root: &Path,
    dir: &Path,
    depth: usize,
    out: &mut Vec<ProviderImageInfo>,
) -> Result<()> {
    if depth > 64 {
        return Ok(()); // symlink/loop guard
    }
    let mut files: Vec<PathBuf> = Vec::new();
    let mut dirs: Vec<PathBuf> = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            dirs.push(path);
        } else {
            files.push(path);
        }
    }
    files.sort();
    dirs.sort();
    for path in files {
        let name = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .into_owned();
        if is_supported_image(&name) {
            let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            out.push(ProviderImageInfo::new(0, name, size));
        }
    }
    for dir in dirs {
        collect_files(root, &dir, depth + 1, out)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn supported_image_filter() {
        assert!(is_supported_image("page1.jpg"));
        assert!(is_supported_image("PAGE2.PNG"));
        assert!(is_supported_image("dir/sub/page3.webp"));
        assert!(!is_supported_image("ComicInfo.xml"));
        assert!(!is_supported_image("cover.txt"));
        assert!(!is_supported_image("noext"));
        // ".DS_Store" survives the thumbnail-folder substring check
        // (the C# literal has a backslash) but its leading-dot
        // "extension" is not an image type.
        assert!(!is_supported_image(".DS_Store"));
        assert!(!is_supported_image("a.DS_Store\\x.jpg"));
        assert!(!is_supported_image("__MACOSX\\page1.jpg"));
    }
}
