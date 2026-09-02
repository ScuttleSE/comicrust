//! The smart-list query language: parse a `Match` string into a matcher
//! tree and render a matcher tree back to a `Match` string.
//!
//! Ports of:
//!
//! - `ComicBookGroupMatcher.ConvertQueryToParamerters` (parse, the C#
//!   typo included in the name only),
//! - `ComicBookGroupMatcher.CreateMatcherFromQuery`,
//! - `ComicBookGroupMatcher.ConvertParametersToQuery` (render),
//! - `ComicSmartListItem`'s query constructor and `ToString` (the
//!   `Name "..."` / `In [...]` prelude around the group query).
//!
//! Grammar (derived; see also the module doc of `crate::tokenizer`):
//!
//! ```text
//! smart-list  := [NAME string] [ [NOT] IN "[" list-name "]" ] group
//! group       := MATCH [ANY|ALL] [ "{" matcher ("," matcher)* "}" | matcher ]
//! matcher     := [NOT] (group | "[" matcher-name "]" operator string*)
//! operator    := one of the neutral operator words of the matcher
//! string      := a double-quoted token (TakeString)
//! ```
//!
//! `ALL` = and-combine, `ANY` = or-combine. A single-matcher group has
//! no `ALL`/`ANY` and no braces. The renderer emits `ALL`/`ANY` and
//! braces only when the group has more than one matcher; a group with
//! zero matchers renders as bare `MATCH` (which does not re-parse —
//! faithful to the C#).
//!
//! Note: `MatcherMode` belongs to the group or list; when a smart list
//! renders, its own mode feeds the group header.

use crate::text;
use crate::tokenizer::{ParseError, Tokenizer};
use cr_core::model::enums::MatcherMode;

use super::spec;
use super::tree::{GroupMatcher, Matcher, ValueMatcher};

/// `ComicBookGroupMatcher.ConvertQueryToParamerters`.
pub fn parse_group_query(t: &mut Tokenizer<'_>) -> Result<GroupMatcher, ParseError> {
    t.expect(&["MATCH"])?;
    let mut g = GroupMatcher::default();
    if t.is_optional(&["ANY"]) {
        g.matcher_mode = MatcherMode::Or;
        t.skip(1);
    } else if t.is_optional(&["ALL"]) {
        t.skip(1);
    }
    let braced = t.is_optional(&["{"]);
    if braced {
        t.skip(1);
    }
    loop {
        if braced && t.is(&["}"])? {
            t.skip(1);
            break;
        }
        g.matchers.push(create_matcher_from_query(t)?);
        if braced {
            if t.is(&["}"])? {
                t.skip(1);
                break;
            }
            t.expect(&[","])?;
            continue;
        }
        break;
    }
    Ok(g)
}

/// `ComicBookGroupMatcher.CreateMatcherFromQuery`.
fn create_matcher_from_query(t: &mut Tokenizer<'_>) -> Result<Matcher, ParseError> {
    let mut not = false;
    if t.is_optional(&["NOT"]) {
        not = true;
        t.skip(1);
    }
    if t.is(&["MATCH"])? {
        let mut g = parse_group_query(t)?;
        g.not = not;
        return Ok(Matcher::Group(g));
    }
    let token = t.take_delimited(Some("["), Some("]"))?;
    let name = text::unescape_brackets(&token.text);
    let spec = spec::by_description(&name)
        .ok_or_else(|| ParseError(format!("Invalid name {name} encountered")))?;
    let op_token = t.expect(spec.operators())?;
    let op = spec
        .operators()
        .iter()
        .position(|o| o.eq_ignore_ascii_case(&op_token.text))
        .ok_or_else(|| ParseError(format!("Invalid operator {} encountered", op_token.text)))?;
    let mut vm = ValueMatcher {
        not,
        spec,
        op,
        value: String::new(),
        value2: String::new(),
        name: String::new(),
        ignore_case: true,
    };
    for i in 0..spec.argument_count(op) {
        let arg = t.take_string()?;
        if i == 0 {
            vm.value = arg.text;
        } else {
            vm.value2 = arg.text;
        }
    }
    Ok(Matcher::Value(vm))
}

