//! The item view state — the book set with sort, grouping, and the
//! selection model (`ItemView` state machine, pure and unit-tested;
//! the GTK widget in [`super::item_view`] drives it).
//!
//! C# reference: `ItemView.cs` — the MRU sorter chain (up to 3,
//! `ChainedComparer`, `Descending` = `comparer.Reverse()`), the group
//! buckets (`GroupManager` → order by `GroupInfo.Compare`: bucket
//! index, tie by `ExtendedStringComparer` IgnoreArticles|IgnoreCase),
//! and the selection state bits (`ItemViewStates`).

use std::collections::HashSet;

use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::CrGuid;
use cr_engine::group::{compare_by_column, groupers, GroupInfo, Grouper, UNSPECIFIED};
use cr_engine::matcher::eval::{match_set, MatchContext};
use cr_engine::matcher::tree::Matcher;
use cr_io::extended_compare::extended_compare_ignore_articles_case;

/// One sort key: a column key plus direction (`ItemViewColumn` +
/// `ItemSortOrder`). The column keys are the C# property names.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SortKey {
    pub column: String,
    pub descending: bool,
}

impl SortKey {
    pub fn new(column: &str, descending: bool) -> SortKey {
        SortKey {
            column: column.to_string(),
            descending,
        }
    }
}

/// The chained sort state — an MRU chain of up to 3 keys (the C#
/// `itemSorters` list `Trim(3)`; the first key sorts, later keys
/// break ties). `Descending` inverts the whole comparison
/// (`comparer.Reverse()`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SortChain {
    keys: Vec<SortKey>,
}

impl SortChain {
    pub const MAX: usize = 3;

    pub fn keys(&self) -> &[SortKey] {
        &self.keys
    }

    /// Header click: same column flips its direction, a new column
    /// moves to the front (`OnHeaderClick` + the MRU chain).
    pub fn toggle_or_push(&mut self, column: &str) {
        if let Some(pos) = self.keys.iter().position(|k| k.column == column) {
            let mut key = self.keys.remove(pos);
            key.descending = !key.descending;
            self.keys.insert(0, key);
        } else {
            self.keys.insert(0, SortKey::new(column, false));
        }
        self.keys.truncate(Self::MAX);
    }

    pub fn set_direction(&mut self, descending: bool) {
        if let Some(first) = self.keys.first_mut() {
            first.descending = descending;
        }
    }

    /// Flips the first key's direction (the Reverse Direction
    /// command).
    pub fn toggle_direction(&mut self) {
        if let Some(first) = self.keys.first_mut() {
            first.descending = !first.descending;
        }
    }

    fn compare(&self, a: &ComicBook, b: &ComicBook) -> std::cmp::Ordering {
        if self.keys.is_empty() {
            // No sort: the input order IS the display order (the C#
            // shows the enumeration order — a reading list's stored
            // order). A Guid fallback here SHUFFLED imported lists.
            return std::cmp::Ordering::Equal;
        }
        for key in &self.keys {
            let mut ord = compare_by_column(a, b, &key.column);
            if ord != std::cmp::Ordering::Equal {
                if key.descending {
                    ord = ord.reverse();
                }
                return ord;
            }
        }
        // Full tie under an ACTIVE sort: deterministic by Id.
        cr_engine::sort::guid_compare(&a.id, &b.id)
    }
}

/// One group: its caption and the display-order items inside.
#[derive(Clone, Debug)]
pub struct Group {
    pub caption: String,
    pub collapsed: bool,
    /// Item indexes into `ViewState::display_order` — the items of
    /// this group in sort order.
    pub items: Vec<usize>,
}

/// The composed view: books → (group) → sort → display order.
#[derive(Clone, Debug, Default)]
pub struct ViewState {
    books: Vec<ComicBook>,
    /// `display_order[i]` = index into `books`.
    display_order: Vec<usize>,
    groups: Vec<Group>,
    sort: SortChain,
    /// The grouper column key (`None` = no grouping).
    grouper: Option<&'static str>,
    /// The quick-search filter (`ComicBookAllPropertiesMatcher` /
    /// a full query) — `None` shows everything.
    filter: Option<Matcher>,
    selected: HashSet<CrGuid>,
    focus: Option<CrGuid>,
    anchor: Option<CrGuid>,
}

