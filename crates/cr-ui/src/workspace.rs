//! The T14 workspace persistence: the conversions between the
//! cr-core `WorkspaceState` (the `<CurrentWorkspace>` element in
//! the unified config) and the cr-ui widgets. The collect/apply lives in the
//! shell (`browser/shell.rs`); this module holds the pure
//! enum-name/DisplayOptions mapping plus the unit tests.

use crate::browser::layout::ItemViewMode;
use crate::reader::display::ImageFitMode;
use crate::reader::page_view::{
    DisplayOptions, ImageBackgroundMode, ImageLayout, PageLayoutMode, PageTransitionEffect,
};
use cr_core::model::enums::SortOrder;
use cr_core::settings::workspace::{BrowserViewState, DisplayState};

// ----- enum name helpers (the C# member names, the XmlSerializer
// shape; cr-core stores them as strings because the enums live in
// cr-ui) -----

pub fn fit_name(mode: ImageFitMode) -> &'static str {
    match mode {
        ImageFitMode::Original => "Original",
        ImageFitMode::Fit => "Fit",
        ImageFitMode::FitWidth => "FitWidth",
        ImageFitMode::FitWidthAdaptive => "FitWidthAdaptive",
        ImageFitMode::FitHeight => "FitHeight",
        ImageFitMode::BestFit => "BestFit",
    }
}

pub fn fit_from_name(s: &str) -> ImageFitMode {
    match s {
        "Original" => ImageFitMode::Original,
        "Fit" => ImageFitMode::Fit,
        "FitWidthAdaptive" => ImageFitMode::FitWidthAdaptive,
        "FitHeight" => ImageFitMode::FitHeight,
        "BestFit" => ImageFitMode::BestFit,
        _ => ImageFitMode::FitWidth,
    }
}

pub fn layout_name(mode: PageLayoutMode) -> &'static str {
    match mode {
        PageLayoutMode::Single => "Single",
        PageLayoutMode::Double => "Double",
        PageLayoutMode::DoubleAdaptive => "DoubleAdaptive",
        PageLayoutMode::Continuous => "Continuous",
    }
}

pub fn layout_from_name(s: &str) -> PageLayoutMode {
    match s {
        "Double" => PageLayoutMode::Double,
        "DoubleAdaptive" => PageLayoutMode::DoubleAdaptive,
        "Continuous" => PageLayoutMode::Continuous,
        _ => PageLayoutMode::Single,
    }
}

fn transition_name(effect: PageTransitionEffect) -> &'static str {
    match effect {
        PageTransitionEffect::None => "None",
        PageTransitionEffect::Fade => "Fade",
        PageTransitionEffect::LeftRight => "LeftRight",
        PageTransitionEffect::TopDown => "TopDown",
        PageTransitionEffect::Paging => "Paging",
    }
}

fn transition_from_name(s: &str) -> PageTransitionEffect {
    match s {
        "None" => PageTransitionEffect::None,
        "LeftRight" => PageTransitionEffect::LeftRight,
        "TopDown" => PageTransitionEffect::TopDown,
        "Paging" => PageTransitionEffect::Paging,
        _ => PageTransitionEffect::Fade,
    }
}

fn background_mode_name(mode: ImageBackgroundMode) -> &'static str {
    match mode {
        ImageBackgroundMode::Auto => "Auto",
        ImageBackgroundMode::Color => "Color",
        ImageBackgroundMode::Texture => "Texture",
    }
}

fn background_mode_from_name(s: &str) -> ImageBackgroundMode {
    match s {
        "Auto" => ImageBackgroundMode::Auto,
        "Texture" => ImageBackgroundMode::Texture,
        _ => ImageBackgroundMode::Color,
    }
}

fn image_layout_name(layout: ImageLayout) -> &'static str {
    match layout {
        ImageLayout::None => "None",
        ImageLayout::Tile => "Tile",
        ImageLayout::Center => "Center",
        ImageLayout::Stretch => "Stretch",
        ImageLayout::Zoom => "Zoom",
    }
}

fn image_layout_from_name(s: &str) -> ImageLayout {
    match s {
        "None" => ImageLayout::None,
        "Center" => ImageLayout::Center,
        "Stretch" => ImageLayout::Stretch,
        "Zoom" => ImageLayout::Zoom,
        _ => ImageLayout::Tile,
    }
}

