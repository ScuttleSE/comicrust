//! Computed `ComicBook` values the matchers read (`Shadow*`, `Published`,
//! `ReadPercentage`, `Week`, ...) — the Rust counterpart of the derived
//! members in `ComicBook.cs`.
//!
//! The proposed-value fallbacks (`ShadowSeries` etc. when the stored
//! value is empty and `EnableProposed` is on) use the `ComicNameInfo`
//! filename parse, like `ComicBook.OnParseFilePath`.

use chrono::Datelike;

use cr_core::model::comic_book::ComicBook;
use cr_core::model::comic_name_info::{self, ComicNameInfo};
use cr_core::model::enums::{MangaYesNo, YesNo};
use cr_core::xml::scalar::CrDateTime;

use super::text_number::parse_comic_number;

/// `ComicBook.ReadPercentage`: 0 when nothing read; `((LastPageRead + 1)
/// * 100 / PageCount).Clamp(1, 100)` otherwise.
pub fn read_percentage(book: &ComicBook) -> i32 {
    let page_count = book.info.page_count;
    let last = book.last_page_read;
    if page_count <= 0 || last <= 0 {
        return 0;
    }
    (((last + 1) * 100) / page_count).clamp(1, 100)
}

/// `ComicBook.HasBeenRead` (`ReadPercentage >= ReadPercentageAsRead`, 95).
pub fn has_been_read(book: &ComicBook) -> bool {
    read_percentage(book) >= 95
}

/// The proposed values parsed from the file name.
pub fn proposed(book: &ComicBook) -> ComicNameInfo {
    comic_name_info::from_file_path(&book.file_path)
}

/// Whether ANY shadow field can fall through to the file-name parse
/// (`EnableProposed` + the stored value empty). The proposed parse is
/// skipped entirely when this is false — every shadow accessor then
/// reads the stored info, so the parsed value is dead. The Phase 8
/// perf work (sort keys, groupers, duplicates, the matcher context)
/// builds parses through this gate instead of per call/comparison.
pub fn needs_prop(book: &ComicBook) -> bool {
    book.enable_proposed
        && (book.info.series.is_empty()
            || book.info.title.is_empty()
            || book.info.number.is_empty()
            || book.info.format.is_empty()
            || book.info.count == -1
            || book.info.volume == -1
            || book.info.year == -1)
}

/// The dead parse for `needs_prop == false` books (never read — every
/// shadow accessor returns the stored value before consulting it).
pub fn empty_prop() -> &'static ComicNameInfo {
    static E: std::sync::OnceLock<ComicNameInfo> = std::sync::OnceLock::new();
    E.get_or_init(ComicNameInfo::new)
}

/// The proposed parses of a book slice, computed LAZILY — on first
/// read, one parse per book per operation (the C# `ComicBook.Proposed`
/// instance cache parses on first access the same way). A dead book
/// (`needs_prop` false) resolves to the shared empty parse and never
/// parses, so a metadata-complete library or an unsorted/grouped view
/// (no getter ever reads) parses nothing.
pub struct PropTable {
    slots: Vec<std::cell::OnceCell<ComicNameInfo>>,
    dead: Vec<bool>,
}

impl PropTable {
    /// The table shape for `books` (no parses run here).
    pub fn build(books: &[ComicBook]) -> Self {
        PropTable {
            slots: (0..books.len())
                .map(|_| std::cell::OnceCell::new())
                .collect(),
            dead: books.iter().map(|b| !needs_prop(b)).collect(),
        }
    }

    /// The proposed parse of `books[i]` — `book` MUST be the book at
    /// index `i` of the slice the table was built from. The returned
    /// reference borrows the table (shared; several may coexist).
    pub fn get(&self, i: usize, book: &ComicBook) -> &ComicNameInfo {
        if self.dead[i] {
            return empty_prop();
        }
        self.slots[i].get_or_init(|| proposed(book))
    }
}

/// `ComicBook.ShadowSeries`.
pub fn shadow_series<'a>(book: &'a ComicBook, prop: &'a ComicNameInfo) -> &'a str {
    if !book.enable_proposed || !book.info.series.is_empty() {
        &book.info.series
    } else {
        &prop.series
    }
}

