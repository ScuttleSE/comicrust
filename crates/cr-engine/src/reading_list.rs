//! The `.cbl` reading-list matching — the `ComicIdListItem
//! .CreateFromReadingList` port (`ComicRack.Engine/Database/
//! ComicIdListItem.cs:154-234`) plus the `ComicInfo.SeriesEquals`
//! helper with its `CompareSeriesOptions` regexes.
//!
//! Each list item resolves against the library: by book Guid, by file
//! name, then by series/number with progressive relaxation (volume in
//! the name, full strip-down, then year ±1, volume, format narrowing
//! with fall-back to the previous candidate set). An unsolved item
//! becomes a placeholder `ComicBook` (fresh Guid, `AddedTime` now,
//! the file-name-parsed series data) that the caller offers to add.

use cr_core::database::reading_list::{ReadingListContainer, ReadingListItem};
use cr_core::model::comic_book::ComicBook;
use cr_core::model::comic_name_info as name_info;
use cr_core::xml::scalar::{CrDateTime, CrGuid};
use std::sync::OnceLock;

use crate::matcher::book_view;

/// `CompareSeriesOptions` (None = 1, IgnoreVolumeInName = 2,
/// StripDown = 4 — the C# flag values; `contains` mirrors `HasFlag`).
#[derive(Clone, Copy, PartialEq)]
pub struct CompareSeriesOptions(pub u32);

impl CompareSeriesOptions {
    pub const NONE: Self = Self(1);
    pub const IGNORE_VOLUME_IN_NAME: Self = Self(2);
    pub const STRIP_DOWN: Self = Self(4);

    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for CompareSeriesOptions {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// `ComicInfo.rxVolume`: `\bv(ol(ume)?)?\.?\s?\d+\b\s*` (IgnoreCase).
fn rx_volume() -> &'static fancy_regex::Regex {
    static R: OnceLock<fancy_regex::Regex> = OnceLock::new();
    R.get_or_init(|| fancy_regex::Regex::new(r"(?i)\bv(ol(ume)?)?\.?\s?\d+\b\s*").expect("regex"))
}

/// `ComicInfo.rxSpecial`: `[^a-z0-9]|\bthe\b|\band\b|` (IgnoreCase).
/// The trailing empty alternative matches nothing but empty — dropped
/// (a no-op for `Replace`).
fn rx_special() -> &'static fancy_regex::Regex {
    static R: OnceLock<fancy_regex::Regex> = OnceLock::new();
    R.get_or_init(|| fancy_regex::Regex::new(r"(?i)[^a-z0-9]|\bthe\b|\band\b").expect("regex"))
}

/// `ComicInfo.SeriesEquals` — OrdinalIgnoreCase after the option
/// transformations (`rxVolume` strip + trim, `rxSpecial` strip).
pub fn series_equals(a: &str, b: &str, options: CompareSeriesOptions) -> bool {
    let mut a = a.to_string();
    let mut b = b.to_string();
    if options.contains(CompareSeriesOptions::IGNORE_VOLUME_IN_NAME) {
        a = strip_all(&a, rx_volume()).trim().to_string();
        b = strip_all(&b, rx_volume()).trim().to_string();
    }
    if options.contains(CompareSeriesOptions::STRIP_DOWN) {
        a = strip_all(&a, rx_special());
        b = strip_all(&b, rx_special());
    }
    a.eq_ignore_ascii_case(&b)
}

