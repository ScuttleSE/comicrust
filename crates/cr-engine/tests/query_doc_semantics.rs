//! The behaviour claims made in `docs/guides/smart-list-queries.md`.
//!
//! `query_doc.rs` proves every example PARSES. These tests prove the
//! guide's statements about what the operators DO, so a reader who
//! follows the guide gets the result the guide promises.

use cr_core::model::comic_book::ComicBook;
use cr_core::model::enums::MatcherMode;
use cr_core::xml::scalar::{CrDateTime, CrGuid};
use cr_engine::matcher::eval::{match_set, MatchContext};
use cr_engine::matcher::query;
use cr_engine::matcher::tree::Matcher;

fn blank() -> ComicBook {
    ComicBook {
        id: CrGuid::new_random(),
        file_path: "/c/x.cbz".into(),
        ..Default::default()
    }
}

/// Runs a query over the given books and returns how many matched.
fn hits(query_text: &str, books: &[ComicBook]) -> usize {
    let parsed =
        query::parse_smart_list_query(query_text).unwrap_or_else(|e| panic!("{query_text}: {e:?}"));
    let refs: Vec<&ComicBook> = books.iter().collect();
    let group = Matcher::Group(parsed.group);
    let ctx = MatchContext::new(&refs);
    match_set(&refs, &[(MatcherMode::And, group.not(), &group)], &ctx).len()
}