/// `ComicBook.ShadowTitle`.
pub fn shadow_title<'a>(book: &'a ComicBook, prop: &'a ComicNameInfo) -> &'a str {
    if !book.enable_proposed || !book.info.title.is_empty() {
        &book.info.title
    } else {
        &prop.title
    }
}

/// `ComicBook.ShadowFormat`.
pub fn shadow_format<'a>(book: &'a ComicBook, prop: &'a ComicNameInfo) -> &'a str {
    if !book.enable_proposed || !book.info.format.is_empty() {
        &book.info.format
    } else {
        &prop.format
    }
}

/// `ComicBook.ShadowVolume`.
pub fn shadow_volume(book: &ComicBook, prop: &ComicNameInfo) -> i32 {
    if !book.enable_proposed || book.info.volume != -1 {
        book.info.volume
    } else {
        prop.volume
    }
}

/// `ComicBook.ShadowCount`.
pub fn shadow_count(book: &ComicBook, prop: &ComicNameInfo) -> i32 {
    if !book.enable_proposed || book.info.count != -1 {
        book.info.count
    } else {
        prop.count
    }
}

/// `ComicBook.ShadowYear`.
pub fn shadow_year(book: &ComicBook, prop: &ComicNameInfo) -> i32 {
    if !book.enable_proposed || book.info.year != -1 {
        book.info.year
    } else {
        prop.year
    }
}

/// `ComicBook.ShadowNumber` (with the `ProposedNumber` "-" guard).
pub fn shadow_number<'a>(book: &'a ComicBook, prop: &'a ComicNameInfo) -> &'a str {
    if !book.enable_proposed || !book.info.number.is_empty() {
        &book.info.number
    } else if book.info.number == "-" {
        ""
    } else {
        &prop.number
    }
}

/// `ComicBook.IsLinked`.
pub fn is_linked(book: &ComicBook) -> bool {
    !book.file_path.is_empty()
}

/// `ComicBook.Published` (from the shadow year; `DateTime.MinValue` when
/// the year is unset).
pub fn published(book: &ComicBook, prop: &ComicNameInfo) -> CrDateTime {
    let year = shadow_year(book, prop);
    if year <= 0 {
        return CrDateTime::min_value();
    }
    let year = year.clamp(1, 10000);
    let month = book.info.month.clamp(1, 12);
    let day = book.info.day.clamp(1, days_in_month(year, month) as i32);
    let date = chrono::NaiveDate::from_ymd_opt(year, month as u32, day as u32)
        .or_else(|| chrono::NaiveDate::from_ymd_opt(year, month as u32, 1))
        .unwrap_or(CrDateTime::min_value().naive.date());
    CrDateTime {
        naive: date.and_hms_opt(0, 0, 0).expect("midnight"),
        kind: cr_core::xml::scalar::DateKind::Unspecified,
    }
}

