//! Groupers — the grouping keys the browser uses (`SingleComicGrouper`
//! subclasses and the `GroupInfo` ladders). The *values* feed Phase 4's
//! ItemView; the ladders and sort keys are the C# spec.
//!
//! Ported shapes (English default captions; TR localization lands in
//! Phase 5):
//!
//! - name groups (`GetNameGroup`): plain text or the compressed-name
//!   group; empty → `Unspecified`.
//! - date groups (`GroupInfo.GetDateGroup`): Never/Today/.../Future
//!   ladder with the exact C# sort keys.
//! - count groups (`ItemGroupCount.GetNumberGroup`): 0-20 ... >1000
//!   buckets, negative → `Unspecified`.
//! - rating groups (`ComicBookGroupRatingBase.GetRatingGroup`):
//!   Not Rated ... 5 Stars with inverted sort keys.
//! - alphabet groups (`GroupInfo.GetAlphabetGroup`): 0-9, A..Z, Other.
//!
//! The `sorters()`/`groupers()` tables map C# column/property keys to
//! the implementations, registry-driven like the C# reflection names.

use std::cmp::Ordering;

use crate::matcher::book_view;
use crate::sort::{compare_series, guid_compare};
use cr_core::model::comic_book::ComicBook;
use cr_core::model::comic_name_info::ComicNameInfo;
use cr_core::registry::{self, PropValue};
use cr_core::xml::scalar::CrDateTime;

/// `GroupInfo`: caption plus sort key (bucket order).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupInfo {
    pub caption: String,
    pub sort_key: i32,
}

impl GroupInfo {
    pub fn new(caption: impl Into<String>, sort_key: i32) -> GroupInfo {
        GroupInfo {
            caption: caption.into(),
            sort_key,
        }
    }
}

/// `GroupInfo.Unspecified` caption.
pub const UNSPECIFIED: &str = "Unspecified";

/// The English default date captions (`GroupInfo.DateGroups`).
pub const DATE_GROUPS: [&str; 23] = [
    "Never",
    "Today",
    "Yesterday",
    "Two Days Ago",
    "Three Days Ago",
    "This Week",
    "Last Week",
    "Two Weeks Ago",
    "Three Weeks Ago",
    "This Month",
    "Last Month",
    "Two Months ago",
    "Three Months ago",
    "Four Months ago",
    "Five Months ago",
    "Six Months ago",
    "This Year",
    "Last Year",
    "Two Years ago",
    "Three Years ago",
    "Older than Three Years",
    "Next Year",
    "In the Future",
];

fn year_of(date: chrono::NaiveDateTime) -> i32 {
    use chrono::Datelike;
    date.year()
}

