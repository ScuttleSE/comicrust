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
//! line), Detail headers resize + auto-size but do not reorder
//! (drag-reorder), and Tile text lines are the caption + file name
//! until the C# text builder lands (T4).

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
use cr_engine::matcher::book_view;
use cr_engine::matcher::series::{self, SeriesKey, SeriesStatistics};

use super::columns::{self, Column};
use super::layout::{
    self, hit_group_arrow, hit_group_header, hit_test, page_step_display, relative_item,
    visible_items, ItemLayout, ItemViewMode, LayoutConfig, Rect,
};
use super::view_state::ViewState;

/// The type-ahead buffer reset (`KeySearch`).
const TYPE_AHEAD_RESET_MS: u64 = 2500;

/// The Detail cell + header font (`base.View.Font` =
/// `SystemFonts.IconTitleFont`, 9 pt ≈ 13 px on the Linux font
/// stack; the C# `CoverViewItem.OnDraw` text uses the view font).
const DETAIL_FONT_SIZE: f64 = 13.0;

/// The Detail row band (`ThemeColors.DetailView.RowHighlight`):
/// `Color.LightGray` blended at alpha 96 over the window base (the
/// dark table swaps the target for RGB 72,72,72 — DarkThemeColorTable).
const ROW_BAND_LIGHT: (f64, f64, f64) = (211.0 / 255.0, 211.0 / 255.0, 211.0 / 255.0);
const ROW_BAND_DARK: (f64, f64, f64) = (72.0 / 255.0, 72.0 / 255.0, 72.0 / 255.0);
const ROW_BAND_ALPHA: f64 = 96.0 / 255.0;

/// The per-column cell inset (`rectangle.Inflate(-2, 0)`).
const DETAIL_CELL_PAD: f64 = 2.0;

/// The focus ring when the view does not hold focus (a neutral gray
/// that reads on both themes — the palette colors cover the rest).
const FOCUS_UNFOCUSED: (f64, f64, f64) = (0.5, 0.5, 0.55);
/// The Ctrl+wheel size limits + step (`Program.Min/MaxThumbHeight`,
/// the wheel steps 16 — `itemView_MouseWheel`).
const THUMB_WHEEL_STEP: f64 = 16.0;

/// The live column drag (`ItemView.resizeColumn`): the column id,
/// the gesture-start x, and the width at start — the move applies
/// `OnMouseMoveResizeColumnHeader`'s clamp math.
#[derive(Clone, Copy)]
struct ResizeState {
    id: i32,
    start_x: f64,
    start_width: f64,
}

type ActivateFn = Rc<dyn Fn(&CrGuid)>;
type SelectionFn = Rc<dyn Fn(usize)>;
type HeaderContextFn = Rc<dyn Fn(f64, f64)>;
type BookContextFn = Rc<dyn Fn(Option<CrGuid>, f64, f64)>;

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
    /// The per-series statistics for the "Series:" columns (the C#
    /// `StatsProvider.Getns` — the library's seriesStats cache). Built
    /// lazily on the first stats-column draw over the view's books;
    /// cleared on every book-set change.
    series_stats: Option<HashMap<SeriesKey, SeriesStatistics>>,
    /// The live column drag (`resizeColumn`): the column id, the
    /// gesture-start x, and the width at start (the C# resize
    /// fields; the move applies the C# clamp math).
    resize: Option<ResizeState>,
    on_activate: Option<ActivateFn>,
    on_selection_changed: Option<SelectionFn>,
    /// The Detail header right-click (the T6 column chooser).
    on_header_context: Option<HeaderContextFn>,
    /// The book right-click (the context menu).
    on_book_context: Option<BookContextFn>,
    type_ahead: String,
    type_ahead_source: Option<glib::SourceId>,
    canvas: DrawingArea,
    /// The "no metadata" tags drawn THIS frame (the probe seam — the
    /// draw re-records every frame, the arrow-zone pattern).
    badge_draws: u32,
}

impl ItemViewState {
    /// `OnMouseDownColumnHeaderSeparator`: the drag starts at the
    /// column's right edge with its current width.
    fn begin_resize(&mut self, id: i32, x: f64) -> bool {
        let Some(c) = self.detail_columns.iter().find(|c| c.id == id) else {
            return false;
        };
        self.resize = Some(ResizeState {
            id,
            start_x: x,
            start_width: c.width,
        });
        true
    }

    /// `OnMouseMoveResizeColumnHeader`:
    /// `width = (startWidth + (x - startX)).Clamp(0, 10000)`, live
    /// reflow each move.
    fn move_resize(&mut self, x: f64) -> f64 {
        let Some(r) = self.resize else {
            return -1.0;
        };
        let width = (r.start_width + (x - r.start_x)).clamp(0.0, 10000.0);
        if let Some(c) = self.detail_columns.iter_mut().find(|c| c.id == r.id) {
            c.width = width;
        }
        self.relayout(self.canvas.width() as f64);
        width
    }

