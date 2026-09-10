//! The Detail-mode column set — the browser defaults from
//! `ComicBrowserControl` (`InitializeColumns`): id, caption, default
//! width, alignment, and the `ComicBook` property each cell shows
//! (`ComicListField.DisplayProperty`). Cell text resolves through the
//! Phase 0 property registry (`cr_core::registry`) — the same source
//! the matchers use — with the proposed-name fallback
//! (`GetColumnStringValue(proposed: true)`).

use cr_core::model::comic_book::ComicBook;
/// `ColumnAlignment` (the C# `StringAlignment` subset).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColumnAlignment {
    Near,
    Far,
    Center,
}

/// One column (`ItemViewColumn` + `ComicListField`).
#[derive(Clone, Debug)]
pub struct Column {
    /// The C# column id (the persisted `ItemViewConfig` key).
    pub id: i32,
    /// The header caption (`ItemViewColumn.Name`).
    pub name: &'static str,
    /// `DisplayProperty` — the registry key for the cell text.
    pub property: &'static str,
    pub width: f64,
    pub alignment: ColumnAlignment,
    /// `Visible` (hidden columns keep `LastTimeVisible`).
    pub visible: bool,
}

/// The browser's full column set in the C# registration order
/// (`ComicBrowserControl.cs:755-843`). The DEFAULT-VISIBLE columns
/// (the 13 `visible: true`) keep the C# ctor defaults (visible is the
/// `ItemViewColumn` ctor default); the rest ship hidden — the column
/// menu reveals them (`CreateHeaderMenu`).
pub fn default_columns() -> Vec<Column> {
    use ColumnAlignment::*;
    let column = |id: i32,
                  name: &'static str,
                  property: &'static str,
                  width: f64,
                  alignment: ColumnAlignment,
                  visible: bool| {
        Column {
            id,
            name,
            property,
            width,
            alignment,
            visible,
        }
    };
    vec![
        column(101, "State", "State", 60.0, Near, true),
        column(100, "Position", "", 30.0, Far, true),
        column(102, "Checked", "Checked", 22.0, Near, true),
        column(0, "Cover", "", 40.0, Center, true),
        column(1, "Series", "Series", 200.0, Near, true),
        column(2, "Number", "NumberAsText", 40.0, Far, true),
        column(3, "Volume", "VolumeAsText", 40.0, Near, true),
        column(4, "Title", "Title", 200.0, Near, false),
        column(5, "Opened", "OpenedTime", 40.0, Far, true),
        column(6, "Added", "AddedTime", 40.0, Far, true),
        column(7, "Pages", "PagesAsTextSimple", 40.0, Far, true),
        column(39, "Published", "PublishedAsText", 40.0, Far, true),
        column(9, "File Path", "FilePath", 200.0, Near, false),
        column(10, "File Name", "FileName", 200.0, Near, false),
        column(11, "Writer", "Writer", 80.0, Near, true),
        column(12, "Penciller", "Penciller", 80.0, Near, false),
        column(13, "Inker", "Inker", 80.0, Near, false),
        column(14, "Colorist", "Colorist", 80.0, Near, false),
        column(15, "My Rating", "Rating", 50.0, Near, true),
        column(16, "Opened Count", "OpenedCountAsText", 40.0, Far, false),
        column(
            17,
            "Read Percentage",
            "ReadPercentageAsText",
            40.0,
            Far,
            false,
        ),
        column(18, "File Modified", "FileModifiedTime", 40.0, Far, false),
        column(19, "Genre", "Genre", 40.0, Near, false),
        column(20, "Publisher", "Publisher", 40.0, Near, false),
        column(21, "Count", "CountAsText", 40.0, Far, false),
        column(22, "Letterer", "Letterer", 80.0, Near, false),
        column(23, "Cover Artist", "CoverArtist", 80.0, Near, false),
        column(24, "Editor", "Editor", 80.0, Near, false),
        column(72, "Translator", "Translator", 80.0, Near, false),
        column(25, "File Size", "FileSizeAsText", 40.0, Far, false),
        column(
            26,
            "Alternate Series",
            "AlternateSeries",
            200.0,
            Near,
            false,
        ),
        column(
            27,
            "Alternate Number",
            "AlternateNumberAsText",
            40.0,
            Far,
            false,
        ),
        column(
            28,
            "Alternate Count",
            "AlternateCountAsText",
            40.0,
            Far,
            false,
        ),
        column(29, "Month", "MonthAsText", 40.0, Far, false),
        column(30, "Caption", "Caption", 200.0, Near, false),
        column(31, "Tags", "Tags", 60.0, Near, false),
        column(32, "Imprint", "Imprint", 40.0, Near, false),
        column(33, "Language", "LanguageAsText", 40.0, Near, false),
        column(34, "Format", "Format", 40.0, Near, false),
        column(35, "B&W", "BlackAndWhiteAsText", 22.0, Near, false),
        column(36, "Manga", "MangaAsText", 22.0, Near, false),
        column(37, "File Format", "FileFormat", 40.0, Near, false),
        column(38, "Age Rating", "AgeRating", 40.0, Near, false),
        column(8, "Year", "YearAsText", 60.0, Far, false),
        column(40, "Characters", "Characters", 60.0, Near, false),
        column(41, "File Directory", "FileDirectory", 60.0, Near, false),
        column(42, "File Created", "FileCreationTime", 60.0, Far, false),
        column(43, "Bookmark Count", "BookmarksAsText", 60.0, Far, false),
        column(44, "New Pages", "NewPagesAsText", 60.0, Far, false),
        column(45, "Teams", "Teams", 60.0, Near, false),
        column(46, "Locations", "Locations", 60.0, Near, false),
        column(47, "Web", "Web", 60.0, Near, false),
        column(48, "Community Rating", "CommunityRating", 50.0, Near, false),
        column(49, "Linked", "IsLinkedAsText", 50.0, Near, false),
        column(50, "Book Price", "BookPriceAsText", 50.0, Far, false),
        column(51, "Book Age", "BookAge", 50.0, Near, false),
        column(52, "Book Store", "BookStore", 50.0, Near, false),
        column(53, "Book Owner", "BookOwner", 50.0, Near, false),
        column(54, "Book Condition", "BookCondition", 50.0, Near, false),
        column(
            55,
            "Book Collection Status",
            "BookCollectionStatus",
            50.0,
            Near,
            false,
        ),
        column(56, "Book Location", "BookLocation", 50.0, Near, false),
        column(57, "ISBN", "ISBN", 50.0, Near, false),
        column(
            58,
            "Series complete",
            "SeriesCompleteAsText",
            22.0,
            Near,
            false,
        ),
        column(
            59,
            "Proposed Values",
            "EnableProposedAsText",
            22.0,
            Near,
            false,
        ),
        // The C# draws an image for the gap state (DrawGapInfo) — the
        // cell text stays empty (recorded deviation).
        column(60, "Gap Information", "GapInformation", 16.0, Near, false),
        column(61, "Read", "HasBeenReadAsText", 22.0, Near, false),
        // Drawn as icons in the C# (ThumbRenderer.DrawImageList).
        column(62, "Icons", "Icons", 100.0, Near, false),
        column(
            63,
            "Scan Information",
            "ScanInformation",
            100.0,
            Near,
            false,
        ),
        column(64, "Story Arc", "StoryArc", 100.0, Near, false),
        column(65, "Series Group", "SeriesGroup", 100.0, Near, false),
        column(
            66,
            "Main Character/Team",
            "MainCharacterOrTeam",
            100.0,
            Near,
            false,
        ),
        column(67, "Review", "Review", 100.0, Near, false),
        column(68, "Day", "DayAsText", 40.0, Far, false),
        column(69, "Week", "WeekAsText", 40.0, Far, false),
        column(70, "Released", "ReleasedTime", 60.0, Far, false),
        column(
            71,
            "Published (Regional)",
            "PublishedRegional",
            40.0,
            Far,
            false,
        ),
        column(
            200,
            "Series: Books",
            "SeriesStatCountAsText",
            50.0,
            Far,
            false,
        ),
        column(
            201,
            "Series: Pages",
            "SeriesStatPageCountAsText",
            50.0,
            Far,
            false,
        ),
        column(
            202,
            "Series: Pages Read",
            "SeriesStatPageReadCountAsText",
            50.0,
            Far,
            false,
        ),
        column(
            203,
            "Series: Percent Read",
            "SeriesStatReadPercentageAsText",
            50.0,
            Far,
            false,
        ),
        column(
            204,
            "Series: First Number",
            "SeriesStatMinNumberAsText",
            50.0,
            Far,
            false,
        ),
        column(
            205,
            "Series: Last Number",
            "SeriesStatMaxNumberAsText",
            50.0,
            Far,
            false,
        ),
        column(
            206,
            "Series: First Year",
            "SeriesStatMinYearAsText",
            50.0,
            Far,
            false,
        ),
        column(
            207,
            "Series: Last Year",
            "SeriesStatMaxYearAsText",
            50.0,
            Far,
            false,
        ),
        column(
            208,
            "Series: Average Rating",
            "SeriesStatAverageRating",
            50.0,
            Near,
            false,
        ),
        column(
            209,
            "Series: Average Community Rating",
            "SeriesStatAverageCommunityRating",
            50.0,
            Far,
            false,
        ),
        column(
            210,
            "Series: Gaps",
            "SeriesStatGapCountAsText",
            50.0,
            Far,
            false,
        ),
        column(
            211,
            "Series: Book added",
            "SeriesStatLastAddedTime",
            50.0,
            Far,
            false,
        ),
        column(
            212,
            "Series: Opened",
            "SeriesStatLastOpenedTime",
            50.0,
            Far,
            false,
        ),
        column(
            213,
            "Series: Book released",
            "SeriesStatLastReleasedTime",
            50.0,
            Far,
            false,
        ),
        column(
            214,
            "Actual File Format (slow)",
            "ActualFileFormat",
            40.0,
            Near,
            false,
        ),
    ]
}

