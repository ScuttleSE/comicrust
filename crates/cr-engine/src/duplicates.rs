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
//! rule — a tie at the group minimum marks nothing).

use cr_core::model::comic_book::ComicBook;
use cr_core::settings::Settings;
use cr_core::xml::scalar::CrGuid;
use cr_io::formats::{self, ids};

use crate::matcher::book_view;
use crate::matcher::eval::duplicate_groups;

/// The duplicate-cleanup rules (the `Settings.Duplicates*` fields —
/// the Preferences duplicates page rows).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
}

impl Default for DuplicateRules {
    fn default() -> Self {
        DuplicateRules {
            cbr_worse_than_cbz: true,
            smaller_file_worse: true,
            fewer_pages_worse: true,
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
        }
    }

    /// The count of the enabled rules — a fileless copy loses every
    /// one of them (ADR-044: no file, so no format, no size, no page
    /// data — it ranks worst).
    fn enabled_count(&self) -> i32 {
        self.cbr_worse_than_cbz as i32
            + self.smaller_file_worse as i32
            + self.fewer_pages_worse as i32
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
/// loses every enabled rule.
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

fn max_size(members: &[&ComicBook]) -> i64 {
    members.iter().map(|b| b.file_size).max().unwrap_or(-1)
}

fn max_pages(members: &[&ComicBook]) -> i32 {
    members.iter().map(|b| b.info.page_count).max().unwrap_or(0)
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
        assert_eq!(
            DuplicateRules::from_settings(&s),
            DuplicateRules {
                cbr_worse_than_cbz: false,
                smaller_file_worse: false,
                fewer_pages_worse: false,
            }
        );
    }
}
