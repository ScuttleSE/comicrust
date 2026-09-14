//! The template engine (the port of `PathMaker` in lobookmover.py).
//!
//! Token syntax: `{prefix<name[args]>postfix}`. `!name` is an
//! inversion (the prefix/postfix only appear when the field is empty,
//! or when the value fails the trailing `(arg)` text/regex match);
//! `?name` is a conditional (requires a trailing `(arg)`; the
//! prefix/postfix appear when the value equals it, or matches the
//! regex after a `!`). Digit args are zero-padding; list args come in
//! `(a)(b)` groups.
//!
//! The evaluation loop mirrors the addon: replace all tokens in one
//! pass, count the un-replaceable ones, stop when a pass makes no
//! progress. Inner tokens resolve before outer ones, which is how
//! conditional groups like `{ ({<month>, }<year>)}` collapse.

use std::collections::HashMap;

use regex::Regex;
use std::sync::OnceLock;

use cr_core::model::comic_book::values_store;
use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::CrDateTime;

use crate::fields::{self, FieldValue};
use crate::profile::Profile;
use crate::series::SeriesIndex;

/// The token regex (`template_regex` in lobookmover.py), verbatim.
pub fn token_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"\{(?P<prefix>[^{}<]*)<(?P<name>[^\d\s(>]*)(?P<args>\d*|(?:\([^){}]*\))*)>(?P<postfix>[^{}]*)\}",
        )
        .expect("token regex")
    })
}

/// The trailing `(arg)` of an args string (`(\([^(]*\))$`).
fn arg_tail_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(\([^(]*\))$").expect("arg tail regex"))
}

/// The first-letter article skipper (`insert_first_letter`), verbatim
/// article list, case-insensitive, start-anchored.
fn first_letter_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?i)^(?:(?:the|a|an|de|het|een|die|der|das|des|dem|ein|eines|einer|einen|la|le|l'|les|un|une|el|las|los|un|una|unos|unas|o|os|um|uma|uns|umas|en|et|il|lo|uno|gli)\s+)?(?P<letter>.).+",
        )
        .expect("first letter regex")
    })
}

fn extract_args(args_match: &str) -> Vec<String> {
    // `re.findall("\(([^)]*)\)", args_match)`.
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\(([^)]*)\)").expect("args regex"));
    re.captures_iter(args_match)
        .map(|c| c[1].to_string())
        .collect()
}

/// Python `re.match` semantics: start-anchored, no-match on an
/// invalid user pattern (the regex crate cannot compile look-arounds;
/// eval.rs uses the same no-match rule).
fn regex_match_start(pattern: &str, text: &str) -> Option<()> {
    let re = Regex::new(&format!("^(?:{})", pattern)).ok()?;
    re.is_match(text).then_some(())
}

/// The shortest-round-trip float text the .NET `ToString` produces
/// (`3`, `3.5`), used when a float lands in a template or rule.
pub fn net_f32_text(f: f32) -> String {
    if f == f.trunc() && f.abs() < 1e9 {
        format!("{}", f as i64)
    } else {
        format!("{}", f)
    }
}

/// Zero-pads like the addon's `pad` (decimal remainders kept, negative
/// sign outside the padding). Values that parse as a number pad;
/// anything else returns unchanged.
pub fn pad(value: &str, padding: usize) -> String {
    let number: f64 = match value.trim().parse::<f64>() {
        Ok(v) => v,
        Err(_) => return value.to_string(),
    };
    let pad_to = |digits: &str| -> String {
        let mut s = digits.to_string();
        while s.chars().count() < padding {
            s.insert(0, '0');
        }
        s
    };
    if number >= 0.0 {
        match value.split_once('.') {
            Some((int_part, rest)) => format!("{}.{}", pad_to(int_part), rest),
            None => pad_to(value),
        }
    } else {
        let digits = &value[1..];
        match digits.split_once('.') {
            Some((int_part, rest)) => format!("-{}.{}", pad_to(int_part), rest),
            None => format!("-{}", pad_to(digits)),
        }
    }
}

/// Collapses runs of whitespace to one space (`\s\s+` → " ").
pub fn collapse_spaces(text: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"\s\s+").expect("space regex"));
    re.replace_all(text, " ").into_owned()
}