fn days_in_month(year: i32, month: i32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap(year) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// `ComicBook.Week`: `GetWeekOfYear(Published, CalendarWeekRule.FirstDay,
/// DayOfWeek.Monday)`; -1 when never published.
///
/// `CalendarWeekRule.FirstDay` semantics: week 1 starts on Jan 1 (at
/// whatever weekday) and runs to the day before the next Monday; after
/// that, weeks start on Monday.
pub fn week(book: &ComicBook, prop: &ComicNameInfo) -> i32 {
    let published = published(book, prop);
    if published.is_min_value() {
        return -1;
    }
    let date = published.naive.date();
    let jan1 = chrono::NaiveDate::from_ymd_opt(date.year(), 1, 1).expect("jan 1");
    let d = (date - jan1).num_days() as i32;
    // Days Jan 1 is past the Monday-start of its own week: the first
    // week of the year is only that long.
    let jan1_weekday = jan1.weekday().num_days_from_monday() as i32;
    let first_week_len = (7 - jan1_weekday) % 7;
    let first_week_len = if first_week_len == 0 {
        7
    } else {
        first_week_len
    };
    if d < first_week_len {
        1
    } else {
        2 + (d - first_week_len) / 7
    }
}

/// `ComicBook.LanguageAsText`: the neutral-culture display name of the
/// ISO code (empty when unknown). The C# resolves via `CultureInfo`
/// neutral cultures; the table covers the common ISO-639-1 codes.
pub fn language_as_text(book: &ComicBook) -> String {
    let iso = book.info.language_iso.trim().to_lowercase();
    if iso.is_empty() {
        return String::new();
    }
    language_name(&iso).unwrap_or_default().to_string()
}

fn language_name(iso: &str) -> Option<&'static str> {
    Some(match iso {
        "aa" => "Afar",
        "ab" => "Abkhazian",
        "af" => "Afrikaans",
        "ak" => "Akan",
        "am" => "Amharic",
        "ar" => "Arabic",
        "an" => "Aragonese",
        "as" => "Assamese",
        "av" => "Avaric",
        "ae" => "Avestan",
        "ay" => "Aymara",
        "az" => "Azerbaijani",
        "ba" => "Bashkir",
        "bm" => "Bambara",
        "be" => "Belarusian",
        "bn" => "Bengali",
        "bh" => "Bihari",
        "bi" => "Bislama",
        "bo" => "Tibetan",
        "bs" => "Bosnian",
        "br" => "Breton",
        "bg" => "Bulgarian",
        "ca" => "Catalan",
        "cs" => "Czech",
        "ch" => "Chamorro",
        "ce" => "Chechen",
        "cu" => "Church Slavic",
        "cv" => "Chuvash",
        "kw" => "Cornish",
        "co" => "Corsican",
        "cr" => "Cree",
        "cy" => "Welsh",
        "da" => "Danish",
        "de" => "German",
        "dv" => "Divehi",
        "dz" => "Dzongkha",
        "el" => "Greek",
        "en" => "English",
        "eo" => "Esperanto",
        "es" => "Spanish",
        "et" => "Estonian",
        "eu" => "Basque",
        "fa" => "Persian",
        "fo" => "Faroese",
        "fr" => "French",
        "fy" => "Frisian",
        "ga" => "Irish",
        "gd" => "Gaelic",
        "gl" => "Galician",
        "gn" => "Guarani",
        "gu" => "Gujarati",
        "gv" => "Manx",
        "ha" => "Hausa",
        "he" => "Hebrew",
        "hi" => "Hindi",
        "ho" => "Hiri Motu",
        "hr" => "Croatian",
        "hu" => "Hungarian",
        "hy" => "Armenian",
        "hz" => "Herero",
        "ia" => "Interlingua",
        "id" => "Indonesian",
        "ie" => "Interlingue",
        "ig" => "Igbo",
        "ii" => "Sichuan Yi",
        "ik" => "Inupiaq",
        "io" => "Ido",
        "is" => "Icelandic",
        "it" => "Italian",
        "iu" => "Inuktitut",
        "ja" => "Japanese",
        "jv" => "Javanese",
        "ka" => "Georgian",
        "kg" => "Kongo",
        "ki" => "Kikuyu",
        "kj" => "Kuanyama",
        "kk" => "Kazakh",
        "kl" => "Greenlandic",
        "km" => "Khmer",
        "kn" => "Kannada",
        "ko" => "Korean",
        "kr" => "Kanuri",
        "ks" => "Kashmiri",
        "ku" => "Kurdish",
        "kv" => "Komi",
        "ky" => "Kyrgyz",
        "la" => "Latin",
        "lb" => "Luxembourgish",
        "lg" => "Ganda",
        "li" => "Limburgish",
        "ln" => "Lingala",
        "lo" => "Lao",
        "lt" => "Lithuanian",
        "lu" => "Luba-Katanga",
        "lv" => "Latvian",
        "mg" => "Malagasy",
        "mh" => "Marshallese",
        "mi" => "Maori",
        "mk" => "Macedonian",
        "ml" => "Malayalam",
        "mn" => "Mongolian",
        "mr" => "Marathi",
        "ms" => "Malay",
        "mt" => "Maltese",
        "my" => "Burmese",
        "na" => "Nauru",
        "nv" => "Navajo",
        "ng" => "Ndonga",
        "nd" => "Ndebele",
        "ne" => "Nepali",
        "nl" => "Dutch",
        "nn" => "Norwegian Nynorsk",
        "nb" => "Norwegian Bokmål",
        "no" => "Norwegian",
        "ny" => "Chichewa",
        "oc" => "Occitan",
        "oj" => "Ojibwa",
        "or" => "Oriya",
        "om" => "Oromo",
        "os" => "Ossetian",
        "pa" => "Punjabi",
        "pi" => "Pali",
        "pl" => "Polish",
        "pt" => "Portuguese",
        "ps" => "Pashto",
        "qu" => "Quechua",
        "rm" => "Romansh",
        "ro" => "Romanian",
        "rn" => "Rundi",
        "ru" => "Russian",
        "rw" => "Kinyarwanda",
        "sa" => "Sanskrit",
        "sc" => "Sardinian",
        "sd" => "Sindhi",
        "se" => "Sami",
        "sg" => "Sango",
        "si" => "Sinhala",
        "sk" => "Slovak",
        "sl" => "Slovenian",
        "sm" => "Samoan",
        "sn" => "Shona",
        "so" => "Somali",
        "sq" => "Albanian",
        "sr" => "Serbian",
        "ss" => "Swati",
        "st" => "Sotho",
        "su" => "Sundanese",
        "sv" => "Swedish",
        "sw" => "Swahili",
        "ta" => "Tamil",
        "te" => "Telugu",
        "tg" => "Tajik",
        "th" => "Thai",
        "ti" => "Tigrinya",
        "tk" => "Turkmen",
        "tl" => "Tagalog",
        "tn" => "Tswana",
        "to" => "Tonga",
        "tr" => "Turkish",
        "ts" => "Tsonga",
        "tt" => "Tatar",
        "tw" => "Twi",
        "ty" => "Tahitian",
        "ug" => "Uyghur",
        "uk" => "Ukrainian",
        "ur" => "Urdu",
        "uz" => "Uzbek",
        "ve" => "Venda",
        "vi" => "Vietnamese",
        "vo" => "Volapük",
        "wa" => "Walloon",
        "wo" => "Wolof",
        "xh" => "Xhosa",
        "yi" => "Yiddish",
        "yo" => "Yoruba",
        "za" => "Zhuang",
        "zh" => "Chinese",
        "zu" => "Zulu",
        _ => return None,
    })
}

