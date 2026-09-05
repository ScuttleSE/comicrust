//! The reader toolbar (`MainForm.mainToolStrip`, Phase 5.5 T5): the
//! strip of split buttons that rides the reader tab row (Dock=Right
//! in the C#; the port mounts it as a right-aligned row above the
//! reader). Buttons in order (Designer:2591-2614): prev/next (page
//! turn + drop), page layout, fit, zoom (text = current %), rotate
//! (text = current angle), magnifier, full screen, tools.
//!
//! C# click parity: prev/next main parts turn the page
//! (`CommandMapper` binds them), magnifier/full screen toggle,
//! layout/fit/zoom/rotate have NO main click (dropdown only), tools
//! is a drop. The drop tables mirror the Designer
//! (tbPrevPage/tbNextPage carry the dynamic bookmark fills with
//! direction -1/+1 via `UpdateBookmarkMenu`).
//!
//! Visibility: the reader-only buttons hide without an open book
//! (`OnUpdateGui` flags); the whole bar hides with MinimalGui. The
//! undocked reader carries the same strip (the shell re-parents it
//! — ReaderForm parity).

use gtk4::prelude::*;
use gtk4::Button;

use crate::reader::display::ImageFitMode;
use crate::reader::page_view::PageLayoutMode;

use super::menubar::{self, Dropdown, MenuNode};
use MenuNode::{Dyn, Item, Sep, Sub};

/// The prev-page drop (`tbPrevPage.DropDownItems`): First Page,
/// Previous Bookmark, the dynamic bookmarks BEFORE the current page
/// (`UpdateBookmarkMenu(-1)`), Previous Book from List.
pub const PREV: &[MenuNode] = &[
    Item("_First Page", "win.first-page", "", "GoFirst"),
    Item(
        "Previous Bookmark",
        "win.prev-bookmark",
        "",
        "PreviousBookmark",
    ),
    Sep,
    Dyn("bookmarks-prev"),
    Sep,
    Item(
        "Previous Book from List",
        "win.prev-book",
        "",
        "PrevFromList",
    ),
];

/// The next-page drop (`tbNextPage.DropDownItems`): Last Page, Next
/// Bookmark, Last Page Read, the bookmarks AFTER the current page,
/// Next/Random Book from List.
pub const NEXT: &[MenuNode] = &[
    Item("_Last Page", "win.last-page", "", "GoLast"),
    Item("Next Bookmark", "win.next-bookmark", "", "NextBookmark"),
    Item("L_ast Page Read", "win.last-page-read", "", ""),
    Sep,
    Dyn("bookmarks-next"),
    Sep,
    Item("Next Book from List", "win.next-book", "", "NextFromList"),
    Item("Random Book", "win.random-book", "", "RandomComic"),
];

/// The fit drop (`tbFit.DropDownItems`): the six fit radios + Only
/// fit if oversized.
pub const FIT: &[MenuNode] = &[
    Item("Original Size", "win.page-fit::original", "", "Original"),
    Item("Fit _All", "win.page-fit::fit-all", "", "FitAll"),
    Item("Fit _Width", "win.page-fit::fit-width", "", "FitWidth"),
    Item(
        "Fit Width (adaptive)",
        "win.page-fit::fit-width-adaptive",
        "",
        "FitWidthAdaptive",
    ),
    Item("Fit _Height", "win.page-fit::fit-height", "", "FitHeight"),
    Item("Fit _Best", "win.page-fit::fit-best", "", "FitBest"),
    Sep,
    Item(
        "_Only fit if oversized",
        "win.only-fit-oversized",
        "",
        "Oversized",
    ),
];

/// The zoom drop (`tbZoom.DropDownItems`).
pub const ZOOM: &[MenuNode] = &[
    Item("Zoom _In", "win.zoom-in", "", "ZoomIn"),
    Item("Zoom _Out", "win.zoom-out", "", "ZoomOut"),
    Sep,
    Item("100%", "win.zoom-preset::100", "", ""),
    Item("125%", "win.zoom-preset::125", "", ""),
    Item("150%", "win.zoom-preset::150", "", ""),
    Item("200%", "win.zoom-preset::200", "", ""),
    Item("400%", "win.zoom-preset::400", "", ""),
    Sep,
    Item("_Custom...", "win.zoom-custom", "", ""),
];

