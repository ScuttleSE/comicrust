//! The `Settings.CurrentWorkspace` port — the workspace snapshot
//! (`ComicRack/Config/DisplayWorkspace.cs` + the `DatabaseView`
//! `ComicExplorerViewSettings` + the `ItemViewConfig` child).
//!
//! The port persists ONE implicit workspace (workspace-lite, ADR-026:
//! no named presets, no workspace UI). T14 scope: the browser view
//! state (sidebar visibility + split, view mode, sort/group, Detail
//! columns, item sizes), the window bounds, the reader page layout
//! (the C# `LandscapeLayout` `BookPageLayout` — the port has one
//! layout family), and the T12 display family (the `DisplayWorkspace`
//! display fields).
//!
//! Shape parity: the serde names are the C# members, so the values
//! stay traceable. `DisplayWorkspace.PanelSize` is not persisted —
//! the Fill dock has no panel size; `FileView`, `PagesViewConfig`,
//! `ComicBookDialogPagesConfig` and the `ScriptOutputBounds` family
//! wait on their tasks. The reader skips unknown TOML keys like the
//! C# `XmlSerializer` skips unknown elements. The reader layout is
//! stored as one flat table (the C# nests it under `FormBounds` /
//! `DatabaseView` / `LandscapeLayout` — the TOML shape is a port
//! store, the VALUES stay the C# member names).

use crate::model::enums::ImageRotation;
use crate::model::enums::ItemViewMode;
use crate::model::enums::SortOrder;

/// One persisted Detail column (`ItemViewColumnInfo`): the id, the
/// visibility, and the width.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct ColumnState {
    pub id: i32,
    pub visible: bool,
    pub width: i32,
}

impl Default for ColumnState {
    fn default() -> Self {
        ColumnState {
            id: 0,
            visible: true,
            width: 80,
        }
    }
}

/// `ComicExplorerViewSettings` (the browser list setup) + the
/// `ItemViewConfig` child.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct BrowserViewState {
    /// `ShowBrowser` (the sidebar visibility).
    pub show_browser: bool,
    /// `BrowserSplit` (the sidebar width, the Paned position).
    pub browser_split: i32,
    /// `ItemViewConfig.ItemViewMode`.
    pub mode: ItemViewMode,
    /// `ItemViewConfig.Grouping`.
    pub grouping: bool,
    /// `ItemViewConfig.SortKey` (None = Not Sorted).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_key: Option<String>,
    /// `ItemViewConfig.GrouperId`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grouper: Option<String>,
    /// `ItemViewConfig.ItemSortOrder`.
    pub sort_order: SortOrder,
    /// `ItemViewConfig.ThumbnailSize.Height`.
    pub thumb_height: i32,
    /// `ItemViewConfig.TileSize.Height`.
    pub tile_height: i32,
    /// `ItemViewConfig.ItemRowHeight` — 0 = unset (the runtime font
    /// default applies; the C# apply guard is `>= 8`).
    pub row_height: i32,
    /// `ItemViewConfig.Columns`.
    pub columns: Vec<ColumnState>,
}

impl Default for BrowserViewState {
    fn default() -> Self {
        BrowserViewState {
            show_browser: true,
            browser_split: 280,
            mode: ItemViewMode::Thumbnail,
            grouping: false,
            sort_key: None,
            grouper: None,
            sort_order: SortOrder::Ascending,
            thumb_height: 256,
            tile_height: 128,
            row_height: 0,
            columns: Vec::new(),
        }
    }
}

/// `BookPageLayout` (the C# `LandscapeLayout` family — the port keeps
/// one layout family, the enum names are the C# members as strings).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct ReaderLayoutState {
    /// `PageDisplayMode` (an `ImageFitMode` member name).
    pub fit: String,
    /// `PageLayout` (a `PageLayoutMode` member name).
    pub layout: String,
    /// `PageImageRotation`.
    pub rotation: ImageRotation,
    /// `PageZoom` (1..8).
    #[serde(with = "crate::settings::unified::f32_shortest")]
    pub zoom: f32,
    /// `DisplayWorkspace.RightToLeftReading`.
    pub rtl: bool,
}

impl Default for ReaderLayoutState {
    fn default() -> Self {
        ReaderLayoutState {
            fit: "FitWidth".to_string(),
            layout: "Single".to_string(),
            rotation: ImageRotation::None,
            zoom: 1.0,
            rtl: false,
        }
    }
}

/// The `DisplayWorkspace` display family (the T12 dialog fields; the
/// cr-ui-only enums stay member-name strings — cr-core cannot see the
/// cr-ui types).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct DisplayState {
    /// `PageTransitionEffect`.
    pub transition: String,
    /// `DrawRealisticPages`.
    pub realistic_pages: bool,
    /// `PageMargin` + `PageMarginPercentWidth`.
    pub page_margin: bool,
    #[serde(with = "crate::settings::unified::f32_shortest")]
    pub page_margin_percent: f32,
    /// `PageImageBackgroundMode`.
    pub background_mode: String,
    /// `BackgroundColor` (the port stores the picked color as
    /// `#rrggbb`; `None` = the ADR-025 theme-following surround —
    /// the C# stores a color name, the port never hands this file to
    /// the C# app).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background_color: Option<String>,
    /// `BackgroundTexture` (None omits the key).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background_texture: Option<String>,
    /// `BackgroundImageLayout`.
    pub background_layout: String,
    /// `PaperTexture` (None omits the key).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub paper_texture: Option<String>,
    /// `PaperTextureStrength`.
    #[serde(with = "crate::settings::unified::f32_shortest")]
    pub paper_strength: f32,
    /// `PaperTextureLayout`.
    pub paper_layout: String,
}