impl ViewState {
    pub fn new(books: Vec<ComicBook>) -> ViewState {
        let mut view = ViewState {
            books,
            ..Default::default()
        };
        view.rebuild();
        view
    }

    pub fn books(&self) -> &[ComicBook] {
        &self.books
    }

    pub fn sort(&self) -> &SortChain {
        &self.sort
    }

    pub fn grouper(&self) -> Option<&'static str> {
        self.grouper
    }

    pub fn groups(&self) -> &[Group] {
        &self.groups
    }

    /// The display order: book indexes in final view order
    /// (grouped: group order + in-group sort order; ungrouped: sort
    /// order). Collapsed groups keep their items in the order (the
    /// layout skips them).
    pub fn display_order(&self) -> &[usize] {
        &self.display_order
    }

    pub fn book(&self, display_index: usize) -> &ComicBook {
        &self.books[self.display_order[display_index]]
    }

    pub fn book_id(&self, display_index: usize) -> CrGuid {
        self.book(display_index).id
    }

    pub fn len(&self) -> usize {
        self.display_order.len()
    }

    pub fn is_empty(&self) -> bool {
        self.display_order.is_empty()
    }

    pub fn set_sort_column(&mut self, column: &str) {
        self.sort.toggle_or_push(column);
        self.rebuild();
    }

    /// `ItemSorter = null` (the Arrange menu's Not Sorted row).
    pub fn clear_sort(&mut self) {
        self.sort = SortChain::default();
        self.rebuild();
    }

    /// Flips the first sort key's direction (the Reverse Direction
    /// command).
    pub fn toggle_direction(&mut self) {
        self.sort.toggle_direction();
        self.rebuild();
    }

    /// Sets the first sort key's direction (the T14 restore).
    pub fn set_direction(&mut self, descending: bool) {
        self.sort.set_direction(descending);
        self.rebuild();
    }

    pub fn set_grouper(&mut self, grouper: Option<&'static str>) {
        self.grouper = grouper;
        self.rebuild();
    }

    /// Group header collapse (`ItemViewGroupsStatus`): collapsed
    /// groups keep their header and lose their items from the
    /// display order (the C# builds `viewableItems` per expanded
    /// group).
    pub fn set_collapsed(&mut self, group: usize, collapsed: bool) {
        if let Some(g) = self.groups.get_mut(group) {
            g.collapsed = collapsed;
        }
        // `rebuild` carries the collapse flags across (by caption).
        self.rebuild();
    }

    pub fn set_books(&mut self, books: Vec<ComicBook>) {
        self.books = books;
        self.rebuild();
    }

    pub fn set_filter(&mut self, filter: Option<Matcher>) {
        self.filter = filter;
        self.rebuild();
    }

    /// The current filter (kept across book-set swaps — a removal
    /// inside a narrowed view stays narrowed).
    pub fn filter_clone(&self) -> Option<Matcher> {
        self.filter.clone()
    }

    fn rebuild(&mut self) {
        // The group buckets: `GroupManager` → containers keyed by
        // (sort_key, caption), ordered by the `GroupInfo.Compare`
        // rule — bucket index first, caption tiebreak through the
        // ExtendedStringComparer (IgnoreArticles | IgnoreCase).
        let (grouper, sort) = (self.grouper, self.sort.clone());
        let grouper_fn: Option<Grouper> =
            grouper.and_then(|key| groupers().iter().find(|(k, _)| *k == key).map(|(_, g)| *g));
        // The collapse flags carry across rebuilds by caption (the
        // C# persists collapse by caption hash) — read them BEFORE
        // the groups reset below.
        let previous_collapse: std::collections::HashMap<String, bool> = self
            .groups
            .iter()
            .map(|g| (g.caption.clone(), g.collapsed))
            .collect();
        self.display_order.clear();
        self.groups.clear();

        // Bucket the books.
        struct Bucket {
            caption: String,
            sort_key: i32,
            items: Vec<usize>,
        }
        let mut buckets: Vec<Bucket> = Vec::new();
        let bucket_of = |caption: &str, sort_key: i32, buckets: &mut Vec<Bucket>| {
            buckets
                .iter()
                .position(|b| b.sort_key == sort_key && b.caption == caption)
                .unwrap_or_else(|| {
                    buckets.push(Bucket {
                        caption: caption.to_string(),
                        sort_key,
                        items: Vec::new(),
                    });
                    buckets.len() - 1
                })
        };
        // The quick-search filter (the C# `quickFilter` in
        // `FillBookList`).
        let allowed: Option<Vec<CrGuid>> = self.filter.as_ref().map(|m| {
            let items: Vec<&ComicBook> = self.books.iter().collect();
            let ctx = MatchContext::new(&items);
            let pairs = [(cr_core::model::enums::MatcherMode::And, false, m)];
            match_set(&items, &pairs, &ctx)
                .iter()
                .map(|b| b.id)
                .collect()
        });
        for (index, book) in self.books.iter().enumerate() {
            if let Some(allowed) = &allowed {
                if !allowed.contains(&book.id) {
                    continue;
                }
            }
            let (caption, sort_key) = match grouper_fn {
                Some(g) => {
                    let info: GroupInfo = g(book);
                    (info.caption, info.sort_key)
                }
                None => (String::new(), 0),
            };
            let bucket = bucket_of(&caption, sort_key, &mut buckets);
            buckets[bucket].items.push(index);
        }
        buckets.sort_by(|a, b| {
            a.sort_key.cmp(&b.sort_key).then_with(|| {
                if a.caption == UNSPECIFIED || b.caption == UNSPECIFIED {
                    // The C# compare: null/empty captions sort
                    // last within their bucket; the ladder
                    // captions never tie across buckets, so this
                    // only matters for the single-bucket case.
                    std::cmp::Ordering::Equal
                } else {
                    extended_compare_ignore_articles_case(&a.caption, &b.caption)
                        // Deterministic tie-break for captions
                        // equal under the ignore-articles
                        // compare (the C# sort is unstable
                        // here).
                        .then_with(|| a.caption.cmp(&b.caption))
                }
            })
        });

        // Sort inside each bucket (the chained comparer), then
        // append to the display order. Collapsed groups keep their
        // header and drop their items.
        self.groups.clear();
        for bucket in buckets {
            let mut items = bucket.items;
            items.sort_by(|&x, &y| sort.compare(&self.books[x], &self.books[y]));
            let collapsed = previous_collapse
                .get(&bucket.caption)
                .copied()
                .unwrap_or(false);
            if collapsed {
                self.groups.push(Group {
                    caption: bucket.caption,
                    collapsed: true,
                    items: Vec::new(),
                });
            } else {
                self.groups.push(Group {
                    caption: bucket.caption,
                    collapsed: false,
                    items: (self.display_order.len()..self.display_order.len() + items.len())
                        .collect(),
                });
                self.display_order.extend(items);
            }
        }
    }

    // ---------- Selection model (`ItemViewStates`) ----------

    pub fn is_selected(&self, id: &CrGuid) -> bool {
        self.selected.contains(id)
    }

    pub fn selection(&self) -> &HashSet<CrGuid> {
        &self.selected
    }

    pub fn focus(&self) -> Option<CrGuid> {
        self.focus
    }

    pub fn anchor(&self) -> Option<CrGuid> {
        self.anchor
    }

    pub fn display_index_of(&self, id: &CrGuid) -> Option<usize> {
        self.display_order
            .iter()
            .position(|&i| self.books[i].id == *id)
    }

    /// Plain click: clear, select + focus, new anchor
    /// (`UpdateSelectionFromMouse`).
    pub fn select_one(&mut self, id: CrGuid) {
        self.selected.clear();
        self.selected.insert(id);
        self.focus = Some(id);
        self.anchor = Some(id);
    }

    /// `RefreshList` selection restoration: replace the selection
    /// with the given ids (focus + anchor follow the first).
    pub fn restore_selection(&mut self, ids: &[CrGuid]) {
        self.selected.clear();
        for id in ids {
            self.selected.insert(*id);
        }
        self.focus = ids.first().copied();
        self.anchor = ids.first().copied();
    }

    /// Ctrl+click: flip the item's selection (`Flip(Selected)`); the
    /// focus follows the item, the anchor stays (it marks the last
    /// plain click — the range start).
    pub fn select_flip(&mut self, id: CrGuid) {
        if self.selected.contains(&id) {
            self.selected.remove(&id);
        } else {
            self.selected.insert(id);
        }
        self.focus = Some(id);
    }

    /// Shift+click: the display-order range from the anchor
    /// (`SelectFromAnchorItem`). Without an anchor it behaves like a
    /// plain click.
    pub fn select_range(&mut self, id: CrGuid) {
        let Some(anchor) = self.anchor else {
            self.select_one(id);
            return;
        };
        let Some(from) = self.display_index_of(&anchor) else {
            self.select_one(id);
            return;
        };
        let Some(to) = self.display_index_of(&id) else {
            return;
        };
        let (lo, hi) = if from <= to { (from, to) } else { (to, from) };
        self.selected.clear();
        for i in lo..=hi {
            self.selected.insert(self.book_id(i));
        }
        self.focus = Some(id);
    }

    pub fn select_all(&mut self) {
        self.selected = self.books.iter().map(|b| b.id).collect();
    }

    pub fn clear_selection(&mut self) {
        self.selected.clear();
    }

    /// Focus movement for keyboard navigation; `extend` = Shift
    /// (range from the anchor), `ctrl` = move focus only.
    pub fn move_focus(&mut self, target: Option<CrGuid>, extend: bool, ctrl: bool) {
        let Some(target) = target else {
            return;
        };
        if ctrl {
            self.focus = Some(target);
            return;
        }
        if extend {
            self.select_range(target);
            self.focus = Some(target);
        } else {
            self.select_one(target);
        }
    }

    /// Rubber-band commit: the band items from a snapshot (Ctrl
    /// flips from the snapshot, `UpdateSelection`).
    pub fn apply_band(&mut self, band_ids: &[CrGuid], flip_from: &HashSet<CrGuid>, ctrl: bool) {
        if ctrl {
            let mut next = flip_from.clone();
            for id in band_ids {
                if next.contains(id) {
                    next.remove(id);
                } else {
                    next.insert(*id);
                }
            }
            self.selected = next;
        } else {
            self.selected = band_ids.iter().copied().collect();
        }
    }

    pub fn selection_snapshot(&self) -> HashSet<CrGuid> {
        self.selected.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(series: &str, number: f32, id: u8) -> ComicBook {
        let mut b = ComicBook {
            id: CrGuid::from_bytes([id; 16]),
            ..Default::default()
        };
        b.info.series = series.to_string();
        b.info.number = format!("{number}");
        b
    }

    /// An unsorted view shows the books in the INPUT order (a
    /// reading list's stored order). The Guid tiebreak must apply
    /// only under an ACTIVE sort — a fallback on the empty chain
    /// shuffled imported lists into Guid order (the 2026-09-06
    /// reading-list order bug).
    #[test]
    fn unsorted_view_keeps_the_input_order() {
        // Deliberately non-alphabetical, non-Guid-ordered input.
        let view = ViewState::new(vec![
            book("Web of Spider-Man", 40.0, 1),
            book("The Amazing Spider-Man", 296.0, 2),
            book("Peter Parker, the Spectacular Spider-Man", 134.0, 3),
        ]);
        let series: Vec<String> = (0..view.len())
            .map(|i| view.book(i).info.series.clone())
            .collect();
        assert_eq!(
            series,
            [
                "Web of Spider-Man",
                "The Amazing Spider-Man",
                "Peter Parker, the Spectacular Spider-Man"
            ]
        );
    }

    #[test]
    fn sort_chain_orders_and_toggles() {
        let mut view = ViewState::new(vec![
            book("Batman", 3.0, 1),
            book("Batman", 1.0, 2),
            book("Superman", 1.0, 3),
        ]);
        assert!(view.sort().keys().is_empty());
        view.set_sort_column("Series");
        // Series order: Batman #1, Batman #3 (series comparer is
        // number-aware), then Superman.
        let names: Vec<String> = (0..view.len())
            .map(|i| view.book(i).info.series.clone())
            .collect();
        assert_eq!(names, ["Batman", "Batman", "Superman"]);
        let numbers: Vec<f32> = (0..view.len())
            .map(|i| view.book(i).info.number.parse().unwrap())
            .collect();
        assert_eq!(numbers, [1.0, 3.0, 1.0]);

        // Same column again → descending (the series comparer stays
        // at the front of the chain).
        view.set_sort_column("Series");
        let names: Vec<String> = (0..view.len())
            .map(|i| view.book(i).info.series.clone())
            .collect();
        assert_eq!(names, ["Superman", "Batman", "Batman"]);
        assert!(view.sort().keys()[0].descending);
    }

    #[test]
    fn sort_chain_is_mru_three_deep() {
        let mut view = ViewState::new(vec![book("A", 1.0, 1), book("B", 1.0, 2)]);
        view.set_sort_column("Series");
        view.set_sort_column("Writer");
        view.set_sort_column("Title");
        view.set_sort_column("Number");
        assert_eq!(view.sort().keys().len(), SortChain::MAX);
        assert_eq!(view.sort().keys()[0].column, "Number");
        assert_eq!(view.sort().keys()[2].column, "Writer");
    }

    #[test]
    fn grouping_buckets_and_orders() {
        let mut view = ViewState::new(vec![
            book("Superman", 1.0, 1),
            book("batman", 2.0, 2),
            book("Batman", 1.0, 3),
        ]);
        view.set_grouper(Some("Series"));
        // Buckets form on the caption; the ORDER tie-breaks
        // case-insensitively (GroupInfo.Compare) — "Batman" and
        // "batman" sit adjacent, before "Superman".
        let captions: Vec<String> = view.groups().iter().map(|g| g.caption.clone()).collect();
        assert_eq!(captions, ["Batman", "batman", "Superman"]);
        assert_eq!(view.groups()[0].items.len(), 1);
        assert_eq!(view.groups()[1].items.len(), 1);
    }

    #[test]
    fn selection_model_matches_the_c_sharp() {
        let mut view = ViewState::new(vec![
            book("A", 1.0, 1),
            book("B", 1.0, 2),
            book("C", 1.0, 3),
            book("D", 1.0, 4),
        ]);
        fn id(view: &ViewState, i: usize) -> CrGuid {
            view.book_id(i)
        }
        view.select_one(id(&view, 1));
        assert!(view.is_selected(&id(&view, 1)) && view.selection().len() == 1);
        assert_eq!(view.anchor(), Some(id(&view, 1)));

        view.select_flip(id(&view, 3));
        assert_eq!(view.selection().len(), 2);
        assert_eq!(
            view.anchor(),
            Some(id(&view, 1)),
            "ctrl-flip keeps the anchor"
        );

        view.select_range(id(&view, 0));
        assert_eq!(
            view.selection().len(),
            2,
            "range anchor..click = items 0..1"
        );

        view.select_one(id(&view, 2));
        view.select_range(id(&view, 3));
        assert_eq!(view.selection().len(), 2, "range 2..3");

        // Ctrl move: focus only, selection untouched.
        view.select_one(id(&view, 0));
        let before = view.selection().clone();
        view.move_focus(Some(id(&view, 3)), false, true);
        assert_eq!(view.focus(), Some(id(&view, 3)));
        assert_eq!(view.selection(), &before);

        // Rubber band: replace or flip-from-snapshot.
        view.apply_band(
            &[id(&view, 1), id(&view, 2)],
            &view.selection_snapshot(),
            false,
        );
        assert_eq!(view.selection().len(), 2);
        let snap = view.selection_snapshot();
        view.apply_band(&[id(&view, 2)], &snap, true);
        assert_eq!(view.selection().len(), 1);
        assert!(view.is_selected(&id(&view, 1)));
    }
}
