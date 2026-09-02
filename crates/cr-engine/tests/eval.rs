//! Matcher evaluation tests: table-driven per matcher family over
//! synthetic `ComicBook`s, plus group-pipeline semantics (And/Or/Not).

use cr_core::model::comic_book::ComicBook;
use cr_core::model::enums::{MatcherMode, YesNo};
use cr_engine::matcher::eval::{match_set, matches, MatchContext};
use cr_engine::matcher::tree::Matcher;

fn book(series: &str, number: &str, year: i32) -> ComicBook {
    let mut b = ComicBook::default();
    b.info.series = series.into();
    b.info.number = number.into();
    b.info.year = year;
    b.enable_proposed = false;
    b
}

/// Parses a `Match` query into its group matcher.
fn m(query: &str) -> Matcher {
    let mut t = cr_engine::tokenizer::Tokenizer::new(query);
    Matcher::Group(cr_engine::matcher::query::parse_group_query(&mut t).expect(query))
}

fn hit(books: &[ComicBook], matcher: &Matcher) -> Vec<usize> {
    let refs: Vec<&ComicBook> = books.iter().collect();
    let ctx = MatchContext::new(&refs);
    match_set(&refs, &[(MatcherMode::And, matcher.not(), matcher)], &ctx)
        .iter()
        .map(|b| {
            refs.iter()
                .position(|r| std::ptr::eq(*r, *b))
                .expect("subset member")
        })
        .collect()
}

#[test]
fn string_family() {
    let books = vec![
        {
            let mut b = book("Batman", "1", 2000);
            b.info.writer = "Alan Moore".into();
            b
        },
        {
            let mut b = book("Watchmen", "1", 1986);
            b.info.writer = "Moore, Davis".into();
            b
        },
        book("Sandman", "3", 1990),
    ];
    let _ = &books;

    let cases: &[(&str, Vec<usize>)] = &[
        ("Match [Writer] equals \"Alan Moore\"", vec![0]),
        ("Match [Writer] equals \"alan moore\"", vec![0]),
        ("Match [Writer] contains \"Moore\"", vec![0, 1]),
        (
            "Match [Writer] contains any of \"moore, davis\"",
            vec![0, 1],
        ),
        ("Match [Writer] contains all of \"alan moore\"", vec![0]),
        ("Match [Writer] starts with \"Alan\"", vec![0]),
        ("Match [Writer] ends with \"Davis\"", vec![1]),
        // "list contains": the BOOK value is the list, the match value
        // the member (C# builds the regex from MatchValue).
        ("Match [Writer] list contains \"davis\"", vec![1]),
        ("Match [Series] regex \"^Sand.*n$\"", vec![2]),
        ("Match Not [Series] equals \"Batman\"", vec![1, 2]),
    ];
    for (query, expected) in cases {
        let matcher = m(query);
        assert_eq!(&hit(&books, &matcher), expected, "query: {query}");
    }
}

#[test]
fn numeric_family() {
    let books = vec![
        {
            let mut b = book("A", "1", 2000);
            b.rating = 3.0;
            b
        },
        {
            let mut b = book("B", "12A", 2005);
            b.info.page_count = 20;
            b.last_page_read = 19; // ReadPercentage: (19+1)*100/20 = 100
            b
        },
        book("C", "-", 2010),
    ];

    let cases: &[(&str, Vec<usize>)] = &[
        ("Match [My Rating] equals \"3\"", vec![0]),
        ("Match [My Rating] is greater \"0\"", vec![0]),
        ("Match [My Rating] is smaller \"1\"", vec![1, 2]),
        ("Match [Number] equals \"12\"", vec![1]),
        ("Match [Number] is greater \"0\"", vec![0, 1]),
        ("Match [Year] in range \"1999\" \"2005\"", vec![0, 1]),
        ("Match [Read Percentage] in range \"90\" \"100\"", vec![1]),
        ("Match [Read Percentage] is greater \"0\"", vec![1]),
    ];
    for (query, expected) in cases {
        let matcher = m(query);
        assert_eq!(&hit(&books, &matcher), expected, "query: {query}");
    }
}

