//! Which issues of a volume the library does not hold (Phase 15 T7).
//!
//! The cache skeleton knows every issue of a volume. The library knows
//! which issue numbers it holds. The difference is the gap the user
//! fills with fileless books.
//!
//! Everything here is pure. The caller reads the library, and the
//! caller creates the books.

use std::collections::{BTreeMap, HashMap};

use cr_core::model::comic_book::ComicBook;

use super::{CvCache, IssueSkeleton};

/// One issue the library does not hold.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MissingIssue {
    pub issue_id: i64,
    pub issue_number: String,
    pub cover_date: Option<String>,
    pub name: Option<String>,
}

/// Compares two issue numbers the way a reader does.
///
/// `1`, `01`, and `001` are the same issue. `1A` and `1a` are the same
/// issue. `1.5` and `1½` are not compared here, because Comic Vine
/// writes the half-issue form in more than one way and the caller can
/// still see both rows.
pub fn normalize_number(number: &str) -> String {
    let trimmed = number.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return String::new();
    }
    // Strip the leading zeros of the leading digit run only, so `007`
    // becomes `7` and `0.5` keeps its zero.
    let digits: String = trimmed.chars().take_while(char::is_ascii_digit).collect();
    if digits.len() > 1 && !trimmed.starts_with("0.") {
        let rest = &trimmed[digits.len()..];
        let stripped = digits.trim_start_matches('0');
        let head = if stripped.is_empty() { "0" } else { stripped };
        return format!("{head}{rest}");
    }
    trimmed
}

/// The issues of `volume` that no owned number matches.
///
/// An owned number that the volume does not list is ignored, because
/// it is the user's business, not a defect.
pub fn missing_issues(volume: &[IssueSkeleton], owned_numbers: &[String]) -> Vec<MissingIssue> {
    let owned: std::collections::BTreeSet<String> = owned_numbers
        .iter()
        .map(|n| normalize_number(n))
        .filter(|n| !n.is_empty())
        .collect();

    volume
        .iter()
        .filter(|issue| {
            let key = normalize_number(&issue.issue_number);
            // An issue with no number cannot be matched, so it is
            // always offered.
            key.is_empty() || !owned.contains(&key)
        })
        .map(|issue| MissingIssue {
            issue_id: issue.issue_id,
            issue_number: issue.issue_number.clone(),
            cover_date: issue.cover_date.clone(),
            name: issue.name.clone(),
        })
        .collect()
}

/// The Comic Vine volume id that a set of books agrees on.
///
/// A library series carries a volume id only after a scrape. The
/// function returns the id that the most books name; `None` means the
/// caller must ask the user to pick the volume.
pub fn volume_id_of(series_keys: impl IntoIterator<Item = String>) -> Option<i64> {
    let mut votes: BTreeMap<i64, usize> = BTreeMap::new();
    for key in series_keys {
        if let Ok(id) = key.trim().parse::<i64>() {
            if id > 0 {
                *votes.entry(id).or_default() += 1;
            }
        }
    }
    votes
        .into_iter()
        .max_by_key(|&(id, count)| (count, std::cmp::Reverse(id)))
        .map(|(id, _)| id)
}

/// The gap-pass grouping key: case-folded visible series name + stored volume.
/// The series uses the enabled filename proposal when its stored value is
/// empty, as the browser does (ADR-067).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct GapSeriesKey {
    series: String,
    volume: i32,
}

#[derive(Clone, Copy, Debug, Default)]
struct GapScopeTrace {
    books: usize,
    blank_stored_series: usize,
    blank_stored_numbers: usize,
    proposed_enabled: usize,
    linked_issue_ids: usize,
}

/// One series' gap report: the volume it was matched to and the issues
/// the library does not hold.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SeriesGap {
    pub series: String,
    pub volume: i32,
    pub volume_id: i64,
    pub missing: Vec<MissingIssue>,
}

