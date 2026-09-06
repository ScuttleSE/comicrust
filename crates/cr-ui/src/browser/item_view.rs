//! The ItemView widget — the DrawingArea host over the pure
//! [`super::view_state`] + [`super::layout`] modules (the reader's
//! one-drawing-area house style; GTK list widgets fight the C#
//! owner-draw model — ADR-018 era surface).
//!
//! Ported behavior: the visible-window culling, selection (click /
//! Ctrl / Shift / rubber-band from a snapshot), column-aware
//! keyboard movement, Home/End/PageUp/PageDown, type-ahead (2500 ms
//! buffer, caption prefix), Enter/double-click activation, group
//! header collapse clicks, and cover thumbnails through the
//! ImagePool thumb queues (ADR-019: queue callbacks + a
//! `timeout_add_local` pump; failed covers render the error
//! thumbnail).
//!
//! Deviations: the wheel scrolls natively (the C# steps 16 px per
//! line), Detail headers draw but do not resize or reorder (T5),
//! and Tile text lines are the caption + file name until the C#
//! text builder lands (T4).

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    cairo, DrawingArea, EventControllerKey, EventControllerMotion, GestureClick, ScrolledWindow,
};

use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::CrGuid;
use cr_engine::image_pool::ImagePool;
use cr_image::keys::{ImageKey, ThumbnailKey};

use super::columns::{self, Column};
use super::layout::{
    self, hit_group_header, hit_test, page_step_display, relative_item, visible_items, ItemLayout,
    ItemViewMode, LayoutConfig, Rect,
};
use super::view_state::ViewState;

/// The type-ahead buffer reset (`KeySearch`).
const TYPE_AHEAD_RESET_MS: u64 = 2500;

/// The focus ring when the view does not hold focus (a neutral gray
/// that reads on both themes — the palette colors cover the rest).
const FOCUS_UNFOCUSED: (f64, f64, f64) = (0.5, 0.5, 0.55);
/// The Ctrl+wheel size limits + step (`Program.Min/MaxThumbHeight`,
/// the wheel steps 16 — `itemView_MouseWheel`).
const THUMB_WHEEL_STEP: f64 = 16.0;

type ActivateFn = Box<dyn Fn(&CrGuid)>;
type SelectionFn = Box<dyn Fn(usize)>;
type HeaderContextFn = Rc<dyn Fn(f64, f64)>;

struct ThumbDone {
    book_id: CrGuid,
    bytes: Option<Vec<u8>>,
}

/// std `Sender` is not Sync — the wrapper (the `PageTx` pattern).
#[derive(Clone)]
struct ThumbTx(Arc<Mutex<std::sync::mpsc::Sender<ThumbDone>>>);

impl ThumbTx {
    fn send(&self, done: ThumbDone) {
        if let Ok(tx) = self.0.lock() {
            let _ = tx.send(done);
        }
    }
}

enum ThumbState {
    Ready(cairo::ImageSurface),
    Failed,
}

pub struct ItemViewState {
    view: ViewState,
    config: LayoutConfig,
    layout: ItemLayout,
    pool: Arc<ImagePool>,
    thumb_rx: std::sync::mpsc::Receiver<ThumbDone>,
    thumb_tx: ThumbTx,
    thumbs: HashMap<CrGuid, ThumbState>,
    queued: HashSet<CrGuid>,
    /// The rendered captions/derived text per book (the proposed-name
    /// fallback parses file names with regexes — never per frame).
    captions: HashMap<CrGuid, String>,
    /// The Detail cell texts per book, aligned with the visible
    /// columns (the same regex hazard as the captions — the Detail
    /// view draws visible_rows × columns cells per frame).
    detail_texts: HashMap<CrGuid, Vec<String>>,
    /// The Tile text lines per book (the same hazard —
    /// `tile_text_lines` resolves proposed names through regexes).
    tile_texts: HashMap<CrGuid, Vec<(String, f64, bool)>>,
    /// The RENDERED tile segments per book — tab stop resolved, long
    /// lines truncated (a char-by-char trim per frame measured
    /// hundreds of text_extents calls per row on summary lines).
    tile_render: HashMap<CrGuid, Vec<TileSeg>>,
    /// The text-block width the segment cache was built for.
    tile_render_width: f64,
    /// Loads in flight (the pump stays alive while this is > 0).
    pending_thumbs: usize,
    pump_active: bool,
    band: Option<Rect>,
    band_start: (f64, f64),
    /// The selection at band start (Ctrl-drag flips from it,
    /// `UpdateSelection`).
    band_snapshot: HashSet<CrGuid>,
    detail_columns: Vec<Column>,
    on_activate: Option<ActivateFn>,
    on_selection_changed: Option<SelectionFn>,
    /// The Detail header right-click (the T6 column chooser).
    on_header_context: Option<HeaderContextFn>,
    type_ahead: String,
    type_ahead_source: Option<glib::SourceId>,
    canvas: DrawingArea,
}

impl ItemViewState {
    fn relayout(&mut self, canvas_width: f64) {
        if canvas_width > 1.0 {
            self.config.view_width = canvas_width;
        }
        // The Detail column strip follows the column table (the C#
        // `GetColumnHeadersWidth`).
        self.config.column_widths = self
            .detail_columns
            .iter()
            .filter(|c| c.visible)
            .map(|c| c.width)
            .collect();
        self.layout = layout::compute(&self.view, &self.config);
    }

    fn notify_selection(&self) {
        if let Some(f) = self.on_selection_changed.as_ref() {
            f(self.view.selection().len());
        }
    }

    fn activate_focus(&self) {
        if let Some(id) = self.view.focus() {
            if let Some(f) = self.on_activate.as_ref() {
                f(&id);
            }
        }
    }

    /// Queues cover loads for the visible items (`AddToTop` — the
    /// demanded thumbs skip the line).
    fn queue_visible_thumbs(&mut self, window: Rect) {
        let wanted: Vec<(CrGuid, String)> = visible_items(&self.layout, window)
            .filter_map(|item| {
                let book = self.view.book(item.display);
                if self.queued.contains(&book.id) {
                    return None;
                }
                Some((book.id, book.file_path.clone()))
            })
            .collect();
        for (id, path) in wanted {
            self.queued.insert(id);
            self.pending_thumbs += 1;
            let key = ThumbnailKey::new(ImageKey::from_file(
                path.clone(),
                std::path::Path::new(&path),
                0,
                cr_core::model::enums::ImageRotation::None,
            ));
            let pool = Arc::clone(&self.pool);
            let tx = self.thumb_tx.clone();
            self.pool.add_thumb_to_queue(key.clone(), None, move |k| {
                let bytes = pool.render_thumbnail(k);
                tx.send(ThumbDone { book_id: id, bytes });
            });
        }
    }

    fn pump_needed(&self) -> bool {
        self.pending_thumbs > 0
    }

