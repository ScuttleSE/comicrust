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
    color: #b0b0b0;
    font-size: 14px;
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
    // Follow the desktop dark preference; we ship no light/dark
    // switch yet (Phase 7 owns full theming).
    if let Some(settings) = gtk4::Settings::default() {
        settings.set_gtk_application_prefer_dark_theme(true);
    }
}
