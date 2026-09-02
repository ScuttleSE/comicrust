//! T1 acceptance: every saved smart list in the real-world ComicDb.xml
//! binds to typed matchers, renders to a `Match` query string, re-parses
//! to an identical tree, and re-renders byte-identically.
//!
//! See `tests/realworld/README.md`: the fixture is byte-identical
//! round-trip tested elsewhere; this test never writes it.

use cr_core::database::comic_database;
use cr_core::database::list_items::{ComicListItem, SmartListItem};
use cr_engine::matcher::{query, spec, tree::GroupMatcher, tree::Matcher};

const REALWORLD_DB: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/realworld/ComicDb.xml"
);

fn smart_lists(db: &comic_database::ComicDatabase) -> Vec<&SmartListItem> {
    fn walk<'a>(items: &'a [ComicListItem], out: &mut Vec<&'a SmartListItem>) {
        for item in items {
            match item {
                ComicListItem::Smart(s) => out.push(s),
                ComicListItem::Folder(f) => walk(&f.items, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&db.comic_lists, &mut out);
    out
}

#[test]
fn realworld_smart_lists_query_round_trip() {
    let db = comic_database::load(REALWORLD_DB.as_ref()).expect("load real-world DB");
    let lists = smart_lists(&db);
    assert!(!lists.is_empty(), "fixture must contain smart lists");

    for list in &lists {
        // Bind the saved XML matcher tree to typed matchers.
        let matchers: Vec<Matcher> = list
            .matchers
            .iter()
            .map(Matcher::from_raw)
            .collect::<Option<Vec<_>>>()
            .unwrap_or_else(|| panic!("list {:?}: unknown matcher class", list.base.name));

        let group = GroupMatcher {
            not: false,
            matcher_mode: list.matcher_mode,
            collapsed: false,
            matchers,
        };
        let query1 = query::render_group_query(&group, true);

        // Re-parse and compare trees.
        let mut t = cr_engine::tokenizer::Tokenizer::new(&query1);
        let reparsed = query::parse_group_query(&mut t)
            .unwrap_or_else(|e| panic!("list {:?}: parse {query1:?}: {e}", list.base.name));
        assert!(
            reparsed.matchers == group.matchers,
            "list {:?}: re-parsed tree differs",
            list.base.name
        );
        assert_eq!(reparsed.matcher_mode, group.matcher_mode);

        // Re-render must be byte-identical.
        let query2 = query::render_group_query(&reparsed, true);
        assert_eq!(
            query2, query1,
            "list {:?}: render not stable",
            list.base.name
        );
    }
}

#[test]
fn realworld_matcher_specs_are_known() {
    // Every matcher class name used in the fixture resolves in the spec
    // table (guards against a typo'd class name in the table).
    let db = comic_database::load(REALWORLD_DB.as_ref()).expect("load real-world DB");
    let lists = smart_lists(&db);
    let mut count = 0;
    for list in lists {
        for m in &list.matchers {
            if let cr_core::database::list_items::ComicBookMatcher::Value(v) = m {
                let spec = spec::by_class_name(&v.type_name);
                assert!(spec.is_some(), "unbound class name {}", v.type_name);
                count += 1;
            }
        }
    }
    assert!(count > 0);
}