/// The missing issues of every series in `scope_books`, read from the
/// local cache skeleton only (never the network).
///
/// `scope_books` and `library_books` are deliberately two different
/// populations. `scope_books` decides which series to report on and
/// which issue numbers count as owned — this is the scoping the user
/// picked (the whole library, or one smart list's matched subset).
/// `library_books` decides which Comic Vine volume each series maps
/// to. These must NOT be the same restriction: which volume a series
/// corresponds to is a fact about the series, not about whichever
/// arbitrary subset the user scoped the report to, and a book only
/// carries the `comicvine_volume` vote after an actual scrape or a
/// Comic-Vine-driven fill — usually a handful of copies, not the bulk
/// of a collection. A scope narrow enough to exclude every one of a
/// series' Comic-Vine-linked copies must not make that series silently
/// vanish from the report; only its OWNED numbers should be scoped.
/// (Before this fix, both maps came from `scope_books` alone, so a
/// smart list scoped to exactly the un-linked copies of a series
/// always read back "0 missing" for it — the linked copies elsewhere
/// in the library were invisible to the vote.)
///
/// Series and Number use their enabled filename proposals when their stored
/// values are empty. This keeps the identity equal to the values that the
/// browser and cache-linking command use (ADR-065 and ADR-067).
///
/// Builds the owned-numbers-by-series map in ONE pass over
/// `scope_books` and the volume-vote map in a SEPARATE single pass
/// over `library_books`, then reads the cache once per distinct scoped
/// series. A series with no `comicvine_volume` custom value carrying a
/// valid vote ANYWHERE in the library, or with an empty cache, yields
/// no rows for that series. Do not scan either slice once per series:
/// that per-series full-collection scan is the exact bug
/// `project_incoming_external_gaps` (`crates/cr-ui/src/library.rs`)
/// still carries, and it drove idle CPU to 100% on 2026-09-18 the last
/// time this shape recurred (see `docs/current-status.md`).
pub fn missing_issues_of_library(
    cache: &dyn CvCache,
    scope_books: &[ComicBook],
    library_books: &[ComicBook],
) -> Vec<SeriesGap> {
    fn identity_of(book: &ComicBook) -> (GapSeriesKey, String, String) {
        let use_proposed_series = book.enable_proposed && book.info.series.is_empty();
        let use_proposed_number = book.enable_proposed && book.info.number.trim().is_empty();
        let proposed = (use_proposed_series || use_proposed_number)
            .then(|| cr_core::model::comic_name_info::from_file_path(&book.file_path));
        let series = if use_proposed_series {
            proposed
                .as_ref()
                .map(|info| info.series.clone())
                .unwrap_or_default()
        } else {
            book.info.series.clone()
        };
        let number = if use_proposed_number {
            proposed
                .as_ref()
                .map(|info| info.number.clone())
                .unwrap_or_default()
        } else {
            book.info.number.clone()
        };
        (
            GapSeriesKey {
                series: series.trim().to_ascii_lowercase(),
                volume: book.info.volume,
            },
            series,
            number,
        )
    }

    let tracing = cr_core::trace::enabled();
    if tracing {
        cr_core::trace::trace(format!(
            "missing issues start scope_books={} library_books={}",
            scope_books.len(),
            library_books.len()
        ));
    }

    let mut owned: HashMap<GapSeriesKey, Vec<String>> = HashMap::new();
    let mut display_name: HashMap<GapSeriesKey, String> = HashMap::new();
    // Trace-only indexes contain metadata values but never paths or book IDs.
    // They explain when an issue link exists under a different Series/Volume
    // key from the gap row that reports it as missing.
    let mut scope_stats: HashMap<GapSeriesKey, GapScopeTrace> = HashMap::new();
    let mut scope_issue_links: HashMap<i64, Vec<(GapSeriesKey, String)>> = HashMap::new();
    for book in scope_books {
        let (key, series, number) = identity_of(book);
        if tracing {
            let stats = scope_stats.entry(key.clone()).or_default();
            stats.books += 1;
            stats.blank_stored_series += usize::from(book.info.series.is_empty());
            stats.blank_stored_numbers += usize::from(book.info.number.trim().is_empty());
            stats.proposed_enabled += usize::from(book.enable_proposed);
            let issue_link = crate::bookdata::get_custom_value(book, "comicvine_issue");
            if let Ok(issue_id) = issue_link.trim().parse::<i64>() {
                if issue_id > 0 {
                    stats.linked_issue_ids += 1;
                    scope_issue_links
                        .entry(issue_id)
                        .or_default()
                        .push((key.clone(), number.clone()));
                }
            }
        }
        owned.entry(key.clone()).or_default().push(number);
        display_name.entry(key).or_insert(series);
    }

    let mut volume_votes: HashMap<GapSeriesKey, Vec<String>> = HashMap::new();
    for book in library_books {
        let (key, _, _) = identity_of(book);
        volume_votes
            .entry(key)
            .or_default()
            .push(crate::bookdata::get_custom_value(book, "comicvine_volume"));
    }

    let mut results = Vec::new();
    for (key, numbers) in owned {
        let votes = volume_votes.remove(&key).unwrap_or_default();
        let mut vote_counts = BTreeMap::new();
        if tracing {
            for vote in &votes {
                if let Ok(id) = vote.trim().parse::<i64>() {
                    if id > 0 {
                        *vote_counts.entry(id).or_insert(0usize) += 1;
                    }
                }
            }
        }
        let shown_series = display_name.get(&key).cloned().unwrap_or_default();
        let stats = scope_stats.get(&key).copied().unwrap_or_default();
        let Some(volume_id) = volume_id_of(votes) else {
            if tracing {
                cr_core::trace::trace(format!(
                    "missing issues group match_series={:?} volume={} display_series={:?} scope_books={} blank_stored_series={} blank_stored_numbers={} proposed_enabled={} linked_issue_ids={} volume_votes={vote_counts:?} result=no_volume_vote",
                    key.series,
                    key.volume,
                    shown_series,
                    stats.books,
                    stats.blank_stored_series,
                    stats.blank_stored_numbers,
                    stats.proposed_enabled,
                    stats.linked_issue_ids
                ));
            }
            continue;
        };
        let issues = match cache.issues_of_volume(volume_id) {
            Ok(issues) => issues,
            Err(error) => {
                if tracing {
                    cr_core::trace::trace(format!(
                        "missing issues group match_series={:?} volume={} display_series={:?} scope_books={} blank_stored_series={} blank_stored_numbers={} proposed_enabled={} linked_issue_ids={} volume_votes={vote_counts:?} chosen_volume_id={} result=cache_error error={error}",
                        key.series,
                        key.volume,
                        shown_series,
                        stats.books,
                        stats.blank_stored_series,
                        stats.blank_stored_numbers,
                        stats.proposed_enabled,
                        stats.linked_issue_ids,
                        volume_id
                    ));
                }
                continue;
            }
        };
        let missing = missing_issues(&issues, &numbers);

        if tracing {
            let mut linked_sources: BTreeMap<String, usize> = BTreeMap::new();
            let mut examples = Vec::new();
            let mut missing_with_scoped_issue_link = 0usize;
            for issue in &missing {
                let Some(sources) = scope_issue_links.get(&issue.issue_id) else {
                    continue;
                };
                missing_with_scoped_issue_link += 1;
                for (source_key, source_number) in sources {
                    let source = format!(
                        "match_series={:?},volume={}",
                        source_key.series, source_key.volume
                    );
                    *linked_sources.entry(source).or_default() += 1;
                    if examples.len() < 5 {
                        examples.push(format!(
                            "issue_id={},cached_number={:?},source_series={:?},source_volume={},source_match_number={:?}",
                            issue.issue_id,
                            issue.issue_number,
                            source_key.series,
                            source_key.volume,
                            source_number
                        ));
                    }
                }
            }
            cr_core::trace::trace(format!(
                "missing issues group match_series={:?} volume={} display_series={:?} scope_books={} blank_stored_series={} blank_stored_numbers={} proposed_enabled={} linked_issue_ids={} volume_votes={vote_counts:?} chosen_volume_id={} cached_issues={} missing={} missing_with_scoped_issue_link={} linked_sources={linked_sources:?}",
                key.series,
                key.volume,
                shown_series,
                stats.books,
                stats.blank_stored_series,
                stats.blank_stored_numbers,
                stats.proposed_enabled,
                stats.linked_issue_ids,
                volume_id,
                issues.len(),
                missing.len(),
                missing_with_scoped_issue_link
            ));
            for example in examples {
                cr_core::trace::trace(format!("missing issues linked example {example}"));
            }
        }
        if missing.is_empty() {
            continue;
        }
        results.push(SeriesGap {
            series: display_name.remove(&key).unwrap_or_default(),
            volume: key.volume,
            volume_id,
            missing,
        });
    }
    if tracing {
        cr_core::trace::trace(format!(
            "missing issues finish series_groups={} missing_rows={}",
            results.len(),
            results.iter().map(|gap| gap.missing.len()).sum::<usize>()
        ));
    }
    results
}

