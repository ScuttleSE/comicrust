//! `TextNumberFloat` / `ComicTextNumberFloat` ports: the text-to-number
//! conversion comic numbering relies on ("12a" → 12, "1/2" → 0.5).

/// `StringUtility.TryParse(this string, out float, invariant)`:
/// the leftmost match of `[-+]?\d*\.?\d+` parsed invariantly.
pub fn parse_float_prefix(text: &str) -> Option<f32> {
    let b = text.as_bytes();
    for i in 0..b.len() {
        let mut j = i;
        if b[j] == b'-' || b[j] == b'+' {
            j += 1;
        }
        let digits_start = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        let mut end = j;
        if j < b.len() && b[j] == b'.' {
            let mut k = j + 1;
            while k < b.len() && b[k].is_ascii_digit() {
                k += 1;
            }
            // The dot branch requires at least one trailing digit.
            if k > j + 1 {
                end = k;
            }
        }
        if end > digits_start {
            return text[i..end].parse::<f32>().ok();
        }
    }
    None
}

/// `ComicTextNumberFloat`: "1/2" is 0.5; otherwise the first float
/// prefix of the text.
pub fn parse_comic_number(text: &str) -> (bool, f32) {
    let t = text.trim();
    if t == "1/2" {
        return (true, 0.5);
    }
    match parse_float_prefix(t) {
        Some(f) => (true, f),
        None => (false, 0.0),
    }
}

/// `TextNumberFloat.GetRange`: a `\d+[ -]+\d+` span expands to the
/// numbers a..=b step 1 (used by the series gap statistics).
pub fn number_range(text: &str) -> Vec<f32> {
    let b = text.as_bytes();
    // Find `\d+ [sep]+ \d+` where [sep] is space or dash.
    let mut ranges = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        if !b[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let a_start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        let a_end = i;
        let mut j = i;
        while j < b.len() && (b[j] == b' ' || b[j] == b'-') {
            j += 1;
        }
        if j == i || j >= b.len() || !b[j].is_ascii_digit() {
            continue;
        }
        let b_start = j;
        while j < b.len() && b[j].is_ascii_digit() {
            j += 1;
        }
        let (Some(a), Some(bn)) = (
            text[a_start..a_end].parse::<f32>().ok(),
            text[b_start..j].parse::<f32>().ok(),
        ) else {
            i = a_end;
            continue;
        };
        if bn > a {
            let mut d = a;
            while d <= bn {
                ranges.push(d);
                d += 1.0;
            }
        }
        i = j;
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_prefix() {
        assert_eq!(parse_float_prefix("12a"), Some(12.0));
        assert_eq!(parse_float_prefix("-3.5b"), Some(-3.5));
        assert_eq!(parse_float_prefix("+7"), Some(7.0));
        assert_eq!(parse_float_prefix(".5"), Some(0.5));
        assert_eq!(parse_float_prefix("abc"), None);
        assert_eq!(parse_float_prefix(""), None);
    }

    #[test]
    fn comic_number() {
        assert_eq!(parse_comic_number("1/2"), (true, 0.5));
        assert_eq!(parse_comic_number("12A"), (true, 12.0));
        assert_eq!(parse_comic_number("Vol. 3"), (true, 3.0));
        assert_eq!(parse_comic_number("-"), (false, 0.0));
    }

    #[test]
    fn ranges() {
        assert_eq!(number_range("1-3"), [1.0, 2.0, 3.0]);
        assert_eq!(number_range("5 7"), [5.0, 6.0, 7.0]);
        assert_eq!(number_range("3"), Vec::<f32>::new());
        assert_eq!(number_range("7-2"), Vec::<f32>::new());
    }
}
