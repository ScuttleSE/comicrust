//! Series-statistics support: the `ComicBookSeriesStatistics` aggregate
//! values the `SmartListSeries*` matchers read, and the stats provider
//! that computes them per series (C# `ComicBookSeriesStatistics.Create`).

use std::collections::HashMap;

use cr_core::model::comic_book::ComicBook;
use cr_core::model::comic_name_info::ComicNameInfo;
use cr_core::model::enums::YesNo;
use cr_core::xml::scalar::CrDateTime;

use super::book_view;
use super::text_number::number_range;

/// One aggregate value of `ComicBookSeriesStatistics`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatKind {
    Count,
    PageCount,
    PageReadCount,
    ReadPercentage,
    AverageRating,
    AverageCommunityRating,
    FirstNumber,
    LastNumber,
    FirstYear,
    LastYear,
    RunningTimeYears,
    MaxGapSize,
    GapCount,
    /// `YesNo` via `IsGapStart(book)`.
    GapStart,
    /// `YesNo` via `IsGapEnd(book)`.
    GapEnd,
    AllComplete,
    LastOpenedTime,
    LastAddedTime,
    LastPublishedTime,
    LastReleasedTime,
    MaxCount,
    MinCount,
}

/// `ComicBookSeriesStatistics.Key`: (ShadowSeries, ShadowVolume).
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub struct SeriesKey {
    pub series: String,
    pub volume: i32,
}

impl SeriesKey {
    pub fn of(book: &ComicBook, prop: &ComicNameInfo) -> Self {
        SeriesKey {
            series: book_view::shadow_series(book, prop).to_string(),
            volume: book_view::shadow_volume(book, prop),
        }
    }
}

/// Aggregates for one series (subset of `ComicBookSeriesStatistics` the
/// matchers read).
#[derive(Clone, Debug)]
pub struct SeriesStatistics {
    pub count: i32,
    pub page_count: i32,
    pub page_read_count: i32,
    pub read_percentage: i32,
    pub average_rating: f32,
    pub average_community_rating: f32,
    pub first_number: f32,
    pub last_number: f32,
    pub first_year: i32,
    pub last_year: i32,
    pub running_time_years: i32,
    pub gap_count: i32,
    pub max_gap_size: i32,
    /// (start, end) pairs with `end - start > 1` — `RangeF` gaps.
    pub gaps: Vec<(f32, f32)>,
    pub all_complete: YesNo,
    pub last_opened_time: CrDateTime,
    pub last_added_time: CrDateTime,
    pub last_published_time: CrDateTime,
    pub last_released_time: CrDateTime,
    pub max_count: i32,
    pub min_count: i32,
}

/// `ComicBookSeriesStatistics.GetSafeNumber`:
/// `CompareNumber.IsNumber ? Number : -1`.
fn safe_number(book: &ComicBook, prop: &ComicNameInfo) -> f32 {
    let (is_num, n) = book_view::compare_number(book, prop);
    if is_num {
        n
    } else {
        -1.0
    }
}

/// `ComicBookSeriesStatistics.GetGaps`: the sorted positive numbers
/// (book numbers plus expanded ranges); consecutive differences > 1 are
/// gaps. The gap range start is the previous number, the "end" is the
/// difference (`RangeF(num, num2)` where num2 = i - num is the LENGTH,
/// faithfully mirrored — `IsGapStart/End` compare against these).
pub fn gaps(books: &[&ComicBook], props: &dyn Fn(&ComicBook) -> ComicNameInfo) -> Vec<(f32, f32)> {
    let mut numbers: Vec<f32> = Vec::new();
    for b in books {
        let prop = props(b);
        let n = safe_number(b, &prop);
        if n > 0.0 {
            numbers.push(n);
        }
        // GetRangeNumber: the expanded "1-3" range minus its first value.
        let range = number_range(book_view::shadow_number(b, &prop));
        for r in range.into_iter().skip(1) {
            if r > 0.0 {
                numbers.push(r);
            }
        }
    }
    numbers.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mut out = Vec::new();
    let mut it = numbers.iter().copied();
    let Some(mut prev) = it.next() else {
        return out;
    };
    for i in it {
        let diff = i - prev;
        if diff > 1.0 {
            out.push((prev, diff));
        }
        prev = i;
    }
    out
}

/// `ComicBookSeriesStatistics.SumComplete`.
fn sum_complete(books: &[&ComicBook]) -> YesNo {
    let mut first = true;
    let mut result = YesNo::Unknown;
    for book in books {
        if first {
            first = false;
            result = book.series_complete;
        } else if result != book.series_complete {
            return YesNo::Unknown;
        }
    }
    result
}

/// Max over a date iterator (CrDateTime has no Ord; the payload does).
fn max_date<I: Iterator<Item = CrDateTime>>(dates: I) -> CrDateTime {
    let mut best = CrDateTime::min_value();
    for d in dates {
        if d.naive > best.naive {
            best = d;
        }
    }
    best
}

/// `ComicBookSeriesStatistics.Create`: group by [`SeriesKey`] and
/// aggregate.
pub fn create(
    books: &[&ComicBook],
    props: &dyn Fn(&ComicBook) -> ComicNameInfo,
) -> HashMap<SeriesKey, SeriesStatistics> {
    let mut groups: HashMap<SeriesKey, Vec<&ComicBook>> = HashMap::new();
    for b in books {
        let key = SeriesKey::of(b, &props(b));
        groups.entry(key).or_default().push(b);
    }
    groups
        .into_iter()
        .map(|(k, group)| (k, statistics(&group, props)))
        .collect()
}

