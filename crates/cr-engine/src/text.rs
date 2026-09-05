//! Ports of the cYo.Common string helpers the query language needs:
//! `StringUtility.Escape`/`Unescape` and `Intent`.
//!
//! The C# implementations are sequential whole-string replaces, not a
//! proper escape parser. The iteration order is load-bearing:
//!
//! - `Escape(chars, '\\')` replaces the escape char FIRST, then each
//!   listed char (`\` → `\\` before `"` → `\"`).
//! - `Unescape(chars, '\\')` replaces each listed char FIRST, then the
//!   escape char (`\"` → `"` before `\\` → `\`).
//!
//! The orders differ, so this is not a textbook escape pair. Port both
//! exactly; `Escape` then `Unescape` still round-trips for C#-escaped
//! text (verified by tests).

/// Newline the C# app writes (`Environment.NewLine` on Windows, where
/// ComicRack runs; reference output is Windows).
pub const NL: &str = "\r\n";

/// `StringUtility.Escape(text, characters, escape)`.
pub fn escape(text: &str, chars: &[char], esc: char) -> String {
    let mut out: Vec<(String, String)> = Vec::with_capacity(chars.len() + 1);
    out.push((esc.to_string(), format!("{esc}{esc}")));
    for c in chars {
        out.push((c.to_string(), format!("{esc}{c}")));
    }
    apply_replaces(text, &out)
}

/// `StringUtility.Unescape(text, characters, escape)`.
pub fn unescape(text: &str, chars: &[char], esc: char) -> String {
    let mut out: Vec<(String, String)> = Vec::with_capacity(chars.len() + 1);
    for c in chars {
        out.push((format!("{esc}{c}"), c.to_string()));
    }
    out.push((format!("{esc}{esc}"), esc.to_string()));
    apply_replaces(text, &out)
}

/// `text.Escape()` — for double-quoted strings.
pub fn escape_default(text: &str) -> String {
    escape(text, &['"'], '\\')
}

/// `text.Unescape()` — for double-quoted strings.
pub fn unescape_default(text: &str) -> String {
    unescape(text, &['"'], '\\')
}

/// `text.Escape("[]", '\\')` — for the `[MatcherName]` token.
pub fn escape_brackets(text: &str) -> String {
    escape(text, &['[', ']'], '\\')
}

/// `text.Unescape("[]", '\\')` — for the `[MatcherName]` token.
pub fn unescape_brackets(text: &str) -> String {
    unescape(text, &['[', ']'], '\\')
}

fn apply_replaces(text: &str, pairs: &[(String, String)]) -> String {
    let mut out = text.to_string();
    for (from, to) in pairs {
        out = out.replace(from.as_str(), to.as_str());
    }
    out
}

/// `NumberedString.GetNumber`: the number inside the FIRST
/// `(\d+)` bracket group of the text (0 when none).
pub fn get_number(s: &str) -> i32 {
    number_match(s).unwrap_or(0)
}

/// `NumberedString.StripNumber`: every `\s*\((\d+)\)` match (leading
/// whitespace included) is removed — `Regex.Replace` replaces ALL
/// matches, not just the first.
pub fn strip_number(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    let mut out: Vec<char> = Vec::with_capacity(n);
    let mut i = 0;
    while i < n {
        if chars[i] == '(' {
            let mut j = i + 1;
            while j < n && chars[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 1 && j < n && chars[j] == ')' {
                // The match includes the `\s*` before the bracket:
                // drop the whitespace already copied.
                while out.last().is_some_and(|c| c.is_whitespace()) {
                    out.pop();
                }
                i = j + 1;
                continue;
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out.into_iter().collect()
}

/// `NumberedString.MaxNumber`: max(GetNumber + 1) over the texts
/// (0 for an empty sequence — the C# catch-all).
pub fn max_number<'a, I: Iterator<Item = &'a str>>(texts: I) -> i32 {
    texts.map(|t| get_number(t) + 1).max().unwrap_or(0)
}

/// `NumberedString.Format`: `"{name} ({n})"` for n >= 2.
pub fn format_numbered(name: &str, number: i32) -> String {
    if number >= 2 {
        return format!("{name} ({number})");
    }
    name.to_string()
}

/// The first `\s*\(\d+\)` match — the digits (the C# `rxBrackets`
/// takes the first match anywhere in the text).
fn number_match(s: &str) -> Option<i32> {
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'(' {
            let mut j = i + 1;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > i + 1 && j < bytes.len() && bytes[j] == b')' {
                return s[i + 1..j].parse().ok();
            }
        }
        i += 1;
    }
    None
}

/// `StringUtility.Intent(s, n)`: indent every line by `n` spaces. The
/// input is split on `\n` after normalizing `\r\n` to `\n`; lines are
/// rejoined with [`NL`].
pub fn intent(s: &str, indentation: usize) -> String {
    let pad = " ".repeat(indentation);
    let normalized = s.replace(NL, "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();
    let mut out = String::new();
    for (i, line) in lines.iter().enumerate() {
        out.push_str(&pad);
        out.push_str(line);
        if i != lines.len() - 1 {
            out.push_str(NL);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_unescape_default_round_trip() {
        for text in [
            "plain",
            "with \"quotes\"",
            "back\\slash",
            "both \\ and \"",
            "\\\" tricky",
            "\\\\\"",
        ] {
            let esc = escape_default(text);
            assert_eq!(
                unescape_default(&esc),
                text,
                "round trip failed for {text:?}"
            );
        }
    }

    #[test]
    fn escape_default_csharp_order() {
        // Escape replaces `\` first, then `"`.
        assert_eq!(escape_default("a\"b\\c"), "a\\\"b\\\\c");
        // Unescape replaces `\"` first, then `\\`.
        assert_eq!(unescape_default("a\\\"b\\\\c"), "a\"b\\c");
    }

    #[test]
    fn brackets() {
        assert_eq!(escape_brackets("Na[me]x"), "Na\\[me\\]x");
        assert_eq!(unescape_brackets("Na\\[me\\]x"), "Na[me]x");
        assert_eq!(escape_brackets("back\\slash"), "back\\\\slash");
    }

    #[test]
    fn intent_indents_every_line() {
        assert_eq!(intent("a", 4), "    a");
        assert_eq!(intent("a\r\nb", 4), "    a\r\n    b");
        assert_eq!(intent("a\r\nb\r\n\r\nc", 2), "  a\r\n  b\r\n  \r\n  c");
    }

    /// `NumberedString` (cYo.Common.Text): first-match GetNumber,
    /// replace-all StripNumber with the `\s*` prefix, and the
    /// Format numbering.
    #[test]
    fn numbered_string() {
        assert_eq!(get_number("Batman (2016)"), 2016);
        assert_eq!(get_number("My List (2)"), 2);
        assert_eq!(get_number("no number"), 0);
        // First bracket group wins.
        assert_eq!(get_number("a (1) b (2)"), 1);
        // StripNumber removes EVERY group plus the leading space.
        assert_eq!(strip_number("Batman (2016)"), "Batman");
        assert_eq!(strip_number("a (1) b (2)"), "a b");
        assert_eq!(strip_number("plain"), "plain");
        assert_eq!(strip_number("parens (x) stay"), "parens (x) stay");
        assert_eq!(max_number(["a (2)", "b (5)"].iter().copied()), 6);
        assert_eq!(max_number(std::iter::empty::<&str>()), 0);
        assert_eq!(format_numbered("x", 1), "x");
        assert_eq!(format_numbered("x", 2), "x (2)");
    }
}