impl Column {
    pub fn is_text_column(&self) -> bool {
        // 0 Cover / 101 State / 60 Gap / 62 Icons draw images in the
        // C# Detail renderer (`CoverViewItem.OnDraw` switch); no text.
        !matches!(self.id, 0 | 60 | 62 | 101) && !self.property.is_empty()
    }
}

/// `GetColumnStringValue(proposed: true)` — the cell text through the
/// engine's display resolver (shadow/computed names), the C# default
/// text ("" when unset).
pub fn cell_text(column: &Column, book: &ComicBook) -> String {
    if !column.is_text_column() {
        return String::new();
    }
    cr_engine::display_text::column_text(book, column.property)
}

/// One chooser entry (id, name, visible) — the input of
/// [`chooser_menu`].
pub type ChooserEntry = (i32, String, bool);

/// The header-menu shape (`ContextMenuBuilder.Create(20)` — the C#
/// `CreateHeaderMenu` fill): the visible columns at top level in
/// registration order, an "All" submenu (every column, alphabetical),
/// and letter submenus whose runs merge while they stay under 20
/// entries (`A-B`, `C-F`, `G-O`, `P-R`, `S`, `T-Y` for the full CR
/// column table). The C# "Recent" submenu (LastTimeVisible order) is
/// not ported — the port tracks no last-used times and the C# hides
/// the submenu when it is empty anyway.
#[derive(Debug, PartialEq)]
pub struct ChooserMenu {
    /// The top-level rows (the visible columns, column order).
    pub top: Vec<ChooserEntry>,
    /// Every column, alphabetical (case-insensitive).
    pub all: Vec<ChooserEntry>,
    /// (label, entries) per letter run — entries alphabetical.
    pub letters: Vec<(String, Vec<ChooserEntry>)>,
}