    /// The caption line (`Comic.Caption`), cached per book — the
    /// proposed-name fallback parses file names with regexes and must
    /// never run per draw frame.
    fn caption(&mut self, display: usize) -> String {
        let book = self.view.book(display);
        let id = book.id;
        if let Some(cached) = self.captions.get(&id) {
            return cached.clone();
        }
        let text = cr_engine::display_text::caption(book);
        self.captions.insert(id, text.clone());
        text
    }
}

/// The clone-able handle (the widget tree keeps it alive — the host
/// stores it).
/// The handle is Clone; every capture keeps the widget alive (the
/// `Rc` must outlive the window).
#[derive(Clone)]
pub struct ItemView {
    state: Rc<RefCell<ItemViewState>>,
    canvas: DrawingArea,
}

pub struct ItemViewWidgets {
    pub scroller: ScrolledWindow,
    pub view: ItemView,
}

impl ItemView {
    /// Builds the widget pair (the scroller is the pane child; the
    /// handle drives the state). Named after the constructed pair —
    /// `ItemView::create` reads better than a `Self`-returning new.
    pub fn create(pool: Arc<ImagePool>) -> ItemViewWidgets {
        let canvas = DrawingArea::new();
        canvas.set_focusable(true);
        // The palette is resolved per frame from the theme colors —
        // a dark/light flip must re-draw (GTK does not invalidate a
        // custom cairo draw on a theme change).
        crate::theme::redraw_on_theme_change(&canvas);
        let scroller = ScrolledWindow::builder()
            .child(&canvas)
            .hscrollbar_policy(gtk4::PolicyType::Automatic)
            .vscrollbar_policy(gtk4::PolicyType::Automatic)
            .hexpand(true)
            .vexpand(true)
            .build();

        let (tx, rx) = std::sync::mpsc::channel::<ThumbDone>();
        let thumb_tx = ThumbTx(Arc::new(Mutex::new(tx)));

        let state = Rc::new(RefCell::new(ItemViewState {
            view: ViewState::default(),
            config: LayoutConfig::default(),
            layout: ItemLayout::default(),
            pool: Arc::clone(&pool),
            thumb_rx: rx,
            thumb_tx,
            thumbs: HashMap::new(),
            queued: HashSet::new(),
            captions: HashMap::new(),
            detail_texts: HashMap::new(),
            tile_texts: HashMap::new(),
            tile_render: HashMap::new(),
            tile_render_width: 0.0,
            pending_thumbs: 0,
            pump_active: false,
            band: None,
            band_start: (0.0, 0.0),
            band_snapshot: HashSet::new(),
            detail_columns: columns::default_columns(),
            on_activate: None,
            on_selection_changed: None,
            on_header_context: None,
            type_ahead: String::new(),
            type_ahead_source: None,
            canvas: canvas.clone(),
        }));

        let iv = ItemView {
            state: Rc::clone(&state),
            canvas: canvas.clone(),
        };

        // The draw function (content coordinates — the canvas is
        // sized to the virtual size and scrolls under GTK).
        {
            let state = Rc::downgrade(&state);
            let scroller = scroller.clone();
            canvas.set_draw_func(move |_, ctx, width, height| {
                let Some(state) = state.upgrade() else {
                    return;
                };
                let (sx, sy, view_w, view_h) = scroll_window(&scroller);
                let window = Rect::new(sx, sy, view_w.max(width as f64), view_h.max(height as f64));
                let queued = draw_frame(ctx, &state, window);
                if queued {
                    start_thumb_pump(&state);
                }
            });
        }

        // Redraw on scroll.
        for adj in [scroller.hadjustment(), scroller.vadjustment()] {
            let canvas = canvas.clone();
            adj.connect_value_changed(move |_| canvas.queue_draw());
        }

        // The thumb pump is NOT started here: the draw path starts it
        // whenever loads are in flight (a dead pump would strand
        // every late completion in the channel).

        // Mouse: select / band / activate / group toggle.
        iv.install_click_controller();
        iv.install_key_controller();

        // Ctrl+wheel resize (`ComicBrowserControl.
        // itemView_MouseWheel`: steps 16 within [96, 512]; Detail
        // mode keeps scrolling — the C# `ItemViewMode != Detail`
        // gate).
        {
            let state = Rc::downgrade(&state);
            let iv_for_wheel = iv.clone();
            let controller = gtk4::EventControllerScroll::new(
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
                let detail = state.borrow().config.mode == ItemViewMode::Detail;
                if detail {
                    return glib::Propagation::Proceed;
                }
                let step = if dy < 0.0 {
                    THUMB_WHEEL_STEP
                } else {
                    -THUMB_WHEEL_STEP
                };
                // `SetItemSize(itemSize.Value + delta * 16)` — the
                // current mode's value + the step (Tile scales too).
                let next = {
                    let s = state.borrow();
                    super::layout::item_size_range(&s.config).map(|(_, _, v)| v + step)
                };
                if let Some(v) = next {
                    iv_for_wheel.apply_item_size(v);
                }
                glib::Propagation::Stop
            });
            canvas.add_controller(controller);
        }

