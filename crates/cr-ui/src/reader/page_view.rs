//! The reader page widget — the port of `ImageDisplayControl`
//! (`ComicRack.Engine.Display.Forms/ImageDisplayControl.cs`).
//!
//! One image (page or composed spread) on a cairo surface, drawn
//! through the `DisplayOutput` transform: fit modes, zoom/pan,
//! rotation, RTL, and the part grid with binding edges all come from
//! `super::display`. The page bytes come from the Phase 2 `ImagePool`
//! render chain (page-key rotation included).
//!
//! Renderer note (ADR-008): cairo first. The GL renderer slots in
//! behind the same geometry later. Input note: this is the T2 test
//! map (arrows, +/-, R, wheel); the full `MainForm` accelerator map
//! is T4 work.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, Condvar, Mutex};

use gtk4::cairo;
use gtk4::prelude::*;
use gtk4::{gdk, glib, DrawingArea, EventControllerKey, EventControllerScroll, GestureDrag};

use cr_core::model::bitmap_adjustment::BitmapAdjustment;
use cr_core::model::enums::ImageRotation;
use cr_engine::image_pool::ImagePool;
use cr_io::ComicProvider;

use super::display::{
    DisplayConfig, DisplayOutput, ImageFitMode, ImagePartInfo, PartPageToDisplay, RtlReadingMode,
};

/// `ImageDisplayControl.MinimumZoom` / `MaximumZoom`.
pub const MINIMUM_ZOOM: f32 = 1.0;
pub const MAXIMUM_ZOOM: f32 = 8.0;

/// `ImageDisplayControl.AnamorphicTolerance` default.
const ANAMORPHIC_TOLERANCE: f32 = 0.25;

/// Reader background (matches the shell CSS `#202020`).
const BACKGROUND: (f64, f64, f64) = (0.1255, 0.1255, 0.1255);

/// Zoom factor per wheel/key tick (`MainForm` zoom commands step by
/// the same factor).
const ZOOM_STEP: f32 = 1.25;

type PageCallback = Box<dyn Fn(usize, usize)>;

/// A finished background page load (`PageImage` analog). Raw RGBA
/// travels across threads; the cairo surface is built on the main
/// thread.
struct LoadedPage {
    source: String,
    page: usize,
    size: (i32, i32),
    rgba: Option<Vec<u8>>,
}

/// Latest-wins page decode mailbox: rapid page turns collapse to at
/// most one render in flight plus one queued request — the C#
/// `AddToTop` queue behavior in miniature until T6 wires the full
/// queues.
type PageMailbox = Arc<(Mutex<Option<(usize, String)>>, Condvar)>;

struct PageWorker {
    mailbox: PageMailbox,
}

impl PageWorker {
    fn spawn(pool: Arc<ImagePool>, tx: std::sync::mpsc::Sender<LoadedPage>) -> PageWorker {
        let mailbox: PageMailbox = Arc::new((Mutex::new(None), Condvar::new()));
        let mb = Arc::clone(&mailbox);
        let _ = std::thread::Builder::new()
            .name("page-worker".into())
            .spawn(move || loop {
                let request = {
                    let (lock, cvar) = &*mb;
                    let mut pending = lock.lock().expect("page worker lock");
                    if pending.is_none() {
                        pending = cvar.wait(pending).expect("page worker wait");
                    }
                    pending.take()
                };
                let Some((page, source)) = request else {
                    continue;
                };
                let key = cr_image::keys::ImageKey::from_file(
                    source.clone(),
                    Path::new(&source),
                    page,
                    ImageRotation::None,
                );
                let page_key = cr_image::keys::PageKey::new(key, BitmapAdjustment::default());
                let loaded = match pool.render_page(&page_key) {
                    Some(image) => LoadedPage {
                        source: source.clone(),
                        page,
                        size: (image.width as i32, image.height as i32),
                        rgba: Some(image.rgba),
                    },
                    None => LoadedPage {
                        source: source.clone(),
                        page,
                        size: (0, 0),
                        rgba: None,
                    },
                };
                let _ = tx.send(loaded);
            });
        PageWorker { mailbox }
    }

