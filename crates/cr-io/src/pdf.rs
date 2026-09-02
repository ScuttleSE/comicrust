//! PDF accessor — port of `PdfComicProvider.cs` + `PdfiumReaderEngine.cs`.
//!
//! Differences from the archive providers (all taken from the C#):
//! the page list is the PDF's own page order — no supported-image
//! filter and no natural sort; entries carry only an index; the
//! provider hash is the SHA-1 of the whole file; the fast format
//! check is the `%PDF` magic.
//!
//! Pages are rasterized with pdfium-render and encoded as JPEG,
//! matching the C# `Bitmap.ImageToBytes(ImageFormat.Jpeg)` output
//! shape. The render size follows `PdfiumReaderEngine.CalculateSize`:
//! portrait pages pin width to 1920, landscape pages pin height to
//! 2540 (the `EngineConfiguration.Default.PdfiumImageSize` defaults,
//! including its landscape-width ballooning behavior — ported as
//! written). JPEG quality is 75, the GDI+ default.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use jpeg_encoder::{ColorType, Encoder};
use pdfium_render::prelude::*;

use crate::error::{Error, Result};
use crate::provider::{ComicAccessor, ProviderImageInfo};

/// `EngineConfiguration.Default.PdfiumImageSize` defaults
/// (1920 is 8.5in at 225dpi, 2540 is 11in at 225dpi).
const MAX_WIDTH: f64 = 1920.0;
const MAX_HEIGHT: f64 = 2540.0;

/// GDI+ default JPEG quality.
const JPEG_QUALITY: u8 = 75;

pub struct PdfAccessor;

impl ComicAccessor for PdfAccessor {
    /// Port of `PdfComicProvider.OnFastFormatCheck`: `%PDF` magic,
    /// `true` on open errors.
    fn is_format(&self, source: &Path) -> bool {
        match File::open(source) {
            Err(_) => true,
            Ok(mut file) => {
                let mut head = [0u8; 4];
                match file.read(&mut head) {
                    Err(_) => true,
                    Ok(n) => n == 4 && head == *b"%PDF",
                }
            }
        }
    }

    /// `PdfiumReaderEngine.GetEntryList`: one index-only entry per
    /// page, in PDF order. Errors propagate (`OnParse` swallows them).
    fn get_entry_list(&self, source: &Path) -> Result<Vec<ProviderImageInfo>> {
        let count = open_document(source)
            .ok_or_else(|| Error::Access("pdfium could not open document".into()))?;
        Ok((0..count)
            .map(|i| ProviderImageInfo::new(i as usize, String::new(), 0))
            .collect())
    }

    /// `PdfiumReaderEngine.ReadByteImage`: render the page, return
    /// JPEG bytes; any failure is `None` (C# `return null`).
    fn read_byte_image(&self, source: &Path, info: &ProviderImageInfo) -> Option<Vec<u8>> {
        let pdfium = open_pdfium()?;
        let document = pdfium.load_pdf_from_file(source, None).ok()?;
        let page = document.pages().get(info.index as u16).ok()?;

        let (width, height) = render_size(page.width().value as f64, page.height().value as f64);
        let config = PdfRenderConfig::new()
            .set_format(PdfBitmapFormat::BGR)
            .set_target_size(width, height);
        let bitmap = page.render_with_config(&config).ok()?;

        let (width, height) = (bitmap.width() as usize, bitmap.height() as usize);
        let raw = bitmap.as_raw_bytes();
        let stride = raw.len() / height.max(1);

        // De-pad BGR rows (PDFium strides align to 4 bytes) and swap
        // to RGB for the JPEG encoder.
        let mut rgb = Vec::with_capacity(width * height * 3);
        for row in 0..height {
            let line = &raw[row * stride..row * stride + width * 3];
            for px in line.as_chunks::<3>().0 {
                rgb.extend_from_slice(&[px[2], px[1], px[0]]);
            }
        }

        let mut jpeg = Vec::new();
        Encoder::new(&mut jpeg, JPEG_QUALITY)
            .encode(&rgb, width as u16, height as u16, ColorType::Rgb)
            .ok()?;
        Some(jpeg)
    }
}

/// Binds to a pdfium library without ever panicking (the
/// `Pdfium::default()` path panics when no library exists). Order:
/// `CR_PDFIUM` env override, library in the working directory,
/// system library.
pub fn open_pdfium() -> Option<Pdfium> {
    if let Ok(path) = std::env::var("CR_PDFIUM") {
        if let Ok(bindings) = Pdfium::bind_to_library(path) {
            return Some(Pdfium::new(bindings));
        }
    }
    let local = Pdfium::pdfium_platform_library_name_at_path("./");
    if let Ok(bindings) = Pdfium::bind_to_library(local) {
        return Some(Pdfium::new(bindings));
    }
    Pdfium::bind_to_system_library().ok().map(Pdfium::new)
}

/// Whether a usable pdfium library could be loaded. Test gating and
/// `cr-cli` diagnostics use this.
pub fn is_available() -> bool {
    open_pdfium().is_some()
}

/// Opens a document and returns its page count (listing only).
fn open_document(source: &Path) -> Option<u16> {
    let pdfium = open_pdfium()?;
    let document = pdfium.load_pdf_from_file(source, None).ok()?;
    Some(document.pages().len())
}

/// Port of `PdfiumReaderEngine.CalculateSize`. Width and height come
/// in points (1/72in). The C# truncates the scaled dimension with an
/// int cast; dimensions saturate at u16::MAX for pdfium-render.
fn render_size(width: f64, height: f64) -> (i32, i32) {
    let target_width = (width * MAX_HEIGHT / height) as i32;
    let target_height = (height * MAX_WIDTH / width) as i32;
    if width > height {
        (target_width.min(i32::from(u16::MAX)), MAX_HEIGHT as i32)
    } else {
        (MAX_WIDTH as i32, target_height.min(i32::from(u16::MAX)))
    }
}

#[cfg(test)]
mod tests {
    use super::render_size;

    #[test]
    fn render_size_matches_calculate_size() {
        // Portrait A4 at 72dpi: 595 x 842 points.
        let (w, h) = render_size(595.0, 842.0);
        assert_eq!(w, 1920);
        assert_eq!(h, (842.0 * 1920.0 / 595.0) as i32);

        // Landscape: height pinned, width balloons (C# behavior).
        let (w, h) = render_size(842.0, 595.0);
        assert_eq!(h, 2540);
        assert_eq!(w, (842.0 * 2540.0 / 595.0) as i32);

        // Square page: portrait branch.
        let (w, h) = render_size(1000.0, 1000.0);
        assert_eq!(w, 1920);
        assert_eq!(h, 1920);
    }
}
