//! Configuration and path-role checks for incoming folders.

/// The `[plugins.incoming]` table in the unified configuration.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IncomingConfig {
    #[serde(default)]
    pub incoming_folders: Vec<String>,
    #[serde(default)]
    pub last_organizer_profile: String,
}

impl IncomingConfig {
    /// Returns true when `path` is below a configured incoming folder.
    pub fn is_incoming_path(&self, path: &str) -> bool {
        self.incoming_folders
            .iter()
            .any(|folder| crate::duplicates::under_path(path, folder))
    }

    /// Incoming folders are always monitored folders.
    pub fn is_monitored_path(&self, path: &str) -> bool {
        self.is_incoming_path(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_empty() {
        assert_eq!(
            IncomingConfig::default(),
            IncomingConfig {
                incoming_folders: Vec::new(),
                last_organizer_profile: String::new(),
            }
        );
    }

    #[test]
    fn serde_uses_the_plugin_field_names() {
        let config: IncomingConfig = toml::from_str(
            r#"
incoming_folders = ["/data/Incoming"]
last_organizer_profile = "Move to Library"
"#,
        )
        .expect("incoming config parses");
        assert_eq!(config.incoming_folders, ["/data/Incoming"]);
        assert_eq!(config.last_organizer_profile, "Move to Library");

        let text = toml::to_string(&config).expect("incoming config serializes");
        assert!(text.contains("incoming_folders = [\"/data/Incoming\"]"));
        assert!(text.contains("last_organizer_profile = \"Move to Library\""));
    }

    #[test]
    fn incoming_match_is_component_aware_and_separator_neutral() {
        let config = IncomingConfig {
            incoming_folders: vec!["C:\\Comics\\Incoming\\".into()],
            ..Default::default()
        };

        assert!(config.is_incoming_path("c:/comics/incoming/Series/book.cbz"));
        assert!(!config.is_incoming_path("c:/comics/incoming-old/book.cbz"));
        assert!(!config.is_incoming_path("c:/comics/incoming"));
    }

    #[test]
    fn any_incoming_role_also_has_the_monitored_role() {
        let config = IncomingConfig {
            incoming_folders: vec!["/staging/incoming".into()],
            ..Default::default()
        };

        let path = "/staging/incoming/series/book.cbz";
        assert!(config.is_incoming_path(path));
        assert!(config.is_monitored_path(path));
        assert!(!config.is_monitored_path("/library/series/book.cbz"));
    }
}
