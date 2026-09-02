//! Matcher evaluation: filter book sets through a matcher tree with the
//! C# semantics.
//!
//! The pipeline mirrors `MatcherSet<T>.Match` (cYo.Common): matchers are
//! applied in order, `And` filters, `Or` appends, and the per-matcher
//! `Not` flag removes instead of keeps. Group matchers evaluate their
//! children (their own `Not` is applied by the parent set, exactly like
//! `ComicBookGroupMatcher.Match`).

use std::collections::HashMap;
use std::marker::PhantomData;

use cr_core::model::comic_book::ComicBook;
use cr_core::model::enums::{MangaYesNo, MatcherMode, YesNo};
use cr_core::registry::{self, PropValue};
use cr_core::xml::scalar::CrDateTime;

use super::book_view;
use super::series::{self, SeriesKey, SeriesStatistics};
use super::spec::{self, ops};
use super::text_number;
use super::tree::{Matcher, ValueMatcher};

/// Evaluation context: the proposed-name parse per book and the series
/// statistics of the full book set (`IComicBookStatsProvider`).
pub struct MatchContext<'a> {
    /// "Now" for the date matcher's `is in last days` value conversion.
    pub now: CrDateTime,
    props: HashMap<*const ComicBook, cr_core::model::comic_name_info::ComicNameInfo>,
    stats: HashMap<SeriesKey, SeriesStatistics>,
    _marker: PhantomData<&'a ()>,
}

impl<'a> MatchContext<'a> {
    /// Builds the context for a book set (the full library — the series
    /// statistics are computed over it, not over the filtered subset).
    pub fn new(books: &[&'a ComicBook]) -> Self {
        let props: HashMap<_, _> = books
            .iter()
            .map(|b| ((*b) as *const ComicBook, book_view::proposed(b)))
            .collect();
        // The statistics reuse the parsed proposed values (the parse is
        // expensive; each book appears in one series group).
        let stats = series::create(books, &|b| {
            props
                .get(&(b as *const ComicBook))
                .cloned()
                .unwrap_or_else(|| book_view::proposed(b))
        });
        let now = chrono::Local::now().naive_local();
        MatchContext {
            now: CrDateTime {
                naive: now,
                kind: cr_core::xml::scalar::DateKind::Unspecified,
            },
            props,
            stats,
            _marker: PhantomData,
        }
    }

    fn prop(&self, book: &ComicBook) -> &cr_core::model::comic_name_info::ComicNameInfo {
        self.props
            .get(&(book as *const ComicBook))
            .expect("context built for this book set")
    }

    fn stats_for(&self, book: &ComicBook) -> Option<&SeriesStatistics> {
        self.stats.get(&SeriesKey::of(book, self.prop(book)))
    }
}

/// Filters `items` through `matchers` (mode + matcher pairs) with the
/// `MatcherSet` pipeline. Returns the surviving books in pipeline order.
pub fn match_set<'a>(
    items: &[&'a ComicBook],
    matchers: &[(MatcherMode, bool, &Matcher)],
    ctx: &MatchContext<'a>,
) -> Vec<&'a ComicBook> {
    let mut result: Option<Vec<&ComicBook>> = None;
    for (mode, not, matcher) in matchers {
        match mode {
            MatcherMode::And => {
                let src: Vec<&ComicBook> = result.unwrap_or_else(|| items.to_vec());
                let matched = match_one(matcher, &src, ctx);
                result = Some(if *not {
                    except(&src, &matched)
                } else {
                    matched
                });
            }
            MatcherMode::Or => {
                let candidate: Vec<&ComicBook> = match &result {
                    None => items.to_vec(),
                    Some(r) => except(items, r),
                };
                let mut matched = match_one(matcher, &candidate, ctx);
                if *not {
                    matched = except(&candidate, &matched);
                }
                result = Some(match result {
                    None => matched,
                    Some(r) => r.into_iter().chain(matched).collect(),
                });
            }
        }
    }
    result.unwrap_or_default()
}

/// `Enumerable.Except(first, second)`: keeps `first` order, removes
/// everything present in `second` (reference identity, like the C#
/// ComicBook class equality).
fn except<'a>(first: &[&'a ComicBook], second: &[&ComicBook]) -> Vec<&'a ComicBook> {
    let removed: Vec<*const ComicBook> = second.iter().map(|b| (*b) as *const ComicBook).collect();
    first
        .iter()
        .copied()
        .filter(|b| !removed.contains(&(*b as *const ComicBook)))
        .collect()
}

