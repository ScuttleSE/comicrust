//! The smart-list queries a user writes to find the books a scan
//! marked (PORT ADDITION, user request 2026-09-11).
//!
//! These tests exist because the matcher has NO "is not empty"
//! operator: `ComicBookCustomValuesMatcher` uses the string operator
//! list, and `contains ""` matches EVERY book (the C# empty-needle
//! rule). The queries below are the ones that actually work, verified
//! end to end: parse the query text, evaluate it, and check which
//! books come back.

use cr_core::model::comic_book::ComicBook;
use cr_core::model::enums::MatcherMode;
use cr_core::scan_status::{self, ScanStatus, ScanVerdict};
use cr_core::xml::scalar::CrGuid;
use cr_engine::matcher::eval::{match_set, MatchContext};
use cr_engine::matcher::query;
use cr_engine::matcher::tree::Matcher;

/// A book carrying the verdict a scan would have stored.
fn book(path: &str, status: Option<ScanStatus>) -> ComicBook {
    let mut b = ComicBook {
        id: CrGuid::new_random(),
        file_path: path.into(),
        ..Default::default()
    };
    let verdict = ScanVerdict {
        status,
        error: status.map(|_| "some reason".to_string()),
        ..Default::default()
    };
    scan_status::apply(&mut b, &verdict, "2026-09-11T18:00:00Z");
    b
}

/// The library used by every case: one book per verdict, plus a clean
/// one and one carrying an unrelated user custom value.
fn library() -> Vec<ComicBook> {
    let mut with_user_value = book("/c/clean-with-custom.cbz", None);
    with_user_value.custom_values_store =
        cr_core::model::comic_book::values_store::encode(&[("Location".into(), "Shelf".into())]);
    vec![
        book("/c/unreadable.cbz", Some(ScanStatus::Unreadable)),
        book("/c/timedout.cbz", Some(ScanStatus::TimedOut)),
        book("/c/skipped.cbz", Some(ScanStatus::Skipped)),
        book("/c/mismatch.cbz", Some(ScanStatus::FormatMismatch)),
        book("/c/clean.cbz", None),
        with_user_value,
    ]
}

/// Parses the query text and returns the matching file paths, sorted.
fn matches(query_text: &str) -> Vec<String> {
    let parsed = query::parse_smart_list_query(query_text)
        .unwrap_or_else(|e| panic!("the query must parse: {query_text}\n{e:?}"));
    let books = library();
    let refs: Vec<&ComicBook> = books.iter().collect();
    let group = Matcher::Group(parsed.group);
    let ctx = MatchContext::new(&refs);
    let mut hits: Vec<String> = match_set(&refs, &[(MatcherMode::And, group.not(), &group)], &ctx)
        .into_iter()
        .map(|b| b.file_path.clone())
        .collect();
    hits.sort();
    hits
}

#[test]
fn every_marked_book_regex_dot() {
    // "is not empty" does not exist. The regex operator with the
    // pattern "." is the honest equivalent: it matches any value with
    // at least one character, and an absent/empty value has none.
    let hits = matches(r#"Match [Custom Value] regex "comicrust.scan.status" ".""#);
    assert_eq!(
        hits,
        vec![
            "/c/mismatch.cbz",
            "/c/skipped.cbz",
            "/c/timedout.cbz",
            "/c/unreadable.cbz",
        ],
        "the regex form must find every marked book and nothing else"
    );
}

#[test]
fn contains_with_an_empty_value_matches_everything() {
    // The trap this test documents: `contains ""` is NOT "is not
    // empty". The C# empty-needle rule makes it match every book.
    let hits = matches(r#"Match [Custom Value] contains "comicrust.scan.status" """#);
    assert_eq!(hits.len(), 6, "contains \"\" matches the whole library");
}

#[test]
fn one_verdict_at_a_time_with_equals() {
    for (status, path) in [
        (ScanStatus::Unreadable, "/c/unreadable.cbz"),
        (ScanStatus::TimedOut, "/c/timedout.cbz"),
        (ScanStatus::Skipped, "/c/skipped.cbz"),
        (ScanStatus::FormatMismatch, "/c/mismatch.cbz"),
    ] {
        let q = format!(
            r#"Match [Custom Value] equals "comicrust.scan.status" "{}""#,
            status.as_text()
        );
        assert_eq!(matches(&q), vec![path.to_string()], "{q}");
    }
}

#[test]
fn the_broken_ones_only_with_contains_any_of() {
    // The three failure verdicts, excluding the readable mismatch.
    // `contains any of` splits on commas, so the two-word texts survive.
    let hits = matches(
        r#"Match [Custom Value] contains any of "comicrust.scan.status" "Unreadable,Timed out,Skipped""#,
    );
    assert_eq!(
        hits,
        vec!["/c/skipped.cbz", "/c/timedout.cbz", "/c/unreadable.cbz"],
        "the readable format-mismatch book must NOT be in the broken list"
    );
}

#[test]
fn the_queries_round_trip_through_the_renderer() {
    // A saved query must survive the editor: parse, render, parse
    // again, and still mean the same thing.
    for q in [
        r#"Match [Custom Value] regex "comicrust.scan.status" ".""#,
        r#"Match [Custom Value] equals "comicrust.scan.status" "Unreadable""#,
        r#"Match [Custom Value] contains any of "comicrust.scan.status" "Unreadable,Timed out,Skipped""#,
    ] {
        let first = query::parse_smart_list_query(q).expect("parses");
        let rendered = query::render_smart_list_query(&first);
        let second = query::parse_smart_list_query(&rendered).expect("re-parses");
        assert_eq!(first, second, "round trip changed the query: {q}");
        assert_eq!(matches(q), matches(&rendered), "{q}");
    }
}
