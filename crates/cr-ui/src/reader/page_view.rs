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
//! the magnifier wait for the GL renderer. Input: the full
//! `MainForm` reader command table (`super::keys`) dispatches keys,
//! wheel/tilt and clicks; pointer drags pan (left) or zoom (middle)
//! like `ImageDisplayControl.OnMouseMove`.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use gtk4::cairo;
use gtk4::prelude::*;
use gtk4::{
    gdk, glib, DrawingArea, EventControllerKey, EventControllerScroll, GestureClick, GestureDrag,
};

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

/// `EngineConfiguration.KeyboardZoomStepping` default (the Z /
/// Shift+Z step zoom commands).
const KEYBOARD_ZOOM_STEPPING: f32 = 0.5;

/// `ComicDisplay.DefaultPageWallTicks` — after a page change, further
/// scroll input within this window is eaten (`EatScrolling`), and
/// part navigation arms the page-change wall (`IsPageChangeWalled`).
const PAGE_WALL: std::time::Duration = std::time::Duration::from_millis(300);

/// Drag distance before a press becomes a pan/zoom
/// (`ImageDisplayControl.OnMouseMove` 5 px threshold).
const DRAG_THRESHOLD: f64 = 5.0;

/// Click-dispatch delay — WinForms parks single clicks for the
/// double-click time so a double click cancels the pending single
/// click (`mouseClickTimer`).
const DOUBLE_CLICK_MS: u64 = 400;

/// `EngineConfiguration.BlendDuration` default.
const BLEND_DURATION_MS: u64 = 400;

/// `ContinuousPageLayout` fallback width (`ContinuousFallbackWidth`).
const CONTINUOUS_FALLBACK_WIDTH: i32 = 1000;

/// `ComicDisplayControl.MagnifierSize` default (200, 200).
const MAGNIFIER_SIZE: i32 = 200;

/// `ComicDisplayControl.MagnifierZoom` default (2).
const MAGNIFIER_ZOOM: f32 = 2.0;

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

/// Shell command forwarder (`NextTab`, `PrevTab`,
/// `ToggleUndockReader`, `ToggleMenu`).
type CommandCallback = Rc<dyn Fn(&str)>;

/// One finished pool-queue page render (`AsyncCallback` payload).
/// The comic source rides along so stale results from a previous
/// comic are dropped.
struct PageDone {
    source: String,
    page: usize,
    rotation: ImageRotation,
    image: Option<cr_image::Image>,
}

/// The completion side of the page channel, shared by the queue
/// callbacks (`Sender` is not `Sync`; the slow queue runs several
/// workers).
#[derive(Clone)]
struct PageTx(Arc<Mutex<std::sync::mpsc::Sender<PageDone>>>);

impl PageTx {
    fn new(tx: std::sync::mpsc::Sender<PageDone>) -> PageTx {
        PageTx(Arc::new(Mutex::new(tx)))
    }

    fn send(&self, done: PageDone) {
        if let Ok(tx) = self.0.lock() {
            let _ = tx.send(done);
        }
    }
}

struct ViewState {
    /// The shared render pools — current pages ride the fast queue
    /// (`AddToTop`), prefetches the bottom (`CachePage` parity).
    pool: Arc<ImagePool>,
    /// The completion side of the pool-queue callbacks.
    page_tx: PageTx,
    page_rx: std::sync::mpsc::Receiver<PageDone>,
    provider: Option<ComicProvider>,
    /// The comic path as a string — the cache-key location.
    source: String,
    page: usize,
    page_count: usize,
    last_read: usize,
    /// Decoded pages (bounded to the window around the current one).
    loaded: HashMap<usize, LoadedPageData>,
    /// Pages rendering in the pool queues, with the rotation their
    /// key carries (`queue_for` enqueues, the pump collects).
    queued: HashSet<(usize, ImageRotation)>,
    /// Pages the composition still waits for, newest first.
    wanted: Vec<(usize, ImageRotation)>,
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
    transition_anim: Option<TransitionAnim>,
    /// Enter the landing page at its last part (backwards navigation
    /// parity: `CurrentPageChanged` picks part = ImagePartCount-1).
    enter_at_last: bool,
    // ----- input state (`ComicDisplay` scroll/wall machinery + the
    // pointer handlers) -----
    /// `Program.Settings.AutoScrolling` (S command; session-only
    /// until the settings port).
    auto_scrolling: bool,
    /// `ComicDisplay.ScrollingDoesBrowse` default.
    scrolling_does_browse: bool,
    /// `ComicDisplay.MouseWheelSpeed` default.
    mouse_wheel_speed: f32,
    /// Lines per scroll command (`ComicDisplay.scrollLines` — 1 for
    /// keys, set per wheel event).
    scroll_lines: f32,
    /// Set after every page change; gates scrolling (`EatScrolling`).
    last_paging: Option<Instant>,
    /// Set after every part move/navigation; arms the page-change
    /// wall (`IsPageChangeWalled`).
    last_part_navigation: Option<Instant>,
    /// `WallState` — armed after a walled page change was suppressed.
    wall_pending: bool,
    wall_start: Option<Instant>,
    /// Per-page permanent rotation (`ComicPageInfo.Rotation`, the Y
    /// commands; persistence lands with the T5 write-back).
    page_rotations: HashMap<usize, ImageRotation>,
    /// Drag start point (for the 5 px threshold).
    drag_start: Option<(f64, f64)>,
    /// Last drag position — GestureDrag reports offsets cumulative
    /// from the press, `MovePart` consumes per-update deltas.
    drag_last: Option<(f64, f64)>,
    /// `MouseActionHappened` — a drag past the threshold suppresses
    /// the click dispatch.
    drag_action: bool,
    /// Middle-button drag zoom: press point + starting zoom.
    zoom_drag: Option<((f64, f64), f32)>,
    /// Pending single-click dispatch (cancelled by a double click or
    /// a drag).
    pending_click: Option<glib::SourceId>,
    exit_callback: Option<Box<dyn Fn()>>,
    /// Shell-level commands the widget cannot serve itself (tab
    /// switching, undock) — forwarded to the reader window.
    command_callback: Option<CommandCallback>,
    /// `MagnifierVisible` (the M command) — a zoom lens at the
    /// cursor.
    magnifier: bool,
    /// The lens center (the last cursor position).
    magnifier_at: Option<(f64, f64)>,
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

