//! The field catalog and typed accessors (the port of the addon's
//! `template_to_field`, `name_to_field`, `field_to_name` tables plus the
//! `getattr(book, field)` reads).
//!
//! Template and rule field names keep the addon's spellings for
//! profile compatibility. Shadow fields (`ShadowSeries`, …) resolve
//! through `cr_engine::matcher::book_view`, which carries the exact
//! C# semantics (the stored value, or the proposed file-name parse
//! when the stored value is empty and `EnableProposed` is on).

use cr_core::model::comic_book::ComicBook;
use cr_core::model::enums::{MangaYesNo, YesNo};
use cr_core::xml::scalar::CrDateTime;
use cr_engine::matcher::book_view;

/// The addon's template-name → C# field table (`template_to_field` in
/// lobookmover.py), verbatim. `month#`/`startmonth#`/`EndMonth#` are
/// full token NAMES for the regex (digits and `#` behave differently).
pub fn template_field(name: &str) -> Option<&'static str> {
    Some(match name {
        "series" => "ShadowSeries",
        "number" => "ShadowNumber",
        "count" => "ShadowCount",
        "Day" => "Day",
        "ReleasedDate" => "ReleasedTime",
        "AddedDate" => "AddedTime",
        "EndYear" => "EndYear",
        "EndMonth" => "EndMonth",
        "EndMonth#" => "EndMonth",
        "month" => "Month",
        "month#" => "Month",
        "year" => "ShadowYear",
        "imprint" => "Imprint",
        "publisher" => "Publisher",
        "altSeries" => "AlternateSeries",
        "altNumber" => "AlternateNumber",
        "altCount" => "AlternateCount",
        "volume" => "ShadowVolume",
        "title" => "ShadowTitle",
        "ageRating" => "AgeRating",
        "language" => "LanguageAsText",
        "format" => "ShadowFormat",
        "startyear" => "StartYear",
        "writer" => "Writer",
        "tags" => "Tags",
        "genre" => "Genre",
        "characters" => "Characters",
        "teams" => "Teams",
        "scaninfo" => "ScanInformation",
        "manga" => "Manga",
        "seriesComplete" => "SeriesComplete",
        "first" => "FirstLetter",
        "read" => "ReadPercentage",
        "counter" => "Counter",
        "startmonth" => "StartMonth",
        "startmonth#" => "StartMonth",
        "colorist" => "Colorist",
        "coverartist" => "CoverArtist",
        "editor" => "Editor",
        "inker" => "Inker",
        "letterer" => "Letterer",
        "locations" => "Locations",
        "penciller" => "Penciller",
        "storyarc" => "StoryArc",
        "seriesgroup" => "SeriesGroup",
        "maincharacter" => "MainCharacterOrTeam",
        "firstissuenumber" => "FirstIssueNumber",
        "lastissuenumber" => "LastIssueNumber",
        "Rating" => "Rating",
        "CommunityRating" => "CommunityRating",
        "Custom" => "Custom",
        _ => return None,
    })
}

