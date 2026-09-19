//! Links a series' books to a Comic Vine volume using ONLY the local
//! cache skeleton — no network. The caller resolves the volume id once
//! (a single Comic Vine search, or an existing vote already present on
//! another book of the series); everything here matches each book's
//! own issue number against the cached issue list purely locally.
//!
//! Everything here is pure. The caller reads the library, writes the
//! custom values, and saves the books.

use std::collections::HashMap;

use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::CrGuid;

use super::{IssueSkeleton, VolumeRow};
use crate::bookdata::{get_custom_value, set_custom_value};
use crate::config::Configuration;

/// One book matched to the cached issue it corresponds to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkedBook {
    pub book_id: CrGuid,
    pub issue_id: i64,
}

/// The result of filling links and shared series metadata from cache.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EnrichmentReport {
    pub books: Vec<ComicBook>,
    pub linked: usize,
    pub metadata_filled: usize,
    pub unmatched: usize,
}

/// Fills blank links and blank series-level metadata from one cached
/// Comic Vine volume. Existing values are never replaced.
pub fn enrich_series_from_cache(
    candidates: &[ComicBook],
    issues: &[IssueSkeleton],
    volume: &VolumeRow,
    config: &Configuration,
) -> EnrichmentReport {
    let mut by_number: HashMap<String, i64> = HashMap::new();
    for issue in issues {
        let key = super::missing::normalize_number(&issue.issue_number);
        by_number.entry(key).or_insert(issue.issue_id);
    }
    cr_core::trace::trace(format!(
        "series enrichment index volume_id={} candidates={} cached_issues={} normalized_keys={}",
        volume.volume_id,
        candidates.len(),
        issues.len(),
        by_number.len()
    ));

    let (publisher, imprint) = volume
        .publisher
        .as_deref()
        .map(|raw| crate::bookdata::convert_volume_publisher(raw, config))
        .unwrap_or_default();
    let mut report = EnrichmentReport::default();

    for candidate in candidates {
        let mut book = candidate.clone();
        let mut changed = false;
        let mut metadata_changed = false;
        let existing_volume = get_custom_value(&book, "comicvine_volume");
        let existing_issue = get_custom_value(&book, "comicvine_issue");

        if existing_volume.is_empty() {
            set_custom_value(&mut book, "comicvine_volume", &volume.volume_id.to_string());
            changed = true;
        }

        if book.info.publisher.trim().is_empty() && !publisher.is_empty() {
            book.info.publisher = publisher.clone();
            changed = true;
            metadata_changed = true;
        }
        if book.info.imprint.trim().is_empty() && !imprint.is_empty() {
            book.info.imprint = imprint.clone();
            changed = true;
            metadata_changed = true;
        }
        if book.info.volume == -1 {
            if let Some(year) = volume.start_year.filter(|year| *year > 0) {
                book.info.volume = year;
                changed = true;
                metadata_changed = true;
            }
        }

        if existing_issue.is_empty() {
            let key = super::missing::normalize_number(&book.info.number);
            let matched_issue = by_number.get(&key).copied();
            cr_core::trace::trace(format!(
                "series enrichment candidate book_id={:?} raw_number={:?} normalized_number={:?} existing_volume={:?} existing_issue={:?} matched_issue={matched_issue:?}",
                book.id,
                book.info.number,
                key,
                existing_volume,
                existing_issue
            ));
            if let Some(issue_id) = matched_issue {
                set_custom_value(&mut book, "comicvine_issue", &issue_id.to_string());
                report.linked += 1;
                changed = true;
            } else {
                report.unmatched += 1;
            }
        } else {
            cr_core::trace::trace(format!(
                "series enrichment candidate book_id={:?} raw_number={:?} existing_volume={:?} existing_issue={:?} action=skip_existing_issue",
                book.id, book.info.number, existing_volume, existing_issue
            ));
        }

        if metadata_changed {
            report.metadata_filled += 1;
        }
        if changed {
            report.books.push(book);
        }
    }
    cr_core::trace::trace(format!(
        "series enrichment result volume_id={} linked={} unmatched={} metadata_filled={} changed_books={}",
        volume.volume_id,
        report.linked,
        report.unmatched,
        report.metadata_filled,
        report.books.len()
    ));
    report
}

