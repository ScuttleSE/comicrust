//! The multi-panel status bar (`MainForm.statusStrip`, the T8
//! kickoff scope): the browser selection-info spring panel, the
//! activity lamps (scan / file-write / export — the C# six-lamp
//! family reduced to the ported activities), the data-source light,
//! the book caption, the current page (click toggles
//! `TrackCurrentPage`), the page count, and the thumbnail-size
//! slider (`ToolStripThumbSize` → a GtkScale driving `SetItemSize`).
//! The C# server-activity panel is omitted (no remote server).
//!
//! The scan lamp carries the C# `ScanAnimation.gif` frames (bundled
//! as PNGs — the resx GIFs were not bundled in T2) animated by a
//! timer while visible (WinForms animates status-label GIFs
//! natively — a recorded deviation), and its click opens a small
//! menu with "Cancel scan" (`library::abort_scan` — the C# lamp
//! opens the Tasks dialog; the abort lives in the Tasks scan row
//! there).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk4::gio;
use gtk4::prelude::*;
use gtk4::{gdk, glib, Image, Label, Popover, Scale};

use crate::icon;

/// The idle text of the selection panel (`tsText.Text = "Ready"`).
pub const READY_TEXT: &str = "Ready";
/// `MainForm.None` — the book panel with no open book.
pub const NONE_TEXT: &str = "None";
/// `MainForm.NotAvailable` — the page panel with no open book.
pub const NA_TEXT: &str = "NA";

/// `ComicBrowserControl.SelectionInfo` (ComicBrowserControl.cs:635):
/// "ListName: N Books (M filtered) / size - K selected / size"; the
/// single selection shows the book's file path instead of the count
/// (the C# takes the first selected non-fileless book). The text
/// sizes go through the `FileLengthFormat` port
/// (`display_text::file_size_as_text`).
pub fn selection_info(
    list_name: &str,
    count: usize,
    total: usize,
    total_size: i64,
    selected: usize,
    selected_size: i64,
    selected_path: Option<&str>,
) -> String {
    let mut out = String::new();
    if !list_name.is_empty() {
        out.push_str(list_name);
        out.push_str(": ");
    }
    // `"{0} Book"` / `"{0} Books"` (eComicText/eComicsText).
    if count == 1 {
        out.push_str("1 Book");
    } else {
        out.push_str(&format!("{count} Books"));
    }
    // `(totalCount != count)` → the filtered remainder.
    if total != count {
        out.push_str(&format!(" ({} filtered)", total.saturating_sub(count)));
    }
    if total_size != 0 {
        out.push_str(" / ");
        out.push_str(&cr_engine::display_text::file_size_as_text(total_size));
    }
    if selected != 0 {
        out.push_str(" - ");
        let mut shown = false;
        if selected == 1 {
            if let Some(path) = selected_path {
                out.push_str(path);
                shown = true;
            }
        }
        if !shown {
            out.push_str(&format!("{selected} selected"));
        }
    }
    if selected_size != 0 {
        out.push_str(" / ");
        out.push_str(&cr_engine::display_text::file_size_as_text(selected_size));
    }
    out
}

/// `ComicBook.PagesAsText` (the `TR["Pages", "{0} Page(s)"]` default;
/// `FormatPages`: 0 pages → `TR["Unknown"]`).
pub fn page_count_text(pages: usize) -> String {
    if pages == 0 {
        "Unknown".to_string()
    } else {
        format!("{pages} Page(s)")
    }
}

/// `OnUpdateGui`'s page panel text (`(CurrentPage + 1)` or `NA`).
pub fn page_text(page: Option<usize>) -> String {
    match page {
        Some(p) => (p + 1).to_string(),
        None => NA_TEXT.to_string(),
    }
}

/// The shared handler types (the clippy type-complexity lint).
type LampFn = Box<dyn Fn()>;
type SliderFn = Box<dyn Fn(f64)>;

