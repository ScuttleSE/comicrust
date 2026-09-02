//! Port of `cYo.Common.Text.Tokenizer` plus the smart-list query
//! tokenizer regex from `ComicSmartListItem.rxTokenizer`.
//!
//! The C# tokenizer runs a regex over the source and hands out the
//! matches as tokens. The regex (`RegexOptions.IgnoreCase | Multiline`,
//! no Singleline) is:
//!
//! ```text
//! (?<!\\)\".*?((?<!\\)\"|$)      (1) quoted string, possibly unclosed
//! | (?<!\\)\[.*?((?<!\\)\]|$)    (2) bracketed string, possibly unclosed
//! | (?<=\]\s+)\s*Match           (3) the word Match after `]` + whitespace
//! | (?<=\]\s+)[\w\s]+            (4) word/whitespace run after `]` + whitespace
//! | [\w]+                        (5) plain word
//! | { | } | , | ;                (6) literals
//! ```
//!
//! Grammar notes (derived; the C# has no formal grammar):
//!
//! - Matches are non-overlapping, found left to right; characters that
//!   start no alternative are skipped one by one.
//! - `.` does not match `\n`, so quoted/bracketed strings end at the
//!   line end when unclosed (`$` with Multiline).
//! - The lookbehind `(?<=\]\s+)` succeeds at a position when the text
//!   immediately before it is a `]` followed by one or more whitespace
//!   characters. At such a position alternative (4) consumes word AND
//!   whitespace characters greedily, so a multi-word operator like
//!   `equals yes` or `is in the range` arrives as ONE token, and text
//!   like `Not Match` after a `]` becomes a single token. The C#
//!   renderer never emits the latter shape, but the parser accepts
//!   whatever the regex accepts.
//! - The escape test is one preceding character (`(?<!\\)`): in
//!   `"a\\"` the closing quote counts as escaped and the string stays
//!   open. This is faithful regex behavior, not a bug fix.
//! - Every token text is trimmed (`Tokenizer(trim: true)`).
//!
//! This module emulates those semantics with a scanner; no regex
//! engine is used. `fancy-regex` rejects the variable-length lookbehind
//! (`LookBehindNotConst`), so a direct regex port is impossible.

/// Token as produced by [`Tokenizer::new`] / [`tokenize`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub text: String,
    /// Byte index of the match in the source.
    pub index: usize,
    /// Byte length of the match in the source.
    pub length: usize,
}

impl Token {
    /// C# `Token.Is`: case-insensitive comparison against any of `p`.
    pub fn is(&self, p: &[&str]) -> bool {
        p.iter().any(|s| self.text.eq_ignore_ascii_case(s))
    }
}

/// Parse failure, mirroring `Tokenizer.ParseException`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(pub String);

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseError {}

fn is_word(c: char) -> bool {
    // .NET `\w` (default, non-ECMAScript): Unicode word characters.
    c.is_alphanumeric() || c == '_'
}

/// `(?<=\]\s+)` at position `i`: the text before `i` must end with a
/// `]` followed by one or more whitespace characters.
fn lookbehind_bracket_ws(src: &str, i: usize) -> bool {
    let mut saw_ws = false;
    for c in src[..i].chars().rev() {
        if c.is_whitespace() {
            saw_ws = true;
        } else {
            return saw_ws && c == ']';
        }
    }
    false
}

/// Lazy `.*?` run starting at `start` (the position AFTER the opening
/// quote/bracket): ends at the first unescaped `stop` character
/// (inclusive) or at the end of the line (`$` in Multiline; the newline
/// is excluded). Returns the full match bounds (opening char included).
fn scan_quoted_like(src: &str, open: usize, stop: char) -> (usize, usize) {
    let open_len = src[open..].chars().next().map_or(1, char::len_utf8);
    let bytes = src.as_bytes();
    let mut k = open + open_len;
    while k < src.len() {
        let c = src[k..].chars().next().unwrap();
        if c == '\n' {
            return (open, k);
        }
        // UTF-8 continuation bytes are never 0x5C, so this byte test
        // is an exact "previous character is a backslash" test.
        let prev_is_escape = k > 0 && bytes[k - 1] == b'\\';
        if c == stop && !prev_is_escape {
            return (open, k + c.len_utf8());
        }
        k += c.len_utf8();
    }
    (open, src.len())
}

