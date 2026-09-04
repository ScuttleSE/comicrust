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

/// The theme colors (dark, matching `theme.rs`; the C# uses the
/// system colors).
const BG: (f64, f64, f64) = (0.13, 0.13, 0.15);
const TEXT: (f64, f64, f64) = (0.88, 0.88, 0.9);
const SELECT_BG: (f64, f64, f64) = (0.2, 0.38, 0.62);
const SELECT_TEXT: (f64, f64, f64) = (1.0, 1.0, 1.0);
const FOCUS_UNFOCUSED: (f64, f64, f64) = (0.5, 0.5, 0.55);
const GROUP_BG: (f64, f64, f64) = (0.18, 0.18, 0.21);
const HEADER_BG: (f64, f64, f64) = (0.2, 0.2, 0.23);

type ActivateFn = Box<dyn Fn(&CrGuid)>;
type SelectionFn = Box<dyn Fn(usize)>;

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
    type_ahead: String,
    type_ahead_source: Option<glib::SourceId>,
    canvas: DrawingArea,
}

impl ItemViewState {
    fn relayout(&mut self, canvas_width: f64) {
        if canvas_width > 1.0 {
            self.config.view_width = canvas_width;
        }
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

    /// The caption line (`Comic.Caption`; the display-text resolver's
    /// stand-in until the C# text builder lands).
    fn caption(&self, display: usize) -> String {
        let book = self.view.book(display);
        let prop = cr_engine::matcher::book_view::proposed(book);
        let series = cr_engine::matcher::book_view::shadow_series(book, &prop);
        let number = cr_engine::matcher::book_view::shadow_number(book, &prop);
        if number.is_empty() {
            series.to_string()
        } else {
            format!("{series} #{number}")
        }
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
            pending_thumbs: 0,
            pump_active: false,
            band: None,
            band_start: (0.0, 0.0),
            band_snapshot: HashSet::new(),
            detail_columns: columns::default_columns(),
            on_activate: None,
            on_selection_changed: None,
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

        ItemViewWidgets { scroller, view: iv }
    }

    /// Replaces the book set (a library selection change).
    pub fn set_books(&self, books: Vec<ComicBook>) {
        let width = self.state.borrow().config.view_width;
        {
            let mut s = self.state.borrow_mut();
            s.view = ViewState::new(books);
            s.thumbs.clear();
            s.queued.clear();
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

    /// Takes the keyboard focus onto the grid (the window-activation
    /// re-grab — the reader's dead-first-keypress fix).
    pub fn grab_focus(&self) {
        self.canvas.grab_focus();
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
            s.notify_selection();
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
                s.notify_selection();
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
                            // Handled — swallow the key.
                            s.notify_selection();
                            drop(s);
                            canvas.queue_draw();
                            return glib::Propagation::Stop;
                        }
                    }
                    return glib::Propagation::Proceed;
                }
            }
            s.type_ahead.clear();
            s.notify_selection();
            drop(s);
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
    s.relayout(window.w);
    let (bg_r, bg_g, bg_b) = BG;
    ctx.set_source_rgb(bg_r, bg_g, bg_b);
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
        ctx.set_source_rgb(GROUP_BG.0, GROUP_BG.1, GROUP_BG.2);
        ctx.rectangle(gh.rect.x, gh.rect.y, gh.rect.w, gh.rect.h);
        ctx.fill().ok();
        ctx.set_source_rgb(TEXT.0, TEXT.1, TEXT.2);
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
        ctx.set_source_rgb(HEADER_BG.0, HEADER_BG.1, HEADER_BG.2);
        ctx.rectangle(header.x, header.y, header.w, header.h);
        ctx.fill().ok();
        ctx.set_source_rgb(TEXT.0, TEXT.1, TEXT.2);
        ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
        ctx.set_font_size(12.0);
        let visible: Vec<&Column> = s
            .detail_columns
            .iter()
            .filter(|c| c.visible && c.is_text_column())
            .collect();
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
            ctx.set_source_rgb(SELECT_BG.0, SELECT_BG.1, SELECT_BG.2);
            ctx.rectangle(rect.x - 2.0, rect.y - 2.0, rect.w + 4.0, rect.h + 4.0);
            ctx.fill().ok();
        }

        match s.config.mode {
            ItemViewMode::Thumbnail => {
                draw_thumbnail_item(ctx, &mut s, item.display, rect, selected);
            }
            ItemViewMode::Tile => {
                draw_tile_item(ctx, &mut s, item.display, rect, selected);
            }
            ItemViewMode::Detail => {
                draw_detail_item(ctx, &s, item.display, rect, selected);
            }
        }

        if focused {
            let (r, g, b) = if s.canvas.has_focus() {
                SELECT_BG
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
        ctx.set_source_rgba(SELECT_BG.0, SELECT_BG.1, SELECT_BG.2, 0.3);
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
) {
    let (tr, tg, tb) = if selected { SELECT_TEXT } else { TEXT };
    // The cover: the loaded thumbnail fit into the image area
    // (top-anchored); the error thumbnail on failure; a dark
    // placeholder until the load lands.
    let image_area_h = rect.h - label_height(&s.config);
    let image_area = Rect::new(rect.x, rect.y, rect.w, image_area_h);
    let id = s.view.book(display).id;
    let thumb = match s.thumbs.get(&id) {
        Some(ThumbState::Ready(surface)) => Some(surface.clone()),
        Some(ThumbState::Failed) => error_surface(),
        None => None,
    };
    match thumb {
        Some(surface) => {
            let (iw, ih) = (surface.width() as f64, surface.height() as f64);
            let scale = (image_area.w / iw).min(image_area.h / ih).min(1.0);
            let dw = iw * scale;
            let dx = image_area.x + (image_area.w - dw) / 2.0;
            ctx.save().ok();
            ctx.translate(dx, image_area.y);
            ctx.scale(scale, scale);
            ctx.set_source_surface(&surface, 0.0, 0.0).ok();
            ctx.rectangle(0.0, 0.0, iw, ih);
            ctx.fill().ok();
            ctx.restore().ok();
        }
        None => {
            ctx.set_source_rgb(0.08, 0.08, 0.09);
            ctx.rectangle(
                image_area.x + 8.0,
                image_area.y + 8.0,
                image_area.w - 16.0,
                image_area.h - 16.0,
            );
            ctx.fill().ok();
        }
    }
    // The caption (one line inside the 3-line strip).
    let caption = s.caption(display);
    ctx.set_source_rgb(tr, tg, tb);
    ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    let scale = (s.config.thumb_height / 192.0).clamp(0.7, 1.0);
    ctx.set_font_size(s.config.font_height * scale);
    ctx.move_to(
        rect.x + 2.0,
        rect.y + image_area_h + s.config.font_height * scale,
    );
    ctx.show_text(&caption).ok();
}

fn label_height(config: &LayoutConfig) -> f64 {
    let scale = (config.thumb_height / 192.0).clamp(0.7, 1.0);
    layout::LABEL_LINES * (config.font_height * scale + 2.0)
}

fn draw_tile_item(
    ctx: &cairo::Context,
    s: &mut ItemViewState,
    display: usize,
    rect: Rect,
    selected: bool,
) {
    let (tr, tg, tb) = if selected { SELECT_TEXT } else { TEXT };
    // Cover: the left half; text: the right side (DrawTile).
    let image_area = Rect::new(rect.x, rect.y, rect.w / 2.0, rect.h);
    let id = s.view.book(display).id;
    let thumb = match s.thumbs.get(&id) {
        Some(ThumbState::Ready(surface)) => Some(surface.clone()),
        Some(ThumbState::Failed) => error_surface(),
        None => None,
    };
    match thumb {
        Some(surface) => {
            let (iw, ih) = (surface.width() as f64, surface.height() as f64);
            let scale = (image_area.w / iw).min(image_area.h / ih).min(1.0);
            let dh = ih * scale;
            ctx.save().ok();
            ctx.translate(image_area.x, image_area.y + (image_area.h - dh) / 2.0);
            ctx.scale(scale, scale);
            ctx.set_source_surface(&surface, 0.0, 0.0).ok();
            ctx.rectangle(0.0, 0.0, iw, ih);
            ctx.fill().ok();
            ctx.restore().ok();
        }
        None => {
            ctx.set_source_rgb(0.08, 0.08, 0.09);
            ctx.rectangle(
                image_area.x + 4.0,
                image_area.y + 4.0,
                image_area.w - 8.0,
                image_area.h - 8.0,
            );
            ctx.fill().ok();
        }
    }
    let text_x = rect.x + rect.w / 2.0 + 4.0;
    ctx.save().ok();
    ctx.rectangle(text_x - 2.0, rect.y, rect.x + rect.w - text_x + 2.0, rect.h);
    ctx.clip();
    let book = s.view.book(display);
    let prop = cr_engine::matcher::book_view::proposed(book);
    let series = cr_engine::matcher::book_view::shadow_series(book, &prop);
    ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Bold);
    ctx.set_font_size((s.config.font_height * 1.0).max(10.0));
    ctx.set_source_rgb(tr, tg, tb);
    ctx.move_to(text_x, rect.y + 16.0);
    ctx.show_text(series).ok();
    ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    ctx.set_font_size(10.0);
    ctx.move_to(text_x, rect.y + 30.0);
    let name = std::path::Path::new(&book.file_path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    ctx.show_text(&name).ok();
    ctx.restore().ok();
}

fn draw_detail_item(
    ctx: &cairo::Context,
    s: &ItemViewState,
    display: usize,
    rect: Rect,
    selected: bool,
) {
    let (tr, tg, tb) = if selected { SELECT_TEXT } else { TEXT };
    ctx.set_source_rgb(tr, tg, tb);
    ctx.select_font_face("Sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    ctx.set_font_size(12.0);
    let book = s.view.book(display);
    let visible: Vec<&Column> = s.detail_columns.iter().filter(|c| c.visible).collect();
    let column_rects = layout::detail_column_rects(&s.config, &rect);
    for (i, column) in visible.iter().enumerate() {
        let Some(cell) = column_rects.get(i) else {
            break;
        };
        let text = match column.name {
            "Position" => (display + 1).to_string(),
            "Cover" => String::new(),
            _ => columns::cell_text(column, book),
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
