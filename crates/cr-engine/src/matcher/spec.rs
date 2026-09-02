//! The matcher registry: every concrete `ComicBookValueMatcher` subclass
//! with its C# class name (`xsi:type` in ComicDb.xml), its neutral
//! (English) description (the `[Name]` token in query strings), and the
//! kind that decides the operator list and argument count.
//!
//! Lookup rules match the C#:
//!
//! - Query parsing (`ComicBookValueMatcher.Create(string)`) finds the
//!   first spec whose description equals the token, case-insensitively.
//! - XML matcher trees carry `xsi:type` = class name; binding uses the
//!   class name.
//!
//! Operator lists are the `*Neutral` (English) lists; localized operator
//! words are a Phase 5 concern (TR) — queries persist in English.

use super::series::StatKind;

/// Operator indices (C# constants).
pub mod ops {
    // ComicBookStringMatcher
    pub const STR_EQUALS: usize = 0;
    pub const STR_CONTAINS: usize = 1;
    pub const STR_CONTAINS_ANY: usize = 2;
    pub const STR_CONTAINS_ALL: usize = 3;
    pub const STR_STARTS_WITH: usize = 4;
    pub const STR_ENDS_WITH: usize = 5;
    pub const STR_LIST_CONTAINS: usize = 6;
    pub const STR_REGEX: usize = 7;
    // ComicBookNumericMatcher
    pub const NUM_EQUAL: usize = 0;
    pub const NUM_GREATER: usize = 1;
    pub const NUM_LESSER: usize = 2;
    pub const NUM_IN_RANGE: usize = 3;
    // ComicBookDateMatcher
    pub const DATE_EQUALS: usize = 0;
    pub const DATE_IS_AFTER: usize = 1;
    pub const DATE_IS_BEFORE: usize = 2;
    pub const DATE_IS_IN_LAST_DAYS: usize = 3;
    pub const DATE_IS_IN_RANGE: usize = 4;
    // ComicBookYesNoMatcher: 0 yes, 1 no, 2 unknown.
    // ComicBookMangaYesNoMatcher: 0 yes, 1 ltr, 2 no, 3 unknown.
    // ComicBookDuplicateMatcher: 0 on, 1 off.
}

pub const STRING_OPS: &[&str] = &[
    "equals",
    "contains",
    "contains any of",
    "contains all of",
    "starts with",
    "ends with",
    "list contains",
    "regex",
];

pub const NUMERIC_OPS: &[&str] = &["equals", "is greater", "is smaller", "in range"];

pub const DATE_OPS: &[&str] = &[
    "equals",
    "is after",
    "is before",
    "is in last days",
    "is in range",
];

pub const YESNO_OPS: &[&str] = &["equals yes", "equals no", "equals unknown"];

pub const MANGA_OPS: &[&str] = &["equals yes", "equals ltr", "equals no", "equals unknown"];

pub const ONOFF_OPS: &[&str] = &["on", "off"];

/// What a value matcher does at evaluation time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatcherKind {
    /// `ComicBookStringMatcher` subclass (1 argument).
    String,
    /// `ComicBookNumericMatcher` subclass (1 argument, 2 for `in range`).
    Numeric,
    /// `ComicBookDateMatcher` subclass (1 argument, 2 for `is in range`).
    Date,
    /// `ComicBookYesNoMatcher` subclass (0 arguments).
    YesNo,
    /// `ComicBookValueMatcher<MangaYesNo>` (0 arguments).
    MangaYesNo,
    /// `ComicBookDuplicateMatcher` (0 arguments, set-based match).
    Duplicate,
    /// `ComicBookAllPropertiesMatcher` (1 argument, wildcard search).
    AllProperties,
    /// `ComicBookCustomValuesMatcher` (2 arguments; value 1 is the key).
    CustomValues,
    /// `SmartListSeries*Matcher`: aggregate value over the book's series.
    Series(StatKind),
}

/// One registered matcher.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatcherSpec {
    /// C# class name = `xsi:type` in ComicDb.xml.
    pub class_name: &'static str,
    /// `DescriptionAttribute` = the bracket token in query strings.
    pub description: &'static str,
    pub kind: MatcherKind,
}

