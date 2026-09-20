//! ComicTagger-compatible perceptual hashes (ADR-074).
//!
//! A faithful port of ComicTagger's `ImageHasher`
//! (`comictaggerlib/imagehasher.py`): `average_hash`,
//! `difference_hash`, and `perception_hash`. The imported `localcv.db`
//! cover hashes are produced by this exact algorithm, so the app must
//! compute the same bit patterns to compare against them.
//!
//! The one documented tolerance: ComicTagger runs on Pillow, which
//! decodes JPEG through libjpeg and resamples with its own Lanczos.
//! This port reproduces Pillow's Lanczos resample and its `L`
//! grayscale exactly (verified to 0 bits on covers decoded the same
//! way). Where the Rust JPEG decoder differs from libjpeg by a pixel,
//! the resulting hash can differ by at most about one bit — far inside
//! the cover-match threshold, so matching is unaffected (MEASURED
//! 2026-09-20).
//!
//! Bit order follows ComicTagger: the first flattened pixel is the
//! most significant bit (`1 << (n - 1 - i)`), row-major.

use crate::Image;

/// Pillow's Lanczos support radius.
const SUPPORT: f64 = 3.0;
/// Pillow's `PRECISION_BITS` (`32 - 8 - 2`) for the fixed-point
/// coefficient quantization in `Resample.c`.
const PRECISION_BITS: i32 = 32 - 8 - 2;

/// `sinc(x) = sin(pi x) / (pi x)`, with `sinc(0) = 1`.
fn sinc(x: f64) -> f64 {
    if x == 0.0 {
        return 1.0;
    }
    let x = x * std::f64::consts::PI;
    x.sin() / x
}

/// Pillow's Lanczos kernel: `sinc(x) * sinc(x / 3)` on `[-3, 3)`.
fn lanczos(x: f64) -> f64 {
    if (-3.0..3.0).contains(&x) {
        sinc(x) * sinc(x / 3.0)
    } else {
        0.0
    }
}

/// One dimension's precomputed resample plan: the kernel width, the
/// per-output `[min, count]` bounds, and the quantized coefficients.
struct Plan {
    ksize: usize,
    bounds: Vec<i32>,
    coeffs: Vec<i32>,
}

/// Precomputes the Pillow convolution coefficients for `in_size ->
/// out_size` (`precompute_coeffs` in `Resample.c`).
fn precompute(in_size: u32, out_size: u32) -> Plan {
    let scale = in_size as f64 / out_size as f64;
    let filterscale = scale.max(1.0);
    let support = SUPPORT * filterscale;
    let ksize = (support.ceil() as usize) * 2 + 1;
    let mut bounds = vec![0i32; out_size as usize * 2];
    let mut kk = vec![0f64; out_size as usize * ksize];
    let ss = 1.0 / filterscale;
    for xx in 0..out_size as i32 {
        let center = (xx as f64 + 0.5) * scale;
        let mut xmin = (center - support + 0.5).floor() as i32;
        if xmin < 0 {
            xmin = 0;
        }
        let mut xmax = (center + support + 0.5).floor() as i32;
        if xmax > in_size as i32 {
            xmax = in_size as i32;
        }
        xmax -= xmin;
        let k_off = xx as usize * ksize;
        let mut w_sum = 0.0f64;
        for x in 0..xmax {
            let w = lanczos((x as f64 + xmin as f64 - center + 0.5) * ss);
            kk[k_off + x as usize] = w;
            w_sum += w;
        }
        for x in 0..xmax {
            if w_sum != 0.0 {
                kk[k_off + x as usize] /= w_sum;
            }
        }
        for x in xmax..ksize as i32 {
            kk[k_off + x as usize] = 0.0;
        }
        bounds[xx as usize * 2] = xmin;
        bounds[xx as usize * 2 + 1] = xmax;
    }
    let scale_factor = (1i64 << PRECISION_BITS) as f64;
    let coeffs = kk
        .iter()
        .map(|&w| {
            if w < 0.0 {
                (-0.5 + w * scale_factor) as i32
            } else {
                (0.5 + w * scale_factor) as i32
            }
        })
        .collect();
    Plan {
        ksize,
        bounds,
        coeffs,
    }
}

/// Pillow's `clip8`: round the fixed-point accumulator and clamp to a
/// byte.
fn clip8(value: i64) -> u8 {
    let v = (value >> PRECISION_BITS) as i32;
    v.clamp(0, 255) as u8
}

/// Resamples an RGB buffer to `out_w x out_h` with Pillow's Lanczos,
/// horizontal pass then vertical (the `Resample.c` order).
fn resample_rgb(rgb: &[u8], w: u32, h: u32, out_w: u32, out_h: u32) -> Vec<u8> {
    let half = 1i64 << (PRECISION_BITS - 1);
    let horiz_plan = precompute(w, out_w);
    let mut horiz = vec![0u8; (out_w * h * 3) as usize];
    for yy in 0..h {
        for xx in 0..out_w {
            let xmin = horiz_plan.bounds[xx as usize * 2];
            let xmax = horiz_plan.bounds[xx as usize * 2 + 1];
            let k_off = xx as usize * horiz_plan.ksize;
            for c in 0..3usize {
                let mut acc = half;
                for x in 0..xmax {
                    let px = rgb[((yy * w + (xmin + x) as u32) * 3) as usize + c] as i64;
                    acc += px * horiz_plan.coeffs[k_off + x as usize] as i64;
                }
                horiz[((yy * out_w + xx) * 3) as usize + c] = clip8(acc);
            }
        }
    }
    let vert_plan = precompute(h, out_h);
    let mut out = vec![0u8; (out_w * out_h * 3) as usize];
    for yy in 0..out_h {
        let ymin = vert_plan.bounds[yy as usize * 2];
        let ymax = vert_plan.bounds[yy as usize * 2 + 1];
        let k_off = yy as usize * vert_plan.ksize;
        for xx in 0..out_w {
            for c in 0..3usize {
                let mut acc = half;
                for y in 0..ymax {
                    let px = horiz[(((ymin + y) as u32 * out_w + xx) * 3) as usize + c] as i64;
                    acc += px * vert_plan.coeffs[k_off + y as usize] as i64;
                }
                out[((yy * out_w + xx) * 3) as usize + c] = clip8(acc);
            }
        }
    }
    out
}