/// The rule display-name → C# field table (`name_to_field` in
/// locommon.py), verbatim. Three names resolve to C# properties that
/// do not exist (`AddedDate`, `Counter`, and the spaced
/// `"Released Date"`); the addon raises there, the port returns None
/// and skips the rule — recorded in the phase file.
pub fn rule_field(display: &str) -> Option<&'static str> {
    Some(match display {
        "Added Date" => "AddedDate",
        "Age Rating" => "AgeRating",
        "Alternate Count" => "AlternateCount",
        "Alternate Number" => "AlternateNumber",
        "Alternate Series" => "AlternateSeries",
        "Black And White" => "BlackAndWhite",
        "Characters" => "Characters",
        "Colorist" => "Colorist",
        "Counter" => "Counter",
        "Count" => "ShadowCount",
        "Cover Artist" => "CoverArtist",
        "Day" => "Day",
        "Editor" => "Editor",
        "End Year" => "EndYear",
        "End Month" => "EndMonth",
        "File Format" => "FileFormat",
        "File Name" => "FileName",
        "File Path" => "FilePath",
        "First Letter" => "FirstLetter",
        "Format" => "ShadowFormat",
        "Genre" => "Genre",
        "Imprint" => "Imprint",
        "Inker" => "Inker",
        "Language" => "LanguageISO",
        "Letterer" => "Letterer",
        "Locations" => "Locations",
        "Main Character Or Team" => "MainCharacterOrTeam",
        "Manga" => "Manga",
        "Month" => "Month",
        "Notes" => "Notes",
        "Number" => "ShadowNumber",
        "Penciller" => "Penciller",
        "Publisher" => "Publisher",
        "Rating" => "Rating",
        "Read Percentage" => "ReadPercentage",
        "Released Date" => "Released Date",
        "Review" => "Review",
        "Scan Information" => "ScanInformation",
        "Series" => "ShadowSeries",
        "Series Complete" => "SeriesComplete",
        "Series Group" => "SeriesGroup",
        "Start Month" => "StartMonth",
        "Start Year" => "StartYear",
        "Story Arc" => "StoryArc",
        "Tags" => "Tags",
        "Teams" => "Teams",
        "Title" => "ShadowTitle",
        "Volume" => "ShadowVolume",
        "Web" => "Web",
        "Writer" => "Writer",
        "Year" => "ShadowYear",
        _ => return None,
    })
}

/// The rule field catalog the config dialog offers
/// (`MetadataExcludeRuleControl` items in configformcontrols.py), sorted
/// like the addon's sorted combobox.
pub const RULE_FIELD_CATALOG: &[&str] = &[
    "Age Rating",
    "Alternate Count",
    "Alternate Number",
    "Alternate Series",
    "Black And White",
    "Characters",
    "Count",
    "File Name",
    "File Path",
    "File Format",
    "Format",
    "Genre",
    "Imprint",
    "Language",
    "Locations",
    "Main Character Or Team",
    "Manga",
    "Month",
    "Number",
    "Notes",
    "Publisher",
    "Rating",
    "Read Percentage",
    "Review",
    "Scan Information",
    "Series",
    "Series Complete",
    "Series Group",
    "Start Month",
    "Start Year",
    "Story Arc",
    "Tags",
    "Teams",
    "Title",
    "Volume",
    "Web",
    "Year",
];

/// True when `name` is a C# field name (`field_to_name` membership,
/// the `args[0] in field_to_name` check of `insert_first_letter`).
pub fn field_to_name(name: &str) -> bool {
    rule_field(name).is_some()
        || matches!(
            name,
            "AddedDate"
                | "AgeRating"
                | "AlternateCount"
                | "AlternateNumber"
                | "AlternateSeries"
                | "BlackAndWhite"
                | "Characters"
                | "Colorist"
                | "Counter"
                | "CoverArtist"
                | "Day"
                | "Editor"
                | "EndMonth"
                | "EndYear"
                | "FileFormat"
                | "FileName"
                | "FilePath"
                | "FirstLetter"
                | "Genre"
                | "Imprint"
                | "Inker"
                | "LanguageISO"
                | "Letterer"
                | "Locations"
                | "MainCharacterOrTeam"
                | "Manga"
                | "Month"
                | "Notes"
                | "Penciller"
                | "Publisher"
                | "Rating"
                | "ReadPercentage"
                | "ReleasedDate"
                | "Review"
                | "ScanInformation"
                | "SeriesComplete"
                | "ShadowCount"
                | "ShadowFormat"
                | "ShadowNumber"
                | "ShadowSeries"
                | "ShadowTitle"
                | "ShadowVolume"
                | "ShadowYear"
                | "SeriesGroup"
                | "StoryArc"
                | "StartMonth"
                | "StartYear"
                | "Tags"
                | "Teams"
                | "Web"
                | "Writer"
        )
}

/// The insert-control catalog (display name, template name, kind), the
/// shape the config dialog builds (`create_*_insert_controls` in
/// configureform.py). Keyed by display name in the per-field tables.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InsertKind {
    Text,
    Number,
    YesNo,
    MultiValue,
    FirstLetter,
    Counter,
    ReadPercentage,
    DateTime,
}

