//! Tree-level list evaluation — the `ComicListItem.GetBooks` family:
//! the Library root (all books), smart lists (the matcher pipeline),
//! folders (Or = union / And = intersect / Empty, by book id), and id
//! lists (stored book ids).
//!
//! C# reference: `ComicListItem.cs`, `ComicListItemFolder.cs`
//! (`OnGetBooks`), `ComicIdListItem.cs`. The C# resolves base lists
//! and folder children through the per-item caches
//! (`RecursionCache`); this port evaluates on demand with a cycle
//! guard (the C# `RecursionTest` renders recursive trees red instead).

use cr_core::database::comic_database::ComicDatabase;
use cr_core::database::list_items::ComicListItem;
use cr_core::model::comic_book::ComicBook;
use cr_core::model::enums::ComicFolderCombineMode;
use cr_core::xml::scalar::CrGuid;

use crate::smart_list::evaluate_smart_list;

/// Evaluates one tree node to its book set (first-seen order).
pub fn evaluate_list<'a>(item: &ComicListItem, db: &'a ComicDatabase) -> Vec<&'a ComicBook> {
    evaluate_inner(item, db, &mut Vec::new())
}

/// Runs `f` with the whole library as a `&[&ComicBook]` slice. The
/// slice borrow stays inside the closure; the returned refs keep the
/// database lifetime.
fn with_books<'a, R>(db: &'a ComicDatabase, f: impl FnOnce(&[&'a ComicBook]) -> R) -> R {
    let books: Vec<&'a ComicBook> = db.books.iter().collect();
    f(&books)
}

fn evaluate_inner<'a>(
    item: &ComicListItem,
    db: &'a ComicDatabase,
    visiting: &mut Vec<CrGuid>,
) -> Vec<&'a ComicBook> {
    let id = item.base().id;
    if visiting.contains(&id) {
        // A recursive list resolves to nothing (the C# marks the
        // recursion red in the tree).
        return Vec::new();
    }
    visiting.push(id);
    let result: Vec<&'a ComicBook> = match item {
        ComicListItem::Library(_) => db.books.iter().collect(),
        ComicListItem::Smart(smart) => with_books(db, |all| {
            let base_list = base_list_books(smart.base_list_id, db, visiting);
            evaluate_smart_list(smart, all, base_list.as_deref())
        }),
        ComicListItem::Folder(folder) => match folder.combine_mode {
            ComicFolderCombineMode::Or => {
                // Union by book id, first-seen order (the C#
                // `Union(..., ComicBook.GuidEquality)`). The membership
                // set is a HashSet — a linear scan made the union
                // O(N²) (the T10 measurement: 28 s at 10k books).
                let mut seen: std::collections::HashSet<CrGuid> = std::collections::HashSet::new();
                let mut union: Vec<&ComicBook> = Vec::new();
                for child in &folder.items {
                    for book in evaluate_inner(child, db, visiting) {
                        if seen.insert(book.id) {
                            union.push(book);
                        }
                    }
                }
                union
            }
            ComicFolderCombineMode::And => {
                // Intersect by book id; an empty child result makes
                // the whole folder empty (the C# breaks early).
                let mut intersection: Option<Vec<&ComicBook>> = None;
                for child in &folder.items {
                    let child_books = evaluate_inner(child, db, visiting);
                    let ids: std::collections::HashSet<CrGuid> =
                        child_books.iter().map(|b| b.id).collect();
                    intersection = Some(match intersection {
                        None => child_books,
                        Some(current) => current
                            .into_iter()
                            .filter(|b| ids.contains(&b.id))
                            .collect(),
                    });
                    if intersection.as_ref().is_none_or(|v| v.is_empty()) {
                        break;
                    }
                }
                intersection.unwrap_or_default()
            }
            ComicFolderCombineMode::Empty => Vec::new(),
        },
        // The C# `OnGetBooks` walks `bookIds` in LIST order (the .cbl
        // item order — the reading-list point) over a HashSet (the
        // first-seen dedupe; stale ids evaluate to nothing). The
        // browser shows the unsorted list in exactly this order.
        ComicListItem::IdList(list) => {
            let index: std::collections::HashMap<CrGuid, &ComicBook> =
                db.books.iter().map(|b| (b.id, b)).collect();
            // First-seen dedupe over a HashSet (a linear Vec scan made
            // the walk O(N²) — the T10 measurement: 277 ms at 10k).
            let mut seen: std::collections::HashSet<CrGuid> = std::collections::HashSet::new();
            list.book_ids
                .iter()
                .filter(|id| seen.insert(**id))
                .filter_map(|id| index.get(id).copied())
                .collect()
        }
    };
    visiting.pop();
    result
}

