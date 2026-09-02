//! Color adjust — port of `ImageProcessing.ApplyAdjustment`
//! (`ImageProcessing.cs:1480`) with `CreateColorMatrix`,
//! `CreateColorScaleMatrix`, `CreateColorSaturationMatrix`,
//! `CreateColorWhitePointMatrix`, `ChangeGamma`, `Sharpen`, and the
//! `Histogram` black/white point scan.
//!
//! Matrix convention (from the C# source): row-vector color
//! `c' = c · M` with `c = [r, g, b, 1]`, scale on the diagonal,
//! additive terms in row 3, and the standard luminance weights
//! `grayRed = 0.3086`, `grayGreen = 0.6094`, `grayBlue = 0.082`.

use cr_core::model::bitmap_adjustment::BitmapAdjustment;

use crate::error::Result;
use crate::Image;

const GRAY_RED: f32 = 0.3086;
const GRAY_GREEN: f32 = 0.6094;
const GRAY_BLUE: f32 = 0.082;

/// `Histogram.GetBlackPointNormalized`/`GetWhitePointNormalized`
/// defaults (`defaultThreshold = 0.005`, `range = 0.25`).
const HISTOGRAM_THRESHOLD: f32 = 0.005;
const HISTOGRAM_RANGE: f32 = 0.25;

/// 4x4 color transform: rows 0..3 multiply `[r, g, b, 1]`.
type Matrix4 = [[f32; 4]; 4];

