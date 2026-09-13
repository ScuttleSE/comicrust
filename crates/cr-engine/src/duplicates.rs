//! Duplicate cleanup: rank the copies of every duplicate group and
//! pick the worst.
//!
//! PORT ADDITION (no C# counterpart — ADR-044): the C#
//! `ComicBookDuplicateMatcher` only filters; this module adds the
//! "Select Worst Duplicates" command behind it. The ranking is a
//! score sum: a copy gets ONE penalty per enabled rule under which it
//! is strictly worse than the group best, and the copies whose
//! penalty exceeds the group minimum are the worst copies. The group
//! minimum survives — when every copy loses at least one rule, only
//! the copies with MORE losses than the best of them are marked.
//!
//! Stated consequence of the score sum: a CBR copy that wins on size
//! and pages against a smaller CBZ can survive (each copy loses one
//! rule — a tie at the group minimum marks nothing) — unless the
//! file-date rule resolves it: when the copies' file stamps differ,
//! the older copy loses the extra rule and ranks worst.
//!
//! The fifth rule is the incoming-path rule (ADR-046, also a PORT
//! ADDITION): a copy under the configured path gets one HEAVY
//! penalty — heavier than all quality rules combined — when the
//! group also holds a copy outside the path. In a mixed group every
//! copy under the path loses (the Library copy wins over format,
//! size, pages, and stamp), while a fileless copy still ranks worst
//! wherever it sits (it loses every enabled rule, the path rule at
//! its full weight). A group entirely on one side of the path ranks
//! by the quality rules alone.

use cr_core::model::comic_book::ComicBook;
use cr_core::settings::Settings;
use cr_core::xml::scalar::CrGuid;
use cr_io::formats::{self, ids};

use crate::matcher::book_view;
use crate::matcher::eval::duplicate_groups;

/// The duplicate-cleanup rules (the `Settings.Duplicates*` fields —
/// the Preferences duplicates page rows).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DuplicateRules {
    /// A CBR copy is worse than a CBZ copy. Only the pair the rule
    /// names compares: other formats (PDF, DjVu, and the rest) take
    /// no side, and a CBR loses only when the group also holds a CBZ.
    pub cbr_worse_than_cbz: bool,
    /// A smaller file is worse than a larger file. Unknown size (−1)
    /// counts as the smallest.
    pub smaller_file_worse: bool,
    /// Fewer pages are worse than more pages. Unknown page count
    /// counts as the lowest.
    pub fewer_pages_worse: bool,
    /// An older file is worse than a newer file (PORT ADDITION, no
    /// C# counterpart). The tie-break for copies equal on every
    /// other rule — a re-downloaded or re-scanned copy carries the
    /// newer file stamp; the stale copy ranks worst. Unknown stamps
    /// (the `DateTime.MinValue` default) tie and take no side.
    pub older_file_worse: bool,
    /// The incoming-path rule (PORT ADDITION, no C# counterpart —
    /// ADR-046). A copy under this path is worse than a copy outside
    /// it, but only when the group also holds a copy outside the
    /// path. The rule carries a weight that outweighs the four
    /// quality rules combined, so in a mixed group every copy under
    /// the path loses — even one that wins on format, size, pages,
    /// and stamp. Empty means the rule is off.
    pub incoming_path: String,
}

impl Default for DuplicateRules {
    fn default() -> Self {
        DuplicateRules {
            cbr_worse_than_cbz: true,
            smaller_file_worse: true,
            fewer_pages_worse: true,
            older_file_worse: true,
            incoming_path: String::new(),
        }
    }
}

impl DuplicateRules {
    /// The rules as configured (the `Settings.Duplicates*` fields).
    pub fn from_settings(s: &Settings) -> Self {
        DuplicateRules {
            cbr_worse_than_cbz: s.duplicates_cbr_worse_than_cbz,
            smaller_file_worse: s.duplicates_smaller_file_worse,
            fewer_pages_worse: s.duplicates_fewer_pages_worse,
            older_file_worse: s.duplicates_older_file_worse,
            incoming_path: s.duplicates_incoming_path.clone(),
        }
    }

