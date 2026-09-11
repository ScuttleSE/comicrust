//! The unified configuration file — `comicrust.toml` under the config
//! tree (ADR-033). ONE TOML document replaces the three previous
//! stores:
//!
//! - the `comicrust.ini` search chain (`ExtendedSettings` +
//!   `EngineConfiguration` keys),
//! - `Config.xml` (the [`Settings`] object, ADR-023 — superseded),
//! - the plugins' private settings files (ADR-031's plugin-local
//!   `settings.json` — superseded; the Comic Vine Scraper rides
//!   `[plugins.comic-vine-scraper]`).
//!
//! The ComicDb.xml database is NOT part of this file and keeps its
//! byte-stability invariant (the one artifact users cannot lose).
//!
//! Sections:
//! - `[extended]` — the `ExtendedSettings` keys, stored and applied
//!   verbatim through the field registry (the argv switches still
//!   overlay at boot; argv values are never written back).
//! - `[engine]` — the `EngineConfiguration` keys, same registry
//!   currency; the Size/Color converter fields keep the .NET text
//!   forms (`"512, 512"`, `"r, g, b"`).
//! - `[settings]` — the [`Settings`] fields under their C# member
//!   names, serde round-tripped.
//! - `[plugins.<name>]` — plugin-owned tables behind typed accessors
//!   (cr-core stores them opaque, so it never depends on cr-scrape).
//! - `[data]` — the user-editable data tables (the Comic Vine
//!   imprint→publisher list among them), seeded on first boot; a
//!   per-table revision marker merges NEW built-in entries on upgrade
//!   without touching user edits.
//!
//! Precedence: defaults < file < command line. Like `Config.xml`
//! before it, the file is read once at boot; every save point rewrites
//! the whole file from the session. Hand edits apply at the next
//! start; hand-added comments do not survive a rewrite (values do).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use toml::Value;

use super::ini::IniValues;

/// The config schema version (`version = 1`).
pub const CONFIG_VERSION: i64 = 1;

/// The file name under the config tree (`comicrust.toml`).
pub const CONFIG_FILE_NAME: &str = "comicrust.toml";

/// One built-in data table entry set.
pub type BuiltinTable = (&'static str, i64, &'static [(&'static str, &'static str)]);
/// One session data table: the stored revision + the entries.
pub type DataTable = (i64, BTreeMap<String, String>);

// ---------- serde for the XML-era scalar types ----------
//
// The settings types serialize as their C# XML string forms, so the
// values stay traceable ("FlipPages", not 1). Unknown member names
// fall back to the type default (the reader's skip tolerance).

macro_rules! serde_xml_enum {
    ($($t:ty),+ $(,)?) => {
        $(
            impl serde::Serialize for $t {
                fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                    s.serialize_str(&self.to_xml())
                }
            }
            impl<'de> serde::Deserialize<'de> for $t {
                fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                    let text = String::deserialize(d)?;
                    Ok(Self::from_xml(&text).unwrap_or_default())
                }
            }
        )+
    };
}

serde_xml_enum!(
    crate::model::enums::ComicPageType,
    super::enums::TabLayouts,
    super::enums::ImageDisplayOptions,
    super::enums::HiddenMessageBoxes,
    super::enums::LibraryGauges,
    super::enums::MagnifierStyle,
    super::enums::RightToLeftReadingMode,
    crate::model::enums::ItemViewMode,
    crate::model::enums::SortOrder,
    crate::model::enums::ImageRotation,
);

impl serde::Serialize for crate::xml::scalar::CrGuid {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_d_string())
    }
}

impl<'de> serde::Deserialize<'de> for crate::xml::scalar::CrGuid {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Ok(crate::xml::scalar::CrGuid::parse(&text).unwrap_or_default())
    }
}