    fn request(&self, page: usize, source: &str) {
        let (lock, cvar) = &*self.mailbox;
        *lock.lock().expect("page request lock") = Some((page, source.to_owned()));
        cvar.notify_one();
    }
}

struct ViewState {
    provider: Option<ComicProvider>,
    /// The comic path as a string — the cache-key location.
    source: String,
    page: usize,
    page_count: usize,
    surface: Option<cairo::ImageSurface>,
    image_size: (i32, i32),
    fit: ImageFitMode,
    fit_only_if_oversized: bool,
    rtl: bool,
    /// The configured mode for portrait images; landscape pages use
    /// `FlipParts` (see the C# `DisplayConfig` getter).
    rtl_mode: RtlReadingMode,
    two_page_navigation: bool,
    auto_rotate: bool,
    rotation: ImageRotation,
    image_zoom: f32,
    visible: ImagePartInfo,
    /// Cached resolution; rebuilt when the config or view size moves.
    cache: Option<(DisplayConfig, DisplayOutput)>,
    page_callback: Option<PageCallback>,
    /// Last drag position — GestureDrag reports offsets cumulative
    /// from the press, `MovePart` consumes per-update deltas.
    drag_last: Option<(f64, f64)>,
    /// The page a background decode is running for (latest wins).
    pending_page: Option<usize>,
    /// Enter the landing page at its last part (backwards navigation
    /// parity: `CurrentPageChanged` picks part = ImagePartCount-1).
    enter_at_last: bool,
    /// Worker + result channel for background page loads.
    worker: PageWorker,
    page_rx: std::sync::mpsc::Receiver<LoadedPage>,
}

impl ViewState {
    /// `ImageDisplayControl.Display` — resolve (and memoize) the
    /// geometry for the current config + view size.
    fn display(&mut self, view: (i32, i32)) -> DisplayOutput {
        let config = self.effective_config(view);
        if let Some((cached_config, out)) = &self.cache {
            if *cached_config == config {
                return out.clone();
            }
        }
        let out = DisplayOutput::create(&config, ANAMORPHIC_TOLERANCE);
        self.cache = Some((config, out.clone()));
        out
    }
    /// The C# `DisplayConfig` getter: auto-rotate landscape pages,
    /// `FlipParts` for landscape, and pair columns
    /// (`two_page_auto_scroll`) only for landscape + navigation.
    fn effective_config(&self, view: (i32, i32)) -> DisplayConfig {
        let landscape = self.image_size.0 > self.image_size.1;
        DisplayConfig {
            view_size: view,
            image_size: self.image_size,
            fit_mode: self.fit,
            fit_only_if_oversized: self.fit_only_if_oversized,
            rtl_mode: if landscape {
                RtlReadingMode::FlipParts
            } else {
                self.rtl_mode
            },
            rtl: self.rtl,
            part: self.visible,
            image_zoom: self.image_zoom,
            zoom: self.image_zoom,
            rotation: if self.auto_rotate && landscape {
                rotate_left(self.rotation)
            } else {
                self.rotation
            },
            two_page_auto_scroll: landscape && self.two_page_navigation,
        }
    }

    fn invalidate(&mut self) {
        self.cache = None;
    }
}

/// `EnumExtensions.RotateLeft` — `(r - 1 + 4) % 4`.
fn rotate_left(r: ImageRotation) -> ImageRotation {
    match r {
        ImageRotation::None => ImageRotation::Rotate270,
        ImageRotation::Rotate90 => ImageRotation::None,
        ImageRotation::Rotate180 => ImageRotation::Rotate90,
        ImageRotation::Rotate270 => ImageRotation::Rotate180,
    }
}

