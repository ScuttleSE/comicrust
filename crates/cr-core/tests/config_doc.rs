//! The config reference drift gate: every key the app seeds into
//! `comicrust.toml` must appear (backticked) in
//! `docs/config-reference.md`. Adding a registry field without a doc
//! entry fails this test.
use cr_core::settings::engine_config::ENGINE_CONFIG_FIELDS;
use cr_core::settings::extended::EXTENDED_FIELDS;
use cr_core::settings::settings::{Settings, SETTINGS_FIELDS};
use cr_core::settings::unified::{settings_to_value, EXTENDED_SEED_EXCLUDED};

#[test]
fn config_reference_documents_every_seeded_key() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/config-reference.md"
    ))
    .expect("docs/config-reference.md exists");

    let check = |name: &str| {
        assert!(
            text.contains(&format!("`{name}`")),
            "config-reference.md does not document key `{name}`"
        );
    };

    // [extended] + [engine]: every ini:true registry key except the
    // restart plumbing (the seed excludes it).
    for f in EXTENDED_FIELDS
        .iter()
        .filter(|f| f.ini_enabled && !EXTENDED_SEED_EXCLUDED.contains(&f.name))
    {
        check(f.name);
    }
    for f in ENGINE_CONFIG_FIELDS.iter().filter(|f| f.ini_enabled) {
        check(f.name);
    }
    // The engine converter fields ride outside the registry.
    for name in [
        "ListCoverSize",
        "PdfiumImageSize",
        "DjVuSizeLimit",
        "PageBowColor",
        "BlankPageColor",
        "BookmarkColors",
    ] {
        check(name);
    }

    // [settings]: the serde keys of the default settings (the ones
    // the file carries) plus the optional members.
    let value = settings_to_value(&Settings::default());
    let mut names: Vec<String> = value
        .as_table()
        .expect("settings map")
        .keys()
        .cloned()
        .collect();
    names.push("CurrentWorkspace".into());
    names.push("PluginsStates".into());
    names.push("SelectedBrowser".into());
    for name in names {
        check(&name);
    }

    let _ = SETTINGS_FIELDS;
}
