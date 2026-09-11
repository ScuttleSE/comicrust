//! The matcher-tree edit operations — the `MatcherEditor` /
//! `MatcherGroupEditor` command model, pure and unit-tested.
//!
//! The C# editors mutate a nested `ComicBookMatcher` collection
//! (`AddRule` = duplicate the node after itself, `AddGroup` = wrap a
//! clone in a new group, Delete, MoveUp/MoveDown) and the matcher
//! TYPE switch keeps the values (`newMatcher.Set(current)`). The
//! nodes are addressed by an index PATH (container indexes, one
//! segment per group level).

use cr_core::database::list_items::{ComicBookMatcher, GroupMatcher, ValueMatcher};
use cr_core::model::enums::MatcherMode;

use super::spec;

/// The `MatcherEditor.MaxLevel` cap (group nesting).
pub const MAX_LEVEL: usize = 5;

fn walk<'a>(
    root: &'a mut Vec<ComicBookMatcher>,
    path: &[usize],
) -> Option<&'a mut ComicBookMatcher> {
    let mut list = root;
    for (i, idx) in path.iter().enumerate() {
        let last = i + 1 == path.len();
        if last {
            return list.get_mut(*idx);
        }
        match list.get_mut(*idx) {
            Some(ComicBookMatcher::Group(g)) => list = &mut g.matchers,
            _ => return None,
        }
    }
    None
}

/// The `MatcherEditor.AddRule`: a clone of the node inserts AFTER it
/// in its container.
pub fn add_rule(root: &mut Vec<ComicBookMatcher>, path: &[usize]) -> bool {
    if path.is_empty() {
        return false;
    }
    let Some(node) = walk(root, path) else {
        return false;
    };
    let clone = node.clone();
    // Re-walk to the CONTAINER (the parent list) and splice.
    let (parent_path, last) = path.split_at(path.len() - 1);
    let Some(parent) = walk_mut_list(root, parent_path) else {
        return false;
    };
    let at = last[0];
    if at >= parent.len() {
        return false;
    }
    parent.insert(at + 1, clone);
    true
}

fn walk_mut_list<'a>(
    root: &'a mut Vec<ComicBookMatcher>,
    path: &[usize],
) -> Option<&'a mut Vec<ComicBookMatcher>> {
    let mut list = root;
    for idx in path {
        match list.get_mut(*idx) {
            Some(ComicBookMatcher::Group(g)) => list = &mut g.matchers,
            _ => return None,
        }
    }
    Some(list)
}

/// The `MatcherEditor.AddGroup`: a new group containing a clone of
/// the node inserts after it. Group depth is capped at
/// [`MAX_LEVEL`] (the path length counts the levels above).
pub fn add_group(root: &mut Vec<ComicBookMatcher>, path: &[usize]) -> bool {
    if path.len() >= MAX_LEVEL {
        return false;
    }
    let Some(node) = walk(root, path) else {
        return false;
    };
    let clone = node.clone();
    let (parent_path, last) = path.split_at(path.len() - 1);
    let Some(parent) = walk_mut_list(root, parent_path) else {
        return false;
    };
    let at = last[0];
    if at >= parent.len() {
        return false;
    }
    let group = ComicBookMatcher::Group(GroupMatcher {
        not: false,
        matcher_mode: MatcherMode::And,
        collapsed: false,
        matchers: vec![clone],
    });
    parent.insert(at + 1, group);
    true
}

/// The `MatcherEditor.DeleteRuleOrGroup`. The C# disables Delete
/// while the edited collection holds a single node.
pub fn remove_node(root: &mut Vec<ComicBookMatcher>, path: &[usize]) -> bool {
    let (parent_path, last) = path.split_at(path.len() - 1);
    let Some(parent) = walk_mut_list(root, parent_path) else {
        return false;
    };
    if parent.len() <= 1 {
        return false;
    }
    let at = last[0];
    if at >= parent.len() {
        return false;
    }
    parent.remove(at);
    true
}

