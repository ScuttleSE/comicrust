//! Small helpers ported from the plugin's `utils.py` — the subset the
//! current modules need. More ports land here as later tasks consume
//! them.

use std::cmp::Ordering;
use std::sync::LazyLock;

use fancy_regex::Regex;

/// Port of `utils.is_number`: true when the string converts to a
/// number (Python `float()` semantics, whitespace tolerated).
pub fn is_number(s: &str) -> bool {
    s.trim().parse::<f64>().is_ok()
}

/// Python `str(float)` formatting: integral floats keep a `.0`
/// (`"21.0000000"` parses to `21.0` and formats back as `"21.0"`),
/// everything else uses the shortest round-trip form (`"7.1"`).
pub fn py_float_string(f: f64) -> String {
    if f.is_finite() && f.fract() == 0.0 {
        format!("{:.1}", f)
    } else {
        format!("{}", f)
    }
}

// ==========================================================================
// natural keys (utils.natural_key / natural_compare)

/// One element of a natural key: Python 2 compares every number less
/// than every string, so the ordering is `Num < Text`. `NaN` never
/// enters a key (parse errors become text), so Eq/Ord are sound.
#[derive(Clone, Debug, PartialEq)]
pub enum NatPart {
    Num(f64),
    Text(String),
}

// f64 ordering must be total for Eq/Ord; NaN never enters a key
// (parse errors become Text), so this is sound here.
impl Eq for NatPart {}

impl Ord for NatPart {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (NatPart::Num(a), NatPart::Num(b)) => a.partial_cmp(b).unwrap_or(Ordering::Equal),
            (NatPart::Num(_), NatPart::Text(_)) => Ordering::Less,
            (NatPart::Text(_), NatPart::Num(_)) => Ordering::Greater,
            (NatPart::Text(a), NatPart::Text(b)) => a.cmp(b),
        }
    }
}

impl PartialOrd for NatPart {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The Unicode fractions the plugin understands, with their values
/// (`⅛`..`⅞`, in the C#'s table order).
fn fraction_value(fracchar: &str) -> f64 {
    match fracchar {
        "⅛" => 1.0 / 8.0,
        "⅙" => 1.0 / 6.0,
        "⅕" => 0.2,
        "¼" => 0.25,
        "⅓" => 1.0 / 3.0,
        "⅜" => 3.0 / 8.0,
        "⅖" => 0.4,
        "½" => 0.5,
        "⅗" => 0.6,
        "⅝" => 5.0 / 8.0,
        "⅔" => 2.0 / 3.0,
        "¾" => 0.75,
        "⅘" => 0.8,
        "⅚" => 5.0 / 6.0,
        "⅞" => 7.0 / 8.0,
        _ => 0.0,
    }
}

static FRACTION_MATCH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A\s*(-?)\s*(\d*)\s*([⅛⅙⅕¼⅓⅜⅖½⅗⅝⅔¾⅘⅚⅞])\s*").unwrap());
static SPLIT_NUM: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"((?:\A\s*-)?(?:(?:\d+\.\d+)|(?:\.\d+)|(?:\d+\.)|(?:\d+)))").unwrap()
});

/// The C# `unicode_fraction_to_float`: anchored at the start, `5½` →
/// 5.5, `½` → 0.5, `-½` → -0.5. Returns None when the string does not
/// start with (optional digits +) a fraction character.
fn fraction_to_float(s: &str) -> Option<f64> {
    let caps = FRACTION_MATCH.captures(s).ok().flatten()?;
    let negative = caps.get(1).map_or("", |m| m.as_str()) == "-";
    let intpart = caps.get(2).map_or("", |m| m.as_str());
    let fracchar = caps.get(3)?.as_str();
    let intpart: f64 = if intpart.is_empty() {
        0.0
    } else {
        intpart.parse().ok()?
    };
    let number = intpart + fraction_value(fracchar);
    Some(if negative { -number } else { number })
}