/// `ValuesStore.GetValue(CustomValuesStore, key)` (case-insensitive key).
pub fn custom_value(book: &ComicBook, key: &str) -> Option<String> {
    cr_core::model::comic_book::values_store::decode(&book.custom_values_store)
        .into_iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(key))
        .map(|(_, v)| v)
}

/// `ComicBook.GetCustomValues()`.
pub fn custom_values(book: &ComicBook) -> Vec<(String, String)> {
    cr_core::model::comic_book::values_store::decode(&book.custom_values_store)
}

/// `ComicBook.CompareNumber` (ComicTextNumberFloat): `(IsNumber, Number)`.
pub fn compare_number(book: &ComicBook, prop: &ComicNameInfo) -> (bool, f32) {
    parse_comic_number(shadow_number(book, prop))
}

// ---------- YesNo views (the ComicBookYesNoMatcher GetValue methods) ----------

pub fn yesno_checked(book: &ComicBook) -> YesNo {
    if book.checked {
        YesNo::Yes
    } else {
        YesNo::No
    }
}

pub fn yesno_has_custom_values(book: &ComicBook) -> YesNo {
    if custom_values(book).is_empty() {
        YesNo::No
    } else {
        YesNo::Yes
    }
}

pub fn yesno_is_linked(book: &ComicBook) -> YesNo {
    if is_linked(book) {
        YesNo::Yes
    } else {
        YesNo::No
    }
}