/// The f32 fields serialize through the shortest f32 text ("0.05",
/// not the f64-widened `0.05000000074505806`) so hand edits stay
/// clean; the round trip is exact.
pub mod f32_shortest {
    pub fn serialize<S: serde::Serializer>(v: &f32, s: S) -> Result<S::Ok, S::Error> {
        let text = format!("{v}");
        s.serialize_f64(text.parse::<f64>().unwrap_or(*v as f64))
    }

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(d: D) -> Result<f32, D::Error> {
        serde::Deserialize::deserialize(d)
    }
}

// ---------- The document ----------

/// The whole `comicrust.toml` payload (serde drives the section
/// layout; the field order is the file order).
#[derive(Serialize, Deserialize, Debug)]
#[serde(default)]
pub struct UnifiedDoc {
    version: i64,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    extended: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    engine: BTreeMap<String, Value>,
    #[serde(default)]
    settings: super::settings::Settings,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    plugins: BTreeMap<String, toml::Table>,
    #[serde(default)]
    data: DataSection,
}

impl Default for UnifiedDoc {
    fn default() -> Self {
        UnifiedDoc {
            version: CONFIG_VERSION,
            extended: BTreeMap::new(),
            engine: BTreeMap::new(),
            settings: super::settings::Settings::default(),
            plugins: BTreeMap::new(),
            data: DataSection::default(),
        }
    }
}

/// The `[data]` section: the per-table revision markers ride
/// `[data.revision]`, the tables themselves are the flattened
/// subtables (`[data.imprints]`).
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub struct DataSection {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    revision: BTreeMap<String, i64>,
    #[serde(default, flatten, skip_serializing_if = "BTreeMap::is_empty")]
    tables: BTreeMap<String, BTreeMap<String, String>>,
}

/// One built-in data table: the name, the revision the app ships, and
/// the verbatim entries. The Comic Vine imprint table is the port of
/// the plugin's `cvimprints.py` — keys and values must match the
/// ComicVine database exactly (case and punctuation included), so the
/// entries stay verbatim. `Bump the revision when a release extends a
/// table; the boot merge adds ONLY the missing keys.
pub const BUILTIN_TABLES: &[BuiltinTable] = &[("imprints", IMPRINTS_REVISION, IMPRINTS)];

/// The revision of the built-in imprint table; bump when the seed
/// grows so existing config files merge the new entries.
pub const IMPRINTS_REVISION: i64 = 1;

