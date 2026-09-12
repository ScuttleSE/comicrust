//! Per-list view settings: the bridge between the live `ItemView`
//! readouts and the `<Item>/<Display>/<View>` subtree of ComicDb.xml.
//!
//! The C# applies a list's own `Display.View` when the list becomes
//! current (`ComicBrowserControl.RegisterBookList`,
//! `ComicBrowserControl.cs:1926-1954`) and writes the live config
//! back to the outgoing list when it leaves
//! (`ComicBrowserControl.UpdateViewConfig`, `:3355-3390`). The port
//! kept the serde for that subtree from the start but never applied
//! it — the "T14 per-list sort deviation". This module closes it.
//!
//! `Display.View == None` means "this list has no settings of its
//! own". Per ADR-039 the browser then keeps whatever it shows, the
//! C# `RegisterBookList` behavior: the C# applies nothing when the
//! config is null.

use cr_core::database::display_config::{ItemViewColumnInfo, ItemViewConfig, Size};

use super::item_view::ItemView;
use crate::workspace::{mode_from_xml, mode_to_xml, sort_descending, sort_order};

/// Reads the live browser state into a `<View>` payload (the C#
/// `UpdateViewConfig`).
///
/// `LastTimeVisible` and `FormatId` keep their defaults: the port has
/// no per-column format picker and no column-visibility history.
pub fn collect(item_view: &ItemView) -> ItemViewConfig {
    let (sort_key, descending, grouper) = item_view.sort_group_summary();
    ItemViewConfig {
        columns: item_view
            .detail_columns_state()
            .into_iter()
            .map(|(id, visible, width)| ItemViewColumnInfo {
                id,
                visible,
                width,
                ..ItemViewColumnInfo::default()
            })
            .collect(),
        item_view_mode: mode_to_xml(item_view.mode()),
        // The port encodes "grouping is on" as a present grouper key
        // (the C# carries the flag and the id apart).
        grouping: grouper.is_some(),
        sort_key,
        grouper_id: grouper.map(|g| g.to_string()),
        stacker_id: None,
        item_sort_order: sort_order(descending),
        group_sort_order: sort_order(false),
        groups_status: None,
        thumbnail_size: Size {
            width: 0,
            height: item_view.thumb_height().round() as i32,
        },
        tile_size: Size {
            width: (item_view.tile_height() * 2.0).round() as i32,
            height: item_view.tile_height().round() as i32,
        },
        item_row_height: item_view.row_height().round() as i32,
    }
}

/// Writes a `<View>` payload into the live browser (the C#
/// `RegisterBookList`). Mirrors `ShellState::apply_workspace` field
/// for field, including the C# `ItemRowHeight >= 8` guard.
pub fn apply(item_view: &ItemView, cfg: &ItemViewConfig) {
    // Mode first: the per-mode item-size clamps depend on it.
    item_view.configure(|c| c.mode = mode_from_xml(cfg.item_view_mode));
    item_view.configure(|c| {
        if cfg.thumbnail_size.height > 0 {
            c.thumb_height = f64::from(cfg.thumbnail_size.height);
        }
        if cfg.tile_size.height > 0 {
            c.tile_size = (
                f64::from(cfg.tile_size.height * 2),
                f64::from(cfg.tile_size.height),
            );
        }
        // The C# guard (`value.ItemRowHeight >= 8`): an unset height
        // keeps the boot default (font height + 6).
        if cfg.item_row_height >= 8 {
            c.row_height = f64::from(cfg.item_row_height);
        }
    });
    match &cfg.sort_key {
        Some(key) => item_view.set_sort_column(key),
        None => item_view.clear_sort(),
    }
    item_view.set_sort_direction(sort_descending(cfg.item_sort_order));
    // The registry owns the 'static grouper keys — a stored key only
    // applies while it still exists.
    let grouper = cfg.grouper_id.as_ref().and_then(|g| {
        cr_engine::group::groupers()
            .iter()
            .find(|(k, _)| k == &g.as_str())
            .map(|(k, _)| *k)
    });
    item_view.set_grouper(grouper);
    let cols: Vec<(i32, bool, i32)> = cfg
        .columns
        .iter()
        .map(|c| (c.id, c.visible, c.width))
        .collect();
    item_view.set_detail_columns_state(&cols);
}

#[cfg(test)]
mod tests {
    use super::*;
    use cr_core::model::enums::{ItemViewMode, SortOrder};

    /// A `<View>` the port can produce: every field inside the range
    /// `collect` emits.
    fn sample() -> ItemViewConfig {
        ItemViewConfig {
            columns: vec![
                ItemViewColumnInfo {
                    id: 1,
                    visible: true,
                    width: 240,
                    ..ItemViewColumnInfo::default()
                },
                ItemViewColumnInfo {
                    id: 5,
                    visible: false,
                    width: 60,
                    ..ItemViewColumnInfo::default()
                },
            ],
            item_view_mode: ItemViewMode::Tile,
            grouping: true,
            sort_key: Some("ShadowSeries".to_string()),
            grouper_id: Some("series".to_string()),
            stacker_id: None,
            item_sort_order: SortOrder::Descending,
            group_sort_order: SortOrder::Ascending,
            groups_status: None,
            thumbnail_size: Size {
                width: 0,
                height: 320,
            },
            tile_size: Size {
                width: 400,
                height: 200,
            },
            item_row_height: 28,
        }
    }

    /// The `<View>` subtree must survive an XML round trip unchanged:
    /// a per-list config the port writes has to read back the same,
    /// or a list's settings drift on every save.
    #[test]
    fn sample_round_trips_through_xml() {
        let cfg = sample();
        let mut out = Vec::new();
        {
            let mut e = cr_core::xml::Emitter::new(&mut out).unwrap();
            cfg.write_xml(&mut e).unwrap();
        }
        let text = String::from_utf8(out).unwrap();
        let parsed = parse_view(&text);
        assert_eq!(parsed, cfg, "round trip changed the config: {text}");
    }

    /// `grouper_id` names a key the registry does not hold: the apply
    /// path must drop it, not panic. (The pure half of that rule —
    /// the apply itself needs a GTK ItemView, so the probe covers it.)
    #[test]
    fn unknown_grouper_key_is_not_in_the_registry() {
        let known = cr_engine::group::groupers()
            .iter()
            .any(|(k, _)| *k == "no-such-grouper");
        assert!(!known);
    }

    fn parse_view(text: &str) -> ItemViewConfig {
        let mut bytes = text.as_bytes();
        let mut r = cr_core::xml::XmlReader::new(&mut bytes);
        loop {
            match r.next_tok().unwrap() {
                cr_core::xml::Tok::Start(s) if s.name == "View" => {
                    return ItemViewConfig::from_start(&s, &mut r).unwrap();
                }
                cr_core::xml::Tok::Eof => panic!("no <View> in {text}"),
                _ => {}
            }
        }
    }
}
