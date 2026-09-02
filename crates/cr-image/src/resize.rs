//! Scale and resize — port of `BitmapExtensions.Scale`/`Resize` and
//! `SizeExtensions.GetScale` (fit-to-box, aspect-preserving).
//!
//! `BitmapResampling.FastBilinear`/`Bilinear` map to the `image`
//! crate's Triangle filter, `BilinearHQ`/`Bicubic`/`BicubicHQ` to
//! CatmullRom, `NearestNeighbor` to Nearest. GDI+ pixel output differs
//! within the phase's documented tolerance.

use crate::error::Result;
use crate::Image;

/// `BitmapResampling.cs` (the members the engine uses).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Resampling {
    NearestNeighbor,
    FastBilinear,
    Bilinear,
    BilinearHQ,
    Bicubic,
    BicubicHQ,
    GdiPlus,
}

/// `SizeExtensions.GetScale` — aspect-preserving scale factor that
/// fits `size` into `target` (a zero target dimension is unbounded).
pub fn fit_scale(size: (u32, u32), target: (u32, u32)) -> f32 {
    let (w, h) = (size.0 as f32, size.1 as f32);
    let (tw, th) = (target.0 as f32, target.1 as f32);
    let mut scale = f32::INFINITY;
    if tw > 0.0 {
        scale = scale.min(tw / w);
    }
    if th > 0.0 {
        scale = scale.min(th / h);
    }
    if !scale.is_finite() {
        return 1.0;
    }
    scale
}

/// `Scale(bmp, size)` / `GetThumbnailSize` result — the fitted pixel
/// size, truncated like `Rectangle.Truncate` after the float scale.
pub fn fitted_size(size: (u32, u32), target: (u32, u32)) -> (u32, u32) {
    let scale = fit_scale(size, target);
    let w = (size.0 as f32 * scale).max(0.0) as u32;
    let h = (size.1 as f32 * scale).max(0.0) as u32;
    (w, h)
}

fn filter(resampling: Resampling) -> image::imageops::FilterType {
    match resampling {
        Resampling::NearestNeighbor => image::imageops::FilterType::Nearest,
        Resampling::FastBilinear | Resampling::Bilinear | Resampling::GdiPlus => {
            image::imageops::FilterType::Triangle
        }
        Resampling::BilinearHQ | Resampling::Bicubic | Resampling::BicubicHQ => {
            image::imageops::FilterType::CatmullRom
        }
    }
}

/// `Scale(bmp, size, resampling)` — fit `image` into the target box
/// (keeping aspect) and resample.
pub fn scale(image: &Image, target: (u32, u32), resampling: Resampling) -> Result<Image> {
    let (w, h) = fitted_size((image.width, image.height), target);
    resize_to(image, w.max(1), h.max(1), resampling)
}

/// `Resize` to explicit pixel dimensions.
pub fn resize_to(image: &Image, width: u32, height: u32, resampling: Resampling) -> Result<Image> {
    let src = image::RgbaImage::from_raw(image.width, image.height, image.rgba.clone()).ok_or(
        crate::Error::BadBufferSize {
            len: image.rgba.len(),
            expected: image.width as usize * image.height as usize * 4,
        },
    )?;
    let resized = image::imageops::resize(&src, width, height, filter(resampling));
    Ok(Image {
        width,
        height,
        rgba: resized.into_raw(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_scale_fits_inside() {
        // Portrait 800x1000 into 512 height, unbounded width.
        assert!((fit_scale((800, 1000), (0, 512)) - 0.512).abs() < 1e-6);
        // Landscape 2000x1000 into 512x512 box.
        assert!((fit_scale((2000, 1000), (512, 512)) - 0.256).abs() < 1e-6);
        // Both zero: identity.
        assert_eq!(fit_scale((100, 100), (0, 0)), 1.0);
    }

    #[test]
    fn fitted_size_truncates() {
        assert_eq!(fitted_size((800, 1000), (0, 512)), (409, 512));
    }

    #[test]
    fn scale_changes_dimensions() {
        let img = Image::new(4, 4, vec![128; 4 * 4 * 4]).unwrap();
        let out = scale(&img, (0, 2), Resampling::FastBilinear).unwrap();
        assert_eq!((out.width, out.height), (2, 2));
    }
}