pub const INSERT_CONTROLS: &[(&str, &str, InsertKind)] = &[
    ("Age Rating", "ageRating", InsertKind::Text),
    ("Alternate Series", "altSeries", InsertKind::Text),
    ("Format", "format", InsertKind::Text),
    ("Imprint", "imprint", InsertKind::Text),
    ("Language", "language", InsertKind::Text),
    ("Main Character Or Team", "maincharacter", InsertKind::Text),
    ("Month", "month", InsertKind::Text),
    ("Publisher", "publisher", InsertKind::Text),
    ("Series", "series", InsertKind::Text),
    ("Series Group", "seriesgroup", InsertKind::Text),
    ("Story Arc", "storyarc", InsertKind::Text),
    ("Title", "title", InsertKind::Text),
    ("Custom", "Custom", InsertKind::FirstLetter),
    ("Alternate Count", "altCount", InsertKind::Number),
    ("Alternate Number", "altNumber", InsertKind::Number),
    ("Count", "count", InsertKind::Number),
    ("Day", "Day", InsertKind::Number),
    ("Month Number", "month#", InsertKind::Number),
    ("Number", "number", InsertKind::Number),
    ("Volume", "volume", InsertKind::Number),
    ("Year", "year", InsertKind::Number),
    ("Rating", "Rating", InsertKind::Text),
    ("CommunityRating", "CommunityRating", InsertKind::Text),
    ("Manga", "manga", InsertKind::YesNo),
    ("Series Complete", "seriesComplete", InsertKind::YesNo),
    (
        "Alternate Series Multi",
        "altSeries",
        InsertKind::MultiValue,
    ),
    ("Characters", "characters", InsertKind::MultiValue),
    ("Colorist", "colorist", InsertKind::MultiValue),
    ("Cover Artist", "coverartist", InsertKind::MultiValue),
    ("Editor", "editor", InsertKind::MultiValue),
    ("Genre", "genre", InsertKind::MultiValue),
    ("Inker", "inker", InsertKind::MultiValue),
    ("Letterer", "letterer", InsertKind::MultiValue),
    ("Locations", "locations", InsertKind::MultiValue),
    ("Penciller", "penciller", InsertKind::MultiValue),
    ("Scan Information", "scaninfo", InsertKind::MultiValue),
    ("Tags", "tags", InsertKind::MultiValue),
    ("Teams", "teams", InsertKind::MultiValue),
    ("Writer", "writer", InsertKind::MultiValue),
    ("First Letter", "first", InsertKind::FirstLetter),
    ("Read Percentage", "read", InsertKind::ReadPercentage),
    ("Added Date", "AddedDate", InsertKind::DateTime),
    ("Released Date", "ReleasedDate", InsertKind::DateTime),
    ("Start Year", "startyear", InsertKind::Number),
    ("Start Month", "startmonth", InsertKind::Text),
    ("End Year", "EndYear", InsertKind::Number),
    ("End Month", "EndMonth", InsertKind::Text),
    ("Counter", "counter", InsertKind::Counter),
    ("First Issue Number", "firstissuenumber", InsertKind::Number),
    ("Last Issue Number", "lastissuenumber", InsertKind::Number),
];

/// The Yes/No value items per field, in combobox order
/// (`field_selection_index_changed` in configformcontrols.py).
pub fn yes_no_values(field: &str) -> Option<&'static [&'static str]> {
    match field {
        "Manga" => Some(&["Yes", "Yes (Right to Left)", "No", "Unknown"]),
        "Series Complete" | "Black And White" => Some(&["Yes", "No", "Unknown"]),
        _ => None,
    }
}

/// The Yes/No C# fields (`yes_no_fields` in lobookmover.py).
pub fn is_yes_no_field(field: &str) -> bool {
    field == "Manga" || field == "SeriesComplete"
}

/// A typed field value (what `getattr(book, field)` returns).
#[derive(Clone, Debug, PartialEq)]
pub enum FieldValue {
    Str(String),
    Int(i32),
    Float(f32),
    Date(CrDateTime),
    YesNo(YesNo),
    Manga(MangaYesNo),
}