impl MatcherSpec {
    /// The neutral operator word list.
    pub fn operators(&self) -> &'static [&'static str] {
        match self.kind {
            MatcherKind::String | MatcherKind::AllProperties | MatcherKind::CustomValues => {
                STRING_OPS
            }
            MatcherKind::Numeric => NUMERIC_OPS,
            MatcherKind::Date => DATE_OPS,
            MatcherKind::YesNo => YESNO_OPS,
            MatcherKind::MangaYesNo => MANGA_OPS,
            MatcherKind::Duplicate => ONOFF_OPS,
            MatcherKind::Series(s) => match s {
                StatKind::AllComplete | StatKind::GapStart | StatKind::GapEnd => YESNO_OPS,
                StatKind::LastOpenedTime
                | StatKind::LastAddedTime
                | StatKind::LastPublishedTime
                | StatKind::LastReleasedTime => DATE_OPS,
                _ => NUMERIC_OPS,
            },
        }
    }

    /// `ArgumentCount` for the given operator index.
    pub fn argument_count(&self, op: usize) -> usize {
        match self.kind {
            MatcherKind::String | MatcherKind::AllProperties => 1,
            MatcherKind::CustomValues => 2,
            MatcherKind::Numeric => 1 + usize::from(op == ops::NUM_IN_RANGE),
            MatcherKind::Date => 1 + usize::from(op == ops::DATE_IS_IN_RANGE),
            MatcherKind::YesNo | MatcherKind::MangaYesNo | MatcherKind::Duplicate => 0,
            MatcherKind::Series(s) => match s {
                StatKind::AllComplete | StatKind::GapStart | StatKind::GapEnd => 0,
                StatKind::LastOpenedTime
                | StatKind::LastAddedTime
                | StatKind::LastPublishedTime
                | StatKind::LastReleasedTime => 1 + usize::from(op == ops::DATE_IS_IN_RANGE),
                _ => 1 + usize::from(op == ops::NUM_IN_RANGE),
            },
        }
    }
}

macro_rules! specs {
    ($($class:literal => $desc:literal : $kind:expr),+ $(,)?) => {
        &[$( MatcherSpec { class_name: $class, description: $desc, kind: $kind }, )+]
    };
}