/// The rotate drop (`tbRotate.DropDownItems`).
pub const ROTATE: &[MenuNode] = &[
    Item("Rotate Left", "win.rotate-left", "", "RotateLeft"),
    Item("Rotate Right", "win.rotate-right", "", "RotateRight"),
    Sep,
    Item("_No Rotation", "win.rotate-0", "", "Rotate0"),
    Item("90°", "win.rotate-90", "", "Rotate90"),
    Item("180°", "win.rotate-180", "", "Rotate180"),
    Item("270°", "win.rotate-270", "", "Rotate270"),
    Sep,
    Item(
        "Autorotate Double Pages",
        "win.auto-rotate",
        "",
        "AutoRotate",
    ),
];

/// The Tools drop (`toolsContextMenu.Items`): the flattened menu.
/// Absent per ADR-024: Open Remote Library, Workspaces, Update Web
/// Comics, Synchronize Devices.
pub const TOOLS: &[MenuNode] = &[
    Item("_Open File...", "win.open-file", "", "Open"),
    Item("Info...", "win.info", "", "GetInfo"),
    Sep,
    Sub(
        "_Bookmarks",
        &[
            Item("Set Bookmark...", "win.set-bookmark", "", "NewBookmark"),
            Item(
                "Remove Bookmark",
                "win.remove-bookmark",
                "",
                "RemoveBookmark",
            ),
            Sep,
            Dyn("bookmarks"),
        ],
    ),
    Item("_Auto Scrolling", "win.auto-scroll", "", "CursorScroll"),
    Sep,
    Item(
        "Minimal User Interface",
        "win.minimal-gui",
        "",
        "MenuToggle",
    ),
    Item(
        "Reader in _own Window",
        "win.undock-reader",
        "",
        "UndockReader",
    ),
    Sep,
    Item("Scan Book _Folders", "win.scan-folders", "", "Scan"),
    Item(
        "Update all Book Files",
        "win.update-book-files",
        "",
        "UpdateSmall",
    ),
    Item(
        "Generate Cover Thumbnails",
        "win.generate-thumbnails",
        "",
        "Screenshot",
    ),
    Sep,
    Item(
        "Book Display Settings...",
        "win.display-settings",
        "",
        "DisplaySettings",
    ),
    Item("_Preferences...", "win.preferences", "", "Preferences"),
    Item("_About...", "win.about", "", "About"),
    Sep,
    // `tbShowMainMenu`: checked while the menu is NOT auto-hidden
    // (the C# command flips `AutoHideMainMenu`).
    Item("Show Main Menu", "win.show-main-menu", "", ""),
    Sep,
    Item("_Exit", "win.quit", "", ""),
];