/// Evaluates one matcher against a set (per-item filter, recursive group
/// pipeline, or the set-based duplicate detection).
fn match_one<'a>(
    matcher: &Matcher,
    items: &[&'a ComicBook],
    ctx: &MatchContext<'a>,
) -> Vec<&'a ComicBook> {
    match matcher {
        Matcher::Group(g) => {
            if g.matchers.is_empty() {
                // ComicBookGroupMatcher.Match: empty group passes all.
                return items.to_vec();
            }
            let pairs: Vec<(MatcherMode, bool, &Matcher)> = g
                .matchers
                .iter()
                .map(|m| (g.matcher_mode, m.not(), m))
                .collect();
            match_set(items, &pairs, ctx)
        }
        Matcher::Value(v) => {
            if v.spec.kind == spec::MatcherKind::Duplicate {
                match_duplicates(items, v.op == 0)
            } else {
                items
                    .iter()
                    .copied()
                    .filter(|b| match_value(b, v, ctx))
                    .collect()
            }
        }
    }
}

impl Matcher {
    /// `ComicBookMatcher.Not`.
    pub fn not(&self) -> bool {
        match self {
            Matcher::Group(g) => g.not,
            Matcher::Value(v) => v.not,
        }
    }
}

/// Convenience: does one book match the tree? (Groups evaluate with the
/// book as the whole set; `Not` of the ROOT is applied here — the C#
/// per-item `Match(bool)` paths used by the UI treat it the same way.)
pub fn matches(book: &ComicBook, matcher: &Matcher, ctx: &MatchContext<'_>) -> bool {
    let result = match matcher {
        Matcher::Group(g) => {
            let pairs: Vec<(MatcherMode, bool, &Matcher)> = g
                .matchers
                .iter()
                .map(|m| (g.matcher_mode, m.not(), m))
                .collect();
            let hit = !match_set(std::slice::from_ref(&book), &pairs, ctx).is_empty();
            if g.not {
                !hit
            } else {
                hit
            }
        }
        Matcher::Value(v) => {
            let hit = match_value(book, v, ctx);
            if v.not {
                !hit
            } else {
                hit
            }
        }
    };
    result
}

// ---------- per-item value matcher evaluation ----------

fn match_value(book: &ComicBook, vm: &ValueMatcher, ctx: &MatchContext<'_>) -> bool {
    match vm.spec.kind {
        spec::MatcherKind::String => {
            let value = string_column(book, vm, ctx);
            let compare = compare_text(book, vm);
            match_string(vm, value, compare)
        }
        spec::MatcherKind::AllProperties => match_all_properties_value(book, vm, ctx),
        spec::MatcherKind::CustomValues => {
            // MatchColumn = 1: the book value is the custom value of
            // the key stored in MatchValue; the comparison text is
            // MatchValue2.
            let value = book_view::custom_value(book, &vm.value).unwrap_or_default();
            match_string(vm, value, vm.value2.clone())
        }
        spec::MatcherKind::Numeric => match_numeric(book, vm, ctx, numeric_value(book, vm, ctx)),
        spec::MatcherKind::Date => match_date(book, vm, ctx),
        spec::MatcherKind::YesNo => {
            let value = yesno_value(book, vm, ctx);
            match_yesno(vm, value)
        }
        spec::MatcherKind::MangaYesNo => match_manga(vm, book_view::manga_yesno(book)),
        spec::MatcherKind::Duplicate => true, // set-based, see match_one
        spec::MatcherKind::Series(stat) => match_series(book, vm, ctx, stat),
    }
}