/// `EnumExtensions.RotateRight` — `(r + 1) % 4`.
fn rotate_right(r: ImageRotation) -> ImageRotation {
    match r {
        ImageRotation::None => ImageRotation::Rotate90,
        ImageRotation::Rotate90 => ImageRotation::Rotate180,
        ImageRotation::Rotate180 => ImageRotation::Rotate270,
        ImageRotation::Rotate270 => ImageRotation::None,
    }
}

/// The page widget. Clone-able handle around shared state; each GTK
/// closure owns one clone.
#[derive(Clone)]
pub struct PageView {
    area: DrawingArea,
    state: Rc<RefCell<ViewState>>,
}

impl PageView {
    pub fn new(pool: Arc<ImagePool>) -> PageView {
        let area = DrawingArea::new();
        area.set_hexpand(true);
        area.set_vexpand(true);
        area.set_focusable(true);

        // Background page decode: one worker, latest request wins.
        // Results pump into the main loop with a 10 ms poll while a
        // load is pending (glib 0.22 has no cross-thread channel;
        // a std mpsc + local timeout keeps the dependency surface
        // small).
        let (tx, rx) = std::sync::mpsc::channel::<LoadedPage>();
        let worker = PageWorker::spawn(Arc::clone(&pool), tx);
        let state = Rc::new(RefCell::new(ViewState {
            provider: None,
            source: String::new(),
            page: 0,
            page_count: 0,
            surface: None,
            image_size: (0, 0),
            fit: ImageFitMode::Fit,
            fit_only_if_oversized: false,
            rtl: false,
            rtl_mode: RtlReadingMode::FlipPages,
            two_page_navigation: true,
            auto_rotate: true,
            rotation: ImageRotation::None,
            image_zoom: 1.0,
            visible: ImagePartInfo::EMPTY,
            cache: None,
            page_callback: None,
            drag_last: None,
            pending_page: None,
            enter_at_last: false,
            worker,
            page_rx: rx,
        }));

        let view = PageView {
            area: area.clone(),
            state: Rc::clone(&state),
        };

        let draw_state = Rc::clone(&state);
        area.set_draw_func(move |_, ctx, width, height| {
            draw_frame(ctx, width, height, &draw_state);
        });

        // Finished loads land here (main thread); stale ones drop.
        view.install_key_controller();
        view.install_scroll_controller();
        view.install_pan_controller();
        view
    }

    pub fn widget(&self) -> &DrawingArea {
        &self.area
    }

    /// Notified as `(current_page, page_count)` after every page
    /// change — the shell updates its header from this.
    pub fn set_page_callback(&self, callback: Option<PageCallback>) {
        self.state.borrow_mut().page_callback = callback;
    }

    /// Attaches a comic and loads its first page.
    pub fn open(&self, provider: ComicProvider, path: &Path) -> Result<(), String> {
        let page_count = provider.page_count();
        {
            let mut st = self.state.borrow_mut();
            st.provider = Some(provider);
            st.source = path.to_string_lossy().into_owned();
            st.page = 0;
            st.page_count = page_count;
            st.visible = ImagePartInfo::EMPTY;
            st.image_zoom = 1.0;
            st.rotation = ImageRotation::None;
            st.invalidate();
        }
        if page_count == 0 {
            self.notify_page();
            return Ok(());
        }
        // Bypass the same-page guard: page 0 is the logical page but
        // has no image yet — request it regardless.
        self.request_and_go(0, false);
        Ok(())
    }

    fn notify_page(&self) {
        let st = self.state.borrow();
        if let Some(cb) = &st.page_callback {
            cb(st.page, st.page_count);
        }
    }