#[test]
fn date_family() {
    let mut added = book("A", "1", 2000);
    added.added_time = cr_core::xml::scalar::CrDateTime::parse("2026-09-01T12:00:00Z").unwrap();
    let mut released = book("B", "1", 1990);
    released.released_time =
        cr_core::xml::scalar::CrDateTime::parse("1990-05-01T00:00:00Z").unwrap();
    let books = vec![added, released];

    let cases: &[(&str, Vec<usize>)] = &[
        ("Match [Added] is after \"2026-08-31\"", vec![0]),
        // The other book's AddedTime is DateTime.MinValue — before everything.
        ("Match [Added] is before \"2026-09-01\"", vec![1]),
        // The other book's ReleasedTime is DateTime.MinValue — before 2000 too.
        ("Match [Released] is before \"2000-01-01\"", vec![0, 1]),
        (
            "Match [Released] is in range \"1989-01-01\" \"1991-01-01\"",
            vec![1],
        ),
    ];
    for (query, expected) in cases {
        let matcher = m(query);
        assert_eq!(&hit(&books, &matcher), expected, "query: {query}");
    }
}

#[test]
fn yesno_and_manga_family() {
    let mut a = book("A", "1", 2000);
    a.checked = true;
    a.series_complete = YesNo::Yes;
    let mut b = book("B", "1", 2000);
    b.checked = false;
    b.info.black_and_white = YesNo::Yes;
    b.series_complete = YesNo::Unknown;
    let books = vec![a, b];

    let cases: &[(&str, Vec<usize>)] = &[
        ("Match [Is Checked] equals yes", vec![0]),
        ("Match Not [Is Checked] equals yes", vec![1]),
        ("Match [Black and White] equals yes", vec![1]),
        ("Match [Series complete] equals Unknown", vec![1]),
        ("Match [Has Custom Values] equals no", vec![0, 1]),
    ];
    for (query, expected) in cases {
        let matcher = m(query);
        assert_eq!(&hit(&books, &matcher), expected, "query: {query}");
    }

    let mut manga_book = book("M", "1", 2020);
    manga_book.info.manga = cr_core::model::enums::MangaYesNo::YesAndRightToLeft;
    let books = vec![manga_book];
    assert_eq!(hit(&books, &m("Match [Manga] equals ltr")), vec![0]);
}

#[test]
fn custom_values_family() {
    let mut a = book("A", "1", 2000);
    a.custom_values_store = "Location=Home,Read=True".into();
    let books = vec![a, book("B", "1", 2000)];

    let cases: &[(&str, Vec<usize>)] = &[
        ("Match [Custom Value] equals \"Location\" \"Home\"", vec![0]),
        ("Match [Custom Value] equals \"location\" \"home\"", vec![0]),
        ("Match [Custom Value] contains \"Read\" \"ru\"", vec![0]),
        ("Match [Has Custom Values] equals yes", vec![0]),
    ];
    for (query, expected) in cases {
        let matcher = m(query);
        assert_eq!(&hit(&books, &matcher), expected, "query: {query}");
    }
}

#[test]
fn all_properties_family() {
    let mut a = book("Batman", "1", 2000);
    a.info.writer = "Moore".into();
    a.info.tags = "favorite".into();
    let books = vec![a, book("Sandman", "2", 1990)];

    // The `All` wildcard searches every string property.
    let cases: &[(&str, Vec<usize>)] = &[
        ("Match [All] equals \"Batman\"", vec![0]),
        ("Match [All] equals \"Moore\"", vec![0]),
        ("Match [All] equals \"Sandman\"", vec![1]),
        ("Match [All] contains \"favor\"", vec![0]),
    ];
    for (query, expected) in cases {
        let matcher = m(query);
        assert_eq!(&hit(&books, &matcher), expected, "query: {query}");
    }
}

