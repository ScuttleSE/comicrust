//! Decode chain — port of `BitmapExtensions.BitmapFromBytes` (with the
//! 32-bit JPEG EXIF quirk) plus the normalize-to-JPEG conversions from
//! `ImageProvider.RetrieveSourceByteImage` (`WebpImage`,
//! `JpegXLImage`, `HeifAvifImage`, `Jpeg2000Image`).
//!
//! Codecs: JPEG via zune-jpeg; PNG/GIF/TIFF/BMP/WebP via the `image`
//! crate; JXL via jxl-oxide. HEIF/AVIF and JPEG2000 need system
//! libraries (libheif/openjpeg) that the build cannot assume — those
//! formats report [`Error::UnsupportedFormat`] until packaging wires
//! the libs (documented gap in AGENTS.md).
//!
//! The EXIF quirk: when a JPEG fails to decode, strip the APPn
//! segments (`JpegFile.RemoveExif`) and retry once.

use crate::error::{Error, Result};
use crate::Image;

/// Sniffed container formats (the C# `ConvertToJpeg` checks use the
/// same leading signatures).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImageFormat {
    Jpeg,
    Png,
    Gif,
    Tiff,
    Bmp,
    Webp,
    Jxl,
    Heif,
    J2k,
    Djvu,
    Unknown,
}

/// Format detection by signature, mirroring the C# checks
/// (`WebpImage.IsWebP` = RIFF....WEBP, `JpegXLImage` = FF 0A or
/// 0000000C 4A584C, `HeifAvifImage` = ftyp box with heic/heif/avif,
/// `Jpeg2000Image` = 0000000C 6A502020, `DjVuImage` = AT&TF).
pub fn detect_format(data: &[u8]) -> ImageFormat {
    if data.len() >= 3 && data[0] == 0xff && data[1] == 0xd8 {
        return ImageFormat::Jpeg;
    }
    if data.len() >= 8 && data[..8] == [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a] {
        return ImageFormat::Png;
    }
    if data.len() >= 6 && (data[..6] == *b"GIF87a" || data[..6] == *b"GIF89a") {
        return ImageFormat::Gif;
    }
    if data.len() >= 4 && (data[..4] == *b"II*\0" || data[..4] == b"MM\0*".to_vec()[..]) {
        return ImageFormat::Tiff;
    }
    if data.len() >= 2 && data[..2] == *b"BM" {
        return ImageFormat::Bmp;
    }
    if data.len() >= 12 && data[..4] == *b"RIFF" && data[8..12] == *b"WEBP" {
        return ImageFormat::Webp;
    }
    if data.len() >= 12 && data[..4] == [0, 0, 0, 0x0c] && data[4..12] == *b"JXL \r\n\x87\n" {
        return ImageFormat::Jxl;
    }
    // JXL naked codestream.
    if data.len() >= 2 && data[..2] == [0xff, 0x0a] {
        return ImageFormat::Jxl;
    }
    if data.len() >= 12 && data[4..8] == *b"ftyp" {
        let brand = &data[8..12];
        if brand == b"heic".as_slice() || brand == b"heif".as_slice() || brand == b"avif".as_slice()
        {
            return ImageFormat::Heif;
        }
    }
    if data.len() >= 12 && data[..4] == [0, 0, 0, 0x0c] && data[4..8] == *b"jP  " {
        return ImageFormat::J2k;
    }
    if data.len() >= 5 && data[..5] == *b"AT&TF" {
        return ImageFormat::Djvu;
    }
    ImageFormat::Unknown
}

/// `BitmapExtensions.BitmapFromBytes` — decode any supported format to
/// the RGBA currency. JPEG gets the 32-bit EXIF-fix retry.
pub fn decode(data: &[u8]) -> Result<Image> {
    match detect_format(data) {
        ImageFormat::Jpeg => decode_jpeg(data).or_else(|first| {
            // `LoadBitmap32BitFix`: strip APPn segments and retry.
            match strip_app_segments(data) {
                Some(stripped) => decode_jpeg(&stripped).map_err(|_| first),
                None => Err(first),
            }
        }),
        ImageFormat::Png => decode_with_image(data, image::ImageFormat::Png),
        ImageFormat::Gif => decode_with_image(data, image::ImageFormat::Gif),
        ImageFormat::Tiff => decode_with_image(data, image::ImageFormat::Tiff),
        ImageFormat::Bmp => decode_with_image(data, image::ImageFormat::Bmp),
        ImageFormat::Webp => decode_with_image(data, image::ImageFormat::WebP),
        ImageFormat::Jxl => decode_jxl(data),
        ImageFormat::Heif | ImageFormat::J2k => Err(Error::UnsupportedFormat),
        ImageFormat::Djvu | ImageFormat::Unknown => Err(Error::UnsupportedFormat),
    }
}