/// The scan-lamp animation step (the GIF's frame cadence; WinForms
/// plays the resx GIF at the file's own delays — the port steps
/// evenly).
const SCAN_ANIM_MS: u64 = 120;

struct Inner {
    widget: gtk4::Box,
    info: Label,
    lamp_export: gtk4::Button,
    lamp_write: gtk4::Button,
    lamp_scan: gtk4::Button,
    /// The animated scan frames (`ScanAnimation.gif` coalesced);
    /// empty = the static PNG fallback.
    scan_frames: Vec<gdk::Texture>,
    scan_image: Image,
    scan_frame: Cell<usize>,
    anim: RefCell<Option<glib::SourceId>>,
    scan_menu: Popover,
    scan_cancel: gtk4::Button,
    scan_skip: gtk4::Button,
    book: Label,
    page_button: gtk4::Button,
    page_label: Label,
    page_locked: Image,
    page_count: Label,
    slider: Scale,
    /// Blocks the slider handler while `sync_slider` writes the view
    /// value back (the C# `TrackBar.Value` assignment fires no
    /// Scroll; GTK's value-changed fires for programmatic sets too).
    slider_syncing: Cell<bool>,
    on_lamp_click: RefCell<Option<LampFn>>,
    on_cancel_scan: RefCell<Option<LampFn>>,
    on_skip_scan_file: RefCell<Option<LampFn>>,
    on_page_click: RefCell<Option<LampFn>>,
    on_slider_change: RefCell<Option<SliderFn>>,
}

/// The clone-able handle (the closures keep the panels alive).
#[derive(Clone)]
pub struct StatusBar {
    inner: Rc<Inner>,
}

pub struct StatusBarWidgets {
    pub widget: gtk4::Box,
    pub bar: StatusBar,
}

/// One image-only lamp button (`ToolStripStatusLabel` with
/// `DisplayStyle = Image`). The C# animations (resx GIFs) are not
/// bundled — the static command PNGs stand in (the T2 record).
fn lamp_button(icon_name: &str, tooltip: &str) -> gtk4::Button {
    let b = gtk4::Button::new();
    b.set_has_frame(false);
    b.set_child(Some(&icon_image(icon_name)));
    b.set_tooltip_text(Some(tooltip));
    b
}

/// A 16 px PNG from the bundled set (the resx fetcher parity).
fn icon_image(icon_name: &str) -> Image {
    match icon::path_for_name(icon_name) {
        Some(path) => Image::from_file(&path),
        None => Image::new(),
    }
}

/// The coalesced `ScanAnimation.gif` frames (the C# resx animation,
/// extracted to `assets/scan/frame-N.png`; the resx GIFs were not
/// bundled in T2). Stops at the first missing frame.
fn scan_frames() -> Vec<gdk::Texture> {
    let mut frames = Vec::new();
    for i in 0..u32::MAX {
        let Some(path) = crate::assets::find(&format!("scan/frame-{i}.png")) else {
            break;
        };
        match gdk::Texture::from_file(&gio::File::for_path(&path)) {
            Ok(texture) => frames.push(texture),
            Err(_) => break,
        }
        if i > 63 {
            break; // a run-loop guard, not a real bound
        }
    }
    frames
}

/// One sunken panel (`Border3DStyle.SunkenOuter` → the
/// `.status-panel` CSS class).
fn panel_label(text: &str) -> Label {
    let l = Label::builder()
        .label(text)
        .valign(gtk4::Align::Center)
        .build();
    l.add_css_class("status-panel");
    l
}

fn panel_image(icon_name: &str) -> Image {
    let img = icon_image(icon_name);
    img.add_css_class("status-panel");
    img
}