/// The built-in imprint → parent publisher seed (the plugin's
/// `cvimprints.py` table, verbatim).
pub const IMPRINTS: &[(&str, &str)] = &[
    ("2000AD", "DC Comics"),
    ("Adventure", "Malibu"),
    ("Aircel Publishing", "Malibu"),
    ("America's Best Comics", "DC Comics"),
    ("Amerotica ", "Nbm"),
    ("Antimatter", "Amryl Entertainment"),
    ("Apparat", "Avatar Press"),
    ("Archaia", "Boom!"),
    ("Berger Books", "Dark Horse Comics"),
    ("BOOM! Box", "Boom!"),
    ("Boundless Comics", "Avatar Press"),
    ("Black Bull", "Wizard"),
    ("Black Crown", "IDW Publishing"),
    ("Blu Manga", "Tokyopop"),
    ("CMX", "DC Comics"),
    ("Chaos! Comics", "Dynamite Entertainment"),
    ("Cliffhanger", "DC Comics"),
    ("Comic Bom Bom", "Kodansha"),
    ("ComicsLit", "Nbm"),
    ("Curtis Magazines", "Marvel"),
    ("Danger Zone", "Action Lab"),
    ("Dark Horse Books", "Dark Horse Comics"),
    ("Dark Horse Manga", "Dark Horse Comics"),
    ("Desperado Publishing", "Image"),
    ("Epic", "Marvel"),
    ("Eternity", "Malibu"),
    ("Eurotica ", "Nbm"),
    ("Focus", "DC Comics"),
    ("Helix", "DC Comics"),
    ("Hero Comics", "Heroic Publishing"),
    ("Homage comics", "DC Comics"),
    ("Hudson Street Press", "Penguin Group"),
    ("Icon Comics", "Marvel"),
    ("Impact", "DC Comics"),
    ("Jets Comics", "Hakusensha"),
    ("KaBOOM!", "Boom!"),
    ("KiZoic", "Ape Entertainment"),
    ("Kodansha Comics Digital-First!", "Kodansha"),
    ("Kodansha Comics USA", "Kodansha"),
    ("MAD", "DC Comics"),
    ("Marvel Digital Comics Unlimited", "Marvel"),
    ("Marvel Knights", "Marvel"),
    ("Marvel Music", "Marvel"),
    ("Marvel Soleil", "Marvel"),
    ("Marvel UK", "Marvel"),
    ("Maverick", "Dark Horse Comics"),
    ("Max", "Marvel"),
    ("Milestone", "DC Comics"),
    ("Minx", "DC Comics"),
    ("Papercutz", "Nbm"),
    ("Paradox Press", "DC Comics"),
    ("Piranha Press", "DC Comics"),
    ("Quillion", "Lion Forge Comics"),
    ("Razorline", "Marvel"),
    ("Roar Comics", "Lion Forge Comics"),
    ("ShadowLine", "Image"),
    ("Silverline", "Image"),
    ("Sin Factory Comix", "Radio Comix"),
    ("Skybound", "Image"),
    ("Slave Labor", "Slg Publishing"),
    ("Star Comics", "Marvel"),
    ("Tangent Comics", "DC Comics"),
    ("Titan Books", "Titan Comics"),
    ("Todd McFarlane Productions", "Image"),
    ("Tokuma Comics", "Tokuma Shoten"),
    ("Top Cow", "Image"),
    ("Top Shelf", "IDW Publishing"),
    ("Ultraverse", "Malibu"),
    ("Vertical", "Kodansha"),
    ("Vertigo", "DC Comics"),
    ("Wildstorm", "DC Comics"),
    ("Zuda Comics", "DC Comics"),
];

// ---------- The session sections ----------
//
// The file is read once at boot; these hold the loaded sections so a
// save point can write the whole file back verbatim (unknown keys
// preserved; the argv overlay never leaks).

static EXTENDED_SECTION: std::sync::RwLock<BTreeMap<String, Value>> =
    std::sync::RwLock::new(BTreeMap::new());
static ENGINE_SECTION: std::sync::RwLock<BTreeMap<String, Value>> =
    std::sync::RwLock::new(BTreeMap::new());
static PLUGINS: std::sync::RwLock<BTreeMap<String, toml::Table>> =
    std::sync::RwLock::new(BTreeMap::new());
static DATA_TABLES: std::sync::RwLock<BTreeMap<String, DataTable>> =
    std::sync::RwLock::new(BTreeMap::new());

/// What one boot read out of the file.
pub struct LoadedConfig {
    /// The `[extended]` keys (flattened; the argv overlay rides on
    /// top inside `ExtendedSettings::load`).
    pub extended: IniValues,
    /// The `[engine]` keys.
    pub engine: IniValues,
    /// The parsed `[settings]` object (defaults on a missing or
    /// corrupt file).
    pub settings: super::settings::Settings,
}

