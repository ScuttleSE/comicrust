//! The Library Organizer profile (the port of `losettings.py`).
//!
//! A profile carries the templates, the per-field tables, the exclude
//! rules, and the move/copy options. Field names in the per-field
//! tables keep the addon's spellings so imported XML profiles round
//! trip: `EmptyData`/`FailedFields` use the C# field names
//! (`ShadowSeries`), `Prefix`/`Postfix`/`Separator`/`TextBox` use the
//! display names (`Series Complete`).
//!
//! The XML import/export mirrors the addon's `save_to_xml`/
//! `load_from_xml` (root `Profiles`, single exported `Profile`, legacy
//! `Settings`/`Setting` roots, the 1.6→2.0 key renames). Element order
//! is interop-relevant only for the addon's own reader, which is
//! XPath-based; byte identity with the addon's files is not claimed.

use std::collections::BTreeMap;

use cr_core::xml::{Emitter, Tok, XmlReader};

pub const VERSION: f64 = 2.1;

pub const MODE_MOVE: &str = "Move";
pub const MODE_COPY: &str = "Copy";
pub const MODE_SIMULATE: &str = "Simulate";

pub const EXCLUDE_ANY: &str = "Any";
pub const EXCLUDE_ALL: &str = "All";
pub const MODE_DO_NOT: &str = "Do not";
pub const MODE_ONLY: &str = "Only";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProfileError {
    #[error("xml error: {0}")]
    Xml(String),
    #[error("no valid profile root element")]
    NoRoot,
}

/// One exclude rule: a display field name, an operator, and the value
/// to compare against (`ExcludeRule` in locommon.py).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ExcludeRule {
    pub field: String,
    pub operator: String,
    pub value: String,
}

/// A rule or a nested rule group (`ExcludeGroup` in locommon.py).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum RuleNode {
    Rule(ExcludeRule),
    Group {
        operator: String,
        rules: Vec<RuleNode>,
    },
}

/// The profile (the `Profile` class in losettings.py).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Profile {
    /// Profile-schema version, like the addon's `Version` attribute.
    /// TOML-only storage keeps it out of the file (the XML carries it).
    #[serde(skip)]
    pub version: f64,
    pub name: String,
    pub folder_template: String,
    pub file_template: String,
    pub base_folder: String,
    /// Text used for blank folder components.
    pub empty_folder: String,
    /// C# field name → replacement when the value is empty.
    pub empty_data: BTreeMap<String, String>,
    pub prefix: BTreeMap<String, String>,
    pub postfix: BTreeMap<String, String>,
    /// Multi-value separator per display field (addon spelling kept in
    /// the XML: `Seperator`).
    pub separator: BTreeMap<String, String>,
    /// Config-UI per-field insert text (persisted like the addon).
    pub text_box: BTreeMap<String, String>,
    /// Illegal path characters → replacement, longest keys first.
    pub illegal_characters: BTreeMap<String, String>,
    /// Month/season number (1–12, 13 Spring … 16 Winter) → name.
    /// String keys: TOML (the config store) has string keys.
    pub months: BTreeMap<String, String>,
    pub use_folder: bool,
    pub use_file_name: bool,
    /// Substrings; a book whose path contains one is skipped.
    pub exclude_folders: Vec<String>,
    pub dont_ask_when_multi_one: bool,
    pub exclude_rules: Vec<RuleNode>,
    /// `Any` or `All`.
    pub exclude_operator: String,
    /// `Do not` (rules exclude) or `Only` (rules include).
    pub exclude_mode: String,
    pub remove_empty_folder: bool,
    /// Paths never pruned by the empty-folder cleanup.
    pub excluded_empty_folder: Vec<String>,
    pub move_fileless: bool,
    /// `.jpg`, `.png` or `.bmp`.
    pub fileless_format: String,
    pub fail_empty_values: bool,
    /// Move anyway (to `failed_folder`) when a watched field is empty.
    pub move_failed: bool,
    pub failed_folder: String,
    /// C# field names watched by `fail_empty_values`.
    pub failed_fields: Vec<String>,
    /// `Move`, `Copy` or `Simulate`.
    pub mode: String,
    /// Copy mode also adds the copy to the library (`CopyMode.AddToLibrary`).
    pub copy_mode: bool,
    pub auto_space_fields: bool,
    pub replace_multiple_spaces: bool,
    /// On overwrite, carry the existing book's read percentage.
    pub copy_read_percentage: bool,
}

