//! The `<Display>` config subtree: DisplayListConfig, ItemViewConfig,
//! ItemViewColumnInfo, ItemViewGroupsStatus, ThumbnailConfig,
//! TileConfig, StacksConfig.

use crate::model::enums::{
    ComicTextElements, GroupStatus, ItemViewMode, MatcherOption, ShowComicType, ShowOptionType,
    SortOrder,
};
use crate::xml::reader::{XmlError, XmlResult};
use crate::xml::scalar::{CrDateTime, CrGuid};
use crate::xml::{Emitter, Start, Tok};
use std::io::Write;

fn parse_i32(v: &str, ctx: &str) -> XmlResult<i32> {
    v.trim()
        .parse()
        .map_err(|_| XmlError(format!("bad int in {ctx}: {v}")))
}

fn parse_bool(v: &str, ctx: &str) -> XmlResult<bool> {
    match v.trim() {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => Err(XmlError(format!("bad bool in {ctx}: {v}"))),
    }
}

fn parse_bool_v(r: &mut crate::xml::XmlReader<'_>, elem: &str) -> XmlResult<bool> {
    let v = r.text_content(elem)?;
    parse_bool(&v, elem)
}

// ---------- Size (System.Drawing.Size: Width/Height as elements) ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Size {
    pub width: i32,
    pub height: i32,
}

impl Size {
    fn write_xml<W: Write>(&self, e: &mut Emitter<W>) -> std::io::Result<()> {
        e.start("Size")?;
        e.text_elem("Width", &self.width.to_string())?;
        e.text_elem("Height", &self.height.to_string())?;
        e.end()
    }

    fn from_start(s: &Start, r: &mut crate::xml::XmlReader<'_>) -> XmlResult<Self> {
        let mut sz = Size::default();
        loop {
            match r.next_tok()? {
                Tok::Eof => return Err(XmlError("eof in Size".into())),
                Tok::End(n) if n == s.name => return Ok(sz),
                Tok::Start(s2) => {
                    let v = r.text_content(&s2.name)?;
                    match s2.name.as_str() {
                        "Width" => sz.width = parse_i32(v.trim(), "Width")?,
                        "Height" => sz.height = parse_i32(v.trim(), "Height")?,
                        _ => {}
                    }
                }
                Tok::Text(_) => {}
                _ => {}
            }
        }
    }
}

// ---------- ItemViewColumnInfo ----------

#[derive(Clone, Debug, PartialEq)]
pub struct ItemViewColumnInfo {
    pub id: i32,
    pub format_id: i32,
    pub visible: bool,
    pub width: i32,
    pub last_time_visible: CrDateTime,
}

impl Default for ItemViewColumnInfo {
    fn default() -> Self {
        ItemViewColumnInfo {
            id: 0,
            format_id: 0,
            visible: true,
            width: 80,
            last_time_visible: CrDateTime::min_value(),
        }
    }
}

impl ItemViewColumnInfo {
    fn write_xml<W: Write>(&self, e: &mut Emitter<W>) -> std::io::Result<()> {
        e.start("Column")?;
        if self.id != 0 {
            e.attr("Id", &self.id.to_string())?;
        }
        if self.format_id != 0 {
            e.attr("FormatId", &self.format_id.to_string())?;
        }
        if !self.visible {
            e.attr("Visible", "false")?;
        }
        if self.width != 80 {
            e.attr("Width", &self.width.to_string())?;
        }
        if !self.last_time_visible.is_min_value() {
            e.text_elem("LastTimeVisible", &self.last_time_visible.to_xml())?;
        }
        e.end()
    }

    fn from_start(s: &Start, r: &mut crate::xml::XmlReader<'_>) -> XmlResult<Self> {
        let mut c = ItemViewColumnInfo::default();
        for (k, v) in &s.attrs {
            match k.as_str() {
                "Id" => c.id = parse_i32(v, "Column Id")?,
                "FormatId" => c.format_id = parse_i32(v, "Column FormatId")?,
                "Visible" => c.visible = parse_bool(v, "Column Visible")?,
                "Width" => c.width = parse_i32(v, "Column Width")?,
                _ => {}
            }
        }
        loop {
            match r.next_tok()? {
                Tok::Eof => return Err(XmlError("eof in Column".into())),
                Tok::End(n) if n == "Column" => return Ok(c),
                Tok::Start(s2) if s2.name == "LastTimeVisible" => {
                    let v = r.text_content("LastTimeVisible")?;
                    c.last_time_visible = CrDateTime::parse(v.trim()).map_err(|e| XmlError(e.0))?;
                }
                Tok::Start(s2) => r.skip_element(&s2.name)?,
                Tok::Text(_) => {}
                _ => {}
            }
        }
    }
}

