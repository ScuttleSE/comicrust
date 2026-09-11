//! Port of the Comic Vine Scraper's `configuration.py` — the scraper
//! settings. The basic options persist as the
//! `[plugins.comic-vine-scraper]` section of the unified config with
//! the C# `settings.dat` key names (`apiKey`, `updateSeries`, …) plus
//! the verbatim advanced-settings string; the advanced `KEY=VALUE`
//! text is reparsed on every change exactly like the C#
//! `__set_advanced_settings_s`.
//!
//! Storage moved into the unified config file (ADR-033; the
//! plugin-local `settings.json` of ADR-031 is superseded) — the load/
//! save rides `cr_core::settings::unified::{get_plugin,set_plugin}`
//! through the app session. The plugin-local directory keeps only
//! `prior_series.json` (the scrape-history cache).

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::LazyLock;

use fancy_regex::Regex;
use serde::{Deserialize, Serialize};

/// The advanced settings parsed out of the raw `KEY=VALUE` string.
#[derive(Clone, Debug, PartialEq)]
pub struct AdvancedSettings {
    pub ignored_publishers: BTreeSet<String>,
    pub ignored_searchterms: BTreeSet<String>,
    pub publisher_aliases: BTreeMap<String, String>,
    pub user_imprints: BTreeMap<String, String>,
    /// `IGNORE_BEFORE_YEAR` — default 0.
    pub ignored_before_year: i32,
    /// `IGNORE_AFTER_YEAR` — default 9999999.
    pub ignored_after_year: i32,
    /// `NEVER_IGNORE_THRESHOLD` — default 9999999.
    pub never_ignore_threshold: i32,
    /// `SCRAPE_RATING` — default false.
    pub update_rating: bool,
    /// `SHOW_COVERS` — default true.
    pub show_covers: bool,
    /// `WELCOME_DIALOG` — default true.
    pub welcome_dialog: bool,
    /// `ALT_SEARCH_REGEX` — default "" (a regex that fails to
    /// compile is silently ignored, C# parity).
    pub alt_search_regex: String,
    /// `IGNORE_FOLDERS` — default false.
    pub ignore_folders: bool,
    /// `FORCE_SERIES_ART` — default false.
    pub force_series_art: bool,
    /// `NOTE_SCRAPE_DATE` — default false.
    pub note_scrape_date: bool,
    /// `SCRAPE_DELAY` — default 1, but a parsed value clamps to
    /// 2..3600 (C# quirk).
    pub scrape_delay: i32,
    /// `MAX_SEARCH_RESULTS` — default 100, parsed clamp 10..5000.
    pub max_search_results: i32,
}

impl AdvancedSettings {
    fn with_defaults() -> Self {
        Self {
            ignored_publishers: BTreeSet::new(),
            ignored_searchterms: BTreeSet::new(),
            publisher_aliases: BTreeMap::new(),
            user_imprints: BTreeMap::new(),
            ignored_before_year: 0,
            ignored_after_year: 9_999_999,
            never_ignore_threshold: 9_999_999,
            update_rating: false,
            show_covers: true,
            welcome_dialog: true,
            alt_search_regex: String::new(),
            ignore_folders: false,
            force_series_art: false,
            note_scrape_date: false,
            scrape_delay: 1,
            max_search_results: 100,
        }
    }
}

/// The basic scraper settings (C# `Configuration`), with the parsed
/// advanced settings riding along. `scrape_in_groups` is carried by
/// the engine but never persisted — C# parity.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Configuration {
    pub api_key: String,

    pub overwrite_existing: bool,
    pub ignore_blanks: bool,
    pub convert_imprints: bool,
    pub autochoose_series: bool,
    pub confirm_issue: bool,
    pub download_thumbs: bool,
    pub preserve_thumbs: bool,
    pub fast_rescrape: bool,
    #[serde(rename = "updateNotes")]
    pub rescrape_notes: bool,
    #[serde(rename = "updateTags")]
    pub rescrape_tags: bool,
    pub summary_dialog: bool,

    pub update_series: bool,
    pub update_number: bool,
    pub update_published: bool,
    pub update_released: bool,
    pub update_title: bool,
    pub update_crossovers: bool,
    pub update_writer: bool,
    pub update_penciller: bool,
    pub update_inker: bool,
    pub update_cover_artist: bool,
    pub update_colorist: bool,
    pub update_letterer: bool,
    pub update_editor: bool,
    pub update_summary: bool,
    pub update_imprint: bool,
    pub update_publisher: bool,
    pub update_volume: bool,
    pub update_characters: bool,
    pub update_teams: bool,
    pub update_locations: bool,
    pub update_webpage: bool,

    /// The advanced-settings string, verbatim (the C# persists it as
    /// `advanced.dat`; here it rides the settings.json payload).
    pub advanced_settings: String,
    /// The parsed advanced settings, derived from
    /// `advanced_settings` on every change (not persisted — serde
    /// skips it; read it through [`Configuration::advanced`]).
    #[serde(skip)]
    advanced: AdvancedSettings,
}

