//! The item view layout engine — the pure geometry of
//! `ItemView.CalcItemPositions` (Thumbnail/Tile/Detail, Top flow,
//! group headers, culling, hit testing, keyboard movement).
//!
//! Numbers from the C# spec: `ItemPadding (1,1)` (between-cell gap
//! 2 px, cell rect inset +1), thumbnail border `Size(4,4)`, label
//! strip = 3 lines × (font × clamp(thumbH/192, 0.7, 1) + 2), tile
//! cell `Size(192,96)`, detail row height `ItemRowHeight`, first
//! column offset x+8, group header height 40, wrap on `>=` (a
//! perfectly-fitting row still breaks), Left layout is never used by
//! the browser and is not ported.

use super::view_state::ViewState;

pub const THUMB_ASPECT: f64 = 2.0 / 3.0;
pub const ITEM_BORDER: f64 = 4.0;
pub const ITEM_PADDING: f64 = 1.0;
pub const COLUMN_OFFSET_X: f64 = 8.0;
pub const LABEL_LINES: f64 = 3.0;
pub const DEFAULT_THUMB_HEIGHT: f64 = 128.0;
pub const DEFAULT_TILE: (f64, f64) = (192.0, 96.0);
pub const DEFAULT_ROW_HEIGHT: f64 = 16.0;
pub const DEFAULT_HEADER_HEIGHT: f64 = 20.0;
pub const DEFAULT_GROUP_HEADER_HEIGHT: f64 = 40.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemViewMode {
    Thumbnail,
    Tile,
    Detail,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }

    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.x + other.w
            && other.x < self.x + self.w
            && self.y < other.y + other.h
            && other.y < self.y + self.h
    }
}

/// The layout inputs (the persisted `ItemViewConfig` + the live view
/// size).
#[derive(Clone, Debug)]
pub struct LayoutConfig {
    pub mode: ItemViewMode,
    /// The client width the items flow in (header excluded).
    pub view_width: f64,
    /// The client height (the page-step size).
    pub view_height: f64,
    /// `ItemThumbSize.Height`; the width follows the 2:3 estimate.
    pub thumb_height: f64,
    pub tile_size: (f64, f64),
    pub row_height: f64,
    /// The view font height in px (the label strip scales from it).
    pub font_height: f64,
    /// The Detail column strip (visible columns in order).
    pub column_widths: Vec<f64>,
    pub header_visible: bool,
    pub header_height: f64,
    /// Group headers show when a grouper is set (`Top` layout).
    pub groups_visible: bool,
    pub group_header_height: f64,
}

impl Default for LayoutConfig {
    fn default() -> LayoutConfig {
        LayoutConfig {
            mode: ItemViewMode::Thumbnail,
            view_width: 800.0,
            view_height: 600.0,
            thumb_height: DEFAULT_THUMB_HEIGHT,
            tile_size: DEFAULT_TILE,
            row_height: DEFAULT_ROW_HEIGHT,
            font_height: 15.0,
            column_widths: Vec::new(),
            header_visible: true,
            header_height: DEFAULT_HEADER_HEIGHT,
            groups_visible: false,
            group_header_height: DEFAULT_GROUP_HEADER_HEIGHT,
        }
    }
}

/// One placed item.
#[derive(Clone, Copy, Debug)]
pub struct ItemRect {
    /// Index into `ViewState::display_order`.
    pub display: usize,
    pub rect: Rect,
    pub column: usize,
    pub row: usize,
}

/// One placed group header.
#[derive(Clone, Debug)]
pub struct GroupRect {
    pub group: usize,
    pub rect: Rect,
}

/// The computed layout (`itemInfos` + `displayedGroups` +
/// `VirtualSize`).
#[derive(Clone, Debug, Default)]
pub struct ItemLayout {
    /// Aligned with `display_order` — `None` for items hidden in a
    /// collapsed group.
    pub items: Vec<Option<ItemRect>>,
    pub group_headers: Vec<GroupRect>,
    pub virtual_size: (f64, f64),
    /// Rows of display indexes (expanded items only) — keyboard
    /// navigation (`GetColumnRowItems`).
    pub rows: Vec<Vec<usize>>,
}

