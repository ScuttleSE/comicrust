//! `ComicNameInfo.FromFilePath` — the filename parser port
//! (`ComicNameInfo.cs`, NewParser and LegacyParser).
//!
//! .NET `RegexOptions.RightToLeft` is emulated by taking the LAST match
//! of the pattern. Lookarounds need the `fancy-regex` engine.

use fancy_regex::Regex;
use std::sync::OnceLock;

#[derive(Clone, Debug, PartialEq, Default)]
pub struct ComicNameInfo {
    pub series: String,
    pub title: String,
    pub number: String,
    pub format: String,
    pub volume: i32,
    pub count: i32,
    pub year: i32,
    pub cover_count: i32,
}

impl ComicNameInfo {
    pub fn new() -> Self {
        ComicNameInfo {
            series: String::new(),
            title: String::new(),
            number: String::new(),
            format: String::new(),
            volume: -1,
            count: -1,
            year: -1,
            cover_count: 1,
        }
    }
}

fn regex(pattern: &str) -> Regex {
    // RegexOptions.IgnoreCase | Singleline
    Regex::new(&format!("(?is){pattern}")).expect("static regex")
}

fn rx_dot_replace() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| regex(r#"((?<!\d)\.|\.(?!\d)|_)"#))
}

fn rx_remove() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| regex(r"\b(ctc|c2c|\d+p|\d{1,2}\-\d{1,2}|\d{1,2}\-(?=\d{4}))\b"))
}

fn rx_brackets() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| regex(r"\(.*?\)|\[.*?\]"))
}

fn rx_count(extra: &str) -> Regex {
    regex(&format!(r"\b({extra})\s*\d+\b"))
}

fn rx_volume() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| regex(r"\b(v|vol\.?|volume)\s*\d+\b"))
}

fn rx_year() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| regex(r"\b(?<!#)(19|2[0-3])\d\d\b(?!\spa)"))
}

fn rx_year_with_month() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| regex(r"(19|2[0-3])\d\d[-/\\\s]\d{1,2}\b"))
}

fn rx_format() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        regex(
            r"\b(annual|director's cut|preview|b(lack)?\s*&\s*w(hite)?|king\s*size|giant\s*size)|sketch\b",
        )
    })
}

fn rx_cover_count() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| regex(r"(?<covers>\d+)\s+cover"))
}

fn rx_num() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| regex(r"\d+\.?\d*"))
}

fn rx_number() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    // The C# pattern has a variable-length lookbehind `(?<!part\s+)`,
    // unsupported here; the guard is applied in `last_match_guard`.
    R.get_or_init(|| regex(r"(\b|#|(c\w*\s*))\d[\d\.]*\b(?!\s*(pa|cov))"))
}

fn rx_part_suffix() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| regex(r"part\s+$"))
}

// Legacy parser patterns
fn rx_series() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        regex(r#"^(\d+)?(([&\w\s'-])(?!v\d|(?<=[ #])(\d(?!\d*\s[#\d]))+(?=(\W|$))(?!\))))*"#)
    })
}

fn rx_legacy_volume() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| regex(r"(?<=\bv)\d(?=\b)"))
}

fn rx_legacy_number() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    // C# lookbehind `(?<=[ #]|c|ch)` is variable-length; emulated with a
    // prefix guard in `last_match_guard`.
    R.get_or_init(|| regex(r"(\d(?!\d*\s[#\d]))+(?=(\W|$))(?!\))"))
}

fn rx_legacy_count() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| regex(r"(?<=[\(\[\s]of\s)\d+"))
}

fn rx_legacy_year() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| regex(r"(?<=[\(\[])\d{4}\b"))
}

/// Last (RightToLeft) match span of `rx` in `text`.
fn last_match(rx: &Regex, text: &str) -> Option<(usize, usize)> {
    let mut found = None;
    for m in rx.find_iter(text) {
        match m {
            Ok(m) => found = Some((m.start(), m.end())),
            Err(_) => break,
        }
    }
    found
}

/// Last match of `rx` whose start position satisfies `guard(prefix)`.
/// Emulates lookbehind/lookahead constraints fancy-regex cannot express.
fn last_match_guard(
    rx: &Regex,
    text: &str,
    guard: impl Fn(&str) -> bool,
) -> Option<(usize, usize)> {
    let mut found = None;
    for m in rx.find_iter(text) {
        let m = match m {
            Ok(m) => m,
            Err(_) => break,
        };
        if guard(&text[..m.start()]) {
            found = Some((m.start(), m.end()));
        }
    }
    found
}

/// Lookbehind guard for rxNumber: `(?<!part\s+)` — the prefix must NOT
/// end with `part` + whitespace.
fn number_guard(prefix: &str) -> bool {
    !rx_part_suffix().is_match(prefix).unwrap_or(false)
}

