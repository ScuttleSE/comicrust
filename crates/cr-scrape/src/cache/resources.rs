//! Inline reference extraction from issue and volume detail JSON
//! (ADR-070). The detail responses embed reference lists — people,
//! characters, teams, locations, concepts, objects, and story arcs —
//! and each reference carries an id, a name, and a role for people.
//! The extraction turns them into resource and credit rows with no
//! extra API request.

use serde_json::Value;

/// The related-resource kinds of ADR-070. `as_str` is also the table
/// name, so the store can build its SQL from a fixed whitelist.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ResourceKind {
    Character,
    Person,
    Team,
    StoryArc,
    Location,
    Concept,
    Object,
    Publisher,
}

impl ResourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ResourceKind::Character => "character",
            ResourceKind::Person => "person",
            ResourceKind::Team => "team",
            ResourceKind::StoryArc => "story_arc",
            ResourceKind::Location => "location",
            ResourceKind::Concept => "concept",
            ResourceKind::Object => "object",
            ResourceKind::Publisher => "publisher",
        }
    }
}

/// Why a credit row exists. The detail responses carry plain credits,
/// first appearances, character deaths, and team disbandings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CreditMarker {
    Credit,
    FirstAppearance,
    DiedIn,
    Disbanded,
}

impl CreditMarker {
    pub fn as_str(self) -> &'static str {
        match self {
            CreditMarker::Credit => "credit",
            CreditMarker::FirstAppearance => "first_appearance",
            CreditMarker::DiedIn => "died_in",
            CreditMarker::Disbanded => "disbanded",
        }
    }
}

/// Who owns a credit row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OwnerKind {
    Issue,
    Volume,
}

impl OwnerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            OwnerKind::Issue => "issue",
            OwnerKind::Volume => "volume",
        }
    }
}

/// One inline reference of a detail response. An id-less reference
/// (the XML heritage shape) carries a name only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceRef {
    pub kind: ResourceKind,
    pub id: Option<i64>,
    pub name: Option<String>,
}

/// One inline credit of an issue or volume. A reference that carries
/// no id becomes a credit with resource id 0; the resource table holds
/// only the identified resources.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreditRef {
    pub kind: ResourceKind,
    pub resource_id: Option<i64>,
    pub name: Option<String>,
    /// People only, else `None`.
    pub role: Option<String>,
    pub marker: CreditMarker,
}

/// The inline references of one detail response.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct References {
    pub resources: Vec<ResourceRef>,
    pub credits: Vec<CreditRef>,
}

/// The inline references of one volume detail response: the six
/// credit lists the volume field list fetches, plus the inline
/// publisher object.
pub fn extract_volume_references(json: &str) -> Option<References> {
    let results = serde_json::from_str::<Value>(json).ok()?;
    let mut out = References::default();
    let mut credits = |field: &'static str, inner: &'static [&'static str], kind: ResourceKind| {
        push_credits(
            &results,
            field,
            inner,
            kind,
            CreditMarker::Credit,
            false,
            &mut out,
        );
    };
    credits("person_credits", &["person"], ResourceKind::Person);
    credits("character_credits", &["character"], ResourceKind::Character);
    credits("team_credits", &["team"], ResourceKind::Team);
    credits("location_credits", &["location"], ResourceKind::Location);
    credits("concept_credits", &["concept"], ResourceKind::Concept);
    credits("object_credits", &["object"], ResourceKind::Object);
    // The inline publisher object is a resource, not a credit.
    if let Some(publisher) = results.get("publisher") {
        out.resources.push(ResourceRef {
            kind: ResourceKind::Publisher,
            id: value_i64(publisher.get("id")),
            name: publisher
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string),
        });
    }
    Some(out)
}