#[test]
fn group_pipeline_and_or_not() {
    let books = vec![
        book("A", "1", 2000),
        book("B", "2", 1990),
        book("C", "3", 2010),
    ];

    // And: both children must match.
    let group = m("Match All\r\n{\r\n    [Year] in range \"1900\" \"2020\",\r\n    [Year] is greater \"1999\"\r\n}");
    assert_eq!(hit(&books, &group), vec![0, 2]);

    // Or: union.
    let group = m("Match Any\r\n{\r\n    [Series] equals \"A\",\r\n    [Series] equals \"C\"\r\n}");
    assert_eq!(hit(&books, &group), vec![0, 2]);

    // Not child inside And removes.
    let group = m("Match All\r\n{\r\n    [Year] in range \"1900\" \"2020\",\r\n    Not [Year] is greater \"1999\"\r\n}");
    assert_eq!(hit(&books, &group), vec![1]);

    // Nested groups.
    let group = m("Match All\r\n{\r\n    Match Any\r\n    {\r\n        [Series] equals \"A\",\r\n        [Series] equals \"B\"\r\n    },\r\n    [Year] is smaller \"2000\"\r\n}");
    assert_eq!(hit(&books, &group), vec![1]);
}

#[test]
fn series_statistics_family() {
    let mut a = book("Batman", "1", 2000);
    a.info.volume = 1;
    let mut b = book("Batman", "2", 2001);
    b.info.volume = 1;
    b.last_page_read = 0;
    let mut c = book("Batman", "5", 2002);
    c.info.volume = 1;
    // Different series key (volume 2) — separate stats.
    let mut d = book("Batman", "1", 2010);
    d.info.volume = 2;
    let books = vec![a, b, c, d];

    let cases: &[(&str, Vec<usize>)] = &[
        ("Match [Series: Book Count] equals \"3\"", vec![0, 1, 2]),
        ("Match [Series: Last Year] equals \"2002\"", vec![0, 1, 2]),
        (
            "Match [Series: Running Time Years] equals \"2\"",
            vec![0, 1, 2],
        ),
        ("Match [Series: Book Count] equals \"1\"", vec![3]),
        // Gap between 2 and 5: GapStart is number 2.
        ("Match [Series: Start of Gap] equals yes", vec![1]),
    ];
    for (query, expected) in cases {
        let matcher = m(query);
        assert_eq!(&hit(&books, &matcher), expected, "query: {query}");
    }
}

#[test]
fn duplicates_family() {
    let mut a = book("The Batman", "1", 2000);
    a.file_path = "C:\\x\\a.cbz".into();
    // Same metadata as a (compressed series name ignores "The").
    let mut b = book("Batman", "1", 2000);
    b.file_path = "C:\\x\\b.cbz".into();
    // Same path as a — path duplicate (case-insensitive).
    let mut c = book("Other", "9", 2020);
    c.file_path = "C:\\X\\A.CBZ".into();
    let solo = book("Solo", "1", 2001);
    let books = vec![a, b, c, solo];

    assert_eq!(hit(&books, &m("Match [Only Duplicates] on")), vec![0, 1, 2]);
    assert_eq!(
        hit(&books, &m("Match [Only Duplicates] off")),
        vec![0, 1, 2, 3]
    );
}

#[test]
fn matches_convenience_applies_not() {
    let b = book("A", "1", 2000);
    let refs = vec![&b];
    let ctx = MatchContext::new(&refs);
    let matcher = m("Match [Series] equals \"A\"");
    assert!(matches(&b, &matcher, &ctx));
    let mut not_matcher = m("Match [Series] equals \"A\"");
    if let Matcher::Group(g) = &mut not_matcher {
        if let Matcher::Value(v) = &mut g.matchers[0] {
            v.not = true;
        }
    }
    assert!(!matches(&b, &not_matcher, &ctx));
}
