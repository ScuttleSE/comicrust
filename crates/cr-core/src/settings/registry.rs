//! The typed field registry — the reflection replacement.
//!
//! The C# reflects over properties (`IniFile.UpdateProperties`,
//! `FormUtility.FillPanelWithOptions`) with `[Category]`,
//! `[Description]`, `[Browsable]` metadata. The port declares every
//! settings field in a [`FieldDesc`] table instead: name (the C#
//! property name — the ini key), kind, category, description,
//! browsability, and get/set function pointers. The ini loader, the
//! command-line parser and the GTK options builder all walk the same
//! tables.

/// A field value in the registry currency.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    Int(i32),
    Float(f32),
    Str(String),
}

/// The widget/parse shape of a field.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldKind {
    Bool,
    Int,
    Float,
    /// Non-nullable C# string (default `""`).
    Str,
    /// Nullable C# string (default `null` → the element is omitted
    /// from Config.xml).
    StrOpt,
    /// Enum stored as a typed value; registry traffic carries the C#
    /// member name (`to_xml`/`from_xml`).
    Enum,
    /// A flags enum: same traffic as [`FieldKind::Enum`], rendered as
    /// a set of check boxes by the UI.
    Flags,
}

/// One settings field descriptor. `category`/`description` mirror the
/// C# attributes (empty = none; the options builder maps no category
/// to the "Other" group and hides description-less fields, exactly
/// like `FillPanelWithOptions`).
pub struct FieldDesc<T> {
    pub name: &'static str,
    pub kind: FieldKind,
    pub category: &'static str,
    pub description: &'static str,
    pub browsable: bool,
    /// The C# `[IniFile(false)]` fields exist on the command line
    /// only; the ini must not set them.
    pub ini_enabled: bool,
    pub get: fn(&T) -> Value,
    pub set: fn(&mut T, Value),
}

impl<T> FieldDesc<T> {
    /// The `FillPanelWithOptions` filter: browsable booleans with a
    /// non-empty description (the auto-filled check-box groups).
    pub fn is_options_checkbox(&self) -> bool {
        self.kind == FieldKind::Bool && self.browsable && !self.description.is_empty()
    }
}

/// Parse an ini text into a [`Value`] by kind. .NET semantics:
/// booleans are `true`/`false` (case-insensitive), numbers are
/// invariant, strings pass through. Enum texts stay strings — the
/// typed `set` closures parse the member names (case-insensitive
/// like `Enum.Parse(type, value, true)`).
pub fn parse_value(kind: FieldKind, text: &str) -> Option<Value> {
    let text = text.trim();
    Some(match kind {
        FieldKind::Bool => Value::Bool(match text.to_ascii_lowercase().as_str() {
            "true" => true,
            "false" => false,
            _ => return None,
        }),
        FieldKind::Int => Value::Int(text.parse().ok()?),
        FieldKind::Float => Value::Float(text.parse().ok()?),
        FieldKind::Str | FieldKind::StrOpt | FieldKind::Enum | FieldKind::Flags => {
            Value::Str(text.to_string())
        }
    })
}

/// Ini application for a table: case-insensitive name match,
/// per-kind parse, unknown keys and failures ignored (the C# swallows
/// the errors in `UpdateProperties`).
pub fn apply_ini<T>(values: &super::ini::IniValues, target: &mut T, fields: &[FieldDesc<T>]) {
    for (key, text) in values.iter() {
        if let Some(field) = fields
            .iter()
            .find(|f| f.name.eq_ignore_ascii_case(key) && f.ini_enabled)
        {
            if let Some(value) = parse_value(field.kind, text) {
                (field.set)(target, value);
            }
        }
    }
}

/// The enum currency for the registry: every `xml_enum`/`xml_flags`
/// type implements this (the macros in `model::enums` generate the
/// impls), so the field tables need no type forwarding.
pub trait EnumValue: Copy {
    /// The numeric form (`.NET` int parse).
    fn from_int(v: i32) -> Option<Self>
    where
        Self: Sized;
    /// The member-name form, case-insensitive like
    /// `Enum.Parse(type, value, true)`.
    fn from_name(s: &str) -> Option<Self>
    where
        Self: Sized;
    /// The serialized member name (`to_xml`).
    fn as_name(&self) -> String;
}