impl Default for Profile {
    fn default() -> Self {
        // The addon's Profile.__init__.
        let illegal: [(&str, &str); 9] = [
            ("?", ""),
            ("/", ""),
            ("\\", ""),
            ("*", ""),
            (":", " - "),
            ("<", "["),
            (">", "]"),
            ("|", "!"),
            ("\"", "'"),
        ];
        let month_names = [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
            "Spring",
            "Summer",
            "Fall",
            "Winter",
        ];
        Profile {
            version: VERSION,
            name: String::new(),
            folder_template: String::new(),
            file_template: String::new(),
            base_folder: String::new(),
            empty_folder: String::new(),
            empty_data: BTreeMap::new(),
            prefix: BTreeMap::new(),
            postfix: BTreeMap::new(),
            separator: BTreeMap::new(),
            text_box: BTreeMap::new(),
            illegal_characters: illegal
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            months: month_names
                .iter()
                .enumerate()
                .map(|(i, n)| ((i as i32 + 1).to_string(), n.to_string()))
                .collect(),
            use_folder: true,
            use_file_name: true,
            exclude_folders: Vec::new(),
            dont_ask_when_multi_one: true,
            exclude_rules: Vec::new(),
            exclude_operator: EXCLUDE_ANY.to_string(),
            exclude_mode: MODE_DO_NOT.to_string(),
            remove_empty_folder: true,
            excluded_empty_folder: Vec::new(),
            move_fileless: false,
            fileless_format: ".jpg".to_string(),
            fail_empty_values: false,
            move_failed: false,
            failed_folder: String::new(),
            failed_fields: Vec::new(),
            mode: MODE_MOVE.to_string(),
            copy_mode: true,
            auto_space_fields: true,
            replace_multiple_spaces: true,
            copy_read_percentage: true,
        }
    }
}

impl Profile {
    /// The built-in fallback profile (`load_profiles` in losettings.py
    /// when no profile exists).
    pub fn builtin_default() -> Self {
        Profile {
            name: "Default".to_string(),
            file_template:
                "{<series>}{ Vol.<volume>}{ #<number2>}{ (of <count2>)}{ ({<month>, }<year>)}"
                    .to_string(),
            folder_template: "{<publisher>}\\{<imprint>}\\{<series>}{ (<startyear>{ <format>})}"
                .to_string(),
            ..Profile::default()
        }
    }

    /// The 1.6→2.0 migration (`Profile.update` in losettings.py). Runs
    /// on load when the file's Version is below 2.0 (or missing).
    pub fn update_legacy(&mut self) {
        let legacy = self.version < 2.0;
        if legacy {
            if self.mode == "Test" {
                self.mode = MODE_SIMULATE.to_string();
            }
            let empty_renames: [(&str, &str); 8] = [
                ("Language", "LanguageISO"),
                ("Format", "ShadowFormat"),
                ("Count", "ShadowCount"),
                ("Number", "ShadowNumber"),
                ("Series", "ShadowSeries"),
                ("Title", "ShadowTitle"),
                ("Volume", "ShadowVolume"),
                ("Year", "ShadowYear"),
            ];
            for (from, to) in empty_renames {
                if let Some(v) = self.empty_data.remove(from) {
                    self.empty_data.insert(to.to_string(), v);
                }
            }
            let insert_renames: [(&str, &str); 12] = [
                ("SeriesComplete", "Series Complete"),
                ("Read", "Read Percentage"),
                ("FirstLetter", "First Letter"),
                ("AgeRating", "Age Rating"),
                ("AlternateSeriesMulti", "Alternate Series Multi"),
                ("MonthNumber", "Month Number"),
                ("AlternateNumber", "Alternate Number"),
                ("StartMonth", "Start Month"),
                ("AlternateSeries", "Alternate Series"),
                ("ScanInformation", "Scan Information"),
                ("StartYear", "Start Year"),
                ("AlternateCount", "Alternate Count"),
            ];
            for (from, to) in insert_renames {
                for table in [
                    &mut self.text_box,
                    &mut self.prefix,
                    &mut self.postfix,
                    &mut self.separator,
                ] {
                    if let Some(v) = table.remove(from) {
                        table.insert(to.to_string(), v);
                    }
                }
            }
        }
        self.version = VERSION;
    }

    pub fn is_simulate(&self) -> bool {
        self.mode == MODE_SIMULATE
    }

    pub fn is_copy(&self) -> bool {
        self.mode == MODE_COPY
    }
}