/// `GroupInfo.GetDateGroup(date, now)`: the exact C# ladder.
pub fn date_group(date: &CrDateTime, now: &CrDateTime) -> GroupInfo {
    let d = date.naive;
    let n = now.naive;
    let day_of_year = |date: chrono::NaiveDateTime| -> i64 {
        use chrono::Datelike;
        i64::from(date.ordinal()) + i64::from(date.year()) * 366
    };
    let month = |date: chrono::NaiveDateTime| -> i64 {
        use chrono::Datelike;
        i64::from(date.year()) * 12 + i64::from(date.month())
    };
    let (now_doy, date_doy) = (day_of_year(n), day_of_year(d));
    let (now_month, date_month) = (month(n), month(d));
    let never = |caption: &str| GroupInfo::new(caption, 1000);

    if date.is_min_value() {
        return never(DATE_GROUPS[0]);
    }
    if now_doy == date_doy {
        return GroupInfo::new(DATE_GROUPS[1], 0);
    }
    if now_doy - 1 == date_doy {
        return GroupInfo::new(DATE_GROUPS[2], 1);
    }
    if now_doy - 2 == date_doy {
        return GroupInfo::new(DATE_GROUPS[3], 2);
    }
    if now_doy - 3 == date_doy {
        return GroupInfo::new(DATE_GROUPS[4], 3);
    }
    if now_doy / 7 == date_doy / 7 {
        return GroupInfo::new(DATE_GROUPS[5], 10);
    }
    if now_doy / 7 - 1 == date_doy / 7 {
        return GroupInfo::new(DATE_GROUPS[6], 11);
    }
    if now_doy / 7 - 2 == date_doy / 7 {
        return GroupInfo::new(DATE_GROUPS[7], 12);
    }
    if now_doy / 7 - 3 == date_doy / 7 {
        return GroupInfo::new(DATE_GROUPS[8], 13);
    }
    if now_month == date_month {
        return GroupInfo::new(DATE_GROUPS[9], 20);
    }
    if now_month - 1 == date_month {
        return GroupInfo::new(DATE_GROUPS[10], 21);
    }
    if now_month - 2 == date_month {
        return GroupInfo::new(DATE_GROUPS[11], 22);
    }
    if now_month - 3 == date_month {
        return GroupInfo::new(DATE_GROUPS[12], 23);
    }
    if now_month - 4 == date_month {
        return GroupInfo::new(DATE_GROUPS[13], 24);
    }
    if now_month - 5 == date_month {
        return GroupInfo::new(DATE_GROUPS[14], 25);
    }
    if now_month - 6 == date_month {
        return GroupInfo::new(DATE_GROUPS[15], 26);
    }
    if year_of(n) == year_of(d) {
        return GroupInfo::new(DATE_GROUPS[16], 30);
    }
    if year_of(n) - 1 == year_of(d) {
        return GroupInfo::new(DATE_GROUPS[17], 31);
    }
    if year_of(n) - 2 == year_of(d) {
        return GroupInfo::new(DATE_GROUPS[18], 32);
    }
    if year_of(n) - 3 == year_of(d) {
        return GroupInfo::new(DATE_GROUPS[19], 33);
    }
    if year_of(n) + 1 == year_of(d) {
        return GroupInfo::new(DATE_GROUPS[21], 34);
    }
    if year_of(n) < year_of(d) {
        return GroupInfo::new(DATE_GROUPS[22], 35);
    }
    GroupInfo::new(DATE_GROUPS[20], 34) // Older than Three Years
}

/// The count bucket captions (`ItemGroupCount`).
pub const COUNT_GROUPS: [&str; 8] = [
    "0-20",
    "21-50",
    "51-100",
    "101-200",
    "201-500",
    "501-1000",
    ">1000",
    "Unspecified",
];

/// `ItemGroupCount.GetNumberGroup`: bucket index is the sort key.
pub fn count_group(n: i32) -> GroupInfo {
    let mut num = 0usize;
    if n < 0 {
        num = COUNT_GROUPS.len() - 1;
    } else {
        if n > 20 {
            num += 1;
        }
        if n > 50 {
            num += 1;
        }
        if n > 100 {
            num += 1;
        }
        if n > 200 {
            num += 1;
        }
        if n > 500 {
            num += 1;
        }
        if n > 1000 {
            num += 1;
        }
    }
    GroupInfo::new(COUNT_GROUPS[num], num as i32)
}

/// The rating captions (`ComicBookGroupRatingBase`).
pub const RATING_GROUPS: [&str; 7] = [
    "Not Rated",
    "No Stars",
    "1 Star",
    "2 Stars",
    "3 Stars",
    "4 Stars",
    "5 Stars",
];

/// `ComicBookGroupRatingBase.GetRatingGroup`: inverted sort keys
/// (Not Rated sorts last).
pub fn rating_group(rating: i32) -> GroupInfo {
    let num = (rating.clamp(-1, RATING_GROUPS.len() as i32 - 2) + 1) as usize;
    GroupInfo::new(RATING_GROUPS[num], (RATING_GROUPS.len() - num) as i32)
}

/// The alphabet captions (`GroupInfo.AlphabetGroups`).
pub const ALPHABET_GROUPS: [&str; 28] = [
    "0-9", "A", "B", "C", "D", "E", "F", "G", "H", "I", "J", "K", "L", "M", "N", "O", "P", "Q",
    "R", "S", "T", "U", "V", "W", "X", "Y", "Z", "Other",
];