/// The .NET `DateTime.ToString()` form (invariant culture) used when a
/// date lands in a template or rule comparison without a format arg.
/// The addon emits the OS culture's form; the invariant shape is the
/// recorded port choice.
pub fn date_display(d: &CrDateTime) -> String {
    d.naive.format("%m/%d/%Y %H:%M:%S").to_string()
}

/// Reads a field by its C# name. `None` when the name has no ported
/// property (`Counter`, `FirstLetter`, `StartYear`, series-relative
/// numbers, `Custom` — their owners handle them).
pub fn field_raw(book: &ComicBook, field: &str) -> Option<FieldValue> {
    let prop = book_view::proposed_cached(book);
    Some(match field {
        "ShadowSeries" => FieldValue::Str(book_view::shadow_series(book, &prop).to_string()),
        "ShadowTitle" => FieldValue::Str(book_view::shadow_title(book, &prop).to_string()),
        "ShadowFormat" => FieldValue::Str(book_view::shadow_format(book, &prop).to_string()),
        "ShadowNumber" => FieldValue::Str(book_view::shadow_number(book, &prop).to_string()),
        "ShadowVolume" => FieldValue::Int(book_view::shadow_volume(book, &prop)),
        "ShadowCount" => FieldValue::Int(book_view::shadow_count(book, &prop)),
        "ShadowYear" => FieldValue::Int(book_view::shadow_year(book, &prop)),
        "Title" => FieldValue::Str(book.info.title.clone()),
        "Publisher" => FieldValue::Str(book.info.publisher.clone()),
        "Imprint" => FieldValue::Str(book.info.imprint.clone()),
        "Writer" => FieldValue::Str(book.info.writer.clone()),
        "Penciller" => FieldValue::Str(book.info.penciller.clone()),
        "Inker" => FieldValue::Str(book.info.inker.clone()),
        "Colorist" => FieldValue::Str(book.info.colorist.clone()),
        "Letterer" => FieldValue::Str(book.info.letterer.clone()),
        "CoverArtist" => FieldValue::Str(book.info.cover_artist.clone()),
        "Editor" => FieldValue::Str(book.info.editor.clone()),
        "AgeRating" => FieldValue::Str(book.info.age_rating.clone()),
        "LanguageISO" => FieldValue::Str(book.info.language_iso.clone()),
        "Genre" => FieldValue::Str(book.info.genre.clone()),
        "Tags" => FieldValue::Str(book.info.tags.clone()),
        "SeriesGroup" => FieldValue::Str(book.info.series_group.clone()),
        "StoryArc" => FieldValue::Str(book.info.story_arc.clone()),
        "AlternateSeries" => FieldValue::Str(book.info.alternate_series.clone()),
        "AlternateNumber" => FieldValue::Str(book.info.alternate_number.clone()),
        "Characters" => FieldValue::Str(book.info.characters.clone()),
        "Teams" => FieldValue::Str(book.info.teams.clone()),
        "Locations" => FieldValue::Str(book.info.locations.clone()),
        "MainCharacterOrTeam" => FieldValue::Str(book.info.main_character_or_team.clone()),
        "Notes" => FieldValue::Str(book.info.notes.clone()),
        "Web" => FieldValue::Str(book.info.web.clone()),
        "ScanInformation" => FieldValue::Str(book.info.scan_information.clone()),
        "Review" => FieldValue::Str(book.info.review.clone()),
        "Summary" => FieldValue::Str(book.info.summary.clone()),
        "Month" => FieldValue::Int(book.info.month),
        "Day" => FieldValue::Int(book.info.day),
        "Count" => FieldValue::Int(book.info.count),
        "Volume" => FieldValue::Int(book.info.volume),
        "Year" => FieldValue::Int(book.info.year),
        "AlternateCount" => FieldValue::Int(book.info.alternate_count),
        "PageCount" => FieldValue::Int(book.info.page_count),
        "ReadPercentage" => FieldValue::Int(book_view::read_percentage(book)),
        "Rating" => FieldValue::Float(book.rating),
        "CommunityRating" => FieldValue::Float(book.info.community_rating),
        "SeriesComplete" => FieldValue::YesNo(book.series_complete),
        "BlackAndWhite" => FieldValue::YesNo(book.info.black_and_white),
        "Manga" => FieldValue::Manga(book.info.manga),
        "AddedTime" => FieldValue::Date(book.added_time),
        "ReleasedTime" => FieldValue::Date(book.released_time),
        "OpenedTime" => FieldValue::Date(book.opened_time),
        "FileName" => FieldValue::Str(
            cr_core::model::comic_name_info::file_name_without_extension(&book.file_path),
        ),
        "FilePath" => FieldValue::Str(book.file_path.clone()),
        "FileFormat" => FieldValue::Str(file_format_name(book)),
        "LanguageAsText" => FieldValue::Str(book_view::language_as_text(book)),
        _ => return None,
    })
}