fn match_all_properties_value(book: &ComicBook, vm: &ValueMatcher, ctx: &MatchContext<'_>) -> bool {
    let option = vm.option.as_deref().unwrap_or("All");
    let prop = ctx.prop(book);
    let values: Vec<String> = match option {
        "Series" => vec![
            book_view::shadow_series(book, prop).to_string(),
            book.info.alternate_series.clone(),
            book_view::shadow_format(book, prop).to_string(),
            book.info.series_group.clone(),
            book.info.story_arc.clone(),
        ],
        "Writer" => vec![book.info.writer.clone()],
        "Artists" => vec![
            book.info.writer.clone(),
            book.info.penciller.clone(),
            book.info.inker.clone(),
            book.info.colorist.clone(),
            book.info.editor.clone(),
            book.info.translator.clone(),
            book.info.letterer.clone(),
            book.info.cover_artist.clone(),
        ],
        "File" => vec![book.file_path.clone()],
        "Descriptive" => vec![
            book.info.notes.clone(),
            book.info.summary.clone(),
            book.info.review.clone(),
            book.info.tags.clone(),
            book.info.characters.clone(),
            book.info.teams.clone(),
            book.info.main_character_or_team.clone(),
            book.info.locations.clone(),
            book.info.scan_information.clone(),
        ],
        "Catalog" => vec![
            book.book_age.clone(),
            book.book_collection_status.clone(),
            book.book_notes.clone(),
            book.book_owner.clone(),
            book.book_store.clone(),
            book.book_location.clone(),
            book.isbn.clone(),
        ],
        _ => {
            // All: every string property, then the custom values.
            let mut v = vec![
                book_view::shadow_series(book, prop).to_string(),
                book.info.alternate_series.clone(),
                book_view::shadow_title(book, prop).to_string(),
                book.info.series_group.clone(),
                book.info.story_arc.clone(),
                book.info.writer.clone(),
                book.info.penciller.clone(),
                book.info.inker.clone(),
                book.info.colorist.clone(),
                book.info.letterer.clone(),
                book.info.editor.clone(),
                book.info.translator.clone(),
                book.info.cover_artist.clone(),
                book.info.summary.clone(),
                book.file_path.clone(),
                book.info.genre.clone(),
                book.info.notes.clone(),
                book.info.review.clone(),
                book.info.publisher.clone(),
                book.info.imprint.clone(),
                book.info.volume.to_string(),
                book.info.number.clone(),
                book.info.alternate_number.clone(),
                book.info.year.to_string(),
                book_view::shadow_format(book, prop).to_string(),
                book.info.age_rating.clone(),
                book.info.tags.clone(),
                book.info.characters.clone(),
                book.info.teams.clone(),
                book.info.main_character_or_team.clone(),
                book.info.locations.clone(),
                book.book_age.clone(),
                book.book_collection_status.clone(),
                book.book_notes.clone(),
                book.book_owner.clone(),
                book.book_store.clone(),
                book.book_location.clone(),
                book.isbn.clone(),
                book.info.scan_information.clone(),
            ];
            for (_, value) in book_view::custom_values(book) {
                v.push(value);
            }
            v
        }
    };
    let compare = compare_text(book, vm);
    values
        .into_iter()
        .any(|t| match_string(vm, t, compare.clone()))
}

/// `ComicBookStringMatcher.MatchBook`: `value` is the book-side text,
/// `compare` the match-side text (`MatchValue`, or `MatchValue2` for
/// the custom-value matcher).
fn match_string(vm: &ValueMatcher, value: String, compare: String) -> bool {
    let mv = &compare;
    let ignore_case = vm.ignore_case;
    let eq = |a: &str, b: &str| {
        if ignore_case {
            a.eq_ignore_ascii_case(b)
        } else {
            a == b
        }
    };
    let contains = |haystack: &str, needle: &str| {
        if ignore_case {
            fold_contains(haystack, needle)
        } else {
            haystack.contains(needle)
        }
    };
    match vm.op {
        ops::STR_EQUALS => eq(&value, mv),
        ops::STR_CONTAINS => {
            if !mv.is_empty() {
                contains(&value, mv)
            } else {
                true
            }
        }
        ops::STR_CONTAINS_ANY => {
            let parts = split_match_values(mv);
            if !parts.is_empty() {
                parts.iter().any(|s| contains(&value, s))
            } else {
                true
            }
        }
        ops::STR_CONTAINS_ALL => {
            if !value.is_empty() {
                split_match_values(mv).iter().all(|s| contains(&value, s))
            } else {
                false
            }
        }
        ops::STR_STARTS_WITH => {
            if !mv.is_empty() {
                if ignore_case {
                    value.get(..mv.len()).is_some_and(|prefix| eq(prefix, mv))
                } else {
                    value.starts_with(mv.as_str())
                }
            } else {
                true
            }
        }
        ops::STR_ENDS_WITH => {
            if !mv.is_empty() {
                if ignore_case {
                    value
                        .get(value.len().saturating_sub(mv.len())..)
                        .is_some_and(|suffix| eq(suffix, mv))
                } else {
                    value.ends_with(mv.as_str())
                }
            } else {
                true
            }
        }
        ops::STR_LIST_CONTAINS => {
            // The C# regex: the value, split on ,/;, contains the match
            // value as a member (trimmed, case-insensitive).
            let mv_trim = mv.trim();
            value
                .split([',', ';'])
                .any(|member| member.trim().eq_ignore_ascii_case(mv_trim))
        }
        ops::STR_REGEX => regex_match(&value, mv),
        _ => false,
    }
}