    /// Queues `page` and its composition neighbors for decode — the
    /// C# `CachePage` pattern: the pages the composition waits for go
    /// first (fast queue, `AddToTop`), prefetches trail
    /// (`CacheBackPage` rides the bottom).
    fn queue_for(&mut self, page: usize) {
        self.wanted.clear();
        let rotation = self.rotation_for(page);
        if !self.loaded.contains_key(&page) {
            self.wanted.push((page, rotation));
        }
        match self.page_layout {
            PageLayoutMode::Single => {}
            PageLayoutMode::Double | PageLayoutMode::DoubleAdaptive => {
                let neighbor = page + 1;
                if neighbor < self.page_count && !self.loaded.contains_key(&neighbor) {
                    self.wanted.push((neighbor, self.rotation_for(neighbor)));
                }
            }
            PageLayoutMode::Continuous => {
                for offset in 1..=PAGE_WINDOW {
                    let n = page + offset;
                    if n < self.page_count && !self.loaded.contains_key(&n) {
                        self.wanted.push((n, self.rotation_for(n)));
                    }
                }
                if page > 0 && !self.loaded.contains_key(&(page - 1)) {
                    self.wanted.push((page - 1, self.rotation_for(page - 1)));
                }
            }
        }
        self.trim_loaded(page);
        self.dispatch_wanted();
    }

    fn rotation_for(&self, page: usize) -> ImageRotation {
        self.page_rotations
            .get(&page)
            .copied()
            .unwrap_or(ImageRotation::None)
    }

    /// The `PageKey` for a page under its current rotation.
    fn page_key(&self, page: usize, rotation: ImageRotation) -> cr_image::keys::PageKey {
        let key = cr_image::keys::ImageKey::from_file(
            self.source.clone(),
            Path::new(&self.source),
            page,
            rotation,
        );
        cr_image::keys::PageKey::new(key, BitmapAdjustment::default())
    }

    /// Enqueues every missing wanted page (`CachePage` →
    /// `AddPageToQueue`: current and forward pages at the top,
    /// backward prefetches at the bottom). Memory hits convert on the
    /// spot; the rest render in the pool queues and report through
    /// the channel. Returns `true` when everything wanted is already
    /// loaded.
    fn dispatch_wanted(&mut self) -> bool {
        let current = self.page;
        let source = self.source.clone();
        let mut all_loaded = true;
        for (page, rotation) in self.wanted.clone() {
            if self.loaded.contains_key(&page) {
                continue;
            }
            let key = self.page_key(page, rotation);
            if let Some(image) = self.pool.get_page_memory(&key) {
                self.insert_loaded_page(page, image);
                continue;
            }
            all_loaded = false;
            if self.queued.contains(&(page, rotation)) {
                continue;
            }
            self.queued.insert((page, rotation));
            let tx = self.page_tx.clone();
            let pool = Arc::clone(&self.pool);
            let done_source = source.clone();
            self.pool.add_page_to_queue(
                key,
                None,
                move |k| {
                    let image = pool.render_page(k);
                    tx.send(PageDone {
                        source: done_source.clone(),
                        page: k.key.index,
                        rotation: k.key.rotation,
                        image,
                    });
                },
                page < current,
            );
        }
        self.wanted
            .retain(|(page, _)| !self.loaded.contains_key(page));
        all_loaded
    }

