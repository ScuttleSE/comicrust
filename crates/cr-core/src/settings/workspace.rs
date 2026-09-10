//! The `<CurrentWorkspace>` port — `Settings.CurrentWorkspace`
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
//! Shape parity: the element/attribute names are the C# members, so
//! the values stay traceable (`DisplayWorkspace.PanelSize` is not
//! persisted — the Fill dock has no panel size; `FileView`,
//! `PagesViewConfig`, `ComicBookDialogPagesConfig` and the
//! `ScriptOutputBounds` family wait on their tasks). The reader is
//! order-tolerant like the C# `XmlSerializer`; attributes at their
//! default value are omitted.

use crate::model::enums::ImageRotation;
use crate::model::enums::ItemViewMode;
use crate::model::enums::SortOrder;
use crate::xml::{Emitter, Start, Tok, XmlError, XmlReader, XmlResult};
use std::io::Write;

/// One persisted Detail column (`ItemViewColumnInfo`): the id, the
/// visibility, and the width.
#[derive(Clone, Debug, PartialEq)]
pub struct ColumnState {
    pub id: i32,
    pub visible: bool,
    pub width: i32,
}

/// `ComicExplorerViewSettings` (the browser list setup) + the
/// `ItemViewConfig` child.
#[derive(Clone, Debug, PartialEq)]
pub struct BrowserViewState {
    /// `ShowBrowser` (the sidebar visibility).
    pub show_browser: bool,
    /// `BrowserSplit` (the sidebar width, the Paned position).
    pub browser_split: i32,
    /// `ItemViewConfig.ItemViewMode`.
    pub mode: ItemViewMode,
    /// `ItemViewConfig.Grouping`.
    pub grouping: bool,
    /// `ItemViewConfig.SortKey` (None = Not Sorted; the attribute
    /// omits like the C# `[DefaultValue(null)]`).
    pub sort_key: Option<String>,
    /// `ItemViewConfig.GrouperId`.
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
#[derive(Clone, Debug, PartialEq)]
pub struct ReaderLayoutState {
    /// `PageDisplayMode` (an `ImageFitMode` member name).
    pub fit: String,
    /// `PageLayout` (a `PageLayoutMode` member name).
    pub layout: String,
    /// `PageImageRotation`.
    pub rotation: ImageRotation,
    /// `PageZoom` (1..8).
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
#[derive(Clone, Debug, PartialEq)]
pub struct DisplayState {
    /// `PageTransitionEffect`.
    pub transition: String,
    /// `DrawRealisticPages`.
    pub realistic_pages: bool,
    /// `PageMargin` + `PageMarginPercentWidth`.
    pub page_margin: bool,
    pub page_margin_percent: f32,
    /// `PageImageBackgroundMode`.
    pub background_mode: String,
    /// `BackgroundColor` (the port stores the picked color as
    /// `#rrggbb`; `None` = the ADR-025 theme-following surround —
    /// the C# stores a color name, the port never hands this file to
    /// the C# app).
    pub background_color: Option<String>,
    /// `BackgroundTexture` (None omits the element).
    pub background_texture: Option<String>,
    /// `BackgroundImageLayout`.
    pub background_layout: String,
    /// `PaperTexture` (None omits the element).
    pub paper_texture: Option<String>,
    /// `PaperTextureStrength`.
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
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorkspaceState {
    /// `FormBounds.Width` + `.Height` (the X/Y pair is written 0 —
    /// Wayland forbids client positioning; only the size restores).
    pub width: i32,
    pub height: i32,
    /// `FormState` (`Normal` / `Maximized`).
    pub maximized: bool,
    pub view: BrowserViewState,
    pub reader: ReaderLayoutState,
    pub display: DisplayState,
}

fn w_bool<W: Write>(e: &mut Emitter<W>, name: &str, v: bool) -> std::io::Result<()> {
    e.text_elem(name, if v { "true" } else { "false" })
}

fn w_f32<W: Write>(e: &mut Emitter<W>, name: &str, v: f32) -> std::io::Result<()> {
    e.text_elem(name, &crate::xml::scalar::net_f32(v))
}

impl WorkspaceState {
    /// Writes the `<CurrentWorkspace>` element (the C# shape:
    /// `DatabaseView` carries the explorer attributes, the
    /// `ItemViewConfig` child carries the list setup attributes).
    pub fn write_xml<W: Write>(&self, e: &mut Emitter<W>) -> std::io::Result<()> {
        e.start("CurrentWorkspace")?;
        // FormBounds — the C# Rectangle element; X/Y stay 0 (see the
        // struct doc).
        e.start("FormBounds")?;
        e.text_elem("X", "0")?;
        e.text_elem("Y", "0")?;
        e.text_elem("Width", &self.width.to_string())?;
        e.text_elem("Height", &self.height.to_string())?;
        e.end()?;
        // FormState (the C# FormWindowState names; the port keeps
        // only the Normal/Maximized pair).
        e.text_elem(
            "FormState",
            if self.maximized {
                "Maximized"
            } else {
                "Normal"
            },
        )?;
        // DatabaseView (ComicExplorerViewSettings): the scalar
        // members are XmlAttributes.
        e.start("DatabaseView")?;
        e.attr(
            "ShowBrowser",
            if self.view.show_browser {
                "true"
            } else {
                "false"
            },
        )?;
        e.attr("BrowserSplit", &self.view.browser_split.to_string())?;
        // ItemViewConfig: the list setup.
        e.start("ItemViewConfig")?;
        e.attr("ItemViewMode", &self.view.mode.to_xml())?;
        if self.view.grouping {
            e.attr("Grouping", "true")?;
        }
        if let Some(key) = &self.view.sort_key {
            e.attr("SortKey", key)?;
        }
        if let Some(g) = &self.view.grouper {
            e.attr("GrouperId", g)?;
        }
        e.attr("ItemSortOrder", &self.view.sort_order.to_xml())?;
        // Columns first (the C# declaration order), then the sizes.
        e.start("Columns")?;
        for c in &self.view.columns {
            e.start("Column")?;
            e.attr("Id", &c.id.to_string())?;
            e.attr("Visible", if c.visible { "true" } else { "false" })?;
            e.attr("Width", &c.width.to_string())?;
            e.end()?;
        }
        e.end()?;
        e.start("ThumbnailSize")?;
        e.text_elem("Width", &self.view.thumb_height.to_string())?;
        e.text_elem("Height", &self.view.thumb_height.to_string())?;
        e.end()?;
        e.start("TileSize")?;
        e.text_elem("Width", &(self.view.tile_height * 2).to_string())?;
        e.text_elem("Height", &self.view.tile_height.to_string())?;
        e.end()?;
        e.text_elem("ItemRowHeight", &self.view.row_height.to_string())?;
        e.end()?; // ItemViewConfig
        e.end()?; // DatabaseView
                  // LandscapeLayout (BookPageLayout): the members are elements.
        e.start("LandscapeLayout")?;
        e.text_elem("PageDisplayMode", &self.reader.fit)?;
        e.text_elem("PageLayout", &self.reader.layout)?;
        e.text_elem("PageImageRotation", &self.reader.rotation.to_xml())?;
        e.text_elem("PageZoom", &crate::xml::scalar::net_f32(self.reader.zoom))?;
        e.end()?;
        w_bool(e, "RightToLeftReading", self.reader.rtl)?;
        e.text_elem("PageTransitionEffect", &self.display.transition)?;
        w_bool(e, "DrawRealisticPages", self.display.realistic_pages)?;
        w_bool(e, "PageMargin", self.display.page_margin)?;
        w_f32(
            e,
            "PageMarginPercentWidth",
            self.display.page_margin_percent,
        )?;
        e.text_elem("PageImageBackgroundMode", &self.display.background_mode)?;
        if let Some(c) = &self.display.background_color {
            e.text_elem("BackgroundColor", c)?;
        }
        if let Some(t) = &self.display.background_texture {
            e.text_elem("BackgroundTexture", t)?;
        }
        e.text_elem("BackgroundImageLayout", &self.display.background_layout)?;
        if let Some(t) = &self.display.paper_texture {
            e.text_elem("PaperTexture", t)?;
        }
        w_f32(e, "PaperTextureStrength", self.display.paper_strength)?;
        e.text_elem("PaperTextureLayout", &self.display.paper_layout)?;
        e.end()
    }

