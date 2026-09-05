//! The Pages panel — the open comic's pages as a thumbnail grid
//! (`PagesView` + `PageViewItem`). Bound to the currently OPEN
//! reader book (never the browser selection — the C# `Book`
//! property binds `ComicDisplay.Book`); visible as a browser-panel
//! tab only while a comic is open (`MainView.OnGuiVisibility`).
//!
//! Ported behavior: one cell per page, sized to the page's stored
//! aspect (the 2:3 estimate when the info lacks dimensions), the
//! 1-based page-number badge (`DrawPageNumber`: top-right, black 75%
//! rounded, white text), the red bookmark pennant on bookmarked
//! pages (`DrawBookmarkH`), the current reader page highlighted and
//! scrolled into view (`Navigation` → `EnsureVisible`), and
//! double-click → `Navigate(page, Absolute)`.
//!
//! The T7 toolbar (the control's own `toolStrip`,
//! ComicPagesView.Designer.cs:66-141): the Views drop with the
//! Thumbnail/Tile mode radios (`tbbView`; Details and the
//! Collapse/Expand-Groups row are cut — the panel has no detail list
//! or groups) with the main click cycling the mode; the Page Filter
//! button is omitted (the panel consumes no page-type filter).
//!
//! Tile mode (`ThumbTileRenderer`): the thumb left, the text lines
//! right (`ComicTextBuilder.GetTextBlocks`, `ComicTextElements.
//! DefaultPage`): "Page #N" bold, the page type, "Size: …",
//! "Resolution: W x H", optional rotation/bookmark lines.
//!
//! Deviations: the 3D-book backdrop and drag-out copy are later
//! polish, and the panel has no edit commands (Phase 5).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{cairo, DrawingArea, EventControllerScroll, GestureClick, ScrolledWindow};

use cr_core::model::comic_book::ComicBook;
use cr_core::model::enums::ImageRotation;
use cr_engine::image_pool::ImagePool;
use cr_image::keys::{ImageKey, ThumbnailKey};

use super::menubar::{self, Dropdown, MenuNode};
use MenuNode::Item;

/// The Views drop (`tbbView.DropDownItems`): the page-grid mode
/// radios (Details + the groups row are cut — no detail list or
/// groups in the port's panel).
pub const PAGES_VIEWS: &[MenuNode] = &[
    Item(
        "T&humbnails",
        "win.pages-view-mode::thumbnail",
        "",
        "ThumbView",
    ),
    Item("&Tiles", "win.pages-view-mode::tile", "", "TileView"),
];

/// The panel's display mode (`ItemViewMode` reduced to the two
/// supported grids).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PagesMode {
    Thumbnail,
    Tile,
}

impl PagesMode {
    pub fn action_name(self) -> &'static str {
        match self {
            PagesMode::Thumbnail => "thumbnail",
            PagesMode::Tile => "tile",
        }
    }

    pub fn from_action_name(name: &str) -> PagesMode {
        match name {
            "tile" => PagesMode::Tile,
            _ => PagesMode::Thumbnail,
        }
    }
}

/// The thumb-size range (`ItemSizeInfo`: [96, 512]).
const MIN_THUMB: f64 = 96.0;
const MAX_THUMB: f64 = 512.0;

const BG: (f64, f64, f64) = (0.13, 0.13, 0.15);
const FOCUS_UNFOCUSED: (f64, f64, f64) = (0.5, 0.5, 0.55);

#[derive(Clone)]
struct PageCell {
    /// The display page (0-based).
    page: usize,
    /// The archive image index (the thumb key's page; the C#
    /// `TranslatePageToImageIndex`).
    image_index: usize,
    /// The page aspect (w/h) — the stored dims or the 2:3 estimate.
    aspect: f64,
    bookmarked: bool,
    /// The tile-text fields (`ComicPageInfo`): the type name, the
    /// byte size, the pixel dims, the stored rotation, the bookmark.
    type_name: String,
    file_size: i32,
    width: i32,
    height: i32,
    rotation: ImageRotation,
    bookmark: Option<String>,
}

struct PageDone {
    page: usize,
    bytes: Option<Vec<u8>>,
    /// The comic the load was queued for — the pump drops stale
    /// completions after a rebind (comic A's in-flight pages must
    /// not land in comic B's map).
    source: String,
}