/// The caption strip height (`ThumbnailLabelHeight`; the Thumbnail
/// cell reserves it at the bottom).
pub fn label_strip_height(config: &LayoutConfig) -> f64 {
    let scale = (config.thumb_height / 192.0).clamp(0.7, 1.0);
    LABEL_LINES * (config.font_height * scale + 2.0)
}

/// `CoverViewItem.MeasureItem` — the estimated cell size from the
/// 2:3 cover aspect (the real thumbnail refines this in T4).
pub fn measure_thumbnail(config: &LayoutConfig) -> (f64, f64) {
    let h = config.thumb_height;
    let w = h * THUMB_ASPECT;
    let label = label_strip_height(config);
    (w + 2.0 * ITEM_BORDER, h + 2.0 * ITEM_BORDER + label)
}

fn measure(config: &LayoutConfig) -> (f64, f64) {
    match config.mode {
        ItemViewMode::Thumbnail => measure_thumbnail(config),
        ItemViewMode::Tile => config.tile_size,
        ItemViewMode::Detail => {
            let w: f64 = config.column_widths.iter().sum();
            (w, config.row_height)
        }
    }
}

/// `ItemView.IsHeaderVisible`: header + Detail (Top layout).
pub fn header_visible(config: &LayoutConfig) -> bool {
    config.header_visible && config.mode == ItemViewMode::Detail
}

fn groups_visible(config: &LayoutConfig) -> bool {
    config.groups_visible
}

/// `CalcItemPositions`: the flow layout over the expanded items with
/// full-width group headers (`Top` layout).
pub fn compute(view: &ViewState, config: &LayoutConfig) -> ItemLayout {
    let mut layout = ItemLayout::default();
    let (cell_w, cell_h) = measure(config);
    let pad = match config.mode {
        ItemViewMode::Detail => 0.0,
        _ => ITEM_PADDING,
    };
    let cell_stride_w = cell_w + 2.0 * pad;
    let cell_stride_h = cell_h + 2.0 * pad;

    let mut y = if header_visible(config) {
        config.header_height
    } else {
        0.0
    };
    let mut column = 0usize;
    let mut row = 0usize;
    let mut row_max_h = cell_h;
    let mut row_start: Vec<Option<ItemRect>> = Vec::new();
    let mut row_width = 0.0f64;
    let mut x = 0.0f64;

    let flush_row = |row_items: &mut Vec<Option<ItemRect>>,
                     layout: &mut ItemLayout,
                     y: &mut f64,
                     row_max_h: f64,
                     row: usize| {
        for mut item in row_items.drain(..).flatten() {
            item.row = row;
            layout.items[item.display] = Some(item);
            layout.rows[row].push(item.display);
        }
        *y += 2.0 * pad + row_max_h;
    };

    layout.items = vec![None; view.len()];
    if config.mode == ItemViewMode::Detail {
        layout.rows.push(Vec::new());
    }

    for (group_index, group) in view.groups().iter().enumerate() {
        // Group header: full-width strip before the group's items.
        if groups_visible(config) {
            layout.group_headers.push(GroupRect {
                group: group_index,
                rect: Rect::new(0.0, y, config.view_width, config.group_header_height),
            });
            y += config.group_header_height;
        }
        if group.collapsed {
            continue;
        }
        for &display in &group.items {
            let _ = display;
            let (dx, dy) = (x + pad, y + pad);
            let (w, h) = match config.mode {
                ItemViewMode::Detail => (cell_w, cell_h),
                _ => (cell_w, cell_h),
            };
            if config.mode == ItemViewMode::Detail {
                // One row per item, width = the column strip.
                let item = ItemRect {
                    display,
                    rect: Rect::new(dx, dy, w, h),
                    column: 0,
                    row,
                };
                layout.items[display] = Some(item);
                if let Some(last) = layout.rows.last_mut() {
                    last.push(display);
                }
                row += 1;
                y += cell_stride_h;
                x = 0.0;
                continue;
            }
            // Flow layout: wrap when the item does not fit (`>=`).
            if column > 0 && x + 2.0 * pad + cell_w >= config.view_width {
                flush_row(&mut row_start, &mut layout, &mut y, row_max_h, row);
                row += 1;
                column = 0;
                x = 0.0;
                row_max_h = cell_h;
                row_width = 0.0;
                layout.rows.push(Vec::new());
            }
            if layout.rows.len() <= row {
                layout.rows.push(Vec::new());
            }
            let item = ItemRect {
                display,
                rect: Rect::new(x + pad, y + pad, w, h),
                column,
                row,
            };
            row_start.push(Some(item));
            row_width = row_width.max(x + 2.0 * pad + cell_w);
            row_max_h = row_max_h.max(cell_h);
            x += cell_stride_w;
            column += 1;
        }
        if config.mode != ItemViewMode::Detail && !row_start.is_empty() {
            flush_row(&mut row_start, &mut layout, &mut y, row_max_h, row);
            row += 1;
            column = 0;
            x = 0.0;
            row_max_h = cell_h;
            layout.rows.push(Vec::new());
        }
    }
    let _ = row_width;

    // Detail content width = the column strip (horizontal scroll
    // when it exceeds the client width).
    let content_w = if config.mode == ItemViewMode::Detail {
        cell_w
    } else {
        config.view_width.max(cell_w)
    };
    layout.virtual_size = (
        content_w,
        y.max(if header_visible(config) {
            config.header_height
        } else {
            0.0
        }),
    );
    layout.rows.retain(|r| !r.is_empty());
    layout
}