/// The `[plugins.library-organizer]` table (ADR-033).
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PluginSettings {
    /// Profile names selected in the last run.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub last_used: Vec<String>,
    #[serde(default, rename = "profile")]
    pub profiles: Vec<Profile>,
}

impl PluginSettings {
    /// The fallback state (`load_profiles` when nothing exists).
    pub fn builtin() -> Self {
        PluginSettings {
            last_used: vec!["Default".to_string()],
            profiles: vec![Profile::builtin_default()],
        }
    }
}

// ---------------------------------------------------------------------------
// XML import / export
// ---------------------------------------------------------------------------

/// Serializes profiles in the addon's store shape:
/// `<Profiles LastUsed="A,B">…</Profiles>`.
pub fn export_profiles_xml(profiles: &[&Profile], last_used: &[&str]) -> String {
    let mut e = Emitter::new(Vec::new()).expect("xml emitter");
    let _ = write_profiles_element(&mut e, profiles, Some(last_used));
    String::from_utf8(e.finish().expect("xml finish")).expect("utf8")
}

/// Serializes one profile as a standalone document (the addon's
/// "export profile" file: root `Profile`).
pub fn export_single_profile_xml(p: &Profile) -> String {
    let mut e = Emitter::new(Vec::new()).expect("xml emitter");
    let _ = write_profile_element(&mut e, p);
    String::from_utf8(e.finish().expect("xml finish")).expect("utf8")
}

fn write_profiles_element(
    e: &mut Emitter<Vec<u8>>,
    profiles: &[&Profile],
    last_used: Option<&[&str]>,
) -> std::io::Result<()> {
    e.start("Profiles")?;
    if let Some(lu) = last_used {
        if !lu.is_empty() {
            e.attr("LastUsed", &lu.join(","))?;
        }
    }
    for p in profiles {
        write_profile_element(e, p)?;
    }
    e.end()
}

fn write_profile_element(e: &mut Emitter<Vec<u8>>, p: &Profile) -> std::io::Result<()> {
    e.start("Profile")?;
    e.attr("Name", &p.name)?;
    // Every in-store profile is 2.x-era; the addon writes its own
    // VERSION after update(). The `version` field only matters at
    // import time (legacy detection).
    e.attr("Version", &format!("{}", VERSION))?;
    // Strings, in the addon's __dict__ order.
    e.text_elem("FolderTemplate", &p.folder_template)?;
    e.text_elem("BaseFolder", &p.base_folder)?;
    e.text_elem("FileTemplate", &p.file_template)?;
    e.text_elem("EmptyFolder", &p.empty_folder)?;
    write_dict(e, "EmptyData", &p.empty_data, false)?;
    write_dict(e, "Postfix", &p.postfix, false)?;
    write_dict(e, "Prefix", &p.prefix, false)?;
    write_dict(e, "Seperator", &p.separator, false)?;
    write_dict(e, "IllegalCharacters", &p.illegal_characters, true)?;
    write_months(e, &p.months)?;
    write_dict(e, "TextBox", &p.text_box, false)?;
    e.text_elem("UseFolder", bool_str(p.use_folder))?;
    e.text_elem("UseFileName", bool_str(p.use_file_name))?;
    write_list(e, "ExcludeFolders", &p.exclude_folders)?;
    e.text_elem("DontAskWhenMultiOne", bool_str(p.dont_ask_when_multi_one))?;
    e.text_elem("RemoveEmptyFolder", bool_str(p.remove_empty_folder))?;
    write_list(e, "ExcludedEmptyFolder", &p.excluded_empty_folder)?;
    e.text_elem("MoveFileless", bool_str(p.move_fileless))?;
    e.text_elem("FilelessFormat", &p.fileless_format)?;
    e.text_elem("ExcludeMode", &p.exclude_mode)?;
    e.text_elem("FailEmptyValues", bool_str(p.fail_empty_values))?;
    e.text_elem("MoveFailed", bool_str(p.move_failed))?;
    e.text_elem("FailedFolder", &p.failed_folder)?;
    write_list(e, "FailedFields", &p.failed_fields)?;
    e.text_elem("Mode", &p.mode)?;
    e.text_elem("CopyMode", bool_str(p.copy_mode))?;
    e.text_elem("AutoSpaceFields", bool_str(p.auto_space_fields))?;
    e.text_elem("ReplaceMultipleSpaces", bool_str(p.replace_multiple_spaces))?;
    e.text_elem("CopyReadPercentage", bool_str(p.copy_read_percentage))?;
    // ExcludeRules last, like save_to_xml.
    e.start("ExcludeRules")?;
    e.attr("Operator", &p.exclude_operator)?;
    e.attr("ExcludeMode", &p.exclude_mode)?;
    for node in &p.exclude_rules {
        write_rule_node(e, node)?;
    }
    e.end()?;
    e.end()
}