/// The picked surround color as `#rrggbb` (the C# stores a color
/// name; the port format is documented on `DisplayState`).
fn color_to_hex(rgb: [f32; 3]) -> String {
    let ch = |v: f32| -> String { format!("{:02x}", (v.clamp(0.0, 1.0) * 255.0).round() as u8) };
    format!("#{}{}{}", ch(rgb[0]), ch(rgb[1]), ch(rgb[2]))
}

fn color_from_hex(s: &str) -> Option<[f32; 3]> {
    let hex = s.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    Some([
        byte(0)? as f32 / 255.0,
        byte(2)? as f32 / 255.0,
        byte(4)? as f32 / 255.0,
    ])
}

// ----- DisplayOptions <-> DisplayState -----

pub fn display_to_state(opts: &DisplayOptions) -> DisplayState {
    DisplayState {
        transition: transition_name(opts.transition).to_string(),
        realistic_pages: opts.realistic_pages,
        page_margin: opts.page_margin,
        page_margin_percent: opts.page_margin_percent,
        background_mode: background_mode_name(opts.background_mode).to_string(),
        background_color: opts.background_color.map(color_to_hex),
        background_texture: opts.background_texture.clone(),
        background_layout: image_layout_name(opts.background_layout).to_string(),
        paper_texture: opts.paper_texture.clone(),
        paper_strength: opts.paper_strength,
        paper_layout: image_layout_name(opts.paper_layout).to_string(),
    }
}

pub fn display_from_state(st: &DisplayState) -> DisplayOptions {
    DisplayOptions {
        transition: transition_from_name(&st.transition),
        realistic_pages: st.realistic_pages,
        page_margin: st.page_margin,
        page_margin_percent: st.page_margin_percent.clamp(0.0, 0.5),
        background_mode: background_mode_from_name(&st.background_mode),
        background_color: st.background_color.as_deref().and_then(color_from_hex),
        background_texture: st.background_texture.clone(),
        background_layout: image_layout_from_name(&st.background_layout),
        paper_texture: st.paper_texture.clone(),
        paper_strength: st.paper_strength.clamp(0.0, 1.0),
        paper_layout: image_layout_from_name(&st.paper_layout),
    }
}

// ----- the browser view state helpers (the shell calls these with
// the ItemView readouts) -----

pub fn mode_to_xml(mode: ItemViewMode) -> cr_core::model::enums::ItemViewMode {
    match mode {
        ItemViewMode::Thumbnail => cr_core::model::enums::ItemViewMode::Thumbnail,
        ItemViewMode::Tile => cr_core::model::enums::ItemViewMode::Tile,
        ItemViewMode::Detail => cr_core::model::enums::ItemViewMode::Detail,
    }
}

pub fn mode_from_xml(mode: cr_core::model::enums::ItemViewMode) -> ItemViewMode {
    match mode {
        cr_core::model::enums::ItemViewMode::Tile => ItemViewMode::Tile,
        cr_core::model::enums::ItemViewMode::Detail => ItemViewMode::Detail,
        _ => ItemViewMode::Thumbnail,
    }
}

pub fn sort_order(descending: bool) -> SortOrder {
    if descending {
        SortOrder::Descending
    } else {
        SortOrder::Ascending
    }
}

pub fn sort_descending(order: SortOrder) -> bool {
    order == SortOrder::Descending
}

/// The ItemView readouts the shell feeds the persistence (one
/// struct, so the pure mapping stays unit-testable).
pub struct BrowserReadouts {
    pub mode: ItemViewMode,
    pub sort_key: Option<String>,
    pub descending: bool,
    pub grouper: Option<&'static str>,
    pub thumb_height: f64,
    pub tile_height: f64,
    pub row_height: f64,
    pub columns: Vec<(i32, bool, i32)>,
}