/// `GroupInfo.GetAlphabetGroup(text)` — article-aware first letter.
pub fn alphabet_group(text: &str, article_aware: bool) -> GroupInfo {
    let mut num;
    if text.is_empty() {
        num = ALPHABET_GROUPS.len() - 1;
    } else {
        let start = if article_aware {
            // IndexAfterArticle (default article list).
            const ARTICLES: [&str; 8] =
                ["the ", "der ", "die ", "das ", "le ", "la ", "les ", "l'"];
            let lower = text.to_lowercase();
            ARTICLES
                .iter()
                .find(|a| lower.starts_with(**a))
                .map_or(0, |a| a.len())
        } else {
            0
        };
        let c = text[start..]
            .chars()
            .next()
            .unwrap_or('\u{0}')
            .to_uppercase()
            .next()
            .unwrap_or('\u{0}');
        if c.is_ascii_digit() {
            num = 0;
        } else {
            let idx = i32::from(c as u8).wrapping_sub(65).saturating_add(1);
            num = idx.clamp(1, ALPHABET_GROUPS.len() as i32 - 2) as usize;
            if !c.is_ascii_alphabetic() || (idx < 1 || idx > ALPHABET_GROUPS.len() as i32 - 2) {
                num = ALPHABET_GROUPS.len() - 1;
            }
        }
    }
    GroupInfo::new(ALPHABET_GROUPS[num], num as i32)
}

/// `SingleComicGrouper.GetNameGroup` (plain or compressed form).
pub fn name_group(text: &str, compress: bool) -> GroupInfo {
    if compress {
        const SEPARATORS: [char; 15] = [
            ' ', '\t', '\n', '\r', '-', '~', ',', '.', ';', ':', '/', '\\', '\'', '\u{b4}', '`',
        ];
        const ARTICLES: [&str; 8] = ["the", "der", "die", "das", "le", "la", "les", "l'"];
        let compressed: String = text
            .split(SEPARATORS)
            .filter(|t| !t.is_empty() && !ARTICLES.iter().any(|a| t.eq_ignore_ascii_case(a)))
            .collect();
        return GroupInfo::new(compressed.to_uppercase(), 0);
    }
    GroupInfo::new(
        if text.is_empty() {
            UNSPECIFIED.to_string()
        } else {
            text.to_string()
        },
        0,
    )
}

// ---------- registry tables ----------

fn prop(book: &ComicBook) -> ComicNameInfo {
    book_view::proposed(book)
}

/// Compare two books by a registry property (numbers numerically,
/// strings case-insensitive, dates by payload).
pub fn compare_by_key(a: &ComicBook, b: &ComicBook, key: &str) -> Ordering {
    match (registry::get(a, key), registry::get(b, key)) {
        (Some(PropValue::Int(x)), Some(PropValue::Int(y))) => x.cmp(&y),
        (Some(PropValue::Float(x)), Some(PropValue::Float(y))) => {
            x.partial_cmp(&y).unwrap_or(Ordering::Equal)
        }
        (Some(PropValue::Str(x)), Some(PropValue::Str(y))) => {
            crate::sort::compare_ignore_case(&x, &y)
        }
        (Some(PropValue::Date(x)), Some(PropValue::Date(y))) => x.naive.cmp(&y.naive),
        (Some(PropValue::Bool(x)), Some(PropValue::Bool(y))) => x.cmp(&y),
        _ => Ordering::Equal,
    }
}

/// A book grouper: maps a book to its group bucket.
pub type Grouper = fn(&ComicBook) -> GroupInfo;

fn group_series(book: &ComicBook) -> GroupInfo {
    name_group(book_view::shadow_series(book, &prop(book)), false)
}

fn group_title(book: &ComicBook) -> GroupInfo {
    name_group(book_view::shadow_title(book, &prop(book)), false)
}

fn group_writer(book: &ComicBook) -> GroupInfo {
    name_group(&book.info.writer, false)
}

fn group_year(book: &ComicBook) -> GroupInfo {
    let year = book_view::shadow_year(book, &prop(book));
    let text = if year > 0 {
        year.to_string()
    } else {
        String::new()
    };
    GroupInfo::new(
        if text.is_empty() {
            UNSPECIFIED.to_string()
        } else {
            text.to_string()
        },
        0,
    )
}

