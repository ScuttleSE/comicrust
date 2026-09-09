//! The bundled ComicRack PNG icon set (`ComicRack/Resources/*.png`,
//! Phase 5.5 T2): every chrome icon the C# fetches through
//! `Properties.Resources.<Name>` lives in `assets/icons/<Name>.png`
//! and the `Dark*` names in `assets/icons/Dark/<Base>.png` (the resx
//! maps `Resources.DarkSort` to `Dark\Sort.png`). Only the PNG set
//! is bundled — the 17 resx GIFs are task animations (scan/export/
//! device sync) and `ComicRackAppSmall.ico` is the Windows icon;
//! none have a ported consumer (recorded deviation).
//!
//! `icon()` returns a cached `gdk::Texture` by resx name. Names may
//! carry a `#variant` suffix (Phase 5.5 T2 kickoff rule); the loader
//! falls back to the base name when the variant file is absent. No
//! C# fetcher produces `#` names for these resources today — the
//! fallback is loader-side convenience for future variant keys.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gtk4::gdk;
use gtk4::gio;

/// The asset roots, tried in order (Phase 11 packaging: the portable
/// tarball + dev roots first, then the system install / XDG roots —
/// see `assets::asset_roots`).
fn asset_roots() -> Vec<PathBuf> {
    crate::assets::asset_roots()
}

thread_local! {
    static CACHE: RefCell<HashMap<String, Option<gdk::Texture>>> =
        RefCell::new(HashMap::new());
}

/// Reads a bundled icon as raw RGBA (a cairo-draw consumer — the
/// `gdk::Texture` cache serves widget consumers only).
pub fn image_for_name(name: &str) -> Option<cr_image::Image> {
    let path = path_for_name(name)?;
    let bytes = std::fs::read(path).ok()?;
    cr_image::decode::decode(&bytes).ok()
}

/// Resolves a resx name to an existing asset path. Rule (verified
/// against `Resources.resx` — see `tests/icons.rs`): the name is the
/// file name; a `Dark` prefix maps into the `Dark/` subfolder with
/// the prefix stripped; a `#variant` suffix falls back to the base
/// name. Returns `None` for GIF/ICO resx names (not bundled).
pub fn path_for_name(name: &str) -> Option<PathBuf> {
    let base = name.split('#').next().unwrap_or(name);
    if base.is_empty() {
        return None;
    }
    let dark = base.strip_prefix("Dark").filter(|rest| !rest.is_empty());
    for root in asset_roots() {
        if let Some(rest) = dark {
            let candidate = root.join("icons").join("Dark").join(format!("{rest}.png"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        let candidate = root.join("icons").join(format!("{base}.png"));
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// The cached texture for a resx name (negative results cache too).
pub fn icon(name: &str) -> Option<gdk::Texture> {
    if let Some(hit) = CACHE.with(|c| c.borrow().get(name).cloned()) {
        return hit;
    }
    let texture = path_for_name(name).and_then(|path| load(&path));
    CACHE.with(|c| c.borrow_mut().insert(name.to_string(), texture.clone()));
    texture
}

/// Loads a PNG into a texture (`gdk_texture_new_from_file`, GDK
/// 4.0-era API per ADR-018).
fn load(path: &Path) -> Option<gdk::Texture> {
    gdk::Texture::from_file(&gio::File::for_path(path)).ok()
}
