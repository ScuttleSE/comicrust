//! The reader page widget — the port of `ImageDisplayControl` plus
//! its `ComicDisplayControl` shell
//! (`ComicRack.Engine.Display.Forms/*.cs`).
//!
//! One virtual image (a page, a two-page spread, or the continuous
//! strip) drawn through the `DisplayOutput` transform: fit modes,
//! zoom/pan, rotation, RTL, and the part grid with binding edges all
//! come from `super::display`. Pages decode on a background worker
//! (latest-wins) and compose per the C# `GetImageInfo`/`DrawImage`
//! rules.
//!
//! Renderer note (ADR-008): cairo first. Fade/slide transitions and
//! the paper texture are cairo-native; the paging bow animation and
//! the magnifier wait for the GL renderer. Input note: this is the
//! T2/T3 test map (arrows, +/-, R, F, L, S, D, C); the full
//! `MainForm` accelerator map is T4 work.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use gtk4::cairo;
use gtk4::prelude::*;
use gtk4::{gdk, glib, DrawingArea, EventControllerKey, EventControllerScroll, GestureDrag};

use cr_core::model::bitmap_adjustment::BitmapAdjustment;
use cr_core::model::enums::ImageRotation;

use super::continuous::{ContinuousPageLayout, SourcePage};
use super::display::{
    DisplayConfig, DisplayOutput, ImageFitMode, ImagePartInfo, PartPageToDisplay, Rect,
    RtlReadingMode,
};
use cr_engine::image_pool::ImagePool;
use cr_io::ComicProvider;

/// `ImageDisplayControl.MinimumZoom` / `MaximumZoom`.
pub const MINIMUM_ZOOM: f32 = 1.0;
pub const MAXIMUM_ZOOM: f32 = 8.0;

/// `ImageDisplayControl.AnamorphicTolerance` default.
const ANAMORPHIC_TOLERANCE: f32 = 0.25;

/// Fallback background (shell CSS `#202020`) for `Color` mode.
const DEFAULT_BACKGROUND: (f64, f64, f64) = (0.1255, 0.1255, 0.1255);

/// Zoom factor per wheel/key tick (`MainForm` zoom commands step by
/// the same factor).
const ZOOM_STEP: f32 = 1.25;

/// `EngineConfiguration.BlendDuration` default.
const BLEND_DURATION_MS: u64 = 400;

/// `ContinuousPageLayout` fallback width (`ContinuousFallbackWidth`).
const CONTINUOUS_FALLBACK_WIDTH: i32 = 1000;

/// How many neighbor pages around the current one stay decoded.
const PAGE_WINDOW: usize = 4;

