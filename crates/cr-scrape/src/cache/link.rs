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

use super::IssueSkeleton;
use crate::bookdata::get_custom_value;

/// One book matched to the cached issue it corresponds to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LinkedBook {
    pub book_id: CrGuid,
    pub issue_id: i64,
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
}
