//! The Library-tree gauges (`LibraryTreeSkin.DrawNodeLabel` /
//! `DrawMarkers`): the per-list Total / Unread / New book counts the
//! original draws as small colored number badges after each node name.
//!
//! C# reference: `ComicRack/Controls/LibraryTreeSkin.cs:43-96` (what
//! is drawn, the merge and zero-hiding rules) and
//! `ComicRack/Engine/Database/ComicListItem.cs:695-717`
//! (`CreateBookCacheStatus` — the classification). The counts are the
//! C# `BookCount` / `NewBookCount` / `UnreadBookCount` fields the
//! port already round-trips in `ListItemBase`.
//!
//! The classification is per book over the LIST's book set, with
//! three INDEPENDENT counts:
//! - Total: every book.
//! - Unread: `!HasBeenRead` (`ReadPercentage < 95`), any age.
//! - New: added within the recent window
//!   (`(now - AddedTime).TotalDays < IsRecentInDays`, default 14),
//!   any read state.
//!
//! A book can count in more than one gauge (a recently added, not yet
//! read book counts in both Unread and New). `now` is snapshotted by
//! the caller per pass. An `AddedTime` of MinValue is ancient and is
//! not New.

use std::collections::HashSet;

use cr_core::database::comic_database::ComicDatabase;
use cr_core::database::list_items::ComicListItem;
use cr_core::model::comic_book::ComicBook;
use cr_core::model::enums::ComicFolderCombineMode;
use cr_core::xml::scalar::{CrDateTime, CrGuid};

use crate::lists::evaluate_list;
use crate::matcher::book_view;

/// The per-list counters. Total / Unread / New are independent: a
/// book can count in more than one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Gauges {
    pub total: i32,
    pub new: i32,
    pub unread: i32,
}

/// True when the book is not yet read (`ReadPercentage < 95`).
pub fn is_unread(book: &ComicBook) -> bool {
    !book_view::has_been_read(book)
}

/// True when the book was added within the recent window
/// (`(now - AddedTime).TotalDays < recent_days`). A fractional day
/// comparison, so a book added 13.5 days ago with a 14-day window is
/// New. A future AddedTime gives a negative span and is New.
pub fn is_new(book: &ComicBook, now: &CrDateTime, recent_days: i32) -> bool {
    let days = (now.naive - book.added_time.naive).num_seconds() as f64 / 86400.0;
    days < f64::from(recent_days)
}

/// Counts the three independent gauges over one list's book set.
pub fn gauge_counts<'a>(
    books: impl IntoIterator<Item = &'a ComicBook>,
    now: &CrDateTime,
    recent_days: i32,
) -> Gauges {
    let mut g = Gauges::default();
    for book in books {
        g.total += 1;
        if is_unread(book) {
            g.unread += 1;
        }
        if is_new(book, now, recent_days) {
            g.new += 1;
        }
    }
    g
}

/// The book-id set of one tree node — the same evaluation the browser
/// fills from (`lists::evaluate_list`), collected by id. Smart-list
/// limits, base lists, and stale reading-list ids behave exactly as
/// the browser sees them.
pub fn list_book_ids(item: &ComicListItem, db: &ComicDatabase) -> HashSet<CrGuid> {
    evaluate_list(item, db).into_iter().map(|b| b.id).collect()
}