fn fold_contains(haystack: &str, needle: &str) -> bool {
    let h = haystack.to_lowercase();
    let n = needle.to_lowercase();
    h.contains(&n)
}

/// `OnMatchValueChanged` split: `[,;]` always; whitespace runs only when
/// the text has no `,`/`;` at all. Trimmed, empties dropped.
fn split_match_values(value: &str) -> Vec<String> {
    let has_sep = value.contains(',') || value.contains(';');
    let raw: Vec<&str> = if has_sep {
        value.split([',', ';']).collect()
    } else {
        value.split_whitespace().collect()
    };
    raw.iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

fn regex_match(value: &str, pattern: &str) -> bool {
    match fancy_regex::Regex::new(pattern) {
        Ok(rx) => rx.is_match(value).unwrap_or(false),
        Err(_) => false, // C#: invalid regex → rxMatch null → false
    }
}

/// The string matcher's book-side column value. For string matchers the
/// column IS the property the concrete class reads; the generic engine
/// matcher tree carries the property access in the spec binding, so the
/// caller resolves it via [`property_string_value`] before calling.
fn string_column(book: &ComicBook, vm: &ValueMatcher, ctx: &MatchContext<'_>) -> String {
    if let Some(prop) = field_expression(&vm.value) {
        return property_as_string(book, &prop);
    }
    let _ = ctx;
    property_string_value(book, vm, ctx)
}

/// The match-side text: `MatchValue` (with `{property}` substitution).
fn compare_text(book: &ComicBook, vm: &ValueMatcher) -> String {
    if let Some(prop) = field_expression(&vm.value) {
        return property_as_string(book, &prop);
    }
    vm.value.clone()
}

/// Resolves the concrete string matcher's property for the book. The
/// per-class `GetValue` implementations mapped to class names.
fn property_string_value(book: &ComicBook, vm: &ValueMatcher, ctx: &MatchContext<'_>) -> String {
    let prop = ctx.prop(book);
    match vm.spec.class_name {
        "ComicBookAgeRatingMatcher" => book.info.age_rating.clone(),
        "ComicBookAlternateSeriesMatcher" => book.info.alternate_series.clone(),
        "ComicBookBookAgeMatcher" => book.book_age.clone(),
        "ComicBookBookCollectionStatusMatcher" => book.book_collection_status.clone(),
        "ComicBookBookConditionMatcher" => book.book_condition.clone(),
        "ComicBookBookLocationMatcher" => book.book_location.clone(),
        "ComicBookBookNotesMatcher" => book.book_notes.clone(),
        "ComicBookBookOwnerMatcher" => book.book_owner.clone(),
        "ComicBookBookStoreMatcher" => book.book_store.clone(),
        "ComicBookCharactersMatcher" => book.info.characters.clone(),
        "ComicBookColoristMatcher" => book.info.colorist.clone(),
        "ComicBookEditorMatcher" => book.info.editor.clone(),
        "ComicBookFileFormatMatcher" => {
            cr_io::formats::source_format(std::path::Path::new(&book.file_path))
                .map(|f| f.name.to_string())
                .unwrap_or_default()
        }
        "ComicBookFileMatcher" => {
            cr_core::model::comic_name_info::file_name_without_extension(&book.file_path)
        }
        "ComicBookDirectoryMatcher" => {
            cr_core::model::comic_name_info::directory_name(&book.file_path)
        }
        "ComicBookFullPathMatcher" => book.file_path.clone(),
        "ComicBookFormatMatcher" => book_view::shadow_format(book, prop).to_string(),
        "ComicBookGenreMatcher" => book.info.genre.clone(),
        "ComicBookImprintMatcher" => book.info.imprint.clone(),
        "ComicBookInkerMatcher" => book.info.inker.clone(),
        "ComicBookISBNMatcher" => book.isbn.clone(),
        "ComicBookLanguageMatcher" => book_view::language_as_text(book),
        "ComicBookLettererMatcher" => book.info.letterer.clone(),
        "ComicBookLocationsMatcher" => book.info.locations.clone(),
        "ComicBookMainCharacterOrTeamMatcher" => book.info.main_character_or_team.clone(),
        "ComicBookNotesMatcher" => book.info.notes.clone(),
        "ComicBookPencillerMatcher" => book.info.penciller.clone(),
        "ComicBookPublisherMatcher" => book.info.publisher.clone(),
        "ComicBookReviewMatcher" => book.info.review.clone(),
        "ComicBookScanInformationMatcher" => book.info.scan_information.clone(),
        "ComicBookSeriesMatcher" => book_view::shadow_series(book, prop).to_string(),
        "ComicBookSeriesGroupMatcher" => book.info.series_group.clone(),
        "ComicBookStoryArcMatcher" => book.info.story_arc.clone(),
        "ComicBookSummaryMatcher" => book.info.summary.clone(),
        "ComicBookTagsMatcher" => book.info.tags.clone(),
        "ComicBookTeamsMatcher" => book.info.teams.clone(),
        "ComicBookTitleMatcher" => book_view::shadow_title(book, prop).to_string(),
        "ComicBookTranslatorMatcher" => book.info.translator.clone(),
        "ComicBookWebMatcher" => book.info.web.clone(),
        "ComicBookWriterMatcher" => book.info.writer.clone(),
        _ => String::new(),
    }
}

/// `{name}` field expression at the start of a match value.
fn field_expression(value: &str) -> Option<String> {
    let open = value.find('{')?;
    let close = value[open..].find('}')? + open;
    let name = &value[open + 1..close];
    if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphabetic() || c == '_') {
        Some(name.to_string())
    } else {
        None
    }
}

