//! Series-relative book lookups (`get_earliest_book` /
//! `get_last_book` in locommon.py), with the addon's exact
//! comparison quirks kept: the `!= 1` year guard of
//! `get_earliest_book`, the string ordering of `ShadowNumber`, and
//! the "adopt whatever follows" behavior when the tracked book's
//! number is not numeric.
//!
//! Results are cached per series key
//! (`Publisher + ShadowSeries + str(ShadowVolume)`), like the addon's
//! module-level `startbooks` / `endbooks` dicts.

use std::collections::HashMap;

use cr_core::model::comic_book::ComicBook;
use cr_engine::matcher::book_view;

/// The series key (`Publisher + ShadowSeries + str(ShadowVolume)`).
fn series_key(book: &ComicBook) -> String {
    let prop = book_view::proposed_cached(book);
    format!(
        "{}{}{}",
        book.info.publisher,
        book_view::shadow_series(book, &prop),
        book_view::shadow_volume(book, &prop)
    )
}

/// The shadow values of one book, resolved once per call.
#[derive(Clone)]
struct Shadow {
    year: i32,
    month: i32,
    number: String,
}

fn shadow(book: &ComicBook) -> Shadow {
    let prop = book_view::proposed_cached(book);
    Shadow {
        year: book_view::shadow_year(book, &prop),
        month: book.info.month,
        number: book_view::shadow_number(book, &prop).to_string(),
    }
}

fn same_series(a: &ComicBook, b: &ComicBook) -> bool {
    let pa = book_view::proposed_cached(a);
    let pb = book_view::proposed_cached(b);
    book_view::shadow_series(a, &pa) == book_view::shadow_series(b, &pb)
        && book_view::shadow_volume(a, &pa) == book_view::shadow_volume(b, &pb)
        && a.info.publisher == b.info.publisher
}

fn is_digits(text: &str) -> bool {
    !text.is_empty() && text.chars().all(|c| c.is_ascii_digit())
}

/// The earliest/latest-of-series index cache over one library
/// snapshot.
pub struct SeriesIndex<'a> {
    pub books: &'a [ComicBook],
    earliest: HashMap<String, usize>,
    latest: HashMap<String, usize>,
}

impl<'a> SeriesIndex<'a> {
    pub fn new(books: &'a [ComicBook]) -> Self {
        SeriesIndex {
            books,
            earliest: HashMap::new(),
            latest: HashMap::new(),
        }
    }

    /// `get_earliest_book` — the earliest published issue of the
    /// series (year, then month, then the string-ordered issue
    /// number).
    pub fn earliest_book(&mut self, book: &ComicBook, book_index: usize) -> &ComicBook {
        let key = series_key(book);
        if let Some(i) = self.earliest.get(&key) {
            return &self.books[*i];
        }
        let mut start_idx = book_index;
        let mut start_shadow = shadow(book);
        for (i, b) in self.books.iter().enumerate() {
            if !same_series(book, b) {
                continue;
            }
            let bs = shadow(b);
            // The addon's guard is `!= 1` (not `!= -1`); kept verbatim.
            if start_shadow.year == -1 && bs.year != 1 {
                start_idx = i;
                start_shadow = bs.clone();
            }
            if bs.year != -1 && bs.year < start_shadow.year {
                start_idx = i;
                start_shadow = bs.clone();
            }
            if bs.year == start_shadow.year && bs.month != -1 {
                if start_shadow.month == -1 || bs.month < start_shadow.month {
                    start_idx = i;
                    start_shadow = bs.clone();
                } else if bs.month == start_shadow.month && bs.number < start_shadow.number {
                    // Python string ordering ("10" < "9").
                    start_idx = i;
                    start_shadow = bs.clone();
                }
            }
        }
        self.earliest.insert(key, start_idx);
        &self.books[start_idx]
    }

    /// `get_last_book` — the highest numeric issue number of the
    /// series.
    pub fn last_book(&mut self, book: &ComicBook, book_index: usize) -> &ComicBook {
        let key = series_key(book);
        if let Some(i) = self.latest.get(&key) {
            return &self.books[*i];
        }
        let mut end_idx = book_index;
        let mut end_number = shadow(book).number;
        for (i, b) in self.books.iter().enumerate() {
            if !same_series(book, b) {
                continue;
            }
            let bs = shadow(b);
            if !is_digits(&end_number) {
                // Adopt whatever follows (the addon's first branch).
                end_idx = i;
                end_number = bs.number.clone();
                continue;
            }
            if is_digits(&bs.number) {
                let end_num: i64 = end_number.parse().unwrap_or(0);
                let b_num: i64 = bs.number.parse().unwrap_or(0);
                if end_num < b_num {
                    end_idx = i;
                    end_number = bs.number.clone();
                }
            }
        }
        self.latest.insert(key, end_idx);
        &self.books[end_idx]
    }
}