/// The `MatcherEditor.PasteClipboard` (`matchers.Insert(indexOf + 1,
/// clipboard)`): the payload inserts AFTER the addressed node. A
/// Group payload is rejected when the target sits at the
/// [`MAX_LEVEL`] cap — the same gate the C# checks
/// (`level <= MaxLevel` around the insert).
pub fn paste_node(
    root: &mut Vec<ComicBookMatcher>,
    path: &[usize],
    payload: &ComicBookMatcher,
) -> bool {
    if path.is_empty() {
        return false;
    }
    if matches!(payload, ComicBookMatcher::Group(_)) && path.len() >= MAX_LEVEL {
        return false;
    }
    let (parent_path, last) = path.split_at(path.len() - 1);
    let Some(parent) = walk_mut_list(root, parent_path) else {
        return false;
    };
    let at = last[0];
    if at >= parent.len() {
        return false;
    }
    parent.insert(at + 1, payload.clone());
    true
}

/// `MoveUp`/`MoveDown` (`matchers.MoveRelative`).
pub fn move_node(root: &mut Vec<ComicBookMatcher>, path: &[usize], delta: i32) -> bool {
    let (parent_path, last) = path.split_at(path.len() - 1);
    let Some(parent) = walk_mut_list(root, parent_path) else {
        return false;
    };
    let at = last[0] as i32;
    let to = at + delta;
    if at < 0 || to < 0 || to as usize >= parent.len() {
        return false;
    }
    parent.swap(at as usize, to as usize);
    true
}