fn bool_str(b: bool) -> &'static str {
    if b {
        "true"
    } else {
        "false"
    }
}

fn write_dict(
    e: &mut Emitter<Vec<u8>>,
    name: &str,
    dict: &BTreeMap<String, String>,
    write_empty: bool,
) -> std::io::Result<()> {
    e.start(name)?;
    for (k, v) in dict {
        if v.is_empty() && !write_empty {
            continue;
        }
        e.start("Item")?;
        e.attr("Name", k)?;
        e.attr("Value", v)?;
        e.end()?;
    }
    e.end()
}

fn write_months(
    e: &mut Emitter<Vec<u8>>,
    months: &BTreeMap<String, String>,
) -> std::io::Result<()> {
    e.start("Months")?;
    for (k, v) in months {
        if v.is_empty() {
            continue;
        }
        e.start("Item")?;
        e.attr("Name", k)?;
        e.attr("Value", v)?;
        e.end()?;
    }
    e.end()
}

fn write_list(e: &mut Emitter<Vec<u8>>, name: &str, list: &[String]) -> std::io::Result<()> {
    e.start(name)?;
    for item in list {
        if !item.is_empty() {
            e.text_elem("Item", item)?;
        }
    }
    e.end()
}

fn write_rule_node(e: &mut Emitter<Vec<u8>>, node: &RuleNode) -> std::io::Result<()> {
    match node {
        RuleNode::Rule(r) => {
            e.start("ExcludeRule")?;
            e.attr("Field", &r.field)?;
            e.attr("Operator", &r.operator)?;
            e.attr("Value", &r.value)?;
            e.end()
        }
        RuleNode::Group { operator, rules } => {
            e.start("ExcludeGroup")?;
            e.attr("Operator", operator)?;
            for child in rules {
                write_rule_node(e, child)?;
            }
            e.end()
        }
    }
}

/// Reads profiles from the addon's XML. Accepts the store root
/// (`Profiles`), a single exported `Profile`, and the legacy
/// `Settings`/`Setting` roots. Returns the profiles in document order
/// plus the `LastUsed` names.
pub fn import_profiles_xml(text: &str) -> Result<(Vec<Profile>, Vec<String>), ProfileError> {
    let mut bytes = text.as_bytes();
    let mut reader = XmlReader::new(&mut bytes);
    let mut profiles = Vec::new();
    let mut last_used = Vec::new();
    loop {
        match reader
            .next_tok()
            .map_err(|e| ProfileError::Xml(e.to_string()))?
        {
            Tok::Eof => break,
            Tok::Start(s) if s.name == "Profiles" => {
                if let Some(lu) = s.attr("LastUsed") {
                    last_used = lu
                        .split(',')
                        .map(|n| n.trim().to_string())
                        .filter(|n| !n.is_empty())
                        .collect();
                }
                read_profile_children(&mut reader, "Profiles", &mut profiles)?;
            }
            Tok::Start(s) if s.name == "Profile" => {
                if let Some(p) = read_profile(&mut reader, &s)? {
                    profiles.push(p);
                }
            }
            Tok::Start(s) if s.name == "Settings" => {
                read_profile_children(&mut reader, "Settings", &mut profiles)?;
            }
            Tok::Start(s) if s.name == "Setting" => {
                if let Some(p) = read_profile(&mut reader, &s)? {
                    profiles.push(p);
                }
            }
            _ => {}
        }
    }
    Ok((profiles, last_used))
}

fn read_profile_children(
    reader: &mut XmlReader,
    root: &str,
    out: &mut Vec<Profile>,
) -> Result<(), ProfileError> {
    loop {
        match reader
            .next_tok()
            .map_err(|e| ProfileError::Xml(e.to_string()))?
        {
            Tok::Eof => return Ok(()),
            Tok::End(name) if name == root => return Ok(()),
            Tok::Start(s) if s.name == "Profile" || s.name == "Setting" => {
                if let Some(p) = read_profile(reader, &s)? {
                    out.push(p);
                }
            }
            _ => {}
        }
    }
}