/// The items intersecting the visible window (`visibleItems`).
pub fn visible_items(layout: &ItemLayout, view_rect: Rect) -> impl Iterator<Item = &ItemRect> {
    layout
        .items
        .iter()
        .flatten()
        .filter(move |item| item.rect.intersects(&view_rect))
}

/// `ItemHitTest`: the item under a point (layout coordinates).
pub fn hit_test(layout: &ItemLayout, x: f64, y: f64) -> Option<usize> {
    layout
        .items
        .iter()
        .flatten()
        .find(|item| item.rect.contains(x, y))
        .map(|item| item.display)
}

/// The group header under a point (arrow/text toggle hit).
pub fn hit_group_header(layout: &ItemLayout, x: f64, y: f64) -> Option<usize> {
    layout
        .group_headers
        .iter()
        .find(|g| g.rect.contains(x, y))
        .map(|g| g.group)
}

/// `GetRelativeItem`: one step left/right = ±1 column, up/down =
/// ±1 row, wrapped across ragged rows by column position.
pub fn relative_item(layout: &ItemLayout, display: usize, dx: i32, dy: i32) -> Option<usize> {
    let item = layout.items.get(display).copied().flatten()?;
    if dx != 0 {
        // Column step within the row; at the row edges it moves
        // between rows (the C# walks the flat display list).
        let row = &layout.rows[item.row];
        let pos = row.iter().position(|&d| d == display)?;
        if dx > 0 {
            row.get(pos + 1).copied().or_else(|| {
                layout
                    .rows
                    .get(item.row + 1)
                    .and_then(|next| next.first().copied())
            })
        } else {
            let prev = pos.checked_sub(1).and_then(|p| row.get(p).copied());
            if prev.is_some() {
                return prev;
            }
            let prev_row = item.row.checked_sub(1).and_then(|r| layout.rows.get(r));
            prev_row.and_then(|r| r.last().copied())
        }
    } else {
        let row = &layout.rows[item.row];
        let pos = row.iter().position(|&d| d == display)?;
        let target_row = if dy > 0 {
            layout.rows.get(item.row + 1)?
        } else {
            layout.rows.get(item.row.checked_sub(1)?)?
        };
        // Same column when it exists; otherwise the closest column
        // from the left (the ragged-row rule: clamp into the row).
        target_row.get(pos).or_else(|| target_row.last()).copied()
    }
}