impl StatusBar {
    pub fn create() -> StatusBarWidgets {
        let widget = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        widget.add_css_class("status-bar");
        widget.set_hexpand(true);

        // 1. The selection-info spring panel (fills, left-aligned,
        //    default "Ready").
        let info = Label::builder()
            .label(READY_TEXT)
            .halign(gtk4::Align::Start)
            .valign(gtk4::Align::Center)
            .ellipsize(gtk4::pango::EllipsizeMode::End)
            .margin_start(6)
            .build();
        info.add_css_class("status-panel");
        info.set_hexpand(true);
        widget.append(&info);

        // 2. The activity lamps (image-only, hidden unless active).
        //    The C# order in the strip: backup, device-sync, export,
        //    read-info, write-info, page, scan — the port shows the
        //    ported activities: export, write, scan. The scan lamp
        //    animates the bundled ScanAnimation frames and opens the
        //    Cancel-scan menu on click (the other lamps open Tasks).
        let lamp_export = lamp_button("Export.png", "Exporting comics...");
        let lamp_write = lamp_button("UpdateBig.png", "Writing info data to files...");
        let scan_frames = scan_frames();
        let scan_image = match scan_frames.first() {
            Some(texture) => Image::from_paintable(Some(texture)),
            None => icon_image("Scan.png"),
        };
        let lamp_scan = gtk4::Button::new();
        lamp_scan.set_has_frame(false);
        lamp_scan.set_child(Some(&scan_image));
        lamp_scan.set_tooltip_text(Some("A scan is running..."));
        for lamp in [&lamp_export, &lamp_write, &lamp_scan] {
            lamp.add_css_class("status-panel");
            widget.append(lamp);
        }

        // The scan lamp's menu: "Skip current file" (abandon the file
        // in flight, keep scanning) and "Cancel scan" (the C# abort
        // lives in the Tasks dialog's scan row; the user asked for
        // the direct menu on the lamp). Parented to the LAMP (the
        // popover-before-toplevel lesson) and positioned above the
        // bar.
        let scan_menu = Popover::new();
        scan_menu.set_position(gtk4::PositionType::Top);
        scan_menu.set_autohide(true);
        let menu_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        menu_box.set_margin_top(4);
        menu_box.set_margin_bottom(4);
        menu_box.set_margin_start(2);
        menu_box.set_margin_end(2);
        let skip = crate::widgets::menu_item_button("Skip current file");
        skip.set_tooltip_text(Some(
            "Abandon the file the scan is reading now and continue with the next one",
        ));
        menu_box.append(&skip);
        let cancel = crate::widgets::menu_item_button("Cancel scan");
        menu_box.append(&cancel);
        scan_menu.set_child(Some(&menu_box));
        scan_menu.set_parent(&lamp_scan);

        // 3. The data-source light (the local XML database is always
        //    connected — the C# `DataSourceConnected.png` state).
        let data_source = panel_image("DataSourceConnected.png");
        data_source.set_margin_start(4);
        data_source.set_margin_end(4);
        widget.append(&data_source);

        // 4. The open-book caption ("None" otherwise).
        let book = panel_label(NONE_TEXT);
        book.set_ellipsize(gtk4::pango::EllipsizeMode::End);
        book.set_max_width_chars(60);
        book.set_tooltip_text(Some("Name of the opened Book"));
        widget.append(&book);

        // 5. The current page (click toggles TrackCurrentPage); the
        //    Locked image shows while tracking is OFF (`tsCurrentPage
        //    .Image = TrackCurrentPage ? null : trackPagesLockedImage`).
        let page_button = gtk4::Button::new();
        page_button.set_has_frame(false);
        page_button.add_css_class("status-panel");
        page_button.set_tooltip_text(Some("Current Page of the open Book"));
        let page_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 2);
        let page_locked = icon_image("Locked.png");
        page_locked.set_visible(false);
        let page_label = Label::builder()
            .label(NA_TEXT)
            .valign(gtk4::Align::Center)
            .build();
        page_box.append(&page_locked);
        page_box.append(&page_label);
        page_button.set_child(Some(&page_box));
        widget.append(&page_button);