fn statistics(
    books: &[&ComicBook],
    props: &dyn Fn(&ComicBook) -> ComicNameInfo,
) -> SeriesStatistics {
    let count = books.len() as i32;
    let page_count: i32 = books.iter().map(|b| b.info.page_count).sum();
    let page_read_count: i32 = books.iter().map(|b| b.last_page_read).sum();
    let read_count = books.iter().filter(|b| book_view::has_been_read(b)).count() as i32;
    let read_percentage = if count != 0 {
        read_count * 100 / count
    } else {
        0
    };
    let rating_count = books.iter().filter(|b| b.rating > 0.0).count() as i32;
    let community_count = books
        .iter()
        .filter(|b| b.info.community_rating > 0.0)
        .count() as i32;
    let average_rating = if rating_count != 0 {
        books
            .iter()
            .filter(|b| b.rating > 0.0)
            .map(|b| b.rating)
            .sum::<f32>()
            / rating_count as f32
    } else {
        0.0
    };
    let average_community_rating = if community_count != 0 {
        books
            .iter()
            .filter(|b| b.info.community_rating > 0.0)
            .map(|b| b.info.community_rating)
            .sum::<f32>()
            / community_count as f32
    } else {
        0.0
    };
    let numbers: Vec<f32> = books.iter().map(|b| safe_number(b, &props(b))).collect();
    let first_number = numbers.iter().copied().fold(f32::INFINITY, f32::min);
    let last_number = numbers.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let years: Vec<i32> = books
        .iter()
        .map(|b| book_view::shadow_year(b, &props(b)))
        .collect();
    let first_year = *years.iter().min().unwrap_or(&0);
    let last_year = *years.iter().max().unwrap_or(&0);
    let counts: Vec<i32> = books
        .iter()
        .map(|b| book_view::shadow_count(b, &props(b)))
        .collect();
    let gap_list = gaps(books, props);
    let read_percentage_agg = read_percentage;
    SeriesStatistics {
        count,
        page_count,
        page_read_count,
        read_percentage: read_percentage_agg,
        average_rating,
        average_community_rating,
        first_number: if numbers.is_empty() {
            0.0
        } else {
            first_number
        },
        last_number: if numbers.is_empty() { 0.0 } else { last_number },
        first_year,
        last_year,
        running_time_years: if last_year >= 0 {
            last_year - first_year
        } else {
            0
        },
        gap_count: gap_list.len() as i32,
        // C#: `Gaps.Max(g => (int)g.Length)` where `Length = End - Start`
        // and End is the gap DIFFERENCE (see `gaps`) — the net value is
        // `difference - previous_number`.
        max_gap_size: gap_list
            .iter()
            .map(|(start, end)| (*end - *start) as i32)
            .max()
            .unwrap_or(0),
        gaps: gap_list,
        all_complete: sum_complete(books),
        last_opened_time: max_date(books.iter().map(|b| b.opened_time)),
        last_added_time: max_date(books.iter().map(|b| b.added_time)),
        last_published_time: max_date(books.iter().map(|b| book_view::published(b, &props(b)))),
        last_released_time: max_date(books.iter().map(|b| b.released_time)),
        max_count: *counts.iter().max().unwrap_or(&0),
        min_count: *counts.iter().min().unwrap_or(&0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prop_of(_b: &ComicBook) -> ComicNameInfo {
        // Deterministic in tests: no file name parse needed for direct
        // field access when EnableProposed is off.
        ComicNameInfo::new()
    }

    #[test]
    fn series_grouping_and_aggregates() {
        let mut b1 = ComicBook::default();
        b1.info.series = "Batman".into();
        b1.info.volume = 1;
        b1.info.year = 2000;
        b1.info.number = "1".into();
        b1.info.page_count = 10;
        b1.enable_proposed = false;
        let mut b2 = ComicBook::default();
        b2.info.series = "Batman".into();
        b2.info.volume = 1;
        b2.info.year = 2001;
        b2.info.number = "3".into();
        b2.info.page_count = 20;
        b2.last_page_read = 19;
        b2.enable_proposed = false;
        let books = vec![&b1, &b2];
        let stats = create(&books, &prop_of);
        let s = stats
            .get(&SeriesKey {
                series: "Batman".into(),
                volume: 1,
            })
            .unwrap();
        assert_eq!(s.count, 2);
        assert_eq!(s.page_count, 30);
        assert_eq!(s.first_year, 2000);
        assert_eq!(s.last_year, 2001);
        assert_eq!(s.running_time_years, 1);
        assert_eq!(s.first_number, 1.0);
        assert_eq!(s.last_number, 3.0);
        // Gap between 1 and 3: RangeF(1, 2) — start 1, "end" (the
        // difference) 2, Length 1 (C# semantics, see `statistics`).
        assert_eq!(s.gap_count, 1);
        assert_eq!(s.max_gap_size, 1);
        // Read: only b2 has been read → 50%.
        assert_eq!(s.read_percentage, 50);
    }

    #[test]
    fn gap_start_and_end() {
        let mut b1 = ComicBook::default();
        b1.info.number = "1".into();
        b1.enable_proposed = false;
        let mut b2 = ComicBook::default();
        b2.info.number = "4".into();
        b2.enable_proposed = false;
        let books = vec![&b1, &b2];
        // Numbers 1 and 4: RangeF(1, 3).
        let gap_list = gaps(&books, &prop_of);
        assert_eq!(gap_list, vec![(1.0, 3.0)]);
        // IsGapStart(b1): start 1 == 1 → yes. IsGapEnd compares the
        // difference (3) against the book number — C# behavior.
        assert!(gap_list.iter().any(|(start, _)| *start == 1.0));
        assert!(gap_list.iter().any(|(_, end)| *end == 3.0));
    }
}