fn property_as_string(book: &ComicBook, prop: &str) -> String {
    match registry::get(book, prop) {
        Some(PropValue::Str(s)) => s,
        Some(PropValue::Int(i)) => i.to_string(),
        Some(PropValue::Float(f)) => f.to_string(),
        Some(PropValue::Bool(b)) => b.to_string(),
        Some(PropValue::Guid(g)) => g.to_d_string(),
        Some(PropValue::Date(d)) => d.to_xml(),
        None => String::new(),
    }
}

// ---------- numeric ----------

/// The book-side value of a numeric matcher (`GetValue` overrides).
fn numeric_value(book: &ComicBook, vm: &ValueMatcher, ctx: &MatchContext<'_>) -> f32 {
    let prop = ctx.prop(book);
    match vm.spec.class_name {
        "ComicBookAlternateCountMatcher" => book.info.alternate_count as f32,
        "ComicBookAlternateNumberMatcher" => {
            text_number::parse_float_prefix(&book.info.alternate_number).unwrap_or(-1.0)
        }
        "ComicBookBookmarkCountMatcher" => {
            // BookmarkCount lives in the page infos; not in the model
            // yet — C# stores it on ComicBook.
            0.0
        }
        "ComicBookBookPriceMatcher" => book.book_price,
        "ComicBookCommunityRatingMatcher" => book.info.community_rating,
        "ComicBookCountMatcher" => book_view::shadow_count(book, prop) as f32,
        "ComicBookDayMatcher" => book.info.day as f32,
        "ComicBookFileSizeMatcher" => book.file_size as f32 / 1024.0 / 1024.0,
        "ComicBookMonthMatcher" => book.info.month as f32,
        "ComicBookNewPagesMatcher" => book.new_pages as f32,
        "ComicBookNumberMatcher" => {
            let (is_num, n) = book_view::compare_number(book, prop);
            if is_num {
                n
            } else {
                -1.0
            }
        }
        "ComicBookPageCountMatcher" => book.info.page_count as f32,
        "ComicBookRatingMatcher" => book.rating,
        "ComicBookReadPercentageMatcher" => book_view::read_percentage(book) as f32,
        "ComicBookVolumeMatcher" => book_view::shadow_volume(book, prop) as f32,
        "ComicBookWeekMatcher" => book_view::week(book, prop) as f32,
        "ComicBookYearMatcher" => book_view::shadow_year(book, prop) as f32,
        _ => -1.0,
    }
}

