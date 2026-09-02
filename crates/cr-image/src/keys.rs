//! Cache keys — port of `IO/ImageKey.cs`, `IO/PageKey.cs`, and
//! `IO/ThumbnailKey.cs`. These keys identify cached pages and
//! thumbnails; Phase 2 engine caches build on them, so the identity
//! semantics (location + size + modified time + index + rotation,
//! page keys adding the bitmap adjustment) must stay stable.

use cr_core::model::bitmap_adjustment::BitmapAdjustment;
use cr_core::model::enums::ImageRotation;

/// `ImageKey` — the identity of a cached image.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ImageKey {
    /// The provider source (the file path for file comics; a URL or
    /// id for dynamic sources).
    pub source: String,
    /// The file location (`Location`).
    pub location: String,
    /// File size in bytes (`GetSafeSize`).
    pub size: u64,
    /// Last write time, UTC seconds (`GetSafeModifiedTime` —
    /// `DateTime.MinValue` on error is encoded as 0).
    pub modified: i64,
    /// Page index inside the source.
    pub index: usize,
    pub rotation: ImageRotation,
}

impl ImageKey {
    pub fn new(
        source: impl Into<String>,
        location: impl Into<String>,
        size: u64,
        modified: i64,
        index: usize,
        rotation: ImageRotation,
    ) -> ImageKey {
        ImageKey {
            source: source.into(),
            location: location.into(),
            size,
            modified,
            index,
            rotation,
        }
    }

    /// `ThumbnailKey(source, file, index, rotation)` — size and
    /// modified time read from the file (safe defaults on error).
    pub fn from_file(
        source: impl Into<String>,
        file: &std::path::Path,
        index: usize,
        rotation: ImageRotation,
    ) -> ImageKey {
        let (size, modified) = file_stats(file);
        ImageKey {
            source: source.into(),
            location: file.to_string_lossy().into_owned(),
            size,
            modified,
            index,
            rotation,
        }
    }

    /// `ImageKey.IsSameFile`.
    pub fn is_same_file(&self, location: &str, size: u64, modified: i64) -> bool {
        self.location == location && self.size == size && self.modified == modified
    }
}

/// `ImageKey.GetSafeSize` / `GetSafeModifiedTime` — 0 encodes the
/// C# `MinValue` error fallback.
pub fn file_stats(file: &std::path::Path) -> (u64, i64) {
    match std::fs::metadata(file) {
        Ok(meta) => {
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            (meta.len(), modified)
        }
        Err(_) => (0, 0),
    }
}

/// `PageKey` — an `ImageKey` plus the applied `BitmapAdjustment`.
#[derive(Clone, Debug, PartialEq)]
pub struct PageKey {
    pub key: ImageKey,
    pub adjustment: BitmapAdjustment,
}

impl PageKey {
    pub fn new(key: ImageKey, adjustment: BitmapAdjustment) -> PageKey {
        PageKey { key, adjustment }
    }
}

/// `ThumbnailKey` source kinds (`ResourceKey`/`FileKey`/`CustomKey`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub enum ThumbnailSource {
    /// A plain file-backed thumbnail (the common case).
    #[default]
    File,
    /// A `type://resource` locator (`rxResource` in the C#).
    Resource {
        resource_type: String,
        resource_location: String,
    },
    /// A user-set custom thumbnail.
    Custom,
}

/// `ThumbnailKey` — an `ImageKey` plus the thumbnail source kind.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ThumbnailKey {
    pub key: ImageKey,
    pub source_kind: ThumbnailSource,
}

impl ThumbnailKey {
    pub fn new(key: ImageKey) -> ThumbnailKey {
        ThumbnailKey {
            key,
            source_kind: ThumbnailSource::File,
        }
    }

    /// `ThumbnailKey.CalcResource` — parse `type://location` locators
    /// (the C# regex: `\A\s*(?<type>[a-z]{4,}):\\\\\\(?<resource>.*)`).
    pub fn with_locator(key: ImageKey) -> ThumbnailKey {
        let location = key.location.clone();
        if let Some((resource_type, resource_location)) = location
            .split_once(":\\\\")
            .filter(|(t, _)| t.len() >= 4 && t.chars().all(|c| c.is_ascii_alphabetic()))
        {
            return ThumbnailKey {
                key,
                source_kind: ThumbnailSource::Resource {
                    resource_type: resource_type.to_ascii_lowercase(),
                    resource_location: resource_location.to_string(),
                },
            };
        }
        ThumbnailKey::new(key)
    }
}

// ---------- queue identity ----------
//
// The ProcessingQueue de-duplicates by key; PageKey carries the
// BitmapAdjustment with f32 fields, so Eq/Hash compare the floats by
// bit pattern (field equality, like the C# Equals).

impl Eq for PageKey {}

impl std::hash::Hash for PageKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.key.hash(state);
        self.adjustment.saturation.to_bits().hash(state);
        self.adjustment.contrast.to_bits().hash(state);
        self.adjustment.brightness.to_bits().hash(state);
        self.adjustment.gamma.to_bits().hash(state);
        self.adjustment.white_point_argb.hash(state);
        self.adjustment.options.hash(state);
        self.adjustment.sharpen.hash(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_same_file_matches_all_parts() {
        let key = ImageKey::new("s", "/a.cbz", 100, 12345, 0, ImageRotation::None);
        assert!(key.is_same_file("/a.cbz", 100, 12345));
        assert!(!key.is_same_file("/a.cbz", 101, 12345));
        assert!(!key.is_same_file("/b.cbz", 100, 12345));
    }

    #[test]
    fn page_key_includes_adjustment() {
        let key = ImageKey::new("s", "/a.cbz", 100, 1, 0, ImageRotation::None);
        let adj = BitmapAdjustment {
            brightness: 0.5,
            ..Default::default()
        };
        let plain = PageKey::new(key.clone(), BitmapAdjustment::default());
        let adjusted = PageKey::new(key, adj);
        assert_ne!(plain, adjusted);
    }

    #[test]
    fn locator_parsing() {
        // The C# locator form: type + colon + two backslashes.
        let key = ImageKey::new("s", "resource:\\\\some\\path", 0, 0, 0, ImageRotation::None);
        let tk = ThumbnailKey::with_locator(key);
        match tk.source_kind {
            ThumbnailSource::Resource {
                resource_type,
                resource_location,
            } => {
                assert_eq!(resource_type, "resource");
                assert_eq!(resource_location, "some\\path");
            }
            _ => panic!("expected resource locator"),
        }
        // Short type names do not match the `[a-z]{4,}` rule.
        let key = ImageKey::new("s", "res:\\\\some\\path", 0, 0, 0, ImageRotation::None);
        assert_eq!(
            ThumbnailKey::with_locator(key).source_kind,
            ThumbnailSource::File
        );
        // Plain paths stay File.
        let key = ImageKey::new("s", "/comics/a.cbz", 0, 0, 0, ImageRotation::None);
        assert_eq!(
            ThumbnailKey::with_locator(key).source_kind,
            ThumbnailSource::File
        );
    }
}