    /// Loads page `page` through the ImagePool render chain
    /// (`pagePool.GetPage` parity; synchronous until the queue-backed
    /// pre-caching lands in T6).
    /// Moves to `page` (`ComicDisplayControl.CurrentPageChanged`
    /// parity): the logical page advances immediately — the header
    /// and the page counter track the book, not the decoder — while
    /// the image streams in from the worker (the old page keeps
    /// painting until then, `pagePool.GetPage` behavior).
    fn goto_page(&self, page: usize, enter_at_last: bool) -> bool {
        {
            let st = self.state.borrow();
            if st.provider.is_none()
                || page >= st.page_count
                || (st.page == page && st.pending_page.is_none())
            {
                return false;
            }
        }
        self.request_and_go(page, enter_at_last)
    }

    /// Sets the logical page state and queues the decode. Bypasses
    /// the same-page guard (open() uses it for page 0).
    fn request_and_go(&self, page: usize, enter_at_last: bool) -> bool {
        let mut st = self.state.borrow_mut();
        if st.provider.is_none() || page >= st.page_count {
            return false;
        }
        st.page = page;
        st.enter_at_last = enter_at_last;
        st.visible = ImagePartInfo::EMPTY;
        // The C# display goes blank on a page change: with the new
        // page not in the pool yet, GetImageInfo yields an empty
        // image and the control paints background only.
        st.surface = None;
        st.image_size = (0, 0);
        let needs_pump = st.pending_page.is_none();
        st.pending_page = Some(page);
        st.invalidate();
        let source = st.source.clone();
        st.worker.request(page, &source);
        drop(st);
        if needs_pump {
            self.start_pump();
        }
        self.notify_page();
        true
    }

    /// Polls the worker channel until the pending load lands.
    fn start_pump(&self) {
        let view = self.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(10), move || {
            let mut received = false;
            loop {
                // Bind first: a `while let` scrutinee borrow would
                // live through the body and conflict with
                // `on_page_loaded`'s mutable borrow.
                let received_now = view.state.borrow().page_rx.try_recv();
                match received_now {
                    Ok(loaded) => {
                        received = true;
                        view.on_page_loaded(loaded);
                    }
                    Err(_) => break,
                }
            }
            if received || view.state.borrow().pending_page.is_some() {
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        });
    }

    /// Applies a finished background load. Stale results (older
    /// requests, other comics) drop.
    fn on_page_loaded(&self, loaded: LoadedPage) {
        {
            let mut st = self.state.borrow_mut();
            if st.pending_page != Some(loaded.page) || st.source != loaded.source {
                return;
            }
            st.pending_page = None;
            st.image_size = loaded.size;
            st.surface = loaded.rgba.map(|rgba| {
                image_surface_from_rgba(&rgba, loaded.size.0 as u32, loaded.size.1 as u32)
            });
            // The zoom persists across pages (the C# keeps ImageZoom);
            // only the visible part resets, at the entry edge.
            let count = {
                let (w, h) = (self.area.width(), self.area.height());
                st.display((w, h)).part_count
            };
            st.visible = if st.enter_at_last {
                ImagePartInfo::new(count - 1, (0, 0))
            } else {
                ImagePartInfo::EMPTY
            };
            st.enter_at_last = false;
            st.invalidate();
        }
        self.area.queue_draw();
    }

    fn resolved_display(&self) -> DisplayOutput {
        let mut st = self.state.borrow_mut();
        let (w, h) = (self.area.width(), self.area.height());
        st.display((w, h))
    }

    // ----- navigation (`ImageDisplayControl.DisplayPart` + the
    // ComicDisplayControl page commands) -----

    /// Next part or page. `true` when something moved.
    pub fn next(&self) -> bool {
        let display = self.resolved_display();
        // Empty display: the current page has no image yet (in
        // flight) — keep advancing the book, one page per press.
        if display.is_empty() {
            let page = self.state.borrow().page;
            return self.goto_page(page + 1, false);
        }
        let visible = self.state.borrow().visible;
        if display.is_end_part(visible) {
            let page = self.state.borrow().page;
            return self.goto_page(page + 1, false);
        }
        self.display_part(PartPageToDisplay::Next)
    }