struct PagesState {
    /// The bound comic: the file path (the thumb key source) and the
    /// page cells (the C# binds the whole `ComicBook`; the panel only
    /// reads the path + pages, and a plain field dodges the Ref-deref
    /// borrow fights).
    bound: Option<(String, Vec<PageCell>)>,
    pool: Arc<ImagePool>,
    thumb_rx: std::sync::mpsc::Receiver<PageDone>,
    thumb_tx: std::sync::mpsc::Sender<PageDone>,
    thumbs: HashMap<usize, cairo::ImageSurface>,
    queued: std::collections::HashSet<usize>,
    pending: usize,
    pump_active: bool,
    thumb_height: f64,
    current_page: usize,
    /// The grid mode (the Views drop radios).
    mode: PagesMode,
    /// The content height the canvas was last sized for (the draw
    /// path self-corrects: the binding can land while the panel is
    /// hidden, where the canvas width is 0 and the layout collapses).
    last_content_height: f64,
    canvas: DrawingArea,
}

/// The activation callback lives OUTSIDE the state: the callee
/// re-enters the state (navigate → page callback →
/// `set_current_page`), so it must run with no state borrow held.
type ActivateCell = Rc<RefCell<Option<Box<dyn Fn(usize)>>>>;

impl PagesState {
    /// The greedy flow (the ItemView Top layout: 1 px padding → 2 px
    /// gaps). Thumbnail cells size to the page aspect plus border 4;
    /// Tile cells are fixed (192×96 at the default thumb height,
    /// scaled with the size slider). Returns the placed cells for
    /// the draw.
    fn relayout(&mut self, width: f64) -> Vec<(f64, f64, f64, f64, usize)> {
        let Some((_, cells)) = &self.bound else {
            return Vec::new();
        };
        let tile = self.mode == PagesMode::Tile;
        let scale = self.thumb_height / 128.0;
        let mut placed = Vec::with_capacity(cells.len());
        let border = 4.0;
        let pad = 1.0;
        let mut x = 0.0;
        let mut y = 0.0;
        let mut row_h = 0.0f64;
        let mut col = 0usize;
        for cell in cells {
            let (w, h) = if tile {
                (192.0 * scale + 2.0 * border, 96.0 * scale + 2.0 * border)
            } else {
                (
                    self.thumb_height * cell.aspect + 2.0 * border,
                    self.thumb_height + 2.0 * border,
                )
            };
            let stride_w = w + 2.0 * pad;
            if col > 0 && x + 2.0 * pad + w >= width {
                y += 2.0 * pad + row_h;
                x = 0.0;
                col = 0;
                row_h = 0.0;
            }
            placed.push((x + pad, y + pad, w, h, cell.page));
            row_h = row_h.max(h);
            x += stride_w;
            col += 1;
        }
        placed
    }

    fn content_height(&mut self, width: f64) -> f64 {
        self.relayout(width)
            .last()
            .map(|(_, y, _, h, _)| y + h + 4.0)
            .unwrap_or(0.0)
    }
}

/// The clone-able handle (the shell keeps it; the Rc must outlive
/// the window).
#[derive(Clone)]
pub struct PagesPanel {
    state: Rc<RefCell<PagesState>>,
    activation: ActivateCell,
    canvas: DrawingArea,
    scroller: ScrolledWindow,
    /// The Views drop (the mode radios) + its anchor (the split
    /// button's main part).
    views_drop: Dropdown,
    views_btn: gtk4::Button,
}

pub struct PagesPanelWidgets {
    /// The mounted widget: [toolbar][scroller].
    pub widget: gtk4::Box,
    pub panel: PagesPanel,
}