// ---------- ItemViewGroupsStatus ----------

#[derive(Clone, Debug, PartialEq, Default)]
pub struct ItemViewGroupsStatus {
    pub status: GroupStatus,
    pub keys: Vec<i32>,
}

impl ItemViewGroupsStatus {
    fn write_xml<W: Write>(&self, e: &mut Emitter<W>) -> std::io::Result<()> {
        e.start("GroupsStatus")?;
        e.text_elem("Status", &self.status.to_xml())?;
        e.start("Keys")?;
        for k in &self.keys {
            e.text_elem("int", &k.to_string())?;
        }
        e.end()?;
        e.end()
    }

    fn from_start(s: &Start, r: &mut crate::xml::XmlReader<'_>) -> XmlResult<Self> {
        // GroupStatus has no DefaultValue: Status element always written.
        let mut g = ItemViewGroupsStatus {
            status: GroupStatus::AllExpanded,
            keys: Vec::new(),
        };
        loop {
            match r.next_tok()? {
                Tok::Eof => return Err(XmlError("eof in GroupsStatus".into())),
                Tok::End(n) if n == s.name => return Ok(g),
                Tok::Start(s2) => match s2.name.as_str() {
                    "Status" => {
                        let v = r.text_content("Status")?;
                        g.status = GroupStatus::from_xml(v.trim())
                            .ok_or_else(|| XmlError(format!("bad GroupStatus: {v}")))?;
                    }
                    "Keys" => loop {
                        match r.next_tok()? {
                            Tok::Eof => return Err(XmlError("eof in Keys".into())),
                            Tok::End(n) if n == "Keys" => break,
                            Tok::Start(s3) => {
                                let v = r.text_content(&s3.name)?;
                                g.keys.push(parse_i32(v.trim(), "Keys int")?);
                            }
                            Tok::Text(_) => {}
                            _ => {}
                        }
                    },
                    _ => r.skip_element(&s2.name)?,
                },
                Tok::Text(_) => {}
                _ => {}
            }
        }
    }
}

// ---------- ItemViewConfig ----------

#[derive(Clone, Debug, PartialEq)]
pub struct ItemViewConfig {
    pub columns: Vec<ItemViewColumnInfo>,
    pub item_view_mode: ItemViewMode,
    pub grouping: bool,
    pub sort_key: Option<String>,
    pub grouper_id: Option<String>,
    pub stacker_id: Option<String>,
    pub item_sort_order: SortOrder,
    pub group_sort_order: SortOrder,
    pub groups_status: Option<ItemViewGroupsStatus>,
    pub thumbnail_size: Size,
    pub tile_size: Size,
    pub item_row_height: i32,
}

impl Default for ItemViewConfig {
    fn default() -> Self {
        ItemViewConfig {
            columns: Vec::new(),
            item_view_mode: ItemViewMode::Detail,
            grouping: false,
            sort_key: None,
            grouper_id: None,
            stacker_id: None,
            item_sort_order: SortOrder::Ascending,
            group_sort_order: SortOrder::Ascending,
            groups_status: None,
            thumbnail_size: Size::default(),
            tile_size: Size::default(),
            item_row_height: 0,
        }
    }
}

impl ItemViewConfig {
    pub fn write_xml<W: Write>(&self, e: &mut Emitter<W>) -> std::io::Result<()> {
        e.start("View")?;
        if self.item_view_mode != ItemViewMode::Detail {
            e.attr("ItemViewMode", &self.item_view_mode.to_xml())?;
        }
        if self.grouping {
            e.attr("Grouping", "true")?;
        }
        if let Some(v) = &self.sort_key {
            e.attr("SortKey", v)?;
        }
        if let Some(v) = &self.grouper_id {
            e.attr("GrouperId", v)?;
        }
        if let Some(v) = &self.stacker_id {
            e.attr("StackerId", v)?;
        }
        if self.item_sort_order != SortOrder::Ascending {
            e.attr("ItemSortOrder", &self.item_sort_order.to_xml())?;
        }
        if self.group_sort_order != SortOrder::Ascending {
            e.attr("GroupSortOrder", &self.group_sort_order.to_xml())?;
        }
        e.start("Columns")?;
        for c in &self.columns {
            c.write_xml(e)?;
        }
        e.end()?;
        if let Some(g) = &self.groups_status {
            g.write_xml(e)?;
        }
        e.start("ThumbnailSize")?;
        self.thumbnail_size.write_xml(e)?;
        e.end()?;
        e.start("TileSize")?;
        self.tile_size.write_xml(e)?;
        e.end()?;
        // ItemRowHeight has no DefaultValue: always written.
        e.text_elem("ItemRowHeight", &self.item_row_height.to_string())?;
        e.end()
    }