    /// Previous part or page. `true` when something moved.
    pub fn previous(&self) -> bool {
        let display = self.resolved_display();
        if display.is_empty() {
            let page = self.state.borrow().page;
            if page == 0 {
                return false;
            }
            return self.goto_page(page - 1, true);
        }
        let visible = self.state.borrow().visible;
        if display.is_start_part(visible) {
            let page = self.state.borrow().page;
            if page == 0 {
                return false;
            }
            return self.goto_page(page - 1, true);
        }
        self.display_part(PartPageToDisplay::Previous)
    }

    pub fn first_page(&self) -> bool {
        self.goto_page(0, false)
    }

    pub fn last_page(&self) -> bool {
        let last = self.state.borrow().page_count.saturating_sub(1);
        self.goto_page(last, false)
    }

    /// `ImageDisplayControl.DisplayPart` (instant, no smooth
    /// scrolling — the animated variant is GL-renderer work).
    fn display_part(&self, ptd: PartPageToDisplay) -> bool {
        let mut st = self.state.borrow_mut();
        let (w, h) = (self.area.width(), self.area.height());
        let display = st.display((w, h));
        if display.is_empty() {
            return false;
        }
        let visible = st.visible;
        let target = match ptd {
            PartPageToDisplay::First => {
                if display.is_start_part(visible) {
                    return false;
                }
                ImagePartInfo::EMPTY
            }
            PartPageToDisplay::Previous => {
                if display.is_start_part(visible) {
                    return false;
                }
                let fit = display.get_best_part_fit(visible);
                ImagePartInfo::new(fit.part - 1, (0, fit.offset.1))
            }
            PartPageToDisplay::Next => {
                if display.is_end_part(visible) {
                    return false;
                }
                let fit = display.get_best_part_fit(visible);
                ImagePartInfo::new(fit.part + 1, (0, fit.offset.1))
            }
            PartPageToDisplay::Last => {
                if display.is_end_part(visible) {
                    return false;
                }
                ImagePartInfo::new(display.part_count - 1, (0, 0))
            }
        };
        st.visible = target;
        st.invalidate();
        drop(st);
        self.area.queue_draw();
        true
    }

    // ----- zoom (`ImageDisplayControl.DoZoom` / `ZoomTo`) -----

    pub fn zoom_to(&self, zoom: f32) {
        let (w, h) = (self.area.width(), self.area.height());
        self.do_zoom((w / 2, h / 2), zoom);
    }

    pub fn zoom_in(&self) {
        let zoom = self.state.borrow().image_zoom * ZOOM_STEP;
        self.zoom_to(zoom);
    }

    pub fn zoom_out(&self) {
        let zoom = self.state.borrow().image_zoom / ZOOM_STEP;
        self.zoom_to(zoom);
    }

    fn do_zoom(&self, center: (i32, i32), zoom: f32) {
        let zoom = zoom.clamp(MINIMUM_ZOOM, MAXIMUM_ZOOM);
        let mut st = self.state.borrow_mut();
        if st.image_zoom == zoom || st.image_size.0 == 0 {
            return;
        }
        let (w, h) = (self.area.width(), self.area.height());
        let display = st.display((w, h));
        if display.is_empty() {
            return;
        }
        // The C# zooms around `ClientToImage(location)` — the view
        // point in image space (through the inverse transform).
        let inverse = match display.mat.invert() {
            Some(inv) => inv,
            None => return,
        };
        let (cx, cy) = inverse.transform_point(center.0 as f32, center.1 as f32);
        let bounds = display.part_bounds;
        let fx = (cx - bounds.x as f32) / bounds.w as f32;
        let fy = (cy - bounds.y as f32) / bounds.h as f32;
        st.image_zoom = zoom;
        let part0 = display.get_part(0);
        let anchor = (
            part0.x + (part0.w as f32 * fx) as i32,
            part0.y + (part0.h as f32 * fy) as i32,
        );
        st.visible = ImagePartInfo::new(0, (cx as i32 - anchor.0, cy as i32 - anchor.1));
        st.invalidate();
        drop(st);
        self.area.queue_draw();
    }