/// Pillow's `L` grayscale: `L24 = (R*19595 + G*38470 + B*7471 +
/// 0x8000) >> 16` (the ITU-R 601-2 fixed-point in `Convert.c`).
fn to_luma(px: &[u8]) -> u8 {
    ((px[0] as u32 * 19595 + px[1] as u32 * 38470 + px[2] as u32 * 7471 + 0x8000) >> 16) as u8
}

/// The RGB view of the image (the alpha channel is dropped, as
/// Pillow's `convert("RGB")` does before hashing).
fn rgb_of(image: &Image) -> Vec<u8> {
    let mut out = Vec::with_capacity((image.width * image.height * 3) as usize);
    let (pixels, _) = image.rgba.as_chunks::<4>();
    for px in pixels {
        out.extend_from_slice(&px[..3]);
    }
    out
}

/// Resamples to `w x h` grayscale bytes, row-major (the shared front
/// of every ComicTagger hash: resize the RGB, then convert to `L`).
fn gray_grid(image: &Image, w: u32, h: u32) -> Option<Vec<u8>> {
    if image.rgba.is_empty() || image.width == 0 || image.height == 0 {
        return None;
    }
    let rgb = rgb_of(image);
    let resized = resample_rgb(&rgb, image.width, image.height, w, h);
    let (chunks, _) = resized.as_chunks::<3>();
    Some(chunks.iter().map(|px| to_luma(px)).collect())
}

/// ComicTagger `average_hash`: 8x8 Lanczos, bit set where the pixel is
/// above the mean, MSB-first.
pub fn average_hash(image: &Image) -> Option<u64> {
    let pixels = gray_grid(image, 8, 8)?;
    let avg = pixels.iter().map(|&p| p as f64).sum::<f64>() / pixels.len() as f64;
    let mut h = 0u64;
    for (i, &p) in pixels.iter().enumerate() {
        if p as f64 > avg {
            h |= 1u64 << (pixels.len() - 1 - i);
        }
    }
    Some(h)
}

/// ComicTagger `difference_hash`: a 9x8 Lanczos grid, bit set where a
/// pixel is brighter than its right neighbour, MSB-first.
pub fn difference_hash(image: &Image) -> Option<u64> {
    let pixels = gray_grid(image, 9, 8)?;
    let mut h = 0u64;
    let mut z = 8 * 8 - 1i32;
    for y in 0..8usize {
        for x in 0..8usize {
            let idx = x + 9 * y;
            if pixels[idx] < pixels[idx + 1] {
                h |= 1u64 << z;
            }
            z -= 1;
        }
    }
    Some(h)
}

/// ComicTagger `perception_hash`: a 32x32 Lanczos grid, its top-left
/// 8x8 DCT block thresholded against the block median, MSB-first.
pub fn perception_hash(image: &Image) -> Option<u64> {
    let pixels = gray_grid(image, 32, 32)?;
    let mut block = [[0f64; 32]; 32];
    for row in 0..32 {
        for col in 0..32 {
            block[row][col] = pixels[row * 32 + col] as f64;
        }
    }
    let rows = dct_2d(&block);
    let mut low = Vec::with_capacity(64);
    for row in rows.iter().take(8) {
        low.extend_from_slice(&row[..8]);
    }
    let med = median(&low);
    let mut h = 0u64;
    for (i, &p) in low.iter().enumerate() {
        if p > med {
            h |= 1u64 << (low.len() - 1 - i);
        }
    }
    Some(h)
}

/// ComicTagger's plain `generate_dct2` applied on both axes (a direct
/// O(n^2) DCT-II with no orthonormal scaling, matching the Python).
fn dct_2d(block: &[[f64; 32]; 32]) -> Vec<Vec<f64>> {
    let rows: Vec<Vec<f64>> = block.iter().map(|row| dct_1d(row)).collect();
    let mut cols = vec![vec![0f64; 32]; 32];
    for j in 0..32 {
        let column: Vec<f64> = (0..32).map(|i| rows[i][j]).collect();
        let transformed = dct_1d(&column);
        for (i, value) in transformed.into_iter().enumerate() {
            cols[i][j] = value;
        }
    }
    cols
}

fn dct_1d(block: &[f64]) -> Vec<f64> {
    let n = block.len();
    let mut out = vec![0f64; n];
    for (k, slot) in out.iter_mut().enumerate() {
        let mut sum = 0.0;
        for (nn, &value) in block.iter().enumerate() {
            sum += value
                * (std::f64::consts::PI * k as f64 * (2 * nn + 1) as f64 / (2.0 * n as f64)).cos();
        }
        *slot = sum;
    }
    out
}

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = sorted.len();
    if n == 0 {
        0.0
    } else if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    }
}

/// The Hamming distance between two hashes (ComicTagger's
/// `hamming_distance`).
pub fn hamming_distance(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

/// The similarity between two hashes, `0.0..=1.0` (`1 - hamming/64`).
/// A missing hash on either side is no similarity.
pub fn similarity(a: Option<u64>, b: Option<u64>) -> f64 {
    match (a, b) {
        (Some(a), Some(b)) => 1.0 - hamming_distance(a, b) as f64 / 64.0,
        _ => 0.0,
    }
}