/// The toolbar widget: the button row + the dropdowns (kept for the
/// state sync).
pub struct ReaderToolbar {
    bar: gtk4::Box,
    /// The reader-only widgets (hidden without an open book).
    reader_only: Vec<gtk4::Widget>,
    zoom_label: gtk4::Label,
    rotate_label: gtk4::Label,
    fit_icon: gtk4::Image,
    layout_icon: gtk4::Image,
    magnify_icon: gtk4::Image,
    rotate_btn_icon: gtk4::Image,
    /// (name, dropdown, anchor button) — the anchor is what the
    /// popover points at and what parents it on first open.
    dropdowns: Vec<(&'static str, Dropdown, gtk4::Button)>,
}

impl Clone for ReaderToolbar {
    fn clone(&self) -> Self {
        Self {
            bar: self.bar.clone(),
            reader_only: self.reader_only.to_vec(),
            zoom_label: self.zoom_label.clone(),
            rotate_label: self.rotate_label.clone(),
            fit_icon: self.fit_icon.clone(),
            layout_icon: self.layout_icon.clone(),
            magnify_icon: self.magnify_icon.clone(),
            rotate_btn_icon: self.rotate_btn_icon.clone(),
            dropdowns: self.dropdowns.clone(),
        }
    }
}

/// A 16 px flat icon button; `action` is the detailed `win.` name
/// ("" = no click action — the C# split buttons without one).
fn icon_button(
    icon: &'static str,
    action: &str,
    window: &gtk4::ApplicationWindow,
    tooltip: &str,
) -> (Button, gtk4::Image) {
    let button = Button::new();
    let image = gtk4::Image::new();
    image.set_pixel_size(16);
    if let Some(texture) = crate::icon::icon(icon) {
        image.set_paintable(Some(&texture));
    }
    button.set_child(Some(&image));
    button.set_tooltip_text(Some(tooltip));
    button.add_css_class("flat");
    if !action.is_empty() {
        let window = window.clone();
        let detailed = action.to_string();
        button.connect_clicked(move |_| {
            let _ = gtk4::prelude::WidgetExt::activate_action(&window, &detailed, None);
        });
    }
    (button, image)
}

/// A split button: [icon part][chevron part] — the C#
/// ToolStripSplitButton (prev/next page turn + menu). Returns the
/// container and the icon Image of the MAIN part.
fn split_button(
    window: &gtk4::ApplicationWindow,
    icon: &'static str,
    click_action: &str,
    tooltip: &str,
    drop: Dropdown,
) -> (gtk4::Box, gtk4::Image, gtk4::Button) {
    let box_ = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    box_.add_css_class("linked");
    let (main, image) = icon_button(icon, click_action, window, tooltip);
    let chevron = Button::from_icon_name("pan-end-symbolic");
    chevron.set_tooltip_text(Some(tooltip));
    chevron.add_css_class("flat");
    {
        let drop = drop.clone();
        let main_ref = main.clone();
        chevron.connect_clicked(move |_| drop.open(&main_ref));
    }
    box_.append(&main);
    box_.append(&chevron);
    (box_, image, main)
}

/// A plain dropdown button: the click opens the drop (the C#
/// split buttons whose main click is unbound — page layout, fit,
/// zoom, rotate, tools). `child` is the button content (icon, or
/// icon + state text).
fn drop_button(child: &impl IsA<gtk4::Widget>, tooltip: &str, drop: Dropdown) -> Button {
    let button = Button::new();
    button.set_child(Some(child));
    button.set_tooltip_text(Some(tooltip));
    button.add_css_class("flat");
    {
        let drop = drop.clone();
        let btn = button.clone();
        button.connect_clicked(move |_| drop.open(&btn));
    }
    button
}

impl ReaderToolbar {
    /// Builds the strip (call once per browser window).
    pub fn create(window: &gtk4::ApplicationWindow) -> ReaderToolbar {
        let bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 2);
        bar.add_css_class("toolbar");
        bar.set_halign(gtk4::Align::End);
        bar.set_valign(gtk4::Align::Center);
        bar.set_margin_top(2);
        bar.set_margin_bottom(2);
        bar.set_margin_end(6);
        let mut reader_only: Vec<gtk4::Widget> = Vec::new();
        let mut dropdowns: Vec<(&'static str, Dropdown, gtk4::Button)> = Vec::new();
        let mk = |defs: &'static [MenuNode]| menubar::build_dropdown(defs, window);

        // tbPrevPage: main = Previous Page; drop = the bookmarks
        // before the current page + Previous Book from List.
        let prev_drop = mk(PREV);
        let (prev_box, _prev_img, prev_main_btn) = split_button(
            window,
            "GoPrevious",
            "win.prev-page",
            "Previous Page",
            prev_drop.clone(),
        );
        dropdowns.push(("prev", prev_drop, prev_main_btn.clone()));
        bar.append(&prev_box);
        reader_only.push(prev_box.clone().upcast());

        // tbNextPage.
        let next_drop = mk(NEXT);
        let (next_box, _next_img, next_main_btn) = split_button(
            window,
            "GoNext",
            "win.next-page",
            "Next Page",
            next_drop.clone(),
        );
        dropdowns.push(("next", next_drop, next_main_btn.clone()));
        bar.append(&next_box);
        reader_only.push(next_box.clone().upcast());

        let sep1 = gtk4::Separator::new(gtk4::Orientation::Horizontal);
        sep1.set_margin_top(4);
        sep1.set_margin_bottom(4);
        bar.append(&sep1);
        reader_only.push(sep1.clone().upcast());

        // tbPageLayout: drop only; the icon tracks the layout
        // (+ the RTL variants).
        let layout_drop = mk(super::menubar::PAGE_LAYOUT);
        let layout_icon = gtk4::Image::new();
        layout_icon.set_pixel_size(16);
        if let Some(texture) = crate::icon::icon("SinglePage") {
            layout_icon.set_paintable(Some(&texture));
        }
        let layout_child = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        layout_child.append(&layout_icon);
        let layout_btn = drop_button(&layout_child, "Page Layout", layout_drop.clone());
        layout_btn.set_child(Some(&layout_child));
        dropdowns.push(("layout", layout_drop, layout_btn.clone()));
        bar.append(&layout_btn);
        reader_only.push(layout_btn.clone().upcast());

        // tbFit: drop only; the icon tracks the fit mode.
        let fit_drop = mk(FIT);
        let fit_icon = gtk4::Image::new();
        fit_icon.set_pixel_size(16);
        if let Some(texture) = crate::icon::icon("FitAll") {
            fit_icon.set_paintable(Some(&texture));
        }
        let fit_btn = drop_button(&fit_icon, "Toggle Fit Mode", fit_drop.clone());
        dropdowns.push(("fit", fit_drop, fit_btn.clone()));
        bar.append(&fit_btn);
        reader_only.push(fit_btn.clone().upcast());

        // tbZoom: icon + "100 %" text.
        let zoom_drop = mk(ZOOM);
        let zoom_child = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        let zoom_icon = gtk4::Image::new();
        zoom_icon.set_pixel_size(16);
        if let Some(texture) = crate::icon::icon("ZoomIn") {
            zoom_icon.set_paintable(Some(&texture));
        }
        let zoom_label = gtk4::Label::new(Some("100%"));
        zoom_child.append(&zoom_icon);
        zoom_child.append(&zoom_label);
        let zoom_btn = drop_button(&zoom_child, "Change the page zoom", zoom_drop.clone());
        dropdowns.push(("zoom", zoom_drop, zoom_btn.clone()));
        bar.append(&zoom_btn);
        reader_only.push(zoom_btn.clone().upcast());

        // tbRotate: icon + angle text.
        let rotate_drop = mk(ROTATE);
        let rotate_child = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
        let rotate_btn_icon = gtk4::Image::new();
        rotate_btn_icon.set_pixel_size(16);
        if let Some(texture) = crate::icon::icon("RotateRight") {
            rotate_btn_icon.set_paintable(Some(&texture));
        }
        let rotate_label = gtk4::Label::new(Some("0°"));
        rotate_child.append(&rotate_btn_icon);
        rotate_child.append(&rotate_label);
        let rotate_btn = drop_button(
            &rotate_child,
            "Change the page rotation",
            rotate_drop.clone(),
        );
        dropdowns.push(("rotate", rotate_drop, rotate_btn.clone()));
        bar.append(&rotate_btn);
        reader_only.push(rotate_btn.clone().upcast());

        let sep2 = gtk4::Separator::new(gtk4::Orientation::Horizontal);
        sep2.set_margin_top(4);
        sep2.set_margin_bottom(4);
        bar.append(&sep2);
        reader_only.push(sep2.clone().upcast());

        // tbMagnify: main = toggle; the icon tracks the state.
        let (magnify_btn, magnify_icon) =
            icon_button("ZoomClear", "win.magnifier", window, "Magnifier");
        bar.append(&magnify_btn);
        reader_only.push(magnify_btn.clone().upcast());

        // tbFullScreen: main = toggle.
        let (fs_btn, _fs_icon) =
            icon_button("FullScreen", "win.full-screen", window, "Full Screen");
        bar.append(&fs_btn);

        let sep3 = gtk4::Separator::new(gtk4::Orientation::Horizontal);
        sep3.set_margin_top(4);
        sep3.set_margin_bottom(4);
        bar.append(&sep3);

        // tbTools: drop only (the flattened menu).
        let tools_drop = mk(TOOLS);
        let tools_icon = gtk4::Image::new();
        tools_icon.set_pixel_size(16);
        if let Some(texture) = crate::icon::icon("Tools") {
            tools_icon.set_paintable(Some(&texture));
        }
        let tools_btn = drop_button(&tools_icon, "Tools", tools_drop.clone());
        dropdowns.push(("tools", tools_drop, tools_btn.clone()));
        bar.append(&tools_btn);

        ReaderToolbar {
            bar,
            reader_only,
            zoom_label,
            rotate_label,
            fit_icon,
            layout_icon,
            magnify_icon,
            rotate_btn_icon,
            dropdowns,
        }
    }