/// The comparison value: the raw text converted invariantly, or the
/// `{property}` field expression read from the book.
fn numeric_compare_value(book: &ComicBook, vm: &ValueMatcher, ctx: &MatchContext<'_>) -> f32 {
    if let Some(prop) = field_expression(&vm.value) {
        return match registry::get(book, &prop) {
            Some(PropValue::Int(i)) => i as f32,
            Some(PropValue::Float(f)) => f,
            _ => -1.0,
        };
    }
    let _ = ctx;
    vm.value.parse::<f32>().unwrap_or(-1.0)
}

fn match_numeric(book: &ComicBook, vm: &ValueMatcher, ctx: &MatchContext<'_>, value: f32) -> bool {
    let mv = numeric_compare_value(book, vm, ctx);
    match vm.op {
        ops::NUM_EQUAL => value == mv,
        ops::NUM_GREATER => value > mv,
        ops::NUM_LESSER => value < mv,
        ops::NUM_IN_RANGE => value >= mv && value <= vm.value2.parse::<f32>().unwrap_or(-1.0),
        _ => false,
    }
}

// ---------- date ----------

fn parse_date_value(input: &str, ctx: &MatchContext<'_>) -> CrDateTime {
    if input.is_empty() {
        return CrDateTime::min_value();
    }
    if let Ok(d) = CrDateTime::parse(input.trim()).or_else(|_| CrDateTime::parse_date(input)) {
        return d;
    }
    if let Ok(d) = chrono::NaiveDate::parse_from_str(input.trim(), "%m/%d/%Y") {
        return CrDateTime {
            naive: d.and_hms_opt(0, 0, 0).unwrap(),
            kind: cr_core::xml::scalar::DateKind::Unspecified,
        };
    }
    // An integer is "that many days ago".
    if let Ok(days) = input.trim().parse::<i64>() {
        use chrono::Duration;
        let naive = ctx.now.naive - Duration::days(days);
        return CrDateTime {
            naive,
            kind: cr_core::xml::scalar::DateKind::Unspecified,
        };
    }
    CrDateTime::min_value()
}

fn date_compare_value(book: &ComicBook, vm: &ValueMatcher, ctx: &MatchContext<'_>) -> CrDateTime {
    if let Some(prop) = field_expression(&vm.value) {
        if let Some(PropValue::Date(d)) = registry::get(book, &prop) {
            return d;
        }
    }
    parse_date_value(&vm.value, ctx)
}

fn match_date(book: &ComicBook, vm: &ValueMatcher, ctx: &MatchContext<'_>) -> bool {
    let value = date_value(book, vm, ctx);
    let mv = date_compare_value(book, vm, ctx);
    // IgnoreTime is always true (constructor default, [XmlIgnore]).
    let num = compare_date_ignore_time(&value, &mv);
    match vm.op {
        ops::DATE_EQUALS => num == 0,
        ops::DATE_IS_AFTER => num == 1,
        ops::DATE_IS_BEFORE => num == -1,
        ops::DATE_IS_IN_LAST_DAYS => num >= 0,
        ops::DATE_IS_IN_RANGE => {
            num >= 0 && compare_date_ignore_time(&value, &parse_date_value(&vm.value2, ctx)) <= 0
        }
        _ => false,
    }
}

/// `DateTime.CompareTo(other, ignoreTime: true)`: y/m/d sign compare.
fn compare_date_ignore_time(a: &CrDateTime, b: &CrDateTime) -> i32 {
    let (ad, bd) = (a.naive.date(), b.naive.date());
    match ad.cmp(&bd) {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Greater => 1,
        std::cmp::Ordering::Equal => 0,
    }
}

/// The book-side date value (`GetValue` overrides).
fn date_value(book: &ComicBook, vm: &ValueMatcher, ctx: &MatchContext<'_>) -> CrDateTime {
    let prop = ctx.prop(book);
    match vm.spec.class_name {
        "ComicBookAddedMatcher" => book.added_time,
        "ComicBookCreationMatcher" => book.file_creation_time,
        "ComicBookModifiedMatcher" => book.file_modified_time,
        "ComicBookOpenedMatcher" => book.opened_time,
        "ComicBookPublishedMatcher" => book_view::published(book, prop),
        "ComicBookReleasedMatcher" => book.released_time,
        _ => CrDateTime::min_value(),
    }
}

// ---------- yes/no ----------