/// Reads one `<Profile Name="…">` node. A missing `Name` attribute
/// skips the profile (the addon shows a message and skips).
fn read_profile(
    reader: &mut XmlReader,
    start: &cr_core::xml::Start,
) -> Result<Option<Profile>, ProfileError> {
    let Some(name_attr) = start.attr("Name") else {
        return skip_element(reader);
    };
    // The addon starts Version at 0; a missing attribute means a
    // pre-2.0 profile and the legacy renames run.
    let mut p = Profile {
        name: name_attr.to_string(),
        ..Profile::default()
    };
    p.version = start
        .attr("Version")
        .and_then(|v| v.trim().parse::<f64>().ok())
        .unwrap_or(0.0);
    loop {
        match reader
            .next_tok()
            .map_err(|e| ProfileError::Xml(e.to_string()))?
        {
            Tok::Eof | Tok::End(_) | Tok::Text(_) => break,
            Tok::Start(s) => match s.name.as_str() {
                "FolderTemplate" => p.folder_template = read_text(reader),
                "BaseFolder" => p.base_folder = read_text(reader),
                "FileTemplate" => p.file_template = read_text(reader),
                "EmptyFolder" => p.empty_folder = read_text(reader),
                "EmptyData" => read_dict(reader, &mut p.empty_data)?,
                "Postfix" => read_dict(reader, &mut p.postfix)?,
                "Prefix" => read_dict(reader, &mut p.prefix)?,
                "Seperator" => read_dict(reader, &mut p.separator)?,
                "TextBox" => read_dict(reader, &mut p.text_box)?,
                "IllegalCharacters" => read_dict(reader, &mut p.illegal_characters)?,
                "Months" => read_months(reader, &mut p.months)?,
                "UseFolder" => p.use_folder = read_bool(reader, p.use_folder),
                "UseFileName" => p.use_file_name = read_bool(reader, p.use_file_name),
                "ExcludeFolders" => read_list(reader, "ExcludeFolders", &mut p.exclude_folders)?,
                "DontAskWhenMultiOne" => {
                    p.dont_ask_when_multi_one = read_bool(reader, p.dont_ask_when_multi_one)
                }
                "RemoveEmptyFolder" => {
                    p.remove_empty_folder = read_bool(reader, p.remove_empty_folder)
                }
                "ExcludedEmptyFolder" => {
                    read_list(reader, "ExcludedEmptyFolder", &mut p.excluded_empty_folder)?
                }
                "MoveFileless" => p.move_fileless = read_bool(reader, p.move_fileless),
                "FilelessFormat" => p.fileless_format = read_text(reader),
                "ExcludeMode" => p.exclude_mode = read_text(reader),
                "FailEmptyValues" => p.fail_empty_values = read_bool(reader, p.fail_empty_values),
                "MoveFailed" => p.move_failed = read_bool(reader, p.move_failed),
                "FailedFolder" => p.failed_folder = read_text(reader),
                "FailedFields" => read_list(reader, "FailedFields", &mut p.failed_fields)?,
                "Mode" => p.mode = read_text(reader),
                "CopyMode" => p.copy_mode = read_bool(reader, p.copy_mode),
                "AutoSpaceFields" => p.auto_space_fields = read_bool(reader, p.auto_space_fields),
                "ReplaceMultipleSpaces" => {
                    p.replace_multiple_spaces = read_bool(reader, p.replace_multiple_spaces)
                }
                "CopyReadPercentage" => {
                    p.copy_read_percentage = read_bool(reader, p.copy_read_percentage)
                }
                "ExcludeRules" => {
                    if let Some(op) = s.attr("Operator") {
                        p.exclude_operator = op.to_string();
                    }
                    if let Some(mode) = s.attr("ExcludeMode") {
                        p.exclude_mode = mode.to_string();
                    }
                    read_rule_children(reader, &mut p.exclude_rules)?;
                }
                _ => {
                    skip_element(reader)?;
                }
            },
        }
    }
    p.update_legacy();
    Ok(Some(p))
}