/// `ContextMenuBuilder.Create(20)`:
/// - under 20 entries → everything flat (one level);
/// - else the visible entries at top level, a separator, "All" with
///   all entries, then letter submenus merged with the exact C#
///   run-merge rule.
pub fn chooser_menu(columns: &[ChooserEntry]) -> ChooserMenu {
    const MAX_LENGTH: usize = 20;
    let by_name = |a: &ChooserEntry, b: &ChooserEntry| a.1.to_lowercase().cmp(&b.1.to_lowercase());
    if columns.len() < MAX_LENGTH {
        return ChooserMenu {
            top: columns.to_vec(),
            all: Vec::new(),
            letters: Vec::new(),
        };
    }
    let top: Vec<ChooserEntry> = columns.iter().filter(|c| c.2).cloned().collect();
    let mut all: Vec<ChooserEntry> = columns.to_vec();
    all.sort_by(by_name);
    // Letter buckets in first-char order (the C# `entry.Text[0]`).
    let mut buckets: Vec<(char, Vec<ChooserEntry>)> = Vec::new();
    for entry in &all {
        let key = entry.1.chars().next().unwrap_or('\u{0}');
        match buckets.iter_mut().find(|(k, _)| *k == key) {
            Some(b) => b.1.push(entry.clone()),
            None => buckets.push((key, vec![entry.clone()])),
        }
    }
    buckets.sort_by_key(|(k, _)| *k);
    // The C# merge: a run stays open while its size + the next
    // bucket's size stays under the cap; the last letter always
    // closes its run.
    let mut letters: Vec<(String, Vec<ChooserEntry>)> = Vec::new();
    let mut num: Option<usize> = None;
    let mut num2 = 0usize;
    for (j, (key, entries)) in buckets.iter().enumerate() {
        if num.is_none() {
            num = Some(j);
        }
        num2 += entries.len();
        let next = buckets.get(j + 1).map(|b| b.1.len()).unwrap_or(0);
        if num2 + next < MAX_LENGTH && j != buckets.len() - 1 {
            continue;
        }
        let start = num.expect("run start");
        let label = if j == start {
            key.to_string()
        } else {
            format!("{}-{}", buckets[start].0, key)
        };
        let mut run: Vec<ChooserEntry> = buckets[start..=j]
            .iter()
            .flat_map(|(_, e)| e.iter().cloned())
            .collect();
        run.sort_by(by_name);
        letters.push((label, run));
        num2 = 0;
        num = None;
    }
    ChooserMenu { top, all, letters }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cr_core::xml::scalar::CrGuid;

    fn book() -> ComicBook {
        let mut b = ComicBook {
            id: CrGuid::from_bytes([7; 16]),
            ..Default::default()
        };
        b.info.series = "Batman".into();
        b.info.number = "1".into();
        b
    }

    #[test]
    fn default_columns_match_the_c_sharp_defaults() {
        let columns = default_columns();
        let visible: Vec<&str> = columns
            .iter()
            .filter(|c| c.visible)
            .map(|c| c.name)
            .collect();
        assert_eq!(
            visible,
            [
                "State",
                "Position",
                "Checked",
                "Cover",
                "Series",
                "Number",
                "Volume",
                "Opened",
                "Added",
                "Pages",
                "Published",
                "Writer",
                "My Rating"
            ]
        );
        let series = columns.iter().find(|c| c.name == "Series").unwrap();
        assert_eq!(
            (series.id, series.property, series.width),
            (1, "Series", 200.0)
        );
    }

    #[test]
    fn cell_text_resolves_through_the_registry() {
        let columns = default_columns();
        let b = book();
        let series = columns.iter().find(|c| c.name == "Series").unwrap();
        assert_eq!(cell_text(series, &b), "Batman");
        let number = columns.iter().find(|c| c.name == "Number").unwrap();
        assert_eq!(cell_text(number, &b), "1");
        // Cover/State draw images, not text.
        let cover = columns.iter().find(|c| c.name == "Cover").unwrap();
        assert_eq!(cell_text(cover, &b), "");
    }

    fn chooser_entries() -> Vec<ChooserEntry> {
        default_columns()
            .iter()
            .map(|c| (c.id, c.name.to_string(), c.visible))
            .collect()
    }

    #[test]
    fn chooser_menu_matches_the_c_sharp_letter_groups() {
        let entries = chooser_entries();
        let menu = chooser_menu(&entries);
        // The `ContextMenuBuilder.Create(20)` run-merge output over
        // the full CR column table (the user's CR menu).
        let labels: Vec<&str> = menu.letters.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(labels, ["A-B", "C-F", "G-O", "P-R", "S", "T-Y"]);
        // The top level = the 13 visible defaults, column order.
        assert_eq!(menu.top.len(), 13);
        assert_eq!(menu.top.first().map(|e| e.1.as_str()), Some("State"));
        assert_eq!(menu.top.last().map(|e| e.1.as_str()), Some("My Rating"));
        // All = every column, alphabetical (case-insensitive).
        assert_eq!(menu.all.len(), entries.len());
        let mut sorted = menu.all.clone();
        sorted.sort_by_key(|e| e.1.to_lowercase());
        assert_eq!(menu.all, sorted);
        // Entries inside a letter run are alphabetical, and every
        // column lands in exactly one run.
        for (_, run) in &menu.letters {
            let mut sorted = run.clone();
            sorted.sort_by_key(|e| e.1.to_lowercase());
            assert_eq!(run, &sorted);
        }
        assert_eq!(
            menu.letters.iter().map(|(_, r)| r.len()).sum::<usize>(),
            entries.len()
        );
    }

    #[test]
    fn chooser_menu_under_twenty_entries_is_flat() {
        let entries: Vec<ChooserEntry> =
            (0..5).map(|i| (i, format!("Col{i}"), i % 2 == 0)).collect();
        let menu = chooser_menu(&entries);
        assert_eq!(menu.top.len(), 5);
        assert!(menu.all.is_empty() && menu.letters.is_empty());
    }
}