/// Translates a .NET date format string to a `chrono` one. The common
/// custom tokens plus the standard single-letter patterns the config
/// dialog offers (`D`, `M`, `Y`, …). Unknown text passes through.
pub fn net_date_format(fmt: &str) -> String {
    match fmt {
        "D" => return "%A, %B %-d, %Y".to_string(),
        "d" => return "%-m/%-d/%Y".to_string(),
        "M" | "m" => return "%B %d".to_string(),
        "Y" | "y" => return "%B %Y".to_string(),
        "T" | "t" => return "%H:%M:%S".to_string(),
        "f" | "F" | "G" | "g" => return "%m/%d/%Y %H:%M:%S".to_string(),
        "s" => return "%Y-%m-%dT%H:%M:%S".to_string(),
        _ => {}
    }
    let mut out = String::new();
    let chars: Vec<char> = fmt.chars().collect();
    let mut i = 0;
    let n = chars.len();
    while i < n {
        let c = chars[i];
        let run = |k: char| chars[i..].iter().take_while(|x| **x == k).count();
        match c {
            'y' => {
                let k = run('y');
                out.push_str(if k >= 3 { "%Y" } else { "%y" });
                i += k;
            }
            'M' => {
                let k = run('M');
                out.push_str(match k {
                    1 => "%-m",
                    2 => "%m",
                    3 => "%b",
                    _ => "%B",
                });
                i += k;
            }
            'd' => {
                let k = run('d');
                out.push_str(if k >= 2 { "%d" } else { "%-d" });
                i += k;
            }
            'H' => {
                let k = run('H');
                out.push_str(if k >= 2 { "%H" } else { "%-H" });
                i += k;
            }
            'h' => {
                let k = run('h');
                out.push_str(if k >= 2 { "%I" } else { "%-I" });
                i += k;
            }
            'm' => {
                let k = run('m');
                out.push_str(if k >= 2 { "%M" } else { "%-M" });
                i += k;
            }
            's' => {
                let k = run('s');
                out.push_str(if k >= 2 { "%S" } else { "%-S" });
                i += k;
            }
            't' => {
                let _k = run('t');
                out.push_str("%p");
                i += 1;
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Multi-value selections
// ---------------------------------------------------------------------------

/// What the UI is asked (`MultiValueSelectionFormArgs`).
#[derive(Clone, Debug)]
pub struct MultiValueAsk {
    pub items: Vec<String>,
    pub selected: Vec<String>,
    pub field_text: String,
    pub book_text: String,
    pub series: bool,
}

/// What the user answered (`MultiValueSelectionFormResult`).
#[derive(Clone, Debug, Default)]
pub struct MultiValueAnswer {
    pub selection: Vec<String>,
    pub every_issue: bool,
    pub folder: bool,
    pub always_use: bool,
    pub always_use_dont_ask: bool,
}

/// The ask callback (the Multi-Value Selection form on the UI side).
pub trait MultiValueAsker {
    fn ask_multi_value(&mut self, ask: MultiValueAsk) -> MultiValueAnswer;
}

/// One remembered "always use" selection (`MultiValueAlwaysUsedValues`).
#[derive(Clone, Debug, PartialEq)]
pub struct AlwaysUsedValues {
    pub do_not_ask: bool,
    pub use_folder_separator: bool,
    pub values: Vec<String>,
}

/// A cached per-issue/series selection (`MultiValueSelectionFormResult`).
#[derive(Clone, Debug, PartialEq)]
pub struct MultiValueResult {
    pub selection: Vec<String>,
    pub every_issue: bool,
    pub folder: bool,
}

/// Per-run multi-value state: cached answers and the "always use"
/// lists (`field_dict` / `<field>AlwaysUse` on the PathMaker).
#[derive(Default)]
pub struct MultiValueState {
    issue_cache: HashMap<String, MultiValueResult>,
    series_cache: HashMap<String, MultiValueResult>,
    always_use: HashMap<String, Vec<AlwaysUsedValues>>,
}

/// Token evaluation state for one book. `'a` is the data lifetime
/// (books, profile); `'s` the mutable working-state borrow.
pub struct TokenCtx<'a, 's> {
    pub book: &'a ComicBook,
    pub book_index: usize,
    pub profile: &'a Profile,
    pub series: &'s mut SeriesIndex<'a>,
    pub failed_fields: &'s mut Vec<String>,
    pub failed: &'s mut bool,
    counter: &'s mut Option<i64>,
    multi: &'s mut MultiValueState,
    asker: &'s mut dyn MultiValueAsker,
    invalid: usize,
    /// Illegal characters cleaned longest-key-first.
    illegal: Vec<(String, String)>,
}

impl<'a, 's> TokenCtx<'a, 's> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        book: &'a ComicBook,
        book_index: usize,
        profile: &'a Profile,
        series: &'s mut SeriesIndex<'a>,
        failed_fields: &'s mut Vec<String>,
        failed: &'s mut bool,
        counter: &'s mut Option<i64>,
        multi: &'s mut MultiValueState,
        asker: &'s mut dyn MultiValueAsker,
    ) -> Self {
        let mut illegal: Vec<(String, String)> = profile
            .illegal_characters
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        illegal.sort_by_key(|(k, _)| std::cmp::Reverse(k.chars().count()));
        TokenCtx {
            book,
            book_index,
            profile,
            series,
            failed_fields,
            failed,
            counter,
            multi,
            asker,
            invalid: 0,
            illegal,
        }
    }

    /// Replaces illegal path characters (longest keys first).
    pub fn replace_illegal(&self, text: &str) -> String {
        let mut out = text.to_string();
        for (from, to) in &self.illegal {
            if from.is_empty() {
                continue;
            }
            if out.contains(from.as_str()) {
                out = out.replace(from.as_str(), to);
            }
        }
        out
    }

    /// Evaluates all tokens (`insert_fields_into_template`).
    pub fn insert_fields(&mut self, template: &str) -> String {
        let re = token_regex();
        let mut template = template.to_string();
        self.invalid = 0;
        while re.is_match(&template) {
            let count = re.find_iter(&template).count();
            if self.invalid == count {
                break;
            }
            self.invalid = 0;
            let mut out = String::with_capacity(template.len());
            let mut last = 0;
            for m in re.find_iter(&template) {
                out.push_str(&template[last..m.start()]);
                let caps = re
                    .captures(&template[m.start()..m.end()])
                    .expect("token capture");
                let g = |name: &str| {
                    caps.name(name)
                        .map(|m| m.as_str())
                        .unwrap_or("")
                        .to_string()
                };
                let replacement =
                    self.insert_field(&g("prefix"), &g("name"), &g("args"), &g("postfix"));
                out.push_str(&replacement);
                last = m.end();
            }
            out.push_str(&template[last..]);
            template = out;
        }
        template
    }

    /// One token replacement (`insert_field`). Returns the original
    /// token text when it cannot be evaluated.
    fn insert_field(&mut self, prefix: &str, name: &str, args: &str, postfix: &str) -> String {
        let original = format!("{{{prefix}<{name}{args}>{postfix}}}");
        let mut name = name.to_string();
        let mut args = args.to_string();
        let mut conditional = false;
        let mut inversion = false;
        let mut inversion_args = String::new();
        let mut conditional_args = String::new();

        // Inversions: `!name` with an optional trailing (arg).
        if name.starts_with('!') {
            inversion = true;
            name = name.trim_start_matches('!').to_string();
            if !args.is_empty() {
                let tail = arg_tail_regex().find(&args).map(|m| m.as_str().to_string());
                if let Some(tail) = tail {
                    args = args[..args.len() - tail.len()].to_string();
                    inversion_args = tail[1..tail.len() - 1].to_string();
                }
            }
        }

        // Conditionals: require a trailing (arg).
        if name.starts_with('?') {
            conditional = true;
            name = name.trim_start_matches('?').to_string();
            if args.is_empty() {
                self.invalid += 1;
                return original;
            }
            let tail = arg_tail_regex().find(&args).map(|m| m.as_str().to_string());
            match tail {
                Some(tail) => {
                    args = args[..args.len() - tail.len()].to_string();
                    conditional_args = tail[1..tail.len() - 1].to_string();
                }
                None => {
                    self.invalid += 1;
                    return original;
                }
            }
        }

        let Some(field) = fields::template_field(&name) else {
            self.invalid += 1;
            return original;
        };
        let field = field.to_string();

        let Some(result) = self.get_field_text(&field, &name, &args) else {
            self.invalid += 1;
            return original;
        };

        // Conditionals: insert the prefix/postfix when the value
        // matches the (arg) text, or the (arg!) regex.
        if conditional {
            if let Some(pat) = conditional_args.strip_prefix('!') {
                if pat.is_empty() {
                    return String::new();
                }
                return if regex_match_start(pat, &result).is_some() {
                    format!("{prefix}{postfix}")
                } else {
                    String::new()
                };
            }
            return if result == conditional_args {
                format!("{prefix}{postfix}")
            } else {
                String::new()
            };
        }

        // Inversions: insert when the value is empty, or fails the
        // (arg!) regex, or differs from the (arg) text.
        if inversion {
            if inversion_args.is_empty() {
                return if result.is_empty() {
                    format!("{prefix}{postfix}")
                } else {
                    String::new()
                };
            }
            if let Some(pat) = inversion_args.strip_prefix('!') {
                return if regex_match_start(pat, &result).is_none() {
                    format!("{prefix}{postfix}")
                } else {
                    String::new()
                };
            }
            return if result != inversion_args {
                format!("{prefix}{postfix}")
            } else {
                String::new()
            };
        }

        // Empty results: the failed-fields watch and the EmptyData
        // substitution.
        if result.is_empty() {
            if self.profile.fail_empty_values && self.profile.failed_fields.contains(&field) {
                if !self.failed_fields.contains(&field) {
                    self.failed_fields.push(field.clone());
                }
                *self.failed = true;
            }
            if let Some(v) = self.profile.empty_data.get(&field) {
                if !v.is_empty() {
                    return v.clone();
                }
            }
            return String::new();
        }

        format!("{prefix}{result}{postfix}")
    }

    /// `get_field_text`: dispatches a field read by its C# name,
    /// template name, and the raw args. `None` marks an invalid token.
    fn get_field_text(
        &mut self,
        field: &str,
        template_name: &str,
        args_match: &str,
    ) -> Option<String> {
        let args = extract_args(args_match);

        match field {
            "StartYear" | "StartMonth" | "EndYear" | "EndMonth" => {
                return self.insert_start_value(field, args_match, template_name);
            }
            "ReadPercentage" if args.len() == 3 => {
                return Some(self.insert_read_percentage(&args));
            }
            "FirstLetter" => {
                // No args, or an arg that is no field name: invalid
                // (the addon falls off its if-chain).
                if args.is_empty() {
                    return None;
                }
                if fields::rule_field(&args[0]).is_some() || fields::field_to_name(&args[0]) {
                    return self.insert_first_letter(&args[0]);
                }
                return None;
            }
            "Counter" if args.len() == 3 => return Some(self.insert_counter(&args)),
            "FirstIssueNumber" => return Some(self.insert_first_issue_number(args_match)),
            "LastIssueNumber" => return Some(self.insert_last_issue_number(args_match)),
            "Custom" => {
                if args.is_empty() {
                    return None;
                }
                return Some(self.insert_custom_value(&args[0]));
            }
            "AddedTime" | "ReleasedTime" | "OpenedTime" => {
                let fmt = args.first().map(|s| s.as_str()).unwrap_or("");
                return Some(self.insert_formatted_datetime(fmt, field));
            }
            "Manga" | "SeriesComplete" if !args.is_empty() && args.len() < 3 => {
                return self.insert_yes_no_field(field, &args);
            }
            _ => {}
        }

        if args.len() == 2 {
            return self.insert_multi_value(field, &args);
        }
        if args_match.is_empty() {
            return self.insert_text_field(field, template_name);
        }
        if !args_match.is_empty() && args_match.chars().all(|c| c.is_ascii_digit()) {
            return self.insert_number_field(field, args_match.parse::<usize>().ok()?);
        }
        None
    }

    /// `insert_text_field`: the plain string form, with the month-name
    /// special case for `<month>`.
    fn insert_text_field(&mut self, field: &str, template_name: &str) -> Option<String> {
        if field == "Month" && !template_name.ends_with('#') {
            return Some(self.insert_month_as_name());
        }
        let text = fields::field_display(self.book, field)?;
        Some(self.replace_illegal(&text))
    }

    /// `insert_number_field`: the padded numeric form; padding 0 takes
    /// its width from the series' last book.
    fn insert_number_field(&mut self, field: &str, padding: usize) -> Option<String> {
        let raw = fields::field_raw(self.book, field)?;
        let number = match raw {
            FieldValue::Int(-1) => return Some(String::new()),
            FieldValue::Int(i) => i.to_string(),
            FieldValue::Str(s) if s.is_empty() => return Some(String::new()),
            FieldValue::Str(s) => s,
            _ => return None,
        };
        let padding = if padding == 0 {
            let last = self.series.last_book(self.book, self.book_index);
            match fields::field_raw(last, field)? {
                FieldValue::Int(i) => i.to_string().chars().count(),
                FieldValue::Str(s) => s.chars().count(),
                _ => return None,
            }
        } else {
            padding
        };
        Some(self.replace_illegal(&pad(&number, padding)))
    }

    /// `insert_yes_no_field`: `(text)` inserts when Yes,
    /// `(text,!)` when No.
    fn insert_yes_no_field(&mut self, field: &str, args: &[String]) -> Option<String> {
        let text = args.first()?;
        let no = args.len() == 2 && args[1] == "!";
        let raw = fields::field_raw(self.book, field)?;
        let is_yes = match raw {
            FieldValue::Manga(m) => matches!(
                m,
                cr_core::model::enums::MangaYesNo::Yes
                    | cr_core::model::enums::MangaYesNo::YesAndRightToLeft
            ),
            FieldValue::YesNo(y) => y == cr_core::model::enums::YesNo::Yes,
            _ => return None,
        };
        let is_no = match raw {
            FieldValue::Manga(m) => m == cr_core::model::enums::MangaYesNo::No,
            FieldValue::YesNo(y) => y == cr_core::model::enums::YesNo::No,
            _ => return None,
        };
        let result = if (!no && is_yes) || (no && is_no) {
            text.clone()
        } else {
            String::new()
        };
        Some(self.replace_illegal(&result))
    }

    /// `insert_read_percentage`: `(text)(op)(percent)`.
    fn insert_read_percentage(&mut self, args: &[String]) -> String {
        let text = &args[0];
        let operator = &args[1];
        let percent = &args[2];
        let read = cr_engine::matcher::book_view::read_percentage(self.book);
        let target: i32 = percent.parse().unwrap_or(0);
        let hit = match operator.as_str() {
            "=" => read == target,
            ">" => read > target,
            "<" => read < target,
            _ => false,
        };
        let result = if hit { text.clone() } else { String::new() };
        self.replace_illegal(&result)
    }

    /// `insert_first_letter`: the first letter of a field, skipping
    /// the multilingual articles.
    fn insert_first_letter(&mut self, arg: &str) -> Option<String> {
        let field = fields::rule_field(arg).unwrap_or(arg);
        let text = fields::field_rule_text(self.book, field)?;
        // No article+second-char match (e.g. a one-letter value)
        // yields the empty string, like the addon.
        let letter = first_letter_regex()
            .captures(&text)
            .map(|c| c["letter"].to_uppercase())
            .unwrap_or_default();
        Some(self.replace_illegal(&letter))
    }

    /// `insert_counter`: the run-wide counter
    /// `(start)(increment)(pad)`.
    fn insert_counter(&mut self, args: &[String]) -> String {
        let start: i64 = args[0].trim().parse().unwrap_or(0);
        let increment: i64 = args[1].trim().parse().unwrap_or(0);
        let pad_width: usize = if args[2].is_empty() {
            0
        } else {
            args[2].trim().parse().unwrap_or(0)
        };
        let value = match *self.counter {
            None => {
                *self.counter = Some(start);
                start
            }
            Some(v) => {
                let next = v + increment;
                *self.counter = Some(next);
                next
            }
        };
        pad(&value.to_string(), pad_width)
    }

    /// `insert_month_as_name`.
    fn insert_month_as_name(&mut self) -> String {
        let Some(FieldValue::Int(month)) = fields::field_raw(self.book, "Month") else {
            return String::new();
        };
        match self.profile.months.get(&month.to_string()) {
            Some(name) if !name.is_empty() => self.replace_illegal(name),
            _ => String::new(),
        }
    }

    /// `insert_start_value`: the series-relative start/end values.
    fn insert_start_value(
        &mut self,
        field: &str,
        args_match: &str,
        template_name: &str,
    ) -> Option<String> {
        match field {
            "StartYear" | "EndYear" => Some(self.insert_start_year(field == "EndYear")),
            _ => Some(self.insert_start_month(args_match, template_name, field == "EndMonth")),
        }
    }

    fn insert_start_year(&mut self, end: bool) -> String {
        let book = if end {
            self.series.last_book(self.book, self.book_index)
        } else {
            self.series.earliest_book(self.book, self.book_index)
        };
        let year = cr_engine::matcher::book_view::shadow_year(
            book,
            &cr_engine::matcher::book_view::proposed_cached(book),
        );
        if year == -1 {
            return String::new();
        }
        self.replace_illegal(&year.to_string())
    }

    fn insert_start_month(&mut self, args_match: &str, template_name: &str, end: bool) -> String {
        let book = if end {
            self.series.last_book(self.book, self.book_index)
        } else {
            self.series.earliest_book(self.book, self.book_index)
        };
        let month = book.info.month;
        if month == -1 {
            return String::new();
        }
        if template_name.ends_with('#') {
            let month = if !args_match.is_empty() && args_match.chars().all(|c| c.is_ascii_digit())
            {
                pad(&month.to_string(), args_match.parse::<usize>().unwrap_or(0))
            } else {
                month.to_string()
            };
            self.replace_illegal(&month)
        } else {
            match self.profile.months.get(&month.to_string()) {
                Some(name) if !name.is_empty() => self.replace_illegal(name),
                _ => String::new(),
            }
        }
    }

    /// `insert_formated_datetime`.
    fn insert_formatted_datetime(&mut self, time_format: &str, field: &str) -> String {
        let date_time: CrDateTime = match field {
            "AddedTime" => self.book.added_time,
            "ReleasedTime" => self.book.released_time,
            "OpenedTime" => self.book.opened_time,
            _ => return String::new(),
        };
        if time_format.is_empty() {
            return fields::date_display(&date_time);
        }
        date_time
            .naive
            .format(&net_date_format(time_format))
            .to_string()
    }

    /// `insert_custom_value`.
    fn insert_custom_value(&mut self, key: &str) -> String {
        values_store::decode(&self.book.custom_values_store)
            .into_iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v)
            .unwrap_or_default()
    }

    fn insert_first_issue_number(&mut self, padding: &str) -> String {
        let book = self.series.earliest_book(self.book, self.book_index);
        let prop = cr_engine::matcher::book_view::proposed_cached(book);
        let number = cr_engine::matcher::book_view::shadow_number(book, &prop).to_string();
        let out = if !padding.is_empty() && padding.chars().all(|c| c.is_ascii_digit()) {
            pad(&number, padding.parse::<usize>().unwrap_or(0))
        } else {
            number
        };
        self.replace_illegal(&out)
    }

    fn insert_last_issue_number(&mut self, padding: &str) -> String {
        let book = self.series.last_book(self.book, self.book_index);
        let prop = cr_engine::matcher::book_view::proposed_cached(book);
        let number = cr_engine::matcher::book_view::shadow_number(book, &prop).to_string();
        let out = if !padding.is_empty() && padding.chars().all(|c| c.is_ascii_digit()) {
            pad(&number, padding.parse::<usize>().unwrap_or(0))
        } else {
            number
        };
        self.replace_illegal(&out)
    }

    // ------------------------------------------------------------------
    // Multi-value fields
    // ------------------------------------------------------------------

    /// `insert_multi_value_field`.
    fn insert_multi_value(&mut self, field: &str, args: &[String]) -> Option<String> {
        let separator = args[0].clone();
        match args[1].as_str() {
            "series" => self.insert_multi_value_series(field, &separator),
            "issue" => self.insert_multi_value_issue(field, &separator),
            _ => None,
        }
    }

    /// The series key used for caches and series lookups
    /// (`Publisher + ShadowSeries + str(ShadowVolume)`).
    fn series_cache_key(&self) -> String {
        let prop = cr_engine::matcher::book_view::proposed_cached(self.book);
        format!(
            "{}{}{}",
            self.book.info.publisher,
            cr_engine::matcher::book_view::shadow_series(self.book, &prop),
            cr_engine::matcher::book_view::shadow_volume(self.book, &prop)
        )
    }

    fn issue_cache_key(&self) -> String {
        let prop = cr_engine::matcher::book_view::proposed_cached(self.book);
        format!(
            "{}{}{}{}",
            self.book.info.publisher,
            cr_engine::matcher::book_view::shadow_series(self.book, &prop),
            cr_engine::matcher::book_view::shadow_volume(self.book, &prop),
            cr_engine::matcher::book_view::shadow_number(self.book, &prop)
        )
    }

    fn book_text(&self) -> String {
        let prop = cr_engine::matcher::book_view::proposed_cached(self.book);
        format!(
            "{} vol. {} #{}",
            cr_engine::matcher::book_view::shadow_series(self.book, &prop),
            cr_engine::matcher::book_view::shadow_volume(self.book, &prop),
            cr_engine::matcher::book_view::shadow_number(self.book, &prop)
        )
    }

    /// `insert_multi_value_issue`: values of one issue; asks the user
    /// unless remembered, single-valued, or always-used.
    fn insert_multi_value_issue(&mut self, field: &str, separator: &str) -> Option<String> {
        let index = self.issue_cache_key();
        let book_text = self.book_text();

        if let Some(result) = self.multi.issue_cache.get(&index).cloned() {
            return Some(self.make_multi_value_issue_string(
                &result.selection,
                separator,
                result.folder,
            ));
        }

        let text = match fields::field_raw(self.book, field)? {
            FieldValue::Str(s) => s,
            _ => return None,
        };
        if text.trim().is_empty() {
            self.multi.issue_cache.insert(
                index,
                MultiValueResult {
                    selection: Vec::new(),
                    every_issue: false,
                    folder: false,
                },
            );
            return Some(String::new());
        }

        let values = fields::split_multi_values(&text);
        if values.len() == 1 && self.profile.dont_ask_when_multi_one {
            return Some(self.replace_illegal(&values[0]));
        }

        let mut selected_values: Vec<String> = Vec::new();
        // "Always use" lists, longest first.
        let always: Vec<AlwaysUsedValues> = self
            .multi
            .always_use
            .entry(field.to_string())
            .or_default()
            .clone();
        let mut lists = always;
        lists.sort_by_key(|l| std::cmp::Reverse(l.values.len()));
        for list in lists {
            let hits = list.values.iter().filter(|v| values.contains(v)).count();
            if hits == list.values.len() {
                if list.do_not_ask {
                    return Some(self.make_multi_value_issue_string(
                        &list.values,
                        separator,
                        list.use_folder_separator,
                    ));
                }
                selected_values = list.values.clone();
                break;
            }
        }

        let answer = self.asker.ask_multi_value(MultiValueAsk {
            items: values,
            selected: selected_values,
            field_text: field.to_string(),
            book_text,
            series: false,
        });
        let result = MultiValueResult {
            selection: answer.selection.clone(),
            every_issue: false,
            folder: answer.folder,
        };
        self.multi.issue_cache.insert(index, result.clone());
        if result.selection.is_empty() {
            return Some(String::new());
        }
        if answer.always_use {
            self.multi
                .always_use
                .entry(field.to_string())
                .or_default()
                .push(AlwaysUsedValues {
                    do_not_ask: answer.always_use_dont_ask,
                    use_folder_separator: answer.folder,
                    values: answer.selection,
                });
        }
        Some(self.make_multi_value_issue_string(&result.selection, separator, result.folder))
    }

    /// `make_multi_value_issue_string`.
    fn make_multi_value_issue_string(
        &mut self,
        values: &[String],
        separator: &str,
        use_folder: bool,
    ) -> String {
        if use_folder {
            let separator = format!("{}\\", self.replace_illegal(separator));
            let values: Vec<String> = values.iter().map(|v| self.replace_illegal(v)).collect();
            return values.join(&separator);
        }
        self.replace_illegal(&values.join(separator))
    }

    /// `insert_multi_value_series`: the values of a whole series;
    /// asks once per series.
    fn insert_multi_value_series(&mut self, field: &str, separator: &str) -> Option<String> {
        let index = self.series_cache_key();

        if let Some(result) = self.multi.series_cache.get(&index).cloned() {
            return Some(self.make_multi_value_series_string(&result, separator, field));
        }

        let values = self.all_multi_values_from_series(field);
        if values.is_empty() {
            self.multi.series_cache.insert(
                index,
                MultiValueResult {
                    selection: Vec::new(),
                    every_issue: false,
                    folder: false,
                },
            );
            return Some(String::new());
        }

        let answer = self.asker.ask_multi_value(MultiValueAsk {
            items: values,
            selected: Vec::new(),
            field_text: field.to_string(),
            book_text: {
                let prop = cr_engine::matcher::book_view::proposed_cached(self.book);
                format!(
                    "{} vol. {}",
                    cr_engine::matcher::book_view::shadow_series(self.book, &prop),
                    cr_engine::matcher::book_view::shadow_volume(self.book, &prop)
                )
            },
            series: true,
        });
        let result = MultiValueResult {
            selection: answer.selection,
            every_issue: answer.every_issue,
            folder: answer.folder,
        };
        self.multi.series_cache.insert(index, result.clone());
        Some(self.make_multi_value_series_string(&result, separator, field))
    }

    /// `make_multi_value_series_string`.
    fn make_multi_value_series_string(
        &mut self,
        result: &MultiValueResult,
        separator: &str,
        field: &str,
    ) -> String {
        let text = match fields::field_raw(self.book, field) {
            Some(FieldValue::Str(s)) => s,
            _ => String::new(),
        };
        if result.folder {
            let separator = format!("{}\\", self.replace_illegal(separator));
            let values: Vec<String> = fields::split_multi_values(&text)
                .iter()
                .map(|v| self.replace_illegal(v))
                .collect();
            let selection: Vec<String> = result
                .selection
                .iter()
                .map(|v| self.replace_illegal(v))
                .collect();
            return if result.every_issue {
                selection.join(&separator)
            } else {
                let items: Vec<String> = selection
                    .iter()
                    .filter(|item| values.contains(item))
                    .cloned()
                    .collect();
                items.join(&separator)
            };
        }
        let values = fields::split_multi_values(&text);
        let out = if result.every_issue {
            result.selection.join(separator)
        } else {
            let items: Vec<String> = result
                .selection
                .iter()
                .filter(|item| values.contains(item))
                .cloned()
                .collect();
            items.join(separator)
        };
        self.replace_illegal(&out)
    }

    /// `get_all_multi_values_from_series`: unique values across the
    /// series' books, first-seen order. The series filter compares the
    /// SHADOW values (like the addon); the values read is the raw
    /// field.
    fn all_multi_values_from_series(&self, field: &str) -> Vec<String> {
        let prop = cr_engine::matcher::book_view::proposed_cached(self.book);
        let series = cr_engine::matcher::book_view::shadow_series(self.book, &prop).to_string();
        let volume = cr_engine::matcher::book_view::shadow_volume(self.book, &prop);
        let publisher = self.book.info.publisher.clone();
        let mut seen: Vec<String> = Vec::new();
        for b in self.series.books.iter() {
            let bprop = cr_engine::matcher::book_view::proposed_cached(b);
            if cr_engine::matcher::book_view::shadow_series(b, &bprop) != series
                || cr_engine::matcher::book_view::shadow_volume(b, &bprop) != volume
                || b.info.publisher != publisher
            {
                continue;
            }
            if let Some(FieldValue::Str(s)) = fields::field_raw(b, field) {
                for v in fields::split_multi_values(&s) {
                    if !v.is_empty() && !seen.contains(&v) {
                        seen.push(v);
                    }
                }
            }
        }
        seen
    }

    // ------------------------------------------------------------------
    // Path building
    // ------------------------------------------------------------------

    /// `make_path`: returns (folder path, file name). The boolean
    /// return is the addon's `failed` (a watched field was empty and
    /// the move is not a move-failed run).
    #[allow(clippy::too_many_arguments)]
    pub fn make_path(
        &mut self,
        folder_template: &str,
        file_template: &str,
    ) -> (String, String, bool) {
        *self.failed = false;
        self.failed_fields.clear();

        let file_path = if self.profile.use_file_name {
            self.make_file_name(file_template)
        } else {
            file_name_with_extension(self.book)
        };
        let folder_path = if self.profile.use_folder {
            self.make_folder_path(folder_template)
        } else {
            file_directory(self.book)
        };

        if *self.failed && !self.profile.move_failed {
            return (
                file_directory(self.book),
                file_name_with_extension(self.book),
                true,
            );
        }
        (folder_path, file_path, false)
    }

    /// `make_folder_path`.
    pub fn make_folder_path(&mut self, template: &str) -> String {
        let mut folder_path = String::new();
        let template = template.trim().trim_matches('\\');
        if !template.is_empty() {
            let rough = self.insert_fields(template);
            for line in rough.split('\\') {
                let mut line = line.to_string();
                if line.trim().is_empty() {
                    line = self.profile.empty_folder.clone();
                }
                let line = self.replace_illegal(&line);
                let line = line.trim_matches('.').trim().to_string();
                folder_path = path_combine(&folder_path, &line);
            }
        }
        let base = if *self.failed && self.profile.move_failed {
            self.profile.failed_folder.clone()
        } else {
            self.profile.base_folder.clone()
        };
        folder_path = path_combine(&base, &folder_path);
        if self.profile.replace_multiple_spaces {
            folder_path = collapse_spaces(&folder_path);
        }
        folder_path
    }

    /// `make_file_name`.
    pub fn make_file_name(&mut self, template: &str) -> String {
        let raw = self.insert_fields(template);
        let file_name = raw.trim().to_string();
        let file_name = self.replace_illegal(&file_name);
        if file_name.is_empty() {
            return String::new();
        }
        let extension = if !self.book.file_path.is_empty() {
            path_extension(&self.book.file_path)
        } else {
            self.profile.fileless_format.clone()
        };
        let file_name = if self.profile.replace_multiple_spaces {
            collapse_spaces(&file_name)
        } else {
            file_name
        };
        format!("{file_name}{extension}")
    }
}

/// .NET `Path.Combine` semantics on POSIX paths: a rooted second path
/// wins; empty parts drop out.
pub fn path_combine(a: &str, b: &str) -> String {
    if b.starts_with('/') {
        return b.to_string();
    }
    if a.is_empty() {
        return b.to_string();
    }
    if b.is_empty() {
        return a.to_string();
    }
    if a.ends_with('/') {
        format!("{a}{b}")
    } else {
        format!("{a}/{b}")
    }
}

/// `Path.GetExtension` — the dot included.
pub fn path_extension(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match name.rfind('.') {
        Some(i) => name[i..].to_string(),
        None => String::new(),
    }
}

/// `book.FileDirectory`.
pub fn file_directory(book: &ComicBook) -> String {
    std::path::Path::new(&book.file_path)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `book.FileNameWithExtension`.
pub fn file_name_with_extension(book: &ComicBook) -> String {
    std::path::Path::new(&book.file_path)
        .file_name()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoAsk;
    impl MultiValueAsker for NoAsk {
        fn ask_multi_value(&mut self, _ask: MultiValueAsk) -> MultiValueAnswer {
            MultiValueAnswer::default()
        }
    }

    /// The evaluation harness: owns the state one run needs.
    struct Harness {
        books: Vec<ComicBook>,
        profile: Profile,
        multi: MultiValueState,
        counter: Option<i64>,
        failed_fields: Vec<String>,
        failed: bool,
        asker: NoAsk,
    }

    impl Harness {
        fn new(profile: Profile, books: Vec<ComicBook>) -> Self {
            Harness {
                books,
                profile,
                multi: MultiValueState::default(),
                counter: None,
                failed_fields: Vec::new(),
                failed: false,
                asker: NoAsk,
            }
        }

        fn eval(&mut self, index: usize, template: &str) -> String {
            let mut series = SeriesIndex::new(&self.books);
            let book = &self.books[index];
            let mut ctx = TokenCtx::new(
                book,
                index,
                &self.profile,
                &mut series,
                &mut self.failed_fields,
                &mut self.failed,
                &mut self.counter,
                &mut self.multi,
                &mut self.asker,
            );
            ctx.insert_fields(template)
        }

        fn make_path(&mut self, index: usize, folder: &str, file: &str) -> (String, String, bool) {
            let mut series = SeriesIndex::new(&self.books);
            let book = &self.books[index];
            let mut ctx = TokenCtx::new(
                book,
                index,
                &self.profile,
                &mut series,
                &mut self.failed_fields,
                &mut self.failed,
                &mut self.counter,
                &mut self.multi,
                &mut self.asker,
            );
            ctx.make_path(folder, file)
        }
    }

    fn book(
        series: &str,
        number: &str,
        year: i32,
        month: i32,
        volume: i32,
        path: &str,
    ) -> ComicBook {
        let mut b = ComicBook::default();
        b.info.series = series.into();
        b.info.number = number.into();
        b.info.year = year;
        b.info.month = month;
        b.info.volume = volume;
        b.file_path = path.into();
        b.enable_proposed = false;
        b
    }

    #[test]
    fn pad_semantics() {
        assert_eq!(pad("5", 2), "05");
        assert_eq!(pad("243", 2), "243");
        assert_eq!(pad("7.1", 2), "07.1");
        assert_eq!(pad("-5", 3), "-005");
        assert_eq!(pad("A", 2), "A");
        assert_eq!(pad("0", 2), "00");
    }

    #[test]
    fn plain_and_padded_tokens() {
        let mut h = Harness::new(
            Profile::default(),
            vec![book("Batman", "5", 2012, 3, 1, "/c/B.cbz")],
        );
        assert_eq!(h.eval(0, "{<series>}"), "Batman");
        assert_eq!(h.eval(0, "{<number2>}"), "05");
        // An empty result drops the whole token, prefix and postfix.
        assert_eq!(h.eval(0, "Vol {<volume>}"), "Vol 1");
        assert_eq!(h.eval(0, "{<year>}"), "2012");
        // Unknown tokens stay verbatim.
        assert_eq!(h.eval(0, "{<nonesuch>}"), "{<nonesuch>}");
    }

    #[test]
    fn conditional_group_collapses() {
        let mut h = Harness::new(
            Profile::default(),
            vec![book("Batman", "5", 2012, 3, 1, "/c/B.cbz")],
        );
        // The default file template's date group with a month set.
        assert_eq!(h.eval(0, "{ ({<month>, }<year>) }"), " (March, 2012) ");
        // Month empty -> only the year survives.
        let mut h2 = Harness::new(
            Profile::default(),
            vec![book("Batman", "5", 2012, -1, 1, "/c/B.cbz")],
        );
        assert_eq!(h2.eval(0, "{ ({<month>, }<year>) }"), " (2012) ");
        // No year either -> the whole group drops.
        let mut h3 = Harness::new(
            Profile::default(),
            vec![book("Batman", "5", -1, -1, 1, "/c/B.cbz")],
        );
        assert_eq!(h3.eval(0, "{ ({<month>, }<year>) }"), "");
    }

    #[test]
    fn month_names_and_seasons() {
        let mut h = Harness::new(
            Profile::default(),
            vec![book("B", "5", 2012, 3, 1, "/c/B.cbz")],
        );
        assert_eq!(h.eval(0, "{<month>}"), "March");
        assert_eq!(h.eval(0, "{<month#>}"), "3");
        assert_eq!(h.eval(0, "{<month2>}"), "03");
    }

    #[test]
    fn empty_data_substitution() {
        let mut p = Profile::default();
        p.empty_data.insert("ShadowSeries".into(), "Unknown".into());
        let mut b = book("", "5", 2012, 3, 1, "/c/B.cbz");
        b.enable_proposed = false;
        let mut h = Harness::new(p, vec![b]);
        assert_eq!(h.eval(0, "{<series>}"), "Unknown");
    }

    #[test]
    fn illegal_characters() {
        let mut p = Profile::default();
        p.illegal_characters.insert(":".into(), " - ".into());
        p.illegal_characters.insert("?".into(), "".into());
        let mut h = Harness::new(p, vec![book("Bat?man: Zero", "1", 2012, 3, 1, "/c/B.cbz")]);
        assert_eq!(h.eval(0, "{<series>}"), "Batman -  Zero");
    }

    #[test]
    fn first_letter_skips_articles() {
        let mut h = Harness::new(
            Profile::default(),
            vec![book("The Batman", "1", 2012, 3, 1, "/c/B.cbz")],
        );
        assert_eq!(h.eval(0, "{<first(Series)>}"), "B");
        // A one-character value has no match (the `.+` guard).
        let mut h2 = Harness::new(
            Profile::default(),
            vec![book("V", "1", 2012, 3, 1, "/c/B.cbz")],
        );
        assert_eq!(h2.eval(0, "{<first(Series)>}"), "");
    }

    #[test]
    fn counter_runs_across_evaluations() {
        let mut h = Harness::new(
            Profile::default(),
            vec![
                book("B", "1", 2012, 3, 1, "/c/B.cbz"),
                book("B", "2", 2012, 4, 1, "/c/B2.cbz"),
            ],
        );
        assert_eq!(h.eval(0, "{<counter(1)(1)(3)>}"), "001");
        assert_eq!(h.eval(1, "{<counter(1)(1)(3)>}"), "002");
    }

    #[test]
    fn read_percentage_conditional() {
        let mut b = book("B", "1", 2012, 3, 1, "/c/B.cbz");
        b.info.page_count = 10;
        b.last_page_read = 9; // (9+1)*100/10 = 100
        let mut h = Harness::new(Profile::default(), vec![b]);
        assert_eq!(h.eval(0, "{<read( read )(>)(99)>}"), " read ");
        assert_eq!(h.eval(0, "{<read( read )(=)(99)>}"), "");
    }

    #[test]
    fn yes_no_tokens() {
        let mut b = book("B", "1", 2012, 3, 1, "/c/B.cbz");
        b.info.manga = cr_core::model::enums::MangaYesNo::Yes;
        let mut h = Harness::new(Profile::default(), vec![b]);
        assert_eq!(h.eval(0, "{<manga( manga )>}"), " manga ");
        let mut b = book("B", "1", 2012, 3, 1, "/c/B.cbz");
        b.info.manga = cr_core::model::enums::MangaYesNo::No;
        let mut h2 = Harness::new(Profile::default(), vec![b]);
        assert_eq!(h2.eval(0, "{<manga( manga )(!)>}"), " manga ");
    }

    #[test]
    fn inversion_on_empty_field() {
        let mut b = book("", "5", 2012, 3, 1, "/c/B.cbz");
        b.enable_proposed = false;
        let mut h = Harness::new(Profile::default(), vec![b]);
        assert_eq!(h.eval(0, "{<!title>No title}"), "No title");
        let mut b2 = book("", "5", 2012, 3, 1, "/c/B.cbz");
        b2.info.title = "T".into();
        let mut h2 = Harness::new(Profile::default(), vec![b2]);
        assert_eq!(h2.eval(0, "{<!title>No title}"), "");
    }

    #[test]
    fn make_path_semantics() {
        let p = Profile {
            base_folder: "/lib".into(),
            ..Profile::default()
        };
        let mut h = Harness::new(p, vec![book("Batman", "5", 2012, 3, 1, "/comics/B.cbz")]);
        let (folder, file, failed) = h.make_path(
            0,
            "{<publisher>}\\{<series>}{ (<startyear>)}",
            "{<series>}{ #<number2>}",
        );
        assert!(!failed);
        assert_eq!(folder, "/lib/Batman (2012)");
        assert_eq!(file, "Batman #05.cbz");
    }

    #[test]
    fn multi_space_collapse() {
        assert_eq!(collapse_spaces("a  b   c"), "a b c");
    }

    #[test]
    fn net_date_formats() {
        let d = CrDateTime {
            naive: chrono::NaiveDate::from_ymd_opt(2013, 2, 1)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
            kind: cr_core::xml::scalar::DateKind::Unspecified,
        };
        assert_eq!(
            d.naive.format(&net_date_format("dd/MM/yy")).to_string(),
            "01/02/13"
        );
        assert_eq!(
            d.naive.format(&net_date_format("MMM d, yyy")).to_string(),
            "Feb 1, 2013"
        );
        assert_eq!(
            d.naive.format(&net_date_format("MMMM dd, yyy")).to_string(),
            "February 01, 2013"
        );
        assert_eq!(
            d.naive.format(&net_date_format("D")).to_string(),
            "Friday, February 1, 2013"
        );
        assert_eq!(
            d.naive.format(&net_date_format("Y")).to_string(),
            "February 2013"
        );
    }

    #[test]
    fn path_combine_semantics() {
        assert_eq!(path_combine("", "b"), "b");
        assert_eq!(path_combine("a", ""), "a");
        assert_eq!(path_combine("a", "b"), "a/b");
        assert_eq!(path_combine("a", "/b"), "/b");
    }

    #[test]
    fn path_extension_semantics() {
        assert_eq!(path_extension("/c/B.cbz"), ".cbz");
        assert_eq!(path_extension("/c/B"), "");
        assert_eq!(path_extension("/c/archive.tar.gz"), ".gz");
    }
}