    // ----- rotation / fit / RTL toggles -----

    pub fn rotate(&self) {
        let mut st = self.state.borrow_mut();
        st.rotation = rotate_right(st.rotation);
        st.invalidate();
        drop(st);
        self.area.queue_draw();
    }

    pub fn set_fit_mode(&self, mode: ImageFitMode) {
        let mut st = self.state.borrow_mut();
        if st.fit == mode {
            return;
        }
        st.fit = mode;
        st.image_zoom = 1.0;
        st.visible = ImagePartInfo::EMPTY;
        st.invalidate();
        drop(st);
        self.area.queue_draw();
    }

    pub fn fit_mode(&self) -> ImageFitMode {
        self.state.borrow().fit
    }

    pub fn set_rtl(&self, rtl: bool) {
        let mut st = self.state.borrow_mut();
        if st.rtl == rtl {
            return;
        }
        st.rtl = rtl;
        st.invalidate();
        drop(st);
        self.area.queue_draw();
    }

    // ----- pan (`ImageDisplayControl.MovePart`, instant) -----

    /// Pans the visible part by `offset` pixels; `true` when the
    /// part moved.
    fn move_part(&self, offset: (i32, i32)) -> bool {
        let mut st = self.state.borrow_mut();
        let (w, h) = (self.area.width(), self.area.height());
        let display = st.display((w, h));
        if display.is_empty() {
            return false;
        }
        let ipi = st.visible;
        let target = (ipi.offset.0 + offset.0, ipi.offset.1 + offset.1);
        let clamped = display.part_offset(ipi.part, target);
        let moved = clamped != ipi.offset;
        st.visible = ImagePartInfo::new(ipi.part, clamped);
        st.invalidate();
        drop(st);
        if moved {
            self.area.queue_draw();
        }
        moved
    }

    // ----- input wiring (the full C# key map lands in T4) -----

