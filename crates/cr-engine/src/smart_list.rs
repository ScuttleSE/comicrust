//! Smart-list evaluation (`ComicSmartListItem`): filter the library
//! through the list's matcher set, then apply the `Limit` selection and
//! the `FilteredIds` exclusion.

use cr_core::database::list_items::{ComicBookMatcher, SmartListItem};
use cr_core::model::comic_book::ComicBook;
use cr_core::model::enums::{
    ComicSmartListLimitSelectionType, ComicSmartListLimitType, MatcherMode,
};

use crate::matcher::eval::{match_set, MatchContext};
use crate::matcher::tree::Matcher;
use crate::sort::{compare_series, randomize};

/// Binds one raw XML matcher tree node. Unknown class names return
/// `None` (the C# would not load such a file at all).
pub fn bind_matcher(raw: &ComicBookMatcher) -> Option<Matcher> {
    Matcher::from_raw(raw)
}

/// Evaluates a smart list against the full library.
///
/// `base_list` is the pre-computed book set of the list's `BaseListId`
/// (the C# resolves it through the library's list tree); `None` means
/// "all books". Returns the books in the C# pipeline order.
pub fn evaluate_smart_list<'a>(
    list: &SmartListItem,
    library: &'a [&'a ComicBook],
    base_list: Option<&'a [&'a ComicBook]>,
) -> Vec<&'a ComicBook> {
    let mut items: Vec<&ComicBook> = match base_list {
        Some(base) => base.to_vec(),
        None => library.to_vec(),
    };
    // `NotInBaseList`: Library.Books.Except(baseList).
    if list.not_in_base_list && base_list.is_some() {
        let base_ids: std::collections::HashSet<_> =
            items.iter().map(|b| (*b) as *const ComicBook).collect();
        items.retain(|b| !base_ids.contains(&(*b as *const ComicBook)));
    }

    // Bind the matchers; skip unknown ones (the C# cannot load them).
    let mut pairs: Vec<(MatcherMode, bool, &Matcher)> = Vec::new();
    let bound: Vec<Matcher> = list.matchers.iter().filter_map(bind_matcher).collect();
    for matcher in &bound {
        pairs.push((list.matcher_mode, matcher.not(), matcher));
    }
    let ctx = MatchContext::new(library);
    let mut result: Vec<&ComicBook> = if bound.is_empty() {
        items.clone()
    } else {
        match_set(&items, &pairs, &ctx)
    };

    // Limit: reorder by the selection type, then cap.
    if list.limit {
        match list.limit_selection_type {
            ComicSmartListLimitSelectionType::SortedBySeries => {
                let mut sorted: Vec<&ComicBook> = result;
                sorted.sort_by(|a, b| compare_series(a, b));
                result = sorted;
            }
            ComicSmartListLimitSelectionType::Random => {
                // The C# fills the seed on first use; the persisted seed
                // decides the shuffle.
                let seed = if list.limit_random_seed == 0 {
                    rand_seed()
                } else {
                    list.limit_random_seed
                };
                let mut shuffled: Vec<&ComicBook> = result;
                shuffled.sort_by(|a, b| crate::sort::guid_compare(&a.id, &b.id));
                randomize(&mut shuffled, seed);
                result = shuffled;
            }
            ComicSmartListLimitSelectionType::Position => {}
        }
        match list.limit_type {
            ComicSmartListLimitType::MB => {
                result = limit_by_size(result, list.limit_value as i64 * 1024 * 1024);
            }
            ComicSmartListLimitType::GB => {
                result = limit_by_size(result, list.limit_value as i64 * 1024 * 1024 * 1024);
            }
            _ => {
                result.truncate(list.limit_value.max(0) as usize);
            }
        }
    }

    // FilteredIds: excluded unless ShowFiltered is set.
    if !list.show_filtered && !list.filtered_ids.is_empty() {
        let filtered: std::collections::HashSet<_> = list.filtered_ids.iter().copied().collect();
        result.retain(|b| !filtered.contains(&b.id));
    }
    result
}

