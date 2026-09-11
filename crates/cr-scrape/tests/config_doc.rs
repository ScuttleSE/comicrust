//! The scraper section drift gate: every persisted field of the
//! scraper `Configuration` (the `[plugins.comic-vine-scraper]`
//! keys) must appear (backticked) in `docs/config-reference.md`.
use cr_scrape::config::Configuration;

#[test]
fn config_reference_documents_the_scraper_keys() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/config-reference.md"
    ))
    .expect("docs/config-reference.md exists");

    // The serde keys of the default configuration (the file's
    // camelCase names).
    let value = serde_json::to_value(Configuration::default()).expect("serialize");
    let table = value.as_object().expect("object");
    for name in table.keys() {
        assert!(
            text.contains(&format!("`{name}`")),
            "config-reference.md does not document `[plugins.comic-vine-scraper]` key `{name}`"
        );
    }

    // The advancedSettings KEY forms.
    for key in cr_scrape::config::ADVANCED_KEYS {
        assert!(
            text.contains(&format!("`{key}`")),
            "config-reference.md does not document advancedSettings key `{key}`"
        );
    }
}
