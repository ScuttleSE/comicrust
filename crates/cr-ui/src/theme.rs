//! CSS theming skeleton (ADR-004: plain GTK4, our own CSS provider,
//! no libadwaita). Dark-mode friendly: the reader paints its own
//! background, the shell stays neutral dark.

use gtk4::gdk;
use gtk4::CssProvider;
use gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION;
pub const CSS: &str = r#"
.placeholder-label {
    color: #808080;
    font-size: 14px;
}

/* The active menu row (the C# highlights the Library/Pages row of
   the shown panel — no checkbox on those items). */
.menu-row-active {
    background: rgba(128, 128, 128, 0.35);
}

/* The workspace tab strip (the C# TabBar row under the menubar):
   every item is one distinct bordered box; the selected one raises.
   Boxes hug their content (valign Center + min-height 0 — the T9
   user test: the row stretched the tabs around the text). The
   embedded reader toolbar compacts to the row (the theme's default
   button min-height + separator margins made the row 48 px). */
.tabstrip .tab {
    background: rgba(128, 128, 128, 0.22);
    border: 1px solid rgba(128, 128, 128, 0.45);
    border-radius: 4px;
    padding: 1px 6px;
    min-height: 0;
}

.tabstrip .tab-active {
    background: rgba(128, 128, 128, 0.55);
}

.tabstrip button {
    min-height: 0;
    padding-top: 2px;
    padding-bottom: 2px;
}

.tabstrip separator {
    margin-top: 0;
    margin-bottom: 0;
}

/* The caption zone inside a comic-tab box. */
.tabstrip .tab-inner {
    padding: 1px 2px;
    min-height: 0;
}

.tabstrip .tab-close {
    padding: 0;
    min-width: 14px;
    min-height: 14px;
    margin-left: 2px;
}

.tabstrip .tab-bold {
    font-weight: bold;
}

/* The buttons inside a tab sit on the tab box's background — the
   theme button surface would cut the active-tab highlight short
   before the X (the T9 user report). Transparent lets the box
   paint; hover keeps a subtle shade. */
.tabstrip .tab button {
    background: transparent;
}

.tabstrip .tab button:hover {
    background: rgba(128, 128, 128, 0.35);
}

/* The status bar (the C# statusStrip): a top-ruled row of sunken
   panels (`Border3DStyle.SunkenOuter` → inset borders). */
.status-bar {
    border-top: 1px solid rgba(128, 128, 128, 0.45);
    min-height: 22px;
}

.status-panel {
    background: rgba(128, 128, 128, 0.14);
    border: 1px solid rgba(128, 128, 128, 0.30);
    border-radius: 3px;
    padding: 0 6px;
    margin-top: 2px;
    margin-bottom: 2px;
}

/* The image-only lamp panels stay square around the icon. */
.status-bar button.status-panel {
    padding: 0 2px;
    min-height: 0;
}
"#;

/// Loads the CSS into the default display. Idempotent enough for the
/// single-instance shell; called once from `app::run`.
pub fn init() {
    let provider = CssProvider::new();
    provider.load_from_data(CSS);
    if let Some(display) = gdk::Display::default() {
        gtk4::style_context_add_provider_for_display(
            &display,
            &provider,
            STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
    // Dark until the settings boot resolves the theme (`set_dark`
    // runs right after `library::initialize` — the probes keep the
    // dark default).
    set_dark(true);
}

/// Applies the dark/light mode at runtime (`ThemeManager.Initialize`
/// parity point): GTK re-styles every widget immediately. The value
/// comes from `ExtendedSettings::effective_theme` — the C# `Theme`
/// getter (`UseDarkMode` forces Dark; `Default` renders light).
pub fn set_dark(dark: bool) {
    if let Some(settings) = gtk4::Settings::default() {
        settings.set_gtk_application_prefer_dark_theme(dark);
    }
}

/// The theme colors the cairo-drawn views consume — the C#
/// `SystemColors` parity (`ThemeColors.ItemView.DefaultBack` resolves
/// to `SystemColors.Window`, and the DarkThemeHandler swaps the system
/// color table): they flip with prefer-dark. Resolved per draw call
/// through the widget's style context, so a theme flip re-resolves
/// with no cache to invalidate.
#[derive(Clone, Copy)]
pub struct Palette {
    /// `theme_base_color` — the list surface (`SystemColors.Window`).
    pub base: (f64, f64, f64),
    /// `theme_bg_color` — the window backdrop (group/detail headers,
    /// the cover placeholders).
    pub window_bg: (f64, f64, f64),
    /// `theme_fg_color` — `SystemColors.WindowText`.
    pub fg: (f64, f64, f64),
    /// `theme_selected_bg_color`.
    pub selected_bg: (f64, f64, f64),
    /// `theme_selected_fg_color`.
    pub selected_fg: (f64, f64, f64),
}

/// The fallbacks are the former hardcoded dark values (the pre-toggle
/// look); a theme that omits a named color keeps them.
pub fn palette(widget: &impl gtk4::prelude::IsA<gtk4::Widget>) -> Palette {
    use gtk4::prelude::*;
    let lookup = |name: &str, fb: (f64, f64, f64)| {
        widget
            .style_context()
            .lookup_color(name)
            .map(|c| {
                (
                    f64::from(c.red()),
                    f64::from(c.green()),
                    f64::from(c.blue()),
                )
            })
            .unwrap_or(fb)
    };
    Palette {
        base: lookup("theme_base_color", (0.13, 0.13, 0.15)),
        window_bg: lookup("theme_bg_color", (0.18, 0.18, 0.21)),
        fg: lookup("theme_fg_color", (0.88, 0.88, 0.9)),
        selected_bg: lookup("theme_selected_bg_color", (0.2, 0.38, 0.62)),
        selected_fg: lookup("theme_selected_fg_color", (1.0, 1.0, 1.0)),
    }
}

/// Queues a redraw when the dark preference flips — a DrawingArea's
/// cairo output is not style-driven, so GTK does not invalidate it on
/// a theme change; the view palettes must re-resolve by redrawing.
pub fn redraw_on_theme_change(widget: &impl gtk4::prelude::IsA<gtk4::Widget>) {
    use gtk4::prelude::*;
    if let Some(settings) = gtk4::Settings::default() {
        let widget = widget.clone();
        settings.connect_notify_local(Some("gtk-application-prefer-dark-theme"), move |_, _| {
            widget.queue_draw()
        });
    }
}