fn decode_jpeg(data: &[u8]) -> Result<Image> {
    use zune_core::colorspace::ColorSpace;
    let mut decoder = zune_jpeg::JpegDecoder::new(data);
    decoder
        .decode_headers()
        .map_err(|e| Error::Decode(format!("jpeg: {e:?}")))?;
    let (width, height) = decoder
        .dimensions()
        .ok_or_else(|| Error::Decode("jpeg: no dimensions".into()))?;
    let color = decoder.get_output_colorspace().unwrap_or(ColorSpace::RGB);
    let pixels = decoder
        .decode()
        .map_err(|e| Error::Decode(format!("jpeg: {e:?}")))?;
    match color {
        ColorSpace::RGB => rgb_to_rgba(&pixels, width, height),
        ColorSpace::Luma => gray_to_rgba(&pixels, width, height),
        _ => Err(Error::Decode("jpeg: unexpected colorspace".into())),
    }
}

fn decode_with_image(data: &[u8], format: image::ImageFormat) -> Result<Image> {
    let img = image::load_from_memory_with_format(data, format)
        .map_err(|e| Error::Decode(format!("{format:?}: {e}")))?;
    Ok(Image {
        width: img.width(),
        height: img.height(),
        rgba: img.into_rgba8().into_raw(),
    })
}

fn decode_jxl(data: &[u8]) -> Result<Image> {
    let decoder = jxl_oxide::JxlImage::builder()
        .read(data)
        .map_err(|e| Error::Decode(format!("jxl: {e:?}")))?;
    let width = decoder.width();
    let height = decoder.height();
    let render = decoder
        .render_frame(0)
        .map_err(|e| Error::Decode(format!("jxl: {e:?}")))?;
    let fb = render.image_all_channels();
    let pixels = fb.buf();
    // jxl-oxide gives 8-bit samples for u8 buffers, one plane per
    // channel.
    let channels = fb.channels();
    let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
    for p in 0..width as usize * height as usize {
        let sample = |i: usize| (pixels[p * channels + i] * 255.0) as u8;
        let r = sample(0);
        let g = if channels > 1 { sample(1) } else { r };
        let b = if channels > 2 { sample(2) } else { r };
        rgba.extend_from_slice(&[r, g, b, 255]);
    }
    Image::new(width, height, rgba)
}

/// `JpegFile.RemoveExif` — copy SOI, skip APP0..APPn segments, copy
/// the rest. Returns `None` when the data is not a JPEG.
fn strip_app_segments(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 4 || data[0] != 0xff || data[1] != 0xd8 {
        return None;
    }
    let mut out = vec![0xff, 0xd8];
    let mut i = 2usize;
    while i + 4 <= data.len() && data[i] == 0xff && (0xe0..=0xef).contains(&data[i + 1]) {
        let len = usize::from(data[i + 2]) << 8 | usize::from(data[i + 3]);
        i += 2 + len;
    }
    if i >= data.len() {
        return None;
    }
    out.extend_from_slice(&data[i..]);
    Some(out)
}

/// Encode the currency as JPEG (`ImageToJpegBytes`, quality 75).
pub fn encode_jpeg(image: &Image, quality: u8) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    jpeg_encoder::Encoder::new(&mut out, quality)
        .encode(
            &image.rgba,
            image.width as u16,
            image.height as u16,
            jpeg_encoder::ColorType::Rgba,
        )
        .map_err(|e| Error::Encode(format!("jpeg: {e:?}")))?;
    Ok(out)
}