fn yesno_value(book: &ComicBook, vm: &ValueMatcher, ctx: &MatchContext<'_>) -> YesNo {
    match vm.spec.class_name {
        "ComicBookBlackAndWhiteMatcher" => book_view::yesno_black_and_white(book),
        "ComicBookCheckedMatcher" => book_view::yesno_checked(book),
        "ComicBookHasCustomValuesMatcher" => book_view::yesno_has_custom_values(book),
        "ComicBookIsLinkedMatcher" => book_view::yesno_is_linked(book),
        "ComicBookIsMissingMatcher" => book_view::yesno_is_missing(book),
        "ComicBookModifiedInfoMatcher" => book_view::yesno_modified_info(book),
        "ComicBookModifiedLibraryInfoMatcher" => book_view::yesno_modified_library_info(book),
        "ComicBookSeriesCompleteMatcher" => book_view::yesno_series_complete(book),
        _ => {
            let _ = ctx;
            YesNo::Unknown
        }
    }
}

fn match_yesno(vm: &ValueMatcher, value: YesNo) -> bool {
    match vm.op {
        0 => value == YesNo::Yes,
        1 => value == YesNo::No,
        _ => value == YesNo::Unknown,
    }
}

fn match_manga(vm: &ValueMatcher, value: MangaYesNo) -> bool {
    match vm.op {
        0 => value == MangaYesNo::Yes,
        1 => value == MangaYesNo::YesAndRightToLeft,
        2 => value == MangaYesNo::No,
        _ => value == MangaYesNo::Unknown,
    }
}

// ---------- series statistics ----------

fn match_series(
    book: &ComicBook,
    vm: &ValueMatcher,
    ctx: &MatchContext<'_>,
    stat: super::series::StatKind,
) -> bool {
    use super::series::StatKind;
    let Some(stats) = ctx.stats_for(book) else {
        return false;
    };
    match stat {
        StatKind::AllComplete => match_yesno(vm, stats.all_complete),
        StatKind::GapStart => {
            let prop = ctx.prop(book);
            let (is_num, n) = book_view::compare_number(book, prop);
            let i = if is_num { n } else { -1.0 };
            stats.gaps.iter().any(|(start, _)| *start == i)
        }
        StatKind::GapEnd => {
            let prop = ctx.prop(book);
            let (is_num, n) = book_view::compare_number(book, prop);
            let i = if is_num { n } else { -1.0 };
            stats.gaps.iter().any(|(_, end)| *end == i)
        }
        StatKind::LastOpenedTime
        | StatKind::LastAddedTime
        | StatKind::LastPublishedTime
        | StatKind::LastReleasedTime => {
            let value = match stat {
                StatKind::LastOpenedTime => stats.last_opened_time,
                StatKind::LastAddedTime => stats.last_added_time,
                StatKind::LastPublishedTime => stats.last_published_time,
                _ => stats.last_released_time,
            };
            let mv = date_compare_value(book, vm, ctx);
            let num = compare_date_ignore_time(&value, &mv);
            match vm.op {
                ops::DATE_EQUALS => num == 0,
                ops::DATE_IS_AFTER => num == 1,
                ops::DATE_IS_BEFORE => num == -1,
                ops::DATE_IS_IN_LAST_DAYS => num >= 0,
                ops::DATE_IS_IN_RANGE => {
                    num >= 0
                        && compare_date_ignore_time(&value, &parse_date_value(&vm.value2, ctx)) <= 0
                }
                _ => false,
            }
        }
        _ => {
            let value = match stat {
                StatKind::Count => stats.count as f32,
                StatKind::PageCount => stats.page_count as f32,
                StatKind::PageReadCount => stats.page_read_count as f32,
                StatKind::ReadPercentage => stats.read_percentage as f32,
                StatKind::AverageRating => stats.average_rating,
                StatKind::AverageCommunityRating => stats.average_community_rating,
                StatKind::FirstNumber => stats.first_number,
                StatKind::LastNumber => stats.last_number,
                StatKind::FirstYear => stats.first_year as f32,
                StatKind::LastYear => stats.last_year as f32,
                StatKind::RunningTimeYears => stats.running_time_years as f32,
                StatKind::MaxGapSize => stats.max_gap_size as f32,
                StatKind::GapCount => stats.gap_count as f32,
                StatKind::MaxCount => stats.max_count as f32,
                StatKind::MinCount => stats.min_count as f32,
                _ => 0.0,
            };
            match_numeric(book, vm, ctx, value)
        }
    }
}

// ---------- duplicates ----------