impl PagesPanel {
    pub fn create(pool: Arc<ImagePool>, window: &gtk4::ApplicationWindow) -> PagesPanelWidgets {
        let canvas = DrawingArea::new();
        canvas.set_focusable(true);
        let scroller = ScrolledWindow::builder()
            .child(&canvas)
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .build();

        // The control's own toolbar (`ComicPagesView.toolStrip`):
        // the Views split button (main click cycles the mode — the
        // C# `tbbView_ButtonClick`; the chevron opens the drop).
        // Click handlers wire after the state exists (the cycle
        // reads the current mode).
        let views_drop = menubar::build_dropdown(PAGES_VIEWS, window);
        let views_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        views_box.add_css_class("linked");
        let (views_btn, _views_icon) = icon_button("View", "Change how Books are displayed");
        let chevron = gtk4::Button::from_icon_name("pan-down-symbolic");
        chevron.set_tooltip_text(Some("Change how Books are displayed"));
        chevron.add_css_class("flat");
        views_box.append(&views_btn);
        views_box.append(&chevron);
        let _ = _views_icon;

        let toolbar = gtk4::Box::new(gtk4::Orientation::Horizontal, 2);
        toolbar.add_css_class("toolbar");
        toolbar.append(&views_box);

        let widget = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        widget.append(&toolbar);
        widget.append(&scroller);

        let (tx, rx) = std::sync::mpsc::channel::<PageDone>();
        let activation: ActivateCell = Rc::new(RefCell::new(None));

        let state = Rc::new(RefCell::new(PagesState {
            bound: None,
            pool,
            thumb_rx: rx,
            thumb_tx: tx,
            thumbs: HashMap::new(),
            queued: std::collections::HashSet::new(),
            pending: 0,
            pump_active: false,
            thumb_height: 128.0,
            current_page: 0,
            mode: PagesMode::Thumbnail,
            last_content_height: 0.0,
            canvas: canvas.clone(),
        }));

        // The main click: cycle the modes through the shell action
        // (Thumbnail → Tile → Thumbnail; the C# cycles three).
        {
            let state = state.clone();
            let window = window.clone();
            views_btn.connect_clicked(move |_| {
                let next = {
                    let s = state.borrow();
                    match s.mode {
                        PagesMode::Thumbnail => PagesMode::Tile,
                        PagesMode::Tile => PagesMode::Thumbnail,
                    }
                };
                let _ = gtk4::prelude::WidgetExt::activate_action(
                    &window,
                    "win.pages-view-mode",
                    Some(&next.action_name().to_variant()),
                );
            });
        }
        // The chevron: open the drop (parented to the anchor first —
        // the T5 lesson).
        {
            let drop = views_drop.clone();
            let main = views_btn.clone();
            chevron.connect_clicked(move |_| drop.open(&main));
        }

        let panel = PagesPanel {
            state: Rc::clone(&state),
            activation: Rc::clone(&activation),
            canvas: canvas.clone(),
            scroller: scroller.clone(),
            views_drop,
            views_btn,
        };

        // The draw function (content coordinates; the canvas is
        // sized to the content and scrolls under GTK).
        {
            let state = Rc::downgrade(&state);
            let scroller = scroller.clone();
            let canvas_for_draw = canvas.clone();
            canvas.set_draw_func(move |_, ctx, _w, _h| {
                let Some(state) = state.upgrade() else {
                    return;
                };
                let v = scroller.vadjustment();
                let window = (v.value(), v.page_size());
                let queued = draw_frame(ctx, &state, window);
                if queued {
                    start_thumb_pump(&state, &canvas_for_draw);
                }
            });
        }

        // Re-flow when the panel first gets its real size (the
        // binding can land while the tab is hidden — width 0).
        {
            let state = Rc::downgrade(&state);
            let canvas = canvas.clone();
            canvas.connect_resize(move |canvas, _width, _height| {
                let Some(state) = state.upgrade() else {
                    return;
                };
                let width = canvas.width() as f64;
                let content = state.borrow_mut().content_height(width);
                canvas.set_content_height(content as i32);
                canvas.queue_draw();
            });
        }

        // Redraw on scroll.
        {
            let canvas = canvas.clone();
            scroller.vadjustment().connect_value_changed(move |_| {
                canvas.queue_draw();
            });
        }

        // The thumb pump (the ItemView shape: lives while loads are
        // in flight).
        {
            let state = Rc::downgrade(&state);
            glib::timeout_add_local(std::time::Duration::from_millis(10), move || {
                let Some(state) = state.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                let mut got = false;
                loop {
                    let next = state.borrow().thumb_rx.try_recv();
                    match next {
                        Ok(done) => {
                            got = true;
                            let mut s = state.borrow_mut();
                            s.pending = s.pending.saturating_sub(1);
                            // Drop stale completions: a load queued
                            // for the PREVIOUS comic must not land in
                            // the current map (the reader-tab flip
                            // race).
                            let current = s.bound.as_ref().map(|(p, _)| p.clone());
                            let fresh = current.as_deref() == Some(done.source.as_str());
                            let surface = if fresh {
                                done.bytes.and_then(|bytes| decode_surface(&bytes))
                            } else {
                                None
                            };
                            if let Some(surface) = surface {
                                s.thumbs.insert(done.page, surface);
                            }
                        }
                        Err(_) => break,
                    }
                }
                if got {
                    state.borrow().canvas.queue_draw();
                }
                let more = state.borrow().pending > 0;
                if !more {
                    state.borrow_mut().pump_active = false;
                }
                if got || more {
                    glib::ControlFlow::Continue
                } else {
                    glib::ControlFlow::Break
                }
            });
        }

        // Click: select + double-click activate; the activation
        // navigates the reader (`ItemView_ItemActivate`).
        {
            let state = Rc::downgrade(&state);
            let canvas_for_grab = canvas.clone();
            let gesture = GestureClick::new();
            gesture.set_button(1);
            gesture.connect_pressed(move |gesture, n, x, y| {
                let Some(state) = state.upgrade() else {
                    return;
                };
                gesture.set_state(gtk4::EventSequenceState::Claimed);
                let mut s = state.borrow_mut();
                let width = s.canvas.width() as f64;
                let placed = s.relayout(width);
                let hit = placed
                    .iter()
                    .find(|(px, py, w, h, _)| x >= *px && x < px + w && y >= *py && y < py + h)
                    .map(|(_, _, _, _, page)| *page);
                if let Some(page) = hit {
                    s.current_page = page;
                }
                drop(s);
                canvas_for_grab.grab_focus();
                if n > 1 {
                    // Double-click → navigate (`ItemView_ItemActivate`).
                    // The callback runs with NO state borrow — it
                    // re-enters this panel through the reader's page
                    // callback.
                    if let Some(page) = hit {
                        if let Some(f) = activation.borrow().as_ref() {
                            f(page);
                        }
                    }
                }
                canvas_for_grab.queue_draw();
            });
            canvas.add_controller(gesture);
        }

        // Ctrl+wheel resize (`PagesView.ItemViewMouseWheel`: steps
        // 16 within [96, 512]; both grid modes scale — the tile cell
        // reads the thumb height). Without Ctrl the event proceeds
        // (the panel scrolls).
        {
            let state = Rc::downgrade(&state);
            let controller = EventControllerScroll::new(
                gtk4::EventControllerScrollFlags::VERTICAL
                    | gtk4::EventControllerScrollFlags::DISCRETE,
            );
            controller.connect_scroll(move |controller, _dx, dy| {
                let Some(state) = state.upgrade() else {
                    return glib::Propagation::Proceed;
                };
                let ctrl = controller
                    .current_event_state()
                    .contains(gdk::ModifierType::CONTROL_MASK);
                if !ctrl {
                    return glib::Propagation::Proceed;
                }
                let step = if dy < 0.0 { 16.0 } else { -16.0 };
                {
                    let mut s = state.borrow_mut();
                    s.thumb_height = (s.thumb_height + step).clamp(MIN_THUMB, MAX_THUMB);
                    let width = s.canvas.width() as f64;
                    let content = s.content_height(width);
                    s.canvas.set_content_height(content as i32);
                }
                state.borrow().canvas.queue_draw();
                glib::Propagation::Stop
            });
            canvas.add_controller(controller);
        }

        PagesPanelWidgets { widget, panel }
    }