/// Encode the currency as one of the page-export formats (the C#
/// `ExportImage` filter: JPEG/BMP/PNG/GIF/TIFF). GIF/TIFF/BMP/PNG go
/// through the `image` crate; JPEG keeps the dedicated encoder.
pub fn encode_image(image: &Image, format: ImageFormat) -> Result<Vec<u8>> {
    match format {
        ImageFormat::Jpeg => encode_jpeg(image, 75),
        ImageFormat::Png | ImageFormat::Bmp | ImageFormat::Gif | ImageFormat::Tiff => {
            let mut buf = Vec::with_capacity((image.width * image.height * 4) as usize);
            let dyn_img = image::RgbaImage::from_raw(image.width, image.height, image.rgba.clone())
                .ok_or_else(|| Error::Encode("bad image buffer".into()))?;
            let mut cursor = std::io::Cursor::new(&mut buf);
            let fmt = match format {
                ImageFormat::Png => image::ImageFormat::Png,
                ImageFormat::Bmp => image::ImageFormat::Bmp,
                ImageFormat::Gif => image::ImageFormat::Gif,
                _ => image::ImageFormat::Tiff,
            };
            dyn_img
                .write_to(&mut cursor, fmt)
                .map_err(|e| Error::Encode(format!("{fmt:?}: {e}")))?;
            Ok(buf)
        }
        _ => Err(Error::Encode("unsupported export format".into())),
    }
}

/// `ImageProvider.RetrieveSourceByteImage` normalize chain: formats
/// GDI+ cannot read (WebP, JXL, HEIF/AVIF, JPEG2000, DjVu) become
/// JPEG bytes; everything else passes through untouched.
pub fn normalize_to_jpeg(data: &[u8]) -> Option<Vec<u8>> {
    let format = detect_format(data);
    match format {
        ImageFormat::Webp | ImageFormat::Jxl | ImageFormat::Heif | ImageFormat::J2k => {
            let image = decode(data).ok()?;
            encode_jpeg(&image, 75).ok()
        }
        _ => None,
    }
}

fn rgb_to_rgba(pixels: &[u8], width: usize, height: usize) -> Result<Image> {
    let mut rgba = Vec::with_capacity(width * height * 4);
    for px in pixels.as_chunks::<3>().0 {
        rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
    }
    Image::new(width as u32, height as u32, rgba)
}

fn gray_to_rgba(pixels: &[u8], width: usize, height: usize) -> Result<Image> {
    let mut rgba = Vec::with_capacity(width * height * 4);
    for &g in pixels {
        rgba.extend_from_slice(&[g, g, g, 255]);
    }
    Image::new(width as u32, height as u32, rgba)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_detection() {
        assert_eq!(detect_format(b"\xff\xd8\xff\xe0rest"), ImageFormat::Jpeg);
        assert_eq!(detect_format(b"\x89PNG\r\n\x1a\nxxxx"), ImageFormat::Png);
        assert_eq!(detect_format(b"GIF89axxxx"), ImageFormat::Gif);
        assert_eq!(detect_format(b"BMxxxx"), ImageFormat::Bmp);
        assert_eq!(detect_format(b"RIFF1234WEBP"), ImageFormat::Webp);
        assert_eq!(detect_format(b"AT&TFmore"), ImageFormat::Djvu);
        assert_eq!(detect_format(b"not an image"), ImageFormat::Unknown);
    }

    #[test]
    fn exif_strip_removes_app_segments() {
        // SOI + APP1 (len 4, payload "xy") + rest.
        let data = [0xff, 0xd8, 0xff, 0xe1, 0x00, 0x04, b'x', b'y', 0xaa, 0xbb];
        let stripped = strip_app_segments(&data).unwrap();
        assert_eq!(stripped, vec![0xff, 0xd8, 0xaa, 0xbb]);
        assert!(strip_app_segments(b"no jpeg").is_none());
    }
}

#[cfg(test)]
mod encode_tests {
    use super::*;

    #[test]
    fn encode_image_detects_back() {
        let image = Image {
            width: 4,
            height: 3,
            rgba: vec![128; 4 * 3 * 4],
        };
        for (format, sig) in [
            (ImageFormat::Jpeg, &[0xffu8, 0xd8u8][..]),
            (ImageFormat::Png, &[0x89u8, b'P', b'N', b'G'][..]),
            (ImageFormat::Gif, b"GIF8".as_slice()),
            (ImageFormat::Bmp, b"BM".as_slice()),
            (ImageFormat::Tiff, &[0x49u8, 0x49u8, 0x2au8, 0x00u8][..]),
        ] {
            let bytes = encode_image(&image, format).unwrap_or_else(|e| panic!("{format:?}: {e}"));
            assert_eq!(&bytes[..sig.len()], sig, "{format:?} signature");
        }
        assert!(encode_image(&image, ImageFormat::Webp).is_err());
    }
}