impl Default for Configuration {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            overwrite_existing: true,
            ignore_blanks: false,
            convert_imprints: true,
            autochoose_series: false,
            confirm_issue: false,
            download_thumbs: true,
            preserve_thumbs: true,
            fast_rescrape: true,
            rescrape_notes: true,
            rescrape_tags: false,
            summary_dialog: true,
            update_series: true,
            update_number: true,
            update_published: true,
            update_released: true,
            update_title: true,
            update_crossovers: true,
            update_writer: true,
            update_penciller: true,
            update_inker: true,
            update_cover_artist: true,
            update_colorist: true,
            update_letterer: true,
            update_editor: true,
            update_summary: true,
            update_imprint: true,
            update_publisher: true,
            update_volume: true,
            update_characters: true,
            update_teams: true,
            update_locations: true,
            update_webpage: true,
            advanced_settings: String::new(),
            advanced: AdvancedSettings::with_defaults(),
        }
    }
}

impl Configuration {
    /// The parsed advanced settings. Always current: both mutation
    /// paths (`load`, `set_advanced_settings`) reparse.
    pub fn advanced(&self) -> &AdvancedSettings {
        &self.advanced
    }

    /// Replaces the advanced-settings string and reparses it
    /// (C# `__set_advanced_settings_s`).
    pub fn set_advanced_settings(&mut self, raw: &str) {
        self.advanced_settings = raw.trim().to_string();
        self.advanced = parse_advanced(&self.advanced_settings);
    }

    /// True when a scrape can run at all: a non-empty API key
    /// (C# `if not self.config.api_key_s`).
    pub fn has_api_key(&self) -> bool {
        !self.api_key.is_empty()
    }
}

/// The plugin-local state directory:
/// `$XDG_CONFIG_HOME/comicrust/plugins/comic-vine-scraper`
/// (default `~/.config/comicrust/plugins/comic-vine-scraper`). Only
/// `prior_series.json` (the scrape-history cache) lives there — the
/// settings moved into the unified config (ADR-033).
pub fn default_config_dir() -> PathBuf {
    config_dir_from(
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    )
}

fn config_dir_from(xdg_config_home: Option<PathBuf>, home: Option<PathBuf>) -> PathBuf {
    let root = match xdg_config_home {
        Some(v) if !v.as_os_str().is_empty() => v,
        _ => {
            let mut home = home.unwrap_or_else(|| PathBuf::from("."));
            home.push(".config");
            home
        }
    };
    root.join("comicrust")
        .join("plugins")
        .join("comic-vine-scraper")
}

/// Port of `__set_advanced_settings_s`: starts from the defaults,
/// then scans each line for the `KEY=VALUE` advanced settings.
pub fn parse_advanced(raw: &str) -> AdvancedSettings {
    let mut a = AdvancedSettings::with_defaults();
    for line in raw.split('\n') {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        parse_line(line, &mut a);
    }
    a
}

/// The advanced-settings line keys (the C# `Configuration` parses
/// one line per key). Public so the doc drift gate can walk them.
pub const ADVANCED_KEYS: [&str; 16] = [
    "IGNORE_PUBLISHER",
    "IGNORE_SEARCHTERM",
    "IGNORE_BEFORE_YEAR",
    "IGNORE_AFTER_YEAR",
    "NEVER_IGNORE_THRESHOLD",
    "SCRAPE_RATING",
    "SHOW_COVERS",
    "WELCOME_DIALOG",
    "ALT_SEARCH_REGEX",
    "IGNORE_FOLDERS",
    "FORCE_SERIES_ART",
    "NOTE_SCRAPE_DATE",
    "PUBLISHER_ALIAS",
    "IMPRINT",
    "SCRAPE_DELAY",
    "MAX_SEARCH_RESULTS",
];