/// `ComicBookGroupMatcher.ConvertParametersToQuery`. `format = true`
/// gives the multi-line form (`Intent(4)` indentation, braces); the
/// single-matcher form never uses braces.
pub fn render_group_query(g: &GroupMatcher, format: bool) -> String {
    let mut sb = String::new();
    let count = g.matchers.len();
    sb.push_str("Match");
    if count > 0 {
        sb.push(' ');
        if count > 1 {
            match g.matcher_mode {
                MatcherMode::And => sb.push_str("All"),
                MatcherMode::Or => sb.push_str("Any"),
            }
            sb.push_str(if format { text::NL } else { " " });
            sb.push('{');
            if format {
                sb.push_str(text::NL);
            }
        }
        for (i, m) in g.matchers.iter().enumerate() {
            let mut matcher_text = m.to_query();
            if format && count > 1 {
                matcher_text = text::intent(&matcher_text, 4);
            }
            sb.push_str(&matcher_text);
            if i != count - 1 && count > 1 {
                sb.push(',');
                sb.push_str(if format { text::NL } else { " " });
            }
        }
        if count > 1 {
            if format {
                sb.push_str(text::NL);
            }
            sb.push('}');
        }
    }
    sb
}

/// A parsed smart-list query: the `Name`/`In` prelude plus the matcher
/// group (`ComicSmartListItem` query constructor output).
#[derive(Clone, Debug, Default)]
pub struct SmartListQuery {
    pub name: Option<String>,
    /// Resolved base list name (`In [...]`); the C# resolves the name to
    /// a list Id against the library at parse time.
    pub base_list: Option<String>,
    pub not_in_base_list: bool,
    pub group: GroupMatcher,
}

impl PartialEq for SmartListQuery {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
            && self.base_list == other.base_list
            && self.not_in_base_list == other.not_in_base_list
            && self.group.not == other.group.not
            && self.group.matcher_mode == other.group.matcher_mode
            && self.group.matchers == other.group.matchers
    }
}

/// `new ComicSmartListItem(name, query, library)` parse path (without
/// the library lookup).
pub fn parse_smart_list_query(query: &str) -> Result<SmartListQuery, ParseError> {
    let mut t = Tokenizer::new(query);
    let mut q = SmartListQuery::default();
    if t.is_optional(&["NAME"]) {
        t.skip(1);
        q.name = Some(t.take_string()?.text);
    }
    if t.is_optional(&["IN", "NOT"]) {
        if t.is_optional(&["NOT"]) {
            t.skip(1);
            q.not_in_base_list = true;
        }
        t.skip(1);
        let list = t.take_delimited(Some("["), Some("]"))?;
        q.base_list = Some(text::unescape_brackets(&list.text));
    }
    q.group = parse_group_query(&mut t)?;
    Ok(q)
}