fn tokenize_query(source: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut i = 0usize;
    while i < source.len() {
        let c = source[i..].chars().next().unwrap();
        let mut matched: Option<(usize, usize)> = None;

        if c == '"' && (i == 0 || !source[..i].ends_with('\\')) {
            // (1) quoted string
            matched = Some(scan_quoted_like(source, i, '"'));
        } else if c == '[' && (i == 0 || !source[..i].ends_with('\\')) {
            // (2) bracketed string
            matched = Some(scan_quoted_like(source, i, ']'));
        } else {
            if lookbehind_bracket_ws(source, i) {
                // (3) `\s*Match` — IgnoreCase applies.
                let mut k = i;
                while k < source.len() {
                    let ch = source[k..].chars().next().unwrap();
                    if ch.is_whitespace() {
                        k += ch.len_utf8();
                    } else {
                        break;
                    }
                }
                if source[k..]
                    .get(..5)
                    .is_some_and(|s| s.eq_ignore_ascii_case("match"))
                {
                    matched = Some((i, k + 5));
                } else if c.is_whitespace() || is_word(c) {
                    // (4) `[\w\s]+` — maximal run of word/whitespace chars.
                    let mut k = i;
                    while k < source.len() {
                        let ch = source[k..].chars().next().unwrap();
                        if ch.is_whitespace() || is_word(ch) {
                            k += ch.len_utf8();
                        } else {
                            break;
                        }
                    }
                    matched = Some((i, k));
                }
            }
            if matched.is_none() && is_word(c) {
                // (5) `[\w]+` — maximal run of word characters.
                let mut k = i;
                while k < source.len() {
                    let ch = source[k..].chars().next().unwrap();
                    if is_word(ch) {
                        k += ch.len_utf8();
                    } else {
                        break;
                    }
                }
                matched = Some((i, k));
            }
            if matched.is_none() && matches!(c, '{' | '}' | ',' | ';') {
                // (6) literals
                matched = Some((i, i + c.len_utf8()));
            }
        }

        match matched {
            Some((s, e)) => {
                let text = source[s..e].trim().to_string();
                tokens.push(Token {
                    text,
                    index: s,
                    length: e - s,
                });
                i = e;
            }
            None => i += c.len_utf8(),
        }
    }
    tokens
}

/// Port of the C# `Tokenizer` (regex + cursor API used by the matcher
/// parser). Token texts are trimmed at construction.
pub struct Tokenizer<'a> {
    #[allow(dead_code)]
    source: &'a str,
    tokens: Vec<Token>,
    position: usize,
}

impl<'a> Tokenizer<'a> {
    pub fn new(source: &'a str) -> Self {
        Tokenizer {
            source,
            tokens: tokenize_query(source),
            position: 0,
        }
    }

    pub fn count(&self) -> usize {
        self.tokens.len()
    }

    pub fn current(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    pub fn text(&self) -> Option<&str> {
        self.current().map(|t| t.text.as_str())
    }

    /// C# `IsOptional`: no match at end of input returns false.
    pub fn is_optional(&self, p: &[&str]) -> bool {
        self.current().is_some_and(|t| t.is(p))
    }

    /// C# `Is`: raises a parse exception at end of input.
    pub fn is(&self, p: &[&str]) -> Result<bool, ParseError> {
        match self.current() {
            None => Err(self.unexpected_end()),
            Some(t) => Ok(t.is(p)),
        }
    }

    pub fn skip(&mut self, count: usize) {
        self.position += count;
    }

    /// C# `Expect`: requires the current token to be one of `expect`.
    pub fn expect(&mut self, expect: &[&str]) -> Result<Token, ParseError> {
        match self.current() {
            Some(t) if t.is(expect) => Ok(self.take()),
            _ => {
                let found = self.current().map_or("<end>", |t| t.text.as_str());
                let what = if expect.len() == 1 {
                    format!("'{}'", expect[0])
                } else {
                    format!("one of ({})", expect.join(", "))
                };
                Err(ParseError(format!("Expected {what}, but found '{found}'")))
            }
        }
    }

    /// C# `Take(startsWith, endsWith)`: takes the current token,
    /// requires the prefix/suffix, and strips them.
    pub fn take_delimited(
        &mut self,
        starts_with: Option<&str>,
        ends_with: Option<&str>,
    ) -> Result<Token, ParseError> {
        let current = self
            .current()
            .cloned()
            .ok_or_else(|| ParseError("Unexpected end reached.".into()))?;
        let mut text = current.text.clone();
        if let Some(p) = starts_with {
            if !text.starts_with(p) {
                return Err(ParseError(format!(
                    "'{}' must start with '{p}'",
                    current.text
                )));
            }
            text = text[p.len()..].to_string();
        }
        if let Some(s) = ends_with {
            if !text.ends_with(s) {
                return Err(ParseError(format!(
                    "'{}' must end with '{s}'",
                    current.text
                )));
            }
            text = text[..text.len() - s.len()].to_string();
        }
        self.position += 1;
        Ok(Token { text, ..current })
    }

    /// C# `TakeString`: takes a `"..."` token and unescapes it.
    pub fn take_string(&mut self) -> Result<Token, ParseError> {
        let mut token = self.take_delimited(Some("\""), Some("\""))?;
        token.text = crate::text::unescape_default(&token.text);
        Ok(token)
    }

    /// C# `Take()`: consumes and returns the current token.
    pub fn take(&mut self) -> Token {
        let token = self.current().cloned().unwrap_or(Token {
            text: String::new(),
            index: 0,
            length: 0,
        });
        self.position += 1;
        token
    }

    fn unexpected_end(&self) -> ParseError {
        ParseError("Unexpected end reached".into())
    }
}

/// `ComicSmartListItem.TokenizeQuery`.
pub fn tokenize(source: &str) -> Tokenizer<'_> {
    Tokenizer::new(source)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(src: &str) -> Vec<String> {
        tokenize_query(src).into_iter().map(|t| t.text).collect()
    }