    /// Binds the open comic (the C# `PagesView.Book` setter — a
    /// full refill).
    pub fn set_book(&self, book: ComicBook) {
        let path = book.file_path.clone();
        let cells: Vec<PageCell> = book
            .info
            .pages
            .iter()
            .enumerate()
            .map(|(page, info)| {
                let image_index = if info.image_index() >= 0 {
                    info.image_index() as usize
                } else {
                    page
                };
                let aspect = if info.image_width > 0 && info.image_height > 0 {
                    f64::from(info.image_width) / f64::from(info.image_height)
                } else {
                    2.0 / 3.0
                };
                PageCell {
                    page,
                    image_index,
                    aspect,
                    bookmarked: info.bookmark.is_some(),
                    type_name: page_type_text(info.page_type),
                    file_size: info.image_file_size,
                    width: i32::from(info.image_width),
                    height: i32::from(info.image_height),
                    rotation: info.rotation,
                    bookmark: info.bookmark.clone(),
                }
            })
            .collect();
        let width = self.state.borrow().canvas.width() as f64;
        {
            let mut s = self.state.borrow_mut();
            s.bound = Some((path, cells));
            s.thumbs.clear();
            s.queued.clear();
            s.pending = 0;
            s.current_page = 0;
            s.relayout(width);
        }
        self.update_size_request();
        self.canvas.queue_draw();
    }

    /// Clears the binding (the reader closed the comic — the C#
    /// hides the panel).
    pub fn clear_book(&self) {
        let mut s = self.state.borrow_mut();
        s.bound = None;
        s.thumbs.clear();
        s.queued.clear();
        s.pending = 0;
        s.current_page = 0;
        self.canvas.queue_draw();
    }

    pub fn has_book(&self) -> bool {
        self.state.borrow().bound.is_some()
    }

    /// Reflows with the current allocation — the panel's tab may
    /// have been hidden through its first binding (width 0 collapses
    /// the layout; the draw-path self-correction covers the rest).
    pub fn reflow(&self) {
        let width = self.state.borrow().canvas.width() as f64;
        if width < 2.0 {
            return;
        }
        let content = self.state.borrow_mut().content_height(width);
        self.canvas.set_content_height(content as i32);
        self.canvas.queue_draw();
    }

