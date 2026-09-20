//! The automatcher's cover hash (ADR-074).
//!
//! The port now uses ComicTagger's `average_hash` (in
//! `cr_image::comictagger_hash`) instead of the ComicRack
//! `imagehash.py` average-hash, so the app computes the same bit
//! pattern that the `localcv.db` import stores. Both sides of every
//! comparison hash the same way, so the similarity score stays
//! meaningful; the match thresholds are configurable (ADR-074).

use cr_image::Image;

/// `imagehash.hash`: the ComicTagger average-hash of an RGBA image.
pub fn hash(image: &Image) -> Option<u64> {
    cr_image::comictagger_hash::average_hash(image)
}

/// The similarity between two hashes, 0.0..1.0 (1 - hamming/64).
/// A None (missing) hash matches nothing (0.0).
pub fn similarity(hash1: Option<u64>, hash2: Option<u64>) -> f64 {
    cr_image::comictagger_hash::similarity(hash1, hash2)
}