impl Default for DisplayState {
    fn default() -> Self {
        DisplayState {
            transition: "Fade".to_string(),
            realistic_pages: true,
            page_margin: false,
            page_margin_percent: 0.05,
            background_mode: "Color".to_string(),
            background_color: None,
            background_texture: None,
            background_layout: "Tile".to_string(),
            paper_texture: None,
            paper_strength: 1.0,
            paper_layout: "Tile".to_string(),
        }
    }
}

/// The persisted workspace (`DisplayWorkspace`, the T14 slice).
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "PascalCase")]
pub struct WorkspaceState {
    /// `FormBounds.Width` + `.Height` (the X/Y pair was written 0 —
    /// Wayland forbids client positioning; only the size restores).
    pub width: i32,
    pub height: i32,
    /// `FormState` (`Normal` / `Maximized`).
    pub maximized: bool,
    pub view: BrowserViewState,
    pub reader: ReaderLayoutState,
    pub display: DisplayState,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    fn round_trip(x: &WorkspaceState) -> WorkspaceState {
        // The workspace rides the Settings serialize path.
        let s = Settings {
            current_workspace: Some(x.clone()),
            ..Settings::default()
        };
        let v = crate::settings::unified::settings_to_value(&s);
        crate::settings::unified::settings_from_value(&v)
            .unwrap()
            .current_workspace
            .unwrap()
    }

    #[test]
    fn default_round_trips() {
        let x = WorkspaceState::default();
        assert_eq!(round_trip(&x), x);
    }

    #[test]
    fn populated_round_trips() {
        let x = WorkspaceState {
            width: 1440,
            height: 900,
            maximized: true,
            view: BrowserViewState {
                show_browser: false,
                browser_split: 312,
                mode: ItemViewMode::Detail,
                grouping: true,
                sort_key: Some("ShadowSeries".to_string()),
                grouper: Some("series".to_string()),
                sort_order: SortOrder::Descending,
                thumb_height: 320,
                tile_height: 200,
                row_height: 28,
                columns: vec![
                    ColumnState {
                        id: 3,
                        visible: false,
                        width: 120,
                    },
                    ColumnState {
                        id: 7,
                        visible: true,
                        width: 90,
                    },
                ],
            },
            reader: ReaderLayoutState {
                fit: "BestFit".to_string(),
                layout: "Continuous".to_string(),
                rotation: ImageRotation::Rotate90,
                zoom: 1.5,
                rtl: true,
            },
            display: DisplayState {
                transition: "LeftRight".to_string(),
                realistic_pages: false,
                page_margin: true,
                page_margin_percent: 0.12,
                background_mode: "Texture".to_string(),
                background_color: Some("#204060".to_string()),
                background_texture: Some("/tmp/tex.png".to_string()),
                background_layout: "Stretch".to_string(),
                paper_texture: Some("Paper1.jpg".to_string()),
                paper_strength: 0.4,
                paper_layout: "Zoom".to_string(),
            },
        };
        assert_eq!(round_trip(&x), x);
    }

    #[test]
    fn shape_matches_the_csharp_members() {
        let mut x = WorkspaceState::default();
        x.view.sort_key = Some("ShadowSeries".to_string());
        x.view.columns.push(ColumnState {
            id: 3,
            visible: true,
            width: 80,
        });
        let text =
            toml::to_string_pretty(&crate::settings::unified::settings_to_value(&Settings {
                current_workspace: Some(x),
                ..Settings::default()
            }))
            .unwrap();
        // The explorer scalars ride the nested view table.
        assert!(text.contains("ShowBrowser"), "{text}");
        assert!(text.contains("BrowserSplit = 280"), "{text}");
        // The list setup carries the C# member names; a None
        // GrouperId omits.
        assert!(text.contains("Mode = \"Thumbnail\""), "{text}");
        assert!(text.contains("SortKey = \"ShadowSeries\""), "{text}");
        assert!(text.contains("SortOrder = \"Ascending\""), "{text}");
        assert!(!text.contains("Grouper"), "{text}");
        // The columns carry the ItemViewColumnInfo members.
        assert!(text.contains("Id = 3"), "{text}");
        assert!(text.contains("Visible = true"), "{text}");
        assert!(text.contains("Width = 80"), "{text}");
        assert!(!text.contains("PaperTexture"), "{text}");
        // A None texture omits the key (the C# null-string rule).
        assert!(!text.contains("PaperTexture"));
    }
}