/// Reads to the matching end of the current element, discarding.
fn skip_element(reader: &mut XmlReader) -> Result<Option<Profile>, ProfileError> {
    let mut depth = 0usize;
    loop {
        match reader
            .next_tok()
            .map_err(|e| ProfileError::Xml(e.to_string()))?
        {
            Tok::Eof => return Ok(None),
            Tok::Start(_) => depth += 1,
            Tok::End(_) => {
                if depth == 0 {
                    return Ok(None);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
}

fn read_text(reader: &mut XmlReader) -> String {
    let mut out = String::new();
    let _ = read_text_into(reader, &mut out);
    out
}

/// Reads the element's text (and nothing else) into `out`, consuming
/// to the element end. Returns false when the end tag never came.
fn read_text_into(reader: &mut XmlReader, out: &mut String) -> bool {
    loop {
        match reader.next_tok() {
            Ok(Tok::Eof) => return false,
            Ok(Tok::Text(t)) => out.push_str(&t),
            Ok(Tok::End(_)) => return true,
            Ok(Tok::Start(_)) => return false,
            Err(_) => return false,
        }
    }
}

fn read_bool(reader: &mut XmlReader, default: bool) -> bool {
    let mut text = String::new();
    if !read_text_into(reader, &mut text) {
        return default;
    }
    match text.trim().to_ascii_lowercase().as_str() {
        "true" => true,
        "false" => false,
        _ => default,
    }
}

fn read_dict(
    reader: &mut XmlReader,
    dict: &mut BTreeMap<String, String>,
) -> Result<(), ProfileError> {
    loop {
        match reader
            .next_tok()
            .map_err(|e| ProfileError::Xml(e.to_string()))?
        {
            Tok::Eof | Tok::End(_) => return Ok(()),
            Tok::Start(s) if s.name == "Item" => {
                let key = s.attr("Name").unwrap_or("").to_string();
                let value = s.attr("Value").unwrap_or("").to_string();
                if read_text_into(reader, &mut String::new()) && !key.is_empty() {
                    dict.insert(key, value);
                } else {
                    return Ok(());
                }
            }
            Tok::Start(_) if !skip_plain(reader) => return Ok(()),
            _ => {}
        }
    }
}

fn read_months(
    reader: &mut XmlReader,
    months: &mut BTreeMap<String, String>,
) -> Result<(), ProfileError> {
    loop {
        match reader
            .next_tok()
            .map_err(|e| ProfileError::Xml(e.to_string()))?
        {
            Tok::Eof | Tok::End(_) => return Ok(()),
            Tok::Start(s) if s.name == "Item" => {
                let key = s.attr("Name").and_then(|k| k.trim().parse::<i32>().ok());
                let value = s.attr("Value").unwrap_or("").to_string();
                if read_text_into(reader, &mut String::new()) {
                    if let Some(k) = key {
                        months.insert(k.to_string(), value);
                    }
                } else {
                    return Ok(());
                }
            }
            Tok::Start(_) if !skip_plain(reader) => return Ok(()),
            _ => {}
        }
    }
}

fn read_list(
    reader: &mut XmlReader,
    name: &str,
    list: &mut Vec<String>,
) -> Result<(), ProfileError> {
    let _ = name;
    loop {
        match reader
            .next_tok()
            .map_err(|e| ProfileError::Xml(e.to_string()))?
        {
            Tok::Eof | Tok::End(_) => return Ok(()),
            Tok::Start(s) if s.name == "Item" => {
                let mut value = String::new();
                if read_text_into(reader, &mut value) {
                    list.push(value);
                } else {
                    return Ok(());
                }
            }
            Tok::Start(_) if !skip_plain(reader) => return Ok(()),
            _ => {}
        }
    }
}

fn read_rule_children(
    reader: &mut XmlReader,
    rules: &mut Vec<RuleNode>,
) -> Result<(), ProfileError> {
    loop {
        match reader
            .next_tok()
            .map_err(|e| ProfileError::Xml(e.to_string()))?
        {
            Tok::Eof | Tok::End(_) => return Ok(()),
            Tok::Start(s) if s.name == "ExcludeRule" => {
                let rule = ExcludeRule {
                    field: s.attr("Field").unwrap_or("").to_string(),
                    operator: s.attr("Operator").unwrap_or("").to_string(),
                    // 1.7.17 profiles carry the value as `Text`.
                    value: s
                        .attr("Value")
                        .or_else(|| s.attr("Text"))
                        .unwrap_or("")
                        .to_string(),
                };
                // Consume to the element end.
                loop {
                    match reader
                        .next_tok()
                        .map_err(|e| ProfileError::Xml(e.to_string()))?
                    {
                        Tok::Eof | Tok::End(_) => break,
                        Tok::Start(_) if !skip_plain(reader) => break,
                        _ => {}
                    }
                }
                rules.push(RuleNode::Rule(rule));
            }
            Tok::Start(s) if s.name == "ExcludeGroup" => {
                let operator = s.attr("Operator").unwrap_or(EXCLUDE_ANY).to_string();
                let mut children = Vec::new();
                read_rule_children(reader, &mut children)?;
                rules.push(RuleNode::Group {
                    operator,
                    rules: children,
                });
            }
            Tok::Start(_) if !skip_plain(reader) => return Ok(()),
            _ => {}
        }
    }
}

/// Skips to the matching end of the current element. False at Eof.
fn skip_plain(reader: &mut XmlReader) -> bool {
    let mut depth = 0usize;
    loop {
        match reader.next_tok() {
            Ok(Tok::Eof) => return false,
            Ok(Tok::Start(_)) => depth += 1,
            Ok(Tok::End(_)) => {
                if depth == 0 {
                    return true;
                }
                depth -= 1;
            }
            Ok(_) => {}
            Err(_) => return false,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_addon() {
        let p = Profile::default();
        assert_eq!(p.illegal_characters.get("?").map(String::as_str), Some(""));
        assert_eq!(
            p.illegal_characters.get(":").map(String::as_str),
            Some(" - ")
        );
        assert_eq!(p.illegal_characters.len(), 9);
        assert_eq!(p.months.get("1").map(String::as_str), Some("January"));
        assert_eq!(p.months.get("13").map(String::as_str), Some("Spring"));
        assert_eq!(p.months.len(), 16);
        assert!(p.use_folder && p.use_file_name && p.dont_ask_when_multi_one);
        assert_eq!(p.exclude_operator, "Any");
        assert_eq!(p.exclude_mode, "Do not");
        assert_eq!(p.fileless_format, ".jpg");
        assert_eq!(p.mode, "Move");
        assert!(
            p.copy_mode
                && p.auto_space_fields
                && p.replace_multiple_spaces
                && p.copy_read_percentage
        );
        assert!(!p.move_fileless && !p.fail_empty_values && !p.move_failed);
    }

    #[test]
    fn builtin_default_has_the_addon_templates() {
        let p = Profile::builtin_default();
        assert_eq!(p.name, "Default");
        assert_eq!(
            p.file_template,
            "{<series>}{ Vol.<volume>}{ #<number2>}{ (of <count2>)}{ ({<month>, }<year>)}"
        );
        assert_eq!(
            p.folder_template,
            "{<publisher>}\\{<imprint>}\\{<series>}{ (<startyear>{ <format>})}"
        );
    }

    #[test]
    fn toml_round_trip() {
        let mut p = Profile::builtin_default();
        p.base_folder = "/comics".into();
        p.exclude_rules.push(RuleNode::Rule(ExcludeRule {
            field: "Read Percentage".into(),
            operator: "greater than".into(),
            value: "0".into(),
        }));
        p.exclude_rules.push(RuleNode::Group {
            operator: "All".into(),
            rules: vec![RuleNode::Rule(ExcludeRule {
                field: "Manga".into(),
                operator: "is".into(),
                value: "Yes (Right to Left)".into(),
            })],
        });
        let doc = PluginSettings {
            last_used: vec!["Default".into()],
            profiles: vec![p.clone()],
        };
        let text = toml::to_string(&doc).expect("toml");
        let back: PluginSettings = toml::from_str(&text).expect("toml parse");
        assert_eq!(back.profiles.len(), 1);
        assert_eq!(back.profiles[0], p);
        assert_eq!(back.last_used, vec!["Default".to_string()]);
    }

    #[test]
    fn toml_partial_profile_fills_defaults() {
        let text = "[[profile]]\nname = \"Only Name\"\n";
        let doc: PluginSettings = toml::from_str(text).expect("parse");
        assert_eq!(doc.profiles.len(), 1);
        let p = &doc.profiles[0];
        assert_eq!(p.name, "Only Name");
        assert_eq!(p.illegal_characters.len(), 9);
        assert_eq!(p.months.len(), 16);
        assert_eq!(p.mode, "Move");
    }

    #[test]
    fn xml_export_import_round_trip() {
        let mut p = Profile::builtin_default();
        p.base_folder = "/comics".into();
        p.exclude_rules.push(RuleNode::Rule(ExcludeRule {
            field: "Read Percentage".into(),
            operator: "greater than".into(),
            value: "0".into(),
        }));
        p.exclude_rules.push(RuleNode::Group {
            operator: "Any".into(),
            rules: vec![RuleNode::Rule(ExcludeRule {
                field: "Manga".into(),
                operator: "is".into(),
                value: "Yes (Right to Left)".into(),
            })],
        });
        p.exclude_folders.push("/keep/out".into());
        p.empty_data.insert("ShadowSeries".into(), "Unknown".into());
        p.months.insert("1".to_string(), "Januar".into());
        let xml = export_profiles_xml(&[&p], &["Default"]);
        let (profiles, last_used) = import_profiles_xml(&xml).expect("import");
        assert_eq!(last_used, vec!["Default".to_string()]);
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0], p);
    }

    #[test]
    fn xml_single_profile_root() {
        let mut p = Profile::builtin_default();
        p.name = "Solo".into();
        let xml = export_single_profile_xml(&p);
        let (profiles, _) = import_profiles_xml(&xml).expect("import");
        assert_eq!(profiles.len(), 1);
        assert_eq!(profiles[0], p);
    }

    #[test]
    fn xml_shape_carries_the_addon_elements() {
        let p = Profile::builtin_default();
        let xml = export_single_profile_xml(&p);
        for name in [
            "FolderTemplate",
            "BaseFolder",
            "FileTemplate",
            "EmptyData",
            "Postfix",
            "Prefix",
            "Seperator",
            "IllegalCharacters",
            "Months",
            "TextBox",
            "UseFolder",
            "UseFileName",
            "ExcludeFolders",
            "DontAskWhenMultiOne",
            "RemoveEmptyFolder",
            "ExcludedEmptyFolder",
            "MoveFileless",
            "FilelessFormat",
            "ExcludeMode",
            "FailEmptyValues",
            "MoveFailed",
            "FailedFolder",
            "FailedFields",
            "Mode",
            "CopyMode",
            "AutoSpaceFields",
            "ReplaceMultipleSpaces",
            "CopyReadPercentage",
            "ExcludeRules",
        ] {
            assert!(xml.contains(&format!("<{name}")), "missing {name}");
        }
        assert!(xml.contains("Name=\"Default\""));
        assert!(xml.contains("Version=\"2.1\""));
        assert!(xml.contains("Value=\"January\""));
    }

    #[test]
    fn legacy_settings_root_and_text_attr() {
        let xml = r#"<Settings>
  <Setting Name="Old">
    <FolderTemplate>{&lt;series&gt;}</FolderTemplate>
    <FileTemplate>{&lt;number&gt;}</FileTemplate>
    <Mode>Test</Mode>
    <ExcludeRules Operator="Any" ExcludeMode="Do not">
      <ExcludeRule Field="Series" Operator="is" Text="Batman" />
    </ExcludeRules>
  </Setting>
</Settings>"#;
        let (profiles, _) = import_profiles_xml(xml).expect("import");
        assert_eq!(profiles.len(), 1);
        let p = &profiles[0];
        assert_eq!(p.name, "Old");
        // The legacy "Test" mode migrates to Simulate (Version missing).
        assert_eq!(p.mode, "Simulate");
        assert_eq!(p.exclude_rules.len(), 1);
        match &p.exclude_rules[0] {
            RuleNode::Rule(r) => {
                assert_eq!(r.field, "Series");
                assert_eq!(r.operator, "is");
                // The legacy `Text` attribute carries the value.
                assert_eq!(r.value, "Batman");
            }
            other => panic!("expected a rule, got {other:?}"),
        }
    }

    #[test]
    fn legacy_16_key_renames() {
        let xml = r#"<Profiles>
  <Profile Name="Old" Version="1.6">
    <EmptyData>
      <Item Name="Series" Value="Unknown Series" />
      <Item Name="Language" Value="Unknown" />
    </EmptyData>
    <TextBox>
      <Item Name="SeriesComplete" Value="complete" />
    </TextBox>
    <Prefix>
      <Item Name="Read" Value="read " />
    </Prefix>
  </Profile>
</Profiles>"#;
        let (profiles, _) = import_profiles_xml(xml).expect("import");
        let p = &profiles[0];
        assert_eq!(
            p.empty_data.get("ShadowSeries").map(String::as_str),
            Some("Unknown Series")
        );
        assert_eq!(
            p.empty_data.get("LanguageISO").map(String::as_str),
            Some("Unknown")
        );
        assert!(!p.empty_data.contains_key("Series"));
        assert_eq!(
            p.text_box.get("Series Complete").map(String::as_str),
            Some("complete")
        );
        assert_eq!(
            p.prefix.get("Read Percentage").map(String::as_str),
            Some("read ")
        );
    }

    #[test]
    fn missing_name_skips_the_profile() {
        let xml = "<Profiles><Profile Version=\"2.1\"><Mode>Move</Mode></Profile></Profiles>";
        let (profiles, _) = import_profiles_xml(xml).expect("import");
        assert!(profiles.is_empty());
    }
}