/// The `btMatcher` type switch (`newMatcher.Set(current)`): a fresh
/// matcher of the new class keeps the operator/values/not, with the
/// operator clamped into the new spec's operator list.
pub fn switch_type(current: &ComicBookMatcher, new_class: &str) -> Option<ComicBookMatcher> {
    let value = match current {
        ComicBookMatcher::Value(v) => v,
        // The C# switches only VALUE matchers in the type menu.
        ComicBookMatcher::Group(_) => return None,
    };
    let matcher_spec = spec::by_class_name(new_class)?;
    let operator = (value.match_operator as usize).min(matcher_spec.operators().len() - 1);
    Some(ComicBookMatcher::Value(ValueMatcher {
        type_name: new_class.to_string(),
        not: value.not,
        name: value.name.clone(),
        match_value: value.match_value.clone(),
        match_value_2: value.match_value_2.clone(),
        match_operator: operator as i32,
        ignore_case: value.ignore_case,
        option: value.option.clone(),
        plugin_key: value.plugin_key.clone(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(series: &str) -> ComicBookMatcher {
        ComicBookMatcher::Value(ValueMatcher {
            type_name: "ComicBookSeriesMatcher".into(),
            match_operator: 3,
            match_value: series.into(),
            ..Default::default()
        })
    }

    fn series_of(m: &ComicBookMatcher) -> &str {
        match m {
            ComicBookMatcher::Value(v) => &v.match_value,
            _ => "",
        }
    }

    #[test]
    fn add_rule_duplicates_after_the_node() {
        let mut root = vec![value("A"), value("B")];
        assert!(add_rule(&mut root, &[0]));
        let names: Vec<&str> = root.iter().map(series_of).collect();
        assert_eq!(names, ["A", "A", "B"]);
    }

    #[test]
    fn add_group_wraps_a_clone() {
        let mut root = vec![value("A"), value("B")];
        assert!(add_group(&mut root, &[1]));
        assert_eq!(root.len(), 3);
        let names: Vec<&str> = root.iter().map(series_of).collect();
        assert_eq!(names, ["A", "B", ""], "the group is not a value");
        match &root[2] {
            ComicBookMatcher::Group(g) => {
                assert_eq!(g.matchers.len(), 1);
                assert_eq!(series_of(&g.matchers[0]), "B");
                assert_eq!(g.matcher_mode, MatcherMode::And);
            }
            _ => panic!("expected a group"),
        }
    }

    #[test]
    fn group_depth_is_capped() {
        let mut root = vec![ComicBookMatcher::Group(GroupMatcher {
            matchers: vec![ComicBookMatcher::Group(GroupMatcher {
                matchers: vec![ComicBookMatcher::Group(GroupMatcher {
                    matchers: vec![ComicBookMatcher::Group(GroupMatcher {
                        matchers: vec![value("deep")],
                        ..Default::default()
                    })],
                    ..Default::default()
                })],
                ..Default::default()
            })],
            ..Default::default()
        })];
        // The value sits at depth 5 — a group there is over the cap.
        let path = [0, 0, 0, 0, 0];
        assert!(!add_group(&mut root, &path));
        // Rules still duplicate at the cap.
        assert!(add_rule(&mut root, &path));
    }

    #[test]
    fn nested_operations_reach_inside_groups() {
        let mut root = vec![ComicBookMatcher::Group(GroupMatcher {
            matchers: vec![value("A"), value("B")],
            ..Default::default()
        })];
        // Remove B (path [0, 1]) — allowed while the group holds 2.
        assert!(remove_node(&mut root, &[0, 1]));
        match &root[0] {
            ComicBookMatcher::Group(g) => assert_eq!(g.matchers.len(), 1),
            _ => panic!(),
        }
        // The group alone cannot be removed (the root holds 1).
        assert!(!remove_node(&mut root, &[0]));
        // Move up inside the group.
        let mut root = vec![ComicBookMatcher::Group(GroupMatcher {
            matchers: vec![value("A"), value("B")],
            ..Default::default()
        })];
        assert!(move_node(&mut root, &[0, 1], -1));
        match &root[0] {
            ComicBookMatcher::Group(g) => {
                assert_eq!(series_of(&g.matchers[0]), "B")
            }
            _ => panic!(),
        }
        assert!(!move_node(&mut root, &[0, 0], -1), "already first");
    }

    #[test]
    fn switch_type_keeps_values_and_clamps_the_operator() {
        let mut current = value("Batman");
        if let ComicBookMatcher::Value(v) = &mut current {
            v.match_operator = 3;
            v.not = true;
        }
        // Switch to a numeric matcher whose operator list is shorter.
        let switched = switch_type(&current, "ComicBookPublishedMatcher").expect("a known spec");
        match switched {
            ComicBookMatcher::Value(v) => {
                assert_eq!(v.type_name, "ComicBookPublishedMatcher");
                assert_eq!(v.match_value, "Batman", "the values carry over");
                assert!(v.not, "the negation carries over");
                let spec = spec::by_class_name("ComicBookPublishedMatcher").unwrap();
                assert!(
                    (v.match_operator as usize) < spec.operators().len(),
                    "the operator clamps into the new list"
                );
            }
            _ => panic!(),
        }
        // Unknown classes fail.
        assert!(switch_type(&current, "NoSuchMatcher").is_none());
    }

    #[test]
    fn paste_inserts_after_the_node() {
        let mut root = vec![value("A"), value("B")];
        let payload = value("P");
        assert!(paste_node(&mut root, &[0], &payload));
        let names: Vec<&str> = root.iter().map(series_of).collect();
        assert_eq!(names, ["A", "P", "B"], "the payload lands after the node");
    }

    #[test]
    fn paste_of_a_group_respects_the_depth_cap() {
        let group = || {
            ComicBookMatcher::Group(GroupMatcher {
                matchers: vec![value("deep")],
                ..Default::default()
            })
        };
        let mut root = vec![ComicBookMatcher::Group(GroupMatcher {
            matchers: vec![ComicBookMatcher::Group(GroupMatcher {
                matchers: vec![ComicBookMatcher::Group(GroupMatcher {
                    matchers: vec![ComicBookMatcher::Group(GroupMatcher {
                        matchers: vec![value("deep")],
                        ..Default::default()
                    })],
                    ..Default::default()
                })],
                ..Default::default()
            })],
            ..Default::default()
        })];
        // A group payload at the cap (path [0,0,0,0,0]) is rejected.
        assert!(!paste_node(&mut root, &[0, 0, 0, 0, 0], &group()));
        // A value payload pastes there.
        assert!(paste_node(&mut root, &[0, 0, 0, 0, 0], &value("x")));
        // A group payload one level up pastes (the C# `level <= MaxLevel`).
        let mut shallow = vec![value("A")];
        assert!(paste_node(&mut shallow, &[0], &group()));
    }
}