/// Resolves a smart list's `BaseListId` to the base list's book set.
/// `Guid.Empty` (the default) means "all books" — `None` here.
fn base_list_books<'a>(
    base_list_id: CrGuid,
    db: &'a ComicDatabase,
    visiting: &mut Vec<CrGuid>,
) -> Option<Vec<&'a ComicBook>> {
    if base_list_id.is_empty() {
        return None;
    }
    let item = find_list_item(&db.comic_lists, &base_list_id)?;
    Some(evaluate_inner(&item, db, visiting))
}

/// Finds a node by id anywhere in the tree (breadth over the tree
/// shape; the trees are small).
pub fn find_list_item(items: &[ComicListItem], id: &CrGuid) -> Option<ComicListItem> {
    for item in items {
        if item.base().id == *id {
            return Some(item.clone());
        }
        if let ComicListItem::Folder(folder) = item {
            if let Some(found) = find_list_item(&folder.items, id) {
                return Some(found);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use cr_core::database::comic_database::create_new;
    use cr_core::database::list_items::{
        ComicBookMatcher, FolderItem, IdListItem, ListItemBase, SmartListItem, ValueMatcher,
    };
    use cr_core::model::comic_book::ComicBook;

    const REALWORLD_DB: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/realworld/ComicDb.xml"
    );

    fn smart(name: &str, type_name: &str, op: i32, v1: &str) -> ComicListItem {
        ComicListItem::Smart(SmartListItem {
            base: ListItemBase {
                id: CrGuid::new_random(),
                name: Some(name.into()),
                ..Default::default()
            },
            matchers: vec![ComicBookMatcher::Value(ValueMatcher {
                type_name: type_name.into(),
                match_operator: op,
                match_value: v1.into(),
                ..Default::default()
            })],
            ..Default::default()
        })
    }

    /// The reading-list order: the C# `OnGetBooks` walks `bookIds`
    /// (the .cbl item order), so an unsorted list shows the books in
    /// exactly that order — NOT the library/DB order.
    #[test]
    fn id_list_evaluates_in_book_ids_order() {
        let mut db = create_new();
        let a = ComicBook {
            id: CrGuid::new_random(),
            ..Default::default()
        };
        let b = ComicBook {
            id: CrGuid::new_random(),
            ..Default::default()
        };
        let c = ComicBook {
            id: CrGuid::new_random(),
            ..Default::default()
        };
        // Deliberately shuffled library order: b, a, c.
        db.books = vec![b.clone(), a.clone(), c.clone()];
        // The list order: c, a, b (the .cbl item order).
        let list = ComicListItem::IdList(IdListItem {
            base: ListItemBase {
                id: CrGuid::new_random(),
                name: Some("Ordered".into()),
                ..Default::default()
            },
            book_ids: vec![c.id, a.id, b.id],
        });
        let books = evaluate_list(&list, &db);
        let ids: Vec<CrGuid> = books.iter().map(|b| b.id).collect();
        assert_eq!(ids, vec![c.id, a.id, b.id]);
    }

    #[test]
    fn library_node_yields_all_books() {
        let db = cr_core::database::load(REALWORLD_DB.as_ref()).unwrap();
        let library = db
            .comic_lists
            .iter()
            .find_map(|i| match i {
                ComicListItem::Library(l) => Some(l),
                _ => None,
            })
            .unwrap();
        let books = evaluate_list(&ComicListItem::Library(library.clone()), &db);
        assert_eq!(books.len(), db.books.len());
        assert_eq!(books.len(), 255);
    }

    #[test]
    fn folder_combine_modes_match_the_c_sharp() {
        let mut db = create_new();
        // Two books: one read, one not (distinct ids — book identity
        // is the guid).
        let read = ComicBook {
            id: CrGuid::new_random(),
            info: cr_core::model::comic_info::ComicInfo {
                page_count: 10,
                ..Default::default()
            },
            ..ComicBook::default()
        };
        let mut unread = ComicBook {
            id: CrGuid::new_random(),
            ..Default::default()
        };
        unread.info.page_count = 10;
        db.books = vec![read, unread];

        let read_list = smart("Read", "ComicBookReadPercentageMatcher", 1, "95");
        let unread_list = smart("Never Read", "ComicBookReadPercentageMatcher", 2, "10");

        // Or: union.
        let or_folder = ComicListItem::Folder(FolderItem {
            base: ListItemBase {
                id: CrGuid::new_random(),
                name: Some("Or".into()),
                ..Default::default()
            },
            items: vec![read_list.clone(), unread_list.clone()],
            ..Default::default()
        });
        assert_eq!(evaluate_list(&or_folder, &db).len(), 2);

        // And: intersection — a book cannot be read and unread.
        let and_folder = ComicListItem::Folder(FolderItem {
            base: ListItemBase {
                id: CrGuid::new_random(),
                name: Some("And".into()),
                ..Default::default()
            },
            items: vec![read_list.clone(), unread_list.clone()],
            combine_mode: cr_core::model::enums::ComicFolderCombineMode::And,
            ..Default::default()
        });
        assert!(evaluate_list(&and_folder, &db).is_empty());

        // Empty combine: nothing.
        let empty_folder = ComicListItem::Folder(FolderItem {
            base: ListItemBase {
                id: CrGuid::new_random(),
                name: Some("Empty".into()),
                ..Default::default()
            },
            items: vec![read_list, unread_list],
            combine_mode: cr_core::model::enums::ComicFolderCombineMode::Empty,
            ..Default::default()
        });
        assert!(evaluate_list(&empty_folder, &db).is_empty());
    }

    #[test]
    fn id_list_resolves_stored_ids() {
        let mut db = create_new();
        let b0 = ComicBook {
            id: CrGuid::new_random(),
            ..Default::default()
        };
        let b1 = ComicBook {
            id: CrGuid::new_random(),
            ..Default::default()
        };
        db.books = vec![b0.clone(), b1.clone()];
        let id_list = ComicListItem::IdList(IdListItem {
            base: ListItemBase {
                id: CrGuid::new_random(),
                name: Some("Picks".into()),
                ..Default::default()
            },
            book_ids: vec![b1.id],
        });
        let books = evaluate_list(&id_list, &db);
        assert_eq!(books.len(), 1);
        assert_eq!(books[0].id, b1.id);
    }

    #[test]
    fn realworld_default_tree_evaluates_like_the_saved_lists() {
        // The default tree: the folder's Or-union of its children
        // must contain every book the children match, and the Library
        // root covers everything.
        let db = cr_core::database::load(REALWORLD_DB.as_ref()).unwrap();
        let library = db
            .comic_lists
            .iter()
            .find_map(|i| match i {
                ComicListItem::Library(l) => Some(l),
                _ => None,
            })
            .unwrap();
        let all = evaluate_list(&ComicListItem::Library(library.clone()), &db);
        assert_eq!(all.len(), 255);

        // The "Smart Lists" folder from the default tree.
        let folder = db
            .comic_lists
            .iter()
            .find_map(|i| match i {
                ComicListItem::Folder(f) => Some(f),
                _ => None,
            })
            .unwrap();
        let union = evaluate_list(&ComicListItem::Folder(folder.clone()), &db);
        // Every child result must be inside the union.
        let union_ids: std::collections::HashSet<CrGuid> = union.iter().map(|b| b.id).collect();
        for child in &folder.items {
            for book in evaluate_list(child, &db) {
                assert!(
                    union_ids.contains(&book.id),
                    "child result missing from the folder union"
                );
            }
        }
        // The Phase 2 ground truth: Never Read = 255, Read = 0.
        let never = evaluate_list(
            &folder
                .items
                .iter()
                .find(|i| i.base().name.as_deref() == Some("Never Read"))
                .unwrap()
                .clone(),
            &db,
        );
        assert_eq!(never.len(), 255);
    }

    #[test]
    fn recursive_base_list_terminates() {
        // A list whose base list is itself must not hang; it yields
        // the plain matcher result (base resolution refuses).
        let mut list = ComicListItem::Smart(SmartListItem {
            base: ListItemBase {
                id: CrGuid::new_random(),
                name: Some("Self".into()),
                ..Default::default()
            },
            matchers: vec![],
            ..Default::default()
        });
        if let ComicListItem::Smart(ref mut s) = list {
            s.base_list_id = s.base.id;
        }
        let db = create_new();
        let books = evaluate_list(&list, &db);
        assert!(books.is_empty());
    }
}