/// The C# `FileFormat` property: the source format's display name from
/// the file extension, `Unknown` when nothing matches.
/// (ProviderFactory.GetSourceFormatName → TR.Default["Unknown"].)
pub fn file_format_name(book: &ComicBook) -> String {
    cr_io::formats::source_format(std::path::Path::new(&book.file_path))
        .map(|f| f.name.to_string())
        .unwrap_or_else(|| "Unknown".to_string())
}

/// The Python truthiness form used by text fields
/// (`insert_text_field`): `not text or text == -1` → empty. An empty
/// result means the token produces nothing.
pub fn field_display(book: &ComicBook, field: &str) -> Option<String> {
    match field_raw(book, field)? {
        FieldValue::Str(s) => Some(s),
        FieldValue::Int(i) if i == -1 || i == 0 => Some(String::new()),
        FieldValue::Int(i) => Some(i.to_string()),
        FieldValue::Float(0.0) => Some(String::new()),
        FieldValue::Float(f) => Some(crate::template::net_f32_text(f)),
        FieldValue::Date(d) if d.is_min_value() => Some(String::new()),
        FieldValue::Date(d) => Some(date_display(&d)),
        FieldValue::YesNo(y) => Some(y.to_xml()),
        FieldValue::Manga(m) => Some(m.to_xml()),
    }
}

/// The `unicode(getattr(book, field))` form used by the exclude rules:
/// no truthiness filter, so `-1` dates/counts compare as `"-1"`.
pub fn field_rule_text(book: &ComicBook, field: &str) -> Option<String> {
    match field_raw(book, field)? {
        FieldValue::Str(s) => Some(s),
        FieldValue::Int(i) => Some(i.to_string()),
        FieldValue::Float(f) => Some(crate::template::net_f32_text(f)),
        FieldValue::Date(d) => Some(date_display(&d)),
        FieldValue::YesNo(y) => Some(y.to_xml()),
        FieldValue::Manga(m) => Some(m.to_xml()),
    }
}

/// Splits a multi-value field value (comma separated, stripped), like
/// the addon's `[item.strip() for item in value.split(",")]` — empty
/// entries dropped at the call sites that need that.
pub fn split_multi_values(text: &str) -> Vec<String> {
    text.split(',').map(|v| v.trim().to_string()).collect()
}

