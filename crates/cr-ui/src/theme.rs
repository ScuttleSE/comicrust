//! CSS theming skeleton (ADR-004: plain GTK4, our own CSS provider,
//! no libadwaita). Dark-mode friendly: the reader paints its own
//! background, the shell stays neutral dark.

use gtk4::gdk;
use gtk4::CssProvider;
use gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION;
pub const CSS: &str = r#"
window.reader-window {
    background: #202020;
}

.reader-page-area {
    background: #202020;
}

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
