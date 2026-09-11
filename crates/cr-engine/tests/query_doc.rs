//! Drift gate for `docs/guides/smart-list-queries.md`.
//!
//! The guide is only useful if it cannot go stale. This test:
//!
//! 1. parses EVERY example query in the guide, and round-trips it
//!    through the renderer,
//! 2. checks every field name the guide's reference tables list exists
//!    in the matcher registry,
//! 3. checks every matcher in the registry appears in the guide,
//! 4. checks every operator word the guide documents is a real
//!    operator.
//!
//! A new matcher, a renamed operator, or a wrong example all fail here.

use cr_engine::matcher::{query, spec};

const GUIDE: &str = include_str!("../../../docs/guides/smart-list-queries.md");

/// The fenced `text` blocks of the guide, in order.
fn fenced_blocks() -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current: Option<String> = None;
    for line in GUIDE.lines() {
        if line.trim_start().starts_with("```") {
            match current.take() {
                Some(block) => blocks.push(block),
                None => current = Some(String::new()),
            }
            continue;
        }
        if let Some(block) = current.as_mut() {
            block.push_str(line);
            block.push('\n');
        }
    }
    blocks
}

/// The example blocks: the ones that actually start a query. The
/// grammar block at the top uses placeholders and is skipped.
fn example_queries() -> Vec<String> {
    fenced_blocks()
        .into_iter()
        .map(|b| b.trim().to_string())
        .filter(|b| {
            let lowered = b.to_lowercase();
            (lowered.starts_with("match ")
                || lowered.starts_with("name \"")
                || lowered.starts_with("in ["))
                && !b.contains('<')
        })
        .collect()
}

/// The field names in the guide's reference tables: the first column of
/// any table row whose cell is a single backticked name.
fn documented_fields() -> Vec<String> {
    let mut names = Vec::new();
    for line in GUIDE.lines() {
        let line = line.trim();
        if !line.starts_with('|') {
            continue;
        }
        let first = line
            .trim_matches('|')
            .split('|')
            .next()
            .unwrap_or("")
            .trim();
        if first.len() > 2 && first.starts_with('`') && first.ends_with('`') {
            let name = first.trim_matches('`').to_string();
            // Operator tables share the same shape; keep only the
            // cells that name a real matcher.
            if spec::by_description(&name).is_some() {
                names.push(name);
            }
        }
    }
    names
}

#[test]
fn every_example_query_parses_and_round_trips() {
    let examples = example_queries();
    // A sanity floor, not an exact count: it only proves the block
    // extraction still finds the examples. The real check is the loop.
    assert!(
        examples.len() >= 20,
        "expected the guide to carry its examples, found {}",
        examples.len()
    );
    for q in &examples {
        let parsed = query::parse_smart_list_query(q)
            .unwrap_or_else(|e| panic!("example query does not parse:\n{q}\n{e:?}"));
        let rendered = query::render_smart_list_query(&parsed);
        let reparsed = query::parse_smart_list_query(&rendered).unwrap_or_else(|e| {
            panic!("example query does not survive a save:\n{q}\nrendered: {rendered}\n{e:?}")
        });
        assert_eq!(
            parsed, reparsed,
            "example query changed meaning when saved:\n{q}\nrendered: {rendered}"
        );
    }
}

#[test]
fn every_registry_matcher_is_documented() {
    let documented = documented_fields();
    let missing: Vec<&str> = spec::all_specs()
        .iter()
        .map(|s| s.description)
        .filter(|d| !documented.iter().any(|n| n.eq_ignore_ascii_case(d)))
        .collect();
    assert!(
        missing.is_empty(),
        "docs/guides/smart-list-queries.md does not document these fields: {missing:?}"
    );
}

#[test]
fn every_documented_field_exists() {
    // `documented_fields` already filters to known names, so this
    // guards the filter itself: the guide must name a useful number of
    // real fields, not zero because the table shape changed.
    let documented = documented_fields();
    assert!(
        documented.len() >= spec::all_specs().len(),
        "the guide names {} fields but the registry has {}",
        documented.len(),
        spec::all_specs().len()
    );
}

#[test]
fn every_documented_operator_is_real() {
    let known: Vec<&str> = [
        spec::STRING_OPS,
        spec::NUMERIC_OPS,
        spec::DATE_OPS,
        spec::YESNO_OPS,
        spec::MANGA_OPS,
        spec::ONOFF_OPS,
        spec::EXPRESSION_OPS,
        spec::PLUGIN_OPS,
    ]
    .concat();
    // The operator words the guide claims, taken from its operator
    // tables (the backticked first cell that is NOT a field name).
    let mut claimed: Vec<String> = Vec::new();
    for line in GUIDE.lines() {
        let line = line.trim();
        if !line.starts_with('|') {
            continue;
        }
        let first = line
            .trim_matches('|')
            .split('|')
            .next()
            .unwrap_or("")
            .trim();
        if first.len() > 2 && first.starts_with('`') && first.ends_with('`') {
            let name = first.trim_matches('`').to_string();
            if spec::by_description(&name).is_none() {
                claimed.push(name);
            }
        }
    }
    assert!(
        !claimed.is_empty(),
        "the operator tables were not found in the guide"
    );
    let bogus: Vec<&String> = claimed
        .iter()
        .filter(|c| !known.iter().any(|k| k.eq_ignore_ascii_case(c)))
        .collect();
    assert!(
        bogus.is_empty(),
        "the guide documents operators that do not exist: {bogus:?}"
    );
}