    /// Parses one `<CurrentWorkspace>` element (already started).
    pub fn parse(r: &mut XmlReader<'_>) -> XmlResult<WorkspaceState> {
        let mut x = WorkspaceState::default();
        loop {
            match r.next_tok()? {
                Tok::Eof => return Err(XmlError("unexpected eof in CurrentWorkspace".into())),
                Tok::End(n) if n == "CurrentWorkspace" => return Ok(x),
                Tok::Start(s) => match s.name.as_str() {
                    "FormBounds" => {
                        let (w, h) = parse_bounds(r)?;
                        x.width = w;
                        x.height = h;
                    }
                    "FormState" => x.maximized = r.text_content("FormState")? == "Maximized",
                    "DatabaseView" => x.view = parse_database_view(r, &s)?,
                    "LandscapeLayout" => x.reader = parse_reader_layout(r)?,
                    "RightToLeftReading" => {
                        x.reader.rtl = r.text_content("RightToLeftReading")? == "true"
                    }
                    "PageTransitionEffect" => {
                        x.display.transition = r.text_content("PageTransitionEffect")?
                    }
                    "DrawRealisticPages" => {
                        x.display.realistic_pages = r.text_content("DrawRealisticPages")? == "true"
                    }
                    "PageMargin" => x.display.page_margin = r.text_content("PageMargin")? == "true",
                    "PageMarginPercentWidth" => {
                        let t = r.text_content("PageMarginPercentWidth")?;
                        x.display.page_margin_percent = t.trim().parse().unwrap_or(0.05);
                    }
                    "PageImageBackgroundMode" => {
                        x.display.background_mode = r.text_content("PageImageBackgroundMode")?
                    }
                    "BackgroundColor" => {
                        x.display.background_color = Some(r.text_content("BackgroundColor")?)
                    }
                    "BackgroundTexture" => {
                        x.display.background_texture = Some(r.text_content("BackgroundTexture")?)
                    }
                    "BackgroundImageLayout" => {
                        x.display.background_layout = r.text_content("BackgroundImageLayout")?
                    }
                    "PaperTexture" => {
                        x.display.paper_texture = Some(r.text_content("PaperTexture")?)
                    }
                    "PaperTextureStrength" => {
                        let t = r.text_content("PaperTextureStrength")?;
                        x.display.paper_strength = t.trim().parse().unwrap_or(1.0);
                    }
                    "PaperTextureLayout" => {
                        x.display.paper_layout = r.text_content("PaperTextureLayout")?
                    }
                    _ => r.skip_element(&s.name)?,
                },
                _ => {}
            }
        }
    }
}

fn parse_bounds(r: &mut XmlReader<'_>) -> XmlResult<(i32, i32)> {
    let (mut w, mut h) = (0, 0);
    loop {
        match r.next_tok()? {
            Tok::End(n) if n == "FormBounds" => return Ok((w, h)),
            Tok::Start(s) => match s.name.as_str() {
                "Width" => w = r.text_content("Width")?.trim().parse().unwrap_or(0),
                "Height" => h = r.text_content("Height")?.trim().parse().unwrap_or(0),
                _ => r.skip_element(&s.name)?,
            },
            Tok::Eof => return Err(XmlError("unexpected eof in FormBounds".into())),
            _ => {}
        }
    }
}

fn parse_database_view(r: &mut XmlReader<'_>, s: &Start) -> XmlResult<BrowserViewState> {
    let mut v = BrowserViewState {
        show_browser: s.attr("ShowBrowser").map(|a| a == "true").unwrap_or(true),
        browser_split: s
            .attr("BrowserSplit")
            .and_then(|a| a.trim().parse().ok())
            .unwrap_or(280),
        ..BrowserViewState::default()
    };
    loop {
        match r.next_tok()? {
            Tok::Eof => return Err(XmlError("unexpected eof in DatabaseView".into())),
            Tok::End(n) if n == "DatabaseView" => return Ok(v),
            Tok::Start(s) => match s.name.as_str() {
                "ItemViewConfig" => parse_item_view_config(r, &s, &mut v)?,
                _ => r.skip_element(&s.name)?,
            },
            _ => {}
        }
    }
}

fn parse_item_view_config(
    r: &mut XmlReader<'_>,
    s: &Start,
    v: &mut BrowserViewState,
) -> XmlResult<()> {
    if let Some(m) = s.attr("ItemViewMode").and_then(ItemViewMode::from_xml) {
        v.mode = m;
    }
    if let Some(g) = s.attr("Grouping") {
        v.grouping = g == "true";
    }
    v.sort_key = s.attr("SortKey").map(|a| a.to_string());
    v.grouper = s.attr("GrouperId").map(|a| a.to_string());
    if let Some(o) = s.attr("ItemSortOrder").and_then(SortOrder::from_xml) {
        v.sort_order = o;
    }
    loop {
        match r.next_tok()? {
            Tok::Eof => return Err(XmlError("unexpected eof in ItemViewConfig".into())),
            Tok::End(n) if n == "ItemViewConfig" => return Ok(()),
            Tok::Start(s) => match s.name.as_str() {
                "Columns" => v.columns = parse_columns(r)?,
                "ThumbnailSize" => {
                    let (_, h) = parse_size(r, "ThumbnailSize")?;
                    v.thumb_height = h;
                }
                "TileSize" => {
                    let (_, h) = parse_size(r, "TileSize")?;
                    v.tile_height = h;
                }
                "ItemRowHeight" => {
                    v.row_height = r
                        .text_content("ItemRowHeight")?
                        .trim()
                        .parse()
                        .unwrap_or(24);
                }
                _ => r.skip_element(&s.name)?,
            },
            _ => {}
        }
    }
}

fn parse_columns(r: &mut XmlReader<'_>) -> XmlResult<Vec<ColumnState>> {
    let mut out = Vec::new();
    loop {
        match r.next_tok()? {
            Tok::Eof => return Err(XmlError("unexpected eof in Columns".into())),
            Tok::End(n) if n == "Columns" => return Ok(out),
            Tok::Start(s) if s.name == "Column" => {
                out.push(ColumnState {
                    id: s
                        .attr("Id")
                        .and_then(|a| a.trim().parse().ok())
                        .unwrap_or(0),
                    visible: s.attr("Visible").map(|a| a == "true").unwrap_or(true),
                    width: s
                        .attr("Width")
                        .and_then(|a| a.trim().parse().ok())
                        .unwrap_or(80),
                });
                r.skip_element("Column")?;
            }
            Tok::Start(s) => r.skip_element(&s.name)?,
            _ => {}
        }
    }
}

fn parse_size(r: &mut XmlReader<'_>, name: &str) -> XmlResult<(i32, i32)> {
    let (mut w, mut h) = (0, 0);
    loop {
        match r.next_tok()? {
            Tok::End(n) if n == name => return Ok((w, h)),
            Tok::Start(s) => match s.name.as_str() {
                "Width" => w = r.text_content("Width")?.trim().parse().unwrap_or(0),
                "Height" => h = r.text_content("Height")?.trim().parse().unwrap_or(0),
                _ => r.skip_element(&s.name)?,
            },
            Tok::Eof => return Err(XmlError("unexpected eof in size".into())),
            _ => {}
        }
    }
}

fn parse_reader_layout(r: &mut XmlReader<'_>) -> XmlResult<ReaderLayoutState> {
    let mut x = ReaderLayoutState::default();
    loop {
        match r.next_tok()? {
            Tok::Eof => return Err(XmlError("unexpected eof in LandscapeLayout".into())),
            Tok::End(n) if n == "LandscapeLayout" => return Ok(x),
            Tok::Start(s) => match s.name.as_str() {
                "PageDisplayMode" => x.fit = r.text_content("PageDisplayMode")?,
                "PageLayout" => x.layout = r.text_content("PageLayout")?,
                "PageImageRotation" => {
                    x.rotation = ImageRotation::from_xml(&r.text_content("PageImageRotation")?)
                        .unwrap_or_default()
                }
                "PageZoom" => {
                    let t = r.text_content("PageZoom")?;
                    x.zoom = t.trim().parse().unwrap_or(1.0);
                }
                _ => r.skip_element(&s.name)?,
            },
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to_bytes(x: &WorkspaceState) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut e = Emitter::new(&mut buf).unwrap();
        x.write_xml(&mut e).unwrap();
        e.finish().unwrap();
        buf
    }

    fn from_bytes(bytes: &[u8]) -> WorkspaceState {
        let mut cursor = std::io::Cursor::new(bytes.to_vec());
        let mut reader = XmlReader::new(&mut cursor);
        match reader.next_tok().unwrap() {
            Tok::Start(s) => {
                assert_eq!(s.name, "CurrentWorkspace");
                WorkspaceState::parse(&mut reader).unwrap()
            }
            tok => panic!("unexpected token {tok:?}"),
        }
    }

    #[test]
    fn default_round_trips_byte_stable() {
        let x = WorkspaceState::default();
        let bytes = to_bytes(&x);
        let back = from_bytes(&bytes);
        assert_eq!(back, x);
        assert_eq!(to_bytes(&back), bytes);
    }

    #[test]
    fn populated_round_trips_byte_stable() {
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
        let bytes = to_bytes(&x);
        let back = from_bytes(&bytes);
        assert_eq!(back, x);
        assert_eq!(to_bytes(&back), bytes);
    }

    #[test]
    fn shape_matches_the_csharp_serializer() {
        let mut x = WorkspaceState::default();
        x.view.sort_key = Some("ShadowSeries".to_string());
        x.view.columns.push(ColumnState {
            id: 3,
            visible: true,
            width: 80,
        });
        let text = String::from_utf8(to_bytes(&x)).unwrap();
        // The explorer scalars ride the DatabaseView element.
        assert!(text.contains("<DatabaseView ShowBrowser=\"true\" BrowserSplit=\"280\">"));
        // The list setup attributes on ItemViewConfig; the null
        // GrouperId omits.
        assert!(text.contains("<ItemViewConfig ItemViewMode=\"Thumbnail\" SortKey=\"ShadowSeries\" ItemSortOrder=\"Ascending\">"));
        // The columns carry the ItemViewColumnInfo attributes.
        assert!(text.contains("<Column Id=\"3\" Visible=\"true\" Width=\"80\" />"));
        // The reader layout family is the LandscapeLayout element.
        assert!(text.contains("<LandscapeLayout>"));
        assert!(text.contains("<PageDisplayMode>FitWidth</PageDisplayMode>"));
        // A null texture omits the element (the C# null-string rule).
        assert!(!text.contains("PaperTexture>"));
    }
}