        ItemViewWidgets { scroller, view: iv }
    }

    /// Replaces the book set (a library selection change).
    pub fn set_books(&self, books: Vec<ComicBook>) {
        let width = self.state.borrow().config.view_width;
        {
            let mut s = self.state.borrow_mut();
            let filter = s.view.filter_clone();
            s.view = ViewState::new(books);
            s.view.set_filter(filter);
            s.thumbs.clear();
            s.queued.clear();
            s.captions.clear();
            s.detail_texts.clear();
            s.tile_texts.clear();
            s.tile_render.clear();
            s.band = None;
            s.relayout(width);
        }
        // Outside the borrow (the RefCell panics on nested borrows).
        self.update_size_request();
        self.notify_and_redraw();
    }

    pub fn connect_activate<F: Fn(&CrGuid) + 'static>(&self, f: F) {
        self.state.borrow_mut().on_activate = Some(Box::new(f));
    }

    pub fn connect_selection_changed<F: Fn(usize) + 'static>(&self, f: F) {
        self.state.borrow_mut().on_selection_changed = Some(Box::new(f));
        self.notify_and_redraw();
    }

    pub fn view_state(&self) -> ViewState {
        self.state.borrow().view.clone()
    }

    /// Mutates the layout config (view mode, sizes) and reflows
    /// (`ItemViewConfig` changes from the shell menus).
    pub fn configure(&self, f: impl FnOnce(&mut LayoutConfig)) {
        let width = self.state.borrow().config.view_width;
        {
            let mut s = self.state.borrow_mut();
            f(&mut s.config);
            s.relayout(width);
            // The tile segments carry per-cell font sizes — rebuild.
            s.tile_render.clear();
        }
        self.update_size_request();
        self.canvas.queue_draw();
    }

    pub fn mode(&self) -> ItemViewMode {
        self.state.borrow().config.mode
    }

    pub fn thumb_height(&self) -> f64 {
        self.state.borrow().config.thumb_height
    }

    /// The Tile cell height (the T14 persistence reads it).
    pub fn tile_height(&self) -> f64 {
        self.state.borrow().config.tile_size.1
    }

    /// The Detail row height (the T14 persistence reads it).
    pub fn row_height(&self) -> f64 {
        self.state.borrow().config.row_height
    }

    /// `ComicBrowserControl.GetItemSize` — the status-bar slider's
    /// (min, max, value) triple for the current mode.
    pub fn item_size(&self) -> Option<(f64, f64, f64)> {
        let s = self.state.borrow();
        super::layout::item_size_range(&s.config)
    }

    /// `ComicBrowserControl.SetItemSize`: the clamped height per
    /// mode (Thumbnail thumb height, Tile tile height with the width
    /// doubled, Detail row height) + reflow.
    pub fn set_item_size(&self, height: f64) {
        self.apply_item_size(height);
    }

    /// The shared resize path (the public setter and the Ctrl+wheel
    /// handler both land here — the C# wheel calls `SetItemSize`).
    fn apply_item_size(&self, height: f64) {
        let width = self.state.borrow().config.view_width;
        {
            let mut s = self.state.borrow_mut();
            let (h, tile_w) = super::layout::clamp_item_size(s.config.mode, height);
            match s.config.mode {
                ItemViewMode::Thumbnail => s.config.thumb_height = h,
                ItemViewMode::Tile => s.config.tile_size = (tile_w, h),
                ItemViewMode::Detail => s.config.row_height = h,
            }
            s.relayout(width);
            // The tile segments carry per-cell font sizes — rebuild.
            s.tile_render.clear();
        }
        self.update_size_request();
        self.canvas.queue_draw();
    }

    /// The pre-filter book count (`totalCount` — the C# fills it
    /// from the list evaluation before the matcher runs).
    pub fn total_count(&self) -> usize {
        self.state.borrow().view.books().len()
    }

    /// The total file size of the DISPLAYED books (`totalSize` — the
    /// C# sums the matched set while filling the list).
    pub fn visible_size(&self) -> i64 {
        let s = self.state.borrow();
        s.view
            .display_order()
            .iter()
            .map(|&i| s.view.books()[i].file_size.max(0))
            .sum()
    }

    /// The total file size of the selected AND displayed books
    /// (`selectedSize` — the C# sums `SelectedItems`).
    pub fn selected_size(&self) -> i64 {
        let s = self.state.borrow();
        let sel = s.view.selection();
        s.view
            .display_order()
            .iter()
            .filter(|&&i| sel.contains(&s.view.books()[i].id))
            .map(|&i| s.view.books()[i].file_size.max(0))
            .sum()
    }

    /// The quick-search filter (`UpdateQuickFilter`): `None` clears.
    pub fn set_filter(&self, filter: Option<cr_engine::matcher::tree::Matcher>) {
        let width = self.state.borrow().config.view_width;
        {
            let mut s = self.state.borrow_mut();
            s.view.set_filter(filter);
            s.relayout(width);
        }
        self.update_size_request();
        self.notify_and_redraw();
    }

    pub fn book_count(&self) -> usize {
        self.state.borrow().view.len()
    }

    /// The selection size (the enable-state reads it — a view-state
    /// clone would copy every book).
    pub fn selection_len(&self) -> usize {
        self.state.borrow().view.selection().len()
    }

    /// The selected ids (the shell commands read them).
    pub fn selection_ids(&self) -> Vec<CrGuid> {
        self.state
            .borrow()
            .view
            .selection()
            .iter()
            .copied()
            .collect()
    }

    /// Selects one book and reveals it (`IComicBrowser.SelectComic`
    /// parity — the Show-in-Browser path). The redraw keeps the
    /// selection marker in sync; scrolling to the item stays with
    /// the layout work.
    pub fn select_book(&self, id: &CrGuid) {
        let width = self.state.borrow().config.view_width;
        {
            let mut s = self.state.borrow_mut();
            s.view.select_one(*id);
            s.relayout(width);
        }
        self.notify_and_redraw();
    }

    /// Restores a selection after a book-set refresh (`RefreshList`
    /// keeps the selection in the C#): intersect the ids with the
    /// books that still exist, focus follows the first.
    pub fn reselect(&self, ids: &[CrGuid]) {
        let width = self.state.borrow().config.view_width;
        {
            let mut s = self.state.borrow_mut();
            let keep: Vec<CrGuid> = ids
                .iter()
                .filter(|id| s.view.books().iter().any(|b| b.id == **id))
                .copied()
                .collect();
            s.view.restore_selection(&keep);
            s.relayout(width);
        }
        self.notify_and_redraw();
    }

    /// The right-click context menu (`tvQueries_MouseDown` shape):
    /// (item under the cursor, x, y) — the coordinates are TOPLEVEL
    /// (window) coordinates, ready for a popover parented to the
    /// window. A click inside the Detail header strip routes to the
    /// column-chooser hook (`autoHeaderContextMenuStrip`) instead.
    pub fn connect_context<F: Fn(Option<CrGuid>, f64, f64) + 'static>(&self, f: F) {
        let state = Rc::downgrade(&self.state);
        let canvas = self.canvas.clone();
        let gesture = GestureClick::new();
        gesture.set_button(3);
        gesture.connect_pressed(move |gesture, _n, x, y| {
            let Some(state) = state.upgrade() else {
                return;
            };
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            // The header hit test needs the config — read it BEFORE
            // the branch (the if-condition temporaries lesson).
            let header_hit = {
                let s = state.borrow();
                layout::header_visible(&s.config) && y <= s.config.header_height
            };
            if header_hit {
                let hook = state.borrow().on_header_context.clone();
                if let Some(f) = hook {
                    // Translate to the toplevel like the book menu.
                    let (wx, wy) = canvas
                        .ancestor(gtk4::Window::static_type())
                        .and_then(|w| w.downcast::<gtk4::Window>().ok())
                        .and_then(|win| canvas.translate_coordinates(&win, x, y))
                        .unwrap_or((x, y));
                    f(wx, wy);
                    return;
                }
            }
            let s = state.borrow();
            let hit = hit_test(&s.layout, x, y).map(|d| s.view.book_id(d));
            drop(s);
            // The canvas lives inside the pane — translate to the
            // toplevel so a window-parented popover lands under the
            // cursor.
            let (wx, wy) = canvas
                .ancestor(gtk4::Window::static_type())
                .and_then(|w| w.downcast::<gtk4::Window>().ok())
                .and_then(|win| canvas.translate_coordinates(&win, x, y))
                .unwrap_or((x, y));
            f(hit, wx, wy);
        });
        self.canvas.add_controller(gesture);
    }

    /// The Detail header right-click (the column chooser; the C#
    /// `autoHeaderContextMenuStrip_Opening`).
    pub fn connect_header_context<F: Fn(f64, f64) + 'static>(&self, f: F) {
        self.state.borrow_mut().on_header_context = Some(Rc::new(f));
    }

    /// Takes the keyboard focus onto the grid (the window-activation
    /// re-grab — the reader's dead-first-keypress fix).
    pub fn grab_focus(&self) {
        self.canvas.grab_focus();
    }

    /// Header sort click: push/flip the column (`OnHeaderClick`).
    pub fn set_sort_column(&self, column: &str) {
        let width = self.state.borrow().config.view_width;
        {
            let mut s = self.state.borrow_mut();
            s.view.set_sort_column(column);
            s.relayout(width);
        }
        self.canvas.queue_draw();
    }

    /// `ItemSorter = null` (the Arrange menu's Not Sorted row).
    pub fn clear_sort(&self) {
        let width = self.state.borrow().config.view_width;
        {
            let mut s = self.state.borrow_mut();
            s.view.clear_sort();
            s.relayout(width);
        }
        self.canvas.queue_draw();
    }

    pub fn toggle_sort_direction(&self) {
        let width = self.state.borrow().config.view_width;
        {
            let mut s = self.state.borrow_mut();
            s.view.toggle_direction();
            s.relayout(width);
        }
        self.canvas.queue_draw();
    }

    /// The sort direction setter (the T14 restore — the chain's
    /// first key flips, the rest stays).
    pub fn set_sort_direction(&self, descending: bool) {
        let width = self.state.borrow().config.view_width;
        {
            let mut s = self.state.borrow_mut();
            s.view.set_direction(descending);
            s.relayout(width);
        }
        self.canvas.queue_draw();
    }

    pub fn set_grouper(&self, grouper: Option<&'static str>) {
        let width = self.state.borrow().config.view_width;
        {
            let mut s = self.state.borrow_mut();
            s.view.set_grouper(grouper);
            s.relayout(width);
        }
        self.update_size_request();
        self.canvas.queue_draw();
    }

    /// Reveals/hides a Detail column (the header column chooser —
    /// `HeaderMenuItemClicked`).
    pub fn toggle_column_visible(&self, id: i32) {
        let width = self.state.borrow().config.view_width;
        {
            let mut s = self.state.borrow_mut();
            if let Some(c) = s.detail_columns.iter_mut().find(|c| c.id == id) {
                c.visible = !c.visible;
            }
            s.relayout(width);
        }
        self.update_size_request();
        self.canvas.queue_draw();
    }

    /// The column set snapshot (id, name, visible) — the column
    /// chooser fill (the C# header menu lists EVERY column with its
    /// check state).
    pub fn detail_columns_snapshot(&self) -> Vec<(i32, String, bool)> {
        self.state
            .borrow()
            .detail_columns
            .iter()
            .map(|c| (c.id, c.name.to_string(), c.visible))
            .collect()
    }

    /// The persisted Detail column state (id, visible, width) — the
    /// T14 save.
    pub fn detail_columns_state(&self) -> Vec<(i32, bool, i32)> {
        self.state
            .borrow()
            .detail_columns
            .iter()
            .map(|c| (c.id, c.visible, c.width as i32))
            .collect()
    }

    /// Restores the Detail column visibility + widths (the T14
    /// load; unknown ids ignore).
    pub fn set_detail_columns_state(&self, cols: &[(i32, bool, i32)]) {
        let width = self.state.borrow().config.view_width;
        {
            let mut s = self.state.borrow_mut();
            for (id, visible, w) in cols {
                if let Some(c) = s.detail_columns.iter_mut().find(|c| c.id == *id) {
                    c.visible = *visible;
                    if *w > 0 {
                        c.width = f64::from(*w);
                    }
                }
            }
            s.relayout(width);
        }
        self.update_size_request();
        self.canvas.queue_draw();
    }

    /// The current sort/group labels (the toolbar button texts —
    /// the C# `OnIdle` tbbSort/tbbGroup text updates). No book clone.
    pub fn sort_group_summary(&self) -> (Option<String>, bool, Option<&'static str>) {
        let s = self.state.borrow();
        let first = s.view.sort().keys().first();
        (
            first.map(|k| k.column.clone()),
            first.is_some_and(|k| k.descending),
            s.view.grouper(),
        )
    }

    fn update_size_request(&self) {
        update_size_request(&self.state, &self.canvas);
    }

    fn notify_and_redraw(&self) {
        self.state.borrow().notify_selection();
        self.canvas.queue_draw();
    }

    fn install_click_controller(&self) {
        let state = Rc::downgrade(&self.state);
        let canvas = self.canvas.clone();
        let state_released = Rc::downgrade(&self.state);
        let canvas_released = self.canvas.clone();
        let gesture = GestureClick::new();
        gesture.set_button(1);
        gesture.connect_pressed(move |gesture, n, x, y| {
            let Some(state) = state.upgrade() else {
                return;
            };
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            // Take the keyboard focus on click (GTK4 has no
            // click-to-focus — the Phase 3 lesson).
            canvas.grab_focus();
            if n != 1 {
                // Double-click → activate.
                let id = state.borrow().view.focus();
                if let Some(id) = id {
                    if let Some(f) = state.borrow().on_activate.as_ref() {
                        f(&id);
                    }
                }
                return;
            }
            let mut s = state.borrow_mut();
            let hit = hit_test(&s.layout, x, y);
            if let Some(group) = hit_group_header(&s.layout, x, y) {
                let collapsed = !s.view.groups()[group].collapsed;
                s.view.set_collapsed(group, collapsed);
                s.relayout(canvas.width() as f64);
                drop(s);
                update_size_request(&state, &canvas);
                canvas.queue_draw();
                return;
            }
            match hit {
                Some(display) => {
                    let id = s.view.book_id(display);
                    let mods = gesture.current_event_state();
                    let ctrl = mods.contains(gtk4::gdk::ModifierType::CONTROL_MASK);
                    let shift = mods.contains(gtk4::gdk::ModifierType::SHIFT_MASK);
                    if ctrl {
                        s.view.select_flip(id);
                    } else if shift {
                        s.view.select_range(id);
                    } else {
                        s.view.select_one(id);
                    }
                    s.band = None;
                }
                None => {
                    // Rubber band start (the C#: empty background,
                    // no modifiers, multiselect).
                    let mods = gesture.current_event_state();
                    if !mods.contains(gtk4::gdk::ModifierType::CONTROL_MASK)
                        && !mods.contains(gtk4::gdk::ModifierType::SHIFT_MASK)
                    {
                        s.band_start = (x, y);
                        s.band = Some(Rect::new(x, y, 0.0, 0.0));
                        s.band_snapshot = s.view.selection_snapshot();
                        s.view.clear_selection();
                    }
                }
            }
            // The selection callback re-enters this widget
            // (`book_count` on the status bar) — drop the borrow
            // first (the RefCell double-borrow lesson).
            drop(s);
            state.borrow().notify_selection();
            canvas.queue_draw();
        });
        gesture.connect_released(move |gesture, _n, _x, _y| {
            let Some(state) = state_released.upgrade() else {
                return;
            };
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            let mut s = state.borrow_mut();
            if let Some(band) = s.band.take() {
                let band_ids: Vec<CrGuid> = layout::visible_items(&s.layout, band)
                    .map(|i| s.view.book_id(i.display))
                    .collect();
                let snap = s.band_snapshot.clone();
                let mods = gesture.current_event_state();
                s.view.apply_band(
                    &band_ids,
                    &snap,
                    mods.contains(gtk4::gdk::ModifierType::CONTROL_MASK),
                );
                drop(s);
                state_released
                    .upgrade()
                    .expect("state")
                    .borrow()
                    .notify_selection();
                canvas_released.queue_draw();
            }
        });
        self.canvas.add_controller(gesture);

        // Band motion.
        let state = Rc::downgrade(&self.state);
        let motion = EventControllerMotion::new();
        motion.connect_motion(move |_, x, y| {
            let Some(state) = state.upgrade() else {
                return;
            };
            let mut s = state.borrow_mut();
            if s.band.is_some() {
                let (sx, sy) = s.band_start;
                let rect = Rect::new(sx.min(x), sy.min(y), (x - sx).abs(), (y - sy).abs());
                s.band = Some(rect);
                s.canvas.queue_draw();
            }
        });
        self.canvas.add_controller(motion);
    }

    fn install_key_controller(&self) {
        let state = Rc::downgrade(&self.state);
        let canvas = self.canvas.clone();
        let keys = EventControllerKey::new();
        keys.set_propagation_phase(gtk4::PropagationPhase::Bubble);
        keys.connect_key_pressed(move |_, key, _code, modifier| {
            let Some(state) = state.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let mut s = state.borrow_mut();
            let focus = s.view.focus();
            let ctrl = modifier.contains(gtk4::gdk::ModifierType::CONTROL_MASK);
            let shift = modifier.contains(gtk4::gdk::ModifierType::SHIFT_MASK);
            let step = |s: &ItemViewState, f: Option<CrGuid>, dx: i32, dy: i32| -> Option<CrGuid> {
                let d = f.and_then(|f| s.view.display_index_of(&f))?;
                let d = if s.layout.items.iter().flatten().count() > d {
                    d
                } else {
                    0
                };
                relative_item(&s.layout, d, dx, dy).map(|t| s.view.book_id(t))
            };
            match key {
                gtk4::gdk::Key::Down => {
                    let target = step(&s, focus, 0, 1);
                    s.view.move_focus(target, shift, ctrl);
                }
                gtk4::gdk::Key::Up => {
                    let target = step(&s, focus, 0, -1);
                    s.view.move_focus(target, shift, ctrl);
                }
                gtk4::gdk::Key::Right => {
                    let target = step(&s, focus, 1, 0);
                    s.view.move_focus(target, shift, ctrl);
                }
                gtk4::gdk::Key::Left => {
                    let target = step(&s, focus, -1, 0);
                    s.view.move_focus(target, shift, ctrl);
                }
                gtk4::gdk::Key::Home => {
                    let target = (!s.view.is_empty()).then(|| s.view.book_id(0));
                    s.view.move_focus(target, shift, ctrl);
                }
                gtk4::gdk::Key::End => {
                    let target = (!s.view.is_empty()).then(|| s.view.book_id(s.view.len() - 1));
                    s.view.move_focus(target, shift, ctrl);
                }
                gtk4::gdk::Key::Page_Down | gtk4::gdk::Key::Page_Up => {
                    let forward = key == gtk4::gdk::Key::Page_Down;
                    let d = focus
                        .and_then(|f| s.view.display_index_of(&f))
                        .or(if forward {
                            Some(0)
                        } else {
                            s.view.len().checked_sub(1)
                        });
                    let target = d
                        .and_then(|d| {
                            page_step_display(&s.layout, d, s.config.view_height, forward)
                        })
                        .map(|t| s.view.book_id(t));
                    s.view.move_focus(target, shift, ctrl);
                }
                gtk4::gdk::Key::Return | gtk4::gdk::Key::KP_Enter => {
                    s.activate_focus();
                }
                gtk4::gdk::Key::space => {
                    if let Some(f) = focus {
                        if ctrl {
                            s.view.select_flip(f);
                        } else {
                            s.view.select_one(f);
                        }
                    }
                }
                _ => {
                    // Type-ahead (`KeySearch`): printable characters
                    // accumulate; the first caption with the prefix
                    // selects.
                    if let Some(ch) = keyval_char(key) {
                        if !ctrl && !modifier.contains(gtk4::gdk::ModifierType::ALT_MASK) {
                            s.type_ahead.push(ch);
                            let needle = s.type_ahead.to_lowercase();
                            let hit = (0..s.view.len())
                                .find(|&i| s.caption(i).to_lowercase().starts_with(&needle));
                            if let Some(i) = hit {
                                let id = s.view.book_id(i);
                                s.view.select_one(id);
                                // Keep the item visible: scroll to
                                // its rect (nearest window edge).
                                let rect = s.layout.items.get(i).copied().flatten().map(|r| r.rect);
                                if let Some(rect) = rect {
                                    let adj = canvas_vadjustment(&canvas);
                                    let view_h = adj.page_size();
                                    let y = rect.y - 8.0;
                                    adj.set_value(if y < adj.value() {
                                        y
                                    } else if rect.y + rect.h > adj.value() + view_h {
                                        rect.y + rect.h - view_h + 8.0
                                    } else {
                                        adj.value()
                                    });
                                }
                            }
                            let state2 = Rc::downgrade(&state);
                            s.type_ahead_source = Some(glib::timeout_add_local(
                                std::time::Duration::from_millis(TYPE_AHEAD_RESET_MS),
                                move || {
                                    if let Some(st) = state2.upgrade() {
                                        st.borrow_mut().type_ahead.clear();
                                    }
                                    glib::ControlFlow::Break
                                },
                            ));
                            // Handled — swallow the key. The notify
                            // re-enters this widget — drop first.
                            drop(s);
                            state.borrow().notify_selection();
                            canvas.queue_draw();
                            return glib::Propagation::Stop;
                        }
                    }
                    return glib::Propagation::Proceed;
                }
            }
            s.type_ahead.clear();
            drop(s);
            state.borrow().notify_selection();
            canvas.queue_draw();
            glib::Propagation::Stop
        });
        self.canvas.add_controller(keys);
    }
}