    pub fn widget(&self) -> &gtk4::Box {
        &self.bar
    }

    /// Installs the shared dynamic fill provider (the bookmark
    /// fills of the prev/next/tools drops).
    pub fn set_dyn_fill(&self, fill: menubar::DynFillFn) {
        for (_, drop, _) in &self.dropdowns {
            drop.set_dyn_fill(fill.clone());
        }
    }

    /// The reader-only visibility (the `OnUpdateGui` reader-only
    /// flags) + the whole-bar flag (MinimalGui).
    pub fn sync_visibility(&self, has_book: bool, bar_visible: bool) {
        for w in &self.reader_only {
            w.set_visible(has_book);
        }
        self.bar.set_visible(bar_visible);
    }

    /// The state text/icons (`viewer_PageDisplayModeChanged` +
    /// `OnUpdateGui`): zoom %, rotation °, the fit/layout images,
    /// the magnifier and auto-rotate icons.
    #[allow(clippy::too_many_arguments)]
    pub fn sync_state(
        &self,
        zoom: Option<f32>,
        rotation: Option<cr_core::model::enums::ImageRotation>,
        fit: Option<ImageFitMode>,
        layout: Option<PageLayoutMode>,
        rtl: Option<bool>,
        magnifier: Option<bool>,
        auto_rotate: Option<bool>,
    ) {
        use cr_core::model::enums::ImageRotation;
        if let Some(z) = zoom {
            self.zoom_label
                .set_text(&format!("{}%", (z * 100.0) as i32));
        }
        if let Some(r) = rotation {
            let deg = match r {
                cr_core::model::enums::ImageRotation::None => 0,
                ImageRotation::Rotate90 => 90,
                ImageRotation::Rotate180 => 180,
                ImageRotation::Rotate270 => 270,
            };
            self.rotate_label.set_text(&format!("{deg}°"));
        }
        if let Some(f) = fit {
            if let Some(texture) = crate::icon::icon(fit_icon_name(f)) {
                self.fit_icon.set_paintable(Some(&texture));
            }
        }
        if let (Some(l), Some(rtl)) = (layout, rtl) {
            if let Some(texture) = crate::icon::icon(layout_icon_name(l, rtl)) {
                self.layout_icon.set_paintable(Some(&texture));
            }
        }
        if let Some(m) = magnifier {
            let name = if m { "Zoom" } else { "ZoomClear" };
            if let Some(texture) = crate::icon::icon(name) {
                self.magnify_icon.set_paintable(Some(&texture));
            }
        }
        if let Some(a) = auto_rotate {
            let name = if a { "AutoRotate" } else { "RotateRight" };
            if let Some(texture) = crate::icon::icon(name) {
                self.rotate_btn_icon.set_paintable(Some(&texture));
            }
        }
    }