/// `ComicBookDuplicateMatcher.Match`: books that are metadata duplicates
/// or file-path duplicates (op 0); op 1 passes everything through.
fn match_duplicates<'a>(items: &[&'a ComicBook], on: bool) -> Vec<&'a ComicBook> {
    if !on {
        return items.to_vec();
    }
    let n = items.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut Vec<usize>, i: usize) -> usize {
        if parent[i] == i {
            i
        } else {
            let root = find(parent, parent[i]);
            parent[i] = root;
            root
        }
    }
    fn union(parent: &mut Vec<usize>, a: usize, b: usize) {
        let ra = find(parent, a);
        let rb = find(parent, b);
        if ra != rb {
            parent[ra] = rb;
        }
    }
    // Metadata duplicates (with the C# ternary-chain quirk preserved,
    // see the comment below) and path duplicates.
    for i in 0..n {
        for j in (i + 1)..n {
            let (a, b) = (items[i], items[j]);
            let meta = {
                // NOTE: the C# source chains the year/month/day checks
                // in one ternary expression, which compiles to
                // `yearCond ? yearEq : (monthCond ? monthEq :
                // (dayCond ? dayEq : bwEq))` — when a year is present
                // on either side, only the year is compared.
                let year_cond = a_shadow_year(a) >= 0 || a_shadow_year(b) >= 0;
                let month_cond = a.info.month >= 0 || b.info.month >= 0;
                let day_cond = a.info.day >= 0 || b.info.day >= 0;
                compressed_name_eq(a, b)
                    && a_shadow_format(a) == a_shadow_format(b)
                    && a_shadow_volume(a) == a_shadow_volume(b)
                    && a_shadow_number(a) == a_shadow_number(b)
                    && a.info.language_iso == b.info.language_iso
                    && if year_cond {
                        a_shadow_year(a) == a_shadow_year(b)
                    } else if month_cond {
                        a.info.month == b.info.month
                    } else if day_cond {
                        a.info.day == b.info.day
                    } else {
                        a.info.black_and_white == b.info.black_and_white
                    }
            };
            let path = book_view::is_linked(a)
                && book_view::is_linked(b)
                && a.file_path.eq_ignore_ascii_case(&b.file_path);
            if meta || path {
                union(&mut parent, i, j);
            }
        }
    }
    // Keep books whose group has more than one member.
    let mut sizes: HashMap<usize, usize> = HashMap::new();
    for i in 0..n {
        *sizes.entry(find(&mut parent, i)).or_default() += 1;
    }
    items
        .iter()
        .enumerate()
        .filter(|(i, _)| sizes[&find(&mut parent, *i)] > 1)
        .map(|(_, b)| *b)
        .collect()
}

// Shadow helpers taking the default proposed parse (the duplicate
// comparer reads Shadow* with EnableProposed as stored).
fn a_shadow_year(b: &ComicBook) -> i32 {
    book_view::shadow_year(b, &book_view::proposed(b))
}
fn a_shadow_volume(b: &ComicBook) -> i32 {
    book_view::shadow_volume(b, &book_view::proposed(b))
}
fn a_shadow_format(b: &ComicBook) -> String {
    book_view::shadow_format(b, &book_view::proposed(b)).to_string()
}
fn a_shadow_number(b: &ComicBook) -> String {
    book_view::shadow_number(b, &book_view::proposed(b)).to_string()
}

/// `GroupInfo.CompressedName`: drop separator-delimited articles,
/// concatenate. The C# uses the (ini-configured) article list; the
/// default here is the list shipped in ComicRack.ini.
fn compressed_name_eq(a: &ComicBook, b: &ComicBook) -> bool {
    const SEPARATORS: [char; 15] = [
        ' ', '\t', '\n', '\r', '-', '~', ',', '.', ';', ':', '/', '\\', '\'', '\u{b4}', '`',
    ];
    const ARTICLES: [&str; 8] = ["the", "der", "die", "das", "le", "la", "les", "l'"];
    let compress = |text: &str| -> String {
        text.split(SEPARATORS)
            .filter(|t| !t.is_empty() && !ARTICLES.iter().any(|a| t.eq_ignore_ascii_case(a)))
            .collect()
    };
    compress(book_view::shadow_series(a, &book_view::proposed(a))).eq_ignore_ascii_case(&compress(
        book_view::shadow_series(b, &book_view::proposed(b)),
    ))
}