fn update_size_request(state: &Rc<RefCell<ItemViewState>>, canvas: &DrawingArea) {
    let (w, h) = state.borrow().layout.virtual_size;
    // GTK upper bounds a widget's size; the layout culls anyway.
    canvas.set_content_height(h.min(1_000_000.0) as i32);
    canvas.set_content_width(w.min(1_000_000.0) as i32);
}

fn canvas_vadjustment(canvas: &DrawingArea) -> gtk4::Adjustment {
    canvas
        .ancestor(gtk4::ScrolledWindow::static_type())
        .and_then(|w| w.downcast::<ScrolledWindow>().ok())
        .map(|s| s.vadjustment())
        .expect("ItemView canvas must live in a ScrolledWindow")
}

fn scroll_window(scroller: &ScrolledWindow) -> (f64, f64, f64, f64) {
    let h = scroller.hadjustment();
    let v = scroller.vadjustment();
    (h.value(), v.value(), h.page_size(), v.page_size())
}

fn keyval_char(key: gtk4::gdk::Key) -> Option<char> {
    // Printable character type-ahead (the C# KeyPress char). The
    // keyval name for printable keys is the character itself.
    let name = key.name()?;
    if name.len() == 1 {
        name.chars().next()
    } else {
        None
    }
}

fn decode_surface(bytes: &[u8]) -> Option<cairo::ImageSurface> {
    // The pool caches the C# `ThumbnailImage` serialization (size
    // header + JPEG data) — parse, then decode the JPEG.
    let jpeg = cr_image::thumbnail::Thumbnail::from_bytes(bytes)
        .map(|t| t.data)
        .unwrap_or_else(|_| bytes.to_vec());
    let img = cr_image::decode::decode(&jpeg).ok()?;
    Some(surface_from_rgba(&img.rgba, img.width, img.height))
}