/// `ComicListItemFolder.OnCacheMatch`: a folder's set combined from
/// its CHILDREN's sets (the incremental shape — the caller supplies
/// the already-computed child sets in child order). Or unions, And
/// intersects (an empty child empties the folder), Empty is empty.
pub fn combine_folder_sets(
    combine_mode: ComicFolderCombineMode,
    child_sets: &[HashSet<CrGuid>],
) -> HashSet<CrGuid> {
    match combine_mode {
        ComicFolderCombineMode::Or => {
            let mut out: HashSet<CrGuid> = HashSet::new();
            for child in child_sets {
                out.extend(child.iter().copied());
            }
            out
        }
        ComicFolderCombineMode::And => {
            let mut iter = child_sets.iter();
            let Some(first) = iter.next() else {
                return HashSet::new();
            };
            let mut out: HashSet<CrGuid> = HashSet::new();
            for id in first {
                if iter.clone().all(|rest| rest.contains(id)) {
                    out.insert(*id);
                }
            }
            out
        }
        ComicFolderCombineMode::Empty => HashSet::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cr_core::database::comic_database::create_new;
    use cr_core::database::list_items::{IdListItem, ListItemBase};

    const DAY_SECS: f64 = 86400.0;

    /// Builds a book relative to a FIXED base instant, so a test's
    /// `now` and the book's AddedTime cannot skew apart.
    fn book_at(
        base: CrDateTime,
        read_pct_pages: Option<(i32, i32)>,
        added_days_ago: f64,
    ) -> ComicBook {
        let mut b = ComicBook {
            id: CrGuid::new_random(),
            info: cr_core::model::comic_info::ComicInfo {
                page_count: 10,
                ..Default::default()
            },
            ..Default::default()
        };
        if let Some((last, count)) = read_pct_pages {
            b.last_page_read = last;
            b.info.page_count = count;
        }
        b.added_time = CrDateTime {
            naive: base.naive
                - chrono::Duration::milliseconds((added_days_ago * DAY_SECS * 1000.0) as i64),
            kind: cr_core::xml::scalar::DateKind::Unspecified,
        };
        b
    }

    fn book(read_pct_pages: Option<(i32, i32)>, added_days_ago: f64) -> ComicBook {
        book_at(CrDateTime::now(), read_pct_pages, added_days_ago)
    }

    #[test]
    fn classification_matches_the_independent_gauges() {
        let now = CrDateTime::now();
        // ReadPercentage (19+1)*100/20 = 100 → read (not Unread), any age.
        let read = book_at(now, Some((19, 20)), 1.0);
        assert!(!is_unread(&read));
        assert!(is_new(&read, &now, 14)); // added 1 day ago → still New
                                          // ReadPercentage 40 → not read → Unread; added 1 day → New too.
        let fresh_unread = book_at(now, Some((3, 10)), 1.0);
        assert!(is_unread(&fresh_unread));
        assert!(is_new(&fresh_unread, &now, 14));
        // Unread, added 30 days ago → Unread, not New.
        let old_unread = book_at(now, Some((3, 10)), 30.0);
        assert!(is_unread(&old_unread));
        assert!(!is_new(&old_unread, &now, 14));
        // The boundary: EXACTLY 14 days is not < 14 → not New.
        let edge = book_at(now, Some((0, 10)), 14.0);
        assert!(!is_new(&edge, &now, 14));
        // Fractional: 13.5 days is still New.
        let frac = book_at(now, Some((0, 10)), 13.5);
        assert!(is_new(&frac, &now, 14));
        // MinValue AddedTime is ancient → not New.
        let mut ancient = book_at(now, Some((0, 10)), 0.0);
        ancient.added_time = CrDateTime::min_value();
        assert!(!is_new(&ancient, &now, 14));
        // A future AddedTime gives a negative span → New.
        let mut future = book_at(now, Some((0, 10)), 0.0);
        future.added_time = CrDateTime {
            naive: now.naive + chrono::Duration::days(2),
            kind: cr_core::xml::scalar::DateKind::Unspecified,
        };
        assert!(is_new(&future, &now, 14));
    }

    #[test]
    fn gauge_counts_totals_the_three_independent_gauges() {
        let now = CrDateTime::now();
        let books = vec![
            book_at(now, Some((19, 20)), 1.0), // read, new
            book_at(now, Some((0, 10)), 1.0),  // unread, new
            book_at(now, Some((0, 10)), 1.0),  // unread, new
            book_at(now, Some((0, 10)), 30.0), // unread, not new
        ];
        // Total 4; Unread = 3 (all but the read one); New = 3 (all but
        // the 30-day-old one). The read+new book counts in New but not
        // Unread; the old unread book counts in Unread but not New.
        assert_eq!(
            gauge_counts(&books, &now, 14),
            Gauges {
                total: 4,
                new: 3,
                unread: 3
            }
        );
    }

    #[test]
    fn list_book_ids_collects_the_evaluated_set() {
        let mut db = create_new();
        let a = book(Some((0, 10)), 1.0);
        let b = book(Some((0, 10)), 1.0);
        let a_id = a.id;
        let b_id = b.id;
        db.books = vec![a, b];
        let list = ComicListItem::IdList(IdListItem {
            base: ListItemBase {
                id: CrGuid::new_random(),
                name: Some("L".into()),
                ..Default::default()
            },
            book_ids: vec![a_id, b_id, a_id],
        });
        let ids = list_book_ids(&list, &db);
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&a_id) && ids.contains(&b_id));
    }

    #[test]
    fn folder_combine_matches_the_c_sharp_modes() {
        use std::collections::HashSet;
        let s1: HashSet<CrGuid> = ["a", "b"]
            .iter()
            .map(|s| CrGuid::parse(&format!("00000000-0000-0000-0000-00000000000{s}")).unwrap())
            .collect();
        let s2: HashSet<CrGuid> = ["b", "c"]
            .iter()
            .map(|s| CrGuid::parse(&format!("00000000-0000-0000-0000-00000000000{s}")).unwrap())
            .collect();
        // Or: union.
        let or = combine_folder_sets(ComicFolderCombineMode::Or, &[s1.clone(), s2.clone()]);
        assert_eq!(or.len(), 3);
        // And: intersection.
        let and = combine_folder_sets(ComicFolderCombineMode::And, &[s1.clone(), s2.clone()]);
        assert_eq!(and.len(), 1);
        // And with an empty child: empty (the C# early-out).
        let and_empty =
            combine_folder_sets(ComicFolderCombineMode::And, &[s1.clone(), HashSet::new()]);
        assert!(and_empty.is_empty());
        // Empty mode: empty regardless of children.
        assert!(combine_folder_sets(ComicFolderCombineMode::Empty, &[s1]).is_empty());
        // No children at all: empty for every mode.
        assert!(combine_folder_sets(ComicFolderCombineMode::Or, &[]).is_empty());
    }
}