    pub fn from_start(s: &Start, r: &mut crate::xml::XmlReader<'_>) -> XmlResult<Self> {
        let mut c = ItemViewConfig::default();
        for (k, v) in &s.attrs {
            match k.as_str() {
                "ItemViewMode" => {
                    c.item_view_mode = ItemViewMode::from_xml(v)
                        .ok_or_else(|| XmlError(format!("bad ItemViewMode: {v}")))?
                }
                "Grouping" => c.grouping = parse_bool(v, "Grouping")?,
                "SortKey" => c.sort_key = Some(v.clone()),
                "GrouperId" => c.grouper_id = Some(v.clone()),
                "StackerId" => c.stacker_id = Some(v.clone()),
                "ItemSortOrder" => {
                    c.item_sort_order = SortOrder::from_xml(v)
                        .ok_or_else(|| XmlError(format!("bad SortOrder: {v}")))?
                }
                "GroupSortOrder" => {
                    c.group_sort_order = SortOrder::from_xml(v)
                        .ok_or_else(|| XmlError(format!("bad SortOrder: {v}")))?
                }
                _ => {}
            }
        }
        loop {
            match r.next_tok()? {
                Tok::Eof => return Err(XmlError("eof in View".into())),
                Tok::End(n) if n == s.name => return Ok(c),
                Tok::Start(s2) => match s2.name.as_str() {
                    "Columns" => loop {
                        match r.next_tok()? {
                            Tok::Eof => return Err(XmlError("eof in Columns".into())),
                            Tok::End(n) if n == "Columns" => break,
                            Tok::Start(s3) if s3.name == "Column" => {
                                c.columns.push(ItemViewColumnInfo::from_start(&s3, r)?);
                            }
                            Tok::Start(s3) => r.skip_element(&s3.name)?,
                            Tok::Text(_) => {}
                            _ => {}
                        }
                    },
                    "GroupsStatus" => {
                        c.groups_status = Some(ItemViewGroupsStatus::from_start(&s2, r)?)
                    }
                    "ThumbnailSize" => c.thumbnail_size = Size::from_start(&s2, r)?,
                    "TileSize" => c.tile_size = Size::from_start(&s2, r)?,
                    "ItemRowHeight" => {
                        let v = r.text_content("ItemRowHeight")?;
                        c.item_row_height = parse_i32(v.trim(), "ItemRowHeight")?;
                    }
                    _ => r.skip_element(&s2.name)?,
                },
                Tok::Text(_) => {}
                _ => {}
            }
        }
    }
}

// ---------- ThumbnailConfig / TileConfig / StacksConfig ----------

#[derive(Clone, Debug, PartialEq)]
pub struct ThumbnailConfig {
    pub hide_captions: bool,
    pub lines: Vec<i32>,
    pub text_elements: ComicTextElements,
}

impl Default for ThumbnailConfig {
    fn default() -> Self {
        ThumbnailConfig {
            hide_captions: false,
            lines: Vec::new(),
            text_elements: ComicTextElements(0x37A), // DefaultFileComic
        }
    }
}

impl ThumbnailConfig {
    fn write_xml<W: Write>(&self, e: &mut Emitter<W>) -> std::io::Result<()> {
        e.start("Thumbnail")?;
        if self.hide_captions {
            e.attr("HideCaptions", "true")?;
        }
        e.start("Lines")?;
        for l in &self.lines {
            e.text_elem("Id", &l.to_string())?;
        }
        e.end()?;
        if self.text_elements != ThumbnailConfig::default().text_elements {
            e.text_elem("TextElements", &self.text_elements.to_xml())?;
        }
        e.end()
    }