    /// Turns a finished queue result into a `LoadedPageData`
    /// (including the continuous size bookkeeping).
    fn insert_loaded_page(&mut self, page: usize, image: cr_image::Image) {
        let background = auto_background_color(&image.rgba, image.width, image.height);
        self.continuous_page_sizes
            .insert(page, (image.width as i32, image.height as i32));
        self.loaded.insert(
            page,
            LoadedPageData {
                surface: image_surface_from_rgba(&image.rgba, image.width, image.height),
                size: (image.width as i32, image.height as i32),
                auto_background: background,
            },
        );
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

        // Page loads ride the pool's fast/slow queues (`CachePage`):
        // the queue callback renders and ships the result here; a
        // 10 ms poll drains it while loads are pending (glib 0.22
        // has no cross-thread channel; a std mpsc + local timeout
        // keeps the dependency surface small).
        let (tx, rx) = std::sync::mpsc::channel::<PageDone>();
        let state = Rc::new(RefCell::new(ViewState {
            pool,
            page_tx: PageTx::new(tx),
            page_rx: rx,
            provider: None,
            source: String::new(),
            page: 0,
            page_count: 0,
            last_read: 0,
            loaded: HashMap::new(),
            queued: HashSet::new(),
            wanted: Vec::new(),
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
            auto_scrolling: false,
            scrolling_does_browse: true,
            mouse_wheel_speed: 2.0,
            scroll_lines: 1.0,
            last_paging: None,
            last_part_navigation: None,
            wall_pending: false,
            wall_start: None,
            page_rotations: HashMap::new(),
            drag_start: None,
            drag_action: false,
            zoom_drag: None,
            pending_click: None,
            exit_callback: None,
            command_callback: None,
            magnifier: false,
            magnifier_at: None,
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
        view.install_zoom_drag_controller();
        view.install_click_controller();
        view.install_magnifier_controller();
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
        self.open_with_state(provider, path, 0, 0)
    }

    /// Attaches a comic and resumes at `page` with the `last_read`
    /// high-water mark (the C# `OpenComic`/`TrackCurrentPage` flow;
    /// the navigator clamps both to the page count).
    pub fn open_with_state(
        &self,
        provider: ComicProvider,
        path: &Path,
        page: usize,
        last_read: usize,
    ) -> Result<(), String> {
        let page_count = provider.page_count();
        let page = page.min(page_count.saturating_sub(1));
        let last_read = last_read.min(page_count.saturating_sub(1));
        {
            let mut st = self.state.borrow_mut();
            st.provider = Some(provider);
            st.source = path.to_string_lossy().into_owned();
            st.page = page;
            st.page_count = page_count;
            st.last_read = last_read;
            st.loaded.clear();
            st.queued.clear();
            st.continuous = None;
            st.continuous_page_sizes.clear();
            st.continuous_content_width = 0;
            st.composition = None;
            st.visible = ImagePartInfo::EMPTY;
            st.image_zoom = 1.0;
            st.rotation = ImageRotation::None;
            st.wanted.clear();
            st.transition_anim = None;
            st.page_rotations.clear();
            st.last_paging = None;
            st.last_part_navigation = None;
            st.wall_pending = false;
            st.wall_start = None;
            st.drag_action = false;
            st.zoom_drag = None;
            st.invalidate();
        }
        if page_count == 0 {
            self.notify_page();
            return Ok(());
        }
        // Bypass the same-page guard: the resume page is the logical
        // page but has no image yet — request it regardless.
        self.request_and_go(page, false);
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
        let all_loaded = st.wanted.is_empty();
        drop(st);
        if all_loaded {
            // Every needed page is already decoded — compose now
            // (navigating back to cached pages never waits for a
            // load that will not happen).
            self.finish_page_setup();
        } else {
            self.start_pump();
        }
        self.notify_page();
        true
    }

    /// Recomposes after the decode set for the current page changed
    /// (shared by the load pump and the already-cached path).
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

    /// Applies a finished pool-queue render; recomposes.
    fn on_page_loaded(&self, done: PageDone) {
        {
            let mut st = self.state.borrow_mut();
            if done.source != st.source {
                return; // stale result from another comic
            }
            st.queued.remove(&(done.page, done.rotation));
            match done.image {
                Some(image) => {
                    st.insert_loaded_page(done.page, image);
                    // Continuous mode keeps `LastPageRead` ahead.
                    if st.page_layout == PageLayoutMode::Continuous {
                        st.last_read = st.last_read.max(done.page);
                    }
                    st.wanted.retain(|(page, _)| *page != done.page);
                }
                None => {
                    // Decode failed — the error page takes the slot
                    // (`CreateErrorPage` parity).
                    let surface = error_page_surface();
                    let (w, h) = (surface.width(), surface.height());
                    st.continuous_page_sizes.insert(done.page, (w, h));
                    st.loaded.insert(
                        done.page,
                        LoadedPageData {
                            surface,
                            size: (w, h),
                            auto_background: (0.0, 0.0, 0.0),
                        },
                    );
                    st.wanted.retain(|(page, _)| *page != done.page);
                }
            }
        }
        self.finish_page_setup();
        self.notify_page();
    }

    /// Drains the pool-queue completion channel until the wanted set
    /// is served.
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
                    Ok(done) => {
                        received = true;
                        view.on_page_loaded(done);
                    }
                    Err(_) => break,
                }
            }
            let more = {
                let st = view.state.borrow();
                !st.wanted.is_empty()
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

    // ----- navigation (`ComicDisplay` page + part commands; the
    // command table in `super::keys` dispatches to these) -----

    /// `ComicDisplay.DisplayNextPageOrPart`: next part, at the part
    /// edge the next page (`PagingMode.Double | Walled`).
    pub fn display_next_page_or_part(&self, force_new_page: bool) {
        // Borrow hoisted: an `if`-condition borrow would live to the
        // end of the statement (edition 2021) and collide with the
        // body's `borrow_mut`.
        let continuous = self.state.borrow().page_layout == PageLayoutMode::Continuous;
        if continuous {
            // Continuous mode scrolls one viewport per step
            // (`DisplayPart(Next)` on the strip); page wall does not
            // apply (single part).
            if !self.eat_scrolling() {
                self.display_part(PartPageToDisplay::Next);
            }
            return;
        }
        if !self.eat_scrolling() && (force_new_page || !self.display_part(PartPageToDisplay::Next))
        {
            self.display_next_page(true, true);
        }
    }

    /// `ComicDisplay.DisplayPreviousPageOrPart`.
    pub fn display_previous_page_or_part(&self, force_new_page: bool) {
        let continuous = self.state.borrow().page_layout == PageLayoutMode::Continuous;
        if continuous {
            if !self.eat_scrolling() {
                self.display_part(PartPageToDisplay::Previous);
            }
            return;
        }
        if !self.eat_scrolling()
            && (force_new_page || !self.display_part(PartPageToDisplay::Previous))
        {
            self.display_previous_page(true, true);
        }
    }

    /// `ComicDisplay.DisplayNextPage`: step 2 while a spread is
    /// displayed (`IsDoubleImage` = more than one image visible), 1
    /// otherwise. The `SeekNewPage` Near-position refinement needs
    /// page metadata the reader does not load yet.
    pub fn display_next_page(&self, double_step: bool, walled: bool) {
        if walled && self.is_page_change_walled() {
            return;
        }
        let (page, step) = {
            let st = self.state.borrow();
            let is_double = st.composition.as_ref().is_some_and(|c| c.pages.len() > 1);
            let step = if double_step && is_double { 2 } else { 1 };
            (st.page, step)
        };
        self.state.borrow_mut().last_paging = Some(Instant::now());
        self.goto_page(page + step, false);
    }

    /// `ComicDisplay.DisplayPreviousPage`: the -1/-2 offset — two
    /// pages back in a two-page layout unless the sought page is
    /// invalid. The single-page-type and Near/Far conditions need
    /// page metadata (defaults make them false), so the offset
    /// reduces to the page index.
    pub fn display_previous_page(&self, double_step: bool, walled: bool) {
        if !walled || !self.is_page_change_walled() {
            let (page, two_page) = {
                let st = self.state.borrow();
                (
                    st.page,
                    matches!(
                        st.page_layout,
                        PageLayoutMode::Double | PageLayoutMode::DoubleAdaptive
                    ),
                )
            };
            let step = if two_page && double_step && page >= 2 {
                2
            } else {
                1
            };
            if page < step {
                return; // `Book.Navigate` fails below page 0
            }
            self.state.borrow_mut().last_paging = Some(Instant::now());
            self.goto_page(page - step, true);
        }
    }

    /// `ComicDisplay.EatScrolling`: scrolling right after a page
    /// change is eaten (a multi-notch wheel must not flip several
    /// pages). A single-part display never eats.
    fn eat_scrolling(&self) -> bool {
        let part_count = self.resolved_display().part_count;
        let st = self.state.borrow();
        if part_count == 1 {
            return false;
        }
        st.last_paging
            .is_some_and(|t| Instant::now().duration_since(t) < PAGE_WALL)
    }

    /// `ComicDisplay.IsPageChangeWalled`: page changes within the
    /// wall window after part navigation need a second press (the
    /// first press arms the wall).
    fn is_page_change_walled(&self) -> bool {
        let part_count = self.resolved_display().part_count;
        let mut st = self.state.borrow_mut();
        if part_count == 1 {
            return false;
        }
        let now = Instant::now();
        let armed = st
            .last_part_navigation
            .is_some_and(|t| now.duration_since(t) < PAGE_WALL);
        if !armed {
            st.wall_pending = false;
            return false;
        }
        if !st.wall_pending {
            st.wall_start = Some(now);
            st.wall_pending = true;
            return true;
        }
        if st
            .wall_start
            .is_some_and(|t| now.duration_since(t) < PAGE_WALL)
        {
            return true;
        }
        st.wall_pending = false;
        st.wall_start = Some(now);
        false
    }

    /// `ComicDisplay.GetLineSize` — pan step per scroll line, from
    /// the virtual image size (continuous uses the strip width/16).
    fn line_size(&self) -> (i32, i32) {
        let st = self.state.borrow();
        if st.page_layout == PageLayoutMode::Continuous {
            let w = st
                .continuous
                .as_ref()
                .map(|l| l.total_size().0)
                .unwrap_or(0);
            return (w / 16, w / 16);
        }
        let (w, h) = st.composition.as_ref().map(|c| c.size).unwrap_or((0, 0));
        let is_double = st.composition.as_ref().is_some_and(|c| c.pages.len() > 1);
        (w / if is_double { 32 } else { 16 }, h / 32)
    }

    /// `ComicDisplay.ScrollUp` — pan a line up; at the part edge,
    /// `ScrollingDoesBrowse` turns the page.
    pub fn scroll_up(&self, lines: f32) {
        if self.eat_scrolling() {
            return;
        }
        // Borrows hoisted out of the conditions — a condition borrow
        // would live to the end of the statement and collide with the
        // calls below.
        let (continuous, auto, browse) = {
            let st = self.state.borrow();
            (
                st.page_layout == PageLayoutMode::Continuous,
                st.auto_scrolling,
                st.scrolling_does_browse,
            )
        };
        if continuous {
            self.scroll_up_lines(lines, false);
        } else if auto {
            self.display_previous_page_or_part(false);
        } else {
            self.scroll_up_lines(lines, browse);
        }
    }

    pub fn scroll_down(&self, lines: f32) {
        if self.eat_scrolling() {
            return;
        }
        let (continuous, auto, browse) = {
            let st = self.state.borrow();
            (
                st.page_layout == PageLayoutMode::Continuous,
                st.auto_scrolling,
                st.scrolling_does_browse,
            )
        };
        if continuous {
            self.scroll_down_lines(lines, false);
        } else if auto {
            self.display_next_page_or_part(false);
        } else {
            self.scroll_down_lines(lines, browse);
        }
    }

    /// `ComicDisplay.ScrollLeft` — horizontal scroll never changes
    /// the page; auto-scrolling turns instead (`IsMovementFlipped`
    /// default false).
    pub fn scroll_left(&self, lines: f32) {
        if self.eat_scrolling() {
            return;
        }
        let auto = self.state.borrow().auto_scrolling;
        if auto {
            self.display_previous_page_or_part(false);
        } else {
            let (lw, _) = self.line_size();
            self.move_part((0 - (lines * (lw as f32)) as i32, 0));
        }
    }

    pub fn scroll_right(&self, lines: f32) {
        if self.eat_scrolling() {
            return;
        }
        let auto = self.state.borrow().auto_scrolling;
        if auto {
            self.display_next_page_or_part(false);
        } else {
            let (lw, _) = self.line_size();
            self.move_part(((lines * (lw as f32)) as i32, 0));
        }
    }

    fn scroll_up_lines(&self, lines: f32, with_page_change: bool) -> bool {
        let (_, lh) = self.line_size();
        if self.move_part((0, -((lines * (lh as f32)) as i32))) {
            return true;
        }
        if !with_page_change {
            return false;
        }
        if self.eat_scrolling() {
            return false;
        }
        self.display_previous_page_or_part(true);
        true
    }

    fn scroll_down_lines(&self, lines: f32, with_page_change: bool) -> bool {
        let (_, lh) = self.line_size();
        if self.move_part((0, (lines * (lh as f32)) as i32)) {
            return true;
        }
        if !with_page_change {
            return false;
        }
        self.display_next_page_or_part(true);
        true
    }

    /// `ImageDisplayControl.MovePartDown` — 10% of the output height
    /// per press (V / B, Ctrl+Down / Ctrl+Up).
    pub fn move_part_down(&self, percent: f32) {
        let display = self.resolved_display();
        if display.is_empty() {
            return;
        }
        let dy = (display.output_bounds().h as f32 * percent) as i32;
        self.move_part((0, dy));
    }

    /// Absolute navigation (`ComicBookNavigator.Navigate(page,
    /// Absolute)`): jumps to `page`, clamped; the page callback
    /// mirrors the state. Returns whether the navigation happened.
    pub fn navigate(&self, page: usize) -> bool {
        self.request_and_go(page, false)
    }

    pub fn first_page(&self) -> bool {
        self.goto_page(0, false)
    }

    pub fn last_page(&self) -> bool {
        let last = self.state.borrow().page_count.saturating_sub(1);
        self.goto_page(last, false)
    }

    /// `ImageDisplayControl.DisplayPart` (instant, no smooth
    /// scrolling — the animated variant is GL-renderer work) wrapped
    /// by `ComicDisplay.DisplayPart` (stamps the part navigation on
    /// success).
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
            st.last_part_navigation = Some(Instant::now());
            st.wall_pending = false;
            let layout = st.continuous.as_ref();
            let hit = layout
                .and_then(|l| l.hit_test(top))
                .map(|hit_page| hit_page.page);
            if let Some(hit) = hit.filter(|hit| *hit != st.page) {
                st.page = hit;
                st.last_read = st.last_read.max(hit);
                st.queue_for(hit);
                let needs_pump = !st.wanted.is_empty();
                drop(st);
                if needs_pump {
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
        st.last_part_navigation = Some(Instant::now());
        st.wall_pending = false;
        drop(st);
        self.area.queue_draw();
        true
    }

    // ----- zoom (`ImageDisplayControl.DoZoom` / `ZoomTo`) -----

    /// The `ImageZoom` setter anchor: `Display.PartBounds.GetCenter()`.
    pub fn zoom_to(&self, zoom: f32) {
        let display = self.resolved_display();
        let b = display.part_bounds;
        self.do_zoom((b.x + b.w / 2, b.y + b.h / 2), zoom);
    }

    /// The MainForm zoom commands: `(zoom + delta).Clamp(lo, hi)`
    /// through the `ImageZoom` setter.
    pub fn zoom_add(&self, delta: f32, lo: f32, hi: f32) {
        let current = self.state.borrow().image_zoom;
        self.zoom_to((current + delta).clamp(lo, hi));
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

    /// The MainForm `RotateC` command (`RotateRight()`; the
    /// Continuous guard lives in the dispatch switch).
    pub fn rotate_right(&self) {
        let mut st = self.state.borrow_mut();
        st.rotation = rotate_right(st.rotation);
        st.invalidate();
        drop(st);
        self.area.queue_draw();
    }

    /// The MainForm `RotateCC` command (`RotateLeft()`).
    pub fn rotate_left(&self) {
        let mut st = self.state.borrow_mut();
        st.rotation = rotate_left(st.rotation);
        st.invalidate();
        drop(st);
        self.area.queue_draw();
    }

    /// The MainForm `AutoRotate` command (`ImageAutoRotate` toggle).
    pub fn toggle_auto_rotate(&self) {
        let mut st = self.state.borrow_mut();
        st.auto_rotate = !st.auto_rotate;
        st.invalidate();
        drop(st);
        self.area.queue_draw();
    }

    /// The MainForm `PageRotateC`/`PageRotateCC` commands
    /// (`GetPageEditor().Rotation`): the page decodes with the new
    /// rotation. View-side port — persistence into `ComicPageInfo`
    /// lands with the T5 write-back.
    pub fn page_rotate(&self, right: bool) {
        let mut st = self.state.borrow_mut();
        let page = st.page;
        let current = st
            .page_rotations
            .get(&page)
            .copied()
            .unwrap_or(ImageRotation::None);
        let next = if right {
            rotate_right(current)
        } else {
            rotate_left(current)
        };
        if next == ImageRotation::None {
            st.page_rotations.remove(&page);
        } else {
            st.page_rotations.insert(page, next);
        }
        // Evict the stale decode; the rotated page decodes fresh
        // (sizes change, so the continuous strip re-derives too).
        st.loaded.remove(&page);
        st.continuous_page_sizes.remove(&page);
        st.continuous_content_width = 0;
        st.composition = None;
        st.invalidate();
        st.queue_for(page);
        let needs_pump = !st.wanted.is_empty();
        drop(st);
        if needs_pump {
            self.start_pump();
        }
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
        if mode == PageLayoutMode::Continuous {
            // The C# setter re-anchors the strip at the current page
            // (`RebuildContinuousLayout(Anchor(CurrentPage, 0))`).
            st.continuous = None;
        }
        st.recompose();
        let current = st.page;
        st.queue_for(current);
        let needs_pump = !st.wanted.is_empty();
        drop(st);
        if needs_pump {
            self.start_pump();
        }
        self.area.queue_draw();
    }

    pub fn page_layout(&self) -> PageLayoutMode {
        self.state.borrow().page_layout
    }

    /// The MainForm `ToggleTwoPages` command — the `TogglePageLayout`
    /// cycle (`Continuous` falls back to `Single`).
    pub fn toggle_page_layout(&self) {
        let next = match self.state.borrow().page_layout {
            PageLayoutMode::Single => PageLayoutMode::Double,
            PageLayoutMode::Double => PageLayoutMode::DoubleAdaptive,
            PageLayoutMode::DoubleAdaptive | PageLayoutMode::Continuous => PageLayoutMode::Single,
        };
        self.set_page_layout(next);
    }

    /// The MainForm `ToggleRealisticPages` command. The reader folds
    /// the paper texture into `background_mode` (Texture ↔ Color);
    /// the C# keeps the paper selection in the workspace settings.
    pub fn toggle_realistic_pages(&self) {
        let next = match self.state.borrow().background_mode {
            ImageBackgroundMode::Texture => ImageBackgroundMode::Color,
            _ => ImageBackgroundMode::Texture,
        };
        let mut st = self.state.borrow_mut();
        st.background_mode = next;
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
    /// part moved (`ComicDisplay.MovePart` stamps the part
    /// navigation on success).
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
        if moved {
            st.invalidate();
            st.last_part_navigation = Some(Instant::now());
            st.wall_pending = false;
        }
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
                let needs_pump = !st.wanted.is_empty();
                drop(st);
                if needs_pump {
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

    // ----- input wiring (the `MainForm` command table; `super::keys`) -----

    /// `ReaderFormKeyDown` → `KeyboardShortcuts.HandleKey`: build the
    /// `CommandKey` from the keyval + modifier state and dispatch
    /// through the table.
    fn install_key_controller(&self) {
        let controller = EventControllerKey::new();
        let view = self.clone();
        controller.connect_key_pressed(move |_, key, _code, state| {
            match super::keys::command_key_from_gdk(key, state).and_then(super::keys::command_for) {
                Some(command) => {
                    view.dispatch_command(command.id);
                    glib::Propagation::Stop
                }
                None => glib::Propagation::Proceed,
            }
        });
        self.area.add_controller(controller);
    }

    /// `display_MouseWheel` / `display_MouseHWheel`: wheel and tilt
    /// dispatch through the command table (Ctrl+Wheel = ZoomIn/
    /// ZoomOut); the wheel updates `scrollLines` first. The
    /// right-button wheel switches tabs in the C# — reader tabs land
    /// in T5.
    fn install_scroll_controller(&self) {
        let controller = EventControllerScroll::new(
            gtk4::EventControllerScrollFlags::VERTICAL
                | gtk4::EventControllerScrollFlags::HORIZONTAL
                | gtk4::EventControllerScrollFlags::DISCRETE,
        );
        let view = self.clone();
        controller.connect_scroll(move |controller, dx, dy| {
            let state = controller.current_event_state();
            let mods = super::keys::Mods {
                ctrl: state.contains(gdk::ModifierType::CONTROL_MASK),
                shift: state.contains(gdk::ModifierType::SHIFT_MASK),
                alt: state.contains(gdk::ModifierType::ALT_MASK),
            };
            if dy != 0.0 {
                // `scrollLines = |delta| * MouseWheelSpeed`
                let speed = view.state.borrow().mouse_wheel_speed;
                view.state.borrow_mut().scroll_lines = dy.abs() as f32 * speed;
                let key = if dy < 0.0 {
                    super::keys::Key::MouseWheelUp
                } else {
                    super::keys::Key::MouseWheelDown
                };
                view.dispatch_key(key, mods);
            } else if dx != 0.0 {
                let key = if dx < 0.0 {
                    super::keys::Key::MouseTiltLeft
                } else {
                    super::keys::Key::MouseTiltRight
                };
                view.dispatch_key(key, mods);
            }
            glib::Propagation::Stop
        });
        self.area.add_controller(controller);
    }

    /// `ImageDisplayControl.OnMouseMove` — left-drag pans after the
    /// 5 px threshold (`MouseActionHappened`); the sequence is only
    /// claimed at the threshold so short presses still dispatch as
    /// clicks.
    fn install_pan_controller(&self) {
        let controller = GestureDrag::new();
        controller.set_button(1);
        let view = self.clone();
        controller.connect_drag_begin(move |_, x, y| {
            let mut st = view.state.borrow_mut();
            st.drag_start = Some((x, y));
            st.drag_last = Some((x, y));
            st.drag_action = false;
        });
        let view = self.clone();
        controller.connect_drag_update(move |gesture, x, y| {
            let dist = {
                let st = view.state.borrow();
                match st.drag_start {
                    Some((sx, sy)) => (x - sx).hypot(y - sy),
                    None => 0.0,
                }
            };
            // Decide first, act with the borrow released — the state
            // must not be held while cancel_pending_click borrows.
            let cross = {
                let st = view.state.borrow();
                !st.drag_action && dist > DRAG_THRESHOLD
            };
            if cross {
                view.state.borrow_mut().drag_action = true;
                view.cancel_pending_click();
                gesture.set_state(gtk4::EventSequenceState::Claimed);
            }
            let mut st = view.state.borrow_mut();
            if !st.drag_action {
                return;
            }
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
        controller.connect_drag_end(move |_, _x, _y| {
            let mut st = view.state.borrow_mut();
            st.drag_start = None;
            st.drag_last = None;
            st.drag_action = false;
        });
        self.area.add_controller(controller);
    }

    /// `ImageDisplayControl.OnMouseMove` middle-button branch —
    /// drag-zooms around the press point: `orgZoom + dy / 100`
    /// clamped to the zoom range.
    fn install_zoom_drag_controller(&self) {
        let controller = GestureDrag::new();
        controller.set_button(2);
        let view = self.clone();
        controller.connect_drag_begin(move |gesture, x, y| {
            let org = view.state.borrow().image_zoom;
            view.state.borrow_mut().zoom_drag = Some(((x, y), org));
            gesture.set_state(gtk4::EventSequenceState::Claimed);
        });
        let view = self.clone();
        controller.connect_drag_update(move |_, _x, y| {
            let drag = view.state.borrow().zoom_drag;
            if let Some(((px, py), org)) = drag {
                view.do_zoom((px as i32, py as i32), org + ((y - py) / 100.0) as f32);
            }
        });
        let view = self.clone();
        controller.connect_drag_end(move |_, _x, _y| {
            view.state.borrow_mut().zoom_drag = None;
        });
        self.area.add_controller(controller);
    }

    /// `ImageDisplayControl.OnClick`/`OnDoubleClick`: the single
    /// click dispatch parks for the double-click time; a second
    /// press cancels it and dispatches the double click.
    fn install_click_controller(&self) {
        let controller = GestureClick::new();
        controller.set_button(1);
        let view = self.clone();
        controller.connect_pressed(move |_, n, _x, _y| {
            if n >= 2 {
                view.cancel_pending_click();
                view.dispatch_key(super::keys::Key::MouseDoubleLeft, Default::default());
            }
        });
        let view = self.clone();
        controller.connect_released(move |_, n, _x, _y| {
            // Borrow hoisted out of the condition (see scroll_up).
            if n == 1 {
                let action = view.state.borrow().drag_action;
                if !action {
                    view.schedule_click();
                }
            }
        });
        self.area.add_controller(controller);
    }

    /// Tracks the cursor for the magnifier lens
    /// (`PositionMagnifier`); the lens auto-hides outside the client
    /// rect (`AutoHideMagnifier`).
    fn install_magnifier_controller(&self) {
        let controller = gtk4::EventControllerMotion::new();
        let view = self.clone();
        controller.connect_motion(move |_c, x, y| {
            let redraw = view.state.borrow().magnifier;
            view.state.borrow_mut().magnifier_at = Some((x, y));
            if redraw {
                view.area.queue_draw();
            }
        });
        let view = self.clone();
        controller.connect_leave(move |_| {
            let redraw = view.state.borrow().magnifier;
            view.state.borrow_mut().magnifier_at = None;
            if redraw {
                view.area.queue_draw();
            }
        });
        self.area.add_controller(controller);
    }

    /// Builds a `CommandKey` and dispatches through the table
    /// (`KeyboardShortcuts.HandleKey`).
    fn dispatch_key(&self, key: super::keys::Key, mods: super::keys::Mods) {
        if let Some(command) = super::keys::command_for(super::keys::CommandKey { key, mods }) {
            self.dispatch_command(command.id);
        }
    }

    /// The command switch. The C# wires each id to the display or
    /// the shell (`MainForm.InitializeKeyboard`); ids for features
    /// that land in T5/T6 (undock, menu chrome, magnifier, tabs) or
    /// later phases (bookmarks, the browser list, page write-back)
    /// dispatch as no-ops until their task.
    fn dispatch_command(&self, id: &str) {
        match id {
            // Library group — the browser list is Phase 4.
            "NextComic" | "PrevComic" | "RandomComic" | "ShowBrowser" => {}
            "MoveToFirstPage" => {
                self.first_page();
            }
            "MoveToPreviousPage" => self.display_previous_page(true, false),
            "MoveToNextPage" => self.display_next_page(true, false),
            "MoveToLastPage" => {
                self.last_page();
            }
            // Bookmarks need the per-book bookmark list (later phase).
            "MoveToPrevBookmark" | "MoveToNextBookmark" => {}
            // Reader slots and the minimal-GUI toggle are shell
            // state — forwarded to the window.
            "PrevTab" | "NextTab" | "ToggleUndockReader" | "ToggleMenu" => {
                let cb = self.state.borrow().command_callback.clone();
                if let Some(cb) = cb {
                    cb(id);
                }
            }
            "MoveToPrevPageSingle" => self.display_previous_page(false, false),
            "MoveToNextPageSingle" => self.display_next_page(false, false),
            "MovePrevPart" => self.display_previous_page_or_part(false),
            "MoveNextPart" => self.display_next_page_or_part(false),
            "MoveFirstPart" => {
                self.display_part(PartPageToDisplay::First);
            }
            "MoveLastPart" => {
                self.display_part(PartPageToDisplay::Last);
            }
            "MovePartDown10" => self.move_part_down(0.1),
            "MovePartUp10" => self.move_part_down(-0.1),
            "ToggleAutoScrolling" => {
                let next = !self.state.borrow().auto_scrolling;
                self.state.borrow_mut().auto_scrolling = next;
            }
            "DoublePageAutoScroll" => {
                let mut st = self.state.borrow_mut();
                st.two_page_navigation = !st.two_page_navigation;
                st.invalidate();
            }
            "MoveUp" => {
                let lines = self.state.borrow().scroll_lines;
                self.scroll_up(lines);
            }
            "MoveDown" => {
                let lines = self.state.borrow().scroll_lines;
                self.scroll_down(lines);
            }
            "MoveLeft" => {
                let lines = self.state.borrow().scroll_lines;
                self.scroll_left(lines);
            }
            "MoveRight" => {
                let lines = self.state.borrow().scroll_lines;
                self.scroll_right(lines);
            }
            "ToggleFullScreen" => self.toggle_full_screen(),
            "ToggleTwoPages" => self.toggle_page_layout(),
            "ToggleRealisticPages" => self.toggle_realistic_pages(),
            // The magnifier lens is handled above.
            "ToggleMagnify" => {
                let next = !self.state.borrow().magnifier;
                self.state.borrow_mut().magnifier = next;
                self.area.queue_draw();
            }
            "Original" => self.set_fit_mode(ImageFitMode::Original),
            // `SetPageFitAll`/`SetPageFitHeight` skip in continuous mode.
            "FitAll" => {
                let continuous = self.state.borrow().page_layout == PageLayoutMode::Continuous;
                if !continuous {
                    self.set_fit_mode(ImageFitMode::Fit);
                }
            }
            "FitWidth" => self.set_fit_mode(ImageFitMode::FitWidth),
            "FitWidthAdaptive" => self.set_fit_mode(ImageFitMode::FitWidthAdaptive),
            "FitHeight" => {
                let continuous = self.state.borrow().page_layout == PageLayoutMode::Continuous;
                if !continuous {
                    self.set_fit_mode(ImageFitMode::FitHeight);
                }
            }
            "FitBest" => self.set_fit_mode(ImageFitMode::BestFit),
            "SinglePage" => self.set_page_layout(PageLayoutMode::Single),
            "TwoPages" => self.set_page_layout(PageLayoutMode::Double),
            "TwoPagesAdaptive" => self.set_page_layout(PageLayoutMode::DoubleAdaptive),
            "Continuous" => self.set_page_layout(PageLayoutMode::Continuous),
            "RightToLeft" => {
                let rtl = self.state.borrow().rtl;
                self.set_rtl(!rtl);
            }
            "OnlyFitIfOversized" => self.toggle_fit_only_if_oversized(),
            // The MainForm guards the rotation commands against
            // continuous mode (the strip keeps its own geometry).
            "RotateC" => {
                let continuous = self.state.borrow().page_layout == PageLayoutMode::Continuous;
                if !continuous {
                    self.rotate_right();
                }
            }
            "RotateCC" => {
                let continuous = self.state.borrow().page_layout == PageLayoutMode::Continuous;
                if !continuous {
                    self.rotate_left();
                }
            }
            "AutoRotate" => {
                let continuous = self.state.borrow().page_layout == PageLayoutMode::Continuous;
                if !continuous {
                    self.toggle_auto_rotate();
                }
            }
            "ZoomIn" => self.zoom_add(0.1, MINIMUM_ZOOM, MAXIMUM_ZOOM),
            "ZoomOut" => self.zoom_add(-0.1, MINIMUM_ZOOM, MAXIMUM_ZOOM),
            "StepZoomIn" => self.zoom_add(KEYBOARD_ZOOM_STEPPING, MINIMUM_ZOOM, 4.0),
            "StepZoomOut" => self.zoom_add(-KEYBOARD_ZOOM_STEPPING, MINIMUM_ZOOM, 4.0),
            // Touch-only binding in the C#.
            "ToggleZoom" => {}
            "PageRotateC" => self.page_rotate(true),
            "PageRotateCC" => self.page_rotate(false),
            "Exit" => {
                let cb = self.state.borrow_mut().exit_callback.take();
                if let Some(cb) = cb {
                    cb();
                    self.state.borrow_mut().exit_callback = Some(cb);
                }
            }
            _ => {}
        }
    }

    /// The MainForm `ToggleFullScreen` command
    /// (`ComicDisplay.ToggleFullScreen`).
    pub fn toggle_full_screen(&self) {
        if let Some(window) = self.area.root().and_downcast::<gtk4::Window>() {
            if window.is_fullscreen() {
                window.unfullscreen();
            } else {
                window.fullscreen();
            }
        }
    }

    /// The MainForm `OnlyFitIfOversized` command
    /// (`ImageFitOnlyIfOversized` toggle).
    pub fn toggle_fit_only_if_oversized(&self) {
        let mut st = self.state.borrow_mut();
        st.fit_only_if_oversized = !st.fit_only_if_oversized;
        st.invalidate();
        drop(st);
        self.area.queue_draw();
    }

    /// The MainForm `RightToLeft` command (`RightToLeftReading`
    /// toggle).
    pub fn toggle_rtl(&self) {
        let rtl = self.state.borrow().rtl;
        self.set_rtl(!rtl);
    }

    /// The Exit command closes the reader window (the C#
    /// `ControlExit` closes the main form).
    pub fn set_exit_callback(&self, callback: Box<dyn Fn()>) {
        self.state.borrow_mut().exit_callback = Some(callback);
    }

    /// Forwards shell-level commands (`NextTab`, `PrevTab`,
    /// `ToggleUndockReader`) to the reader window — the C# binds
    /// them to `OpenBooks`/`MainForm` methods.
    pub fn set_command_callback(&self, callback: Rc<dyn Fn(&str)>) {
        self.state.borrow_mut().command_callback = Some(callback);
    }

    fn schedule_click(&self) {
        let view = self.clone();
        let source = glib::timeout_add_local(
            std::time::Duration::from_millis(DOUBLE_CLICK_MS),
            move || {
                view.state.borrow_mut().pending_click = None;
                view.dispatch_key(super::keys::Key::MouseLeft, Default::default());
                glib::ControlFlow::Break
            },
        );
        self.state.borrow_mut().pending_click = Some(source);
    }

    fn cancel_pending_click(&self) {
        if let Some(source) = self.state.borrow_mut().pending_click.take() {
            source.remove();
        }
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
/// The cached error-page surface: the C# `CreateErrorPage` bitmap
/// (bundled `ErrorPage.jpg`) with the message drawn on top
/// (`PageFailedToLoad`, 32 pt black at 40,40).
fn error_page_surface() -> cairo::ImageSurface {
    // Cairo surfaces are neither Send nor Sync — the cache stays on
    // the UI thread.
    thread_local! {
        static SURFACE: RefCell<Option<cairo::ImageSurface>> = const { RefCell::new(None) };
    }
    SURFACE.with(|cell| {
        if let Some(surface) = cell.borrow().as_ref() {
            return surface.clone();
        }
        let surface = build_error_page_surface();
        *cell.borrow_mut() = Some(surface.clone());
        surface
    })
}

fn build_error_page_surface() -> cairo::ImageSurface {
    let image = cr_image::error_assets::error_page_image().expect("bundled error page decodes");
    let surface = image_surface_from_rgba(&image.rgba, image.width, image.height);
    let ctx = cairo::Context::new(&surface).expect("error page context");
    ctx.set_source_rgb(0.0, 0.0, 0.0);
    ctx.select_font_face("sans", cairo::FontSlant::Normal, cairo::FontWeight::Normal);
    ctx.set_font_size(32.0 * 1.4);
    ctx.move_to(40.0, 80.0);
    let _ = ctx.show_text("Page failed to load.");
    ctx.move_to(40.0, 130.0);
    let _ = ctx.show_text("Try refresh to load again...");
    surface
}

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

        // The magnifier lens (`DrawMagnifier`): a clipped second
        // scene pass, zoomed about the cursor, with the glass rim on
        // top.
        if st.magnifier {
            if let Some((mx, my)) = st.magnifier_at {
                let (w, h) = (f64::from(width), f64::from(height));
                if mx >= 0.0 && my >= 0.0 && mx <= w && my <= h && !display.is_empty() {
                    draw_magnifier(ctx, &display, &mut st, width, height, mx, my);
                }
            }
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
                if !st.wanted.is_empty() {
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

/// The magnifier lens (`ComicDisplayControl.DrawMagnifier`): the
/// scene re-rendered through the display matrix with a zoom about
/// the cursor (`premultiply_zoom`), clipped to the lens circle. The
/// glass bitmap overlay of the C# style stays out — cairo draws a
/// simple rim.
fn draw_magnifier(
    ctx: &cairo::Context,
    display: &DisplayOutput,
    st: &mut ViewState,
    width: i32,
    height: i32,
    mx: f64,
    my: f64,
) {
    let radius = f64::from(MAGNIFIER_SIZE) / 2.0;
    let zoomed = {
        let mut d = display.clone();
        d.mat.premultiply_zoom(MAGNIFIER_ZOOM, mx as f32, my as f32);
        d
    };
    // Lens interior: the display background, then the zoomed scene
    // (`RenderImageBackground` runs inside the C# lens too).
    ctx.save().ok();
    ctx.identity_matrix();
    ctx.new_path();
    ctx.arc(mx, my, radius - 2.0, 0.0, std::f64::consts::TAU);
    ctx.clip();
    let background = match st.background_mode {
        ImageBackgroundMode::Auto => st
            .loaded
            .get(&st.page)
            .map(|p| p.auto_background)
            .map(|(r, g, b)| (f64::from(r), f64::from(g), f64::from(b)))
            .unwrap_or(DEFAULT_BACKGROUND),
        _ => DEFAULT_BACKGROUND,
    };
    ctx.set_source_rgb(background.0, background.1, background.2);
    ctx.rectangle(mx - radius, my - radius, radius * 2.0, radius * 2.0);
    let _ = ctx.fill();
    if st.page_layout == PageLayoutMode::Continuous {
        draw_continuous(ctx, &zoomed, st);
    } else if let Some(comp) = st.composition.clone() {
        let paper = st.paper.clone();
        let paper_mode = st.background_mode == ImageBackgroundMode::Texture;
        draw_composition(
            ctx,
            &zoomed,
            &comp,
            &st.loaded,
            paper.as_ref(),
            paper_mode,
            background,
        );
    }
    ctx.restore().ok();
    // The rim.
    ctx.save().ok();
    ctx.identity_matrix();
    ctx.new_path();
    ctx.arc(mx, my, radius, 0.0, std::f64::consts::TAU);
    ctx.set_source_rgba(0.0, 0.0, 0.0, 0.55);
    ctx.set_line_width(3.0);
    let _ = ctx.stroke();
    ctx.restore().ok();
    let _ = (width, height);
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