fn error_surface() -> Option<cairo::ImageSurface> {
    let img = cr_image::error_assets::error_thumbnail(256)?;
    Some(surface_from_rgba(&img.rgba, img.width, img.height))
}

fn surface_from_rgba(rgba: &[u8], width: u32, height: u32) -> cairo::ImageSurface {
    // ARGB pre-multiplication with the pixel stride (the reader's
    // `image_surface_from_rgba` shape).
    let stride = width as usize * 4;
    let mut argb = vec![0u8; stride * height as usize];
    for (src, dst) in rgba
        .as_chunks::<4>()
        .0
        .iter()
        .zip(argb.as_chunks_mut::<4>().0.iter_mut())
    {
        dst[0] = src[2];
        dst[1] = src[1];
        dst[2] = src[0];
        dst[3] = src[3];
    }
    cairo::ImageSurface::create_for_data(
        argb,
        cairo::Format::ARgb32,
        width as i32,
        height as i32,
        stride as i32,
    )
    .expect("surface")
}

/// The thumbnail-completion pump (the ADR-019 shape): a 10 ms poll
/// that decodes finished thumbs and redraws, alive while loads are in
/// flight. Started by the draw path after queueing — the single
/// source of truth is `pending_thumbs`.
fn start_thumb_pump(state: &Rc<RefCell<ItemViewState>>) {
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
            // Bind first: a `while let` scrutinee borrow would live
            // through the body and conflict with the `borrow_mut`.
            let next = state.borrow().thumb_rx.try_recv();
            match next {
                Ok(done) => {
                    got = true;
                    let mut s = state.borrow_mut();
                    s.pending_thumbs = s.pending_thumbs.saturating_sub(1);
                    let surface = done.bytes.and_then(|bytes| decode_surface(&bytes));
                    match surface {
                        Some(surface) => {
                            s.thumbs.insert(done.book_id, ThumbState::Ready(surface));
                        }
                        None => {
                            s.thumbs.insert(done.book_id, ThumbState::Failed);
                        }
                    }
                }
                Err(_) => break,
            }
        }
        let more = state.borrow().pending_thumbs > 0;
        if !more {
            state.borrow_mut().pump_active = false;
        }
        if got {
            state.borrow().canvas.queue_draw();
        }
        if got || more {
            glib::ControlFlow::Continue
        } else {
            glib::ControlFlow::Break
        }
    });
}

