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

fn month_as_text(book: &ComicBook) -> String {
    if book.info.month != -1 {
        book.info.month.to_string()
    } else {
        String::new()
    }
}

fn day_as_text(book: &ComicBook) -> String {
    if book.info.day != -1 {
        book.info.day.to_string()
    } else {
        String::new()
    }
}

/// `FileLengthFormat`: "512 Bytes" / ".00 kB" / ".00 MB" / ".00 GB".
pub fn file_size_as_text(size: i64) -> String {
    if size < 0 {
        return "Unknown".into();
    }
    let f = size as f64;
    if size < 1024 {
        format!("{size} Bytes")
    } else if size < 1_048_576 {
        format!("{:.2} kB", f / 1024.0)
    } else if size < 1_073_741_824 {
        format!("{:.2} MB", f / 1024.0 / 1024.0)
    } else {
        format!("{:.2} GB", f / 1024.0 / 1024.0 / 1024.0)
    }
}

/// The column text for one `DisplayProperty` name.
pub fn column_text(book: &ComicBook, name: &str) -> String {
    let prop = book_view::proposed_cached(book);
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
        "FileSizeAsText" => file_size_as_text(book.file_size),
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

// ---------- The caption (`ComicBook.Caption`) ----------

/// `ComicBook.DefaultCaptionFormat` — the caption template.
pub const CAPTION_FORMAT: &str =
    "[{format} ][{series}][ {volume}][ #{number}][ - {title}][ ({year}[/{month}[/{day}]])]";

/// `ExtendedStringFormater.Format`: `[...]` groups emit iff every
/// DIRECT `{placeholder}` inside resolved non-empty (nested failed
/// groups do not fail the parent); `\x` escapes. The `$...<...>`
/// function form does not appear in the browser formats and is not
/// ported. Returns `(text, success)`.
pub fn extended_format(format: &str, resolver: &dyn Fn(&str) -> Option<String>) -> (String, bool) {
    let mut out = String::new();
    let mut success = true;
    let bytes: Vec<char> = format.chars().collect();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            '[' => {
                let (part, next) = matching_part(&bytes, i, '[', ']');
                i = next;
                let (text, ok) = extended_format(&part, resolver);
                if ok {
                    out.push_str(&text);
                }
                success |= ok;
            }
            '{' => {
                let (part, next) = matching_part(&bytes, i, '{', '}');
                i = next;
                // `name:numeric-format` — the caption format uses
                // bare names; the numeric format is ignored (the
                // values are strings).
                let name = part.split(':').next().unwrap_or(&part);
                match resolver(name) {
                    Some(v) if !v.is_empty() => out.push_str(&v),
                    _ => success = false,
                }
            }
            '\\' => {
                i += 1;
                if i < bytes.len() {
                    out.push(bytes[i]);
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    (out, success)
}

/// `GetPart`: the text between the matching bracket pair (nesting
/// counted; escapes already consumed by the caller's `\` case).
fn matching_part(chars: &[char], open_at: usize, open: char, close: char) -> (String, usize) {
    let mut depth = 0usize;
    let mut i = open_at;
    let start = open_at + 1;
    while i < chars.len() {
        let c = chars[i];
        if c == open {
            depth += 1;
        } else if c == close {
            depth -= 1;
            if depth == 0 {
                return (chars[start..i].iter().collect(), i);
            }
        }
        i += 1;
    }
    (chars[start..].iter().collect(), i)
}

/// The caption resolver (`GetPropertyValue<string>(name, Shadow)`
/// with `MapPropertyNameToAsText`).
fn caption_value(book: &ComicBook, name: &str, ignore: &[&str]) -> Option<String> {
    if ignore.iter().any(|i| i.eq_ignore_ascii_case(name)) {
        return None;
    }
    let prop = book_view::proposed_cached(book);
    let value = match name.to_ascii_lowercase().as_str() {
        "format" => book_view::shadow_format(book, &prop).to_string(),
        "series" => book_view::shadow_series(book, &prop).to_string(),
        "volume" => format_volume(book_view::shadow_volume(book, &prop)),
        "number" => book_view::shadow_number(book, &prop).to_string(),
        "title" => book_view::shadow_title(book, &prop).to_string(),
        "year" => format_year(book_view::shadow_year(book, &prop)),
        "month" => month_as_text(book),
        "day" => day_as_text(book),
        other => column_text(book, other),
    };
    Some(value).filter(|v| !v.is_empty())
}

/// `ComicBook.Caption` — the full title from the caption format.
pub fn caption(book: &ComicBook) -> String {
    extended_format(CAPTION_FORMAT, &|name| caption_value(book, name, &[])).0
}

/// `ComicBook.CaptionWithoutTitle`.
pub fn caption_without_title(book: &ComicBook) -> String {
    extended_format(CAPTION_FORMAT, &|name| {
        caption_value(book, name, &["title"])
    })
    .0
}

/// `ComicBook.CaptionWithoutFormat`.
pub fn caption_without_format(book: &ComicBook) -> String {
    extended_format(CAPTION_FORMAT, &|name| {
        caption_value(book, name, &["format"])
    })
    .0
}

#[cfg(test)]
mod caption_tests {
    use super::*;
    use cr_core::xml::scalar::CrGuid;

    fn book() -> ComicBook {
        let mut b = ComicBook {
            id: CrGuid::from_bytes([3; 16]),
            ..Default::default()
        };
        b.info.series = "Batman".into();
        b.info.number = "12".into();
        b.info.title = "The Cape".into();
        b.info.volume = 2005;
        b.info.year = 2008;
        b.info.month = 5;
        b
    }

    #[test]
    fn caption_matches_the_c_sharp_format() {
        let b = book();
        assert_eq!(caption(&b), "Batman V2005 #12 - The Cape (2008/5)");
        // The book-format group leads when the format is set.
        let mut bf = book();
        bf.info.format = "Normal".into();
        assert_eq!(caption(&bf), "Normal Batman V2005 #12 - The Cape (2008/5)");
        // Degradation: no month → (2008); no volume → the group
        // disappears; no title → the "- title" group disappears.
        let mut b2 = book();
        b2.info.month = -1;
        b2.info.volume = -1;
        b2.info.title = String::new();
        assert_eq!(caption(&b2), "Batman #12 (2008)");
        // Bare minimum: series + number only.
        let mut b3 = ComicBook {
            id: CrGuid::from_bytes([4; 16]),
            ..Default::default()
        };
        b3.info.series = "Foo".into();
        b3.info.number = "3".into();
        assert_eq!(caption(&b3), "Foo #3");
    }

    #[test]
    fn caption_without_title_drops_the_title_group() {
        let b = book();
        assert_eq!(caption_without_title(&b), "Batman V2005 #12 (2008/5)");
        assert_eq!(
            caption_without_format(&b),
            "Batman V2005 #12 - The Cape (2008/5)"
        );
    }
}