/// `Regex.Replace(input, "", String.Empty)` — all matches removed.
fn strip_all(text: &str, rx: &fancy_regex::Regex) -> String {
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    let mut pos = 0;
    while pos < text.len() {
        match rx.find_from_pos(text, pos) {
            Ok(Some(m)) => {
                out.push_str(&text[last..m.start()]);
                last = m.end();
                // Both static patterns always consume; guard a
                // zero-width match anyway (advance one char).
                pos = if m.end() > m.start() {
                    m.end()
                } else {
                    text[m.end()..]
                        .chars()
                        .next()
                        .map(|c| m.end() + c.len_utf8())
                        .unwrap_or(m.end())
                };
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }
    out.push_str(&text[last..]);
    out
}

/// The `CreateFromReadingList` result: the book ids in list order
/// (placeholders included) and the unsolved placeholder books.
pub struct ReadingListMatch {
    pub book_ids: Vec<CrGuid>,
    pub new_books: Vec<ComicBook>,
}

/// Resolves `crli`'s stored fields from its file name
/// (`ComicReadingListItem.SetFileNameInfo`: `SetInfo` with
/// `onlyEmpty: false` — the parsed values OVERWRITE).
fn set_file_name_info(item: &mut ReadingListItem) {
    if item.file_name.is_empty() {
        return;
    }
    let cni = name_info::from_file_path_configured(&item.file_name);
    item.series = cni.series;
    item.number = cni.number;
    item.volume = cni.volume;
    item.year = cni.year;
    item.format = cni.format;
}

/// The shadow accessors need the per-book proposed `ComicNameInfo`.
fn shadow_parts<'a>(
    book: &'a ComicBook,
    prop: &'a name_info::ComicNameInfo,
) -> (&'a str, &'a str, i32, i32, &'a str) {
    (
        book_view::shadow_series(book, prop),
        book_view::shadow_number(book, prop),
        book_view::shadow_year(book, prop),
        book_view::shadow_volume(book, prop),
        book_view::shadow_format(book, prop),
    )
}

/// The per-book row of the precomputed shadow table: the five shadow
/// values the series ladder reads, plus the two transformed series
/// forms `series_equals` would recompute per call (the
/// IGNORE_VOLUME_IN_NAME form, and the IV|StripDown form — the exact
/// `rxVolume` → trim → `rxSpecial` chain of the C# helper).
struct BookShadow {
    series: String,
    series_iv: String,
    series_sd: String,
    number: String,
    year: i32,
    volume: i32,
    format: String,
}

impl BookShadow {
    fn new(book: &ComicBook) -> Self {
        // The proposed values are read only where a shadow field
        // falls through to the file-name parse (`EnableProposed` +
        // an empty stored value). One parse per book, not per match.
        let needs_prop = book.enable_proposed
            && (book.info.series.is_empty()
                || book.info.number.is_empty()
                || book.info.format.is_empty()
                || book.info.volume == -1
                || book.info.year == -1);
        let prop = if needs_prop {
            book_view::proposed(book)
        } else {
            name_info::ComicNameInfo::new()
        };
        let (series, number, year, volume, format) = shadow_parts(book, &prop);
        let series = series.to_string();
        let series_iv = strip_all(&series, rx_volume()).trim().to_string();
        let series_sd = strip_all(&series_iv, rx_special());
        BookShadow {
            series,
            series_iv,
            series_sd,
            number: number.to_string(),
            year,
            volume,
            format: format.to_string(),
        }
    }
}

/// The per-call precompute: book id / file-name indexes (first book
/// wins, `find` parity) plus the shadow table.
struct LibraryIndex {
    by_id: std::collections::HashMap<CrGuid, usize>,
    by_file: std::collections::HashMap<String, usize>,
    rows: Vec<BookShadow>,
}

impl LibraryIndex {
    fn new(library: &[ComicBook]) -> Self {
        let mut by_id: std::collections::HashMap<CrGuid, usize> =
            std::collections::HashMap::with_capacity(library.len());
        let mut by_file: std::collections::HashMap<String, usize> =
            std::collections::HashMap::with_capacity(library.len());
        for (i, b) in library.iter().enumerate() {
            by_id.entry(b.id).or_insert(i);
            if !b.file_path.is_empty() {
                by_file
                    .entry(
                        name_info::file_name_without_extension(&b.file_path).to_ascii_lowercase(),
                    )
                    .or_insert(i);
            }
        }
        LibraryIndex {
            by_id,
            by_file,
            rows: library.iter().map(BookShadow::new).collect(),
        }
    }
}

/// The `ComicIdListItem.CreateFromReadingList` port over a library
/// book slice (the C# runs it against `Library.Books`).
pub fn create_from_reading_list(
    library: &[ComicBook],
    reading_items: &[ReadingListItem],
) -> ReadingListMatch {
    let mut match_result = ReadingListMatch {
        book_ids: Vec::new(),
        new_books: Vec::new(),
    };
    let index = LibraryIndex::new(library);
    for crli in reading_items {
        let mut crli = crli.clone();
        // 1. By book Guid (`library[crli.Id]`).
        let mut book = if crli.id.is_empty() {
            None
        } else {
            index.by_id.get(&crli.id).copied().map(|i| &library[i])
        };
        // 2. By file name (`FindItemByFileName` — OrdinalIgnoreCase
        // against the book's name-without-extension; the `.cbl`
        // stores exactly that).
        if book.is_none() && !crli.file_name.is_empty() {
            book = index
                .by_file
                .get(&crli.file_name.to_ascii_lowercase())
                .copied()
                .map(|i| &library[i]);
        }
        // 3. Series/number matching over the file-name-parsed values.
        if book.is_none() {
            set_file_name_info(&mut crli);
            // The item-side series forms (`series_equals` transforms
            // both sides; the book side lives in the shadow table).
            let item_series_iv = strip_all(&crli.series, rx_volume()).trim().to_string();
            let item_series_sd = strip_all(&item_series_iv, rx_special());
            let series_match = |key: fn(&BookShadow) -> &str, item_series: &str| -> Vec<usize> {
                index
                    .rows
                    .iter()
                    .enumerate()
                    .filter(|(_, r)| {
                        r.number == crli.number && key(r).eq_ignore_ascii_case(item_series)
                    })
                    .map(|(i, _)| i)
                    .collect()
            };
            let mut candidates = series_match(|r| &r.series, &crli.series);
            if candidates.is_empty() {
                candidates = series_match(|r| &r.series_iv, &item_series_iv);
            }
            if candidates.is_empty() {
                candidates = series_match(|r| &r.series_sd, &item_series_sd);
            }
            // Year ±1 narrowing, fall-back to the previous set.
            if candidates.len() > 1 {
                let narrowed: Vec<_> = candidates
                    .iter()
                    .copied()
                    .filter(|&i| (index.rows[i].year - crli.year).abs() <= 1)
                    .collect();
                if !narrowed.is_empty() {
                    candidates = narrowed;
                }
            }
            // Volume narrowing.
            if candidates.len() > 1 {
                let narrowed: Vec<_> = candidates
                    .iter()
                    .copied()
                    .filter(|&i| index.rows[i].volume == crli.volume)
                    .collect();
                if !narrowed.is_empty() {
                    candidates = narrowed;
                }
            }
            // Format narrowing (only when the item carries one).
            if candidates.len() > 1 && !crli.format.is_empty() {
                let narrowed: Vec<_> = candidates
                    .iter()
                    .copied()
                    .filter(|&i| index.rows[i].format.eq_ignore_ascii_case(&crli.format))
                    .collect();
                if !narrowed.is_empty() {
                    candidates = narrowed;
                }
            }
            book = candidates.first().map(|&i| &library[i]);
            // 4. Unsolved: a placeholder book (fresh Guid, AddedTime
            // now, the parsed series data).
            if book.is_none() {
                let cb = ComicBook {
                    id: CrGuid::new_random(),
                    added_time: CrDateTime::now(),
                    info: cr_core::model::comic_info::ComicInfo {
                        series: crli.series.clone(),
                        number: crli.number.clone(),
                        volume: crli.volume,
                        year: crli.year,
                        format: crli.format.clone(),
                        ..Default::default()
                    },
                    ..Default::default()
                };
                match_result.new_books.push(cb);
                book = match_result.new_books.last();
            }
        }
        match_result
            .book_ids
            .push(book.expect("book or placeholder").id);
    }
    match_result
}

/// The whole `.cbl` in one call — parse, then match (the C# splits
/// the parse (`Deserialize`) from the matching (`CreateFromReadingList`)).
pub fn match_container(
    library: &[ComicBook],
    container: &ReadingListContainer,
) -> ReadingListMatch {
    create_from_reading_list(library, &container.items)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book(
        series: &str,
        number: &str,
        volume: i32,
        year: i32,
        format: &str,
        file: &str,
    ) -> ComicBook {
        ComicBook {
            id: CrGuid::new_random(),
            file_path: file.to_string(),
            info: cr_core::model::comic_info::ComicInfo {
                series: series.into(),
                number: number.into(),
                volume,
                year,
                format: format.into(),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn series_equals_strips_the_and_volume_words() {
        // NONE: plain OrdinalIgnoreCase.
        assert!(!series_equals(
            "The Avengers",
            "Avengers",
            CompareSeriesOptions::NONE
        ));
        // STRIP_DOWN removes the article and all non-alphanumerics.
        assert!(series_equals(
            "The Avengers",
            "Avengers",
            CompareSeriesOptions::STRIP_DOWN
        ));
        assert!(series_equals(
            "Batman Vol. 2",
            "Batman",
            CompareSeriesOptions::IGNORE_VOLUME_IN_NAME
        ));
        assert!(series_equals(
            "Bat-Man!",
            "Batman",
            CompareSeriesOptions::IGNORE_VOLUME_IN_NAME | CompareSeriesOptions::STRIP_DOWN
        ));
        assert!(!series_equals(
            "Superman",
            "Batman",
            CompareSeriesOptions::NONE
        ));
    }

    #[test]
    fn resolves_by_id_and_file_name() {
        let b1 = book("Batman", "1", -1, 2010, "", "/tmp/Batman 1.cbz");
        let lib = vec![b1];
        let by_id = ReadingListItem {
            id: lib[0].id,
            ..Default::default()
        };
        let m = create_from_reading_list(&lib, &[by_id]);
        assert_eq!(m.book_ids, vec![lib[0].id]);
        assert!(m.new_books.is_empty());

        let by_name = ReadingListItem {
            file_name: "batman 1".into(),
            ..Default::default()
        };
        let m = create_from_reading_list(&lib, &[by_name]);
        assert_eq!(m.book_ids, vec![lib[0].id]);
        assert!(m.new_books.is_empty());
    }

    #[test]
    fn unsolved_item_becomes_a_placeholder_with_parsed_values() {
        let lib = vec![];
        let item = ReadingListItem {
            series: "ignored".into(),
            number: "0".into(),
            volume: -1,
            year: -1,
            format: "ignored".into(),
            file_name: "/comics/Daredevil 025 (2014).cbz".into(),
            id: CrGuid::EMPTY,
        };
        let m = create_from_reading_list(&lib, &[item]);
        assert_eq!(m.book_ids.len(), 1);
        assert_eq!(m.new_books.len(), 1);
        let cb = &m.new_books[0];
        assert!(!cb.id.is_empty());
        assert!(!cb.added_time.is_min_value());
        // The file-name parse replaced the stored values.
        assert_eq!(cb.info.series, "Daredevil");
        assert_eq!(cb.info.number, "25");
        assert_eq!(cb.info.year, 2014);
    }

    #[test]
    fn series_match_relaxes_and_keeps_the_year_volume_format_narrowing() {
        let lib = vec![
            book("Batman", "1", 1, 2010, "", "/a.cbz"),
            book("Batman", "1", 2, 2015, "", "/b.cbz"),
            book("Batman", "1", 3, 2019, "", "/c.cbz"),
        ];
        // Exact series + number hits three; the year ±1 narrowing
        // (2016) keeps the 2015 book, the volume narrowing confirms it.
        let item = ReadingListItem {
            series: "Batman".into(),
            number: "1".into(),
            volume: 2,
            year: 2016,
            format: "".into(),
            ..Default::default()
        };
        let m = create_from_reading_list(&lib, std::slice::from_ref(&item));
        assert_eq!(m.book_ids, vec![lib[1].id]);

        // No year (stored -1): the narrowing stays off, the volume
        // narrowing picks volume 1.
        let item2 = ReadingListItem {
            series: "Batman".into(),
            number: "1".into(),
            volume: 1,
            year: -1,
            format: "".into(),
            ..Default::default()
        };
        let m2 = create_from_reading_list(&lib, &[item2]);
        assert_eq!(m2.book_ids, vec![lib[0].id]);

        // Strip-down pass: the stored series carries punctuation.
        let lib2 = vec![book("Bat-Man", "1", -1, 2010, "", "/d.cbz")];
        let item3 = ReadingListItem {
            series: "Batman".into(),
            number: "1".into(),
            ..Default::default()
        };
        let m3 = create_from_reading_list(&lib2, &[item3]);
        assert_eq!(m3.book_ids, vec![lib2[0].id]);
    }

    #[test]
    fn format_narrows_the_candidates() {
        let lib = vec![
            book("Batman", "1", -1, 2010, "", "/a.cbz"),
            book("Batman", "1", -1, 2010, "Annual", "/b.cbz"),
        ];
        let item = ReadingListItem {
            series: "Batman".into(),
            number: "1".into(),
            format: "Annual".into(),
            ..Default::default()
        };
        let m = create_from_reading_list(&lib, &[item]);
        assert_eq!(m.book_ids, vec![lib[1].id]);
    }
}