/// Reads the unified config at `file` (a missing or corrupt file
/// means the defaults), installs the session sections, seeds the
/// built-in data tables, and REWRITES the file when the seed changed
/// (first boot). The argv overlay happens in the caller.
pub fn load(file: &Path) -> LoadedConfig {
    let (mut doc, corrupt) = match std::fs::read_to_string(file)
        .map_err(|e| e.to_string())
        .and_then(|text| toml::from_str::<UnifiedDoc>(&text).map_err(|e| e.to_string()))
    {
        Ok(doc) => (doc, false),
        Err(_) => (UnifiedDoc::default(), true),
    };
    let seeded = ensure_builtin_data(&mut doc);
    let corrupt = corrupt || seeded;

    *EXTENDED_SECTION
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = doc.extended.clone();
    *ENGINE_SECTION
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = doc.engine.clone();
    *PLUGINS
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = doc.plugins.clone();
    *DATA_TABLES
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = doc
        .data
        .tables
        .iter()
        .filter_map(|(name, t)| {
            doc.data
                .revision
                .get(name)
                .map(|r| (name.clone(), (*r, t.clone())))
        })
        .collect();

    if corrupt {
        if let Err(e) = write_doc(file, &doc) {
            eprintln!("saving {} failed: {e}", file.display());
        }
    }

    LoadedConfig {
        extended: flatten(&doc.extended),
        engine: flatten(&doc.engine),
        settings: doc.settings,
    }
}

/// Collects the session state into the document and writes it
/// (atomic tmp + rename, like the database save). The `[extended]`
/// and `[engine]` sections write back VERBATIM from the loaded
/// session (the C# never writes its ini either — only the explicit
/// [`update_extended_keys`] changes them).
pub fn save_file(file: &Path, settings: &super::settings::Settings) -> std::io::Result<()> {
    let doc = UnifiedDoc {
        version: CONFIG_VERSION,
        extended: EXTENDED_SECTION
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone(),
        engine: ENGINE_SECTION
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone(),
        settings: settings.clone(),
        plugins: PLUGINS
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone(),
        data: data_section(),
    };
    write_doc(file, &doc)
}

fn write_doc(file: &Path, doc: &UnifiedDoc) -> std::io::Result<()> {
    let text = toml::to_string_pretty(doc).map_err(std::io::Error::other)?;
    let tmp = file.with_extension("toml.tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(tmp, file)
}

fn data_section() -> DataSection {
    let tables = DATA_TABLES
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    DataSection {
        revision: tables.iter().map(|(k, (r, _))| (k.clone(), *r)).collect(),
        tables: tables
            .iter()
            .map(|(k, (_, t))| (k.clone(), t.clone()))
            .collect(),
    }
}

/// Inserts the `KEY=VALUE` pairs into the `[extended]` session section
/// (the old ini merge-writer's replacement; takes effect on the next
/// save/boot). The key match is case-insensitive like the ini was —
/// a stored key keeps its position, the canonical spelling replaces.
pub fn update_extended_keys<'a>(keys: &[(&'a str, &'a str)]) {
    let mut sec = EXTENDED_SECTION
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for (key, value) in keys {
        let existing = sec.keys().find(|k| k.eq_ignore_ascii_case(key)).cloned();
        match existing {
            Some(k) => {
                sec.remove(&k);
                sec.insert(k, text_to_value(value));
            }
            None => {
                sec.insert(key.to_string(), text_to_value(value));
            }
        }
    }
}