/// Draws one frame; returns whether new thumbnail loads were queued
/// (the caller starts the pump outside the borrow).
fn draw_frame(ctx: &cairo::Context, state: &Rc<RefCell<ItemViewState>>, window: Rect) -> bool {
    let mut s = state.borrow_mut();
    s.config.view_height = window.h;
    // The layout is maintained by the setters; the draw path only
    // tracks the viewport width (a full reflow per frame made the
    // full-library view crawl).
    if (window.w - s.config.view_width).abs() > 0.5 {
        s.relayout(window.w);
    }
    // The theme palette (the C# `SystemColors` parity) — resolved
    // fresh every frame, so a dark/light flip re-styles on the next
    // draw.
    let pal = crate::theme::palette(&s.canvas);
    ctx.set_source_rgb(pal.base.0, pal.base.1, pal.base.2);
    ctx.paint().ok();

    // Queue thumb loads for the visible set.
    s.queue_visible_thumbs(window);
    let queued_thumbs = s.pump_needed();

    // Group headers (Top layout).
    for gh in &s.layout.group_headers {
        if !gh.rect.intersects(&window) {
            continue;
        }
        let group = &s.view.groups()[gh.group];
        ctx.set_source_rgb(pal.window_bg.0, pal.window_bg.1, pal.window_bg.2);
        ctx.rectangle(gh.rect.x, gh.rect.y, gh.rect.w, gh.rect.h);
        ctx.fill().ok();
        ctx.set_source_rgb(pal.fg.0, pal.fg.1, pal.fg.2);
        ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
        ctx.set_font_size(13.0 * 1.15);
        let collapsed_mark = if group.collapsed { "▸ " } else { "▾ " };
        let text = format!("{collapsed_mark}{} ({})", group.caption, group.items.len());
        ctx.move_to(gh.rect.x + 8.0, gh.rect.y + gh.rect.h * 0.68);
        ctx.show_text(&text).ok();
    }

    // Detail column header strip.
    if layout::header_visible(&s.config) {
        let header = Rect::new(0.0, 0.0, s.config.view_width, s.config.header_height);
        ctx.set_source_rgb(pal.window_bg.0, pal.window_bg.1, pal.window_bg.2);
        ctx.rectangle(header.x, header.y, header.w, header.h);
        ctx.fill().ok();
        ctx.set_source_rgb(pal.fg.0, pal.fg.1, pal.fg.2);
        ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
        ctx.set_font_size(12.0);
        // All visible columns — the same list the cells draw (the
        // image-only columns hold their slot).
        let visible: Vec<&Column> = s.detail_columns.iter().filter(|c| c.visible).collect();
        let mut x = header.x + layout::COLUMN_OFFSET_X;
        for column in visible {
            ctx.move_to(x + 2.0, header.y + header.h * 0.7);
            ctx.show_text(column.name).ok();
            x += column.width;
        }
    }

    // Items (culled to the window) — collected first, the draw
    // helpers mutate the thumb cache.
    let visible: Vec<layout::ItemRect> = visible_items(&s.layout, window).copied().collect();
    for item in &visible {
        let book = s.view.book(item.display);
        let selected = s.view.is_selected(&book.id);
        let focused = s.view.focus() == Some(book.id);
        let rect = item.rect;

        if selected {
            ctx.set_source_rgb(pal.selected_bg.0, pal.selected_bg.1, pal.selected_bg.2);
            ctx.rectangle(rect.x - 2.0, rect.y - 2.0, rect.w + 4.0, rect.h + 4.0);
            ctx.fill().ok();
        }

        match s.config.mode {
            ItemViewMode::Thumbnail => {
                draw_thumbnail_item(ctx, &mut s, item.display, rect, selected, &pal);
            }
            ItemViewMode::Tile => {
                draw_tile_item(ctx, &mut s, item.display, rect, selected, &pal);
            }
            ItemViewMode::Detail => {
                draw_detail_item(ctx, &mut s, item.display, rect, selected, &pal);
            }
        }

        if focused {
            let (r, g, b) = if s.canvas.has_focus() {
                pal.selected_bg
            } else {
                FOCUS_UNFOCUSED
            };
            ctx.set_source_rgb(r, g, b);
            let lw = 1.0;
            ctx.set_line_width(lw);
            ctx.rectangle(
                rect.x - lw,
                rect.y - lw,
                rect.w + 2.0 * lw,
                rect.h + 2.0 * lw,
            );
            ctx.stroke().ok();
        }
    }

    // The rubber band (translucent highlight, inflated −2).
    if let Some(band) = s.band {
        ctx.set_source_rgba(pal.selected_bg.0, pal.selected_bg.1, pal.selected_bg.2, 0.3);
        ctx.rectangle(band.x - 2.0, band.y - 2.0, band.w + 4.0, band.h + 4.0);
        ctx.fill().ok();
    }

    queued_thumbs
}