/// Lookbehind guard for the legacy number: the prefix must end with one
/// of `[ #]`, `c`, `ch`.
fn legacy_number_guard(prefix: &str) -> bool {
    prefix.ends_with(' ')
        || prefix.ends_with('#')
        || prefix.ends_with('c')
        || prefix.ends_with("ch")
}

/// First match span of `rx` in `text`.
fn first_match(rx: &Regex, text: &str) -> Option<(usize, usize)> {
    match rx.find(text) {
        Ok(Some(m)) => Some((m.start(), m.end())),
        _ => None,
    }
}

/// `GetNumber`: the last embedded number of `text`.
fn get_number(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    match last_match(rx_num(), text) {
        Some((s, e)) => text[s..e].to_string(),
        None => String::new(),
    }
}

/// `Path.GetFileNameWithoutExtension` (Windows semantics; both
/// separators tolerated).
pub fn file_name_without_extension(path: &str) -> String {
    let name = {
        let idx = path.rfind(['/', '\\']).map(|i| i + 1).unwrap_or(0);
        &path[idx..]
    };
    match name.rfind('.') {
        Some(i) if i > 0 => name[..i].to_string(),
        _ => name.to_string(),
    }
}

/// `Path.GetDirectoryName` (Windows semantics).
pub fn directory_name(path: &str) -> String {
    match path.rfind(['/', '\\']) {
        Some(i) if i > 0 => path[..i].to_string(),
        Some(0) => path[..1].to_string(),
        _ => String::new(),
    }
}

/// `StringUtility.CutOff`: substring before the first delimiter.
fn cut_off(text: &str, delimiters: &[char]) -> String {
    match text.find(delimiters) {
        Some(i) => text[..i].to_string(),
        None => text.to_string(),
    }
}

/// `StringUtility.IsNumber`: every char is a digit (empty is true).
fn is_number(text: &str) -> bool {
    text.chars().all(|c| c.is_ascii_digit())
}

/// Normalizes a number string like `float.TryParse` + `ToString`.
fn normalize_number(s: &str) -> String {
    match s.trim().parse::<f32>() {
        Ok(f) => {
            if f.fract() == 0.0 && f.abs() < 1e7 {
                format!("{}", f as i64)
            } else {
                format!("{f}")
            }
        }
        Err(_) => s.to_string(),
    }
}

/// The NewParser entry point (the default `LegacyFilenameParser=false`
/// path). `of_values` is `EngineConfiguration.Default.OfValues`.
pub fn from_file_path_with_of(path: &str, of_values: &str) -> ComicNameInfo {
    let mut info = ComicNameInfo::new();
    let mut text = file_name_without_extension(path);
    // If the name contains spaces and no underscores, dots stay.
    if !text.contains(' ') || text.contains('_') {
        text = rx_dot_replace().replace_all(&text, " ").into_owned();
    }
    text = rx_remove().replace_all(&text, "").into_owned();

    let of_pattern = of_values
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("|");
    let count_rx = rx_count(&of_pattern);
    if let Some((s, e)) = last_match(&count_rx, &text) {
        let value = text[s..e].to_string();
        text = format!("{}{}", &text[..s], &text[e..]);
        info.count = get_number(&value).trim().parse().unwrap_or(-1);
    }
    if let Some((s, e)) = last_match(rx_volume(), &text) {
        let value = text[s..e].to_string();
        text = format!("{}{}", &text[..s], &text[e..]);
        info.volume = get_number(&value).trim().parse().unwrap_or(-1);
    }
    let year_match =
        last_match(rx_year_with_month(), &text).or_else(|| last_match(rx_year(), &text));
    if let Some((s, e)) = year_match {
        let value = text[s..e].to_string();
        text = format!("{}{}", &text[..s], &text[e..]);
        let prefix: String = value.chars().take(4).collect();
        info.year = get_number(&prefix).trim().parse().unwrap_or(-1);
    }
    if let Some((s, e)) = last_match(rx_format(), &text) {
        info.format = text[s..e].to_string();
        text = format!("{}{}", &text[..s], &text[e..]);
    }
    // rxNumber is RightToLeft: the LAST match is removed, and its text
    // feeds GetNumber (captured before removal, like the C# match.Value).
    let number_value = match last_match_guard(rx_number(), &text, number_guard) {
        Some((s, e)) => {
            let value = text[s..e].to_string();
            text = format!("{}{}", &text[..s], &text[e..]);
            get_number(&value)
        }
        None => String::new(),
    };
    info.number = normalize_number(&number_value);
    // rxCoverCount is NOT RightToLeft: first match wins.
    if let Some((s, e)) = first_match(rx_cover_count(), &text) {
        let value = &text[s..e];
        info.cover_count = value
            .split_whitespace()
            .next()
            .and_then(|n| n.parse().ok())
            .unwrap_or_else(|| {
                // fallback: leading digits of the span
                let digits: String = value.chars().take_while(|c| c.is_ascii_digit()).collect();
                digits.parse().unwrap_or(1)
            });
    } else if text.to_lowercase().contains("two cove") {
        info.cover_count = 2;
    } else if text.to_lowercase().contains("three cove") {
        info.cover_count = 3;
    } else if text.to_lowercase().contains("four cove") {
        info.cover_count = 4;
    }
    text = cut_off(&text, &['(', '[', '#', ','][..]).trim().to_string();
    info.series = text
        .trim_matches(|c| c == ' ' || c == '-' || c == '.')
        .to_string();
    if info.series.is_empty() || is_number(&info.series) {
        info.series = rx_brackets()
            .replace_all(&directory_name(path), "")
            .into_owned();
        return info;
    }
    info
}