fn parse_line(line: &str, a: &mut AdvancedSettings) {
    // the C# tries every key pattern against the line; the keys are
    // distinct prefixes, so at most one can match
    for key in ADVANCED_KEYS {
        if let Some(value) = extract_value(line, key) {
            apply(key, value, a);
            return;
        }
    }
}

fn apply(key: &str, value: &str, a: &mut AdvancedSettings) {
    match key {
        "IGNORE_PUBLISHER" => {
            a.ignored_publishers.insert(value.to_lowercase());
        }
        "IGNORE_SEARCHTERM" => {
            let term = value.to_lowercase();
            if !term.is_empty() && term.chars().all(|c| c.is_alphanumeric()) {
                a.ignored_searchterms.insert(term);
            }
        }
        "IGNORE_BEFORE_YEAR" => {
            if let Some(n) = as_int(value) {
                a.ignored_before_year = n;
            }
        }
        "IGNORE_AFTER_YEAR" => {
            if let Some(n) = as_int(value) {
                a.ignored_after_year = n;
            }
        }
        "NEVER_IGNORE_THRESHOLD" => {
            if let Some(n) = as_int(value) {
                a.never_ignore_threshold = n;
            }
        }
        "SCRAPE_RATING" => a.update_rating = is_true(value),
        "SHOW_COVERS" => a.show_covers = is_true(value),
        "WELCOME_DIALOG" => a.welcome_dialog = is_true(value),
        "ALT_SEARCH_REGEX" => {
            if Regex::new(value).is_ok() {
                a.alt_search_regex = value.to_string();
            }
        }
        "IGNORE_FOLDERS" => a.ignore_folders = is_true(value),
        "FORCE_SERIES_ART" => a.force_series_art = is_true(value),
        "NOTE_SCRAPE_DATE" => a.note_scrape_date = is_true(value),
        "PUBLISHER_ALIAS" => {
            if let Some((publisher, alias)) = split_arrow(value) {
                let publisher = trim_quotes(&publisher).to_lowercase();
                let alias = trim_quotes(&alias);
                if !publisher.is_empty() && !alias.is_empty() && alias.chars().count() <= 50 {
                    a.publisher_aliases.insert(publisher, alias);
                }
            }
        }
        "IMPRINT" => {
            if let Some((imprint, publisher)) = split_arrow(value) {
                let imprint = trim_quotes(&imprint).to_lowercase();
                let publisher = trim_quotes(&publisher);
                if !publisher.is_empty()
                    && !imprint.is_empty()
                    && publisher.chars().count() <= 50
                    && imprint.chars().count() <= 50
                {
                    a.user_imprints.insert(imprint, publisher);
                }
            }
        }
        "SCRAPE_DELAY" => {
            if let Some(n) = as_int(value) {
                a.scrape_delay = n.clamp(2, 3600);
            }
        }
        "MAX_SEARCH_RESULTS" => {
            if let Some(n) = as_int(value) {
                a.max_search_results = n.clamp(10, 5000);
            }
        }
        _ => {}
    }
}

/// The value pattern is `KEY\s*=\s*['\"]?(.+?)['\"]?$` with a
/// case-insensitive key: after the key, optional whitespace, `=`,
/// optional whitespace, one optional leading quote, at least one
/// character, one optional trailing quote.
fn extract_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let head = line.get(..key.len())?;
    if !head.eq_ignore_ascii_case(key) {
        return None;
    }
    let rest = line[key.len()..].trim_start();
    let rest = rest.strip_prefix('=')?;
    let value = rest.trim_start();
    let value = if value.starts_with('"') || value.starts_with('\'') {
        &value[1..]
    } else {
        value
    };
    let value = if value.ends_with('"') || value.ends_with('\'') {
        &value[..value.len() - 1]
    } else {
        value
    };
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn as_int(value: &str) -> Option<i32> {
    value.trim().parse::<f64>().ok().map(|f| f as i32)
}

fn is_true(value: &str) -> bool {
    value.trim().eq_ignore_ascii_case("true")
}

fn trim_quotes(s: &str) -> String {
    s.trim_matches(|c| c == ' ' || c == '\'' || c == '"')
        .to_string()
}

static ARROW: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(\S.*?)\s*[-=]+>\s*(\S.*?)$").unwrap());