fn draw_thumbnail_item(
    ctx: &cairo::Context,
    s: &mut ItemViewState,
    display: usize,
    rect: Rect,
    selected: bool,
    pal: &crate::theme::Palette,
) {
    let (tr, tg, tb) = if selected { pal.selected_fg } else { pal.fg };
    let image_area_h = rect.h - label_height(&s.config);
    let image_area = Rect::new(rect.x, rect.y, rect.w, image_area_h);
    let book = s.view.book(display);
    let id = book.id;
    let thumb = match s.thumbs.get(&id) {
        Some(ThumbState::Ready(surface)) => Some(surface.clone()),
        Some(ThumbState::Failed) => error_surface(),
        None => None,
    };
    // The placeholder until the load lands.
    if thumb.is_none() {
        ctx.set_source_rgb(pal.window_bg.0, pal.window_bg.1, pal.window_bg.2);
        ctx.rectangle(
            image_area.x + 8.0,
            image_area.y + 8.0,
            image_area.w - 16.0,
            image_area.h - 16.0,
        );
        ctx.fill().ok();
    }
    // The cover (border/shadow/frame/tint — `ThumbRenderer`).
    super::item::draw_cover(
        ctx,
        thumb.as_ref(),
        (image_area.x, image_area.y, image_area.w, image_area.h),
        selected,
    );
    // The read markers: CurrentPage (Orange) / LastPageRead (Green)
    // ribbons on the right edge (`DrawBookmarkV`).
    if thumb.is_some() {
        super::item::draw_bookmarks(
            ctx,
            (image_area.x, image_area.y, image_area.w, image_area.h),
            (book.current_page, book.last_page_read),
            book.info.page_count,
        );
        // The numeric rating tags (the default rating mode).
        super::item::draw_rating_tags(
            ctx,
            (image_area.x, image_area.y, image_area.w, image_area.h),
            book.rating,
            book.info.community_rating,
        );
        if book.file_is_missing {
            super::item::draw_missing_marker(
                ctx,
                (image_area.x, image_area.y, image_area.w, image_area.h),
                missing_cross().as_ref(),
            );
        }
    }
    // The caption: the exact `Comic.Caption`, centered, wrapping in
    // the 3-line strip (skipped when captions hide — QuickOpen).
    if s.config.hide_captions {
        return;
    }
    ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    let scale = (s.config.thumb_height / 192.0).clamp(0.7, 1.0);
    ctx.set_font_size(s.config.font_height * scale);
    let caption = s.caption(display);
    super::item::draw_wrapped_centered(
        ctx,
        &caption,
        rect.x,
        rect.y + image_area_h,
        rect.w,
        3,
        (tr, tg, tb),
    );
}

use layout::label_strip_height as label_height;

/// The RedCross marker surface, decoded once (cairo surfaces are not
/// Send — a thread-local cache, the error-page pattern).
fn missing_cross() -> Option<cairo::ImageSurface> {
    thread_local! {
        static CROSS: Option<cairo::ImageSurface> =
            cr_image::error_assets::red_cross_image()
                .map(|img| surface_from_rgba(&img.rgba, img.width, img.height));
    }
    CROSS.with(|c| c.clone())
}

fn draw_tile_item(
    ctx: &cairo::Context,
    s: &mut ItemViewState,
    display: usize,
    rect: Rect,
    selected: bool,
    pal: &crate::theme::Palette,
) {
    let (tr, tg, tb) = if selected { pal.selected_fg } else { pal.fg };
    // Cover: the left half; text: the right side (`DrawTile`).
    let image_area = Rect::new(rect.x, rect.y, rect.w / 2.0, rect.h);
    let book = s.view.book(display);
    let id = book.id;
    let thumb = match s.thumbs.get(&id) {
        Some(ThumbState::Ready(surface)) => Some(surface.clone()),
        Some(ThumbState::Failed) => error_surface(),
        None => None,
    };
    if thumb.is_none() {
        ctx.set_source_rgb(pal.window_bg.0, pal.window_bg.1, pal.window_bg.2);
        ctx.rectangle(
            image_area.x + 4.0,
            image_area.y + 4.0,
            image_area.w - 8.0,
            image_area.h - 8.0,
        );
        ctx.fill().ok();
    }
    super::item::draw_cover(
        ctx,
        thumb.as_ref(),
        (image_area.x, image_area.y, image_area.w, image_area.h),
        selected,
    );
    // The text block: the `DefaultFileComic` lines with the shared
    // tab stop (`SimpleTextRenderer` two-column shape). The segments
    // render once per book (tab stops resolved, lines truncated) —
    // the per-frame work is show_text only.
    ctx.save().ok();
    let text_x = rect.x + rect.w / 2.0 + 4.0;
    let text_w = rect.x + rect.w - text_x - 4.0;
    ctx.rectangle(text_x - 2.0, rect.y, text_w + 4.0, rect.h);
    ctx.clip();
    let segs = {
        let book = s.view.book(display).clone();
        tile_segments(s, &id, &book, rect.h, text_w, ctx)
    };
    let mut y = rect.y + 4.0;
    for seg in &segs {
        ctx.select_font_face(
            "Sans",
            cairo::FontSlant::Normal,
            if seg.bold {
                cairo::FontWeight::Bold
            } else {
                cairo::FontWeight::Normal
            },
        );
        ctx.set_font_size(seg.size.max(6.0));
        let line_h = super::item::line_height(ctx).max(2.0);
        if y + line_h > rect.y + rect.h {
            break;
        }
        if seg.text.is_empty() {
            y += line_h * 0.5;
            continue;
        }
        ctx.set_source_rgb(tr, tg, tb);
        ctx.move_to(text_x, y + line_h * 0.85);
        ctx.show_text(&seg.text).ok();
        if let Some(label) = &seg.label {
            ctx.move_to(text_x + seg.tab, y + line_h * 0.85);
            ctx.show_text(label).ok();
        }
        y += line_h;
    }
    ctx.restore().ok();
}