/// Port of `utils.natural_key`: two strings that are "naturally"
/// identical produce identical keys (`1` and `1.0`, `003a` and
/// `3a  `, `½` and `0.5000`). The shape mirrors Python
/// `re.split(pattern, s)` with a capturing group: every split piece
/// (including empty pre/post strings) becomes a key part.
pub fn natural_key(s: &str) -> Vec<NatPart> {
    let s = s.trim();
    // C# truthiness: a fraction result of 0.0 falls through to the
    // split path (unreachable through the regex — kept for parity)
    if let Some(f) = fraction_to_float(s) {
        if f != 0.0 {
            return vec![
                NatPart::Text(String::new()),
                NatPart::Num(f),
                NatPart::Text(String::new()),
            ];
        }
    }
    let mut parts = Vec::new();
    let mut last = 0;
    for whole in SPLIT_NUM.find_iter(s).flatten() {
        parts.push(convert_part(&s[last..whole.start()]));
        parts.push(convert_part(whole.as_str()));
        last = whole.end();
    }
    parts.push(convert_part(&s[last..]));
    parts
}

/// The C# `convert`: a float when the text is a number, else the
/// lowercased, trimmed text.
fn convert_part(text: &str) -> NatPart {
    match text.trim().parse::<f64>() {
        Ok(f) => NatPart::Num(f),
        Err(_) => NatPart::Text(text.trim().to_lowercase()),
    }
}

/// Port of `utils.natural_compare`: orders by the natural keys.
pub fn natural_compare(a: &str, b: &str) -> Ordering {
    natural_key(a).cmp(&natural_key(b))
}

// ==========================================================================
// number words (utils.convert_number_words)

/// The number-word table, longest keys first so expansion/contraction
/// never clobbers a longer key (`1rst` must expand before `1` runs).
static NUMBER_WORDS: &[(&str, &str)] = &[
    ("1rst", "first"),
    ("0th", "zeroth"),
    ("13th", "thirteenth"),
    ("14th", "fourteenth"),
    ("15th", "fifteenth"),
    ("16th", "sixteenth"),
    ("17th", "seventeenth"),
    ("18th", "eighteenth"),
    ("19th", "nineteenth"),
    ("20th", "twentieth"),
    ("11th", "eleventh"),
    ("12th", "twelveth"),
    ("10th", "tenth"),
    ("2nd", "second"),
    ("3rd", "third"),
    ("4th", "fourth"),
    ("5th", "fifth"),
    ("6th", "sixth"),
    ("7th", "seventh"),
    ("8th", "eighth"),
    ("9th", "ninth"),
    ("13", "thirteen"),
    ("14", "fourteen"),
    ("15", "fifteen"),
    ("16", "sixteen"),
    ("17", "seventeen"),
    ("18", "eighteen"),
    ("19", "nineteen"),
    ("20", "twenty"),
    ("10", "ten"),
    ("11", "eleven"),
    ("12", "twelve"),
    ("0", "zero"),
    ("1", "one"),
    ("2", "two"),
    ("3", "three"),
    ("4", "four"),
    ("5", "five"),
    ("6", "six"),
    ("7", "seven"),
    ("8", "eight"),
    ("9", "nine"),
];

/// Port of `utils.convert_number_words`: converts number words
/// (numbers up to 20) between digit and word forms. Expansion turns
/// `1`/`2nd` into `one`/`second`; contraction goes in reverse (with
/// the C#'s `twelfth`/`eightteenth` repair forms). Lowercase input
/// expected.
pub fn convert_number_words(phrase: &str, expand: bool) -> String {
    let mut out = phrase.to_string();
    if expand {
        for (digit, word) in NUMBER_WORDS {
            out = replace_word(&out, digit, word);
        }
        out = replace_word(&out, "1st", "first");
    } else {
        for (digit, word) in NUMBER_WORDS {
            out = replace_word(&out, word, digit);
        }
        out = replace_word(&out, "twelfth", "12th");
        out = replace_word(&out, "eightteenth", "18th");
    }
    out
}

/// `re.sub(r'\b' + word + r'\b', replacement, s)` — a whole-word
/// replace, case-sensitive like the C#.
fn replace_word(s: &str, word: &str, replacement: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(idx) = find_whole_word(rest, word) {
        result.push_str(&rest[..idx]);
        result.push_str(replacement);
        rest = &rest[idx + word.len()..];
    }
    result.push_str(rest);
    result
}

fn find_whole_word(haystack: &str, needle: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(rel) = haystack[from..].find(needle) {
        let start = from + rel;
        let end = start + needle.len();
        let before_ok = haystack[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !is_word_char(c));
        let after_ok = haystack[end..]
            .chars()
            .next()
            .is_none_or(|c| !is_word_char(c));
        if before_ok && after_ok {
            return Some(start);
        }
        from = start + 1;
    }
    None
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}