/// `LimitBySize`: the C# `TakeWhile(cb => (size += cb.FileSize) < max)`
/// updates the accumulator inside the predicate, so the item that
/// crosses the cap is excluded and the sequence stops there.
fn limit_by_size(books: Vec<&ComicBook>, max_size: i64) -> Vec<&ComicBook> {
    let mut size = 0i64;
    let mut out = Vec::new();
    for b in books {
        size += b.file_size;
        if size >= max_size {
            break;
        }
        out.push(b);
    }
    out
}

fn rand_seed() -> i32 {
    // `new Random().Next()` — unseeded; any entropy source works
    // because the C# stores the value on first use.
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as i32)
        .unwrap_or(0);
    nanos & i32::MAX
}

#[cfg(test)]
mod tests {
    use super::*;
    use cr_core::database::list_items::{ComicBookMatcher, ValueMatcher};

    fn raw_matcher(type_name: &str, op: i32, value: &str) -> ComicBookMatcher {
        ComicBookMatcher::Value(ValueMatcher {
            type_name: type_name.into(),
            match_operator: op,
            match_value: value.into(),
            ..Default::default()
        })
    }

    fn smart_list(matchers: Vec<ComicBookMatcher>) -> SmartListItem {
        SmartListItem {
            matchers,
            ..Default::default()
        }
    }

    #[test]
    fn evaluates_saved_matcher_tree() {
        let mut a = ComicBook::default();
        a.info.series = "A".into();
        a.rating = 4.0;
        a.enable_proposed = false;
        let mut b = ComicBook::default();
        b.info.series = "B".into();
        b.rating = 1.0;
        b.enable_proposed = false;
        let library = vec![&a, &b];

        // The real-world DB's "My Favorites": [My Rating] is greater "3".
        let list = smart_list(vec![raw_matcher("ComicBookRatingMatcher", 1, "3")]);
        let result = evaluate_smart_list(&list, &library, None);
        assert_eq!(result.len(), 1);
        assert!(std::ptr::eq(result[0], library[0]));
    }

    #[test]
    fn limit_count_and_size() {
        let mut books = Vec::new();
        for i in 0..5 {
            let mut b = ComicBook::default();
            b.info.series = format!("S{i}");
            b.enable_proposed = false;
            books.push(b);
        }
        let refs: Vec<&ComicBook> = books.iter().collect();
        let mut list = smart_list(Vec::new());
        list.limit = true;
        list.limit_value = 3;
        list.limit_selection_type = ComicSmartListLimitSelectionType::Position;
        let result = evaluate_smart_list(&list, &refs, None);
        assert_eq!(result.len(), 3);

        list.limit_type = ComicSmartListLimitType::MB;
        list.limit_value = 1; // 1 MB
                              // 512 KB per book: the second accumulates to exactly 1 MB,
                              // which is not below the cap — only one book is taken.
        for b in &mut books {
            b.file_size = 512 * 1024;
        }
        let refs: Vec<&ComicBook> = books.iter().collect();
        let result = evaluate_smart_list(&list, &refs, None);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn filtered_ids_excluded() {
        let mut a = ComicBook::default();
        a.info.series = "A".into();
        a.enable_proposed = false;
        let mut b = ComicBook {
            id: cr_core::xml::scalar::CrGuid::parse("11111111-2222-3333-4444-555555555555")
                .unwrap(),
            ..Default::default()
        };
        b.info.series = "B".into();
        b.enable_proposed = false;
        let library = vec![&a, &b];

        let mut list = smart_list(Vec::new());
        list.filtered_ids = vec![b.id];
        let result = evaluate_smart_list(&list, &library, None);
        assert_eq!(result.len(), 1);

        list.show_filtered = true;
        let result = evaluate_smart_list(&list, &library, None);
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn unknown_matcher_is_skipped() {
        let list = smart_list(vec![raw_matcher("ComicBookNoSuchMatcher", 0, "")]);
        let a = ComicBook {
            enable_proposed: false,
            ..Default::default()
        };
        let library = vec![&a];
        let result = evaluate_smart_list(&list, &library, None);
        assert_eq!(result.len(), 1);
    }
}