fn group_page_count(book: &ComicBook) -> GroupInfo {
    count_group(book.info.page_count)
}

fn group_checked(book: &ComicBook) -> GroupInfo {
    if book.checked {
        GroupInfo::new("Checked", 0)
    } else {
        GroupInfo::new("Not Checked", 1)
    }
}

fn group_rating(book: &ComicBook) -> GroupInfo {
    rating_group(book.rating.round() as i32)
}

fn group_community_rating(book: &ComicBook) -> GroupInfo {
    rating_group(book.info.community_rating.round() as i32)
}

fn date_grouper(pick: fn(&ComicBook) -> CrDateTime) -> impl Fn(&ComicBook) -> GroupInfo {
    move |book| {
        let now = CrDateTime {
            naive: chrono::Local::now().naive_local(),
            kind: cr_core::xml::scalar::DateKind::Unspecified,
        };
        date_group(&pick(book), &now)
    }
}

fn group_added(book: &ComicBook) -> GroupInfo {
    date_grouper(|b| b.added_time)(book)
}

fn group_opened(book: &ComicBook) -> GroupInfo {
    date_grouper(|b| b.opened_time)(book)
}

fn group_published(book: &ComicBook) -> GroupInfo {
    date_grouper(|b| {
        let p = prop(b);
        book_view::published(b, &p)
    })(book)
}

fn group_released(book: &ComicBook) -> GroupInfo {
    date_grouper(|b| b.released_time)(book)
}

fn group_black_and_white(book: &ComicBook) -> GroupInfo {
    match book.info.black_and_white {
        cr_core::model::enums::YesNo::Yes => GroupInfo::new("Yes", 0),
        cr_core::model::enums::YesNo::No => GroupInfo::new("No", 1),
        cr_core::model::enums::YesNo::Unknown => GroupInfo::new(UNSPECIFIED, 2),
    }
}

/// The grouper table: C# column key → grouping shape.
pub fn groupers() -> &'static [(&'static str, Grouper)] {
    static TABLE: std::sync::OnceLock<Vec<(&'static str, Grouper)>> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        vec![
            ("Series", group_series),
            ("Title", group_title),
            ("Writer", group_writer),
            ("Year", group_year),
            ("PageCount", group_page_count),
            ("Checked", group_checked),
            ("Rating", group_rating),
            ("CommunityRating", group_community_rating),
            ("AddedTime", group_added),
            ("OpenedTime", group_opened),
            ("ReleasedTime", group_released),
            ("Published", group_published),
            ("BlackAndWhite", group_black_and_white),
        ]
    })
}