    /// Applies the action states to every dropdown (the shell
    /// resolves the same states the menubar gets).
    pub fn sync(&self, resolve: &dyn Fn(&str) -> Option<menubar::ActionState>) {
        for (_, drop, _) in &self.dropdowns {
            drop.sync(resolve);
        }
    }

    /// The dropdowns by name (the probe).
    pub fn dropdown(&self, name: &str) -> Option<Dropdown> {
        self.dropdowns
            .iter()
            .find(|(n, _, _)| *n == name)
            .map(|(_, d, _)| d.clone())
    }

    /// The zoom state text (the probe).
    pub fn zoom_text(&self) -> String {
        self.zoom_label.text().to_string()
    }

    /// The rotation state text (the probe).
    pub fn rotate_text(&self) -> String {
        self.rotate_label.text().to_string()
    }

    /// Opens one dropdown through its stored anchor (the probe's
    /// real open path — the same call the chevron handler makes).
    pub fn open_dropdown(&self, name: &str) -> bool {
        let hit = self
            .dropdowns
            .iter()
            .find(|(n, _, _)| *n == name)
            .map(|(_, d, b)| (d.clone(), b.clone()));
        match hit {
            Some((drop, anchor)) => {
                drop.open(&anchor);
                true
            }
            None => false,
        }
    }

