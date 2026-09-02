//! The typed matcher tree the engine evaluates. This is the Rust
//! counterpart of the C# `ComicBookMatcher` class hierarchy: a group
//! (`ComicBookGroupMatcher`) or one of the registered value matchers.

use super::spec::{self, MatcherSpec};
use crate::text;
use cr_core::model::enums::MatcherMode;

/// `ComicBookGroupMatcher`.
#[derive(Clone, Debug)]
pub struct GroupMatcher {
    pub not: bool,
    pub matcher_mode: MatcherMode,
    /// XML-only member (`Collapsed` attribute); not in query strings.
    pub collapsed: bool,
    pub matchers: Vec<Matcher>,
}

impl Default for GroupMatcher {
    fn default() -> Self {
        GroupMatcher {
            not: false,
            matcher_mode: MatcherMode::And,
            collapsed: false,
            matchers: Vec::new(),
        }
    }
}

/// One registered value matcher (`ComicBookValueMatcher` subclass).
#[derive(Clone, Debug)]
pub struct ValueMatcher {
    pub not: bool,
    pub spec: &'static MatcherSpec,
    /// Operator index into [`MatcherSpec::operators`].
    pub op: usize,
    /// `MatchValue` — kept as raw text, like the C# string member the
    /// query renderer writes.
    pub value: String,
    /// `MatchValue2` (range end, custom-key name, ...).
    pub value2: String,
    /// The C# `Name` property: XML-only, never in query strings.
    pub name: String,
    /// `ComicBookStringMatcher.IgnoreCase` (default true, XML-only).
    pub ignore_case: bool,
    /// `<Option>` element of the AllProperties matcher (XML-only).
    pub option: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Matcher {
    Group(GroupMatcher),
    Value(ValueMatcher),
}

impl Matcher {
    /// `ComicBookMatcher.ToString()` / `ComicBookGroupMatcher.ToString()`:
    /// the query-text form of one matcher.
    pub fn to_query(&self) -> String {
        match self {
            Matcher::Group(g) => {
                let mut s = String::new();
                if g.not {
                    s.push_str("Not ");
                }
                s.push_str(&super::query::render_group_query(g, true));
                s
            }
            Matcher::Value(v) => {
                let mut s = String::new();
                if v.not {
                    s.push_str("Not ");
                }
                s.push('[');
                s.push_str(&text::escape_brackets(v.spec.description));
                s.push(']');
                s.push(' ');
                s.push_str(v.spec.operators()[v.op]);
                let argc = v.spec.argument_count(v.op);
                if argc > 0 {
                    s.push(' ');
                    s.push('"');
                    s.push_str(&text::escape_default(&v.value));
                    s.push('"');
                }
                if argc > 1 {
                    s.push(' ');
                    s.push('"');
                    s.push_str(&text::escape_default(&v.value2));
                    s.push('"');
                }
                s
            }
        }
    }

    /// `ComicBookMatcher.IsSame`: structural equality of the members the
    /// matcher semantics depend on. `name`, `ignore_case`, and `collapsed`
    /// are compared too (the C# group matcher compares `Collapsed`).
    pub fn is_same(&self, other: &Matcher) -> bool {
        match (self, other) {
            (Matcher::Group(a), Matcher::Group(b)) => {
                a.not == b.not
                    && a.matcher_mode == b.matcher_mode
                    && a.collapsed == b.collapsed
                    && a.matchers.len() == b.matchers.len()
                    && a.matchers
                        .iter()
                        .zip(&b.matchers)
                        .all(|(x, y)| x.is_same(y))
            }
            (Matcher::Value(a), Matcher::Value(b)) => {
                a.not == b.not
                    && std::ptr::eq(a.spec, b.spec)
                    && a.op == b.op
                    && a.value == b.value
                    && a.value2 == b.value2
                    && a.name == b.name
                    && a.ignore_case == b.ignore_case
            }
            _ => false,
        }
    }
}

impl PartialEq for Matcher {
    fn eq(&self, other: &Self) -> bool {
        self.is_same(other)
    }
}

// ---------- binding to the ComicDb.xml raw tree ----------

use cr_core::database::list_items as raw;

impl Matcher {
    /// Binds a raw matcher from the XML tree (`xsi:type` + strings) to
    /// the typed tree. Unknown class names return `None` — the C#
    /// `XmlSerializer` would fail the whole load on them, so data with
    /// unknown matchers never reaches evaluation in C# either.
    pub fn from_raw(r: &raw::ComicBookMatcher) -> Option<Matcher> {
        match r {
            raw::ComicBookMatcher::Value(v) => {
                let spec = spec::by_class_name(&v.type_name)?;
                Some(Matcher::Value(ValueMatcher {
                    not: v.not,
                    spec,
                    op: usize::try_from(v.match_operator).unwrap_or(0),
                    value: v.match_value.clone(),
                    value2: v.match_value_2.clone(),
                    name: v.name.clone(),
                    ignore_case: v.ignore_case,
                    option: v.option.clone(),
                }))
            }
            raw::ComicBookMatcher::Group(g) => {
                let matchers = g
                    .matchers
                    .iter()
                    .map(Matcher::from_raw)
                    .collect::<Option<Vec<_>>>()?;
                Some(Matcher::Group(GroupMatcher {
                    not: g.not,
                    matcher_mode: g.matcher_mode,
                    collapsed: g.collapsed,
                    matchers,
                }))
            }
        }
    }

    /// Back to the raw XML-tree form (used when writing a modified
    /// library back).
    pub fn to_raw(&self) -> raw::ComicBookMatcher {
        match self {
            Matcher::Group(g) => raw::ComicBookMatcher::Group(raw::GroupMatcher {
                not: g.not,
                matcher_mode: g.matcher_mode,
                collapsed: g.collapsed,
                matchers: g.matchers.iter().map(Matcher::to_raw).collect(),
            }),
            Matcher::Value(v) => raw::ComicBookMatcher::Value(raw::ValueMatcher {
                type_name: v.spec.class_name.to_string(),
                not: v.not,
                name: v.name.clone(),
                match_value: v.value.clone(),
                match_value_2: v.value2.clone(),
                match_operator: v.op as i32,
                ignore_case: v.ignore_case,
                option: v.option.clone(),
            }),
        }
    }
}