/// All concrete matchers. Order is irrelevant for lookups (both are
/// exact-name based); rows are grouped like the C# sources.
pub fn all_specs() -> &'static [MatcherSpec] {
    use MatcherKind as K;
    use StatKind as S;
    specs!(
        // --- ComicRack.Engine/Metadata/ComicBook/Matcher/ (string) ---
        "ComicBookAgeRatingMatcher" => "Age Rating": K::String,
        "ComicBookAllPropertiesMatcher" => "All": K::AllProperties,
        "ComicBookAlternateSeriesMatcher" => "Alternate Series": K::String,
        "ComicBookBookAgeMatcher" => "Book Age": K::String,
        "ComicBookBookCollectionStatusMatcher" => "Book Collection Status": K::String,
        "ComicBookBookConditionMatcher" => "Book Condition": K::String,
        "ComicBookBookLocationMatcher" => "Book Location": K::String,
        "ComicBookBookNotesMatcher" => "Book Notes": K::String,
        "ComicBookBookOwnerMatcher" => "Book Owner": K::String,
        "ComicBookBookStoreMatcher" => "Book Store": K::String,
        "ComicBookCharactersMatcher" => "Characters": K::String,
        "ComicBookColoristMatcher" => "Colorist": K::String,
        "ComicBookCustomValuesMatcher" => "Custom Value": K::CustomValues,
        "ComicBookEditorMatcher" => "Editor": K::String,
        "ComicBookFileFormatMatcher" => "File Format": K::String,
        "ComicBookFileMatcher" => "File": K::String,
        "ComicBookDirectoryMatcher" => "File Directory": K::String,
        "ComicBookFullPathMatcher" => "File Path": K::String,
        "ComicBookFormatMatcher" => "Format": K::String,
        "ComicBookGenreMatcher" => "Genre": K::String,
        "ComicBookImprintMatcher" => "Imprint": K::String,
        "ComicBookInkerMatcher" => "Inker": K::String,
        "ComicBookISBNMatcher" => "ISBN": K::String,
        "ComicBookLanguageMatcher" => "Language": K::String,
        "ComicBookLettererMatcher" => "Letterer": K::String,
        "ComicBookLocationsMatcher" => "Locations": K::String,
        "ComicBookMainCharacterOrTeamMatcher" => "Main Character/Team": K::String,
        "ComicBookNotesMatcher" => "Notes": K::String,
        "ComicBookPencillerMatcher" => "Penciller": K::String,
        "ComicBookPublisherMatcher" => "Publisher": K::String,
        "ComicBookReviewMatcher" => "Review": K::String,
        "ComicBookScanInformationMatcher" => "Scanning Information": K::String,
        "ComicBookSeriesMatcher" => "Series": K::String,
        "ComicBookSeriesGroupMatcher" => "Series Group": K::String,
        "ComicBookStoryArcMatcher" => "Story Arc": K::String,
        "ComicBookSummaryMatcher" => "Summary": K::String,
        "ComicBookTagsMatcher" => "Tags": K::String,
        "ComicBookTeamsMatcher" => "Teams": K::String,
        "ComicBookTitleMatcher" => "Title": K::String,
        "ComicBookTranslatorMatcher" => "Translator": K::String,
        "ComicBookWebMatcher" => "Web": K::String,
        "ComicBookWriterMatcher" => "Writer": K::String,
        // --- numeric ---
        "ComicBookAlternateCountMatcher" => "Alternate Count": K::Numeric,
        "ComicBookAlternateNumberMatcher" => "Alternate Number": K::Numeric,
        "ComicBookBookmarkCountMatcher" => "Bookmark Count": K::Numeric,
        "ComicBookBookPriceMatcher" => "Book Price": K::Numeric,
        "ComicBookCommunityRatingMatcher" => "Community Rating": K::Numeric,
        "ComicBookCountMatcher" => "Count": K::Numeric,
        "ComicBookDayMatcher" => "Day": K::Numeric,
        "ComicBookFileSizeMatcher" => "File Size": K::Numeric,
        "ComicBookMonthMatcher" => "Month": K::Numeric,
        "ComicBookNewPagesMatcher" => "New Pages": K::Numeric,
        "ComicBookNumberMatcher" => "Number": K::Numeric,
        "ComicBookPageCountMatcher" => "Page Count": K::Numeric,
        "ComicBookRatingMatcher" => "My Rating": K::Numeric,
        "ComicBookReadPercentageMatcher" => "Read Percentage": K::Numeric,
        "ComicBookVolumeMatcher" => "Volume": K::Numeric,
        "ComicBookWeekMatcher" => "Week": K::Numeric,
        "ComicBookYearMatcher" => "Year": K::Numeric,
        // --- date ---
        "ComicBookAddedMatcher" => "Added": K::Date,
        "ComicBookCreationMatcher" => "File Created": K::Date,
        "ComicBookModifiedMatcher" => "File Modified": K::Date,
        "ComicBookOpenedMatcher" => "Opened": K::Date,
        "ComicBookPublishedMatcher" => "Published": K::Date,
        "ComicBookReleasedMatcher" => "Released": K::Date,
        // --- yes/no ---
        "ComicBookBlackAndWhiteMatcher" => "Black and White": K::YesNo,
        "ComicBookCheckedMatcher" => "Is Checked": K::YesNo,
        "ComicBookHasCustomValuesMatcher" => "Has Custom Values": K::YesNo,
        "ComicBookIsLinkedMatcher" => "Is Linked": K::YesNo,
        "ComicBookIsMissingMatcher" => "Is Missing": K::YesNo,
        "ComicBookModifiedInfoMatcher" => "Modified Info": K::YesNo,
        "ComicBookModifiedLibraryInfoMatcher" => "Modified Library Info": K::YesNo,
        "ComicBookSeriesCompleteMatcher" => "Series complete": K::YesNo,
        // --- manga (ComicRack.Engine top level) ---
        "ComicBookMangaMatcher" => "Manga": K::MangaYesNo,
        // --- duplicate ---
        "ComicBookDuplicateMatcher" => "Only Duplicates": K::Duplicate,
        // --- series statistics (ComicRack.Engine/Database/) ---
        "SmartListSeriesAllCompleteMatcher" => "Series: All complete": K::Series(S::AllComplete),
        "SmartListSeriesAverageCommunityRatingMatcher" => "Series: Average Community Rating": K::Series(S::AverageCommunityRating),
        "SmartListSeriesAverageRatingMatcher" => "Series: Average Rating": K::Series(S::AverageRating),
        "SmartListSeriesCountMatcher" => "Series: Book Count": K::Series(S::Count),
        "SmartListSeriesMaxGapSizeMatcher" => "Series: Biggest Gap": K::Series(S::MaxGapSize),
        "SmartListSeriesEndOfGapMatcher" => "Series: End of Gap": K::Series(S::GapEnd),
        "SmartListSeriesFirstNumberMatcher" => "Series: First Number": K::Series(S::FirstNumber),
        "SmartListSeriesMinYearMatcher" => "Series: First Year": K::Series(S::FirstYear),
        "SmartListSeriesGapsMatcher" => "Series: Gaps": K::Series(S::GapCount),
        "SmartListSeriesMaxCountMatcher" => "Series: Highest Count": K::Series(S::MaxCount),
        "SmartListSeriesLastNumberMatcher" => "Series: Last Number": K::Series(S::LastNumber),
        "SmartListSeriesMaxYearMatcher" => "Series: Last Year": K::Series(S::LastYear),
        "SmartListSeriesMinCountMatcher" => "Series: Lowest Count": K::Series(S::MinCount),
        "SmartListSeriesLastOpenedTimeMatcher" => "Series: Opened": K::Series(S::LastOpenedTime),
        "SmartListSeriesPageCountMatcher" => "Series: Pages": K::Series(S::PageCount),
        "SmartListSeriesPagesReadMatcher" => "Series: Pages Read": K::Series(S::PageReadCount),
        "SmartListSeriesPercentReadMatcher" => "Series: Percent Read": K::Series(S::ReadPercentage),
        "SmartListSeriesLastPublishedTimeMatcher" => "Series: Published": K::Series(S::LastPublishedTime),
        "SmartListSeriesLastAddedTimeMatcher" => "Series: Book added": K::Series(S::LastAddedTime),
        "SmartListSeriesLastReleasedTimeMatcher" => "Series: Book released": K::Series(S::LastReleasedTime),
        "SmartListSeriesRunningTimeYearsMatcher" => "Series: Running Time Years": K::Series(S::RunningTimeYears),
        "SmartListSeriesStartOfGapMatcher" => "Series: Start of Gap": K::Series(S::GapStart),
    )
}