/// One rendered tile text segment (a tab line carries the VALUE in
/// `text`, the LABEL in `label`, and the shared tab stop in `tab`).
#[derive(Clone)]
struct TileSeg {
    label: Option<String>,
    text: String,
    tab: f64,
    size: f64,
    bold: bool,
}

/// Builds (or fetches) the rendered segments for one book. The
/// expensive parts — the proposed-name regexes, the tab-stop
/// measurement, the truncation — run once per book and cell width.
fn tile_segments(
    s: &mut ItemViewState,
    id: &CrGuid,
    book: &ComicBook,
    cell_h: f64,
    text_w: f64,
    ctx: &cairo::Context,
) -> Vec<TileSeg> {
    if s.tile_render_width == text_w {
        if let Some(segs) = s.tile_render.get(id) {
            return segs.clone();
        }
    }
    if s.tile_render_width != text_w {
        s.tile_render.clear();
        s.tile_render_width = text_w;
    }
    let lines = super::item::tile_text_lines(book);
    // The C# tile font: clamp(cell height * 0.07, 0.8font, 1.0font).
    let tile_font = (cell_h * 0.07)
        .clamp(s.config.font_height * 0.8, s.config.font_height)
        .max(6.0);
    // The tab stop: the widest first-segment width + 8 px.
    ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    ctx.set_font_size(tile_font * 0.9);
    let mut tab = 0.0f64;
    for (text, _, _) in &lines {
        if let Some(pos) = text.find('\t') {
            if let Ok(ext) = ctx.text_extents(&text[..pos]) {
                tab = tab.max(ext.width());
            }
        }
    }
    let tab = tab + 8.0;
    let mut segs: Vec<TileSeg> = Vec::new();
    for (text, scale, bold) in &lines {
        let size = (tile_font * scale).max(6.0);
        ctx.select_font_face(
            "Sans",
            cairo::FontSlant::Normal,
            if *bold {
                cairo::FontWeight::Bold
            } else {
                cairo::FontWeight::Normal
            },
        );
        ctx.set_font_size(size);
        if text.is_empty() {
            segs.push(TileSeg {
                label: None,
                text: String::new(),
                tab: 0.0,
                size,
                bold: *bold,
            });
            continue;
        }
        if let Some(pos) = text.find('\t') {
            segs.push(TileSeg {
                label: Some(text[pos + 1..].to_string()),
                text: text[..pos].to_string(),
                tab,
                size,
                bold: *bold,
            });
        } else {
            // Truncate with an ellipsis at the block width — a
            // binary search over the prefix (the old per-character
            // walk measured long summaries hundreds of times per
            // frame).
            let mut line = truncate_to_width(ctx, text, text_w);
            if line != *text {
                line.push('…');
            }
            segs.push(TileSeg {
                label: None,
                text: line,
                tab: 0.0,
                size,
                bold: *bold,
            });
        }
    }
    s.tile_render.insert(*id, segs.clone());
    segs
}

/// The largest prefix of `text` whose measured width fits `width`
/// (binary search over the character count).
fn truncate_to_width(ctx: &cairo::Context, text: &str, width: f64) -> String {
    let fits = |n: usize| -> bool {
        let candidate: String = text.chars().take(n).collect();
        ctx.text_extents(&candidate)
            .map(|e| e.width() <= width)
            .unwrap_or(true)
    };
    if fits(text.chars().count()) {
        return text.to_string();
    }
    let total = text.chars().count();
    let (mut lo, mut hi) = (0usize, total);
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        if fits(mid) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    text.chars().take(lo).collect()
}

fn draw_detail_item(
    ctx: &cairo::Context,
    s: &mut ItemViewState,
    display: usize,
    rect: Rect,
    selected: bool,
    pal: &crate::theme::Palette,
) {
    let (tr, tg, tb) = if selected { pal.selected_fg } else { pal.fg };
    ctx.set_source_rgb(tr, tg, tb);
    ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    ctx.set_font_size(12.0);
    // The row's cell texts, cached per book (the same regex hazard
    // as the captions — the resolver's proposed fallback parses file
    // names).
    let visible: Vec<Column> = s
        .detail_columns
        .iter()
        .filter(|c| c.visible)
        .cloned()
        .collect();
    let book = s.view.book(display);
    let id = book.id;
    let texts: Vec<String> = match s.detail_texts.get(&id) {
        Some(texts) => texts.clone(),
        None => {
            let row: Vec<String> = visible
                .iter()
                .map(|column| match column.name {
                    "Cover" => String::new(),
                    _ => columns::cell_text(column, s.view.book(display)),
                })
                .collect();
            s.detail_texts.insert(id, row.clone());
            row
        }
    };
    let column_rects = layout::detail_column_rects(&s.config, &rect);
    for (i, column) in visible.iter().enumerate() {
        let Some(cell) = column_rects.get(i) else {
            break;
        };
        let text = match column.name {
            "Position" => (display + 1).to_string(),
            _ => texts.get(i).cloned().unwrap_or_default(),
        };
        if text.is_empty() {
            continue;
        }
        let (tx, ty) = match column.alignment {
            columns::ColumnAlignment::Far => {
                // Right-align inside the cell (approximate: 0.55
                // per char).
                let w = text.len() as f64 * 6.6;
                (cell.x + cell.w - w - 4.0, cell.y + cell.h * 0.75)
            }
            _ => (cell.x + 4.0, cell.y + cell.h * 0.75),
        };
        ctx.move_to(tx, ty);
        ctx.show_text(&text).ok();
    }
}