pub fn yesno_is_missing(book: &ComicBook) -> YesNo {
    if is_linked(book) && book.file_is_missing {
        YesNo::Yes
    } else {
        YesNo::No
    }
}

pub fn yesno_modified_info(book: &ComicBook) -> YesNo {
    if book.comic_info_is_dirty {
        YesNo::Yes
    } else {
        YesNo::No
    }
}

pub fn yesno_modified_library_info(book: &ComicBook) -> YesNo {
    if book.comic_book_is_dirty {
        YesNo::Yes
    } else {
        YesNo::No
    }
}

pub fn yesno_black_and_white(book: &ComicBook) -> YesNo {
    book.info.black_and_white
}

pub fn yesno_series_complete(book: &ComicBook) -> YesNo {
    book.series_complete
}

pub fn manga_yesno(book: &ComicBook) -> MangaYesNo {
    book.info.manga
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_percentage_matches_csharp() {
        let mut b = ComicBook::default();
        b.info.page_count = 10;
        b.last_page_read = 4;
        assert_eq!(read_percentage(&b), 50); // (4+1)*100/10
        b.last_page_read = 0;
        assert_eq!(read_percentage(&b), 0);
        b.last_page_read = 9;
        assert_eq!(read_percentage(&b), 100);
        b.last_page_read = 100;
        assert_eq!(read_percentage(&b), 100);
        b.info.page_count = 0;
        assert_eq!(read_percentage(&b), 0);
    }

    #[test]
    fn week_matches_first_day_monday() {
        let mut b = ComicBook::default();
        b.info.year = 2024;
        b.enable_proposed = false;
        // 2024-01-01 is a Monday: week 1 is a full week.
        assert_eq!(week_of(&mut b, 1, 1), 1);
        assert_eq!(week_of(&mut b, 1, 8), 2);
        // 2024-12-31: d = 365, first_week_len = 7 → 2 + 358/7 = 53.
        assert_eq!(week_of(&mut b, 12, 31), 53);
        b.info.year = 2023;
        // 2023-01-01 is a Sunday: week 1 is one day long.
        assert_eq!(week_of(&mut b, 1, 1), 1);
        assert_eq!(week_of(&mut b, 1, 2), 2);
        assert_eq!(week_of(&mut b, 12, 31), 53);
    }

    fn week_of(book: &mut ComicBook, month: i32, day: i32) -> i32 {
        book.info.month = month;
        book.info.day = day;
        let prop = proposed(book);
        week(book, &prop)
    }

    #[test]
    fn published_clamps() {
        let mut b = ComicBook::default();
        b.info.year = 2000;
        b.info.month = 13; // clamps to 12
        b.info.day = 31;
        let prop = proposed(&b);
        let p = published(&b, &prop);
        assert_eq!(p.naive.date().month(), 12);
        assert_eq!(p.naive.date().day(), 31);
        b.info.month = 2;
        b.info.day = 30; // clamps to 29 (2000 is a leap year)
        let p = published(&b, &prop);
        assert_eq!(p.naive.date().day(), 29);
        b.info.year = -1; // no year → min value
        let prop = proposed(&b);
        assert!(published(&b, &prop).is_min_value());
    }

    #[test]
    fn language_names() {
        let mut b = ComicBook::default();
        b.info.language_iso = "en".into();
        assert_eq!(language_as_text(&b), "English");
        b.info.language_iso = "DE".into();
        assert_eq!(language_as_text(&b), "German");
        b.info.language_iso = "xyz".into();
        assert_eq!(language_as_text(&b), "");
        b.info.language_iso = String::new();
        assert_eq!(language_as_text(&b), "");
    }

    #[test]
    fn custom_values_lookup() {
        let b = ComicBook {
            custom_values_store: "Read=true,Location=Home".into(),
            ..Default::default()
        };
        assert_eq!(custom_value(&b, "read").as_deref(), Some("true"));
        assert_eq!(custom_value(&b, "Location").as_deref(), Some("Home"));
        assert_eq!(custom_value(&b, "Nope"), None);
    }
}
