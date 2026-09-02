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
        let (format, accessor): (&'static FileFormat, Box<dyn ComicAccessor>) = if source.is_dir() {
            (&FOLDER_FORMAT, Box::new(FolderAccessor))
        } else {
            let format = formats::source_format(source)
                .ok_or_else(|| Error::UnsupportedFormat(source.to_path_buf()))?;
            let accessor = crate::accessors::accessor_for(format.id)
                .ok_or_else(|| Error::UnsupportedFormat(source.to_path_buf()))?;
            (format, accessor)
        };
        let mut provider = ComicProvider {
            source: source.to_path_buf(),
            format,
            pages: Vec::new(),
        };
        provider.parse(
            &*accessor,
            format.id == formats::ids::PDF || format.id == formats::ids::DJVU,
        );
        Ok(provider)
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
    /// or `PdfComicProvider.OnRetrieveSourceByteImage`. The
    /// DjVu/WebP/HEIF/J2K/JXL normalize-to-JPEG conversion chain lands
    /// with the cr-image decode work (T3).
    pub fn read_page(&self, index: usize) -> Option<Vec<u8>> {
        let info = self.pages.get(index)?;
        if self.format.id == formats::ids::FOLDER {
            return FolderAccessor.read_byte_image(&self.source, info);
        }
        crate::accessors::accessor_for(self.format.id)?.read_byte_image(&self.source, info)
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
    fn parse(&mut self, accessor: &dyn ComicAccessor, raw_page_list: bool) {
        let entries = accessor.get_entry_list(&self.source).unwrap_or_default();
        if raw_page_list {
            self.pages = entries;
            return;
        }
        let mut list = entries
            .into_iter()
            .filter(|ii| is_supported_image(&ii.name))
            .collect::<Vec<_>>();
        list.sort_by(|a, b| extended_compare_ignore_case(&a.name, &b.name));
        self.pages = list;
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