    /// `OnMouseUpResizeColumnHeader`.
    fn end_resize(&mut self) {
        self.resize = None;
    }

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
        let notify = self
            .on_selection_changed
            .as_ref()
            .map(|f| (Rc::clone(f), self.view.selection().len()));
        if let Some((f, count)) = notify {
            f(count);
        }
    }

    fn activate_focus(&self) -> Option<CrGuid> {
        self.view.focus()
    }

    /// Queues cover loads for the visible items (`AddToTop` — the
    /// demanded thumbs skip the line). Keys ride
    /// `front_cover_thumbnail_key` (the C# `GetThumbnailKey`): the
    /// custom thumbnail for fileless books, the cover page index for
    /// file-backed ones.
    ///
    /// With `GenerateThumbnailsOnDemand` OFF (a port addition — the
    /// C# always generates on demand), only already-cached covers
    /// load; the rest stay placeholders until File ▸ Generate Cover
    /// Thumbnails backfills the cache.
    fn queue_visible_thumbs(&mut self, window: Rect) {
        let on_demand = crate::library::settings()
            .borrow()
            .generate_thumbnails_on_demand;
        let wanted: Vec<(CrGuid, cr_core::model::comic_book::ComicBook)> =
            visible_items(&self.layout, window)
                .filter_map(|item| {
                    let book = self.view.book(item.display);
                    if self.queued.contains(&book.id) {
                        return None;
                    }
                    Some((book.id, book.clone()))
                })
                .collect();
        for (id, book) in wanted {
            let key = cr_engine::image_pool::front_cover_thumbnail_key(&book);
            if !on_demand && !self.pool.thumbnail_cached(&key) {
                // The cover stays a placeholder; not marked queued so
                // a later backfill lands on the next draw.
                continue;
            }
            self.queued.insert(id);
            self.pending_thumbs += 1;
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
        // The viewport's scroll-to-focus (GTK 4.6+, default ON) is the
        // right-click jump: grab_focus on the full-content canvas makes
        // the viewport scroll its "into view" position — y=0 (the C#
        // `Focus()` never scrolls). The in-grid scroll paths (keyboard
        // nav, Home/End, Ctrl+wheel) move the adjustment directly and
        // never rely on focus, so disabling this is safe. The C#
        // ItemView draws into one canvas the same way.
        if let Some(viewport) = scroller.child().and_downcast::<gtk4::Viewport>() {
            viewport.set_scroll_to_focus(false);
        }

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
            series_stats: None,
            resize: None,
            on_activate: None,
            on_selection_changed: None,
            on_header_context: None,
            on_book_context: None,
            type_ahead: String::new(),
            type_ahead_source: None,
            canvas: canvas.clone(),
            badge_draws: 0,
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
                // The culling window is the VIEWPORT (the adjustment
                // page size). The draw size is the canvas's FULL
                // virtual allocation (set_content_height) — taking
                // its max made every frame draw every item below the
                // scroll position (a 2875-book list measured ~2.5k
                // items / ~1 s per frame; the real visible set is
                // ~35). The fallback covers a draw before the first
                // allocation (page size still 0).
                let window = if view_w > 1.0 && view_h > 1.0 {
                    Rect::new(sx, sy, view_w, view_h)
                } else {
                    Rect::new(sx, sy, width as f64, height as f64)
                };
                let queued = draw_frame(ctx, &state, window);
                if queued {
                    start_thumb_pump(&state);
                }
            });
        }

        // Redraw on scroll; under CR_TRACE, a jump to the top prints
        // a backtrace — the scroll reset culprit evidence.
        for adj in [scroller.hadjustment(), scroller.vadjustment()] {
            let canvas = canvas.clone();
            let is_v = adj == scroller.vadjustment();
            let last = std::rc::Rc::new(std::cell::Cell::new(0.0f64));
            adj.connect_value_changed(move |a| {
                let new = a.value();
                let old = last.replace(new);
                if is_v && crate::trace::enabled() && new < 50.0 && old > 200.0 {
                    crate::trace::trace(format!(
                        "SCROLL JUMP {old} -> {new}; backtrace:\n{}",
                        std::backtrace::Backtrace::force_capture()
                    ));
                }
                canvas.queue_draw();
            });
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

    /// Replaces the book set (a library selection change). The
    /// FILTER, the GROUPER and the SORT survive the swap (the C#
    /// keeps the view config on the ItemView across refreshes —
    /// `FillBookList` re-creates the items, never the view config).
    pub fn set_books(&self, books: Vec<ComicBook>) {
        let width = self.state.borrow().config.view_width;
        let t0 = std::time::Instant::now();
        {
            let mut s = self.state.borrow_mut();
            let filter = s.view.filter_clone();
            let grouper = s.view.grouper();
            let sort = s.view.sort().clone();
            let t1 = std::time::Instant::now();
            s.view = ViewState::new(books);
            crate::trace::trace(format!("set_books: ViewState::new {:?}", t1.elapsed()));
            let t2 = std::time::Instant::now();
            s.view.set_filter(filter);
            s.view.set_sort_chain(sort);
            crate::trace::trace(format!("set_books: set_filter {:?}", t2.elapsed()));
            if grouper.is_some() {
                s.view.set_grouper(grouper);
            }
            s.thumbs.clear();
            s.queued.clear();
            s.captions.clear();
            s.detail_texts.clear();
            s.tile_texts.clear();
            s.tile_render.clear();
            s.series_stats = None;
            s.band = None;
            let t3 = std::time::Instant::now();
            s.relayout(width);
            crate::trace::trace(format!("set_books: relayout {:?}", t3.elapsed()));
        }
        let t4 = std::time::Instant::now();
        // Outside the borrow (the RefCell panics on nested borrows).
        self.update_size_request();
        crate::trace::trace(format!(
            "set_books: size_request {:?} (total {:?})",
            t4.elapsed(),
            t0.elapsed()
        ));
        self.notify_and_redraw();
    }

    /// Appends books WITHOUT dropping the per-book caches (the scan
    /// batches: existing books' display data is unchanged — thumbs,
    /// captions and tile/detail texts stay; only the new books
    /// compute on first draw). `set_books` remains the full-swap path
    /// (list switches, edits).
    pub fn append_books(&self, batch: Vec<ComicBook>) {
        if batch.is_empty() {
            return;
        }
        let width = self.state.borrow().config.view_width;
        let t0 = std::time::Instant::now();
        let added = batch.len();
        {
            let mut s = self.state.borrow_mut();
            let filter = s.view.filter_clone();
            s.view.append_books(batch, filter);
            // New books change the per-series aggregates.
            s.series_stats = None;
            s.band = None;
            s.relayout(width);
        }
        self.update_size_request();
        self.notify_and_redraw();
        crate::trace::trace(format!(
            "append_books: +{added} books in {:?}",
            t0.elapsed()
        ));
    }

    /// The live read-ribbon update: a page-turn read-state change
    /// lands in the view's book copy and repaints (the C# ItemView
    /// draws the live book objects and repaints per book change; the
    /// port's cloned snapshots need the push). No rebuild — a page
    /// turn never re-sorts or re-filters.
    pub fn update_read_state(&self, id: CrGuid, current_page: i32, last_page_read: i32) {
        let changed = {
            let mut s = self.state.borrow_mut();
            s.view.update_read_state(id, current_page, last_page_read)
        };
        if changed {
            self.canvas.queue_draw();
        }
    }

    pub fn connect_activate<F: Fn(&CrGuid) + 'static>(&self, f: F) {
        self.state.borrow_mut().on_activate = Some(Rc::new(f));
    }

    pub fn connect_selection_changed<F: Fn(usize) + 'static>(&self, f: F) {
        self.state.borrow_mut().on_selection_changed = Some(Rc::new(f));
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
        self.state.borrow_mut().on_book_context = Some(Rc::new(f));
        let state = Rc::downgrade(&self.state);
        let canvas = self.canvas.clone();
        let gesture = GestureClick::new();
        gesture.set_button(3);
        gesture.connect_pressed(move |gesture, _n, x, y| {
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            if let Some(state) = state.upgrade() {
                Self::emit_context(&state, &canvas, x, y);
            }
        });
        self.canvas.add_controller(gesture);
    }

    /// The right-click body (shared by the gesture and the probe):
    /// the header hit routes to the column chooser, otherwise the hit
    /// id + TOPLEVEL coordinates reach the hook (a window-parented
    /// popover lands under the cursor).
    pub fn emit_context(state: &Rc<RefCell<ItemViewState>>, canvas: &DrawingArea, x: f64, y: f64) {
        crate::trace::trace(format!("context: press at ({x}, {y})"));
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
                let (wx, wy) = Self::toplevel_xy(canvas, x, y);
                crate::trace::trace(format!("context: header hit, toplevel ({wx}, {wy})"));
                f(wx, wy);
                return;
            }
        }
        let s = state.borrow();
        let hit = hit_test(&s.layout, x, y).map(|d| s.view.book_id(d));
        drop(s);
        let (wx, wy) = Self::toplevel_xy(canvas, x, y);
        crate::trace::trace(format!(
            "context: hit {} toplevel ({wx}, {wy}) — firing menu",
            hit.is_some()
        ));
        if let Some(f) = state.borrow().on_book_context.clone() {
            f(hit, wx, wy);
        } else {
            crate::trace::trace("context: no menu hook registered");
        }
    }

    fn toplevel_xy(canvas: &DrawingArea, x: f64, y: f64) -> (f64, f64) {
        canvas
            .ancestor(gtk4::Window::static_type())
            .and_then(|w| w.downcast::<gtk4::Window>().ok())
            .and_then(|win| canvas.translate_coordinates(&win, x, y))
            .unwrap_or((x, y))
    }

    /// The group-header press paths (`OnMouseClickGroupHeader` +
    /// `OnMouseDoubleClickGroupHeader`): `n == 1` — the ARROW toggles
    /// that group's collapse, the LABEL selects ALL the group's items;
    /// `n > 1` — the ARROW expands/collapses ALL groups; the
    /// LABEL toggles that group. Returns false when the point is not
    /// on a header (the caller falls through to the item hit).
    ///
    /// Double-click direction: the C# fires the single-click toggle
    /// on BOTH MouseUps before the DoubleClick event, so the clicked
    /// header is back at its ORIGINAL state when the all-toggle
    /// reads it — the net effect is every group taking the OPPOSITE
    /// of the clicked group's original state. The port fires the
    /// single toggle once (press n=1), so the all-toggle applies the
    /// POST-first-click state (= the negated original) directly.
    ///
    /// The header hit reads through a HOISTED borrow: a borrow inside
    /// the `if let` SCRUTINEE lives until the end of the whole
    /// if/else statement (the edition-2021 temporaries lesson) and
    /// collided with the `borrow_mut` below — the group-by-series
    /// double-click crash.
    /// The shared press body (the real `connect_pressed` closure AND
    /// the probe seam run it): group headers, the Detail separator
    /// zones, the activate, selection and the rubber-band start. The
    /// activate callback runs with NO borrow held — the open path
    /// re-enters `update_read_state` through the reader hook (the
    /// 2026-09-11 double-click-open crash).
    fn handle_press(
        state: &Rc<RefCell<ItemViewState>>,
        canvas: &DrawingArea,
        n: u32,
        x: f64,
        y: f64,
        mods: gtk4::gdk::ModifierType,
    ) {
        if n != 1 {
            // The group-header double-click first (`OnMouseDoubleClick
            // GroupHeader`): the ARROW expands/collapses ALL groups —
            // the direction is the clicked header's post-first-click
            // state (the single click of the sequence already toggled
            // it); the LABEL toggles that group again.
            if Self::handle_group_header_press(state, canvas, n, x, y) {
                return;
            }
            // Double-click: a header separator auto-sizes the
            // column first (`OnDoubleClickColumnHeaderSeperator`),
            // otherwise it activates the focused book.
            let header_hit = {
                let s = state.borrow();
                layout::column_separator_hit(&s.config, &s.detail_columns, x, y)
            };
            if let Some(id) = header_hit {
                autosize_column_state(state, id);
                canvas.queue_draw();
                return;
            }
            let id = state.borrow().view.focus();
            if let Some(id) = id {
                // Clone the callback out and call it with NO borrow
                // held (the scrutinee temporary in the old `if let`
                // held the Ref across the call).
                let f = state.borrow().on_activate.clone();
                if let Some(f) = f {
                    f(&id);
                }
            }
            return;
        }
        {
            let mut s = state.borrow_mut();
            // The header separator zone wins first (the C#
            // `OnMouseDown` checks it before the item hit).
            if let Some(id) = layout::column_separator_hit(&s.config, &s.detail_columns, x, y) {
                s.begin_resize(id, x);
                drop(s);
                canvas.queue_draw();
                return;
            }
        }
        let s = state.borrow_mut();
        let hit = hit_test(&s.layout, x, y);
        drop(s);
        // The group-header click (`OnMouseClickGroupHeader`): the
        // ARROW toggles the group's collapse; the LABEL selects
        // ALL the group's items (no collapse).
        if Self::handle_group_header_press(state, canvas, n, x, y) {
            return;
        }
        let mut s = state.borrow_mut();
        match hit {
            Some(display) => {
                let id = s.view.book_id(display);
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
    }

    fn handle_group_header_press(
        state: &Rc<RefCell<ItemViewState>>,
        canvas: &DrawingArea,
        n: u32,
        x: f64,
        y: f64,
    ) -> bool {
        let group_hit = {
            let s = state.borrow();
            hit_group_header(&s.layout, x, y)
        };
        let Some(group) = group_hit else {
            return false;
        };
        if n > 1 {
            let (arrow, collapsed) = {
                let s = state.borrow();
                (
                    hit_group_arrow(&s.layout, group, x, y),
                    s.view.groups()[group].collapsed,
                )
            };
            let mut s = state.borrow_mut();
            if arrow {
                // `collapsed` = the post-first-click state = the
                // NEGATED original; the C# net applies that to ALL.
                s.view.set_all_collapsed(collapsed);
            } else {
                s.view.set_collapsed(group, !collapsed);
            }
            s.relayout(canvas.width() as f64);
            drop(s);
            update_size_request(state, canvas);
            canvas.queue_draw();
            return true;
        }
        let mut s = state.borrow_mut();
        if hit_group_arrow(&s.layout, group, x, y) {
            let collapsed = !s.view.groups()[group].collapsed;
            s.view.set_collapsed(group, collapsed);
            s.relayout(canvas.width() as f64);
            drop(s);
            update_size_request(state, canvas);
            canvas.queue_draw();
            return true;
        }
        s.view.select_group_items(group);
        s.band = None;
        drop(s);
        // The selection callback re-enters the shell (the status
        // panels) — fire OUTSIDE the state borrow (the RefCell
        // double-borrow lesson).
        state.borrow().notify_selection();
        canvas.queue_draw();
        true
    }

    /// The Detail header right-click (the column chooser; the C#
    /// `autoHeaderContextMenuStrip_Opening`).
    pub fn connect_header_context<F: Fn(f64, f64) + 'static>(&self, f: F) {
        self.state.borrow_mut().on_header_context = Some(Rc::new(f));
    }

    /// The live vadjustment value.
    pub fn scroll_value(&self) -> f64 {
        self.canvas
            .ancestor(gtk4::ScrolledWindow::static_type())
            .and_then(|w| w.downcast::<ScrolledWindow>().ok())
            .map(|s| s.vadjustment().value())
            .unwrap_or(-1.0)
    }

    /// Sets the vadjustment value.
    pub fn set_scroll_value(&self, y: f64) {
        if let Some(scroller) = self
            .canvas
            .ancestor(gtk4::ScrolledWindow::static_type())
            .and_then(|w| w.downcast::<ScrolledWindow>().ok())
        {
            scroller.vadjustment().set_value(y);
        }
    }

    /// Scrolls the grid and reports the resulting vadjustment value
    /// (the context-menu probe).
    pub fn probe_scroll_to(&self, y: f64) -> f64 {
        self.set_scroll_value(y);
        self.scroll_value()
    }

    /// The live vadjustment value (the context-menu probe).
    pub fn probe_scroll_value(&self) -> f64 {
        self.scroll_value()
    }

    /// Fires the right-click hook through the shared gesture body
    /// (the context-menu probe).
    pub fn probe_context(&self, x: f64, y: f64) {
        Self::emit_context(&self.state, &self.canvas, x, y);
    }

    /// The probe path: a REAL left press (`n` = 1 single, 2 the
    /// double-click's second press) through the shared group-header
    /// handler. Returns whether the point hit a group header.
    pub fn probe_group_press(&self, n: u32, x: f64, y: f64) -> bool {
        Self::handle_group_header_press(&self.state, &self.canvas, n, x, y)
    }

    /// The probe path: the REAL press body (`n` = 1 single, 2 the
    /// double-click's second press — the activate) through the shared
    /// `handle_press`. No modifiers.
    pub fn probe_press(&self, n: u32, x: f64, y: f64) {
        Self::handle_press(
            &self.state,
            &self.canvas,
            n,
            x,
            y,
            gtk4::gdk::ModifierType::empty(),
        );
    }

    /// The recorded arrow zone of one group header (the draw records
    /// it — zero until the first paint).
    pub fn probe_group_arrow_zone(&self, group: usize) -> (f64, f64, f64, f64) {
        let s = self.state.borrow();
        s.layout
            .group_headers
            .iter()
            .find(|g| g.group == group)
            .map(|g| (g.arrow.x, g.arrow.y, g.arrow.w, g.arrow.h))
            .unwrap_or((0.0, 0.0, 0.0, 0.0))
    }

    /// The "no metadata" tags drawn in the LAST frame (the draw
    /// resets the count at every frame start — settle before reading).
    pub fn probe_metadata_badge_draws(&self) -> u32 {
        self.state.borrow().badge_draws
    }

    /// The center of one placed item's rect (the probe's press
    /// coordinates).
    pub fn probe_item_center(&self, display: usize) -> Option<(f64, f64)> {
        let s = self.state.borrow();
        s.layout
            .items
            .get(display)
            .and_then(|it| it.as_ref())
            .map(|it| (it.rect.x + it.rect.w / 2.0, it.rect.y + it.rect.h / 2.0))
    }

    /// The center of one book's placed rect by id (the probe's press
    /// coordinates for a known book).
    pub fn probe_book_center(&self, id: &CrGuid) -> Option<(f64, f64)> {
        let s = self.state.borrow();
        let d = s.view.display_index_of(id)?;
        s.layout
            .items
            .get(d)
            .and_then(|it| it.as_ref())
            .map(|it| (it.rect.x + it.rect.w / 2.0, it.rect.y + it.rect.h / 2.0))
    }

    /// The read state of one book in the view's copy (the probe gate
    /// for the reader-hook push).
    pub fn probe_book_read_state(&self, id: &CrGuid) -> Option<(i32, i32)> {
        let s = self.state.borrow();
        s.view
            .books()
            .iter()
            .find(|b| &b.id == id)
            .map(|b| (b.current_page, b.last_page_read))
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

    /// `ItemView.ToggleGroups` (the Collapse/Expand all Groups
    /// command): the first group's state decides the direction.
    pub fn toggle_all_groups(&self) {
        let width = self.state.borrow().config.view_width;
        {
            let mut s = self.state.borrow_mut();
            s.view.toggle_groups();
            s.relayout(width);
        }
        self.update_size_request();
        self.canvas.queue_draw();
    }

    /// `AreGroupsVisible` (the Views-menu enable gate): a grouper
    /// is set.
    pub fn has_groups(&self) -> bool {
        self.state.borrow().view.grouper().is_some()
    }

    /// Whether ANY group is collapsed (the probe).
    pub fn any_group_collapsed(&self) -> bool {
        self.state.borrow().view.any_collapsed()
    }

    /// The group + collapsed counts (the probe gates).
    pub fn group_count(&self) -> usize {
        self.state.borrow().view.groups().len()
    }

    pub fn collapsed_count(&self) -> usize {
        self.state
            .borrow()
            .view
            .groups()
            .iter()
            .filter(|g| g.collapsed)
            .count()
    }

    /// The per-group TRUE counts (the probe — the collapsed headers
    /// keep showing them).
    pub fn group_counts(&self) -> Vec<usize> {
        self.state
            .borrow()
            .view
            .groups()
            .iter()
            .map(|g| g.count)
            .collect()
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

    /// The grid canvas (the ancestor walks in the probes).
    pub fn grid_widget(&self) -> DrawingArea {
        self.canvas.clone()
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

    /// `AutoSizeHeader` — the widest displayed cell text + padding;
    /// a non-text column keeps its width (the C# measures the item
    /// renderer's bounds; the port's image cells have no text — a
    /// recorded deviation).
    pub fn autosize_column(&self, id: i32) -> f64 {
        let width = autosize_column_state(&self.state, id);
        self.canvas.queue_draw();
        width
    }

    /// The drag start through the real click path (the probe).
    pub fn probe_column_resize_start(&self, id: i32, x: f64) -> bool {
        self.state.borrow_mut().begin_resize(id, x)
    }

    /// The drag move through the real path; returns the live width.
    pub fn probe_column_resize_move(&self, x: f64) -> f64 {
        let w = self.state.borrow_mut().move_resize(x);
        self.canvas.queue_draw();
        w
    }

    /// The drag end; returns the final width.
    pub fn probe_column_resize_end(&self) -> f64 {
        let width = {
            let s = self.state.borrow();
            s.resize
                .as_ref()
                .and_then(|r| {
                    s.detail_columns
                        .iter()
                        .find(|c| c.id == r.id)
                        .map(|c| c.width)
                })
                .unwrap_or(-1.0)
        };
        self.state.borrow_mut().end_resize();
        self.canvas.queue_draw();
        width
    }

    fn update_size_request(&self) {
        update_size_request(&self.state, &self.canvas);
    }

    fn notify_and_redraw(&self) {
        // Lift the hook + payload OUT of the state borrow and fire
        // AFTER it drops — the hook chain reaches sync_enabled →
        // the status-bar slider → `set_item_size`, which borrows
        // again (the Phase 3 re-entrancy lesson; the slider fires
        // whenever the configured value actually changes).
        let notify = {
            let s = self.state.borrow();
            s.on_selection_changed
                .as_ref()
                .map(|f| (f.clone(), s.view.selection().len()))
        };
        if let Some((f, count)) = notify {
            f(count);
        }
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
            Self::handle_press(
                &state,
                &canvas,
                n as u32,
                x,
                y,
                gesture.current_event_state(),
            );
        });
        gesture.connect_released(move |gesture, _n, _x, _y| {
            let Some(state) = state_released.upgrade() else {
                return;
            };
            gesture.set_state(gtk4::EventSequenceState::Claimed);
            let mut s = state.borrow_mut();
            if s.resize.take().is_some() {
                // `OnMouseUpResizeColumnHeader`.
                drop(s);
                canvas_released.queue_draw();
                return;
            }
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

        // Band motion + the Detail column drag (`OnMouseMove`).
        let state = Rc::downgrade(&self.state);
        let motion = EventControllerMotion::new();
        motion.connect_motion(move |_, x, y| {
            let Some(state) = state.upgrade() else {
                return;
            };
            let mut s = state.borrow_mut();
            if s.resize.is_some() {
                // The C# returns early while resizing — the cursor
                // stays the split marker and the band never runs.
                s.move_resize(x);
                s.canvas.set_cursor_from_name(Some("col-resize"));
                s.canvas.queue_draw();
                return;
            }
            if s.band.is_some() {
                let (sx, sy) = s.band_start;
                let rect = Rect::new(sx.min(x), sy.min(y), (x - sx).abs(), (y - sy).abs());
                s.band = Some(rect);
                s.canvas.queue_draw();
            }
            // The VSplit cursor over a separator (`OnMouseMove` —
            // `Cursors.VSplit`); the default elsewhere.
            let over_separator =
                layout::column_separator_hit(&s.config, &s.detail_columns, x, y).is_some();
            let cursor = if over_separator {
                Some("col-resize")
            } else {
                None
            };
            s.canvas.set_cursor_from_name(cursor);
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
            // The activate callback opens the reader; the open fires
            // the reader page hook which re-enters ANY state borrow
            // (`update_read_state`) — the callback MUST run with no
            // borrow held (the 2026-09-11 double-click-open crash).
            let mut activate: Option<CrGuid> = None;
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
                    activate = s.activate_focus();
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
            // The activate fires with NO borrow held (the open path
            // re-enters `update_read_state` through the reader hook).
            if let Some(id) = activate {
                let f = state.borrow().on_activate.clone();
                if let Some(f) = f {
                    f(&id);
                }
            }
            state.borrow().notify_selection();
            canvas.queue_draw();
            glib::Propagation::Stop
        });
        self.canvas.add_controller(keys);
    }
}

fn update_size_request(state: &Rc<RefCell<ItemViewState>>, canvas: &DrawingArea) {
    let (w, h) = state.borrow().layout.virtual_size;
    if crate::trace::enabled() {
        // A collapsing content size resets the ScrolledWindow value —
        // trace every height change.
        thread_local! {
            static LAST_H: std::cell::Cell<i32> = const { std::cell::Cell::new(-1) };
        }
        let h_i = h.min(1_000_000.0) as i32;
        if LAST_H.with(|c| c.replace(h_i)) != h_i {
            crate::trace::trace(format!("size request height -> {h_i}"));
        }
    }
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

/// The error thumbnail surface, decoded once (cairo surfaces are not
/// Send — a thread-local cache, the marker pattern; the per-frame
/// decode ran for every failed-thumb item on every frame).
fn error_surface() -> Option<cairo::ImageSurface> {
    thread_local! {
        static ERR: Option<cairo::ImageSurface> = cr_image::error_assets::error_thumbnail(256)
            .map(|img| surface_from_rgba(&img.rgba, img.width, img.height));
    }
    ERR.with(|e| e.clone())
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
    // Env-gated frame evidence (`CR_TRACE=1`): the culling window, the
    // drawn item count, the thumb load backlog, and the frame cost.
    let t0 = crate::trace::enabled().then(std::time::Instant::now);
    let mut s = state.borrow_mut();
    s.config.view_height = window.h;
    // The per-frame draw record resets here (the badge count is the
    // probe seam — one settled frame decides).
    s.badge_draws = 0;
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

    // Group headers (all modes while a grouper is set — the C#
    // `IsTopLayout` covers Detail too). The draw RECORDS the arrow
    // zone (`GroupHeaderInformation.ArrowBounds`) for the click
    // split: arrow toggles, label selects the group's items.
    let headers: Vec<(usize, Rect, bool, String, usize)> = s
        .layout
        .group_headers
        .iter()
        .filter(|gh| gh.rect.intersects(&window))
        .map(|gh| {
            let g = &s.view.groups()[gh.group];
            (gh.group, gh.rect, g.collapsed, g.caption.clone(), g.count)
        })
        .collect();
    for (gi, rect, collapsed, caption, count) in &headers {
        ctx.set_source_rgb(pal.window_bg.0, pal.window_bg.1, pal.window_bg.2);
        ctx.rectangle(rect.x, rect.y, rect.w, rect.h);
        ctx.fill().ok();
        ctx.set_source_rgb(pal.fg.0, pal.fg.1, pal.fg.2);
        // The disclosure TRIANGLE (the C# `groupCollapsedImage` /
        // `groupExpandedImage` bitmaps, drawn as vector geometry —
        // the font glyphs render inconsistently across systems):
        // RIGHT = collapsed, DOWN = expanded. One arrow click
        // "rotates" it 90° — the toggle collapses/expands.
        const ARROW: f64 = 12.0;
        let ax = rect.x + 8.0;
        let ay = rect.y + (rect.h - ARROW) / 2.0;
        let half = ARROW / 2.0;
        if *collapsed {
            // Apex at the right edge.
            ctx.move_to(ax, ay);
            ctx.line_to(ax, ay + ARROW);
            ctx.line_to(ax + ARROW, ay + half);
        } else {
            // Apex at the bottom edge (the collapsed triangle
            // rotated 90° clockwise).
            ctx.move_to(ax, ay);
            ctx.line_to(ax + ARROW, ay);
            ctx.line_to(ax + half, ay + ARROW);
        }
        ctx.close_path();
        ctx.fill().ok();
        // The arrow hit zone: the triangle square + slack, the full
        // header height (like the C# bitmap bounds, but an easier
        // target).
        if let Some(gh) = s.layout.group_headers.get_mut(*gi) {
            gh.arrow = Rect::new(ax - 4.0, rect.y, ARROW + 12.0, rect.h);
        }
        ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
        ctx.set_font_size(13.0 * 1.15);
        let baseline = rect.y + rect.h * 0.68;
        let text = format!("{caption} ({count})");
        ctx.move_to(ax + ARROW + 8.0, baseline);
        ctx.show_text(&text).ok();
    }

    // Detail column header strip: the C# `OnDrawColumnHeaders` —
    // each header cell paints clipped to its rect with the framed
    // edge (the 1 px separator the drag handle sits on).
    if layout::header_visible(&s.config) {
        let header = Rect::new(0.0, 0.0, s.config.view_width, s.config.header_height);
        ctx.set_source_rgb(pal.window_bg.0, pal.window_bg.1, pal.window_bg.2);
        ctx.rectangle(header.x, header.y, header.w, header.h);
        ctx.fill().ok();
        ctx.set_source_rgb(pal.fg.0, pal.fg.1, pal.fg.2);
        ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
        ctx.set_font_size(DETAIL_FONT_SIZE);
        // All visible columns — the same list the cells draw (the
        // image-only columns hold their slot).
        let visible: Vec<&Column> = s.detail_columns.iter().filter(|c| c.visible).collect();
        let mut column_lines: Vec<f64> = Vec::new();
        let mut x = header.x + layout::COLUMN_OFFSET_X;
        for column in visible {
            // Clip the caption to its column (the C#
            // `gr.IntersectClip(columnHeaderRectangle)`); a 0-width
            // column shows nothing.
            ctx.save().ok();
            ctx.rectangle(x, header.y, column.width.max(0.0), header.h);
            ctx.clip();
            ctx.move_to(x + 2.0, header.y + header.h * 0.7);
            ctx.show_text(column.name).ok();
            ctx.restore().ok();
            // The framed edge (the C# `DrawStyledRectangle` — the
            // separator the drag handle sits on).
            if column.width > 0.0 {
                ctx.set_source_rgba(pal.fg.0, pal.fg.1, pal.fg.2, 0.35);
                ctx.rectangle(x + column.width - 0.5, header.y + 2.0, 1.0, header.h - 4.0);
                ctx.fill().ok();
                ctx.set_source_rgb(pal.fg.0, pal.fg.1, pal.fg.2);
            }
            column_lines.push(x + column.width - 0.5);
            x += column.width;
        }
        // The thin vertical column lines (a user addition — the C#
        // Detail body has no grid): one 1 px line per column
        // boundary, running from the top THROUGH the header down the
        // rows. Painted under the row content (the banding is
        // translucent, the line stays visible).
        let bottom = s.layout.virtual_size.1.max(s.config.view_height);
        ctx.set_source_rgba(pal.fg.0, pal.fg.1, pal.fg.2, 0.2);
        for lx in column_lines {
            ctx.rectangle(lx, 0.0, 1.0, bottom);
            ctx.fill().ok();
        }
    }

    // Items (culled to the window) — collected first, the draw
    // helpers mutate the thumb cache.
    let visible: Vec<layout::ItemRect> = visible_items(&s.layout, window).copied().collect();
    // The "Series:" stat columns need the per-series aggregates —
    // build once per book set (the C# `ComicBooknistics.Create` over
    // the library books, built on the first stats access).
    if s.config.mode == ItemViewMode::Detail
        && s.series_stats.is_none()
        && s.detail_columns
            .iter()
            .any(|c| c.visible && c.property.starts_with("SeriesStat"))
    {
        let books: Vec<&ComicBook> = s.view.books().iter().collect();
        s.series_stats = Some(series::create(&books, &|b| book_view::proposed_cached(b)));
    }
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
                draw_detail_item(
                    ctx,
                    &mut s,
                    item.display,
                    rect,
                    selected,
                    item.group_row,
                    &pal,
                );
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

    // The resize marker: a full-height line at the dragged column's
    // right edge (`ThemePens.ItemView.ResizeMarker`).
    if let Some(r) = s.resize {
        if let Some(edge) = layout::column_right_edge(&s.detail_columns, r.id) {
            ctx.set_source_rgb(pal.selected_bg.0, pal.selected_bg.1, pal.selected_bg.2);
            ctx.set_line_width(1.0);
            ctx.move_to(edge, 0.0);
            ctx.line_to(edge, s.layout.virtual_size.1);
            ctx.stroke().ok();
        }
    }

    if let Some(t0) = t0 {
        crate::trace::trace(format!(
            "draw_frame win=({:.0},{:.0} {:.0}x{:.0}) items={} thumbs_pending={} {:.1} ms",
            window.x,
            window.y,
            window.w,
            window.h,
            visible.len(),
            s.pending_thumbs,
            t0.elapsed().as_secs_f64() * 1e3
        ));
    }

    queued_thumbs
}

/// `AutoSizeHeader` (`GetAutoHeaderSize`): the widest cell text over
/// the DISPLAYED items + padding, clamped to the C# 0..10000. The
/// scratch context measures with the cell font (Sans 13).
fn autosize_column_state(state: &Rc<RefCell<ItemViewState>>, id: i32) -> f64 {
    let surface =
        cairo::ImageSurface::create(cairo::Format::ARgb32, 1, 1).expect("autosize scratch surface");
    let ctx = cairo::Context::new(&surface).expect("autosize scratch context");
    ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    ctx.set_font_size(DETAIL_FONT_SIZE);
    let mut s = state.borrow_mut();
    let Some(idx) = s.detail_columns.iter().position(|c| c.id == id) else {
        return 0.0;
    };
    let width =
        if !s.detail_columns[idx].is_text_column() && s.detail_columns[idx].name != "Position" {
            s.detail_columns[idx].width
        } else {
            let mut max = 0.0f64;
            for (d, &di) in s.view.display_order().iter().enumerate() {
                let text = if s.detail_columns[idx].name == "Position" {
                    (d + 1).to_string()
                } else {
                    columns::cell_text(&s.detail_columns[idx], s.view.book(di))
                };
                if text.is_empty() {
                    continue;
                }
                if let Ok(ext) = ctx.text_extents(&text) {
                    max = max.max(ext.width());
                }
            }
            (max + 8.0).clamp(0.0, 10000.0)
        };
    s.detail_columns[idx].width = width;
    let cw = s.canvas.width() as f64;
    s.relayout(cw);
    width
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
        // The state markers, the C# row order: fileless before the
        // missing cross (`!IsLinked` vs `IsLinked && FileIsMissing` —
        // mutually exclusive).
        if book.file_path.is_empty() {
            super::item::draw_state_marker(
                ctx,
                (image_area.x, image_area.y, image_area.w, image_area.h),
                fileless_marker().as_ref(),
            );
        }
        if book.file_is_missing {
            super::item::draw_state_marker(
                ctx,
                (image_area.x, image_area.y, image_area.w, image_area.h),
                missing_cross().as_ref(),
            );
        }
        // The "no metadata" tag (the port addition): comics whose
        // scan/open imported nothing. Fileless/missing books carry
        // their own markers and never show it.
        let no_meta = super::item::metadata_missing(book);
        if no_meta {
            super::item::draw_metadata_tag(
                ctx,
                (image_area.x, image_area.y, image_area.w, image_area.h),
            );
            s.badge_draws += 1;
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

/// The fileless marker (`MarkerIsFileLessImage` — the bundled
/// `FilelessMarker.png`), decoded once.
fn fileless_marker() -> Option<cairo::ImageSurface> {
    thread_local! {
        static FILELESS: Option<cairo::ImageSurface> = crate::icon::image_for_name(
            "FilelessMarker",
        )
        .map(|img| surface_from_rgba(&img.rgba, img.width, img.height));
    }
    FILELESS.with(|c| c.clone())
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
    // The "no metadata" tag (the same condition as Thumbnail — the
    // Tile cover carries it too).
    let no_meta = {
        let b = s.view.book(display);
        super::item::metadata_missing(b)
    };
    if no_meta {
        super::item::draw_metadata_tag(
            ctx,
            (image_area.x, image_area.y, image_area.w, image_area.h),
        );
        s.badge_draws += 1;
    }
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
    group_row: usize,
    pal: &crate::theme::Palette,
) {
    // The row band: even rows within their group, never the selected
    // one (`drawInfo.GroupItem % 2 == 0 && !Selected` — the first row
    // of each group bands). The C# row bounds span the full client
    // width (`GetItemBounds` = (clientWidth, ItemRowHeight)).
    if !selected && group_row.is_multiple_of(2) {
        let luminance = pal.base.0 + pal.base.1 + pal.base.2;
        let band = if luminance < 1.5 {
            ROW_BAND_DARK
        } else {
            ROW_BAND_LIGHT
        };
        ctx.set_source_rgba(band.0, band.1, band.2, ROW_BAND_ALPHA);
        let w = rect.w.max(s.config.view_width);
        ctx.rectangle(rect.x, rect.y, w, rect.h);
        ctx.fill().ok();
    }
    let (tr, tg, tb) = if selected { pal.selected_fg } else { pal.fg };
    ctx.set_source_rgb(tr, tg, tb);
    ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    ctx.set_font_size(DETAIL_FONT_SIZE);
    // The row's cell texts, cached per book (the same regex hazard
    // as the captions — the resolver's proposed fallback parses file
    // names). The stats columns resolve LIVE against the series-stats
    // table (it rides the whole book set, not the book).
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
        let text = if column.property.starts_with("SeriesStat") {
            series_stat_text(s.series_stats.as_ref(), column, s.view.book(display))
        } else {
            match column.name {
                "Position" => (display + 1).to_string(),
                _ => texts.get(i).cloned().unwrap_or_default(),
            }
        };
        if text.is_empty() {
            continue;
        }
        // Center the text in the cell (`LineAlignment.Center`) with
        // the per-column alignment and the 2 px inset — the widths
        // measured, not estimated (the old 6.6/char estimate drifts
        // on proportional text).
        let w = ctx.text_extents(&text).map(|e| e.width()).unwrap_or(0.0);
        let tx = match column.alignment {
            columns::ColumnAlignment::Far => cell.x + cell.w - DETAIL_CELL_PAD - w,
            columns::ColumnAlignment::Center => cell.x + (cell.w - w) / 2.0,
            columns::ColumnAlignment::Near => cell.x + DETAIL_CELL_PAD,
        };
        ctx.move_to(tx.max(cell.x), cell.y + cell.h * 0.75);
        ctx.show_text(&text).ok();
    }
}

/// The "Series:" stat columns — the live text against the view's
/// series aggregates (`StatsProvider.Getns` + the
/// `ComicBookSeriesStatistics.*AsText` formats). The table is built
/// before the draw loop (the row draw holds shared borrows only).
fn series_stat_text(
    table: Option<&HashMap<SeriesKey, SeriesStatistics>>,
    column: &Column,
    book: &ComicBook,
) -> String {
    let Some(table) = table else {
        return String::new();
    };
    let key = SeriesKey::of(book, &book_view::proposed_cached(book));
    let Some(stats) = table.get(&key) else {
        return String::new();
    };
    match column.property {
        "SeriesStatCountAsText" => stats.count.to_string(),
        "SeriesStatPageCountAsText" | "SeriesStatPageReadCountAsText" => {
            let n = if column.property == "SeriesStatPageCountAsText" {
                stats.page_count
            } else {
                stats.page_read_count
            };
            // `ComicBook.FormatPages` ("{0} Page(s)" / Unknown).
            if n > 0 {
                format!("{n} Page(s)")
            } else {
                "Unknown".into()
            }
        }
        "SeriesStatReadPercentageAsText" => format!("{}%", stats.read_percentage),
        "SeriesStatMinNumberAsText" => stat_number(stats.first_number),
        "SeriesStatMaxNumberAsText" => stat_number(stats.last_number),
        "SeriesStatMinYearAsText" => format_year(stats.first_year),
        "SeriesStatMaxYearAsText" => format_year(stats.last_year),
        "SeriesStatAverageRating" => cr_core::xml::scalar::net_f32(stats.average_rating),
        "SeriesStatAverageCommunityRating" => {
            cr_core::xml::scalar::net_f32(stats.average_community_rating)
        }
        "SeriesStatGapCountAsText" => {
            if stats.gap_count > 0 {
                stats.gap_count.to_string()
            } else {
                "None".into()
            }
        }
        "SeriesStatLastAddedTime" => cr_engine::display_text::date_text(&stats.last_added_time),
        "SeriesStatLastOpenedTime" => cr_engine::display_text::date_text(&stats.last_opened_time),
        "SeriesStatLastReleasedTime" => {
            cr_engine::display_text::date_text(&stats.last_released_time)
        }
        _ => String::new(),
    }
}

/// `MinNumberAsText`/`MaxNumberAsText`: a non-negative number renders,
/// a negative (the un-numbered sentinel) stays empty.
fn stat_number(n: f32) -> String {
    if n >= 0.0 {
        cr_core::xml::scalar::net_f32(n)
    } else {
        String::new()
    }
}

/// `ComicBook.FormatYear`.
fn format_year(year: i32) -> String {
    if year != -1 {
        year.to_string()
    } else {
        String::new()
    }
}
