//! Port of the plugin's `imagehash.py` — a 64-bit average-hash for
//! cover comparison. The C# grays the image through a color matrix
//! while squashing to 8×8 with bicubic interpolation; since both are
//! linear, the bit pattern is identical for any linear luminance, so
//! the port squashes first and grays with the standard weights.
//!
//! Bit order (the C# `reduce(x | val << i)` over
//! `[GetPixel(x, y) for x in range(8) for y in range(8)]`): bit
//! `x * 8 + y`, column-major, 1 when the pixel luminance is strictly
//! above the image average.

use cr_image::Image;

use crate::matching::MATCH_THRESHOLD;

/// The similarity between two hashes, 0.0..1.0 (1 - hamming/64).
/// A None (missing) hash matches nothing (0.0).
pub fn similarity(hash1: Option<u64>, hash2: Option<u64>) -> f64 {
    match (hash1, hash2) {
        (Some(h1), Some(h2)) => {
            let distance = (h1 ^ h2).count_ones() as f64;
            1.0 - distance / 64.0
        }
        _ => 0.0,
    }
}

/// `imagehash.hash`: the perceptual hash of an RGBA image.
pub fn hash(image: &Image) -> Option<u64> {
    if image.rgba.is_empty() || image.width == 0 || image.height == 0 {
        return None;
    }
    // the image squashes to exactly 8×8 (aspect ratio ignored, the
    // C# Graphics.DrawImage parity)
    let small =
        cr_image::resize::resize_to(image, 8, 8, cr_image::resize::Resampling::BicubicHQ).ok()?;
    let mut luminance = [0.0f64; 64];
    for x in 0..8usize {
        for y in 0..8usize {
            let i = x * 8 + y;
            let r = small.rgba[i * 4] as f64;
            let g = small.rgba[i * 4 + 1] as f64;
            let b = small.rgba[i * 4 + 2] as f64;
            luminance[i] = 0.3 * r + 0.59 * g + 0.11 * b;
        }
    }
    let average = luminance.iter().sum::<f64>() / 64.0;
    let mut value = 0u64;
    for (i, l) in luminance.iter().enumerate() {
        if *l > average {
            value |= 1u64 << i;
        }
    }
    Some(value)
}

/// True when two covers are similar enough to be "the same"
/// (`are_the_same`, the automatcher's threshold).
pub fn are_the_same(hash1: Option<u64>, hash2: Option<u64>) -> bool {
    similarity(hash1, hash2) > MATCH_THRESHOLD
}