    #[test]
    fn plain_words_and_literals() {
        assert_eq!(
            texts("Match All { [Series] equals \"x\", [Count] in range \"1\" \"9\" }"),
            [
                "Match", "All", "{", "[Series]", "equals", "\"x\"", ",", "[Count]", "in range",
                "\"1\"", "\"9\"", "}"
            ]
        );
    }

    #[test]
    fn multi_word_operator_is_one_token_after_bracket() {
        // Alternation (4) consumes word+whitespace greedily after `]`.
        assert_eq!(
            texts("[Is Checked] equals Yes"),
            ["[Is Checked]", "equals Yes"]
        );
        assert_eq!(
            texts("[Published] is in the range \"2000\" \"2010\""),
            ["[Published]", "is in the range", "\"2000\"", "\"2010\""]
        );
    }

    #[test]
    fn quoted_string_with_escapes() {
        assert_eq!(texts(r#""a \"b\" c""#), [r#""a \"b\" c""#]);
        assert_eq!(texts("equals \"a, b\" ;"), ["equals", "\"a, b\"", ";"]);
    }

    #[test]
    fn escaped_backslash_before_quote_keeps_string_open() {
        // `\\"` — the quote counts as escaped (faithful regex behavior).
        assert_eq!(texts(r#""a\\" x"#), [r#""a\\" x"#]);
    }

    #[test]
    fn unclosed_quote_ends_at_line_end() {
        assert_eq!(texts("equals \"abc\nmore"), ["equals", "\"abc", "more"]);
    }

    #[test]
    fn match_after_bracket_uses_alternation_3() {
        assert_eq!(
            texts("In [Foo]\nMatch All { }"),
            ["In", "[Foo]", "Match", "All", "{", "}"]
        );
    }

    #[test]
    fn non_match_word_after_bracket_is_one_run() {
        assert_eq!(
            texts("In [Foo]\nNot Match All { }"),
            ["In", "[Foo]", "Not Match All", "{", "}"]
        );
    }

    #[test]
    fn whitespace_between_tokens_is_skipped() {
        assert_eq!(
            texts("  Match\tAll\r\n{\r\n    [X] }"),
            ["Match", "All", "{", "[X]", "}"]
        );
    }

    #[test]
    fn semicolon_is_a_literal() {
        assert_eq!(texts("a; b"), ["a", ";", "b"]);
    }

    #[test]
    fn escaped_bracket_inside_name_is_part_of_token() {
        assert_eq!(texts("[a\\] b]"), ["[a\\] b]"]);
    }

    #[test]
    fn tokenizer_cursor_semantics() {
        let mut t = tokenize("Match Any { [X] }");
        assert!(t.expect(&["MATCH"]).is_ok());
        assert!(t.is_optional(&["ANY"]));
        t.skip(1);
        assert!(t.expect(&["{"]).is_ok());
        let tok = t.take_delimited(Some("["), Some("]")).unwrap();
        assert_eq!(tok.text, "X");
        assert!(t.expect(&["}"]).is_ok());
        assert!(t.current().is_none());
        // `Is` errors at end, `IsOptional` does not.
        assert!(t.is(&["}"]).is_err());
        assert!(!t.is_optional(&["}"]));
        // TakeString at end errors.
        assert!(t.take_string().is_err());
    }

    #[test]
    fn take_string_unescapes() {
        let mut t = tokenize(r#"equals "a \"b\"""#);
        t.skip(1);
        let tok = t.take_string().unwrap();
        assert_eq!(tok.text, "a \"b\"");
    }
}
