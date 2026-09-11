//! The known imprints and their parent publishers — the port of the
//! plugin's `cvimprints.py`. The table lives in the unified config
//! file (`[data.imprints]`, ADR-033): seeded on first boot from
//! `cr_core::settings::unified::IMPRINTS`, user-editable without a
//! recompile, and the built-in seed applies when the session has no
//! table (headless tests, uninitialized session). Keys and values
//! must match the ComicVine database exactly (case and punctuation
//! included, so the seed is verbatim).
//!
//! The advanced-settings `IMPRINT=` entries still override on top
//! (the `__update_publishers` chain in `bookdata.rs`).

/// `find_parent_publisher`: the parent publisher for a known imprint,
/// or the original string when unknown.
pub fn find_parent_publisher(imprint: &str) -> String {
    let imprint = imprint.trim();
    cr_core::settings::unified::data_table("imprints")
        .get(imprint)
        .cloned()
        .unwrap_or_else(|| imprint.to_string())
}
