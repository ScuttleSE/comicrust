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
//! ADR-048 (a measured design change): real libraries fill with
//! symmetric-loss pairs — a re-encode copy that is newer AND smaller
//! against an original that is older AND larger — where every copy
//! loses exactly one rule and the old rule marked nothing, so the
//! command selected nothing at all over 989 groups of a measured
//! library. An exact tie now breaks deterministically: the smaller
//! file ranks worst, then the fewer pages, then the older stamp;
//! copies identical on all three mark nothing (no basis to prefer
//! one). This holds whatever the rule switches say — with every rule
//! off, a differing pair still resolves to the smaller copy (the
//! recorded all-rules-off expectation moved with ADR-048).
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
}

impl Default for DuplicateRules {
    fn default() -> Self {
        DuplicateRules {
            cbr_worse_than_cbz: true,
            smaller_file_worse: true,
            fewer_pages_worse: true,
            older_file_worse: true,
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
}

/// The ids of the worst copies per duplicate group over `books`, in
/// the first-appearance order of their groups. Groups of one copy are
/// not duplicates and never rank; a group whose copies tie at the
/// lowest penalty breaks the tie deterministically (ADR-048).
pub fn worst_duplicate_ids(books: &[&ComicBook], rules: &DuplicateRules) -> Vec<CrGuid> {
    let mut out = Vec::new();
    for group in duplicate_groups(books) {
        let members: Vec<&ComicBook> = group.iter().map(|&i| books[i]).collect();
        let penalties: Vec<i32> = members
            .iter()
            .map(|b| penalty(b, &members, rules))
            .collect();
        for i in selected_indices(&members, &penalties) {
            out.push(members[i].id);
        }
    }
    out
}

/// The selected (worst) member indexes of one group: every copy whose
/// penalty EXCEEDS the group minimum, and — when every copy ties at
/// the minimum — the ADR-048 deterministic tie-break. The tie-break
/// ranks the tied copies worst-first by the same scale the rules use:
/// the smaller file, then the fewer pages, then the older file stamp;
/// copies identical on all three mark nothing (there is no basis to
/// prefer one).
fn selected_indices(members: &[&ComicBook], penalties: &[i32]) -> Vec<usize> {
    let best = penalties.iter().copied().min().unwrap_or(0);
    let losers: Vec<usize> = (0..members.len())
        .filter(|&i| penalties[i] > best)
        .collect();
    if !losers.is_empty() || members.len() < 2 {
        return losers;
    }
    let mut tied: Vec<usize> = (0..members.len()).collect();
    for key in [
        |b: &ComicBook| b.file_size,
        |b: &ComicBook| b.info.page_count as i64,
        |b: &ComicBook| modified_secs(b),
    ] {
        let worst_value = tied
            .iter()
            .map(|&i| key(members[i]))
            .min()
            .unwrap_or_default();
        tied.retain(|&i| key(members[i]) == worst_value);
        if tied.len() == 1 {
            return tied;
        }
    }
    Vec::new()
}

/// One member of one duplicate group in the diagnostic report: the
/// loaded ranking inputs, the rule penalty, and whether the command
/// selects the copy.
pub struct MemberReport {
    pub id: CrGuid,
    pub file_path: String,
    pub file_size: i64,
    pub page_count: i32,
    /// The file stamp the older-file rule compares (naive UTC
    /// seconds; the `DateTime.MinValue` default is 0 or negative).
    pub modified_secs: i64,
    pub penalty: i32,
    pub worst: bool,
}

/// One duplicate group (more than one member), in first-appearance
/// order.
pub struct GroupReport {
    pub members: Vec<MemberReport>,
}

/// The full diagnostic report over `books`: every duplicate group
/// with each member's ranking inputs and penalties, and the copies
/// the command would select. Same grouping and rules as
/// [`worst_duplicate_ids`]; read-only. The `cr-cli duplicates`
/// subcommand prints it (read-only diagnostic, ADR-044/046).
pub fn duplicate_report(books: &[&ComicBook], rules: &DuplicateRules) -> Vec<GroupReport> {
    let mut out = Vec::new();
    for group in duplicate_groups(books) {
        let members: Vec<&ComicBook> = group.iter().map(|&i| books[i]).collect();
        let penalties: Vec<i32> = members
            .iter()
            .map(|b| penalty(b, &members, rules))
            .collect();
        let selected = selected_indices(&members, &penalties);
        let members = members
            .iter()
            .enumerate()
            .map(|(i, b)| MemberReport {
                id: b.id,
                file_path: b.file_path.clone(),
                file_size: b.file_size,
                page_count: b.info.page_count,
                modified_secs: modified_secs(b),
                penalty: penalties[i],
                worst: selected.contains(&i),
            })
            .collect();
        out.push(GroupReport { members });
    }
    out
}

/// The rule penalties of one copy against its group. A fileless copy
/// loses every enabled rule (ADR-044: an empty record ranks worst
/// wherever it sits).
fn penalty(book: &ComicBook, members: &[&ComicBook], rules: &DuplicateRules) -> i32 {
    if !book_view::is_linked(book) {
        return rules.enabled_count();
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
    p
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

/// Is `path` inside the `root` directory? Component-wise prefix
/// compare, case-insensitive; empty components (a trailing
/// separator, a doubled one) are dropped.
pub(crate) fn under_path(path: &str, root: &str) -> bool {
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
        // the only loser.
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
        // With the stamp rule off the pair ties under the rules and
        // the ADR-048 tie-break resolves it by the stamp scale: the
        // older copy still ranks worst.
        let rules = DuplicateRules {
            older_file_worse: false,
            ..DuplicateRules::default()
        };

        assert_eq!(worst(&[old, new], &rules), ["/c/tie-a.cbz"]);
    }

    #[test]
    fn a_conflicting_tie_breaks_to_the_smaller_file() {
        // The CBR is larger; the CBZ is not better by format alone —
        // both lose one rule and the tie at the group minimum breaks
        // by size (ADR-048): the smaller CBZ ranks worst.
        let books = vec![
            book("Conflict", "1", "/c/conflict.cbz", 500, 30),
            book("Conflict", "1", "/c/conflict.cbr", 900, 30),
        ];
        assert_eq!(
            worst(&books, &DuplicateRules::default()),
            ["/c/conflict.cbz"]
        );
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
    fn all_rules_off_breaks_ties_by_size() {
        // With every rule off all penalties tie at zero and the
        // ADR-048 tie-break resolves the pair by size: the smaller
        // copy ranks worst.
        let books = vec![
            book("Alpha", "1", "/c/alpha.cbz", 2000, 20),
            book("Alpha", "1", "/c/alpha.cbr", 1000, 10),
        ];
        let rules = DuplicateRules {
            cbr_worse_than_cbz: false,
            smaller_file_worse: false,
            fewer_pages_worse: false,
            older_file_worse: false,
        };
        assert_eq!(worst(&books, &rules), ["/c/alpha.cbr"]);
    }

    #[test]
    fn identical_copies_still_mark_nothing() {
        // The tie-break scale falls through size, pages, and stamp —
        // copies identical on all three have no basis to prefer one
        // and mark nothing (the same-file-twice case included).
        let a = book("Same", "1", "/c/same-a.cbz", 1000, 20);
        let b = book("Same", "1", "/c/same-b.cbz", 1000, 20);
        let rules = DuplicateRules {
            older_file_worse: false,
            ..DuplicateRules::default()
        };
        assert!(worst(&[a, b], &rules).is_empty());
    }

    #[test]
    fn the_tie_break_prefers_smaller_then_pages_then_stamp() {
        // With every rule off all penalties tie at zero and the
        // tie-break scale decides, one key at a time: the smallest
        // file; when sizes tie, the fewest pages; when pages tie, the
        // older stamp.
        let all_off = DuplicateRules {
            cbr_worse_than_cbz: false,
            smaller_file_worse: false,
            fewer_pages_worse: false,
            older_file_worse: false,
        };
        let sizes = vec![
            book("Scale", "1", "/c/scale-a.cbz", 2000, 20),
            book("Scale", "1", "/c/scale-b.cbz", 3000, 20),
            book("Scale", "1", "/c/scale-c.cbz", 4000, 20),
        ];
        assert_eq!(worst(&sizes, &all_off), ["/c/scale-a.cbz"]);

        let pages = vec![
            book("Pages", "1", "/c/pages-a.cbz", 1000, 30),
            book("Pages", "1", "/c/pages-b.cbz", 1000, 20),
        ];
        assert_eq!(worst(&pages, &all_off), ["/c/pages-b.cbz"]);

        let mut old = book("Stamp", "1", "/c/stamp-a.cbz", 1000, 20);
        let mut new = book("Stamp", "1", "/c/stamp-b.cbz", 1000, 20);
        old.file_modified_time =
            cr_core::xml::scalar::CrDateTime::parse("2019-01-01T00:00:00").unwrap();
        new.file_modified_time =
            cr_core::xml::scalar::CrDateTime::parse("2024-01-01T00:00:00").unwrap();
        assert_eq!(worst(&[old, new], &all_off), ["/c/stamp-a.cbz"]);
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
        assert_eq!(
            DuplicateRules::from_settings(&s),
            DuplicateRules {
                cbr_worse_than_cbz: false,
                smaller_file_worse: false,
                fewer_pages_worse: false,
                older_file_worse: false,
            }
        );
    }
}
