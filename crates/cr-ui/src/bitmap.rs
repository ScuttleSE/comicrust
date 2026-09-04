//! Small cairo/image helpers shared by the dialog widgets.

use cr_image::Image;
use gtk4::cairo::{self, ImageSurface};

/// RGBA8 → premultiplied ARGB [`ImageSurface`] (the reader's helper;
/// the surface stride, not `width * 4`).
pub fn image_surface_from_rgba(rgba: &[u8], width: u32, height: u32) -> ImageSurface {
    let stride = width as usize * 4;
    let mut argb = vec![0u8; stride * height as usize];
    for (src, dst) in rgba
        .as_chunks::<4>()
        .0
        .iter()
        .zip(argb.as_chunks_mut::<4>().0)
    {
        let a = u32::from(src[3]);
        let r = ((u32::from(src[0]) * a) / 255) as u8;
        let g = ((u32::from(src[1]) * a) / 255) as u8;
        let b = ((u32::from(src[2]) * a) / 255) as u8;
        dst[0] = b;
        dst[1] = g;
        dst[2] = r;
        dst[3] = src[3];
    }
    ImageSurface::create_for_data(
        argb,
        cairo::Format::ARgb32,
        width as i32,
        height as i32,
        stride as i32,
    )
    .expect("valid surface data")
}

/// A decoded [`Image`] → surface.
pub fn surface_from_image(img: &Image) -> ImageSurface {
    image_surface_from_rgba(&img.rgba, img.width, img.height)
}

/// Decoded JPEG/PNG bytes → surface.
pub fn surface_from_bytes(bytes: &[u8]) -> Option<ImageSurface> {
    let img = cr_image::decode::decode(bytes).ok()?;
    Some(surface_from_image(&img))
}

/// The pool's thumbnail blob (the C# `ThumbnailImage`
/// serialization: size header + JPEG) → surface. A plain image
/// falls back to a direct decode (the pages_view tolerance).
pub fn surface_from_thumb_blob(bytes: &[u8]) -> Option<ImageSurface> {
    let jpeg = cr_image::thumbnail::Thumbnail::from_bytes(bytes)
        .map(|t| t.data)
        .unwrap_or_else(|_| bytes.to_vec());
    surface_from_bytes(&jpeg)
}