    fn from_start(s: &Start, r: &mut crate::xml::XmlReader<'_>) -> XmlResult<Self> {
        let mut t = ThumbnailConfig::default();
        for (k, v) in &s.attrs {
            if k == "HideCaptions" {
                t.hide_captions = parse_bool(v, "HideCaptions")?;
            }
        }
        loop {
            match r.next_tok()? {
                Tok::Eof => return Err(XmlError("eof in Thumbnail".into())),
                Tok::End(n) if n == s.name => return Ok(t),
                Tok::Start(s2) => match s2.name.as_str() {
                    "Lines" => loop {
                        match r.next_tok()? {
                            Tok::Eof => return Err(XmlError("eof in Lines".into())),
                            Tok::End(n) if n == "Lines" => break,
                            Tok::Start(s3) => {
                                let v = r.text_content(&s3.name)?;
                                t.lines.push(parse_i32(v.trim(), "Lines Id")?);
                            }
                            Tok::Text(_) => {}
                            _ => {}
                        }
                    },
                    "TextElements" => {
                        let v = r.text_content("TextElements")?;
                        t.text_elements = ComicTextElements::from_xml(v.trim())
                            .ok_or_else(|| XmlError(format!("bad ComicTextElements: {v}")))?;
                    }
                    _ => r.skip_element(&s2.name)?,
                },
                Tok::Text(_) => {}
                _ => {}
            }
        }
    }
}

/// TileConfig has no members: `<Tile />`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct TileConfig;

#[derive(Clone, Debug, PartialEq, Default)]
pub struct StacksConfigItem {
    pub stack: Option<String>,
    pub top_id: CrGuid,
    pub thumbnail_key: Option<String>,
    pub config: Option<ItemViewConfig>,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct StacksConfig {
    pub configs: Vec<StacksConfigItem>,
}

impl StacksConfig {
    fn write_xml<W: Write>(&self, e: &mut Emitter<W>) -> std::io::Result<()> {
        e.start("StackConfig")?;
        e.start("Configs")?;
        for item in &self.configs {
            e.start("StackConfigItem")?;
            if let Some(s) = &item.stack {
                e.attr("Stack", s)?;
            }
            if !item.top_id.is_empty() {
                e.attr("TopId", &item.top_id.to_d_string())?;
            }
            if let Some(k) = &item.thumbnail_key {
                e.attr("ThumbnailKey", k)?;
            }
            if let Some(c) = &item.config {
                c.write_xml(e)?;
            }
            e.end()?;
        }
        e.end()?;
        e.end()
    }

    fn from_start(s: &Start, r: &mut crate::xml::XmlReader<'_>) -> XmlResult<Self> {
        let mut cfg = StacksConfig::default();
        loop {
            match r.next_tok()? {
                Tok::Eof => return Err(XmlError("eof in StackConfig".into())),
                Tok::End(n) if n == s.name => return Ok(cfg),
                Tok::Start(s2) if s2.name == "Configs" => loop {
                    match r.next_tok()? {
                        Tok::Eof => return Err(XmlError("eof in Configs".into())),
                        Tok::End(n) if n == "Configs" => break,
                        Tok::Start(s3) if s3.name == "StackConfigItem" => {
                            let mut it = StacksConfigItem::default();
                            for (k, v) in &s3.attrs {
                                match k.as_str() {
                                    "Stack" => it.stack = Some(v.clone()),
                                    "TopId" => it.top_id = CrGuid::parse(v)?,
                                    "ThumbnailKey" => it.thumbnail_key = Some(v.clone()),
                                    _ => {}
                                }
                            }
                            loop {
                                match r.next_tok()? {
                                    Tok::Eof => {
                                        return Err(XmlError("eof in StackConfigItem".into()))
                                    }
                                    Tok::End(n) if n == "StackConfigItem" => break,
                                    Tok::Start(s4) if s4.name == "Config" => {
                                        it.config = Some(ItemViewConfig::from_start(&s4, r)?);
                                    }
                                    Tok::Start(s4) => r.skip_element(&s4.name)?,
                                    Tok::Text(_) => {}
                                    _ => {}
                                }
                            }
                            cfg.configs.push(it);
                        }
                        Tok::Start(s3) => r.skip_element(&s3.name)?,
                        Tok::Text(_) => {}
                        _ => {}
                    }
                },
                Tok::Start(s2) => r.skip_element(&s2.name)?,
                Tok::Text(_) => {}
                _ => {}
            }
        }
    }
}

// ---------- DisplayListConfig ----------

#[derive(Clone, Debug, PartialEq)]
pub struct DisplayListConfig {
    pub view: Option<ItemViewConfig>,
    pub thumbnail: Option<ThumbnailConfig>,
    pub tile: Option<TileConfig>,
    pub stack_config: Option<StacksConfig>,
    pub background_image_source: Option<String>,
    pub show_option_type: ShowOptionType,
    pub show_comic_type: ShowComicType,
    pub show_only_duplicates: bool,
    pub show_group_headers: bool,
    pub show_group_headers_width: i32,
    pub quick_search: Option<String>,
    pub quick_search_type: MatcherOption,
}

impl Default for DisplayListConfig {
    fn default() -> Self {
        DisplayListConfig {
            view: None,
            thumbnail: None,
            tile: None,
            stack_config: None,
            background_image_source: None,
            show_option_type: ShowOptionType::All,
            show_comic_type: ShowComicType::All,
            show_only_duplicates: false,
            show_group_headers: false,
            show_group_headers_width: 0,
            quick_search: None,
            quick_search_type: MatcherOption::All,
        }
    }
}

impl DisplayListConfig {
    pub fn write_xml<W: Write>(&self, e: &mut Emitter<W>) -> std::io::Result<()> {
        e.start("Display")?;
        if let Some(v) = &self.view {
            v.write_xml(e)?;
        }
        if let Some(t) = &self.thumbnail {
            t.write_xml(e)?;
        }
        if self.tile.is_some() {
            e.start("Tile")?;
            e.end()?;
        }
        if let Some(s) = &self.stack_config {
            s.write_xml(e)?;
        }
        if let Some(b) = &self.background_image_source {
            e.text_elem("BackgroundImageSource", b)?;
        }
        if self.show_option_type != ShowOptionType::All {
            e.text_elem("ShowOptionType", &self.show_option_type.to_xml())?;
        }
        if self.show_comic_type != ShowComicType::All {
            e.text_elem("ShowComicType", &self.show_comic_type.to_xml())?;
        }
        if self.show_only_duplicates {
            e.text_elem("ShowOnlyDuplicates", "true")?;
        }
        if self.show_group_headers {
            e.text_elem("ShowGroupHeaders", "true")?;
        }
        if self.show_group_headers_width != 0 {
            e.text_elem(
                "ShowGroupHeadersWidth",
                &self.show_group_headers_width.to_string(),
            )?;
        }
        if let Some(q) = &self.quick_search {
            e.text_elem("QuickSearch", q)?;
        }
        if self.quick_search_type != MatcherOption::All {
            e.text_elem("QuickSearchType", &self.quick_search_type.to_xml())?;
        }
        e.end()
    }