        // 6. The page count.
        let page_count = panel_label("0 Page(s)");
        page_count.set_tooltip_text(Some("Page count of the open Book"));
        widget.append(&page_count);

        // 7. The thumbnail-size slider (the C# 120x16 trackbar;
        //    visible while the browser shows, drives `SetItemSize`).
        let slider = Scale::with_range(gtk4::Orientation::Horizontal, 96.0, 512.0, 1.0);
        slider.set_draw_value(false);
        slider.set_width_request(120);
        slider.add_css_class("status-panel");
        slider.set_margin_start(4);
        slider.set_margin_end(4);
        slider.set_valign(gtk4::Align::Center);
        widget.append(&slider);

        let bar = StatusBar {
            inner: Rc::new(Inner {
                widget: widget.clone(),
                info,
                lamp_export,
                lamp_write,
                lamp_scan,
                scan_frames,
                scan_image,
                scan_frame: Cell::new(0),
                anim: RefCell::new(None),
                scan_menu,
                scan_cancel: cancel,
                scan_skip: skip,
                book,
                page_button,
                page_label,
                page_locked,
                page_count,
                slider,
                slider_syncing: Cell::new(false),
                on_lamp_click: RefCell::new(None),
                on_cancel_scan: RefCell::new(None),
                on_skip_scan_file: RefCell::new(None),
                on_page_click: RefCell::new(None),
                on_slider_change: RefCell::new(None),
            }),
        };
        bar.wire();
        StatusBarWidgets { widget, bar }
    }

    fn wire(&self) {
        // The export/write lamps open the Tasks dialog (the C#
        // `ShowPendingTasks` handlers; the export lamp shows the
        // errors dialog first — T13). The SCAN lamp opens the
        // Cancel-scan menu instead.
        for lamp in [&self.inner.lamp_export, &self.inner.lamp_write] {
            let inner = Rc::clone(&self.inner);
            lamp.connect_clicked(move |_| {
                if let Some(f) = inner.on_lamp_click.borrow().as_ref() {
                    f();
                }
            });
        }
        {
            let inner = Rc::clone(&self.inner);
            self.inner.lamp_scan.connect_clicked(move |_| {
                if !inner.scan_menu.is_visible() {
                    inner.scan_menu.popup();
                }
            });
        }
        {
            let inner = Rc::clone(&self.inner);
            self.inner.scan_cancel.connect_clicked(move |_| {
                inner.scan_menu.popdown();
                if let Some(f) = inner.on_cancel_scan.borrow().as_ref() {
                    f();
                }
            });
        }
        {
            let inner = Rc::clone(&self.inner);
            self.inner.scan_skip.connect_clicked(move |_| {
                inner.scan_menu.popdown();
                if let Some(f) = inner.on_skip_scan_file.borrow().as_ref() {
                    f();
                }
            });
        }
        let inner = Rc::clone(&self.inner);
        self.inner.page_button.connect_clicked(move |_| {
            if let Some(f) = inner.on_page_click.borrow().as_ref() {
                f();
            }
        });
        let inner = Rc::clone(&self.inner);
        self.inner.slider.connect_value_changed(move |scale| {
            if inner.slider_syncing.get() {
                return;
            }
            if let Some(f) = inner.on_slider_change.borrow().as_ref() {
                f(scale.value());
            }
        });
    }

    pub fn widget(&self) -> &gtk4::Box {
        &self.inner.widget
    }

    pub fn set_selection_info(&self, text: &str) {
        self.inner.info.set_text(text);
    }

    /// The book caption; empty/absent shows the "None" default.
    pub fn set_book(&self, caption: Option<&str>) {
        match caption {
            Some(c) if !c.is_empty() => self.inner.book.set_text(c),
            _ => self.inner.book.set_text(NONE_TEXT),
        }
    }

    /// The page number (1-based display) + the TrackCurrentPage
    /// state (the locked image shows while tracking is off).
    pub fn set_page(&self, page: Option<usize>, track: bool) {
        self.inner.page_label.set_text(&page_text(page));
        self.inner.page_locked.set_visible(!track);
    }

    pub fn set_page_count(&self, text: &str) {
        self.inner.page_count.set_text(text);
    }

    /// `thumbSize.SetSlider(min, max, value)` + the visibility rule
    /// (`mainViewContainer.Expanded && itemSizeInfo != null` — the
    /// port shows it on the browser workspace only).
    pub fn sync_slider(&self, size: Option<(f64, f64, f64)>, visible: bool) {
        self.inner.slider.set_visible(visible && size.is_some());
        let Some((min, max, value)) = size else {
            return;
        };
        // The guard covers the RANGE set too: a range that clamps
        // the current value (per-mode ranges differ) emits
        // value_changed, and the handler re-enters the ItemView.
        self.inner.slider_syncing.set(true);
        self.inner.slider.set_range(min, max);
        self.inner.slider.set_value(value);
        self.inner.slider_syncing.set(false);
    }

    /// `UpdateActivityTimerTick`'s lamp visibility. The scan lamp
    /// also starts/stops its frame animation with the visibility.
    pub fn update_lamps(&self, scan: bool, write: bool, export: bool) {
        self.inner.lamp_scan.set_visible(scan);
        self.inner.lamp_write.set_visible(write);
        self.inner.lamp_export.set_visible(export);
        self.sync_scan_anim(scan);
    }

    /// The scan-lamp frame timer: runs ONLY while the lamp shows
    /// (the C# animates the resx GIF natively whenever visible).
    fn sync_scan_anim(&self, scan: bool) {
        if scan && !self.inner.scan_frames.is_empty() {
            let anim = self.inner.anim.borrow().is_some();
            if anim {
                return;
            }
            let inner = Rc::downgrade(&self.inner);
            let source = glib::timeout_add_local(
                std::time::Duration::from_millis(SCAN_ANIM_MS),
                move || {
                    let Some(inner) = inner.upgrade() else {
                        return glib::ControlFlow::Break;
                    };
                    if !inner.lamp_scan.is_visible() || inner.scan_frames.is_empty() {
                        *inner.anim.borrow_mut() = None;
                        return glib::ControlFlow::Break;
                    }
                    let count = inner.scan_frames.len();
                    let next = (inner.scan_frame.get() + 1) % count;
                    inner.scan_frame.set(next);
                    inner
                        .scan_image
                        .set_paintable(Some(&inner.scan_frames[next]));
                    glib::ControlFlow::Continue
                },
            );
            *self.inner.anim.borrow_mut() = Some(source);
            return;
        }
        if let Some(source) = self.inner.anim.borrow_mut().take() {
            source.remove();
        }
    }

    pub fn connect_lamp_click<F: Fn() + 'static>(&self, f: F) {
        *self.inner.on_lamp_click.borrow_mut() = Some(Box::new(f));
    }

    /// The scan lamp's "Cancel scan" row (`library::abort_scan`
    /// through the shell hook).
    pub fn connect_cancel_scan<F: Fn() + 'static>(&self, f: F) {
        *self.inner.on_cancel_scan.borrow_mut() = Some(Box::new(f));
    }

    /// The scan lamp's "Skip current file" row
    /// (`library::skip_current_scan_file` through the shell hook).
    pub fn connect_skip_scan_file<F: Fn() + 'static>(&self, f: F) {
        *self.inner.on_skip_scan_file.borrow_mut() = Some(Box::new(f));
    }

    pub fn connect_page_click<F: Fn() + 'static>(&self, f: F) {
        *self.inner.on_page_click.borrow_mut() = Some(Box::new(f));
    }

    pub fn connect_slider<F: Fn(f64) + 'static>(&self, f: F) {
        *self.inner.on_slider_change.borrow_mut() = Some(Box::new(f));
    }

    // ---- probe accessors ----

    pub fn info_text(&self) -> String {
        self.inner.info.text().into()
    }

    pub fn book_text(&self) -> String {
        self.inner.book.text().into()
    }

    pub fn page_label_text(&self) -> String {
        self.inner.page_label.text().into()
    }

    pub fn page_count_text(&self) -> String {
        self.inner.page_count.text().into()
    }

    pub fn slider_value(&self) -> f64 {
        self.inner.slider.value()
    }

    pub fn slider_visible(&self) -> bool {
        self.inner.slider.is_visible()
    }

    pub fn lamp_visible(&self, which: &str) -> bool {
        match which {
            "scan" => self.inner.lamp_scan.is_visible(),
            "write" => self.inner.lamp_write.is_visible(),
            "export" => self.inner.lamp_export.is_visible(),
            _ => false,
        }
    }

    /// The scan animation is running (a frame timer is live).
    pub fn scan_anim_running(&self) -> bool {
        self.inner.anim.borrow().is_some()
    }

    /// The scan-lamp frame count (the bundled animation).
    pub fn scan_frame_count(&self) -> usize {
        self.inner.scan_frames.len()
    }

    /// The probe path: the REAL scan-lamp click (the menu opens).
    pub fn click_scan_lamp(&self) {
        self.inner.lamp_scan.emit_clicked();
    }

    /// The probe path: the REAL "Cancel scan" row click.
    pub fn click_cancel_scan(&self) {
        self.inner.scan_cancel.emit_clicked();
    }

    /// The probe path: the REAL "Skip current file" row click.
    pub fn click_skip_scan_file(&self) {
        self.inner.scan_skip.emit_clicked();
    }

    pub fn cancel_menu_visible(&self) -> bool {
        self.inner.scan_menu.is_visible()
    }

    pub fn locked_visible(&self) -> bool {
        self.inner.page_locked.is_visible()
    }

    /// The probe path: the REAL page-panel click (`emit_clicked` →
    /// handler → the `win.track-current-page` activation).
    pub fn click_page(&self) {
        self.inner.page_button.emit_clicked();
    }

    /// The probe path: a user drag (the guard stays off, so the
    /// value-changed handler fires — the real drag path).
    pub fn drag_slider(&self, value: f64) {
        self.inner.slider.set_value(value);
    }
}

