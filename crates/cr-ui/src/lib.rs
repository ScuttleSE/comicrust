//! GTK4 shell: reader, ItemView browser, dialogs, theming, i18n.
//!
//! Phase 3 scope: the reader (cairo renderer first, ADR-008). The
//! browser, dialogs, and i18n wiring arrive in Phases 4/5/7.

pub mod app;
pub mod bitmap;
pub mod browser;
pub mod dialogs;
pub mod library;
pub mod reader;
pub mod reader_shell;
pub mod settings;
pub mod theme;

/// Boots the GTK application. `args` are the command-line arguments
/// after the program name; `args[0]` may name a comic file to open
/// directly (the C# associates comic files with the executable).
pub fn run(args: Vec<String>) {
    app::run(args);
}
