//! The browser display text — `ComicBook.GetPropertyValue` with
//! `proposed: true` for the column `DisplayProperty` names: the
//! shadow/computed forms (`Series` resolves through
//! `ShadowSeries`/proposed, `NumberAsText` through
//! `ShadowNumberAsText`, ...) with the registry as the base-source
//! fallback. Unknown names render empty (the C# returns `DefaultText`).

use cr_core::model::comic_book::ComicBook;
use cr_core::registry::{self, PropValue};

use crate::matcher::book_view;

/// `ComicBook.FormatVolume`: −1 → empty, else `V{volume}`.
pub fn format_volume(volume: i32) -> String {
    if volume != -1 {
        format!("V{volume}")
    } else {
        String::new()
    }
}

/// `ComicBook.FormatYear`: −1 → empty, else the year.
pub fn format_year(year: i32) -> String {
    if year != -1 {
        year.to_string()
    } else {
        String::new()
    }
}

fn date_text(d: &cr_core::xml::scalar::CrDateTime) -> String {
    // The default short date column format (`ComicDateFormat`
    // default); the per-column format pickers arrive in Phase 5.
    if d.naive <= cr_core::xml::scalar::CrDateTime::min_value().naive {
        return String::new();
    }
    d.naive.format("%Y-%m-%d").to_string()
}

/// The column text for one `DisplayProperty` name.
pub fn column_text(book: &ComicBook, name: &str) -> String {
    let prop = book_view::proposed(book);
    match name {
        "Series" => book_view::shadow_series(book, &prop).to_string(),
        "Title" => book_view::shadow_title(book, &prop).to_string(),
        "NumberAsText" => book_view::shadow_number(book, &prop).to_string(),
        "VolumeAsText" => format_volume(book_view::shadow_volume(book, &prop)),
        "YearAsText" => format_year(book_view::shadow_year(book, &prop)),
        "CountAsText" => format_year(book_view::shadow_count(book, &prop)),
        "PagesAsTextSimple" => {
            if book.info.page_count > 0 {
                book.info.page_count.to_string()
            } else {
                String::new()
            }
        }
        "OpenedCountAsText" => book.opened_count.to_string(),
        "ReadPercentageAsText" => book_view::read_percentage(book).to_string(),
        "PublishedAsText" => date_text(&book_view::published(book, &prop)),
        "OpenedTime" | "AddedTime" | "FileModifiedTime" | "FileCreationTime" | "ReleasedTime" => {
            match registry::get(book, name) {
                Some(PropValue::Date(d)) => date_text(&d),
                _ => String::new(),
            }
        }
        "Caption" => {
            // The caption stand-in (the exact C# `FormatTitle` output
            // rides on the browser settings — Phase 5).
            let series = book_view::shadow_series(book, &prop);
            let number = book_view::shadow_number(book, &prop);
            if series.is_empty() {
                return String::new();
            }
            if number.is_empty() {
                return series.to_string();
            }
            format!("{series} #{number}")
        }
        "Rating" => match registry::get(book, "Rating") {
            Some(PropValue::Float(f)) if f != -1.0 => cr_core::xml::scalar::net_f32(f),
            _ => String::new(),
        },
        "Checked" => match book_view::yesno_checked(book) {
            cr_core::model::enums::YesNo::Yes => "Yes".into(),
            cr_core::model::enums::YesNo::No => "No".into(),
            cr_core::model::enums::YesNo::Unknown => "Unknown".into(),
        },
        "LanguageAsText" => book_view::language_as_text(book),
        "FilePath" => book.file_path.clone(),
        "FileName" | "FileDirectory" => {
            let path = std::path::Path::new(&book.file_path);
            match name {
                "FileName" => path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                _ => path
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            }
        }
        other => match registry::get(book, other) {
            Some(PropValue::Str(s)) => s,
            Some(PropValue::Int(i)) => i.to_string(),
            Some(PropValue::Float(f)) => cr_core::xml::scalar::net_f32(f),
            Some(PropValue::Bool(b)) => {
                if b {
                    "Yes".into()
                } else {
                    "No".into()
                }
            }
            Some(PropValue::Date(d)) => date_text(&d),
            _ => String::new(),
        },
    }
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
        b.info.volume = 2005;
        b.info.year = 2005;
        b.info.page_count = 22;
        b
    }

    #[test]
    fn as_text_names_format_like_the_c_sharp() {
        let b = book();
        assert_eq!(column_text(&b, "Series"), "Batman");
        assert_eq!(column_text(&b, "NumberAsText"), "1");
        assert_eq!(column_text(&b, "VolumeAsText"), "V2005");
        assert_eq!(column_text(&b, "YearAsText"), "2005");
        assert_eq!(column_text(&b, "PagesAsTextSimple"), "22");
        // Unset values render empty (FormatVolume/FormatYear −1).
        let empty = ComicBook {
            id: CrGuid::from_bytes([8; 16]),
            ..Default::default()
        };
        assert_eq!(column_text(&empty, "VolumeAsText"), "");
        assert_eq!(column_text(&empty, "YearAsText"), "");
        assert_eq!(column_text(&empty, "PagesAsTextSimple"), "");
    }

    #[test]
    fn shadow_fallback_uses_the_proposed_names() {
        // No stored series: the proposed (filename-parsed) series
        // shows — `proposed: true` parity.
        let b = ComicBook {
            id: CrGuid::from_bytes([9; 16]),
            file_path: "/comics/Batman 001 (2024).cbz".into(),
            ..Default::default()
        };
        let text = column_text(&b, "Series");
        assert!(
            text.to_lowercase().contains("batman"),
            "proposed series fallback failed: {text:?}"
        );
    }

    #[test]
    fn caption_stands_in_until_the_settings_port() {
        let b = book();
        assert_eq!(column_text(&b, "Caption"), "Batman #1");
    }
}