/// The inline references of one issue detail response. The field
/// names are MEASURED on the API documentation page (2026-09-20).
/// The page documents both `teams_disbanded_in` and `disbanded_teams`
/// with the same meaning, so the extractor reads both.
pub fn extract_issue_references(json: &str) -> Option<References> {
    let results = serde_json::from_str::<Value>(json).ok()?;
    let mut out = References::default();
    let mut credits = |field: &'static str,
                       inner: &'static [&'static str],
                       kind: ResourceKind,
                       with_role: bool| {
        push_credits(
            &results,
            field,
            inner,
            kind,
            CreditMarker::Credit,
            with_role,
            &mut out,
        );
    };
    credits("person_credits", &["person"], ResourceKind::Person, true);
    credits(
        "character_credits",
        &["character"],
        ResourceKind::Character,
        false,
    );
    credits("team_credits", &["team"], ResourceKind::Team, false);
    credits(
        "location_credits",
        &["location"],
        ResourceKind::Location,
        false,
    );
    credits(
        "concept_credits",
        &["concept"],
        ResourceKind::Concept,
        false,
    );
    credits("object_credits", &["object"], ResourceKind::Object, false);
    credits(
        "story_arc_credits",
        &["story_arc"],
        ResourceKind::StoryArc,
        false,
    );
    let mut marker = |field: &'static str,
                      inner: &'static [&'static str],
                      kind: ResourceKind,
                      marker: CreditMarker| {
        push_credits(&results, field, inner, kind, marker, false, &mut out);
    };
    marker(
        "characters_died_in",
        &["character"],
        ResourceKind::Character,
        CreditMarker::DiedIn,
    );
    marker(
        "teams_disbanded_in",
        &["team"],
        ResourceKind::Team,
        CreditMarker::Disbanded,
    );
    marker(
        "disbanded_teams",
        &["team"],
        ResourceKind::Team,
        CreditMarker::Disbanded,
    );
    marker(
        "first_appearance_characters",
        &["character"],
        ResourceKind::Character,
        CreditMarker::FirstAppearance,
    );
    marker(
        "first_appearance_concepts",
        &["concept"],
        ResourceKind::Concept,
        CreditMarker::FirstAppearance,
    );
    marker(
        "first_appearance_locations",
        &["location"],
        ResourceKind::Location,
        CreditMarker::FirstAppearance,
    );
    marker(
        "first_appearance_objects",
        &["object"],
        ResourceKind::Object,
        CreditMarker::FirstAppearance,
    );
    marker(
        "first_appearance_storyarcs",
        &["story_arc", "storyarcs"],
        ResourceKind::StoryArc,
        CreditMarker::FirstAppearance,
    );
    marker(
        "first_appearance_teams",
        &["team"],
        ResourceKind::Team,
        CreditMarker::FirstAppearance,
    );
    Some(out)
}

/// Collects one credit field into the output. The `inner` keys name
/// the wrapped forms (`{"character": [...]}`, the shape the proven
/// `parse_issue` reads); a plain array or a single reference object
/// parses too.
fn push_credits(
    results: &Value,
    field: &'static str,
    inner: &[&str],
    kind: ResourceKind,
    marker: CreditMarker,
    with_role: bool,
    out: &mut References,
) {
    for item in ref_list(results.get(field), inner) {
        let id = value_i64(item.get("id"));
        let name = item.get("name").and_then(Value::as_str).map(str::to_string);
        let role = with_role.then(|| {
            item.get("role")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|role| !role.is_empty())
                .map(str::to_string)
        });
        out.credits.push(CreditRef {
            kind,
            resource_id: id,
            name: name.clone(),
            role: role.flatten(),
            marker,
        });
        if let Some(id) = id {
            out.resources.push(ResourceRef {
                kind,
                id: Some(id),
                name,
            });
        }
    }
}

/// The items of one reference field. The wrapped object form holds
/// the list under one of the `inner` keys; a bare array holds the
/// items directly; a lone object with a name or an id is one item.
pub(crate) fn ref_list<'a>(field: Option<&'a Value>, inner: &[&str]) -> Vec<&'a Value> {
    let Some(value) = field else {
        return Vec::new();
    };
    match value {
        Value::Array(items) => items.iter().collect(),
        Value::Object(map) => {
            for key in inner {
                if let Some(wrapped) = map.get(*key) {
                    return as_list(wrapped);
                }
            }
            if map.contains_key("name") || map.contains_key("id") {
                vec![value]
            } else {
                Vec::new()
            }
        }
        _ => Vec::new(),
    }
}

/// The element itself, a one-element list, or an empty list (the C#
/// `__as_list` behavior).
fn as_list(value: &Value) -> Vec<&Value> {
    match value {
        Value::Array(items) => items.iter().collect(),
        single => vec![single],
    }
}

/// The C# dom values are strings; the JSON API returns numbers for
/// ids. Both parse (the same union behavior as `queries.rs`).
fn value_i64(value: Option<&Value>) -> Option<i64> {
    match value {
        Some(Value::Number(number)) => number.as_i64(),
        Some(Value::String(text)) => text.trim().parse().ok(),
        _ => None,
    }
}
