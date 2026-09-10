//! Port of the plugin's `automatcher.py` — finds a series for a book
//! without user interaction: series search + the match score for the
//! top three candidates, then cover-hash confirmation against the
//! ComicVine art.
//!
//! The book's page-0 image is decoded by the engine (worker thread)
//! and passed in — this module never opens an archive itself.

use cr_image::Image;

use crate::bookdata::BookData;
use crate::config::Configuration;
use crate::cv::connection::CvError;
use crate::cv::models::SeriesRef;
use crate::cv::queries::{Cv, SeriesProgressFn};
use crate::matching::imagehash::{hash, similarity};
use crate::matching::matchscore::MatchScore;
use crate::matching::{filter_series_refs, strip_back_cover, MATCH_THRESHOLD};

/// The threshold the first-issue "too similar" bail-out uses
/// (threshold - 0.10, the C# constant).
const SIMILARITY_THRESHOLD: f64 = MATCH_THRESHOLD - 0.10;

/// `find_series_ref`: the best auto-matched series for the book, or
/// None. `page0` is the book's decoded front cover.
pub fn find_series_ref(
    book: &BookData,
    config: &Configuration,
    cv: &mut Cv,
    score: &MatchScore,
    current_year: i32,
    page0: Option<&Image>,
    progress: &mut SeriesProgressFn,
) -> Result<Option<SeriesRef>, CvError> {
    // 1. the series search + the preference filters
    let advanced = config.advanced();
    let ignored: Vec<String> = advanced.ignored_searchterms.iter().cloned().collect();
    let series_refs = cv.query_series_refs(
        &book.series,
        &ignored,
        advanced.max_search_results,
        progress,
    )?;
    let series_refs = filter_series_refs(
        series_refs,
        &advanced.ignored_publishers,
        advanced.ignored_before_year,
        advanced.ignored_after_year,
        advanced.never_ignore_threshold,
    );

    // 2. the best series guess (with the trade-paperback bail-out)
    let Some(series_ref) = find_best_series(book, score, current_year, series_refs, cv) else {
        return Ok(None);
    };

    // 3. the local and remote hashes must match: the series ref
    //    becomes an IssueRef when the issue number resolves
    let issue_ref = if !book.issue_num.is_empty() {
        cv.query_issue_ref(&series_ref, &book.issue_num)?
    } else {
        None
    };
    let hash_local = page0.and_then(local_hash);

    let mut matches = false;
    match &issue_ref {
        Some(issue_ref) => {
            if let Some(thumb) = &issue_ref.thumb_url {
                matches = similarity(hash_local, remote_hash(cv, thumb)) > MATCH_THRESHOLD;
            }
            // an IssueRef also tries the alternate cover images
            if !matches {
                let issue = cv.query_issue(issue_ref, true)?;
                for url in &issue.image_urls {
                    if similarity(hash_local, remote_hash(cv, url)) > MATCH_THRESHOLD {
                        matches = true;
                        break;
                    }
                }
            }
        }
        None => {
            if let Some(thumb) = &series_ref.thumb_url {
                matches = similarity(hash_local, remote_hash(cv, thumb)) > MATCH_THRESHOLD;
            }
        }
    }
    Ok(if matches { Some(series_ref) } else { None })
}

/// `__find_best_series`: the top-scoring series, unless the cover of
/// the first issue is too similar to the runner-up (a trade paperback
/// vs a regular issue — the guess is unreliable, so bail).
fn find_best_series(
    book: &BookData,
    score: &MatchScore,
    current_year: i32,
    series_refs: Vec<SeriesRef>,
    cv: &mut Cv,
) -> Option<SeriesRef> {
    if series_refs.is_empty() {
        return None;
    }

    let best_of = |pool: &[SeriesRef]| -> Option<SeriesRef> {
        // the C# reduce keeps the FIRST candidate on ties (>= keeps x)
        let mut best: Option<(f64, &SeriesRef)> = None;
        for candidate in pool {
            let s = score.compute(book, candidate, current_year);
            let take = match &best {
                Some((best_score, _)) => s > *best_score,
                None => true,
            };
            if take {
                best = Some((s, candidate));
            }
        }
        best.map(|(_, r)| r.clone())
    };

    let primary = best_of(&series_refs);
    let mut secondary: Option<SeriesRef> = None;
    let mut tertiary: Option<SeriesRef> = None;
    if primary.is_some() {
        let pool: Vec<SeriesRef> = series_refs
            .iter()
            .filter(|r| Some(*r) != primary.as_ref())
            .cloned()
            .collect();
        secondary = best_of(&pool);
        if secondary.is_some() {
            let pool: Vec<SeriesRef> = pool
                .iter()
                .filter(|r| Some(*r) != secondary.as_ref())
                .cloned()
                .collect();
            tertiary = best_of(&pool);
        }
    }

    // the first-issue trade-paperback bail-out: near-identical covers
    // between the top candidates make the guess unreliable
    let is_first_issue = book
        .issue_num
        .parse::<f64>()
        .map(|f| f == 1.0)
        .unwrap_or(book.issue_num.is_empty());
    if is_first_issue {
        if let (Some(primary_ref), Some(secondary_ref)) = (&primary, &secondary) {
            let hash1 = cover_hash(primary_ref, cv);
            let hash2 = cover_hash(secondary_ref, cv);
            let hash3 = tertiary.as_ref().and_then(|t| cover_hash(t, cv));
            let too_similar = similarity(hash1, hash2) > SIMILARITY_THRESHOLD
                || hash3
                    .map(|h3| similarity(hash1, Some(h3)) > SIMILARITY_THRESHOLD)
                    .unwrap_or(false);
            if too_similar {
                return None;
            }
        }
    }
    primary
}

fn cover_hash(series_ref: &SeriesRef, cv: &mut Cv) -> Option<u64> {
    let thumb = series_ref.thumb_url.clone()?;
    remote_hash(cv, &thumb)
}

/// Decodes a remote cover url and hashes it (the C# db.query_image
/// strips the back cover before hashing).
fn remote_hash(cv: &mut Cv, url: &str) -> Option<u64> {
    let bytes = cv.query_image(url)?;
    let image = cr_image::decode(&bytes).ok()?;
    hash(&strip_back_cover(&image))
}

/// The book's cover hash (`__get_local_hash`): the page-0 image is
/// stripped and hashed.
pub fn local_hash(page0: &Image) -> Option<u64> {
    hash(&strip_back_cover(page0))
}