/// Builds the persisted browser view state from the ItemView
/// readouts (the shell's collect path; a pure function so the shape
/// is unit-testable).
pub fn browser_view_state(
    show_browser: bool,
    browser_split: i32,
    readouts: BrowserReadouts,
) -> BrowserViewState {
    BrowserViewState {
        show_browser,
        browser_split,
        mode: mode_to_xml(readouts.mode),
        grouping: readouts.grouper.is_some(),
        sort_key: readouts.sort_key,
        grouper: readouts.grouper.map(|g| g.to_string()),
        sort_order: sort_order(readouts.descending),
        thumb_height: readouts.thumb_height.round() as i32,
        tile_height: readouts.tile_height.round() as i32,
        row_height: readouts.row_height.round() as i32,
        columns: readouts
            .columns
            .into_iter()
            .map(
                |(id, visible, width)| cr_core::settings::workspace::ColumnState {
                    id,
                    visible,
                    width,
                },
            )
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cr_core::settings::workspace::ColumnState;

    #[test]
    fn display_options_round_trip() {
        let opts = DisplayOptions::default();
        assert_eq!(display_from_state(&display_to_state(&opts)), opts);
        let mut custom = opts;
        custom.transition = PageTransitionEffect::LeftRight;
        custom.realistic_pages = false;
        custom.page_margin = true;
        custom.page_margin_percent = 0.2;
        custom.background_mode = ImageBackgroundMode::Texture;
        // The color quantizes to 8-bit through the hex form — use
        // values that survive the byte round-trip exactly.
        custom.background_color = Some([0.2, 0.0, 0.8]);
        custom.background_texture = Some("/tmp/bg.png".to_string());
        custom.background_layout = ImageLayout::Stretch;
        custom.paper_texture = Some("Paper1.jpg".to_string());
        custom.paper_strength = 0.35;
        custom.paper_layout = ImageLayout::Zoom;
        assert_eq!(display_from_state(&display_to_state(&custom)), custom);
    }

    #[test]
    fn color_hex_round_trips() {
        let hex = color_to_hex([1.0, 0.0, 0.5]);
        assert_eq!(hex, "#ff0080");
        // The decode quantizes back to 8-bit (the stored format).
        assert_eq!(color_from_hex(&hex), Some([1.0, 0.0, 0x80 as f32 / 255.0]));
        assert_eq!(color_from_hex("WhiteSmoke"), None);
    }

    #[test]
    fn browser_view_state_maps_the_item_readouts() {
        let v = browser_view_state(
            false,
            312,
            BrowserReadouts {
                mode: ItemViewMode::Detail,
                sort_key: Some("ShadowSeries".to_string()),
                descending: true,
                grouper: Some("series"),
                thumb_height: 320.4,
                tile_height: 200.6,
                row_height: 28.2,
                columns: vec![(3, false, 120), (7, true, 90)],
            },
        );
        assert_eq!(v.mode, cr_core::model::enums::ItemViewMode::Detail);
        assert_eq!(v.sort_order, SortOrder::Descending);
        assert!(v.grouping);
        assert_eq!(v.grouper.as_deref(), Some("series"));
        assert_eq!(v.thumb_height, 320);
        assert_eq!(v.tile_height, 201);
        assert_eq!(v.row_height, 28);
        assert_eq!(
            v.columns,
            vec![
                ColumnState {
                    id: 3,
                    visible: false,
                    width: 120
                },
                ColumnState {
                    id: 7,
                    visible: true,
                    width: 90
                }
            ]
        );
        // A sort of None + a grouper of None stays Not Sorted / Not
        // Grouped.
        let plain = browser_view_state(
            true,
            280,
            BrowserReadouts {
                mode: ItemViewMode::Thumbnail,
                sort_key: None,
                descending: false,
                grouper: None,
                thumb_height: 256.0,
                tile_height: 128.0,
                row_height: 24.0,
                columns: vec![],
            },
        );
        assert_eq!(plain.sort_key, None);
        assert!(!plain.grouping);
        assert_eq!(plain.grouper, None);
    }

    #[test]
    fn enum_names_are_the_csharp_members() {
        assert_eq!(fit_name(ImageFitMode::BestFit), "BestFit");
        assert_eq!(
            fit_from_name("FitWidthAdaptive"),
            ImageFitMode::FitWidthAdaptive
        );
        assert_eq!(fit_from_name("junk"), ImageFitMode::FitWidth);
        assert_eq!(
            layout_name(PageLayoutMode::DoubleAdaptive),
            "DoubleAdaptive"
        );
        assert_eq!(transition_name(PageTransitionEffect::TopDown), "TopDown");
        assert_eq!(
            background_mode_name(ImageBackgroundMode::Texture),
            "Texture"
        );
        assert_eq!(image_layout_name(ImageLayout::Center), "Center");
        assert_eq!(image_layout_from_name("junk"), ImageLayout::Tile);
    }
}