/// The 1 s activity poll (`updateActivityTimer`, Interval = 1000).
pub fn start_activity_timer<F: Fn() + 'static>(tick: F) -> glib::SourceId {
    glib::timeout_add_local(std::time::Duration::from_secs(1), move || {
        tick();
        glib::ControlFlow::Continue
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_info_matches_the_csharp_shape() {
        // Name + count.
        assert_eq!(
            selection_info("Library", 5, 5, 0, 0, 0, None),
            "Library: 5 Books"
        );
        // Singular.
        assert_eq!(selection_info("", 1, 1, 0, 0, 0, None), "1 Book");
        // Filtered remainder.
        assert_eq!(
            selection_info("Never Read", 3, 10, 0, 0, 0, None),
            "Never Read: 3 Books (7 filtered)"
        );
        // Total size + multi selection.
        assert_eq!(
            selection_info("L", 2, 2, 2048, 2, 1024, None),
            "L: 2 Books / 2.00 kB - 2 selected / 1.00 kB"
        );
        // Single selection shows the path, then the size.
        assert_eq!(
            selection_info("", 1, 1, 0, 1, 100, Some("/books/a.cbz")),
            "1 Book - /books/a.cbz / 100 Bytes"
        );
    }

    #[test]
    fn page_count_and_page_text() {
        assert_eq!(page_count_text(24), "24 Page(s)");
        assert_eq!(page_count_text(0), "Unknown");
        assert_eq!(page_text(Some(0)), "1");
        assert_eq!(page_text(Some(23)), "24");
        assert_eq!(page_text(None), "NA");
    }
}