    /// The reader's current page — highlight + scroll into view
    /// (`Navigation` → `EnsureVisible`).
    pub fn set_current_page(&self, page: usize) {
        let width = self.state.borrow().canvas.width() as f64;
        let (rect, content) = {
            let mut s = self.state.borrow_mut();
            s.current_page = page;
            let placed = s.relayout(width);
            let rect = placed
                .iter()
                .find(|(_, _, _, _, p)| *p == page)
                .map(|(x, y, w, h, _)| (*x, *y, *w, *h));
            (rect, s.content_height(width))
        };
        self.update_size_request();
        if let Some((x, y, _w, h)) = rect {
            let adj = self.scroller.vadjustment();
            let view_h = adj.page_size();
            if y < adj.value() {
                adj.set_value((y - 4.0).max(0.0));
            } else if y + h > adj.value() + view_h {
                adj.set_value(y + h - view_h + 4.0);
            }
            let _ = (x, content);
        }
        self.canvas.queue_draw();
    }

    /// Ctrl+wheel resize parity (the C# steps 16 within [96, 512]).
    pub fn resize(&self, delta: f64) {
        let h = {
            let s = self.state.borrow();
            (s.thumb_height + delta).clamp(MIN_THUMB, MAX_THUMB)
        };
        self.state.borrow_mut().thumb_height = h;
        let width = self.state.borrow().canvas.width() as f64;
        let content = self.state.borrow_mut().content_height(width);
        self.canvas.set_content_height(content as i32);
        self.canvas.queue_draw();
    }

    pub fn connect_activate<F: Fn(usize) + 'static>(&self, f: F) {
        *self.activation.borrow_mut() = Some(Box::new(f));
    }

    /// Sets the grid mode (the `win.pages-view-mode` handler).
    pub fn set_mode(&self, mode: PagesMode) {
        self.state.borrow_mut().mode = mode;
        self.reflow();
        self.canvas.queue_draw();
    }

    /// The current grid mode (the sync's source of truth).
    pub fn mode(&self) -> PagesMode {
        self.state.borrow().mode
    }

    /// Applies the action states to the Views drop (the shell's
    /// shared resolve closure).
    pub fn sync(&self, resolve: &dyn Fn(&str) -> Option<menubar::ActionState>) {
        self.views_drop.sync(resolve);
    }

    /// Opens the Views drop through its anchor (the probe's real
    /// open path — the T5 OPEN gate).
    pub fn open_views(&self) -> bool {
        self.views_drop.open(&self.views_btn);
        self.views_drop.popover().is_mapped()
    }

    /// Closes the Views drop (the probe cleanup).
    pub fn close_views(&self) {
        self.views_drop.popover().popdown();
    }

    /// Clicks a Views radio row through the real handler (the probe).
    pub fn click_view(&self, action: &str) -> bool {
        self.views_drop.click_row(action)
    }

    /// Clicks the Views MAIN part (the mode cycle — the probe walks
    /// the real handler).
    pub fn click_main(&self) {
        self.views_btn.emit_clicked();
    }

    fn update_size_request(&self) {
        let width = self.state.borrow().canvas.width() as f64;
        let content = self.state.borrow_mut().content_height(width);
        self.canvas.set_content_height(content as i32);
    }
}