#[test]
fn text_rules_ignore_case() {
    let mut b = blank();
    b.info.series = "Batman".into();
    assert_eq!(hits(r#"Match [Series] equals "batman""#, &[b.clone()]), 1);
    assert_eq!(hits(r#"Match [Series] equals "BATMAN""#, &[b]), 1);
}

#[test]
fn an_empty_search_value_matches_everything() {
    // The guide's first trap: `contains ""` is not "is empty".
    let mut has = blank();
    has.info.series = "Batman".into();
    let books = [blank(), has];
    assert_eq!(hits(r#"Match [Series] contains """#, &books), 2);
    assert_eq!(hits(r#"Match [Series] starts with """#, &books), 2);
    assert_eq!(hits(r#"Match [Series] ends with """#, &books), 2);
}

#[test]
fn regex_dot_is_the_is_not_empty_form_on_a_field_with_no_fallback() {
    // `Summary` reads the stored field only, so the guide's
    // "is not empty" trick behaves as described.
    let mut has = blank();
    has.info.summary = "Something happens.".into();
    let books = [blank(), has];
    assert_eq!(hits(r#"Match [Summary] regex ".""#, &books), 1);
    assert_eq!(hits(r#"Match Not [Summary] regex ".""#, &books), 1);
}

#[test]
fn series_falls_back_to_the_file_name_when_the_stored_field_is_empty() {
    // The guide's "fields that fall back to the file name" section.
    // A book with an EMPTY stored series still matches, because the
    // file name supplies one. This is why the metadata-cleanup example
    // uses Summary/Writer/Publisher instead of Series.
    let mut b = blank();
    b.file_path = "/comics/Batman 001.cbz".into();
    assert!(b.info.series.is_empty());
    assert!(b.enable_proposed, "the fallback needs proposed values on");
    assert_eq!(
        hits(r#"Match [Series] equals "Batman""#, &[b.clone()]),
        1,
        "the series comes from the file name when the field is empty"
    );
    assert_eq!(
        hits(r#"Match [Series] regex ".""#, &[b.clone()]),
        1,
        "so `regex \".\"` does NOT find a blank stored series"
    );
    // Turning proposed values off exposes the stored field again.
    b.enable_proposed = false;
    assert_eq!(hits(r#"Match [Series] regex ".""#, &[b]), 0);
}

#[test]
fn in_range_includes_both_ends() {
    let year = |y: i32| {
        let mut b = blank();
        b.info.year = y;
        b
    };
    let books = [year(1999), year(2000), year(2005), year(2009), year(2010)];
    assert_eq!(hits(r#"Match [Year] in range "2000" "2009""#, &books), 3);
}

#[test]
fn contains_any_of_splits_on_commas_so_two_word_values_survive() {
    let writer = |w: &str| {
        let mut b = blank();
        b.info.writer = w.into();
        b
    };
    let books = [
        writer("Grant Morrison"),
        writer("Alan Moore"),
        writer("Ennis"),
    ];
    assert_eq!(
        hits(
            r#"Match [Writer] contains any of "Grant Morrison,Alan Moore""#,
            &books
        ),
        2,
        "with a comma present the value is split on commas only"
    );
}

#[test]
fn list_contains_matches_a_whole_member_not_a_substring() {
    let tags = |t: &str| {
        let mut b = blank();
        b.info.tags = t.into();
        b
    };
    let books = [tags("favorite, reread"), tags("favorites"), tags("fav")];
    assert_eq!(hits(r#"Match [Tags] list contains "favorite""#, &books), 1);
    // `contains` would also hit "favorites"; that is the difference.
    assert_eq!(hits(r#"Match [Tags] contains "favorite""#, &books), 2);
}

#[test]
fn file_size_is_measured_in_megabytes() {
    // The guide states MB, not bytes. A 150 MB file must match
    // `is greater "100"`, and must NOT match `is greater "1000000"`.
    let mut b = blank();
    b.file_size = 150 * 1024 * 1024;
    assert_eq!(
        hits(r#"Match [File Size] is greater "100""#, &[b.clone()]),
        1
    );
    assert_eq!(
        hits(r#"Match [File Size] is greater "1000000""#, &[b]),
        0,
        "a byte-valued threshold would never match; the field is MB"
    );
}

#[test]
fn a_book_with_no_issue_number_reads_as_minus_one() {
    let mut numbered = blank();
    numbered.info.number = "5".into();
    let books = [blank(), numbered];
    assert_eq!(hits(r#"Match [Number] is smaller "0""#, &books), 1);
}

#[test]
fn is_in_last_days_counts_back_from_now() {
    let days_ago = |d: i64| {
        let mut b = blank();
        b.added_time = CrDateTime {
            naive: chrono::Utc::now().naive_utc() - chrono::Duration::days(d),
            kind: cr_core::xml::scalar::DateKind::Utc,
        };
        b
    };
    let books = [days_ago(1), days_ago(10), days_ago(100)];
    assert_eq!(hits(r#"Match [Added] is in last days "30""#, &books), 2);
}

#[test]
fn any_needs_one_rule_and_all_needs_every_rule() {
    let mut b = blank();
    b.info.series = "Batman".into();
    b.info.year = 2015;
    let books = [b];
    assert_eq!(
        hits(
            r#"Match Any { [Series] equals "Superman", [Year] is greater "2010" }"#,
            &books
        ),
        1
    );
    assert_eq!(
        hits(
            r#"Match All { [Series] equals "Superman", [Year] is greater "2010" }"#,
            &books
        ),
        0
    );
}

#[test]
fn not_inverts_a_rule_and_a_whole_group() {
    let mut b = blank();
    b.info.series = "Batman".into();
    b.info.format = "Annual".into();
    let books = [b];
    assert_eq!(hits(r#"Match Not [Series] equals "Batman""#, &books), 0);
    assert_eq!(
        hits(
            r#"Match All { [Series] equals "Batman", Not Match Any { [Format] equals "Annual", [Format] equals "TPB" } }"#,
            &books
        ),
        0,
        "the negated group removes the annual"
    );
}

#[test]
fn the_script_fields_never_match() {
    // ADR-027: no scripting host. The guide says these save but never
    // match, so a user is not left wondering why a list is empty.
    let mut b = blank();
    b.info.series = "Batman".into();
    let books = [b];
    assert_eq!(
        hits(
            r#"Match [Expression] is true "book.Series == 'Batman'""#,
            &books
        ),
        0
    );
    assert_eq!(hits(r#"Match [User Scripts] None"#, &books), 0);
}

#[test]
fn writer_regex_dot_finds_books_that_have_an_author() {
    // The guide's "books that have an author" recipe. `Writer` has no
    // file-name fallback, so the result is the stored field.
    let no_writer = blank();
    let mut has_writer = blank();
    has_writer.info.writer = "Alan Moore".into();
    let mut unlinked = ComicBook {
        file_path: String::new(),
        ..blank()
    };
    unlinked.info.writer = "Grant Morrison".into();
    let books = [no_writer, has_writer, unlinked];

    assert_eq!(hits(r#"Match [Writer] regex ".""#, &books), 2);
    assert_eq!(hits(r#"Match Not [Writer] regex ".""#, &books), 1);
    // The trap the guide warns about: an empty value is not "is empty".
    assert_eq!(hits(r#"Match [Writer] contains """#, &books), 3);
}

#[test]
fn an_unlinked_book_also_passes_the_empty_file_tests() {
    // The guide's "finding empty books" trap: a book with no file has
    // a size of 0 and a page count of 0, so `[Is Linked] equals yes`
    // is required to keep it out.
    let unlinked = ComicBook {
        file_path: String::new(),
        ..blank()
    };
    let mut real = blank();
    real.file_size = 40 * 1024 * 1024;
    real.info.page_count = 24;
    let books = [unlinked, real];

    assert_eq!(hits(r#"Match [File Size] is smaller "0.01""#, &books), 1);
    assert_eq!(hits(r#"Match [Page Count] equals "0""#, &books), 1);
    assert_eq!(
        hits(
            r#"Match All { [Is Linked] equals yes, [File Size] is smaller "0.01" }"#,
            &books
        ),
        0
    );
    assert_eq!(hits(r#"Match [Is Linked] equals no"#, &books), 1);
}

#[test]
fn the_empty_book_table_queries_parse() {
    // The "finding empty books" table holds its queries in table
    // cells, which `query_doc.rs` does not scan. Pin them here.
    let bare = blank();
    let mut tagged = blank();
    tagged.info.publisher = "Marvel".into();
    let books = [bare, tagged];

    // Each query parses, and the counts are the ones the table promises
    // for these two books.
    assert_eq!(hits(r#"Match [Is Linked] equals no"#, &books), 0);
    assert_eq!(hits(r#"Match [Is Missing] equals yes"#, &books), 0);
    assert_eq!(
        hits(
            r#"Match All { [Is Linked] equals yes, [File Size] is smaller "0.01" }"#,
            &books
        ),
        2
    );
    assert_eq!(
        hits(
            r#"Match All { [Is Linked] equals yes, [Page Count] equals "0" }"#,
            &books
        ),
        2
    );
    // The "no metadata" row: only the book with nothing stored.
    assert_eq!(
        hits(
            r#"Match Not Match Any { [Writer] regex ".", [Publisher] regex ".", [Summary] regex ".", [Notes] regex "." }"#,
            &books
        ),
        1
    );
}