    /// Closes one dropdown (the probe cleanup).
    pub fn close_dropdown(&self, name: &str) {
        if let Some((_, d, _)) = self.dropdowns.iter().find(|(n, _, _)| *n == name) {
            d.popover().popdown();
        }
    }
}

fn fit_icon_name(mode: ImageFitMode) -> &'static str {
    match mode {
        ImageFitMode::Original => "Original",
        ImageFitMode::Fit => "FitAll",
        ImageFitMode::FitWidth => "FitWidth",
        ImageFitMode::FitWidthAdaptive => "FitWidthAdaptive",
        ImageFitMode::FitHeight => "FitHeight",
        ImageFitMode::BestFit => "FitBest",
    }
}

fn layout_icon_name(mode: PageLayoutMode, rtl: bool) -> &'static str {
    match mode {
        PageLayoutMode::Single => {
            if rtl {
                "SinglePageRtl"
            } else {
                "SinglePage"
            }
        }
        PageLayoutMode::Double => {
            if rtl {
                "TwoPageForcedRtl"
            } else {
                "TwoPageForced"
            }
        }
        PageLayoutMode::DoubleAdaptive => {
            if rtl {
                "TwoPageRtl"
            } else {
                "TwoPage"
            }
        }
        // Continuous renders pages in one strip; the C# icon is the
        // single-page image (`miContinuous.Image`).
        PageLayoutMode::Continuous => "SinglePage",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every toolbar action exists in the registry (the COMMANDS
    /// table or the shell-only actions).
    #[test]
    fn toolbar_actions_exist() {
        let known: std::collections::HashSet<&str> = crate::commands::COMMANDS
            .iter()
            .map(|c| c.action)
            .chain([
                "view-mode",
                "sort-column",
                "sort-direction",
                "group-by",
                "thumb-bigger",
                "thumb-smaller",
                "open-tab",
                "recent-book",
                "open-bookmark",
                "page-type",
                "page-rotation",
                "show-main-menu",
            ])
            .collect();
        for defs in [PREV, NEXT, ZOOM, ROTATE, TOOLS] {
            for node in defs {
                if let Item(_, action, _, _) = node {
                    let base = action
                        .split("::")
                        .next()
                        .unwrap()
                        .strip_prefix("win.")
                        .unwrap();
                    assert!(
                        known.contains(base),
                        "toolbar action {action} has no command"
                    );
                }
            }
        }
    }

    /// The menu-table reuse: the layout drop IS the menubar's Page
    /// Layout table (same actions, same radio values).
    #[test]
    fn layout_drop_matches_the_menubar_table() {
        for node in menubar::PAGE_LAYOUT {
            if let Item(_, action, _, _) = node {
                let base = action.split("::").next().unwrap();
                assert!(
                    base == "win.page-layout"
                        || base == "win.right-to-left"
                        || base == "win.page-fit"
                        || base == "win.only-fit-oversized",
                    "unexpected action {base} in PAGE_LAYOUT"
                );
            }
        }
    }

    /// The toolbar icon names resolve in the bundled set.
    #[test]
    fn toolbar_icons_resolve() {
        for mode in [
            ImageFitMode::Original,
            ImageFitMode::Fit,
            ImageFitMode::FitWidth,
            ImageFitMode::FitWidthAdaptive,
            ImageFitMode::FitHeight,
            ImageFitMode::BestFit,
        ] {
            assert!(
                crate::icon::path_for_name(fit_icon_name(mode)).is_some(),
                "fit icon {} missing",
                fit_icon_name(mode)
            );
        }
        for (mode, rtl) in [
            (PageLayoutMode::Single, false),
            (PageLayoutMode::Single, true),
            (PageLayoutMode::Double, false),
            (PageLayoutMode::Double, true),
            (PageLayoutMode::DoubleAdaptive, false),
            (PageLayoutMode::DoubleAdaptive, true),
            (PageLayoutMode::Continuous, false),
        ] {
            assert!(
                crate::icon::path_for_name(layout_icon_name(mode, rtl)).is_some(),
                "layout icon {} missing",
                layout_icon_name(mode, rtl)
            );
        }
        for name in [
            "Zoom",
            "ZoomClear",
            "AutoRotate",
            "RotateRight",
            "Tools",
            "FullScreen",
        ] {
            assert!(crate::icon::path_for_name(name).is_some(), "{name} missing");
        }
    }
}