/// Draws one frame; returns whether new page loads were queued (the
/// caller starts the pump outside the borrow — a dead pump strands
/// every completion in the channel, the ItemView lesson).
fn draw_frame(
    ctx: &cairo::Context,
    state: &Rc<RefCell<PagesState>>,
    (scroll_y, view_h): (f64, f64),
) -> bool {
    let mut resize_after = false;
    let (path, cells) = {
        let binding = state.borrow();
        match binding.bound.as_ref() {
            Some((p, c)) => (p.clone(), c.clone()),
            None => return false,
        }
    };
    let mut s = state.borrow_mut();
    let mut queued_thumbs = false;
    let (bg_r, bg_g, bg_b) = BG;
    ctx.set_source_rgb(bg_r, bg_g, bg_b);
    ctx.paint().ok();

    let width = s.canvas.width() as f64;
    let placed = s.relayout(width);

    // The self-correction: the panel may have been bound while
    // hidden (width 0) — the real layout height applies now, and the
    // canvas resize lands after this draw.
    let content = s.content_height(width);
    if (content - s.last_content_height).abs() > 0.5 {
        s.last_content_height = content;
        resize_after = true;
    }

    // Queue thumb loads for the visible cells (AddToTop semantics —
    // the demanded pages skip the line).
    for (x, y, w, h, page) in &placed {
        let visible = *y + *h >= scroll_y && *y <= scroll_y + view_h;
        if !visible || s.queued.contains(page) {
            continue;
        }
        let Some(cell) = cells.iter().find(|c| c.page == *page) else {
            continue;
        };
        let _ = (x, w);
        s.queued.insert(*page);
        s.pending += 1;
        queued_thumbs = true;
        let key = ThumbnailKey::new(ImageKey::from_file(
            path.clone(),
            std::path::Path::new(&path),
            cell.image_index,
            cr_core::model::enums::ImageRotation::None,
        ));
        let pool = Arc::clone(&s.pool);
        let tx = s.thumb_tx.clone();
        let page_no = *page;
        let source = path.clone();
        s.pool.add_thumb_to_queue(key.clone(), None, move |k| {
            let bytes = pool.render_thumbnail(k);
            let _ = tx.send(PageDone {
                page: page_no,
                bytes,
                source: source.clone(),
            });
        });
    }

    // Draw the visible cells.
    let tile = s.mode == PagesMode::Tile;
    for (x, y, w, h, page) in &placed {
        if !(*y + *h >= scroll_y && *y <= scroll_y + view_h) {
            continue;
        }
        let selected = s.current_page == *page;
        let thumb = s.thumbs.get(page).cloned();
        if tile {
            // The tile cell (`ThumbTileRenderer.DrawTile`): the thumb
            // left, the text lines right, one border around the cell.
            let image_w = w * 0.45;
            if thumb.is_none() {
                ctx.set_source_rgb(0.08, 0.08, 0.09);
                ctx.rectangle(x + 8.0, y + 8.0, image_w - 16.0, h - 16.0);
                ctx.fill().ok();
            }
            super::item::draw_cover(ctx, thumb.as_ref(), (*x, *y, image_w, *h), selected);
            let cell = cells.iter().find(|c| c.page == *page);
            draw_tile_text(ctx, (*x + image_w, *y, w - image_w, *h), cell);
            // The cell border (the selection/hot frame).
            ctx.set_source_rgb(
                if selected { FOCUS_UNFOCUSED.0 } else { 0.25 },
                if selected { FOCUS_UNFOCUSED.1 } else { 0.25 },
                if selected { FOCUS_UNFOCUSED.2 } else { 0.25 },
            );
            ctx.set_line_width(1.0);
            ctx.rectangle(*x - 1.0, *y - 1.0, w + 2.0, h + 2.0);
            ctx.stroke().ok();
        } else {
            if thumb.is_none() {
                ctx.set_source_rgb(0.08, 0.08, 0.09);
                ctx.rectangle(x + 8.0, y + 8.0, w - 16.0, h - 16.0);
                ctx.fill().ok();
            }
            super::item::draw_cover(ctx, thumb.as_ref(), (*x, *y, *w, *h), selected);
            // The page-number badge (`DrawPageNumber`: 1-based, top
            // right, black 75% rounded, white text).
            super::item::draw_page_number(ctx, (*x, *y, *w, *h), page + 1);
            // The bookmark pennant (display-only; the editor is
            // Phase 5).
            if let Some(cell) = cells.iter().find(|c| c.page == *page) {
                if cell.bookmarked {
                    super::item::draw_bookmark_h(ctx, (*x, *y, *w, *h));
                }
            }
            if selected {
                ctx.set_source_rgb(FOCUS_UNFOCUSED.0, FOCUS_UNFOCUSED.1, FOCUS_UNFOCUSED.2);
                ctx.set_line_width(1.0);
                ctx.rectangle(*x - 1.0, *y - 1.0, w + 2.0, h + 2.0);
                ctx.stroke().ok();
            }
        }
    }

    if resize_after {
        let canvas = s.canvas.clone();
        let height = s.last_content_height;
        glib::idle_add_local(move || {
            canvas.set_content_height(height as i32);
            canvas.queue_draw();
            glib::ControlFlow::Break
        });
    }

    queued_thumbs
}