    /// The count of the enabled quality rules — a fileless copy loses
    /// every one of them (ADR-044: no file, so no format, no size, no
    /// page data — it ranks worst).
    fn enabled_count(&self) -> i32 {
        self.cbr_worse_than_cbz as i32
            + self.smaller_file_worse as i32
            + self.fewer_pages_worse as i32
            + self.older_file_worse as i32
    }

    /// The incoming-path rule is on when a path is configured.
    fn path_rule_enabled(&self) -> bool {
        !self.incoming_path.is_empty()
    }

    /// The weight of the incoming-path rule: it outweighs every
    /// quality rule combined, so a copy that loses it loses against
    /// any copy that does not (ADR-046).
    fn heavy_weight(&self) -> i32 {
        self.enabled_count() + 1
    }
}

/// The ids of the worst copies per duplicate group over `books`, in
/// the first-appearance order of their groups. Groups of one copy are
/// not duplicates and never rank; a group whose copies tie at the
/// lowest penalty marks nothing.
pub fn worst_duplicate_ids(books: &[&ComicBook], rules: &DuplicateRules) -> Vec<CrGuid> {
    let mut out = Vec::new();
    for group in duplicate_groups(books) {
        let members: Vec<&ComicBook> = group.iter().map(|&i| books[i]).collect();
        let penalties: Vec<i32> = members
            .iter()
            .map(|b| penalty(b, &members, rules))
            .collect();
        let best = penalties.iter().copied().min().unwrap_or(0);
        for (i, p) in penalties.iter().enumerate() {
            if *p > best {
                out.push(members[i].id);
            }
        }
    }
    out
}

/// The rule penalties of one copy against its group. A fileless copy
/// loses every enabled rule — the quality rules and, when the
/// incoming-path rule is on, the path rule at its full weight
/// (ADR-046: an empty record ranks worst wherever it sits).
fn penalty(book: &ComicBook, members: &[&ComicBook], rules: &DuplicateRules) -> i32 {
    if !book_view::is_linked(book) {
        let mut p = rules.enabled_count();
        if rules.path_rule_enabled() {
            p += rules.heavy_weight();
        }
        return p;
    }
    let mut p = 0;
    if rules.cbr_worse_than_cbz && format_worse(book, members) {
        p += 1;
    }
    if rules.smaller_file_worse && book.file_size < max_size(members) {
        p += 1;
    }
    if rules.fewer_pages_worse && book.info.page_count < max_pages(members) {
        p += 1;
    }
    if rules.older_file_worse && modified_secs(book) < modified_max(members) {
        p += 1;
    }
    if rules.path_rule_enabled()
        && under_incoming_path(book, rules)
        && members
            .iter()
            .any(|m| !book_view::is_linked(m) || !under_incoming_path(m, rules))
    {
        p += rules.heavy_weight();
    }
    p
}

/// The incoming-path rule: the copy's file sits under the configured
/// path. The match is a case-insensitive component prefix; `/` and
/// `\` both separate, so a configured Windows path matches its
/// lower-cased book paths the same way.
fn under_incoming_path(book: &ComicBook, rules: &DuplicateRules) -> bool {
    under_path(&book.file_path, &rules.incoming_path)
}

/// Is `path` inside the `root` directory? Component-wise prefix
/// compare, case-insensitive; empty components (a trailing
/// separator, a doubled one) are dropped.
fn under_path(path: &str, root: &str) -> bool {
    let comps = |s: &str| -> Vec<String> {
        s.split(['/', '\\'])
            .filter(|c| !c.is_empty())
            .map(|c| c.to_ascii_lowercase())
            .collect()
    };
    let root_comps = comps(root);
    if root_comps.is_empty() {
        return false;
    }
    let path_comps = comps(path);
    path_comps.len() > root_comps.len() && path_comps.starts_with(&root_comps)
}

/// The format rule: the copy is a CBR and the group also holds a CBZ.
/// The physical format comes from the file path extension (`cr-io`
/// `source_format` — the `ActualFileFormat` source), not from the
/// metadata `Format` string.
fn format_worse(book: &ComicBook, members: &[&ComicBook]) -> bool {
    if file_format(book) != Some(ids::CBR) {
        return false;
    }
    members.iter().any(|m| file_format(m) == Some(ids::CBZ))
}

fn file_format(book: &ComicBook) -> Option<i32> {
    formats::source_format(std::path::Path::new(&book.file_path)).map(|f| f.id)
}