/// The C# field name → display name for the config dialogs
/// (`field_to_name` in locommon.py, plus the Shadow* spellings the
/// tables actually key on).
pub fn display_name_of(field: &str) -> &str {
    match field {
        "AddedDate" => "Added Date",
        "AgeRating" => "Age Rating",
        "AlternateCount" => "Alternate Count",
        "AlternateNumber" => "Alternate Number",
        "AlternateSeries" => "Alternate Series",
        "BlackAndWhite" => "Black And White",
        "Characters" => "Characters",
        "Colorist" => "Colorist",
        "Counter" => "Counter",
        "CoverArtist" => "Cover Artist",
        "Day" => "Day",
        "Editor" => "Editor",
        "EndMonth" => "End Month",
        "EndYear" => "End Year",
        "FileFormat" => "File Format",
        "FileName" => "File Name",
        "FilePath" => "File Path",
        "FirstLetter" => "First Letter",
        "Genre" => "Genre",
        "Imprint" => "Imprint",
        "Inker" => "Inker",
        "LanguageISO" => "Language",
        "Letterer" => "Letterer",
        "Locations" => "Locations",
        "MainCharacterOrTeam" => "Main Character Or Team",
        "Manga" => "Manga",
        "Month" => "Month",
        "Notes" => "Notes",
        "Penciller" => "Penciller",
        "Publisher" => "Publisher",
        "Rating" => "Rating",
        "ReadPercentage" | "Read" => "Read Percentage",
        "ReleasedDate" => "Released Date",
        "Review" => "Review",
        "ScanInformation" => "Scan Information",
        "SeriesComplete" => "Series Complete",
        "ShadowCount" | "Count" => "Count",
        "ShadowFormat" | "Format" => "Format",
        "ShadowNumber" | "Number" => "Number",
        "ShadowSeries" | "Series" => "Series",
        "ShadowTitle" | "Title" => "Title",
        "ShadowVolume" | "Volume" => "Volume",
        "ShadowYear" | "Year" => "Year",
        "SeriesGroup" => "Series Group",
        "StoryArc" => "Story Arc",
        "StartMonth" => "Start Month",
        "StartYear" => "Start Year",
        "Tags" => "Tags",
        "Teams" => "Teams",
        "Web" => "Web",
        "Writer" => "Writer",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn book() -> ComicBook {
        let mut b = ComicBook::default();
        b.info.series = "Batman".into();
        b.info.number = "5".into();
        b.info.year = 2012;
        b.info.month = 3;
        b.info.writer = "Grant Morrison, Frank Quitely".into();
        b.file_path = "/comics/Batman 005.cbz".into();
        b
    }

    #[test]
    fn template_table_is_verbatim() {
        assert_eq!(template_field("series"), Some("ShadowSeries"));
        assert_eq!(template_field("number"), Some("ShadowNumber"));
        assert_eq!(template_field("month#"), Some("Month"));
        assert_eq!(template_field("startmonth#"), Some("StartMonth"));
        assert_eq!(template_field("AddedDate"), Some("AddedTime"));
        assert_eq!(template_field("CommunityRating"), Some("CommunityRating"));
        assert_eq!(template_field("nonesuch"), None);
    }

    #[test]
    fn rule_table_is_verbatim() {
        assert_eq!(rule_field("Series"), Some("ShadowSeries"));
        assert_eq!(rule_field("Count"), Some("ShadowCount"));
        assert_eq!(rule_field("Read Percentage"), Some("ReadPercentage"));
        assert_eq!(rule_field("Added Date"), Some("AddedDate"));
        assert_eq!(rule_field("First Letter"), Some("FirstLetter"));
        assert_eq!(rule_field("Counter"), Some("Counter"));
    }

    #[test]
    fn field_reads() {
        let b = book();
        assert_eq!(
            field_raw(&b, "ShadowSeries"),
            Some(FieldValue::Str("Batman".into()))
        );
        assert_eq!(field_raw(&b, "ShadowYear"), Some(FieldValue::Int(2012)));
        assert_eq!(field_raw(&b, "ShadowCount"), Some(FieldValue::Int(-1)));
        // A .cbz book reads as the ZIP eComic format.
        assert_eq!(
            field_rule_text(&b, "FileFormat"),
            Some("eComic (ZIP)".to_string())
        );
        // Rule text keeps "-1"; display text empties it.
        assert_eq!(field_rule_text(&b, "ShadowCount"), Some("-1".to_string()));
        assert_eq!(field_display(&b, "ShadowCount"), Some(String::new()));
        assert_eq!(field_display(&b, "ShadowNumber"), Some("5".to_string()));
    }

    #[test]
    fn multi_value_split() {
        assert_eq!(
            split_multi_values("Grant Morrison,  Frank Quitely "),
            vec!["Grant Morrison".to_string(), "Frank Quitely".to_string()]
        );
    }
}
