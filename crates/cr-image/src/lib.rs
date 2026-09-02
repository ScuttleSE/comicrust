//! Image currency type, decode/encode pipeline, resize and color
//! adjust filters, thumbnail rendering (Phase 1 T3, port plan).
//!
//! The C# specification is `cYo.Common/Drawing/`
//! (`ImageProcessing.cs`, `BitmapExtensions.cs`, `JpegFile.cs`) and
//! `ComicRack.Engine/IO/ThumbnailImage.cs`. The image currency here is
//! an 8-bit RGBA buffer (`Image`), standing in for the 32bpp ARGB
//! `System.Drawing.Bitmap` the C# code uses everywhere.
//!
//! Documented tolerances against GDI+:
//! - Resampling uses the `image` crate filters (Triangle for the C#
//!   bilinear modes, CatmullRom for bicubic/HQ); pixel results differ
//!   from GDI+ within the phase's documented tolerance.
//! - The color-adjust matrix math is ported exactly (row-vector
//!   convention with the luma weights 0.3086/0.6094/0.0820), but
//!   GDI+'s internal 8-bit rounding differs slightly.

pub mod adjust;
pub mod decode;
pub mod disk;
pub mod error;
pub mod keys;
pub mod memory;
pub mod resize;
pub mod thumbnail;

pub use decode::{decode, encode_jpeg, normalize_to_jpeg, ImageFormat};
pub use error::{Error, Result};
pub use thumbnail::{thumbnail_from_image, MAX_HEIGHT, THUMBNAIL_QUALITY};

/// The image currency — 8-bit RGBA, row-major, no row padding (the
/// `System.Drawing.Bitmap` stand-in; the C# standard pixel format is
/// 32bpp ARGB).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` bytes, RGBA order.
    pub rgba: Vec<u8>,
}

impl Image {
    pub fn new(width: u32, height: u32, rgba: Vec<u8>) -> Result<Image> {
        if rgba.len() != width as usize * height as usize * 4 {
            return Err(Error::BadBufferSize {
                len: rgba.len(),
                expected: width as usize * height as usize * 4,
            });
        }
        Ok(Image {
            width,
            height,
            rgba,
        })
    }
}
