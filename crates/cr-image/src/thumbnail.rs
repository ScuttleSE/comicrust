//! Thumbnail rendering — port of `ThumbnailImage.CreateFrom` /
//! `GetThumbnail` (`IO/ThumbnailImage.cs`).
//!
//! A thumbnail is the image scaled to fit `MaxHeight` (512), stored as
//! JPEG at `ThumbnailQuality` (60, the `EngineConfiguration` default;
//! the C# resampling default is `FastBilinear`). The serialized form
//! (sizes + JPEG bytes, `Save`/`CreateFrom`) is a fresh format — the
//! C# cache files have no compat requirement (invariant #5).

use cr_core::model::comic_book::ComicBook;

use crate::error::Result;
use crate::resize::{fitted_size, scale, Resampling};
use crate::{decode, encode_jpeg, Image};

/// `ThumbnailImage.MaxHeight`.
pub const MAX_HEIGHT: u32 = 512;

/// `EngineConfiguration.Default.ThumbnailQuality`.
pub const THUMBNAIL_QUALITY: u8 = 60;

/// The default thumbnail resampling
/// (`EngineConfiguration.Default.ThumbnailResampling`).
pub const THUMBNAIL_RESAMPLING: Resampling = Resampling::FastBilinear;

/// A rendered thumbnail: the scaled image bytes plus the sizes the
/// C# stores.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Thumbnail {
    /// `Bitmap.Size` — the thumbnail's own size.
    pub size: (u32, u32),
    /// `OriginalSize` — the source page size.
    pub original_size: (u32, u32),
    /// JPEG bytes of the scaled image.
    pub data: Vec<u8>,
}

/// `ThumbnailImage.CreateFrom(image, originalSize)` — scale to
/// MaxHeight, JPEG-encode.
pub fn thumbnail_from_image(image: &Image, original_size: (u32, u32)) -> Result<Thumbnail> {
    let scaled = scale(image, (0, MAX_HEIGHT), THUMBNAIL_RESAMPLING)?;
    let data = encode_jpeg(&scaled, THUMBNAIL_QUALITY)?;
    Ok(Thumbnail {
        size: (scaled.width, scaled.height),
        original_size,
        data,
    })
}

impl Thumbnail {
    /// `ThumbnailImage.GetThumbnailSize` — size of the thumbnail at an
    /// arbitrary height (aspect-preserved, truncated).
    pub fn get_thumbnail_size(&self, height: u32) -> (u32, u32) {
        fitted_size(self.size, (0, height))
    }

    /// Serialized form (`ThumbnailImage.Save`): 4 i32 sizes + data.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(20 + self.data.len());
        for v in [
            self.size.0 as i32,
            self.size.1 as i32,
            self.original_size.0 as i32,
            self.original_size.1 as i32,
            self.data.len() as i32,
        ] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&self.data);
        out
    }

    /// `ThumbnailImage.CreateFrom(Stream)`.
    pub fn from_bytes(bytes: &[u8]) -> Result<Thumbnail> {
        if bytes.len() < 20 {
            return Err(crate::Error::Decode("thumbnail: truncated".into()));
        }
        let i32_at = |i: usize| {
            i32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]) as u32
        };
        let size = (i32_at(0), i32_at(4));
        let original_size = (i32_at(8), i32_at(12));
        let len = i32_at(16) as usize;
        let data = bytes[20..]
            .get(..len)
            .ok_or(crate::Error::Decode("thumbnail: data truncated".into()))?
            .to_vec();
        Ok(Thumbnail {
            size,
            original_size,
            data,
        })
    }

    /// Decoded thumbnail bitmap.
    pub fn image(&self) -> Result<Image> {
        decode(&self.data)
    }
}

/// The page the engine uses for a book thumbnail: the current page's
/// front cover logic lands in Phase 2; for now the cover page pick is
/// page 0, matching `ComicBook.GetCoverPageKey` for books without
/// proposed covers. Kept here so T4 cache keys can build on it.
pub fn cover_page_index(_book: &ComicBook) -> usize {
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnail_roundtrip_bytes() {
        let img = Image::new(8, 8, vec![200; 8 * 8 * 4]).unwrap();
        let thumb = thumbnail_from_image(&img, (8, 8)).unwrap();
        // The C# Scale also scales UP (no OnlyShrink): 8px → 512px.
        assert_eq!(thumb.size, (512, 512));

        let bytes = thumb.to_bytes();
        let parsed = Thumbnail::from_bytes(&bytes).unwrap();
        assert_eq!(parsed, thumb);

        let image = parsed.image().unwrap();
        assert_eq!((image.width, image.height), (512, 512));
    }

    #[test]
    fn thumbnail_scales_to_max_height() {
        let img = Image::new(4096, 2048, vec![10; 4096 * 2048 * 4]).unwrap();
        let thumb = thumbnail_from_image(&img, (4096, 2048)).unwrap();
        assert_eq!(thumb.size, (1024, 512));
        assert_eq!(thumb.original_size, (4096, 2048));
    }
}