/// The `XXXX-->YYYY` / `XXXX=>YYYY` form used by PUBLISHER_ALIAS and
/// IMPRINT lines (C# `re.match` on the value).
fn split_arrow(value: &str) -> Option<(String, String)> {
    let caps = ARROW.captures(value).ok().flatten()?;
    let g1 = caps.get(1)?.as_str().to_string();
    let g2 = caps.get(2)?.as_str().to_string();
    Some((g1, g2))
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advanced_defaults_parse_from_an_empty_string() {
        let a = parse_advanced("");
        assert!(a.ignored_publishers.is_empty());
        assert!(a.ignored_searchterms.is_empty());
        assert!(a.publisher_aliases.is_empty());
        assert!(a.user_imprints.is_empty());
        assert_eq!(a.ignored_before_year, 0);
        assert_eq!(a.ignored_after_year, 9_999_999);
        assert_eq!(a.never_ignore_threshold, 9_999_999);
        assert!(!a.update_rating);
        assert!(a.show_covers);
        assert!(a.welcome_dialog);
        assert_eq!(a.alt_search_regex, "");
        assert!(!a.ignore_folders);
        assert!(!a.force_series_art);
        assert!(!a.note_scrape_date);
        assert_eq!(a.scrape_delay, 1);
        assert_eq!(a.max_search_results, 100);
    }

    #[test]
    fn publishers_accumulate_and_searchterms_are_alnum_only() {
        let a = parse_advanced(
            "IGNORE_PUBLISHER=Marvel Italia\n\
             ignore_publisher = \"Panini Comics\"\n\
             IGNORE_SEARCHTERM=c2c\n\
             IGNORE_SEARCHTERM=noads\n\
             IGNORE_SEARCHTERM=two words\n\
             IGNORE_SEARCHTERM=\n",
        );
        assert_eq!(
            a.ignored_publishers,
            BTreeSet::from(["marvel italia".into(), "panini comics".into()])
        );
        // only alphanumeric terms are allowed; "two words" and "" are not
        assert_eq!(
            a.ignored_searchterms,
            BTreeSet::from(["c2c".into(), "noads".into()])
        );
    }

    #[test]
    fn years_parse_as_int_of_float() {
        let a = parse_advanced(
            "IGNORE_BEFORE_YEAR=2006.9\n\
             IGNORE_AFTER_YEAR='1995'\n\
             NEVER_IGNORE_THRESHOLD=500",
        );
        assert_eq!(a.ignored_before_year, 2006);
        assert_eq!(a.ignored_after_year, 1995);
        assert_eq!(a.never_ignore_threshold, 500);
    }

    #[test]
    fn boolean_keys_only_accept_true() {
        let a = parse_advanced(
            "SCRAPE_RATING=True\n\
             SHOW_COVERS= true\n\
             WELCOME_DIALOG=yes\n\
             IGNORE_FOLDERS=\"TRUE\"\n",
        );
        assert!(a.update_rating);
        assert!(a.show_covers);
        assert!(!a.welcome_dialog);
        assert!(a.ignore_folders);
    }

    #[test]
    fn alt_search_regex_keeps_only_parsable_patterns() {
        let a = parse_advanced(
            "ALT_SEARCH_REGEX=(?P<series>.+?) v(?P<year>\\d+)\n\
             ALT_SEARCH_REGEX=[unclosed\n",
        );
        assert_eq!(a.alt_search_regex, "(?P<series>.+?) v(?P<year>\\d+)");
    }

    #[test]
    fn publisher_aliases_and_imprints_parse() {
        let a = parse_advanced(
            "PUBLISHER_ALIAS=Marvel Italia-->Marvel\n\
             PUBLISHER_ALIAS = \"Panini\" => Panini Comics\n\
             IMPRINT=Vertigo-->DC Comics\n\
             IMPRINT = Marvel Knights => Marvel\n",
        );
        assert_eq!(
            a.publisher_aliases.get("marvel italia"),
            Some(&"Marvel".to_string())
        );
        assert_eq!(
            a.publisher_aliases.get("panini"),
            Some(&"Panini Comics".to_string())
        );
        assert_eq!(
            a.user_imprints.get("vertigo"),
            Some(&"DC Comics".to_string())
        );
        assert_eq!(
            a.user_imprints.get("marvel knights"),
            Some(&"Marvel".to_string())
        );
    }

    #[test]
    fn alias_and_imprint_length_caps_apply() {
        let long: String = "x".repeat(51);
        let a = parse_advanced(&format!(
            "PUBLISHER_ALIAS=A-->{long}\n\
             IMPRINT=Vertigo-->{long}\n"
        ));
        assert!(a.publisher_aliases.is_empty());
        assert!(a.user_imprints.is_empty());
    }

    #[test]
    fn scrape_delay_and_max_results_clamp() {
        let a = parse_advanced("SCRAPE_DELAY=1\nMAX_SEARCH_RESULTS=5\n");
        assert_eq!(a.scrape_delay, 2);
        assert_eq!(a.max_search_results, 10);
        let a = parse_advanced("SCRAPE_DELAY=5000\nMAX_SEARCH_RESULTS=99999\n");
        assert_eq!(a.scrape_delay, 3600);
        assert_eq!(a.max_search_results, 5000);
    }

    #[test]
    fn quote_stripping_matches_the_python_pattern() {
        let a = parse_advanced(
            "IGNORE_PUBLISHER=\"Marvel\"\n\
             IGNORE_PUBLISHER='Panini'\n\
             IGNORE_PUBLISHER=Apostrophe's\n\
             IGNORE_PUBLISHER=\n\
             IGNORE_PUBLISHER=\"\n",
        );
        // one leading and one trailing quote are stripped independently
        assert_eq!(
            a.ignored_publishers,
            BTreeSet::from(["marvel".into(), "panini".into(), "apostrophe's".into(),])
        );
    }

    #[test]
    fn set_advanced_settings_strips_the_outer_whitespace() {
        let mut config = Configuration::default();
        config.set_advanced_settings("  \n IGNORE_PUBLISHER=Marvel \n ");
        assert_eq!(config.advanced_settings, "IGNORE_PUBLISHER=Marvel");
        assert_eq!(config.advanced.ignored_publishers.len(), 1);
    }

    #[test]
    fn json_round_trip_keeps_the_csharp_key_names() {
        let mut config = Configuration {
            api_key: "abc123".into(),
            ignore_blanks: true,
            update_writer: false,
            rescrape_notes: false,
            ..Default::default()
        };
        config.set_advanced_settings("IGNORE_PUBLISHER=Marvel\nSCRAPE_DELAY=3");
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains(r#""apiKey":"abc123""#));
        assert!(json.contains(r#""updateNotes":false"#));
        assert!(json.contains(r#""updateWriter":false"#));
        // serde skips `advanced` — the parsed settings are rebuilt
        // from advanced_settings (the load() path does this)
        let mut back: Configuration = serde_json::from_str(&json).unwrap();
        assert_eq!(back.advanced(), &AdvancedSettings::with_defaults());
        let advanced = back.advanced_settings.clone();
        back.set_advanced_settings(&advanced);
        assert_eq!(config, back);
    }

    #[test]
    fn plugin_section_round_trip() {
        // The section storage rides the unified config accessors
        // (ADR-033); a missing section means the defaults.
        let mut config = Configuration {
            api_key: "key".into(),
            ignore_blanks: true,
            ..Default::default()
        };
        config.set_advanced_settings("IGNORE_PUBLISHER=Marvel\nSCRAPE_DELAY=3");
        cr_core::settings::unified::set_plugin("comic-vine-scraper", &config);
        let mut loaded =
            cr_core::settings::unified::get_plugin::<Configuration>("comic-vine-scraper").unwrap();
        // `advanced` is skipped by serde — the caller reparses from
        // advanced_settings (the old load(dir) shape).
        let raw = loaded.advanced_settings.clone();
        loaded.set_advanced_settings(&raw);
        assert_eq!(config, loaded);
        assert!(
            cr_core::settings::unified::get_plugin::<Configuration>("no-such-plugin").is_none()
        );
    }

    #[test]
    fn config_dir_resolves_the_xdg_roots() {
        let dir = config_dir_from(
            Some(PathBuf::from("/custom/cfg")),
            Some(PathBuf::from("/home/u")),
        );
        assert_eq!(
            dir,
            PathBuf::from("/custom/cfg/comicrust/plugins/comic-vine-scraper")
        );
        let dir = config_dir_from(None, Some(PathBuf::from("/home/u")));
        assert_eq!(
            dir,
            PathBuf::from("/home/u/.config/comicrust/plugins/comic-vine-scraper")
        );
        let dir = config_dir_from(Some(PathBuf::from("")), None);
        assert_eq!(
            dir,
            PathBuf::from("./.config/comicrust/plugins/comic-vine-scraper")
        );
    }
}