/// The thumbnail-completion pump (the ItemView shape): started by
/// the draw path whenever loads are in flight.
fn start_thumb_pump(state: &Rc<RefCell<PagesState>>, canvas: &DrawingArea) {
    {
        let s = state.borrow();
        if s.pump_active {
            return;
        }
    }
    {
        let mut s = state.borrow_mut();
        s.pump_active = true;
    }
    let state = Rc::downgrade(state);
    glib::timeout_add_local(std::time::Duration::from_millis(10), move || {
        let Some(state) = state.upgrade() else {
            return glib::ControlFlow::Break;
        };
        let mut got = false;
        loop {
            let next = state.borrow().thumb_rx.try_recv();
            match next {
                Ok(done) => {
                    got = true;
                    let mut s = state.borrow_mut();
                    s.pending = s.pending.saturating_sub(1);
                    if let Some(surface) = done.bytes.and_then(|bytes| decode_surface(&bytes)) {
                        s.thumbs.insert(done.page, surface);
                    }
                }
                Err(_) => break,
            }
        }
        if got {
            state.borrow().canvas.queue_draw();
        }
        let more = state.borrow().pending > 0;
        if !more {
            state.borrow_mut().pump_active = false;
        }
        if got || more {
            glib::ControlFlow::Continue
        } else {
            glib::ControlFlow::Break
        }
    });
    let _ = canvas;
}

/// A 16 px flat icon button with a tooltip (the split button's main
/// part).
fn icon_button(icon: &'static str, tooltip: &str) -> (gtk4::Button, gtk4::Image) {
    let button = gtk4::Button::new();
    let image = gtk4::Image::new();
    image.set_pixel_size(16);
    if let Some(texture) = crate::icon::icon(icon) {
        image.set_paintable(Some(&texture));
    }
    button.set_child(Some(&image));
    button.set_tooltip_text(Some(tooltip));
    button.add_css_class("flat");
    (button, image)
}

/// The `PageTypeAsText` text: the enum member name (the English
/// default of `LocalizeUtility.LocalizeEnum`); 0 maps to Story (the
/// C# effective-type getter).
fn page_type_text(t: cr_core::model::enums::ComicPageType) -> String {
    if t.0 == 0 {
        "Story".to_string()
    } else {
        t.to_xml()
    }
}

/// The page tile text lines (`ComicTextBuilder.GetTextBlocks`,
/// `ComicTextElements.DefaultPage`): (text, font scale, bold). The
/// tab-stop lines carry a `"\t"` the renderer splits into the
/// two-column block.
fn tile_lines(cell: &PageCell) -> Vec<(String, f64, bool)> {
    let mut lines: Vec<(String, f64, bool)> = vec![
        (format!("Page #{}", cell.page + 1), 1.0, true),
        (cell.type_name.clone(), 0.95, false),
        (String::new(), 0.95, false), // the 10 px spacer
    ];
    if cell.file_size > 0 {
        lines.push((
            format!(
                "Size:\t{}",
                cr_engine::display_text::file_size_as_text(i64::from(cell.file_size))
            ),
            0.95,
            false,
        ));
    } else {
        lines.push(("Unknown Size".to_string(), 0.95, false));
    }
    lines.push((
        format!("Resolution:\t{} x {}", cell.width, cell.height),
        0.95,
        false,
    ));
    if cell.rotation != ImageRotation::None {
        let deg = match cell.rotation {
            ImageRotation::Rotate90 => 90,
            ImageRotation::Rotate180 => 180,
            ImageRotation::Rotate270 => 270,
            ImageRotation::None => 0,
        };
        lines.push((format!("Rotation:\t{deg}°"), 0.95, false));
    }
    lines.push((String::new(), 0.95, false)); // the 6 px spacer
    if let Some(name) = &cell.bookmark {
        lines.push((format!("Bookmark:\t{name}"), 0.95, false));
    }
    lines
}

/// Draws the text block of one tile cell (the two-column tab-stop
/// shape the ItemView tiles use).
fn draw_tile_text(
    ctx: &cairo::Context,
    (x, y, w, h): (f64, f64, f64, f64),
    cell: Option<&PageCell>,
) {
    let Some(cell) = cell else {
        return;
    };
    ctx.save().ok();
    ctx.rectangle(x, y, w, h);
    ctx.clip();
    ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    ctx.set_font_size(9.0);
    // The shared tab stop: the widest first-segment width + 4 px.
    let lines = tile_lines(cell);
    let mut tab = 0.0f64;
    for (text, _, _) in &lines {
        if let Some(pos) = text.find('\t') {
            if let Ok(ext) = ctx.text_extents(&text[..pos]) {
                tab = tab.max(ext.width());
            }
        }
    }
    let tab = tab + 4.0;
    let mut ty = y + 4.0;
    for (text, scale, bold) in &lines {
        ctx.select_font_face(
            "Sans",
            cairo::FontSlant::Normal,
            if *bold {
                cairo::FontWeight::Bold
            } else {
                cairo::FontWeight::Normal
            },
        );
        ctx.set_font_size(9.0 * scale);
        let line_h = super::item::line_height(ctx).max(2.0);
        if ty + line_h > y + h {
            break;
        }
        if text.is_empty() {
            ty += line_h * 0.5;
            continue;
        }
        ctx.set_source_rgb(0.9, 0.9, 0.92);
        ctx.move_to(x + 4.0, ty + line_h * 0.85);
        if let Some(pos) = text.find('\t') {
            ctx.show_text(&text[..pos]).ok();
            ctx.move_to(x + 4.0 + tab, ty + line_h * 0.85);
            ctx.show_text(&text[pos + 1..]).ok();
        } else {
            ctx.show_text(text).ok();
        }
        ty += line_h;
    }
    ctx.restore().ok();
}

