//! Port of the Comic Vine Scraper's `fnameparser.py` — extracts the
//! series name, issue number, and volume year out of a comic book
//! filename. The two public functions mirror the Python module:
//! `extract` (the default algorithm) and `regex` (a user-supplied
//! pattern with `series`/`num`/`year` groups).
//!
//! Python regex semantics are reproduced deliberately: `re.match`
//! anchoring (`captures_at(0)`), non-overlapping `replace_all` scans,
//! fixed-length lookbehind/ahead via fancy-regex, and the character
//! classes exactly as written (`[, -_]` is a range `0x20..0x5F` whose
//! lowercase fold-ins count under `(?i)`).

use std::sync::{LazyLock, Mutex};

use fancy_regex::Regex;

// `re.match(...)` anchored patterns (the Python calls `re.match`).
static RULE1: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(\d+)[\s._-]+([^#]+?#-?\d+.*)").unwrap());
static RULE2: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^((?:[a-zA-Z,.-]+\s+)+#?(?:\d+[.0-9]*))\s*(?:-).*?((?:\(.*)?)$").unwrap()
});
static IS_2000AD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A(?i)\s*2000[\s\.-_]*a[\s.-_]*d.*").unwrap());
static IS_BEANO: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\A(?i)\s*the[\s\.-_]+beano[\s.-_]+#?\d{4}").unwrap());
static ZERO_STRIP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^(0+)([0-9].*)$").unwrap());
static NUM_ONLY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^-?[.0-9]+$").unwrap());
static RL_STRIP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*\d+(\.\s+|\s*-\s*(?=\D))").unwrap());

// `re.sub` / `re.findall` patterns (search-anywhere semantics).
static SUB_PAREN: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\([^(]*?\)").unwrap());
static SUB_BRACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{[^{]*?\}").unwrap());
static SUB_SQUARE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[[^\[]*?\]").unwrap());
static VYEAR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(^|[, -_])v(\d{4})($|[, -_])").unwrap());
static YBRACKET: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\([^\[\](){}]*?\)").unwrap());
static YSQUARE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[[^\[\](){}]*?\]").unwrap());
static YBRACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\{[^\[\](){}]*?\}").unwrap());
static YRANGE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(\d{4})\s*-\s*\d{1,4}").unwrap());
static UNDERSCORE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"_").unwrap());
static VOLUME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(\b((v|vol)\.?|volume))\s*-?\s*[0-9]+[.0-9a-z]*").unwrap());
static PAGES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b[.,]?\s*\d+\s*(p|pg|pgs|pages)\b[.,]?").unwrap());
static COVERS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(\d+\s*of\s*\d+\s*covers)").unwrap());
static OF_LANG: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)(?<=\d)(\s*(of|de|di|von|van|z)\s*#*\d+)").unwrap());
static DASH_NUM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?<=\d)(-\d+)").unwrap());
static DASH_SEP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?<![-_# ])-").unwrap());
static NUMBERS: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(^|[_\s#])(-?\d*\.?\d\w*)").unwrap());
static WS2: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s{2,}").unwrap());

// `fnameparser.regex` caches the regex string that failed to compile
// or produced no series (Python `__failed_regex`).
static FAILED_REGEX: LazyLock<Mutex<Option<String>>> = LazyLock::new(|| Mutex::new(None));

/// One "issue-number-like" substring found in a working name: the
/// full match span (including the leading separator character) plus
/// the number text and its own span start (group 2).
struct NumMatch {
    start: usize,
    end: usize,
    start_of_num: usize,
    num: String,
}

/// Port of `fnameparser.regex`: extracts `[series, num, year]` from a
/// filename with a user-supplied regular expression that must match
/// from the start and define a non-blank `series` group (`num` and
/// `year` are optional). Returns `None` when the regex is unusable or
/// yields no series — a regex that fails once is remembered and
/// returns `None` right away afterwards (C# parity).
pub fn regex(filename: &str, user_regex: &str) -> Option<[String; 3]> {
    if FAILED_REGEX
        .lock()
        .unwrap()
        .as_deref()
        .is_some_and(|f| f == user_regex)
    {
        return None;
    }
    let wrapped = format!(r"\A(?:{})", user_regex);
    let re = match Regex::new(&wrapped) {
        Ok(re) => re,
        Err(_) => {
            *FAILED_REGEX.lock().unwrap() = Some(user_regex.to_string());
            return None;
        }
    };
    let caps = match re.captures(filename) {
        Ok(Some(caps)) => caps,
        _ => return None,
    };
    let nonblank = |name: &str| {
        caps.name(name)
            .map(|m| m.as_str())
            .filter(|v| !v.trim().is_empty())
    };
    let series = nonblank("series")?;
    let num = nonblank("num").unwrap_or("");
    let year = nonblank("year").unwrap_or("");
    Some([series.to_string(), num.to_string(), year.to_string()])
}

/// Port of `fnameparser.extract`: always returns a triple
/// `[series, issue number, volume year]`, with at least a non-empty
/// series name (falling back to the whole filename when the parse
/// comes up blank).
pub fn extract(filename: &str) -> [String; 3] {
    // remove the file extension, unless it's the whole filename
    let base = basename(filename.trim());
    let mut name: String = match base.rfind('.') {
        Some(i) if i > 0 => base[..i].to_string(),
        _ => base.to_string(),
    };

    // 1. "nnn series name #xx (etc) (etc)" -> "series name #xx (etc) (etc)"
    if let Ok(Some(caps)) = RULE1.captures(&name) {
        if let Some(g2) = caps.get(2) {
            name = g2.as_str().to_string();
        }
    }

    // 2. "series name #xxx - title (etc) (etc)" -> "series name #xxx (etc)
    //    (etc)" — the "- title" part before the brackets is dropped
    if let Ok(Some(caps)) = RULE2.captures(&name) {
        let g1 = caps.get(1).map_or("", |m| m.as_str());
        let g2 = caps.get(2).map_or("", |m| m.as_str());
        name = format!("{} {}", g1, g2);
    }

    let extracted = extract_inner(&name);
    if !extracted[0].trim().is_empty() {
        extracted
    } else {
        [name, String::new(), String::new()]
    }
}

fn extract_inner(name: &str) -> [String; 3] {
    // 1-2. extract the volume year before anything else is stripped
    let volume_year = extract_year(name);

    let mut s = name.to_string();

    // 3. strip out all bracketed data (recurse until no pairs remain)
    for re in [&SUB_PAREN, &SUB_BRACE, &SUB_SQUARE] {
        while re.is_match(&s).unwrap_or(false) {
            s = re.replace_all(&s, "").to_string();
        }
    }

    // 4. clean out underscores
    s = UNDERSCORE.replace_all(&s, " ").to_string();

    // 5. remove all trace of volume ("vol. 2a", "vol -3.1", "v5")
    s = VOLUME.replace_all(&s, "").to_string();

    // 6. remove page counts ("245p", "50 pages")
    s = PAGES.replace_all(&s, "").to_string();

    // 7. remove "02 of 02 covers"
    s = COVERS.replace_all(&s, "").to_string();

    // 8. remove " of 5" / "-6" suffixes ("of" in several languages)
    s = OF_LANG.replace_all(&s, "").to_string();
    s = DASH_NUM.replace_all(&s, "").to_string();

    // 9. dashes-as-spaces, but only when the name has no spaces at all
    if s.contains('-') && !s.contains(' ') {
        s = DASH_SEP.replace_all(&s, " ").to_string();
    }

    // 10. collect the issue-number-like substrings
    let mut matches = extract_numbers(&s);

    // 11. reading-list prefixes ("05. ", "12 - ") get stripped once,
    //     but only when more than one number is present
    if matches.len() > 1 && rl_strip_matches(&s) {
        s = RL_STRIP.replace(&s, "").to_string();
        matches = extract_numbers(&s);
    }

    // 12. the LAST number is the issue number; remove it from the name
    let (series, issue_num) = match matches.last() {
        Some(last) => {
            let mut series = String::new();
            series.push_str(&s[..last.start]);
            series.push_str(&s[last.end..]);
            let mut issue_num = last.num.clone();
            // strip off leading zeroes
            if let Ok(Some(caps)) = ZERO_STRIP.captures(&issue_num) {
                if let Some(g2) = caps.get(2) {
                    issue_num = g2.as_str().to_string();
                }
            }
            if NUM_ONLY.is_match(&issue_num).unwrap_or(false) && crate::utils::is_number(&issue_num)
            {
                issue_num = if issue_num.contains('.') {
                    crate::utils::py_float_string(issue_num.parse().unwrap())
                } else {
                    issue_num.parse::<i64>().unwrap().to_string()
                };
            }
            (series, issue_num)
        }
        None => (s.clone(), String::new()),
    };

    // 13. contract repeating whitespace, strip bad chars off the ends
    let series = WS2.replace_all(&series, " ").to_string();
    let series = series
        .trim_matches(|c| c == ' ' || c == ',' || c == '-' || c == '_')
        .to_string();

    [series, issue_num, volume_year]
}

fn rl_strip_matches(s: &str) -> bool {
    matches!(RL_STRIP.find(s), Ok(Some(_)))
}

/// Port of `__extract_year`: the `V2003` form first (used only when
/// exactly one valid result exists), then the last valid year inside
/// any bracket, with year ranges collapsed to their start.
fn extract_year(s: &str) -> String {
    // type one years appear exactly as "V2003"
    let mut vresults = Vec::new();
    for caps in VYEAR.captures_iter(s).flatten() {
        if let Some(y) = caps.get(2) {
            if is_year(y.as_str()) {
                vresults.push(y.as_str().to_string());
            }
        }
    }
    if vresults.len() == 1 {
        return vresults.remove(0);
    }

    // roughly, we're looking for a year or year range inside brackets:
    // so [2003], (2004-6), {2000-2010}, etc.
    let mut results = Vec::new();
    for re in [&YBRACKET, &YSQUARE, &YBRACE] {
        for caps in re.captures_iter(s).flatten() {
            if let Some(m) = caps.get(0) {
                results.push(m.as_str().to_string());
            }
        }
    }
    // strip off the outer brackets and spaces
    let mut results: Vec<String> = results
        .iter()
        .map(|x| x.trim_matches(|c| "()[]{}".contains(c)).trim().to_string())
        .collect();
    // a year range collapses to its start ("2006-2009" -> "2006")
    results = results
        .iter()
        .map(|x| YRANGE.replace_all(x, "$1").to_string())
        .collect();
    // only valid 4 digit years survive; the last one wins
    results
        .iter()
        .rev()
        .find(|x| is_year(x))
        .cloned()
        .unwrap_or_default()
}

/// Port of `__extract_numbers`: the ordered list of
/// "issue-number-like" matches, minus 4-digit years (except on the
/// 2000AD and The Beano series, and years prefixed with `#`).
fn extract_numbers(s: &str) -> Vec<NumMatch> {
    let mut matches = Vec::new();
    for caps in NUMBERS.captures_iter(s).flatten() {
        let (Some(whole), Some(num)) = (caps.get(0), caps.get(2)) else {
            continue;
        };
        matches.push(NumMatch {
            start: whole.start(),
            end: whole.end(),
            start_of_num: num.start(),
            num: num.as_str().to_string(),
        });
    }
    let is_2000ad = matches!(IS_2000AD.captures(s), Ok(Some(_)));
    let is_beano = matches!(IS_BEANO.captures(s), Ok(Some(_)));
    if !is_2000ad && !is_beano {
        matches.retain(|m| {
            !is_year(&m.num) || (m.start_of_num > 0 && char_before(s, m.start_of_num) == Some('#'))
        });
    }
    matches
}

/// Port of `__isYear`: four digits, strictly inside 1900..2100.
fn is_year(d: &str) -> bool {
    d.len() == 4
        && d.chars().all(|c| c.is_ascii_digit())
        && d.parse::<i32>().is_ok_and(|y| y > 1900 && y < 2100)
}

fn char_before(s: &str, byte_index: usize) -> Option<char> {
    s[..byte_index].chars().next_back()
}

fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}
