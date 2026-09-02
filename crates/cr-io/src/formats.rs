//! File format registry — port of `KnownFileFormats.cs`,
//! `FileFormat.cs`, `FileFormatAttribute.cs`, and the extension lookup
//! of `ProviderFactory.cs`.
//!
//! The C# factory discovers reader providers by reflection; the
//! registration order is therefore unspecified there. Overlapping
//! extensions route to the first registered format. We fix a
//! deterministic order (see [`FORMATS`]); `.cbr`/`.rar` map to the CBR
//! format first, but both CBR and RAR5 route to the same RAR accessor,
//! so observable behavior is unchanged.

use std::path::Path;

/// Format ids — `KnownFileFormats.cs`.
pub mod ids {
    pub const PDF: i32 = 1;
    pub const CBZ: i32 = 2;
    pub const CBR: i32 = 3;
    pub const XML: i32 = 4;
    pub const CBT: i32 = 5;
    pub const CB7: i32 = 6;
    pub const CBW: i32 = 7;
    pub const DJVU: i32 = 8;
    pub const RAR5: i32 = 9;
    pub const FOLDER: i32 = 100;
}

/// A file format — port of `FileFormat.cs` (the fields the engine
/// uses; shell registration and icon id are Windows-only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFormat {
    pub name: &'static str,
    pub id: i32,
    pub extensions: &'static [&'static str],
    /// `FileFormatAttribute(EnableUpdate = true)` — metadata write-back
    /// is allowed for this format.
    pub supports_update: bool,
    /// `FileFormatAttribute(Dynamic = true)` — content can change
    /// without a file rewrite (web comics).
    pub dynamic: bool,
}

impl FileFormat {
    /// `FileFormat.HasExtension` — case-insensitive comparison.
    pub fn has_extension(&self, extension: &str) -> bool {
        self.extensions
            .iter()
            .any(|ext| extension.eq_ignore_ascii_case(ext))
    }

    /// `FileFormat.Supports` — extension match on the source path.
    pub fn supports(&self, source: &Path) -> bool {
        path_extension(source)
            .map(|ext| self.has_extension(&ext))
            .unwrap_or(false)
    }
}

/// `.NET Path.GetExtension` — includes the leading dot; a filename
/// that starts with a dot is all extension (".gitignore" rule). Uses
/// `None` when there is no dot after the last separator. Handles both
/// separators because archive entry names are not always native paths.
pub fn path_extension(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let sep = name.rfind(['/', '\\']).map_or(0, |i| i + 1);
    let after_sep = &name[sep..];
    after_sep
        .rfind('.')
        .map(|dot| after_sep[dot..].to_ascii_lowercase())
}

/// `KnownFileFormats.GetSignature` — leading byte signatures for the
/// fast format check (`FileBasedAccessor.IsFormat`).
pub fn signature(format: i32) -> Option<&'static [u8]> {
    match format {
        ids::CBZ => Some(b"PK"),
        ids::CB7 => Some(b"7z"),
        ids::CBR => Some(b"Rar!\x1a\x07\x00"),
        ids::RAR5 => Some(b"Rar!\x1a\x07\x01"),
        _ => None,
    }
}

/// The registered comic reader formats, in provider-factory order.
/// Ids, names, and extension lists come from the `FileFormatAttribute`s
/// on the C# provider classes.
pub const FORMATS: &[FileFormat] = &[
    FileFormat {
        name: "eComic (ZIP)",
        id: ids::CBZ,
        extensions: &[".cbz"],
        supports_update: true,
        dynamic: false,
    },
    FileFormat {
        name: "ZIP Archive",
        id: ids::CBZ,
        extensions: &[".zip"],
        supports_update: false,
        dynamic: false,
    },
    FileFormat {
        name: "eComic (TAR)",
        id: ids::CBT,
        extensions: &[".cbt"],
        supports_update: true,
        dynamic: false,
    },
    FileFormat {
        name: "TAR Archive",
        id: ids::CBT,
        extensions: &[".tar"],
        supports_update: false,
        dynamic: false,
    },
    FileFormat {
        name: "eComic (7z)",
        id: ids::CB7,
        extensions: &[".cb7"],
        supports_update: true,
        dynamic: false,
    },
    FileFormat {
        name: "7z Archive",
        id: ids::CB7,
        extensions: &[".7z"],
        supports_update: false,
        dynamic: false,
    },
    FileFormat {
        name: "eComic (RAR)",
        id: ids::CBR,
        extensions: &[".cbr"],
        supports_update: false,
        dynamic: false,
    },
    FileFormat {
        name: "RAR Archive",
        id: ids::CBR,
        extensions: &[".rar"],
        supports_update: false,
        dynamic: false,
    },
    FileFormat {
        name: "eComic (RAR5)",
        id: ids::RAR5,
        extensions: &[".cbr"],
        supports_update: false,
        dynamic: false,
    },
    FileFormat {
        name: "RAR5 Archive",
        id: ids::RAR5,
        extensions: &[".rar"],
        supports_update: false,
        dynamic: false,
    },
    FileFormat {
        name: "PDF Document (PDF)",
        id: ids::PDF,
        extensions: &[".pdf"],
        supports_update: false,
        dynamic: false,
    },
    // DjVu and web-comic providers land with their accessors;
    // see docs/phase-1-kickoff.md T1.
];

/// `ProviderFactory.GetSourceProviderInfo` — first registered format
/// whose extensions match.
pub fn source_format(source: &Path) -> Option<&'static FileFormat> {
    FORMATS.iter().find(|f| f.supports(source))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_lookup() {
        let f = source_format(Path::new("x/Some Comic.CBZ")).unwrap();
        assert_eq!(f.name, "eComic (ZIP)");
        assert!(f.supports_update);

        let f = source_format(Path::new("plain.zip")).unwrap();
        assert_eq!(f.name, "ZIP Archive");

        let f = source_format(Path::new("book.rar")).unwrap();
        assert_eq!(f.id, ids::CBR);

        assert!(source_format(Path::new("book.txt")).is_none());
        assert!(source_format(Path::new("noext")).is_none());
    }

    #[test]
    fn signatures_match_the_c_sharp_bytes() {
        assert_eq!(signature(ids::CBZ), Some(&b"PK"[..]));
        assert_eq!(signature(ids::CB7), Some(&b"7z"[..]));
        assert_eq!(signature(ids::CBR), Some(b"Rar!\x1a\x07\x00".as_slice()));
        assert_eq!(signature(ids::RAR5), Some(b"Rar!\x1a\x07\x01".as_slice()));
        assert_eq!(signature(ids::CBT), None);
    }
}