/// NewParser with the configured `OfValues`
/// (`EngineConfiguration.Default.OfValues ?? "of,von,de"`).
pub fn from_file_path(path: &str) -> ComicNameInfo {
    from_file_path_with_of(
        path,
        &crate::settings::EngineConfiguration::global().of_values_or_default(),
    )
}

/// The LegacyParser entry point.
pub fn from_file_path_legacy(path: &str) -> ComicNameInfo {
    let mut info = ComicNameInfo::new();
    let mut text = file_name_without_extension(path).replace(['.', '_'], " ");
    let series_of = |t: &str| {
        rx_series()
            .find(t)
            .ok()
            .flatten()
            .map(|m| m.as_str().to_string())
            .unwrap_or_default()
    };
    let number_of = |t: &str| {
        last_match_guard(rx_legacy_number(), t, legacy_number_guard)
            .map(|(s, e)| t[s..e].to_string())
            .unwrap_or_default()
    };
    info.series = series_of(&text);
    if info.series.is_empty()
        || (info
            .series
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
            && number_of(&text).is_empty())
    {
        text = format!("{} {}", directory_name(path), text);
        info.series = series_of(&text);
    }
    if info.series.is_empty() {
        info.series = text.clone();
    }
    info.series = info.series.trim().to_string();
    let number = number_of(&text);
    info.number = normalize_number(&number);
    info.count = rx_legacy_count()
        .find(&text)
        .ok()
        .flatten()
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(-1);
    info.volume = rx_legacy_volume()
        .find(&text)
        .ok()
        .flatten()
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(-1);
    info.year = rx_legacy_year()
        .find(&text)
        .ok()
        .flatten()
        .and_then(|m| m.as_str().parse().ok())
        .unwrap_or(-1);
    info
}

/// Dispatch like `ComicNameInfo.FromFilePath(path, legacy)` — the
/// legacy flag comes from `EngineConfiguration.Default`.
pub fn parse(path: &str, legacy: bool) -> ComicNameInfo {
    if legacy {
        from_file_path_legacy(path)
    } else {
        from_file_path(path)
    }
}

/// The C# `ComicNameInfo.FromFilePath(path)` single-argument entry:
/// the legacy flag is `EngineConfiguration.Default.LegacyFilenameParser`.
pub fn from_file_path_configured(path: &str) -> ComicNameInfo {
    parse(
        path,
        crate::settings::EngineConfiguration::global().legacy_filename_parser,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_series_number() {
        let n = from_file_path(r"C:\Books\Amazing Adventures 12.cbz");
        assert_eq!(n.series, "Amazing Adventures");
        assert_eq!(n.number, "12");
        assert_eq!(n.volume, -1);
        assert_eq!(n.year, -1);
    }

    #[test]
    fn dots_and_underscores() {
        let n = from_file_path("My_Series_003_(2019).cbz");
        assert_eq!(n.series, "My Series");
        assert_eq!(n.number, "3");
        assert_eq!(n.year, 2019);
    }

    #[test]
    fn volume_and_count() {
        let n = from_file_path("Super Comics vol 2 014 of 20.cbz");
        assert_eq!(n.volume, 2);
        assert_eq!(n.count, 20);
        assert_eq!(n.number, "14");
    }

    #[test]
    fn series_from_directory_fallback() {
        // Number-only filename falls back to the directory path
        // (brackets removed) — exactly like the C# parser.
        let n = from_file_path(r"C:\Comics\Daredevil\12.cbr");
        assert_eq!(n.series, r"C:\Comics\Daredevil");
        assert_eq!(n.number, "12");
    }

    #[test]
    fn legacy_parser_basic() {
        let n = from_file_path_legacy("Batman 5 (1966).cbz");
        assert_eq!(n.series, "Batman");
        assert_eq!(n.number, "5");
        assert_eq!(n.year, 1966);
    }
}