/// Page step (`PageDown`/`PageUp`): one page = the view height in
/// rows; the column clamps into the target row.
pub fn page_step_display(
    layout: &ItemLayout,
    display: usize,
    view_height: f64,
    forward: bool,
) -> Option<usize> {
    let item = layout.items.get(display).copied().flatten()?;
    let item_h = item.rect.h.max(1.0);
    let step_rows = ((view_height / item_h).floor() as usize).max(1);
    let target_row = if forward {
        item.row + step_rows
    } else {
        item.row.saturating_sub(step_rows)
    };
    let row = match layout.rows.get(target_row) {
        Some(r) => r,
        None if forward => layout.rows.last()?,
        None => layout.rows.first()?,
    };
    row.get(item.column).or_else(|| row.last()).copied()
}

/// The column x-ranges for Detail mode (first column at +8).
pub fn detail_column_rects(config: &LayoutConfig, row_rect: &Rect) -> Vec<Rect> {
    let mut rects = Vec::with_capacity(config.column_widths.len());
    let mut x = row_rect.x + COLUMN_OFFSET_X;
    for w in &config.column_widths {
        rects.push(Rect::new(x, row_rect.y, *w, row_rect.h));
        x += w;
    }
    rects
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::view_state::ViewState;
    use cr_core::model::comic_book::ComicBook;
    use cr_core::xml::scalar::CrGuid;

    fn book(id: u8) -> ComicBook {
        let mut b = ComicBook {
            id: CrGuid::from_bytes([id; 16]),
            ..Default::default()
        };
        b.info.series = format!("S{id}");
        b
    }

    fn thumb_config(width: f64) -> LayoutConfig {
        LayoutConfig {
            mode: ItemViewMode::Thumbnail,
            view_width: width,
            ..Default::default()
        }
    }

    #[test]
    fn thumbnail_cells_flow_and_wrap() {
        let view = ViewState::new((0..7).map(book).collect());
        let (cell_w, cell_h) = measure_thumbnail(&thumb_config(800.0));
        // Width fits exactly 3 cells (the >= rule: a 4th cell that
        // perfectly fills must wrap — keep one pixel of slack).
        let width = cell_w * 3.0 + 4.0 + 2.0 * ITEM_PADDING + 1.0;
        let config = thumb_config(width);
        let layout = compute(&view, &config);
        assert_eq!(layout.items.len(), 7);
        let row0: Vec<usize> = layout.rows[0].clone();
        assert_eq!(row0.len(), 3, "three cells per row");
        // Rows: 3 + 3 + 1.
        assert_eq!(layout.rows.len(), 3);
        // Cell geometry: inset by the padding.
        let first = layout.items[0].unwrap();
        assert!((first.rect.x - ITEM_PADDING).abs() < 1e-9);
        assert!((first.rect.w - cell_w).abs() < 1e-9);
        // Virtual height covers all rows.
        assert!(layout.virtual_size.1 >= cell_h * 3.0);
        let _ = cell_h;
    }

    #[test]
    fn group_headers_take_full_rows() {
        let mut view = ViewState::new((0..6).map(book).collect());
        view.set_grouper(Some("Series")); // every book its own bucket
        let config = LayoutConfig {
            mode: ItemViewMode::Thumbnail,
            view_width: 4000.0,
            group_header_height: 40.0,
            groups_visible: true,
            ..Default::default()
        };
        let layout = compute(&view, &config);
        assert_eq!(layout.group_headers.len(), 6);
        for (i, gh) in layout.group_headers.iter().enumerate() {
            assert_eq!(gh.rect.w, 4000.0);
            assert_eq!(gh.rect.h, 40.0);
            assert_eq!(gh.group, i);
        }
        // Headers sit between the item rows.
        let items: Vec<&ItemRect> = layout.items.iter().flatten().collect();
        assert_eq!(items.len(), 6);
        for pair in items.windows(2) {
            assert!(
                pair[0].rect.y < pair[1].rect.y,
                "each item is under its header"
            );
        }
    }

    #[test]
    fn collapsed_groups_hide_their_items() {
        let mut view = ViewState::new((0..4).map(book).collect());
        view.set_grouper(Some("Series"));
        view.set_collapsed(0, true);
        let config = LayoutConfig {
            mode: ItemViewMode::Thumbnail,
            view_width: 4000.0,
            ..Default::default()
        };
        let layout = compute(&view, &config);
        let placed: Vec<usize> = layout.items.iter().flatten().map(|i| i.display).collect();
        assert_eq!(placed.len(), 3, "the collapsed group's item is hidden");
        assert!(view.groups()[0].collapsed);
    }

    #[test]
    fn culling_and_hit_testing() {
        let view = ViewState::new((0..30).map(book).collect());
        let config = thumb_config(400.0);
        let layout = compute(&view, &config);
        let window = Rect::new(0.0, 0.0, 400.0, 300.0);
        let visible: Vec<usize> = visible_items(&layout, window).map(|i| i.display).collect();
        assert!(!visible.is_empty());
        assert!(visible.len() < 30, "culling must skip offscreen items");
        let first = layout.items[0].unwrap();
        let hit = hit_test(&layout, first.rect.x + 1.0, first.rect.y + 1.0);
        assert_eq!(hit, Some(0));
        assert_eq!(hit_test(&layout, -5.0, -5.0), None);
    }

    #[test]
    fn keyboard_movement_walks_columns_and_rows() {
        let view = ViewState::new((0..7).map(book).collect());
        let (cell_w, _cell_h) = measure_thumbnail(&thumb_config(400.0));
        // Exactly 2 cells per row: the second spans x+2pad+cell_w,
        // so the width must clear 2*cell_w + 4*pad (the >= wrap rule).
        let config = thumb_config(2.0 * cell_w + 4.0 * ITEM_PADDING + 1.0);
        let layout = compute(&view, &config);
        assert_eq!(layout.rows[0].len(), 2);
        // Right from item 0 → item 1; down → row 1 same column.
        assert_eq!(relative_item(&layout, 0, 1, 0), Some(1));
        assert_eq!(relative_item(&layout, 0, 0, 1), Some(2));
        // Right at the row edge → the next row's first item.
        assert_eq!(relative_item(&layout, 1, 1, 0), Some(2));
        // Left at column 0 of row 1 → the last item of row 0.
        assert_eq!(relative_item(&layout, 2, -1, 0), Some(1));
        // Up from the top row: none.
        assert_eq!(relative_item(&layout, 0, 0, -1), None);
        // Ragged last row (1 item): down clamps into it.
        assert_eq!(relative_item(&layout, 5, 0, 1), Some(6));
    }

    #[test]
    fn detail_layout_uses_columns_and_header() {
        let view = ViewState::new((0..3).map(book).collect());
        let config = LayoutConfig {
            mode: ItemViewMode::Detail,
            view_width: 600.0,
            row_height: 22.0,
            header_height: 20.0,
            header_visible: true,
            column_widths: vec![200.0, 40.0, 60.0],
            ..Default::default()
        };
        let layout = compute(&view, &config);
        let rows: Vec<&ItemRect> = layout.items.iter().flatten().collect();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].rect.y, 20.0, "below the header");
        assert_eq!(rows[0].rect.h, 22.0);
        // Column rects inside the row: first at +8.
        let cols = detail_column_rects(&config, &rows[0].rect);
        assert_eq!(cols.len(), 3);
        assert_eq!(cols[0].x, rows[0].rect.x + 8.0);
        assert_eq!(cols[1].x, cols[0].x + 200.0);
        // Content width = the column sum.
        assert_eq!(layout.virtual_size.0, 300.0);
    }

    #[test]
    fn tile_cells_are_fixed() {
        let view = ViewState::new((0..5).map(book).collect());
        let config = LayoutConfig {
            mode: ItemViewMode::Tile,
            view_width: 600.0,
            tile_size: (192.0, 96.0),
            ..Default::default()
        };
        let layout = compute(&view, &config);
        let items: Vec<&ItemRect> = layout.items.iter().flatten().collect();
        assert_eq!(items.len(), 5);
        // 3 tiles per 600px row (192+2), 2 rows.
        assert_eq!(layout.rows[0].len(), 3);
        for item in &items {
            assert_eq!(item.rect.w, 192.0);
            assert_eq!(item.rect.h, 96.0);
        }
    }
}