fn max_size(members: &[&ComicBook]) -> i64 {
    members.iter().map(|b| b.file_size).max().unwrap_or(-1)
}

fn max_pages(members: &[&ComicBook]) -> i32 {
    members.iter().map(|b| b.info.page_count).max().unwrap_or(0)
}

/// The file stamp of one copy as a comparable count (naive UTC
/// seconds; kind semantics take no part — the stamp is a file
/// `mtime`, always local).
fn modified_secs(book: &ComicBook) -> i64 {
    book.file_modified_time.naive.and_utc().timestamp()
}

fn modified_max(members: &[&ComicBook]) -> i64 {
    members.iter().map(|b| modified_secs(b)).max().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (series, number, path, file size, page count). The physical
    /// format comes from the path extension.
    fn book(series: &str, number: &str, path: &str, size: i64, pages: i32) -> ComicBook {
        let mut b = ComicBook {
            id: CrGuid::new_random(),
            file_path: path.into(),
            file_size: size,
            ..Default::default()
        };
        b.info.series = series.into();
        b.info.number = number.into();
        b.info.page_count = pages;
        b
    }

    fn worst(books: &[ComicBook], rules: &DuplicateRules) -> Vec<String> {
        let refs: Vec<&ComicBook> = books.iter().collect();
        worst_duplicate_ids(&refs, rules)
            .iter()
            .filter_map(|id| {
                books
                    .iter()
                    .find(|b| b.id == *id)
                    .map(|b| b.file_path.clone())
            })
            .collect()
    }

    #[test]
    fn cbz_beats_cbr_on_every_rule() {
        let books = vec![
            book("Alpha", "1", "/c/alpha.cbz", 2000, 20),
            book("Alpha", "1", "/c/alpha.cbr", 1000, 10),
        ];
        assert_eq!(worst(&books, &DuplicateRules::default()), ["/c/alpha.cbr"]);
    }

    #[test]
    fn an_older_file_loses_when_everything_else_ties() {
        // The report case: two CBZ copies equal on size and pages,
        // one file stamp newer (a re-download). The stale copy is
        // the only loser — with every rule off it ties and marks
        // nothing again.
        let mut old = book("Tie", "1", "/c/tie-a.cbz", 100, 20);
        let mut new = book("Tie", "1", "/c/tie-b.cbz", 100, 20);
        old.file_modified_time =
            cr_core::xml::scalar::CrDateTime::parse("2020-01-01T00:00:00").unwrap();
        new.file_modified_time =
            cr_core::xml::scalar::CrDateTime::parse("2024-01-01T00:00:00").unwrap();
        assert_eq!(
            worst(&[old.clone(), new.clone()], &DuplicateRules::default()),
            ["/c/tie-a.cbz"]
        );
        let rules = DuplicateRules {
            older_file_worse: false,
            ..DuplicateRules::default()
        };

        assert!(worst(&[old, new], &rules).is_empty());
    }

    #[test]
    fn a_conflicting_tie_marks_nothing() {
        // The CBR is larger; the CBZ is not better by format alone —
        // both lose one rule, the tie at the group minimum marks
        // nothing (the ADR-044 example).
        let books = vec![
            book("Conflict", "1", "/c/conflict.cbz", 500, 30),
            book("Conflict", "1", "/c/conflict.cbr", 900, 30),
        ];
        assert!(worst(&books, &DuplicateRules::default()).is_empty());
    }

    #[test]
    fn mid_and_worst_mark_best_stays() {
        // Best: CBZ, large, many pages. Mid: CBZ, smaller. Worst: CBR.
        let books = vec![
            book("Best", "1", "/c/best.cbr", 1000, 10), // 3 losses
            book("Best", "1", "/c/mid.cbz", 1000, 20),  // 1 loss
            book("Best", "1", "/c/top.cbz", 2000, 20),  // 0 losses
        ];
        assert_eq!(
            worst(&books, &DuplicateRules::default()),
            ["/c/best.cbr", "/c/mid.cbz"]
        );
    }

    #[test]
    fn disabled_rules_change_the_penalties() {
        // Without the format rule the smaller CBZ of the conflict
        // pair loses alone.
        let rules = DuplicateRules {
            cbr_worse_than_cbz: false,
            ..DuplicateRules::default()
        };
        let books = vec![
            book("Conflict", "1", "/c/conflict.cbz", 500, 30),
            book("Conflict", "1", "/c/conflict.cbr", 900, 30),
        ];
        assert_eq!(worst(&books, &rules), ["/c/conflict.cbz"]);
    }

    #[test]
    fn all_rules_off_marks_nothing() {
        let books = vec![
            book("Alpha", "1", "/c/alpha.cbz", 2000, 20),
            book("Alpha", "1", "/c/alpha.cbr", 1000, 10),
        ];
        let rules = DuplicateRules {
            cbr_worse_than_cbz: false,
            smaller_file_worse: false,
            fewer_pages_worse: false,
            older_file_worse: false,
            incoming_path: String::new(),
        };
        assert!(worst(&books, &rules).is_empty());
    }

    #[test]
    fn a_fileless_copy_loses_every_enabled_rule() {
        let mut fileless = book("Fileless", "1", "", -1, 0);
        fileless.file_path = String::new();
        let books = vec![fileless, book("Fileless", "1", "/c/kept.cbz", 1000, 15)];
        assert_eq!(worst(&books, &DuplicateRules::default()).len(), 1);
        // The marked one is the fileless entry (empty path).
        let refs: Vec<&ComicBook> = books.iter().collect();
        let ids = worst_duplicate_ids(&refs, &DuplicateRules::default());
        assert_eq!(ids[0], books[0].id);
    }

    #[test]
    fn identical_path_records_rank_by_metadata() {
        // Two records of the SAME file are path duplicates. With
        // equal metadata they tie and mark nothing; a page-count
        // difference marks the poorer record.
        let a = book("Path", "1", "/c/same.cbz", 1000, 20);
        let mut b = book("Path", "1", "/c/same.cbz", 1000, 20);
        b.info.title = "different record".into();
        assert!(worst(&[a.clone(), b.clone()], &DuplicateRules::default()).is_empty());
        b.info.page_count = 0;
        assert_eq!(
            worst_duplicate_ids(&[&a, &b], &DuplicateRules::default()),
            [b.id]
        );
    }

    #[test]
    fn other_formats_take_no_side() {
        // A PDF loses nothing under the format rule (only CBR vs CBZ
        // compares), but the smaller-file rule still applies.
        let books = vec![
            book("Other", "1", "/c/other.pdf", 500, 20),
            book("Other", "1", "/c/other.cbz", 900, 20),
        ];
        assert_eq!(worst(&books, &DuplicateRules::default()), ["/c/other.pdf"]);
    }

    #[test]
    fn non_duplicates_never_rank() {
        let books = vec![
            book("Solo", "1", "/c/solo.cbz", 100, 5),
            book("Other", "1", "/c/other.cbz", 2000, 20),
        ];
        assert!(worst(&books, &DuplicateRules::default()).is_empty());
    }

    #[test]
    fn rules_read_from_settings() {
        let mut s = Settings::default();
        assert_eq!(DuplicateRules::from_settings(&s), DuplicateRules::default());
        s.duplicates_cbr_worse_than_cbz = false;
        s.duplicates_smaller_file_worse = false;
        s.duplicates_fewer_pages_worse = false;
        s.duplicates_older_file_worse = false;
        s.duplicates_incoming_path = "/x".into();
        assert_eq!(
            DuplicateRules::from_settings(&s),
            DuplicateRules {
                cbr_worse_than_cbz: false,
                smaller_file_worse: false,
                fewer_pages_worse: false,
                older_file_worse: false,
                incoming_path: "/x".into(),
            }
        );
    }

    fn incoming_rules(path: &str) -> DuplicateRules {
        DuplicateRules {
            incoming_path: path.into(),
            ..DuplicateRules::default()
        }
    }

    #[test]
    fn the_incoming_copy_loses_even_when_it_is_the_better_file() {
        // ADR-046: the Library copy wins over cbr, larger, and more
        // pages — the heavy path-rule penalty outweighs the quality
        // rules. Without the path the rules keep the CBZ (the same
        // case the ADR-044 tests cover).
        let books = vec![
            book("Example", "1", "/data/library/example.cbr", 500, 20),
            book("Example", "1", "/data/incoming/example.cbz", 2000, 30),
        ];
        assert_eq!(
            worst(&books, &incoming_rules("/data/incoming")),
            ["/data/incoming/example.cbz"]
        );
        // Without the path the quality rules keep the CBZ — the
        // Library CBR is the copy the command marks (the behavior
        // the rule exists to fix).
        assert_eq!(
            worst(&books, &DuplicateRules::default()),
            ["/data/library/example.cbr"]
        );
    }

    #[test]
    fn an_empty_comic_beats_the_incoming_path_rule() {
        // ADR-046: the fileless record loses every enabled rule —
        // the quality rules AND the path rule at its full weight —
        // so an empty Library record marks even against a real file
        // under the incoming path (the fileless book counts as the
        // outside copy that arms the rule).
        let mut empty = book("Empty", "1", "", -1, 0);
        empty.file_path = String::new();
        let real = book("Empty", "1", "/data/incoming/empty.cbr", 1000, 20);
        let refs: Vec<&ComicBook> = vec![&empty, &real];
        let ids = worst_duplicate_ids(&refs, &incoming_rules("/data/incoming"));
        assert_eq!(ids, vec![empty.id]);
    }

    #[test]
    fn a_former_tie_resolves_to_the_incoming_copy() {
        // The ADR-044 conflict pair (each copy loses one rule) ties
        // and marks nothing without the path; with it the Incoming
        // copy loses the extra weight and marks.
        let books = vec![
            book("Conflict", "1", "/data/library/conflict.cbz", 500, 30),
            book("Conflict", "1", "/data/incoming/conflict.cbr", 900, 30),
        ];
        assert!(worst(&books, &DuplicateRules::default()).is_empty());
        assert_eq!(
            worst(&books, &incoming_rules("/data/incoming")),
            ["/data/incoming/conflict.cbr"]
        );
    }

    #[test]
    fn a_group_on_one_side_of_the_path_ranks_normally() {
        // Both copies under the path: the rule takes no side (no
        // outside copy arms it). Both outside: unchanged. The CBR
        // copy is the loser in both pairs.
        let both_under = vec![
            book("Under", "1", "/data/incoming/under-a.cbz", 2000, 20),
            book("Under", "1", "/data/incoming/under-b.cbr", 1000, 10),
        ];
        assert_eq!(
            worst(&both_under, &incoming_rules("/data/incoming")),
            ["/data/incoming/under-b.cbr"]
        );
        let both_outside = vec![
            book("Outside", "1", "/data/library/outside-a.cbr", 1000, 10),
            book("Outside", "1", "/data/library/outside-b.cbz", 2000, 20),
        ];
        assert_eq!(
            worst(&both_outside, &incoming_rules("/data/incoming")),
            ["/data/library/outside-a.cbr"]
        );
    }

    #[test]
    fn every_incoming_copy_marks_in_a_mixed_group() {
        // One Library copy, two Incoming copies: both Incoming
        // copies sit above the group minimum.
        let books = vec![
            book("Mixed", "1", "/data/library/mixed.cbz", 2000, 20),
            book("Mixed", "1", "/data/incoming/mixed-a.cbz", 2000, 20),
            book("Mixed", "1", "/data/incoming/mixed-b.cbz", 2000, 20),
        ];
        assert_eq!(
            worst(&books, &incoming_rules("/data/incoming")),
            ["/data/incoming/mixed-a.cbz", "/data/incoming/mixed-b.cbz",]
        );
    }

    #[test]
    fn the_path_match_tolerates_case_and_separators() {
        // A trailing separator and letter case on the configured path
        // do not matter; the same holds for a Windows-styled root
        // against a forward-slashed book path.
        let books = vec![
            book("Case", "1", "/data/library/case.cbz", 2000, 20),
            book("Case", "1", "/data/incoming/case.cbr", 1000, 10),
        ];
        assert_eq!(
            worst(&books, &incoming_rules("/data/Incoming/")),
            ["/data/incoming/case.cbr"]
        );
        let win = vec![
            book("Win", "1", "C:/Comics/Library/win.cbz", 2000, 20),
            book("Win", "1", "c:/comics/incoming/win.cbr", 1000, 10),
        ];
        assert_eq!(
            worst(&win, &incoming_rules("C:\\Comics\\Incoming")),
            ["c:/comics/incoming/win.cbr"]
        );
    }
}
