//! The About dialog — the C# `ShowAboutDialog` shows the Splash form
//! (the splash image with the version line); the port renders a small
//! modal About dialog with the same image (bundled, include-bytes —
//! the papers/icons precedent) and the `0.0.<commits>` version from
//! ADR-020 (the build sets `VERSION`; dev builds fall back to the
//! local git count).
//!
//! The C# splash overlay text (copyright + "V x.y.z" + bitness) maps
//! to the About dialog's version + comments lines. The Help links and
//! the copyright line are the recorded ADR-024 omissions.

use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{gdk, AboutDialog};

/// The bundled splash image (the C# `Resources.Splash` — the About
/// box shows the same picture).
const SPLASH_PNG: &[u8] = include_bytes!("../../assets/splash.png");

/// The app version (`0.0.<commits>` per ADR-020; the release
/// workflow sets `VERSION`, dev builds fall back to the local git
/// count through build.rs).
pub fn app_version() -> &'static str {
    env!("COMICRUST_VERSION")
}

/// The About dialog (`MainForm.ShowAboutDialog` — modal, the splash
/// image, the version line).
pub fn show_about(parent: &impl IsA<gtk4::Window>) {
    let dialog = AboutDialog::builder()
        .title("About ComicRust")
        .transient_for(parent)
        .modal(true)
        .program_name("ComicRust")
        .version(app_version())
        .comments("A Linux-native port of ComicRack Community Edition")
        .resizable(false)
        .build();
    if let Ok(texture) = gdk::Texture::from_bytes(&glib::Bytes::from(SPLASH_PNG)) {
        dialog.set_logo(Some(&texture));
    }
    dialog.present();
}

#[cfg(test)]
mod tests {
    #[test]
    fn version_carries_the_zero_dot_zero_scheme() {
        let v = super::app_version();
        assert!(
            v.starts_with("0.0."),
            "version {v} misses the ADR-020 scheme"
        );
    }

    #[test]
    fn splash_bytes_are_a_png() {
        assert_eq!(&super::SPLASH_PNG[..8], b"\x89PNG\r\n\x1a\n");
    }
}