    pub fn from_start(s: &Start, r: &mut crate::xml::XmlReader<'_>) -> XmlResult<Self> {
        let mut d = DisplayListConfig::default();
        loop {
            match r.next_tok()? {
                Tok::Eof => return Err(XmlError("eof in Display".into())),
                Tok::End(n) if n == s.name => return Ok(d),
                Tok::Start(s2) => match s2.name.as_str() {
                    "View" => d.view = Some(ItemViewConfig::from_start(&s2, r)?),
                    "Thumbnail" => d.thumbnail = Some(ThumbnailConfig::from_start(&s2, r)?),
                    "Tile" => {
                        r.skip_element("Tile")?;
                        d.tile = Some(TileConfig);
                    }
                    "StackConfig" => d.stack_config = Some(StacksConfig::from_start(&s2, r)?),
                    "BackgroundImageSource" => {
                        d.background_image_source = Some(r.text_content("BackgroundImageSource")?)
                    }
                    "ShowOptionType" => {
                        let v = r.text_content("ShowOptionType")?;
                        d.show_option_type = ShowOptionType::from_xml(v.trim())
                            .ok_or_else(|| XmlError(format!("bad ShowOptionType: {v}")))?;
                    }
                    "ShowComicType" => {
                        let v = r.text_content("ShowComicType")?;
                        d.show_comic_type = ShowComicType::from_xml(v.trim())
                            .ok_or_else(|| XmlError(format!("bad ShowComicType: {v}")))?;
                    }
                    "ShowOnlyDuplicates" => {
                        d.show_only_duplicates = parse_bool_v(r, "ShowOnlyDuplicates")?
                    }
                    "ShowGroupHeaders" => {
                        d.show_group_headers = parse_bool_v(r, "ShowGroupHeaders")?
                    }
                    "ShowGroupHeadersWidth" => {
                        let v = r.text_content("ShowGroupHeadersWidth")?;
                        d.show_group_headers_width = parse_i32(v.trim(), "ShowGroupHeadersWidth")?;
                    }
                    "QuickSearch" => d.quick_search = Some(r.text_content("QuickSearch")?),
                    "QuickSearchType" => {
                        let v = r.text_content("QuickSearchType")?;
                        d.quick_search_type = MatcherOption::from_xml(v.trim())
                            .ok_or_else(|| XmlError(format!("bad MatcherOption: {v}")))?;
                    }
                    _ => r.skip_element(&s2.name)?,
                },
                Tok::Text(_) => {}
                _ => {}
            }
        }
    }
}