fn decode_surface(bytes: &[u8]) -> Option<cairo::ImageSurface> {
    // The pool caches the C# `ThumbnailImage` serialization (size
    // header + JPEG data) — parse, then decode the JPEG.
    let jpeg = cr_image::thumbnail::Thumbnail::from_bytes(bytes)
        .map(|t| t.data)
        .unwrap_or_else(|_| bytes.to_vec());
    let img = cr_image::decode::decode(&jpeg).ok()?;
    let stride = img.width as usize * 4;
    let mut argb = vec![0u8; stride * img.height as usize];
    for (src, dst) in rgba_chunks(&img.rgba).zip(argb.as_chunks_mut::<4>().0.iter_mut()) {
        dst[0] = src[2];
        dst[1] = src[1];
        dst[2] = src[0];
        dst[3] = src[3];
    }
    Some(
        cairo::ImageSurface::create_for_data(
            argb,
            cairo::Format::ARgb32,
            img.width as i32,
            img.height as i32,
            stride as i32,
        )
        .expect("surface"),
    )
}

fn rgba_chunks(rgba: &[u8]) -> impl Iterator<Item = &[u8; 4]> {
    rgba.as_chunks::<4>().0.iter()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(page: usize) -> PageCell {
        PageCell {
            page,
            image_index: page,
            aspect: 2.0 / 3.0,
            bookmarked: false,
            type_name: "Story".to_string(),
            file_size: 0,
            width: 800,
            height: 1200,
            rotation: ImageRotation::None,
            bookmark: None,
        }
    }

    /// The tile text lines (`ComicTextBuilder.GetTextBlocks`,
    /// `ComicTextElements.DefaultPage`).
    #[test]
    fn tile_lines_match_the_csharp_blocks() {
        let lines = tile_lines(&cell(4));
        assert_eq!(lines[0], ("Page #5".to_string(), 1.0, true));
        assert_eq!(lines[1].0, "Story");
        // An unknown size renders the literal (the C# UnknownSizeText).
        assert_eq!(lines[3].0, "Unknown Size");
        assert_eq!(lines[4].0, "Resolution:\t800 x 1200");
        // No rotation, no bookmark → no further lines but the spacer.
        assert_eq!(lines.len(), 6);
    }

    #[test]
    fn tile_lines_carry_size_rotation_bookmark() {
        let mut c = cell(0);
        c.file_size = 250_000;
        c.rotation = ImageRotation::Rotate90;
        c.bookmark = Some("fight".to_string());
        let lines = tile_lines(&c);
        assert!(lines
            .iter()
            .any(|(t, _, _)| t.starts_with("Size:\t") && t.contains("kB")));
        assert!(lines.iter().any(|(t, _, _)| t == "Rotation:\t90°"));
        assert!(lines.iter().any(|(t, _, _)| t == "Bookmark:\tfight"));
    }

    /// Every Pages-toolbar action exists in the registry (the
    /// COMMANDS table or the shell-only actions).
    #[test]
    fn pages_toolbar_actions_exist() {
        let known: std::collections::HashSet<&str> = crate::commands::COMMANDS
            .iter()
            .map(|c| c.action)
            .chain(["pages-view-mode"])
            .collect();
        for node in PAGES_VIEWS {
            if let Item(_, action, _, _) = node {
                let base = action
                    .split("::")
                    .next()
                    .unwrap()
                    .strip_prefix("win.")
                    .unwrap();
                assert!(
                    known.contains(base),
                    "pages toolbar action {action} has no command"
                );
            }
        }
    }

    /// The toolbar icon names resolve in the bundled set.
    #[test]
    fn pages_toolbar_icons_resolve() {
        for name in ["View", "ThumbView", "TileView"] {
            assert!(crate::icon::path_for_name(name).is_some(), "{name} missing");
        }
    }

    #[test]
    fn pages_mode_names_round_trip() {
        for mode in [PagesMode::Thumbnail, PagesMode::Tile] {
            assert_eq!(PagesMode::from_action_name(mode.action_name()), mode);
        }
    }
}