    fn install_key_controller(&self) {
        let controller = EventControllerKey::new();
        let view = self.clone();
        controller.connect_key_pressed(move |_, key, _code, modifier| {
            let view = view.clone();
            match key {
                gdk::Key::Right | gdk::Key::Down => {
                    view.next();
                    glib::Propagation::Stop
                }
                gdk::Key::Left | gdk::Key::Up => {
                    view.previous();
                    glib::Propagation::Stop
                }
                gdk::Key::Home => {
                    view.first_page();
                    glib::Propagation::Stop
                }
                gdk::Key::End => {
                    view.last_page();
                    glib::Propagation::Stop
                }
                gdk::Key::r | gdk::Key::R => {
                    view.rotate();
                    glib::Propagation::Stop
                }
                gdk::Key::plus | gdk::Key::KP_Add | gdk::Key::equal => {
                    view.zoom_in();
                    glib::Propagation::Stop
                }
                gdk::Key::minus | gdk::Key::KP_Subtract => {
                    view.zoom_out();
                    glib::Propagation::Stop
                }
                gdk::Key::f | gdk::Key::F if modifier.is_empty() => {
                    let mode = match view.fit_mode() {
                        ImageFitMode::Fit => ImageFitMode::FitWidth,
                        ImageFitMode::FitWidth => ImageFitMode::FitHeight,
                        _ => ImageFitMode::Fit,
                    };
                    view.set_fit_mode(mode);
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            }
        });
        self.area.add_controller(controller);
    }

    fn install_scroll_controller(&self) {
        let controller = EventControllerScroll::new(
            gtk4::EventControllerScrollFlags::VERTICAL | gtk4::EventControllerScrollFlags::DISCRETE,
        );
        let view = self.clone();
        controller.connect_scroll(move |_, _dx, dy| {
            let view = view.clone();
            if dy > 0.0 {
                view.next();
            } else if dy < 0.0 {
                view.previous();
            }
            glib::Propagation::Stop
        });
        self.area.add_controller(controller);
    }

    fn install_pan_controller(&self) {
        let controller = GestureDrag::new();
        let view = self.clone();
        controller.connect_drag_begin(move |gesture, x, y| {
            let view = view.clone();
            view.state.borrow_mut().drag_last = Some((x, y));
            gesture.set_state(gtk4::EventSequenceState::Claimed);
        });
        let view = self.clone();
        controller.connect_drag_update(move |gesture, x, y| {
            let view = view.clone();
            let _ = gesture.start_point();
            let mut st = view.state.borrow_mut();
            let Some((lx, ly)) = st.drag_last else {
                return;
            };
            st.drag_last = Some((x, y));
            drop(st);
            // GestureDrag reports positions cumulative from the
            // press; MovePart consumes deltas.
            view.move_part(((x - lx) as i32, (y - ly) as i32));
        });
        let view = self.clone();
        controller.connect_drag_end(move |_gesture, _x, _y| {
            let view = view.clone();
            view.state.borrow_mut().drag_last = None;
        });
        self.area.add_controller(controller);
    }
}

/// Converts the decoded RGBA page into a cairo ARGB32 surface
/// (premultiplied; comic pages are opaque, so this is an R/B swap).
fn image_surface_from_rgba(rgba: &[u8], width: u32, height: u32) -> cairo::ImageSurface {
    let stride = width as usize * 4;
    let mut argb = vec![0u8; stride * height as usize];
    for (src, dst) in rgba
        .as_chunks::<4>()
        .0
        .iter()
        .zip(argb.as_chunks_mut::<4>().0)
    {
        let a = u32::from(src[3]);
        // Premultiply (identity for opaque pages).
        let r = ((u32::from(src[0]) * a) / 255) as u8;
        let g = ((u32::from(src[1]) * a) / 255) as u8;
        let b = ((u32::from(src[2]) * a) / 255) as u8;
        dst[0] = b;
        dst[1] = g;
        dst[2] = r;
        dst[3] = src[3];
    }
    cairo::ImageSurface::create_for_data(
        argb,
        cairo::Format::ARgb32,
        width as i32,
        height as i32,
        stride as i32,
    )
    .expect("valid image surface")
}

fn draw_frame(ctx: &cairo::Context, width: i32, height: i32, state: &Rc<RefCell<ViewState>>) {
    let mut st = state.borrow_mut();
    let display = st.display((width, height));

    // Background first, always in identity space.
    ctx.identity_matrix();
    ctx.set_source_rgb(BACKGROUND.0, BACKGROUND.1, BACKGROUND.2);
    ctx.rectangle(0.0, 0.0, f64::from(width), f64::from(height));
    let _ = ctx.fill();

    if display.is_empty() {
        return;
    }
    let Some(surface) = &st.surface else {
        return;
    };
    // The C# render chain draws the PART SOURCE rectangle through
    // the transform (`RenderImage`: DrawImage(destination, source)
    // with source = Display.PartBounds). The matrix positions the
    // part; the pattern is offset so image pixel (part.x, part.y)
    // sits at user-space origin.
    let bounds = display.part_bounds;
    let m = &display.mat;
    ctx.set_matrix(cairo::Matrix::new(
        f64::from(m.e[0]),
        f64::from(m.e[1]),
        f64::from(m.e[2]),
        f64::from(m.e[3]),
        f64::from(m.e[4]),
        f64::from(m.e[5]),
    ));
    ctx.set_source_surface(surface, -f64::from(bounds.x), -f64::from(bounds.y))
        .ok();
    ctx.rectangle(0.0, 0.0, f64::from(bounds.w), f64::from(bounds.h));
    let _ = ctx.fill();
    ctx.identity_matrix();
}