// ----- C# display enums (Engine/Display/*.cs) -----

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PageLayoutMode {
    #[default]
    Single,
    Double,
    DoubleAdaptive,
    Continuous,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PageTransitionEffect {
    None,
    #[default]
    Fade,
    LeftRight,
    TopDown,
    Paging,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageBackgroundMode {
    Auto,
    #[default]
    Color,
    Texture,
}

/// A decoded page kept for composition (`PageImage` analog).
struct LoadedPageData {
    surface: cairo::ImageSurface,
    size: (i32, i32),
    /// `GetAutoBackgroundColor` — the sampled page-corner color.
    auto_background: (f32, f32, f32),
}

/// One page inside the virtual image: destination rect in virtual
/// coordinates plus the source rect in page pixels (`DrawPage`
/// signature: image, destination, source).
#[derive(Clone, Debug)]
struct PagePlacement {
    page: usize,
    dest: Rect,
    source: (i32, i32, i32, i32),
}

/// The composed "image" the part machinery sees (`ImageInfo` analog).
#[derive(Clone, Debug)]
struct Composition {
    size: (i32, i32),
    pages: Vec<PagePlacement>,
}

/// A running page transition (`BlendAnimation` analog).
struct TransitionAnim {
    old: Composition,
    old_surfaces: HashMap<usize, cairo::ImageSurface>,
    old_display: DisplayOutput,
    start: Instant,
    effect: PageTransitionEffect,
    backward: bool,
}

type PageCallback = Box<dyn Fn(usize, usize)>;

/// A finished background page load. Raw RGBA travels across threads;
/// the cairo surface is built on the main thread.
struct LoadedPage {
    source: String,
    page: usize,
    size: (i32, i32),
    rgba: Option<Vec<u8>>,
    auto_background: (f32, f32, f32),
}

/// Latest-wins page decode mailbox: at most one render in flight,
/// one queued request — the C# `AddToTop` queue behavior in
/// miniature until the full queues are wired.
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
                let (size, rgba, auto_background) = match pool.render_page(&page_key) {
                    Some(image) => {
                        let background =
                            auto_background_color(&image.rgba, image.width, image.height);
                        (
                            (image.width as i32, image.height as i32),
                            Some(image.rgba),
                            background,
                        )
                    }
                    None => ((0, 0), None, (0.0, 0.0, 0.0)),
                };
                let _ = tx.send(LoadedPage {
                    source,
                    page,
                    size,
                    rgba,
                    auto_background,
                });
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
    worker: PageWorker,
    page_rx: std::sync::mpsc::Receiver<LoadedPage>,
    provider: Option<ComicProvider>,
    /// The comic path as a string — the cache-key location.
    source: String,
    page: usize,
    page_count: usize,
    last_read: usize,
    /// Decoded pages (bounded to the window around the current one).
    loaded: HashMap<usize, LoadedPageData>,
    /// The page decode currently in flight.
    in_flight: Option<usize>,
    /// Decodes queued behind the in-flight one.
    wanted: VecDeque<usize>,
    /// The composed virtual image; `None` while the needed pages are
    /// still decoding (the C# renders blank meanwhile).
    composition: Option<Composition>,
    /// Continuous strip layout.
    continuous: Option<ContinuousPageLayout>,
    /// Last drawn continuous viewport top (virtual coordinates).
    continuous_viewport_top: i64,
    /// Cached strip content width (`continuousContentWidth` — reset
    /// on open, fit change, or a decoded-size mismatch).
    continuous_content_width: i32,
    /// Known page pixel sizes for the continuous layout.
    continuous_page_sizes: HashMap<usize, (i32, i32)>,
    fit: ImageFitMode,
    fit_only_if_oversized: bool,
    rtl: bool,
    /// The configured mode for portrait images; landscape pages use
    /// `FlipParts` (see the C# `DisplayConfig` getter).
    rtl_mode: RtlReadingMode,
    page_layout: PageLayoutMode,
    /// `DoublePageOverlap` (defaults to 0 — no spine overlap).
    double_page_overlap: f32,
    transition: PageTransitionEffect,
    background_mode: ImageBackgroundMode,
    /// White-composited paper texture (`workingPaperTexture`).
    paper: Option<cairo::ImageSurface>,
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
    transition_anim: Option<TransitionAnim>,
    /// Enter the landing page at its last part (backwards navigation
    /// parity: `CurrentPageChanged` picks part = ImagePartCount-1).
    enter_at_last: bool,
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

    /// The C# `DisplayConfig` getter with the `ComicDisplayControl`
    /// overrides: continuous mode forces FitWidth, clears rotation
    /// and RTL, and uses the strip's total size as the image size.
    fn effective_config(&self, view: (i32, i32)) -> DisplayConfig {
        if self.page_layout == PageLayoutMode::Continuous {
            let total = self
                .continuous
                .as_ref()
                .map(|l| l.total_size())
                .unwrap_or((0, 0));
            return DisplayConfig {
                view_size: view,
                image_size: total,
                fit_mode: match self.fit {
                    ImageFitMode::BestFit | ImageFitMode::FitHeight | ImageFitMode::Fit => {
                        ImageFitMode::FitWidth
                    }
                    other => other,
                },
                fit_only_if_oversized: self.fit_only_if_oversized,
                rtl_mode: RtlReadingMode::FlipParts,
                rtl: false,
                part: self.visible,
                image_zoom: self.image_zoom,
                zoom: self.image_zoom,
                rotation: ImageRotation::None,
                two_page_auto_scroll: false,
            };
        }
        let image_size = self.composition.as_ref().map(|c| c.size).unwrap_or((0, 0));
        // `IsDoubleImage` parity: a composed spread is never treated
        // as landscape (no auto-rotate, no FlipParts, no paired
        // part grid — `flag` in the C# DisplayConfig getter).
        let is_double = self.composition.as_ref().is_some_and(|c| c.pages.len() > 1);
        let landscape = image_size.0 > image_size.1 && !is_double;
        DisplayConfig {
            view_size: view,
            image_size,
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

    /// Evicts decoded pages outside the window around `page`.
    fn trim_loaded(&mut self, page: usize) {
        let keep = |n: usize| n.abs_diff(page) <= PAGE_WINDOW;
        self.loaded.retain(|n, _| keep(*n));
    }

    /// Queues `page` and its composition neighbor for decode.
    fn queue_for(&mut self, page: usize) {
        self.wanted.clear();
        if !self.loaded.contains_key(&page) {
            self.wanted.push_back(page);
        }
        match self.page_layout {
            PageLayoutMode::Single => {}
            PageLayoutMode::Double | PageLayoutMode::DoubleAdaptive => {
                let neighbor = page + 1;
                if neighbor < self.page_count && !self.loaded.contains_key(&neighbor) {
                    self.wanted.push_back(neighbor);
                }
            }
            PageLayoutMode::Continuous => {
                for offset in 1..=PAGE_WINDOW {
                    let n = page + offset;
                    if n < self.page_count && !self.loaded.contains_key(&n) {
                        self.wanted.push_back(n);
                    }
                }
                if page > 0 && !self.loaded.contains_key(&(page - 1)) {
                    self.wanted.push_back(page - 1);
                }
            }
        }
        self.trim_loaded(page);
    }

    /// Dispatches the next wanted page to the idle worker.
    fn dispatch_next(&mut self) {
        if self.in_flight.is_some() {
            return;
        }
        while let Some(candidate) = self.wanted.pop_front() {
            if !self.loaded.contains_key(&candidate) {
                self.in_flight = Some(candidate);
                let source = self.source.clone();
                self.worker.request(candidate, &source);
                return;
            }
        }
    }

    /// Recomputes the composition for the current state.
    fn recompose(&mut self) {
        match self.page_layout {
            PageLayoutMode::Single | PageLayoutMode::Double | PageLayoutMode::DoubleAdaptive => {
                self.composition = self.compose_current();
            }
            PageLayoutMode::Continuous => {
                self.rebuild_continuous_layout();
                self.composition = None;
            }
        }
        self.invalidate();
    }

    /// `ComicDisplayControl.GetImageInfo` + the `DrawImage`
    /// placement: single page, forced double, or a two-page spread
    /// with the RTL/cover rules (page types default to Story until
    /// ComicInfo page metadata reaches the reader).
    fn compose_current(&self) -> Option<Composition> {
        let current = self.loaded.get(&self.page)?;
        let two_page = matches!(
            self.page_layout,
            PageLayoutMode::Double | PageLayoutMode::DoubleAdaptive
        );
        let neighbor = if two_page {
            self.loaded.get(&(self.page + 1))
        } else {
            None
        };
        // A spread needs both pages portrait (landscape pages stand
        // alone), and both present.
        let spread = match neighbor {
            Some(next) => {
                two_page
                    && current.size.1 > current.size.0
                    && next.size.1 > next.size.0
                    && next.size.0 > 0
            }
            None => false,
        };
        if !spread {
            let (w, h) = current.size;
            if w <= 0 || h <= 0 {
                return None;
            }
            // `IsForcedDoublePage` (Double mode, portrait page, no
            // second page): the C# draws the page once at natural
            // aspect in one slot — right for normal pages, left for
            // the cover (`a`/`b` flags after the flag3 swap) — the
            // other slot stays background. No stretching.
            if two_page && self.page_layout == PageLayoutMode::Double && h > w {
                let extra = (w as f32 * (1.0 - self.double_page_overlap)) as i32;
                let comp_w = w + extra;
                let cover_left = self.page == 0;
                let dest = if cover_left {
                    Rect::new(0, 0, w, h)
                } else {
                    Rect::new(comp_w - w, 0, w, h)
                };
                return Some(Composition {
                    size: (comp_w, h),
                    pages: vec![PagePlacement {
                        page: self.page,
                        dest,
                        source: (0, 0, w, h),
                    }],
                });
            }
            return Some(Composition {
                size: (w, h),
                pages: vec![PagePlacement {
                    page: self.page,
                    dest: Rect::new(0, 0, w, h),
                    source: (0, 0, w, h),
                }],
            });
        }
        let next = neighbor.expect("checked");
        compose_spread(SpreadInput {
            current_page: self.page,
            current_size: current.size,
            next_page: self.page + 1,
            next_size: next.size,
            rtl_flip: self.rtl && self.rtl_mode == RtlReadingMode::FlipPages,
            overlap: self.double_page_overlap,
        })
    }

    /// `RebuildContinuousLayout` — strip geometry from known page
    /// sizes, preserving the viewport anchor.
    fn rebuild_continuous_layout(&mut self) {
        let count = self.page_count;
        let mut sources = Vec::with_capacity(count);
        for page in 0..count {
            let size = self
                .continuous_page_sizes
                .get(&page)
                .copied()
                .unwrap_or((CONTINUOUS_FALLBACK_WIDTH, CONTINUOUS_FALLBACK_WIDTH));
            sources.push(SourcePage {
                page,
                source_size: size,
            });
        }
        // Original fit keeps native widths (max width is the content
        // width); other fits scale to the cached content width
        // (`GetContinuousContentWidth` caches until invalidated).
        let preserve = self.fit == ImageFitMode::Original;
        let content_width = if self.continuous_content_width > 0 {
            self.continuous_content_width
        } else if preserve {
            sources
                .iter()
                .filter(|s| s.source_size.0 > 0)
                .map(|s| s.source_size.0)
                .max()
                .unwrap_or(CONTINUOUS_FALLBACK_WIDTH)
        } else {
            self.loaded
                .get(&self.page)
                .map(|p| p.size.0)
                .filter(|w| *w > 0)
                .unwrap_or(CONTINUOUS_FALLBACK_WIDTH)
        };
        self.continuous_content_width = content_width;
        // Rebuilds only when the inputs changed (the C# schedules
        // rebuilds on a size mismatch; rebuilding per decoded page
        // would reset the scroll position).
        if let Some(layout) = &self.continuous {
            if layout.matches(&sources, content_width, preserve) {
                return;
            }
        }
        let anchor = self.continuous_viewport_anchor();
        let layout = ContinuousPageLayout::new(&sources, content_width, preserve);
        let y = layout.resolve_anchor(anchor);
        self.continuous = Some(layout);
        self.visible = ImagePartInfo::new(0, (0, y as i32));
    }

    /// `CaptureContinuousViewportAnchor` — anchored at the viewport
    /// top (the current part offset).
    /// `CaptureContinuousViewportAnchor` — anchored at the drawn
    /// viewport top (`base.PagePartBounds` parity: the part position
    /// carries the scroll, not the offset).
    fn continuous_viewport_anchor(&self) -> super::continuous::Anchor {
        if let Some(layout) = &self.continuous {
            return layout.capture_anchor(self.continuous_viewport_top);
        }
        super::continuous::Anchor::new(self.page, 0.0)
    }
}

fn rotate_left(r: ImageRotation) -> ImageRotation {
    match r {
        ImageRotation::None => ImageRotation::Rotate270,
        ImageRotation::Rotate90 => ImageRotation::None,
        ImageRotation::Rotate180 => ImageRotation::Rotate90,
        ImageRotation::Rotate270 => ImageRotation::Rotate180,
    }
}

fn rotate_right(r: ImageRotation) -> ImageRotation {
    match r {
        ImageRotation::None => ImageRotation::Rotate90,
        ImageRotation::Rotate90 => ImageRotation::Rotate180,
        ImageRotation::Rotate180 => ImageRotation::Rotate270,
        ImageRotation::Rotate270 => ImageRotation::None,
    }
}

/// `ComicDisplayControl.GetAutoBackgroundColor`: average the four
/// 4x4 page corners; pick the brighter set on a dark page, the
/// darker one on a bright page. Returns 0..1 floats.
fn auto_background_color(rgba: &[u8], width: u32, height: u32) -> (f32, f32, f32) {
    let (w, h) = (width as usize, height as usize);
    if w < 8 || h < 8 || rgba.len() < w * h * 4 {
        return (0.0, 0.0, 0.0);
    }
    let corner = |x0: usize, y0: usize| -> (f32, f32, f32) {
        let mut acc = (0u32, 0u32, 0u32);
        for y in y0..y0 + 4 {
            for x in x0..x0 + 4 {
                let i = (y * w + x) * 4;
                acc.0 += u32::from(rgba[i]);
                acc.1 += u32::from(rgba[i + 1]);
                acc.2 += u32::from(rgba[i + 2]);
            }
        }
        let n = 16.0_f32;
        (
            acc.0 as f32 / n / 255.0,
            acc.1 as f32 / n / 255.0,
            acc.2 as f32 / n / 255.0,
        )
    };
    let corners = [
        corner(2, 2),
        corner(w - 2 - 4, 2),
        corner(w - 2 - 4, h - 2 - 4),
        corner(2, h - 2 - 4),
    ];
    let brightness = |c: (f32, f32, f32)| 0.3 * c.0 + 0.59 * c.1 + 0.11 * c.2;
    let avg: f32 = corners.iter().map(|c| brightness(*c)).sum::<f32>() / 4.0;
    let best = if avg < 0.5 {
        corners
            .iter()
            .copied()
            .max_by(|a, b| brightness(*a).total_cmp(&brightness(*b)))
            .expect("non-empty")
    } else {
        corners
            .iter()
            .copied()
            .min_by(|a, b| brightness(*a).total_cmp(&brightness(*b)))
            .expect("non-empty")
    };
    best
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
            worker,
            page_rx: rx,
            provider: None,
            source: String::new(),
            page: 0,
            page_count: 0,
            last_read: 0,
            loaded: HashMap::new(),
            in_flight: None,
            wanted: VecDeque::new(),
            composition: None,
            continuous: None,
            continuous_viewport_top: 0,
            continuous_content_width: 0,
            continuous_page_sizes: HashMap::new(),
            fit: ImageFitMode::Fit,
            fit_only_if_oversized: false,
            rtl: false,
            rtl_mode: RtlReadingMode::FlipPages,
            page_layout: PageLayoutMode::Single,
            double_page_overlap: 0.0,
            transition: PageTransitionEffect::Fade,
            background_mode: ImageBackgroundMode::Color,
            paper: None,
            two_page_navigation: true,
            // `AutoRotate` defaults to false (workspace
            // `[DefaultValue(false)]`; the MainForm toggles it).
            auto_rotate: false,
            rotation: ImageRotation::None,
            image_zoom: 1.0,
            visible: ImagePartInfo::EMPTY,
            cache: None,
            page_callback: None,
            drag_last: None,
            transition_anim: None,
            enter_at_last: false,
        }));

        let view = PageView {
            area: area.clone(),
            state: Rc::clone(&state),
        };

        let draw_state = Rc::clone(&state);
        {
            let draw_area = area.clone();
            let draw_area_for_fn = draw_area.clone();
            draw_area.set_draw_func(move |_, ctx, width, height| {
                draw_frame(ctx, &draw_area_for_fn, width, height, &draw_state);
            });
        }

        view.install_key_controller();
        view.install_scroll_controller();
        view.install_pan_controller();
        view
    }

    pub fn widget(&self) -> &DrawingArea {
        &self.area
    }

    /// Notified as `(current_page, page_count)` after every logical
    /// page change — the shell updates its header from this.
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
            st.last_read = 0;
            st.loaded.clear();
            st.continuous = None;
            st.continuous_page_sizes.clear();
            st.continuous_content_width = 0;
            st.composition = None;
            st.visible = ImagePartInfo::EMPTY;
            st.image_zoom = 1.0;
            st.rotation = ImageRotation::None;
            st.in_flight = None;
            st.wanted.clear();
            st.transition_anim = None;
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

    /// Moves to `page` (`ComicDisplayControl.CurrentPageChanged`
    /// parity): the logical page advances immediately — the header
    /// and the page counter track the book, not the decoder — while
    /// the image composes in the background. A snapshot of the old
    /// frame feeds the transition animation.
    fn goto_page(&self, page: usize, enter_at_last: bool) -> bool {
        {
            let st = self.state.borrow();
            if st.provider.is_none()
                || page >= st.page_count
                || (st.page == page && st.composition.is_some())
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
        let backward = page < st.page;
        let snapshot = if st.transition != PageTransitionEffect::None {
            st.transition_anim.take();
            take_transition_snapshot(&st, backward)
        } else {
            None
        };
        st.page = page;
        st.last_read = st.last_read.max(page);
        st.enter_at_last = enter_at_last;
        // Continuous mode positions the viewport at the target
        // page's top instead of blanking (`CurrentPageChanged` →
        // continuous scroll restore).
        if st.page_layout == PageLayoutMode::Continuous {
            st.recompose();
            if let Some(layout) = st.continuous.as_ref() {
                let top = layout.resolve_anchor(super::continuous::Anchor::new(page, 0.0));
                st.visible = ImagePartInfo::new(0, (0, top as i32));
            }
        } else {
            st.visible = ImagePartInfo::EMPTY;
            // The C# display goes blank on a page change: with the
            // new page not composed yet, GetImageInfo yields an
            // empty image and the control paints background only.
            st.composition = None;
        }
        st.invalidate();
        st.queue_for(page);
        st.transition_anim = snapshot;
        let idle = st.in_flight.is_none();
        drop(st);
        if idle {
            self.state.borrow_mut().dispatch_next();
            let still_idle = self.state.borrow().in_flight.is_none();
            if still_idle {
                // Every needed page is already decoded — compose now
                // (navigating back to cached pages never waits for a
                // load that will not happen).
                self.finish_page_setup();
            }
            self.start_pump();
        }
        self.notify_page();
        true
    }

    /// Recomposes after the decode set for the current page changed
    /// (shared by `on_page_loaded` and the already-cached path).
    fn finish_page_setup(&self) {
        let enter_at_last = {
            let mut st = self.state.borrow_mut();
            st.recompose();
            let ready = st.composition.is_some();
            if !ready {
                false
            } else {
                st.enter_at_last && st.page_layout != PageLayoutMode::Continuous
            }
        };
        if enter_at_last {
            let mut st = self.state.borrow_mut();
            let (w, h) = (self.area.width(), self.area.height());
            let count = st.display((w, h)).part_count;
            st.visible = ImagePartInfo::new(count - 1, (0, 0));
            st.enter_at_last = false;
            st.invalidate();
        }
        self.area.queue_draw();
    }

    /// Applies a finished background load; recomposes and dispatches
    /// the next queued decode.
    fn on_page_loaded(&self, loaded: LoadedPage) {
        {
            let mut st = self.state.borrow_mut();
            if Some(loaded.page) == st.in_flight && loaded.source == st.source {
                st.in_flight = None;
            }
            if loaded.source != st.source {
                return; // stale result from another comic
            }
            if loaded.size.0 > 0 {
                st.continuous_page_sizes.insert(loaded.page, loaded.size);
                st.loaded.insert(
                    loaded.page,
                    LoadedPageData {
                        surface: image_surface_from_rgba(
                            &loaded.rgba.expect("non-empty size implies rgba"),
                            loaded.size.0 as u32,
                            loaded.size.1 as u32,
                        ),
                        size: loaded.size,
                        auto_background: loaded.auto_background,
                    },
                );
                // Continuous mode keeps `LastPageRead` ahead.
                if st.page_layout == PageLayoutMode::Continuous {
                    st.last_read = st.last_read.max(loaded.page);
                }
            }
            st.dispatch_next();
        }
        self.finish_page_setup();
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
            let more = {
                let st = view.state.borrow();
                st.in_flight.is_some()
            };
            if received || more {
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        });
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
        // Continuous mode scrolls one viewport (`DisplayPart(Next)`
        // with the strip anchor following).
        if self.state.borrow().page_layout == PageLayoutMode::Continuous {
            return self.display_part(PartPageToDisplay::Next);
        }
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
            return self.goto_page(page + self.page_step(), false);
        }
        self.display_part(PartPageToDisplay::Next)
    }

    /// `DisplayNextPage` PagingMode.Double: two pages per turn while
    /// a spread is displayed, one from a single-page view.
    fn page_step(&self) -> usize {
        let st = self.state.borrow();
        let two_page = matches!(
            st.page_layout,
            PageLayoutMode::Double | PageLayoutMode::DoubleAdaptive
        );
        let spread = st.composition.as_ref().is_some_and(|c| c.pages.len() > 1);
        if two_page && spread {
            2
        } else {
            1
        }
    }

    /// Previous part or page. `true` when something moved.
    pub fn previous(&self) -> bool {
        if self.state.borrow().page_layout == PageLayoutMode::Continuous {
            return self.display_part(PartPageToDisplay::Previous);
        }
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
            let step = self.page_step();
            return self.goto_page(page.saturating_sub(step), true);
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
        // Continuous mode scrolls part 0's offset across the whole
        // strip (the C# keeps the scroll in the offset; the clamp in
        // `GetClampedPartOffset` runs against the full image). One
        // step = one viewport height.
        if st.page_layout == PageLayoutMode::Continuous {
            let tile = display.get_part(0).h;
            let total = st
                .continuous
                .as_ref()
                .map(|l| l.total_height())
                .unwrap_or(0);
            let max_top = (total - i64::from(tile)).max(0);
            // Derive from the visible state (always part 0 in this
            // model); the drawn-top fallback stays for rebuilds.
            let current = i64::from(st.visible.offset.1).min(max_top);
            let top = match ptd {
                PartPageToDisplay::Next => current + i64::from(tile),
                PartPageToDisplay::Previous => current - i64::from(tile),
                PartPageToDisplay::First => 0,
                PartPageToDisplay::Last => max_top,
            }
            .clamp(0, max_top);
            if top == current {
                return false;
            }
            st.visible = ImagePartInfo::new(0, (0, top as i32));
            st.invalidate();
            let layout = st.continuous.as_ref();
            let hit = layout
                .and_then(|l| l.hit_test(top))
                .map(|hit_page| hit_page.page);
            if let Some(hit) = hit.filter(|hit| *hit != st.page) {
                st.page = hit;
                st.last_read = st.last_read.max(hit);
                st.queue_for(hit);
                let idle = st.in_flight.is_none();
                drop(st);
                if idle {
                    self.state.borrow_mut().dispatch_next();
                    self.start_pump();
                }
                self.notify_page();
                self.area.queue_draw();
                return true;
            }
            drop(st);
            self.area.queue_draw();
            return true;
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
        if st.image_zoom == zoom || st.composition.is_none() {
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

    // ----- rotation / fit / RTL / layout toggles -----

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
        // Continuous layouts depend on the fit (Original preserves
        // source sizes); the content width re-derives.
        st.continuous_content_width = 0;
        st.recompose();
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
        st.recompose();
        drop(st);
        self.area.queue_draw();
    }

    /// `ComicDisplay.PageLayout`.
    pub fn set_page_layout(&self, mode: PageLayoutMode) {
        let mut st = self.state.borrow_mut();
        if st.page_layout == mode {
            return;
        }
        st.page_layout = mode;
        st.visible = ImagePartInfo::EMPTY;
        st.image_zoom = 1.0;
        st.recompose();
        let current = st.page;
        st.queue_for(current);
        let needs_pump = st.in_flight.is_none();
        drop(st);
        if needs_pump {
            self.state.borrow_mut().dispatch_next();
            self.start_pump();
        }
        self.area.queue_draw();
    }

    pub fn page_layout(&self) -> PageLayoutMode {
        self.state.borrow().page_layout
    }

    /// Cycles the background mode; Texture loads the bundled
    /// checkered paper (the C# keeps the file in the workspace
    /// settings — Phase 5/7 wiring).
    pub fn cycle_background(&self) {
        let next = match self.state.borrow().background_mode {
            ImageBackgroundMode::Color => ImageBackgroundMode::Auto,
            ImageBackgroundMode::Auto => ImageBackgroundMode::Texture,
            ImageBackgroundMode::Texture => ImageBackgroundMode::Color,
        };
        let mut st = self.state.borrow_mut();
        st.background_mode = next;
        // The bundled paper rides the Texture mode until the
        // preferences dialogs land (Phase 5): Color/Auto clear the
        // overlay, Texture loads the checkered paper.
        st.paper = if next == ImageBackgroundMode::Texture {
            Self::bundled_paper("Checkered.jpg")
        } else {
            None
        };
        st.invalidate();
        drop(st);
        self.area.queue_draw();
    }

    fn bundled_paper(name: &str) -> Option<cairo::ImageSurface> {
        let bytes = std::fs::read(name).ok().or_else(|| {
            std::fs::read(format!("assets/papers/{name}"))
                .ok()
                .or_else(|| std::fs::read(format!("crates/cr-ui/assets/papers/{name}")).ok())
        })?;
        let image = cr_image::decode::decode(&bytes).ok()?;
        Some(white_composited(
            &image.rgba,
            image.width,
            image.height,
            1.0,
        ))
    }

    pub fn set_transition(&self, effect: PageTransitionEffect) {
        self.state.borrow_mut().transition = effect;
    }

    pub fn set_background_mode(&self, mode: ImageBackgroundMode) {
        let mut st = self.state.borrow_mut();
        st.background_mode = mode;
        st.invalidate();
        drop(st);
        self.area.queue_draw();
    }

    /// `ComicDisplayControl.PaperTexture` + strength — loads the
    /// texture and pre-composites it over white
    /// (`CreateWorkingPaperTexture`).
    pub fn set_paper_texture(&self, path: Option<&Path>, strength: f32) {
        let mut st = self.state.borrow_mut();
        st.paper = path.and_then(|p| {
            let bytes = std::fs::read(p).ok()?;
            let image = cr_image::decode::decode(&bytes).ok()?;
            let working = white_composited(&image.rgba, image.width, image.height, strength);
            Some(working)
        });
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
        // Continuous mode: the visible page follows the viewport.
        if st.page_layout == PageLayoutMode::Continuous {
            let top = i64::from(clamped.1);
            let hit = st
                .continuous
                .as_ref()
                .and_then(|l| l.hit_test(top))
                .map(|h| h.page);
            if let Some(hit) = hit.filter(|hit| *hit != st.page) {
                st.page = hit;
                st.last_read = st.last_read.max(hit);
                st.queue_for(hit);
                let needs_pump = st.in_flight.is_none();
                drop(st);
                if needs_pump {
                    self.state.borrow_mut().dispatch_next();
                    self.start_pump();
                }
                self.notify_page();
                if moved {
                    self.area.queue_draw();
                }
                return moved;
            }
        }
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
                gdk::Key::_1 => {
                    view.set_page_layout(PageLayoutMode::Single);
                    glib::Propagation::Stop
                }
                gdk::Key::_2 => {
                    view.set_page_layout(PageLayoutMode::Double);
                    glib::Propagation::Stop
                }
                gdk::Key::_3 => {
                    view.set_page_layout(PageLayoutMode::DoubleAdaptive);
                    glib::Propagation::Stop
                }
                gdk::Key::_4 => {
                    view.set_page_layout(PageLayoutMode::Continuous);
                    glib::Propagation::Stop
                }
                // Temporary test hook until the Phase 5 preferences:
                // cycle Color → Auto → Texture (bundled checkered
                // paper). 'P' = paper.
                gdk::Key::p | gdk::Key::P => {
                    view.cycle_background();
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

/// Snapshot of the outgoing frame for the transition (`BlendAnimation`
/// captures old page renders the same way). `backward` records the
/// direction for the slide effects.
fn take_transition_snapshot(st: &ViewState, backward: bool) -> Option<TransitionAnim> {
    let composition = st.composition.clone()?;
    if composition.pages.is_empty() {
        return None;
    }
    let mut old_surfaces = HashMap::new();
    for placement in &composition.pages {
        if let Some(data) = st.loaded.get(&placement.page) {
            old_surfaces.insert(placement.page, data.surface.clone());
        }
    }
    let old_display = st.cache.as_ref().map(|(_, out)| out.clone())?;
    Some(TransitionAnim {
        old: composition,
        old_surfaces,
        old_display,
        start: Instant::now(),
        effect: st.transition,
        backward,
    })
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

/// `CreateWorkingPaperTexture` — the paper composited over white at
/// the given strength.
fn white_composited(rgba: &[u8], width: u32, height: u32, strength: f32) -> cairo::ImageSurface {
    let stride = width as usize * 4;
    let mut argb = vec![255u8; stride * height as usize];
    let s = strength.clamp(0.0, 1.0);
    for (src, dst) in rgba
        .as_chunks::<4>()
        .0
        .iter()
        .zip(argb.as_chunks_mut::<4>().0)
    {
        // White base with the paper drawn at alpha=strength on top.
        let blend = |base: u8, v: u8| (f32::from(base) * (1.0 - s) + f32::from(v) * s) as u8;
        dst[0] = blend(255, src[2]);
        dst[1] = blend(255, src[1]);
        dst[2] = blend(255, src[0]);
        dst[3] = 255;
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

fn draw_frame(
    ctx: &cairo::Context,
    area: &DrawingArea,
    width: i32,
    height: i32,
    state: &Rc<RefCell<ViewState>>,
) {
    let mut notify: Option<(usize, usize)> = None;
    {
        let mut st = state.borrow_mut();
        let display = st.display((width, height));

        // Background first, always in identity space
        // (`RenderImageBackground`).
        let background = match st.background_mode {
            ImageBackgroundMode::Auto => st
                .loaded
                .get(&st.page)
                .map(|p| p.auto_background)
                .map(|(r, g, b)| (f64::from(r), f64::from(g), f64::from(b)))
                .unwrap_or(DEFAULT_BACKGROUND),
            _ => DEFAULT_BACKGROUND,
        };
        ctx.identity_matrix();
        ctx.set_source_rgb(background.0, background.1, background.2);
        ctx.rectangle(0.0, 0.0, f64::from(width), f64::from(height));
        let _ = ctx.fill();

        if display.is_empty() {
            return;
        }

        // A running transition blends the old frame into the new one.
        if let Some(anim) = &st.transition_anim {
            let elapsed = anim.start.elapsed().as_millis() as f64;
            let p = (elapsed / BLEND_DURATION_MS as f64).clamp(0.0, 1.0);
            let anim_done = p >= 1.0;
            let effect = anim.effect;
            let backward = anim.backward;
            let old = (anim.old.clone(), anim.old_surfaces.clone());
            let old_display = anim.old_display.clone();
            let new_comp = st.composition.clone();
            let background = match st.background_mode {
                ImageBackgroundMode::Auto => st
                    .loaded
                    .get(&st.page)
                    .map(|pg| pg.auto_background)
                    .map(|(r, g, b)| (f64::from(r), f64::from(g), f64::from(b)))
                    .unwrap_or(DEFAULT_BACKGROUND),
                _ => DEFAULT_BACKGROUND,
            };
            let paper = st.paper.clone();
            drop(st);

            draw_transition_frame(
                ctx,
                width,
                height,
                &old,
                &old_display,
                new_comp.as_ref(),
                state,
                effect,
                backward,
                p,
                background,
                paper.as_ref(),
            );
            if anim_done {
                state.borrow_mut().transition_anim = None;
            } else {
                // Keep ticking until the blend completes.
                let view = PageView {
                    area: area.clone(),
                    state: Rc::clone(state),
                };
                glib::timeout_add_local_once(std::time::Duration::from_millis(16), move || {
                    view.area.queue_draw();
                });
            }
            return;
        }

        if st.page_layout == PageLayoutMode::Continuous {
            draw_continuous(ctx, &display, &mut st);
        } else {
            let comp = match &st.composition {
                Some(comp) => comp.clone(),
                None => return,
            };
            let paper = st.paper.clone();
            let paper_mode = st.background_mode == ImageBackgroundMode::Texture;
            let background = match st.background_mode {
                ImageBackgroundMode::Auto => st
                    .loaded
                    .get(&st.page)
                    .map(|pg| pg.auto_background)
                    .map(|(r, g, b)| (f64::from(r), f64::from(g), f64::from(b)))
                    .unwrap_or(DEFAULT_BACKGROUND),
                _ => DEFAULT_BACKGROUND,
            };
            draw_composition(
                ctx,
                &display,
                &comp,
                &st.loaded,
                paper.as_ref(),
                paper_mode,
                background,
            );
        }

        // Continuous: the logical page follows the viewport top.
        if st.page_layout == PageLayoutMode::Continuous {
            let top = i64::from(st.visible.offset.1);
            let hit = st
                .continuous
                .as_ref()
                .and_then(|l| l.hit_test(top))
                .map(|h| h.page);
            if let Some(hit) = hit.filter(|hit| *hit != st.page) {
                st.page = hit;
                st.last_read = st.last_read.max(hit);
                notify = Some((st.page, st.page_count));
                st.queue_for(hit);
                if st.in_flight.is_none() {
                    st.dispatch_next();
                    let view = PageView {
                        area: area.clone(),
                        state: Rc::clone(state),
                    };
                    view.start_pump();
                }
            }
        }
    }
    if let Some((page, count)) = notify {
        let st = state.borrow();
        if let Some(cb) = &st.page_callback {
            cb(page, count);
        }
    }
}

/// Draws one composed frame through the part transform
/// (`RenderImage` → `DrawImage(destination, source)` per page).
fn draw_composition(
    ctx: &cairo::Context,
    display: &DisplayOutput,
    comp: &Composition,
    loaded: &HashMap<usize, LoadedPageData>,
    paper: Option<&cairo::ImageSurface>,
    paper_mode: bool,
    background: (f64, f64, f64),
) {
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
    // Clip to the part source window.
    ctx.rectangle(0.0, 0.0, f64::from(bounds.w), f64::from(bounds.h));
    ctx.clip();
    // The frame owns its background (`RenderImageSafe` with
    // background): blank slots must not show whatever rendered
    // underneath (the previous frame during transitions).
    ctx.set_source_rgb(background.0, background.1, background.2);
    ctx.rectangle(0.0, 0.0, f64::from(bounds.w), f64::from(bounds.h));
    let _ = ctx.fill();
    for placement in &comp.pages {
        let Some(data) = loaded.get(&placement.page) else {
            continue;
        };
        let (sx, sy, sw, sh) = placement.source;
        if sw <= 0 || sh <= 0 || placement.dest.w <= 0 || placement.dest.h <= 0 {
            continue;
        }
        ctx.save().ok();
        // The part matrix is part-local: composition coordinates
        // shift by the part window origin (same as the continuous
        // strip placement).
        ctx.translate(
            f64::from(placement.dest.x - bounds.x),
            f64::from(placement.dest.y - bounds.y),
        );
        ctx.scale(
            f64::from(placement.dest.w) / f64::from(sw),
            f64::from(placement.dest.h) / f64::from(sh),
        );
        ctx.set_source_surface(&data.surface, -f64::from(sx), -f64::from(sy))
            .ok();
        ctx.rectangle(f64::from(sx), f64::from(sy), f64::from(sw), f64::from(sh));
        let _ = ctx.fill();
        ctx.restore().ok();
    }
    // Paper texture MULTIPLY over the visible page area
    // (`RenderImageEffect`).
    if let Some(paper) = paper {
        let pattern = cairo::SurfacePattern::create(paper);
        pattern.set_extend(cairo::Extend::Repeat);
        ctx.set_source(pattern).ok();
        ctx.set_operator(cairo::Operator::Multiply);
        ctx.rectangle(0.0, 0.0, f64::from(bounds.w), f64::from(bounds.h));
        let _ = ctx.fill();
        ctx.set_operator(cairo::Operator::Over);
        if paper_mode {
            // Texture background mode tiles the paper behind the
            // pages too; done in the background pass.
        }
    }
    ctx.identity_matrix();
}

/// `DrawContinuousImage` — the visible strip pages through the part
/// transform.
fn draw_continuous(ctx: &cairo::Context, display: &DisplayOutput, st: &mut ViewState) {
    let Some(layout) = st.continuous.as_ref() else {
        return;
    };
    let bounds = display.part_bounds;
    // The viewport top lives in virtual coordinates (part position +
    // offset) — the anchor for layout rebuilds reads it from here.
    st.continuous_viewport_top = i64::from(bounds.y);
    let viewport = Rect::new(bounds.x, bounds.y, bounds.w, bounds.h);
    let visible: Vec<(usize, Rect)> = layout
        .get_visible(&viewport)
        .into_iter()
        .map(|p| (p.page, p.bounds))
        .collect();
    let m = &display.mat;
    ctx.set_matrix(cairo::Matrix::new(
        f64::from(m.e[0]),
        f64::from(m.e[1]),
        f64::from(m.e[2]),
        f64::from(m.e[3]),
        f64::from(m.e[4]),
        f64::from(m.e[5]),
    ));
    ctx.rectangle(0.0, 0.0, f64::from(bounds.w), f64::from(bounds.h));
    ctx.clip();
    for (page, page_bounds) in visible {
        let Some(data) = st.loaded.get(&page) else {
            continue;
        };
        ctx.save().ok();
        // The part matrix is part-local: strip coordinates shift by
        // the part window origin (`DrawContinuousImage` maps the
        // source-window intersection the same way).
        ctx.translate(
            f64::from(page_bounds.x - viewport.x),
            f64::from(page_bounds.y - viewport.y),
        );
        let kw = f64::from(page_bounds.w) / f64::from(data.size.0.max(1));
        let kh = f64::from(page_bounds.h) / f64::from(data.size.1.max(1));
        ctx.scale(kw, kh);
        ctx.set_source_surface(&data.surface, 0.0, 0.0).ok();
        ctx.rectangle(0.0, 0.0, f64::from(data.size.0), f64::from(data.size.1));
        let _ = ctx.fill();
        ctx.restore().ok();
    }
    ctx.identity_matrix();
}

/// One animation frame (`FadeInBlending` / the scroll blends). The
/// old frame fades/slides out while the new one fades/slides in.
#[allow(clippy::too_many_arguments)] // mirrors the C# blender signature
#[allow(clippy::type_complexity)]
fn draw_transition_frame(
    ctx: &cairo::Context,
    width: i32,
    height: i32,
    old: &(Composition, HashMap<usize, cairo::ImageSurface>),
    old_display: &DisplayOutput,
    new_comp: Option<&Composition>,
    state: &Rc<RefCell<ViewState>>,
    effect: PageTransitionEffect,
    backward: bool,
    p: f64,
    background: (f64, f64, f64),
    paper: Option<&cairo::ImageSurface>,
) {
    let (old_comp, old_surfaces) = old;
    // Background.
    ctx.identity_matrix();
    ctx.set_source_rgb(background.0, background.1, background.2);
    ctx.rectangle(0.0, 0.0, f64::from(width), f64::from(height));
    let _ = ctx.fill();

    let old_loaded = fake_loaded_map(old_surfaces);

    // The new frame's geometry (the composition is ready when the
    // new page has arrived; until then only the old frame shows).
    let new_display = new_comp.map(|_| {
        let mut st = state.borrow_mut();
        st.display((width, height))
    });

    match effect {
        PageTransitionEffect::LeftRight | PageTransitionEffect::TopDown => {
            // `PageForward`/`PageBackward`: the old frame stays put;
            // the new one slides in from the leading edge (right/down
            // forward, left/up backward).
            let horizontal = effect == PageTransitionEffect::LeftRight;
            let span = if horizontal {
                f64::from(width)
            } else {
                f64::from(height)
            };
            let slide = span * (1.0 - p);
            let (dx_new, dy_new) = match (horizontal, backward) {
                (true, false) => (slide, 0.0),
                (true, true) => (-slide, 0.0),
                (false, false) => (0.0, slide),
                (false, true) => (0.0, -slide),
            };
            draw_composition(
                ctx,
                old_display,
                old_comp,
                &old_loaded,
                paper,
                false,
                background,
            );
            if let (Some(comp), Some(display)) = (new_comp, new_display.as_ref()) {
                ctx.save().ok();
                ctx.translate(dx_new, dy_new);
                draw_composition(
                    ctx,
                    display,
                    comp,
                    &state.borrow().loaded,
                    paper,
                    false,
                    background,
                );
                ctx.restore().ok();
            }
        }
        // Fade covers None (no-op at the endpoints) and degrades
        // Paging (the bow animation needs the GL renderer, ADR-008).
        // Both frames fade — the old one OUT (`FadeInBlending`), so
        // areas only the old frame covered return to background.
        _ => {
            ctx.push_group();
            draw_composition(
                ctx,
                old_display,
                old_comp,
                &old_loaded,
                paper,
                false,
                background,
            );
            ctx.pop_group_to_source().ok();
            ctx.paint_with_alpha(1.0 - p).ok();
            if let (Some(comp), Some(display)) = (new_comp, new_display.as_ref()) {
                ctx.push_group();
                draw_composition(
                    ctx,
                    display,
                    comp,
                    &state.borrow().loaded,
                    paper,
                    false,
                    background,
                );
                ctx.pop_group_to_source().ok();
                ctx.paint_with_alpha(p).ok();
            }
        }
    }
}

/// Adapter so transition draws can reuse `draw_composition` with the
/// old surfaces.
fn fake_loaded_map(
    surfaces: &HashMap<usize, cairo::ImageSurface>,
) -> HashMap<usize, LoadedPageData> {
    surfaces
        .iter()
        .map(|(page, surface)| {
            let size = (surface.width(), surface.height());
            (
                *page,
                LoadedPageData {
                    surface: surface.clone(),
                    size,
                    auto_background: (0.0, 0.0, 0.0),
                },
            )
        })
        .collect()
}

/// Inputs for `compose_spread`.
struct SpreadInput {
    current_page: usize,
    current_size: (i32, i32),
    next_page: usize,
    next_size: (i32, i32),
    rtl_flip: bool,
    overlap: f32,
}

/// The `GetImageInfo` + `DrawImage` spread math: two portrait pages
/// scaled to a common height, the cover (page 0) always on the
/// right, RTL FlipPages swapping the order, and the overlap strip
/// hiding on one side.
fn compose_spread(input: SpreadInput) -> Option<Composition> {
    let (w1, h1) = input.current_size;
    let (w2, h2) = input.next_size;
    if w1 <= 0 || h1 <= 0 || w2 <= 0 || h2 <= 0 {
        return None;
    }
    let comp_h = h1.max(h2);
    let s1 = comp_h as f32 / h1 as f32;
    let s2 = comp_h as f32 / h2 as f32;
    let mut ri = (w1 as f32 * s1) as i32;
    let mut r2 = (w2 as f32 * s2) as i32;
    // RTL FlipPages swaps the spread for manga order; the cover
    // (page 0) always sits on the right (`flag3` in the C#).
    let mut swapped = input.current_page != 0 && input.rtl_flip;
    if input.current_page == 0 {
        swapped = !swapped;
    }
    let num7 = (input.overlap * ri as f32) as i32;
    if swapped {
        ri -= num7;
    } else {
        r2 -= num7;
    }
    let ri = ri.max(0);
    let r2 = r2.max(0);
    let comp_w = ri + r2;
    if comp_w <= 0 || comp_h <= 0 {
        return None;
    }
    // The overlap strip hides on one side: swapped spreads trim the
    // current page, normal ones trim the neighbor's left edge
    // (`rect`/`rect2` in the C# DrawImage).
    let pages = if swapped {
        vec![
            PagePlacement {
                page: input.next_page,
                dest: Rect::new(0, 0, r2, comp_h),
                source: (0, 0, (r2 as f32 / s2) as i32, h2),
            },
            PagePlacement {
                page: input.current_page,
                dest: Rect::new(r2, 0, ri, comp_h),
                source: (
                    (num7 as f32 / s1) as i32,
                    0,
                    w1 - (num7 as f32 / s1) as i32,
                    h1,
                ),
            },
        ]
    } else {
        vec![
            PagePlacement {
                page: input.current_page,
                dest: Rect::new(0, 0, ri, comp_h),
                source: (0, 0, (ri as f32 / s1) as i32, h1),
            },
            PagePlacement {
                page: input.next_page,
                dest: Rect::new(ri + num7, 0, r2, comp_h),
                source: (
                    (num7 as f32 / s2) as i32,
                    0,
                    w2 - (num7 as f32 / s2) as i32,
                    h2,
                ),
            },
        ]
    };
    Some(Composition {
        size: (comp_w, comp_h),
        pages,
    })
}

#[cfg(test)]
mod spread_tests {
    use super::*;

    fn spread(current_page: usize, rtl_flip: bool) -> Composition {
        compose_spread(SpreadInput {
            current_page,
            current_size: (800, 1200),
            next_page: current_page + 1,
            next_size: (800, 1200),
            rtl_flip,
            overlap: 0.0,
        })
        .expect("valid spread")
    }

    #[test]
    fn ltr_spread_places_current_left() {
        let comp = spread(4, false);
        assert_eq!(comp.size, (1600, 1200));
        assert_eq!(comp.pages[0].page, 4);
        assert_eq!(comp.pages[0].dest, Rect::new(0, 0, 800, 1200));
        assert_eq!(comp.pages[1].page, 5);
        assert_eq!(comp.pages[1].dest, Rect::new(800, 0, 800, 1200));
    }

    #[test]
    fn cover_sits_on_the_right() {
        // Page 0 = cover: even LTR puts the cover right and page 1
        // left (the C# `flag3` cover rule).
        let comp = spread(0, false);
        assert_eq!(comp.pages[0].page, 1);
        assert_eq!(comp.pages[0].dest, Rect::new(0, 0, 800, 1200));
        assert_eq!(comp.pages[1].page, 0);
        assert_eq!(comp.pages[1].dest, Rect::new(800, 0, 800, 1200));
    }

    #[test]
    fn rtl_swaps_the_order() {
        // Manga: page N on the right, page N+1 on the left.
        let comp = spread(4, true);
        assert_eq!(comp.pages[0].page, 5);
        assert_eq!(comp.pages[0].dest.x, 0);
        assert_eq!(comp.pages[1].page, 4);
        assert_eq!(comp.pages[1].dest.x, 800);
        // The cover stays right even in RTL.
        let comp = spread(0, true);
        assert_eq!(comp.pages[1].page, 0);
        assert_eq!(comp.pages[1].dest.x, 800);
    }

    #[test]
    fn overlap_hides_one_strip() {
        let comp = compose_spread(SpreadInput {
            current_page: 4,
            current_size: (800, 1200),
            next_page: 5,
            next_size: (800, 1200),
            rtl_flip: false,
            overlap: 0.2,
        })
        .expect("valid spread");
        // num7 = 160: the total shrinks by the overlap (1440); the
        // neighbor's left strip hides and its dest overlaps the
        // boundary.
        assert_eq!(comp.size, (1440, 1200));
        assert_eq!(comp.pages[1].dest, Rect::new(960, 0, 640, 1200));
        assert_eq!(comp.pages[1].source, (160, 0, 640, 1200));
    }

    #[test]
    fn unequal_heights_scale_to_common() {
        let comp = compose_spread(SpreadInput {
            current_page: 0,
            current_size: (800, 1200),
            next_page: 1,
            next_size: (600, 600),
            rtl_flip: false,
            overlap: 0.0,
        })
        .expect("valid spread");
        // The short page scales up to 1200 high → 1200 wide. Page 0
        // is the cover → swapped: the neighbor sits left (1200
        // wide), the cover right (800 wide).
        assert_eq!(comp.size, (2000, 1200));
        assert_eq!(comp.pages[0].dest, Rect::new(0, 0, 1200, 1200));
        assert_eq!(comp.pages[1].dest, Rect::new(1200, 0, 800, 1200));
    }
}
