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

/// The browser's default column set in creation order
/// (`ComicBrowserControl.cs:755-848`). Only the DEFAULT-VISIBLE
/// columns list `visible: true`; the rest ship hidden (the column
/// menu can reveal them — T5).
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
        column(5, "Opened", "OpenedTime", 40.0, Far, true),
        column(6, "Added", "AddedTime", 40.0, Far, true),
        column(7, "Pages", "PagesAsTextSimple", 40.0, Far, true),
        column(39, "Published", "PublishedAsText", 40.0, Far, true),
        column(11, "Writer", "Writer", 80.0, Near, true),
        column(15, "My Rating", "Rating", 50.0, Near, true),
        // Hidden by default (the full C# list, in creation order).
        column(4, "Title", "Title", 200.0, Near, false),
        column(9, "File Path", "FilePath", 200.0, Near, false),
        column(10, "File Name", "FileName", 200.0, Near, false),
        column(12, "Penciller", "Penciller", 80.0, Near, false),
        column(13, "Inker", "Inker", 80.0, Near, false),
        column(14, "Colorist", "Colorist", 80.0, Near, false),
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
        column(35, "B&W", "BlackAndWhite", 22.0, Near, false),
        column(36, "Manga", "Manga", 22.0, Near, false),
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
        column(70, "Released", "ReleasedTime", 60.0, Far, false),
        column(48, "Community Rating", "CommunityRating", 50.0, Near, false),
        column(58, "Series Complete", "SeriesComplete", 22.0, Near, false),
        column(61, "Read", "Read", 22.0, Near, false),
        column(64, "Story Arc", "StoryArc", 100.0, Near, false),
        column(65, "Series Group", "SeriesGroup", 100.0, Near, false),
        column(68, "Day", "DayAsText", 40.0, Far, false),
        column(69, "Week", "WeekAsText", 40.0, Far, false),
    ]
}

impl Column {
    pub fn is_text_column(&self) -> bool {
        !matches!(self.id, 0 | 101 | 62) && !self.property.is_empty()
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
}