/// Finds a spec by description (query `[Name]` token), case-insensitive.
/// Mirrors `ComicBookValueMatcher.Create(string)`.
pub fn by_description(name: &str) -> Option<&'static MatcherSpec> {
    all_specs()
        .iter()
        .find(|s| s.description.eq_ignore_ascii_case(name))
}

/// Finds a spec by C# class name (`xsi:type`), case-sensitive like the
/// XML type reference.
pub fn by_class_name(class_name: &str) -> Option<&'static MatcherSpec> {
    all_specs().iter().find(|s| s.class_name == class_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_spec_has_unique_class_name() {
        let all = all_specs();
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                assert_ne!(a.class_name, b.class_name, "duplicate class name");
            }
        }
    }

    #[test]
    fn lookups_work() {
        assert_eq!(
            by_description("my rating").unwrap().class_name,
            "ComicBookRatingMatcher"
        );
        assert_eq!(
            by_class_name("ComicBookRatingMatcher").unwrap().description,
            "My Rating"
        );
        assert!(by_description("No Such Matcher").is_none());
        assert!(by_class_name("ComicBookNoSuchMatcher").is_none());
    }

    #[test]
    fn argument_counts() {
        let rating = by_class_name("ComicBookRatingMatcher").unwrap();
        assert_eq!(rating.argument_count(ops::NUM_EQUAL), 1);
        assert_eq!(rating.argument_count(ops::NUM_IN_RANGE), 2);
        let checked = by_class_name("ComicBookCheckedMatcher").unwrap();
        assert_eq!(checked.argument_count(0), 0);
        let custom = by_class_name("ComicBookCustomValuesMatcher").unwrap();
        assert_eq!(custom.argument_count(0), 2);
        let published = by_class_name("ComicBookPublishedMatcher").unwrap();
        assert_eq!(published.argument_count(ops::DATE_IS_IN_RANGE), 2);
        assert_eq!(published.argument_count(ops::DATE_IS_AFTER), 1);
    }
}