/// The four-digit year of a Comic Vine cover date ("YYYY-MM-DD", with an
/// optional time suffix). `None` when absent or malformed.
pub fn year_of_cover_date(cover_date: Option<&str>) -> Option<i32> {
    let value = cover_date?.trim();
    let head = value.split([' ', 'T']).next()?;
    head.split('-').next()?.parse::<i32>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::SqliteCache;

    fn cache() -> SqliteCache {
        SqliteCache::in_memory().expect("in-memory cache opens")
    }

    fn series_book(series: &str, volume: i32, number: &str, volume_id: Option<i64>) -> ComicBook {
        let mut book = ComicBook::default();
        book.info.series = series.to_string();
        book.info.volume = volume;
        book.info.number = number.to_string();
        if let Some(volume_id) = volume_id {
            crate::bookdata::set_custom_value(
                &mut book,
                "comicvine_volume",
                &volume_id.to_string(),
            );
        }
        book
    }

    fn numbers_of(gap: &SeriesGap) -> Vec<&str> {
        gap.missing
            .iter()
            .map(|m| m.issue_number.as_str())
            .collect()
    }

    fn issue(id: i64, number: &str) -> IssueSkeleton {
        IssueSkeleton {
            issue_id: id,
            volume_id: 771,
            issue_number: number.to_string(),
            ..Default::default()
        }
    }

    fn numbers(missing: &[MissingIssue]) -> Vec<&str> {
        missing.iter().map(|m| m.issue_number.as_str()).collect()
    }

    #[test]
    fn leading_zeros_do_not_make_a_new_issue() {
        assert_eq!(normalize_number("1"), "1");
        assert_eq!(normalize_number("01"), "1");
        assert_eq!(normalize_number("001"), "1");
        assert_eq!(normalize_number(" 007 "), "7");
        assert_eq!(normalize_number("010"), "10");
        assert_eq!(normalize_number("0"), "0");
        assert_eq!(normalize_number("000"), "0");
    }

    #[test]
    fn a_letter_suffix_keeps_its_issue_and_loses_its_case() {
        assert_eq!(normalize_number("1A"), "1a");
        assert_eq!(normalize_number("01a"), "1a");
        assert_eq!(normalize_number("AU"), "au");
    }

    #[test]
    fn a_decimal_number_keeps_its_leading_zero() {
        assert_eq!(normalize_number("0.5"), "0.5");
        assert_eq!(normalize_number("1.5"), "1.5");
    }

    #[test]
    fn an_empty_number_normalizes_to_empty() {
        assert_eq!(normalize_number(""), "");
        assert_eq!(normalize_number("   "), "");
    }

    #[test]
    fn the_gap_is_what_the_library_does_not_hold() {
        let volume = vec![issue(1, "1"), issue(2, "2"), issue(3, "3"), issue(4, "4")];
        let owned = vec!["1".to_string(), "3".to_string()];
        assert_eq!(numbers(&missing_issues(&volume, &owned)), vec!["2", "4"]);
    }

    #[test]
    fn a_padded_owned_number_still_matches() {
        let volume = vec![issue(1, "1"), issue(2, "02"), issue(3, "3")];
        let owned = vec!["001".to_string(), "2".to_string()];
        assert_eq!(numbers(&missing_issues(&volume, &owned)), vec!["3"]);
    }

    #[test]
    fn an_owned_number_the_volume_does_not_list_changes_nothing() {
        let volume = vec![issue(1, "1")];
        let owned = vec!["1".to_string(), "99".to_string()];
        assert!(missing_issues(&volume, &owned).is_empty());
    }

    #[test]
    fn an_issue_with_no_number_is_always_offered() {
        let volume = vec![issue(1, "1"), issue(2, "")];
        let owned = vec!["1".to_string()];
        assert_eq!(numbers(&missing_issues(&volume, &owned)), vec![""]);
    }

    #[test]
    fn an_empty_library_is_missing_everything() {
        let volume = vec![issue(1, "1"), issue(2, "2")];
        assert_eq!(missing_issues(&volume, &[]).len(), 2);
    }

    #[test]
    fn the_missing_row_carries_the_detail_the_dialog_shows() {
        let volume = vec![IssueSkeleton {
            issue_id: 92_469,
            volume_id: 771,
            issue_number: "1".into(),
            cover_date: Some("2000-11-01".into()),
            name: Some("Quelque part entre les ombres".into()),
            ..Default::default()
        }];
        let got = missing_issues(&volume, &[]);
        assert_eq!(got[0].issue_id, 92_469);
        assert_eq!(got[0].cover_date.as_deref(), Some("2000-11-01"));
        assert_eq!(
            got[0].name.as_deref(),
            Some("Quelque part entre les ombres")
        );
    }

    #[test]
    fn the_volume_id_is_the_one_the_most_books_name() {
        let keys = ["771", "771", "999", "", "not a number", "0", "-3"];
        assert_eq!(volume_id_of(keys.iter().map(|s| s.to_string())), Some(771));
    }

    #[test]
    fn no_scraped_book_means_no_volume_id() {
        assert_eq!(volume_id_of(Vec::<String>::new()), None);
        assert_eq!(
            volume_id_of(["", "  ", "x"].iter().map(|s| s.to_string())),
            None
        );
    }

    #[test]
    fn a_tie_picks_the_lower_id_so_the_answer_is_stable() {
        assert_eq!(
            volume_id_of(["999", "771"].iter().map(|s| s.to_string())),
            Some(771)
        );
        assert_eq!(
            volume_id_of(["771", "999"].iter().map(|s| s.to_string())),
            Some(771)
        );
    }

    #[test]
    fn owned_subtraction_is_per_series_across_several_series() {
        let cache = cache();
        cache
            .put_issues(&[
                IssueSkeleton {
                    issue_id: 1,
                    volume_id: 100,
                    issue_number: "1".into(),
                    ..Default::default()
                },
                IssueSkeleton {
                    issue_id: 2,
                    volume_id: 100,
                    issue_number: "2".into(),
                    ..Default::default()
                },
                IssueSkeleton {
                    issue_id: 3,
                    volume_id: 200,
                    issue_number: "1".into(),
                    ..Default::default()
                },
                IssueSkeleton {
                    issue_id: 4,
                    volume_id: 200,
                    issue_number: "2".into(),
                    ..Default::default()
                },
            ])
            .expect("seed issues");
        let books = vec![
            series_book("Alpha", 2020, "1", Some(100)),
            series_book("Beta", 2021, "1", Some(200)),
        ];

        let mut gaps = missing_issues_of_library(&cache, &books, &books);
        gaps.sort_by(|a, b| a.series.cmp(&b.series));

        assert_eq!(gaps.len(), 2);
        assert_eq!(gaps[0].series, "Alpha");
        assert_eq!(numbers_of(&gaps[0]), vec!["2"]);
        assert_eq!(gaps[1].series, "Beta");
        assert_eq!(numbers_of(&gaps[1]), vec!["2"]);
    }

    #[test]
    fn a_series_with_no_volume_id_is_skipped() {
        let cache = cache();
        cache
            .put_issues(&[IssueSkeleton {
                issue_id: 1,
                volume_id: 100,
                issue_number: "1".into(),
                ..Default::default()
            }])
            .expect("seed issues");
        let books = vec![series_book("Alpha", 2020, "1", None)];

        assert!(missing_issues_of_library(&cache, &books, &books).is_empty());
    }

    #[test]
    fn an_empty_cache_yields_nothing() {
        let cache = cache();
        let books = vec![series_book("Alpha", 2020, "1", Some(100))];

        assert!(missing_issues_of_library(&cache, &books, &books).is_empty());
    }

    #[test]
    fn a_scope_missing_every_linked_copy_still_votes_from_the_whole_library() {
        // The exact bug the user found: a smart list (the scope) that
        // happens to hold none of a series' Comic-Vine-linked copies
        // must not make the whole series vanish from the report — the
        // volume vote must come from the library, not the scope.
        let cache = cache();
        cache
            .put_issues(&[
                IssueSkeleton {
                    issue_id: 1,
                    volume_id: 100,
                    issue_number: "1".into(),
                    ..Default::default()
                },
                IssueSkeleton {
                    issue_id: 2,
                    volume_id: 100,
                    issue_number: "2".into(),
                    ..Default::default()
                },
            ])
            .expect("seed issues");
        let unlinked = series_book("Alpha", 2020, "1", None);
        let linked = series_book("Alpha", 2020, "1", Some(100));
        let library = vec![unlinked.clone(), linked];

        let gaps = missing_issues_of_library(&cache, std::slice::from_ref(&unlinked), &library);

        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].series, "Alpha");
        assert_eq!(numbers_of(&gaps[0]), vec!["2"]);
    }

    #[test]
    fn proposed_series_and_number_join_the_stored_series_gap() {
        let cache = cache();
        cache
            .put_issues(&[
                IssueSkeleton {
                    issue_id: 1,
                    volume_id: 19_752,
                    issue_number: "1".into(),
                    ..Default::default()
                },
                IssueSkeleton {
                    issue_id: 2,
                    volume_id: 19_752,
                    issue_number: "2".into(),
                    ..Default::default()
                },
                IssueSkeleton {
                    issue_id: 3,
                    volume_id: 19_752,
                    issue_number: "3".into(),
                    ..Default::default()
                },
            ])
            .expect("seed issues");

        let mut stored = series_book("Alpha", 1977, "1", Some(19_752));
        crate::bookdata::set_custom_value(&mut stored, "comicvine_issue", "1");
        let mut proposed = ComicBook {
            file_path: "/library/Alpha 2.cbz".into(),
            ..Default::default()
        };
        proposed.info.volume = 1977;
        crate::bookdata::set_custom_value(&mut proposed, "comicvine_volume", "19752");
        crate::bookdata::set_custom_value(&mut proposed, "comicvine_issue", "2");
        let books = vec![stored, proposed];

        let gaps = missing_issues_of_library(&cache, &books, &books);

        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].series, "Alpha");
        assert_eq!(gaps[0].volume, 1977);
        assert_eq!(numbers_of(&gaps[0]), vec!["3"]);
    }

    #[test]
    fn an_owned_number_the_volume_does_not_list_is_ignored_by_the_gap_engine() {
        let cache = cache();
        cache
            .put_issues(&[IssueSkeleton {
                issue_id: 1,
                volume_id: 100,
                issue_number: "1".into(),
                ..Default::default()
            }])
            .expect("seed issues");
        let books = vec![
            series_book("Alpha", 2020, "1", Some(100)),
            series_book("Alpha", 2020, "99", Some(100)),
        ];

        assert!(missing_issues_of_library(&cache, &books, &books).is_empty());
    }

    #[test]
    fn year_of_cover_date_reads_the_leading_four_digits() {
        assert_eq!(year_of_cover_date(Some("2000-11-01")), Some(2000));
        assert_eq!(year_of_cover_date(Some("2000-11-01 00:00:00")), Some(2000));
        assert_eq!(year_of_cover_date(Some("2000-11-01T00:00:00")), Some(2000));
        assert_eq!(year_of_cover_date(Some("")), None);
        assert_eq!(year_of_cover_date(Some("unknown")), None);
        assert_eq!(year_of_cover_date(None), None);
    }
}