/// `CreateColorScaleMatrix(scale, offset)` as a 4x4 (the C# embeds it
/// in 5x5; the unused alpha/weight row is folded out here).
fn scale_matrix(scale: f32, offset: f32) -> Matrix4 {
    [
        [scale, 0.0, 0.0, offset],
        [0.0, scale, 0.0, offset],
        [0.0, 0.0, scale, offset],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

/// `CreateColorSaturationMatrix(sat)`.
fn saturation_matrix(sat: f32) -> Matrix4 {
    let dr = (1.0 - sat) * GRAY_RED;
    let dg = (1.0 - sat) * GRAY_GREEN;
    let db = (1.0 - sat) * GRAY_BLUE;
    [
        [dr + sat, dr, dr, 0.0],
        [dg, dg + sat, dg, 0.0],
        [db, db, db + sat, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

/// `CreateColorWhitePointMatrix(whitePoint)` — 8-bit RGB input like
/// the C# (`whitePoint.R` etc.), with its identity-on-error behavior
/// preserved.
fn white_point_matrix(r: u8, g: u8, b: u8) -> Matrix4 {
    let gray = GRAY_RED * f32::from(r) + GRAY_GREEN * f32::from(g) + GRAY_BLUE * f32::from(b);
    let dr = (gray - f32::from(r)) / 256.0;
    let dg = (gray - f32::from(g)) / 256.0;
    let db = (gray - f32::from(b)) / 256.0;
    // The C# sets matrix[2,2] = 1/(1 - matrix[3,1]) — the green
    // denominator for the blue channel; ported as written.
    if dr >= 1.0 || dg >= 1.0 || db >= 1.0 {
        return identity_matrix();
    }
    let br = 1.0 / (1.0 - dr);
    let bg = 1.0 / (1.0 - dg);
    let bb = 1.0 / (1.0 - dg);
    [
        [br, 0.0, 0.0, 0.0],
        [0.0, bg, 0.0, 0.0],
        [0.0, 0.0, bb, 0.0],
        [dr, dg, db, 1.0],
    ]
}

fn identity_matrix() -> Matrix4 {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

/// Row-vector 4x4 multiply: `(c · M1) · M2 == c · (M1 · M2)`.
fn multiply(a: &Matrix4, b: &Matrix4) -> Matrix4 {
    let mut out = [[0.0f32; 4]; 4];
    for i in 0..4 {
        for j in 0..4 {
            out[i][j] = (0..4).map(|k| a[i][k] * b[k][j]).sum();
        }
    }
    out
}

/// `CreateColorMatrix(blackLevel, whiteLevel, contrast, brightness,
/// saturation, whitePoint)` — scale · saturation · white point, plus
/// the C#'s 0.001 epsilon on the translation row.
fn color_matrix(
    black_level: f32,
    white_level: f32,
    contrast: f32,
    brightness: f32,
    saturation: f32,
    white_point: Option<(u8, u8, u8)>,
) -> Matrix4 {
    let scale = (contrast + 1.0) / (white_level - black_level);
    let offset = brightness - black_level;
    let mut m = multiply(
        &scale_matrix(scale, offset),
        &saturation_matrix(saturation + 1.0),
    );
    let wp = match white_point {
        Some((r, g, b)) if !(r == 0 && g == 0 && b == 0) && !(r == 255 && g == 255 && b == 255) => {
            white_point_matrix(r, g, b)
        }
        // `WhitePointColor.IsBlackOrWhite()` → no white point matrix.
        _ => identity_matrix(),
    };
    m = multiply(&m, &wp);
    // matrix[4,0..2] = 0.001 epsilon (applied per channel via the
    // translation row in the C# 5x5 form).
    for row in m.iter_mut().take(3) {
        row[3] += 0.001;
    }
    m
}

/// `GetHistogram` grays + `Histogram` black/white point scan. Grays
/// are the luma-weighted 8-bit values; thresholds over the pixel
/// count. Returns `(bp, wp)` normalized.
fn black_white_point(image: &Image) -> (f32, f32) {
    let mut grays = [0i64; 256];
    for px in image.rgba.as_chunks::<4>().0 {
        let gray = GRAY_RED * f32::from(px[0])
            + GRAY_GREEN * f32::from(px[1])
            + GRAY_BLUE * f32::from(px[2]);
        grays[gray as usize] += 1;
    }
    let count = image.width as f32 * image.height as f32;
    let norm = |v: i64| v as f32 / count;

    // FindLowThreshold: first index where the running sum from the
    // bottom reaches the threshold; i/size.
    let mut acc = 0.0f32;
    let mut bp = 0.0f32;
    for (i, &v) in grays.iter().enumerate() {
        acc += norm(v);
        if acc >= HISTOGRAM_THRESHOLD {
            bp = i as f32 / grays.len() as f32;
            break;
        }
    }
    // FindTopThreshold: from the top; return value is index/size.
    let mut acc = 0.0f32;
    let mut wp = 1.0f32;
    for (i, &v) in grays.iter().enumerate().rev() {
        acc += norm(v);
        if acc >= HISTOGRAM_THRESHOLD {
            wp = i as f32 / grays.len() as f32;
            break;
        }
    }
    (bp.min(HISTOGRAM_RANGE), wp.max(3.0 * HISTOGRAM_RANGE))
}

/// `ChangeGamma` — 8-bit LUT, `255·(i/255)^(1/gamma) + 0.5`, gamma
/// clamped to 0.2..5.0.
fn gamma_lut(gamma: f32) -> [u8; 256] {
    let gamma = gamma.clamp(0.2, 5.0);
    let mut lut = [0u8; 256];
    for (i, slot) in lut.iter_mut().enumerate() {
        let v = 255.0 * (i as f32 / 255.0).powf(1.0 / gamma) + 0.5;
        *slot = v.min(255.0) as u8;
    }
    lut
}

/// `Sharpen` convolution: center weight `a`, 4-neighbors `-b`,
/// divisor `a - 4b` (1 when 0), border pixels untouched, source
/// sampled from the pre-convolution copy.
fn sharpen(image: &mut Image, a: i32, b: i32) {
    let divisor = a - 4 * b;
    let divisor = if divisor == 0 { 1 } else { divisor };
    let w = image.width as usize;
    let h = image.height as usize;
    if w < 3 || h < 3 {
        return;
    }
    let src = image.rgba.clone();
    let at = |x: usize, y: usize, c: usize| i32::from(src[(y * w + x) * 4 + c]);
    let clamp = |v: i32| v.clamp(0, 255) as u8;
    for y in 1..h - 1 {
        for x in 1..w - 1 {
            for c in 0..3 {
                let sum = at(x, y - 1, c) * -b
                    + at(x - 1, y, c) * -b
                    + at(x, y, c) * a
                    + at(x + 1, y, c) * -b
                    + at(x, y + 1, c) * -b;
                image.rgba[(y * w + x) * 4 + c] = clamp(sum / divisor);
            }
        }
    }
}

/// `ApplyAdjustment` — color matrix (with auto-contrast points),
/// gamma, sharpening. Operates in place like the C# extension.
pub fn apply_adjustment(image: &mut Image, adjustment: &BitmapAdjustment) -> Result<()> {
    let (wp, bp) = if adjustment.has_auto_contrast() {
        black_white_point(image)
    } else {
        (1.0, 0.0)
    };

    let has_color = adjustment.has_color_transformations() || wp < 0.95 || bp > 0.05;
    if has_color {
        let white_point = adjustment.white_point_rgb();
        let m = color_matrix(
            bp,
            wp,
            adjustment.contrast,
            adjustment.brightness,
            adjustment.saturation,
            Some(white_point),
        );
        // Row-vector convention: out_r = r·m[0][0] + g·m[1][0] +
        // b·m[2][0] + 1·m[3][0] (the additive row 3 is the C#
        // matrix[3,x] offsets).
        for px in image.rgba.as_chunks_mut::<4>().0 {
            let (r, g, b) = (f32::from(px[0]), f32::from(px[1]), f32::from(px[2]));
            let out = [
                r * m[0][0] + g * m[1][0] + b * m[2][0] + m[3][0],
                r * m[0][1] + g * m[1][1] + b * m[2][1] + m[3][1],
                r * m[0][2] + g * m[1][2] + b * m[2][2] + m[3][2],
            ];
            px[0] = out[0].round().clamp(0.0, 255.0) as u8;
            px[1] = out[1].round().clamp(0.0, 255.0) as u8;
            px[2] = out[2].round().clamp(0.0, 255.0) as u8;
        }
    }

    if adjustment.has_gamma() {
        let lut = gamma_lut(1.0 + adjustment.gamma);
        for px in image.rgba.as_chunks_mut::<4>().0 {
            for channel in px.iter_mut().take(3) {
                *channel = lut[*channel as usize];
            }
        }
    }

    if adjustment.sharpen != 0 {
        sharpen(image, (4 - adjustment.sharpen) * 5, 1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_matrix_applies() {
        let m = scale_matrix(2.0, 10.0);
        // c = [100, 50, 0, 1]
        let c = [100.0, 50.0, 0.0, 1.0];
        let out: Vec<f32> = (0..4)
            .map(|i| (0..4).map(|j| c[j] * m[i][j]).sum())
            .collect();
        assert_eq!(out, [210.0, 110.0, 10.0, 1.0]);
    }

    #[test]
    fn saturation_matrix_gray() {
        // Full desaturation (adj.Saturation = -1 → sat = 0) preserves
        // gray inputs (luma weights sum to 1) — row-vector application.
        let m = saturation_matrix(0.0);
        let r = 100.0f32;
        let out_r = r * m[0][0] + r * m[1][0] + r * m[2][0] + m[3][0];
        assert!((out_r - 100.0).abs() < 0.01);
        // Neutral (adj.Saturation = 0 → sat = 1) is identity.
        let m = saturation_matrix(1.0);
        let out_r = r * m[0][0] + r * m[1][0] + r * m[2][0] + m[3][0];
        assert!((out_r - 100.0).abs() < 0.01);
    }

    #[test]
    fn gamma_lut_midpoint() {
        let lut = gamma_lut(1.0);
        assert_eq!(lut[128], (255.0 * (128.0f32 / 255.0) + 0.5) as u8);
        let lut = gamma_lut(2.0);
        assert_eq!(lut[128], (255.0 * (128.0f32 / 255.0).powf(0.5) + 0.5) as u8);
    }

    #[test]
    fn sharpen_center_weight() {
        let mut img = Image::new(3, 3, vec![100; 9 * 4]).unwrap();
        sharpen(&mut img, 10, 1);
        // a=10, b=1 → divisor 6; sum = 10*100 - 4*100 = 600 → 100.
        assert_eq!(img.rgba[4], 100);
    }

    #[test]
    fn black_white_point_degenerate() {
        let img = Image::new(2, 2, vec![255; 2 * 2 * 4]).unwrap();
        let (bp, wp) = black_white_point(&img);
        // All white: FindLowThreshold reaches the threshold at the
        // last bin (255/256), clamped to range (0.25); FindTopThreshold
        // also lands at 255/256.
        assert!((bp - 0.25).abs() < 1e-6);
        assert!((wp - 255.0 / 256.0).abs() < 1e-6);
    }
}