/// The declaration-side macro: builds the `FieldDesc` table for a
/// settings struct, one arm per field:
///
/// ```text
/// Bool "ShowSplash" => show_splash: bool, cat: "", desc: "...", browsable: true, ini: true;
/// ```
///
/// The struct's `Default` impl stays the single source of defaults;
/// the tables carry only accessors and metadata.
#[macro_export]
macro_rules! settings_fields {
    (
        $table:ident, $ty:ty,
        $( $kind:ident $name:literal => $field:ident : $ety:ty,
            cat: $cat:literal, desc: $desc:literal, browsable: $b:literal,
            ini: $ini:literal
        );* $(;)?
    ) => {
        pub const $table: &'static [$crate::settings::registry::FieldDesc<$ty>] = &[
            $( $crate::settings::registry::FieldDesc {
                name: $name,
                kind: $crate::settings::registry::FieldKind::$kind,
                category: $cat,
                description: $desc,
                browsable: $b,
                ini_enabled: $ini,
                get: (|s: &$ty| $crate::settings::registry::Value::new(
                    $crate::settings_fields!(@get $kind, s.$field)
                )) as fn(&$ty) -> $crate::settings::registry::Value,
                set: (|s: &mut $ty, v: $crate::settings::registry::Value| {
                    match v {
                        $crate::settings::registry::Value::Bool(x) => {
                            $crate::settings_fields!(@set_bool $kind, s.$field, x);
                        }
                        $crate::settings::registry::Value::Int(x) => {
                            $crate::settings_fields!(@set_int $kind, s.$field, x);
                        }
                        $crate::settings::registry::Value::Float(x) => {
                            $crate::settings_fields!(@set_float $kind, s.$field, x);
                        }
                        $crate::settings::registry::Value::Str(x) => {
                            $crate::settings_fields!(@set_str $kind, s.$field, x);
                        }
                    }
                }) as fn(&mut $ty, $crate::settings::registry::Value),
            } ),*
        ];
    };
    // getters
    (@get Bool, $v:expr) => { $v };
    (@get Int, $v:expr) => { $v };
    (@get Float, $v:expr) => { $v };
    (@get Str, $v:expr) => { $v.clone() };
    (@get StrOpt, $v:expr) => { $v.clone().unwrap_or_default() };
    (@get Enum, $v:expr) => { $v.as_name() };
    (@get Flags, $v:expr) => { $v.as_name() };
    // setters: each Value variant arms only its own kinds; enum
    // conversions ride [`EnumValue`] (inference binds the field type).
    // Bodies are statements (the outer expansion wraps in a block).
    (@set_bool Bool, $f:expr, $x:ident) => { $f = $x; };
    (@set_bool Enum, $f:expr, $x:ident) => { let _ = $x; };
    (@set_bool Flags, $f:expr, $x:ident) => { let _ = $x; };
    (@set_bool Str, $f:expr, $x:ident) => { let _ = $x; };
    (@set_bool StrOpt, $f:expr, $x:ident) => { let _ = $x; };
    (@set_bool Int, $f:expr, $x:ident) => { let _ = $x; };
    (@set_bool Float, $f:expr, $x:ident) => { let _ = $x; };
    (@set_int Int, $f:expr, $x:ident) => { $f = $x; };
    (@set_int Enum, $f:expr, $x:ident) => {
        if let Some(e) = $crate::settings::registry::EnumValue::from_int($x) { $f = e; }
    };
    (@set_int Flags, $f:expr, $x:ident) => {
        if let Some(e) = $crate::settings::registry::EnumValue::from_int($x) { $f = e; }
    };
    (@set_int Bool, $f:expr, $x:ident) => { let _ = $x; };
    (@set_int Float, $f:expr, $x:ident) => { let _ = $x; };
    (@set_int Str, $f:expr, $x:ident) => { let _ = $x; };
    (@set_int StrOpt, $f:expr, $x:ident) => { let _ = $x; };
    (@set_float Float, $f:expr, $x:ident) => { $f = $x; };
    (@set_float Bool, $f:expr, $x:ident) => { let _ = $x; };
    (@set_float Int, $f:expr, $x:ident) => { let _ = $x; };
    (@set_float Enum, $f:expr, $x:ident) => { let _ = $x; };
    (@set_float Flags, $f:expr, $x:ident) => { let _ = $x; };
    (@set_float Str, $f:expr, $x:ident) => { let _ = $x; };
    (@set_float StrOpt, $f:expr, $x:ident) => { let _ = $x; };
    (@set_str Str, $f:expr, $x:ident) => { $f = $x; };
    (@set_str StrOpt, $f:expr, $x:ident) => {
        // The C# consumers check `IsNullOrEmpty` — an empty ini value
        // means unset (the seeded `""` for a `null`-default key must
        // not shadow the default with `Some("")`).
        $f = if $x.is_empty() { None } else { Some($x) };
    };
    (@set_str Enum, $f:expr, $x:ident) => {
        if let Some(e) = $crate::settings::registry::EnumValue::from_name(&$x) { $f = e; }
    };
    (@set_str Flags, $f:expr, $x:ident) => {
        if let Some(e) = $crate::settings::registry::EnumValue::from_name(&$x) { $f = e; }
    };
    (@set_str Bool, $f:expr, $x:ident) => { let _ = $x; };
    (@set_str Int, $f:expr, $x:ident) => { let _ = $x; };
    (@set_str Float, $f:expr, $x:ident) => { let _ = $x; };
}

impl Value {
    /// The kind-generic constructor the macro uses.
    pub fn new(v: impl Into<Value>) -> Value {
        v.into()
    }
}

impl From<bool> for Value {
    fn from(v: bool) -> Value {
        Value::Bool(v)
    }
}
impl From<i32> for Value {
    fn from(v: i32) -> Value {
        Value::Int(v)
    }
}
impl From<f32> for Value {
    fn from(v: f32) -> Value {
        Value::Float(v)
    }
}
impl From<String> for Value {
    fn from(v: String) -> Value {
        Value::Str(v)
    }
}
impl From<&str> for Value {
    fn from(v: &str) -> Value {
        Value::Str(v.to_string())
    }
}