/// `ComicSmartListItem.ToString()`: the prelude uses literal `\n`
/// newlines (the C# appends `"\n"`, not `Environment.NewLine`); the
/// group part uses the formatted [`render_group_query`].
pub fn render_smart_list_query(q: &SmartListQuery) -> String {
    let mut sb = String::new();
    if let Some(name) = &q.name {
        sb.push_str("Name \"");
        sb.push_str(&text::escape_default(name));
        sb.push_str("\"\n");
    }
    if let Some(base) = &q.base_list {
        if q.not_in_base_list {
            sb.push_str("Not ");
        }
        sb.push_str("In [");
        sb.push_str(&text::escape_brackets(base));
        sb.push_str("]\n");
    }
    sb.push_str(&render_group_query(&q.group, true));
    sb
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matcher::spec;

    fn round_trip(query: &str) {
        let q1 = parse_smart_list_query(query).unwrap_or_else(|e| panic!("parse {query:?}: {e}"));
        let rendered = render_smart_list_query(&q1);
        assert_eq!(rendered, query, "render(parse(q)) != q");
        let q2 = parse_smart_list_query(&rendered).unwrap();
        assert!(
            q1.group
                .matchers
                .iter()
                .zip(&q2.group.matchers)
                .all(|(a, b)| a.is_same(b))
                && q1.group.matchers.len() == q2.group.matchers.len()
        );
    }

    #[test]
    fn single_matcher_group() {
        round_trip("Match [Series] equals \"Batman\"");
        round_trip("Match Not [Series] equals \"Batman\"");
        round_trip("Match [My Rating] is greater \"3\"");
        round_trip("Match [Read Percentage] in range \"10\" \"95\"");
        round_trip("Match [Added] is in last days \"14\"");
    }

    #[test]
    fn yesno_and_manga_operators() {
        round_trip("Match [Is Checked] equals yes");
        round_trip("Match [Black and White] equals no");
        round_trip("Match [Manga] equals ltr");
        round_trip("Match [Modified Info] equals unknown");
    }

    #[test]
    fn date_operators() {
        round_trip("Match [Published] is after \"2000-01-01\"");
        round_trip("Match [Published] is in range \"2000\" \"2010\"");
    }

    #[test]
    fn multi_matcher_group() {
        round_trip("Match All\r\n{\r\n    [Series] equals \"Batman\",\r\n    [My Rating] is greater \"3\"\r\n}");
        round_trip(
            "Match Any\r\n{\r\n    [Series] contains \"x\",\r\n    [Title] contains \"y\"\r\n}",
        );
    }

    #[test]
    fn nested_groups() {
        round_trip(
            "Match All\r\n{\r\n    [Series] equals \"Batman\",\r\n    Not Match Any\r\n    {\r\n        [My Rating] is greater \"3\",\r\n        [Is Checked] equals yes\r\n    }\r\n}",
        );
    }

    #[test]
    fn name_and_in_prelude() {
        round_trip("Name \"My search\"\nIn [Base]\nMatch [Series] equals \"x\"");
        round_trip("Name \"Other\"\nNot In [Ba\\[se\\]]\nMatch Any\r\n{\r\n    [Series] equals \"x\",\r\n    [Title] equals \"y\"\r\n}");
    }

    #[test]
    fn escaped_values_round_trip() {
        // Quotes and backslashes in values.
        round_trip("Match [Notes] contains \"a \\\"quoted\\\" \\\\ word\"");
    }

    #[test]
    fn custom_values_matcher_two_args() {
        round_trip("Match [Custom Value] equals \"MyKey\" \"MyValue\"");
    }

    #[test]
    fn unknown_matcher_name_errors() {
        let q = parse_smart_list_query("Match [Nope] equals \"x\"");
        assert_eq!(q.unwrap_err().0, "Invalid name Nope encountered");
    }

    #[test]
    fn invalid_operator_errors() {
        let q = parse_smart_list_query("Match [Series] isnot \"x\"");
        assert!(q
            .unwrap_err()
            .0
            .starts_with("Expected one of (equals, contains"));
    }

    #[test]
    fn wrong_arg_count_errors() {
        // YesNo matchers take no argument; a following quoted token is
        // never consumed, like in C# (no error).
        assert!(parse_smart_list_query("Match [Is Checked] equals yes \"x\"").is_ok());
        // Missing argument errors:
        assert!(parse_smart_list_query("Match [Series] equals").is_err());
    }

    #[test]
    fn operator_index_lookup() {
        let mut t = Tokenizer::new("Match [My Rating] IS GREATER \"3\"");
        let g = parse_group_query(&mut t).unwrap();
        let Matcher::Value(v) = &g.matchers[0] else {
            panic!("value expected");
        };
        assert_eq!(v.spec.class_name, "ComicBookRatingMatcher");
        assert_eq!(v.op, spec::ops::NUM_GREATER);
        assert_eq!(v.value, "3");
    }
}
