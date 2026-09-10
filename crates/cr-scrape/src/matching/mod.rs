//! The matching layer (T4): the perceptual cover hash, the series
//! match score, the series-ref filters (the C# `dbutils.py`), and the
//! auto-match engine (`automatcher.py`).

pub mod automatcher;
pub mod imagehash;
pub mod matchscore;

use cr_image::Image;

/// The minimum similarity for two covers to count as "the same"
/// (the automatcher threshold).
pub const MATCH_THRESHOLD: f64 = 0.87;

/// `utils.strip_back_cover`: an image with the 2-page pixel ratio
/// (1.2 < width/height < 1.5) is assumed to be front+back cover
/// side by side — only the front (RIGHT half) is kept.
pub fn strip_back_cover(image: &Image) -> Image {
    let ratio = image.width as f64 / image.height as f64;
    if ratio < 1.5 && ratio > 1.2 {
        let half = (image.width / 2) as usize;
        let w = half;
        let h = image.height as usize;
        let mut rgba = Vec::with_capacity(w * h * 4);
        for y in 0..h {
            let row = y * image.width as usize * 4;
            rgba.extend_from_slice(&image.rgba[row + half * 4..row + half * 4 + w * 4]);
        }
        Image::new(half as u32, image.height, rgba).expect("stripped cover size")
    } else {
        image.clone()
    }
}

/// `dbutils.filter_series_refs`: drops series by publisher and start
/// year; series with at least `never_ignore_threshold` issues are
/// never filtered.
pub fn filter_series_refs(
    refs: Vec<crate::cv::models::SeriesRef>,
    ignored_publishers: &std::collections::BTreeSet<String>,
    ignore_before_year: i32,
    ignore_after_year: i32,
    never_ignore_threshold: i32,
) -> Vec<crate::cv::models::SeriesRef> {
    refs.into_iter()
        .filter(|series_ref| {
            if series_ref.issue_count >= never_ignore_threshold {
                return true;
            }
            let publisher = series_ref.publisher.trim().to_lowercase();
            let year_passes = series_ref.volume_year == -1
                || (series_ref.volume_year >= ignore_before_year
                    && series_ref.volume_year <= ignore_after_year);
            let pub_passes = !ignored_publishers.contains(&publisher);
            year_passes && pub_passes
        })
        .collect()
}