/// The `[data]` lookup: the session table, or the built-in seed when
/// the section is absent (headless tests and an uninitialized
/// session — the values are identical by construction).
pub fn data_table(name: &str) -> BTreeMap<String, String> {
    if let Some((_, t)) = DATA_TABLES
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(name)
    {
        return t.clone();
    }
    BUILTIN_TABLES
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(_, _, entries)| {
            entries
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// The plugin table accessor: deserializes the section payload.
pub fn get_plugin<T: serde::de::DeserializeOwned>(name: &str) -> Option<T> {
    let table = PLUGINS
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(name)?
        .clone();
    T::deserialize(Value::Table(table)).ok()
}

/// Stores the plugin section payload (typed through serde; a
/// non-table payload is ignored).
pub fn set_plugin<T: serde::Serialize>(name: &str, value: &T) {
    if let Ok(Value::Table(table)) = Value::try_from(value) {
        PLUGINS
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(name.to_string(), table);
    }
}

/// The seed/merge: a missing built-in table is inserted whole; a
/// table whose stored revision is behind the built-in revision gains
/// ONLY the missing keys (user edits survive the merge); the revision
/// marker moves to the built-in revision. Returns true when the
/// document changed.
fn ensure_builtin_data(doc: &mut UnifiedDoc) -> bool {
    let mut changed = false;
    for (name, revision, entries) in BUILTIN_TABLES {
        let stored = doc.data.revision.get(*name).copied().unwrap_or(0);
        match doc.data.tables.get_mut(*name) {
            None => {
                doc.data.tables.insert(
                    (*name).to_string(),
                    entries
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect(),
                );
                doc.data.revision.insert((*name).to_string(), *revision);
                changed = true;
            }
            Some(table) if stored < *revision => {
                let before = table.len();
                for (k, v) in *entries {
                    table.entry(k.to_string()).or_insert(v.to_string());
                }
                doc.data.revision.insert((*name).to_string(), *revision);
                changed |= table.len() != before;
            }
            Some(_) => {}
        }
    }
    changed
}

/// The loaded `KEY=VALUE` text → a typed TOML value (bools/numbers
/// when the text reads like one, strings otherwise — the ini-style
/// traffic stays stringly through [`IniValues`]).
fn text_to_value(text: &str) -> Value {
    let trimmed = text.trim();
    match trimmed.to_ascii_lowercase().as_str() {
        "true" => return Value::Boolean(true),
        "false" => return Value::Boolean(false),
        _ => {}
    }
    if let Ok(i) = trimmed.parse::<i64>() {
        return Value::Integer(i);
    }
    if let Ok(f) = trimmed.parse::<f64>() {
        if trimmed.contains('.') || trimmed.contains('e') || trimmed.contains('E') {
            return Value::Float(f);
        }
    }
    Value::String(text.to_string())
}

/// The section → [`IniValues`] flatten (registry traffic is stringly
/// like the ini was).
fn flatten(section: &BTreeMap<String, Value>) -> IniValues {
    let mut out = IniValues::new();
    for (key, value) in section {
        let text = match value {
            Value::String(s) => s.clone(),
            Value::Boolean(b) => b.to_string(),
            Value::Integer(i) => i.to_string(),
            Value::Float(f) => f.to_string(),
            other => other.to_string(),
        };
        out.set(key.clone(), text);
    }
    out
}

/// The `cr-cli migrate` merge: loads the unified file (a missing file
/// starts fresh; a CORRUPT file is an error — never overwritten
/// blindly), inserts the entries into `[extended]`, and writes the
/// whole document back.
pub fn merge_extended_keys(file: &Path, entries: &[(&str, &str)]) -> Result<(), std::io::Error> {
    let mut doc = match std::fs::read_to_string(file) {
        Ok(text) => match toml::from_str::<UnifiedDoc>(&text) {
            Ok(doc) => doc,
            Err(e) => {
                return Err(std::io::Error::other(format!(
                    "parsing {} failed: {e}",
                    file.display()
                )))
            }
        },
        Err(_) if !file.exists() => UnifiedDoc::default(),
        Err(e) => return Err(e),
    };
    for (key, value) in entries {
        let existing = doc
            .extended
            .keys()
            .find(|k| k.eq_ignore_ascii_case(key))
            .cloned();
        match existing {
            Some(k) => {
                doc.extended.remove(&k);
                doc.extended.insert(k, text_to_value(value));
            }
            None => {
                doc.extended.insert(key.to_string(), text_to_value(value));
            }
        }
    }
    write_doc(file, &doc)
}

// ---------- Settings <-> TOML (the test seams) ----------

/// The `[settings]` payload as a TOML value (the round-trip test seam).
pub fn settings_to_value(s: &super::settings::Settings) -> Value {
    Value::try_from(s).expect("Settings serializes to TOML")
}

/// Parses a `[settings]` value; unknown or malformed fields fall back
/// to the defaults (the corrupt-file tolerance).
pub fn settings_from_value(v: &Value) -> Option<super::settings::Settings> {
    super::settings::Settings::deserialize(v.clone()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::settings::Settings;
    use std::sync::Mutex;

    /// The section globals are process-wide state — the tests that
    /// touch them hold this lock for their whole body (tests run in
    /// parallel).
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn lock_sections() -> std::sync::MutexGuard<'static, ()> {
        TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "comicrust-unified-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn reset_sections() {
        *EXTENDED_SECTION.write().unwrap() = BTreeMap::new();
        *ENGINE_SECTION.write().unwrap() = BTreeMap::new();
        *PLUGINS.write().unwrap() = BTreeMap::new();
        *DATA_TABLES.write().unwrap() = BTreeMap::new();
    }

    #[test]
    fn settings_round_trip_toml() {
        let mut s = Settings {
            track_current_page: false,
            quick_open_thumbnail_size: 256,
            mouse_wheel_speed: 3.5,
            magnify_size: (320, 240),
            right_to_left_reading_mode: super::super::enums::RightToLeftReadingMode::FlipPages,
            last_open_files: vec!["/a.cbz".into(), "/b.cbz".into()],
            favorite_folders: vec!["/comics".into()],
            selected_browser: Some(String::new()),
            plugins_states: None,
            ..Settings::default()
        };
        let value = settings_to_value(&s);
        // The C# member names ride the keys (the MB quirk included).
        let text = toml::to_string_pretty(&value).unwrap();
        assert!(text.contains("ThumbCacheSizeMB"), "{text}");
        assert!(text.contains("RemoveFilesfromDatabase"), "{text}");
        assert!(text.contains("InformationCover3D"), "{text}");
        assert!(!text.contains("PluginsStates"), "None omits the key");
        assert_eq!(settings_from_value(&value).unwrap(), s);
        // The composite workspace rides the nested tables.
        let mut ws = super::super::workspace::WorkspaceState::default();
        ws.view.mode = crate::model::enums::ItemViewMode::Detail;
        ws.view.columns.push(super::super::workspace::ColumnState {
            id: 3,
            visible: true,
            width: 90,
        });
        s.current_workspace = Some(ws);
        let value = settings_to_value(&s);
        assert_eq!(settings_from_value(&value).unwrap(), s);
    }

    #[test]
    fn registered_field_names_all_appear() {
        // Every registry field name (the C# property name — the ini
        // key spelling) must survive the PascalCase mapping.
        let text = toml::to_string_pretty(&settings_to_value(&Settings::default())).unwrap();
        for f in crate::settings::settings::SETTINGS_FIELDS {
            assert!(
                text.contains(f.name),
                "missing {} in the TOML output",
                f.name
            );
        }
    }

    #[test]
    fn missing_file_seeds_the_builtin_tables_and_rewrites() {
        let _guard = lock_sections();
        reset_sections();
        let dir = tmp_dir("seed");
        let file = dir.join(CONFIG_FILE_NAME);
        let loaded = load(&file);
        assert!(loaded.extended.is_empty());
        assert_eq!(loaded.settings, Settings::default());
        // The imprints table is seeded into the session AND the file.
        let imprints = data_table("imprints");
        assert_eq!(
            imprints.get("Vertigo").map(String::as_str),
            Some("DC Comics")
        );
        assert_eq!(imprints.len(), IMPRINTS.len());
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("[data.imprints]"), "{text}");
        assert!(text.contains("Vertigo = \"DC Comics\""), "{text}");
        // A second load is idempotent (the seed does not duplicate).
        let before = std::fs::read_to_string(&file).unwrap();
        let _ = load(&file);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), before);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupt_file_falls_back_to_defaults() {
        let _guard = lock_sections();
        reset_sections();
        let dir = tmp_dir("corrupt");
        let file = dir.join(CONFIG_FILE_NAME);
        std::fs::write(&file, "[settings\nunclosed").unwrap();
        let loaded = load(&file);
        assert_eq!(loaded.settings, Settings::default());
        // The corrupt file is rewritten with the defaults + the seed.
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("version = 1"), "{text}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn round_trip_all_sections() {
        let _guard = lock_sections();
        reset_sections();
        let dir = tmp_dir("roundtrip");
        let file = dir.join(CONFIG_FILE_NAME);

        // Boot 1: seed, mutate every section, save.
        let loaded = load(&file);
        update_extended_keys(&[("CachePath", "/mnt/cache"), ("Theme", "Dark")]);
        ENGINE_SECTION
            .write()
            .unwrap()
            .insert("IsRecentInDays".to_string(), Value::Integer(20));
        let mut settings = loaded.settings.clone();
        settings.show_quick_open = false;
        settings.track_current_page = false;
        set_plugin("comic-vine-scraper", &serde_json::json!({"apiKey": "k1"}));
        std::fs::write(
            &file,
            toml::to_string_pretty(&UnifiedDoc {
                extended: EXTENDED_SECTION.read().unwrap().clone(),
                engine: ENGINE_SECTION.read().unwrap().clone(),
                settings: settings.clone(),
                plugins: PLUGINS.read().unwrap().clone(),
                data: data_section(),
                version: CONFIG_VERSION,
            })
            .unwrap(),
        )
        .unwrap();

        // Boot 2: everything reads back.
        reset_sections();
        let loaded = load(&file);
        assert_eq!(loaded.extended.get("CachePath"), Some("/mnt/cache"));
        assert_eq!(loaded.engine.get("IsRecentInDays"), Some("20"));
        assert!(!loaded.settings.show_quick_open);
        let cfg: serde_json::Value = get_plugin("comic-vine-scraper").unwrap();
        assert_eq!(cfg["apiKey"], "k1");
        assert_eq!(
            data_table("imprints").get("Vertigo").map(String::as_str),
            Some("DC Comics")
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn revision_merge_adds_only_missing_keys() {
        let _guard = lock_sections();
        reset_sections();
        let dir = tmp_dir("merge");
        let file = dir.join(CONFIG_FILE_NAME);
        // A user-edited table: one built-in key DELETED, one custom
        // key ADDED, one built-in key EDITED — stamped with the
        // built-in revision.
        let mut doc = UnifiedDoc::default();
        let mut imprints: BTreeMap<String, String> = IMPRINTS
            .iter()
            .filter(|(k, _)| *k != "Adventure")
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        imprints.insert("My Imprint".to_string(), "My Publisher".to_string());
        imprints.insert("Vertigo".to_string(), "Not DC".to_string());
        doc.data.tables.insert("imprints".into(), imprints);
        doc.data
            .revision
            .insert("imprints".into(), IMPRINTS_REVISION);
        std::fs::write(&file, toml::to_string_pretty(&doc).unwrap()).unwrap();

        let loaded = load(&file);
        let imprints = data_table("imprints");
        // A current-revision table is untouched: the deletion stays,
        // the custom key stays, the edited value stays.
        assert_eq!(imprints.get("Vertigo").map(String::as_str), Some("Not DC"));
        assert_eq!(
            imprints.get("My Imprint").map(String::as_str),
            Some("My Publisher")
        );
        assert!(!imprints.contains_key("Adventure"));
        let _ = loaded;
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn older_revision_merges_new_builtins_only() {
        let _guard = lock_sections();
        reset_sections();
        let dir = tmp_dir("upgrade");
        let file = dir.join(CONFIG_FILE_NAME);
        let mut doc = UnifiedDoc::default();
        // An old install: revision 0, the seed minus one entry.
        let mut imprints: BTreeMap<String, String> = IMPRINTS
            .iter()
            .filter(|(k, _)| *k != "Vertigo")
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        imprints.insert("Custom".to_string(), "Custom Pub".to_string());
        doc.data.tables.insert("imprints".into(), imprints);
        doc.data.revision.insert("imprints".into(), 0);
        std::fs::write(&file, toml::to_string_pretty(&doc).unwrap()).unwrap();

        let _ = load(&file);
        let imprints = data_table("imprints");
        // The missing built-in key was ADDED, the custom key kept.
        assert_eq!(
            imprints.get("Vertigo").map(String::as_str),
            Some("DC Comics")
        );
        assert_eq!(
            imprints.get("Custom").map(String::as_str),
            Some("Custom Pub")
        );
        assert!(std::fs::read_to_string(&file)
            .unwrap()
            .contains("Custom = \"Custom Pub\""));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn data_table_falls_back_to_the_builtin_when_uninitialized() {
        let _guard = lock_sections();
        reset_sections();
        // Headless (no load() call): the built-in seed applies.
        assert_eq!(
            data_table("imprints").get("2000AD").map(String::as_str),
            Some("DC Comics")
        );
        assert!(data_table("no-such-table").is_empty());
    }

    #[test]
    fn extended_and_engine_flatten_apply() {
        let _guard = lock_sections();
        reset_sections();
        let dir = tmp_dir("flatten");
        let file = dir.join(CONFIG_FILE_NAME);
        update_extended_keys(&[("CachePath", "/mnt/cache"), ("Theme", "Dark")]);
        let extended = {
            let sec = EXTENDED_SECTION.read().unwrap();
            flatten(&sec)
        };
        let mut ext = super::super::ExtendedSettings::default();
        ext.load(&extended, &[]);
        assert_eq!(ext.cache_path.as_deref(), Some("/mnt/cache"));
        // UseDarkMode stays false (not in the section); Theme=Dark wins.
        assert_eq!(ext.effective_theme(), super::super::enums::Themes::Dark);

        // The engine section: the registry fields plus the Size/Color
        // converter texts.
        let mut engine_doc = UnifiedDoc::default();
        engine_doc
            .engine
            .insert("IsRecentInDays".to_string(), Value::Integer(21));
        engine_doc.engine.insert(
            "ListCoverSize".to_string(),
            Value::String("256, 384".into()),
        );
        engine_doc.engine.insert(
            "BlankPageColor".to_string(),
            Value::String("10, 20, 30".into()),
        );
        engine_doc
            .engine
            .insert("OfValues".to_string(), Value::String("of,von".into()));
        std::fs::write(&file, toml::to_string_pretty(&engine_doc).unwrap()).unwrap();
        reset_sections();
        let loaded = load(&file);
        let mut engine = super::super::EngineConfiguration::default();
        engine.load(&loaded.engine);
        assert_eq!(engine.is_recent_in_days, 21);
        assert_eq!(engine.list_cover_size, (256, 384));
        assert_eq!(engine.blank_page_color, (10, 20, 30));
        assert_eq!(engine.of_values.as_deref(), Some("of,von"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn merge_extended_keys_creates_and_updates() {
        let _guard = lock_sections();
        reset_sections();
        let dir = tmp_dir("migrate");
        let file = dir.join(CONFIG_FILE_NAME);
        merge_extended_keys(&file, &[("CachePath", "/tmp/ce-cache")]).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("[extended]"), "{text}");
        assert!(text.contains("CachePath = \"/tmp/ce-cache\""), "{text}");
        // A second merge replaces the key.
        merge_extended_keys(&file, &[("cachepath", "/other")]).unwrap();
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(text.contains("CachePath = \"/other\""), "{text}");
        // A corrupt file refuses (never overwritten blindly).
        std::fs::write(&file, "[unclosed").unwrap();
        assert!(merge_extended_keys(&file, &[("A", "1")]).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn plugin_round_trip() {
        let _guard = lock_sections();
        reset_sections();
        set_plugin("test-plugin", &serde_json::json!({"apiKey": "abc"}));
        let v: serde_json::Value = get_plugin("test-plugin").unwrap();
        assert_eq!(v["apiKey"], "abc");
        assert!(get_plugin::<serde_json::Value>("missing").is_none());
    }
}