/// Matches each candidate's issue number against the cached issue list
/// of one volume, using the same number normalization the gap engine
/// uses (`super::missing::normalize_number`), so `"007"` matches `"7"`
/// the same way it does when computing gaps.
///
/// A book that already carries a `comicvine_issue` custom value is left
/// alone — never re-linked — and does not count as unmatched. A
/// remaining book whose number matches no cached issue is left alone
/// and counted in the returned unmatched total. On a duplicate cached
/// issue number, the first occurrence wins.
pub fn match_series_to_volume(
    candidates: &[ComicBook],
    issues: &[IssueSkeleton],
) -> (Vec<LinkedBook>, usize) {
    let mut by_number: HashMap<String, i64> = HashMap::new();
    for issue in issues {
        let key = super::missing::normalize_number(&issue.issue_number);
        by_number.entry(key).or_insert(issue.issue_id);
    }

    let mut linked = Vec::new();
    let mut unmatched = 0;
    for book in candidates {
        if !get_custom_value(book, "comicvine_issue").is_empty() {
            continue;
        }
        let key = super::missing::normalize_number(&book.info.number);
        match by_number.get(&key) {
            Some(&issue_id) => linked.push(LinkedBook {
                book_id: book.id,
                issue_id,
            }),
            None => unmatched += 1,
        }
    }
    (linked, unmatched)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(number: &str, issue_id_custom: Option<&str>) -> ComicBook {
        let mut book = ComicBook::default();
        book.info.number = number.to_string();
        if let Some(issue_id) = issue_id_custom {
            crate::bookdata::set_custom_value(&mut book, "comicvine_issue", issue_id);
        }
        book
    }

    fn issue(id: i64, number: &str) -> IssueSkeleton {
        IssueSkeleton {
            issue_id: id,
            volume_id: 100,
            issue_number: number.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn a_normalized_number_match_links_the_book() {
        let candidates = vec![book("007", None)];
        let issues = vec![issue(1, "7")];
        let (linked, unmatched) = match_series_to_volume(&candidates, &issues);
        assert_eq!(
            linked,
            vec![LinkedBook {
                book_id: candidates[0].id,
                issue_id: 1
            }]
        );
        assert_eq!(unmatched, 0);
    }

    #[test]
    fn an_already_linked_book_is_skipped_and_not_counted() {
        let candidates = vec![book("1", Some("999"))];
        let issues = vec![issue(1, "1")];
        let (linked, unmatched) = match_series_to_volume(&candidates, &issues);
        assert!(linked.is_empty());
        assert_eq!(unmatched, 0);
    }

    #[test]
    fn a_number_with_no_cached_match_is_counted_unmatched() {
        let candidates = vec![book("99", None)];
        let issues = vec![issue(1, "1")];
        let (linked, unmatched) = match_series_to_volume(&candidates, &issues);
        assert!(linked.is_empty());
        assert_eq!(unmatched, 1);
    }

    #[test]
    fn empty_candidates_and_empty_issues_produce_nothing() {
        assert_eq!(match_series_to_volume(&[], &[]), (Vec::new(), 0));
        let candidates = vec![book("1", None)];
        let (linked, unmatched) = match_series_to_volume(&candidates, &[]);
        assert!(linked.is_empty());
        assert_eq!(unmatched, 1);
        let issues = vec![issue(1, "1")];
        let (linked, unmatched) = match_series_to_volume(&[], &issues);
        assert!(linked.is_empty());
        assert_eq!(unmatched, 0);
    }

    #[test]
    fn a_duplicate_cached_number_keeps_the_first_occurrence() {
        let candidates = vec![book("1", None)];
        let issues = vec![issue(1, "1"), issue(2, "1")];
        let (linked, _) = match_series_to_volume(&candidates, &issues);
        assert_eq!(
            linked,
            vec![LinkedBook {
                book_id: candidates[0].id,
                issue_id: 1
            }]
        );
    }

    #[test]
    fn enrichment_fills_only_blank_series_fields_and_links() {
        let mut first = book("1", None);
        first.info.publisher = "My Publisher".to_string();
        let second = book("2", None);
        let volume = VolumeRow {
            volume_id: 100,
            publisher: Some("Vertigo".to_string()),
            start_year: Some(2000),
            ..Default::default()
        };
        let report = enrich_series_from_cache(
            &[first, second],
            &[issue(1, "1"), issue(2, "2")],
            &volume,
            &Configuration::default(),
        );

        assert_eq!(report.linked, 2);
        assert_eq!(report.metadata_filled, 2);
        assert_eq!(report.unmatched, 0);
        assert_eq!(report.books[0].info.publisher, "My Publisher");
        assert_eq!(report.books[1].info.publisher, "DC Comics");
        assert_eq!(report.books[0].info.imprint, "Vertigo");
        assert_eq!(report.books[1].info.imprint, "Vertigo");
        assert_eq!(report.books[0].info.volume, 2000);
        assert_eq!(get_custom_value(&report.books[0], "comicvine_issue"), "1");
    }

    #[test]
    fn enrichment_preserves_existing_links_and_metadata() {
        let mut candidate = book("1", Some("999"));
        set_custom_value(&mut candidate, "comicvine_volume", "888");
        candidate.info.publisher = "Local Publisher".to_string();
        candidate.info.imprint = "Local Imprint".to_string();
        candidate.info.volume = 1999;
        let volume = VolumeRow {
            volume_id: 100,
            publisher: Some("DC Comics".to_string()),
            start_year: Some(2000),
            ..Default::default()
        };
        let report = enrich_series_from_cache(
            &[candidate],
            &[issue(1, "1")],
            &volume,
            &Configuration::default(),
        );

        assert!(report.books.is_empty());
        assert_eq!(report.linked, 0);
        assert_eq!(report.metadata_filled, 0);
        assert_eq!(report.unmatched, 0);
    }

    #[test]
    fn enrichment_fills_common_metadata_when_the_issue_is_not_cached() {
        let candidate = book("99", None);
        let volume = VolumeRow {
            volume_id: 100,
            publisher: Some("DC Comics".to_string()),
            start_year: Some(2000),
            ..Default::default()
        };
        let report = enrich_series_from_cache(
            &[candidate],
            &[issue(1, "1")],
            &volume,
            &Configuration::default(),
        );

        assert_eq!(report.books.len(), 1);
        assert_eq!(report.linked, 0);
        assert_eq!(report.metadata_filled, 1);
        assert_eq!(report.unmatched, 1);
        assert_eq!(
            get_custom_value(&report.books[0], "comicvine_volume"),
            "100"
        );
        assert!(get_custom_value(&report.books[0], "comicvine_issue").is_empty());
    }
}