/// The special-case sorters; everything else sorts through the
/// property registry via `compare_by_key`. `Series` uses the series
/// comparer (articles + number-aware), `Id` the Guid order.
pub fn compare_by_column(a: &ComicBook, b: &ComicBook, key: &str) -> Ordering {
    match key {
        "Series" => compare_series(a, b),
        "Id" => guid_compare(&a.id, &b.id),
        _ => compare_by_key(a, b, key),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn date(y: i32, m: u32, d: u32) -> CrDateTime {
        CrDateTime {
            naive: NaiveDate::from_ymd_opt(y, m, d)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
            kind: cr_core::xml::scalar::DateKind::Unspecified,
        }
    }

    #[test]
    fn date_group_ladder() {
        let now = date(2026, 9, 3);
        assert_eq!(date_group(&CrDateTime::min_value(), &now).caption, "Never");
        assert_eq!(date_group(&date(2026, 9, 3), &now).caption, "Today");
        assert_eq!(date_group(&date(2026, 9, 2), &now).caption, "Yesterday");
        assert_eq!(date_group(&date(2026, 9, 1), &now).caption, "Two Days Ago");
        assert_eq!(
            date_group(&date(2026, 8, 31), &now).caption,
            "Three Days Ago"
        );
        assert_eq!(date_group(&date(2026, 9, 20), &now).caption, "This Month");
        assert_eq!(date_group(&date(2026, 8, 27), &now).caption, "Last Week");
        // Same year, later month → still This Year; a later year is Next/Future.
        assert_eq!(date_group(&date(2026, 12, 20), &now).caption, "This Year");
        // Next Year precedes the Future check in the ladder — any date
        // in the next year is "Next Year".
        assert_eq!(date_group(&date(2027, 6, 1), &now).caption, "Next Year");
        assert_eq!(date_group(&date(2028, 6, 1), &now).caption, "In the Future");
        assert_eq!(
            date_group(&date(2022, 8, 20), &now).caption,
            "Older than Three Years"
        );
        assert_eq!(
            date_group(&date(2023, 8, 20), &now).caption,
            "Three Years ago"
        );
        assert_eq!(
            date_group(&date(2026, 7, 20), &now).caption,
            "Two Months ago"
        );
        // Sort keys rise with age.
        assert!(
            date_group(&date(2026, 9, 3), &now).sort_key
                < date_group(&date(2025, 9, 3), &now).sort_key
        );
    }

    #[test]
    fn count_group_buckets() {
        assert_eq!(count_group(0).caption, "0-20");
        assert_eq!(count_group(20).caption, "0-20");
        assert_eq!(count_group(21).caption, "21-50");
        assert_eq!(count_group(50).caption, "21-50");
        assert_eq!(count_group(101).caption, "101-200");
        assert_eq!(count_group(1001).caption, ">1000");
        assert_eq!(count_group(-1).caption, "Unspecified");
        // Sort key = bucket index.
        assert!(count_group(10).sort_key < count_group(30).sort_key);
        assert!(count_group(600).sort_key < count_group(2000).sort_key);
    }

    #[test]
    fn rating_group_inverted_keys() {
        // 0 rating = "No Stars"; -1 (unset) = "Not Rated".
        assert_eq!(rating_group(0).caption, "No Stars");
        assert_eq!(rating_group(-1).caption, "Not Rated");
        assert_eq!(rating_group(1).caption, "1 Star");
        assert_eq!(rating_group(5).caption, "5 Stars");
        assert_eq!(rating_group(9).caption, "5 Stars"); // clamped
                                                        // Not Rated sorts last.
        assert!(rating_group(0).sort_key > rating_group(5).sort_key);
    }

    #[test]
    fn alphabet_buckets() {
        assert_eq!(alphabet_group("batman", false).caption, "B");
        assert_eq!(alphabet_group("the batman", true).caption, "B");
        assert_eq!(alphabet_group("the batman", false).caption, "T");
        assert_eq!(alphabet_group("123", false).caption, "0-9");
        assert_eq!(alphabet_group("!foo", false).caption, "Other");
        assert_eq!(alphabet_group("", false).caption, "Other");
    }

    #[test]
    fn grouper_table_and_column_sort() {
        let mut a = ComicBook::default();
        a.info.series = "The Batman".into();
        a.info.number = "2".into();
        a.enable_proposed = false;
        let mut b = ComicBook::default();
        b.info.series = "batman".into();
        b.info.number = "10".into();
        b.enable_proposed = false;

        let series_grouper = groupers()
            .iter()
            .find(|(k, _)| *k == "Series")
            .map(|(_, f)| *f)
            .unwrap();
        assert_eq!(series_grouper(&a).caption, "The Batman");

        // Series column sort is number-aware when the series text is
        // equal (2 < 10 despite strings).
        b.info.series = a.info.series.clone();
        assert_eq!(compare_by_column(&a, &b, "Series"), Ordering::Less);
        assert_eq!(compare_by_column(&b, &a, "Series"), Ordering::Greater);
        // Different article prefixes compare by the article-skip
        // offsets (C# quirk): "The Batman" sorts after "Batman".
        b.info.series = "Batman".into();
        assert_eq!(compare_by_column(&a, &b, "Series"), Ordering::Greater);
        // Numeric registry compare.
        a.info.page_count = 10;
        b.info.page_count = 5;
        assert_eq!(compare_by_column(&a, &b, "PageCount"), Ordering::Greater);
    }
}
