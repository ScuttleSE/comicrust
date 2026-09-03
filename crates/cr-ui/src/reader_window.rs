//! The reader window shell around the page view.
//!
//! Phase 3 T1 keeps this to the walking skeleton: open a comic, put
//! page 0 on screen through a GDK paintable (ADR-008 names the
//! GDK-paintable/cairo path as the first renderer). Layout modes, fit
//! modes, zoom/pan arrive in T2/T3 as the `ImageDisplayControl` port.

use std::path::Path;

use anyhow::Context;
use gtk4::gdk;
use gtk4::prelude::*;
use gtk4::{glib, Application, ApplicationWindow, HeaderBar, Label, Picture};

use cr_io::ComicProvider;

/// Default reader window size (the C# persists its own window layout;
/// workspace persistence arrives in Phase 7).
const DEFAULT_WIDTH: i32 = 1200;
const DEFAULT_HEIGHT: i32 = 800;

pub struct ReaderWindow {
    window: ApplicationWindow,
}

impl ReaderWindow {
    /// Opens `path` and renders page 0. Errors surface to the caller
    /// (the app shows a dialog) — the C# treats an unopenable comic
    /// the same way.
    pub fn open(app: &Application, path: &Path) -> anyhow::Result<Self> {
        let provider = ComicProvider::open(path)
            .with_context(|| format!("Unsupported or unreadable comic: {}", path.display()))?;
        let page_count = provider.page_count();
        let texture = load_page_texture(&provider, 0);

        let window = ApplicationWindow::builder()
            .application(app)
            .title(Self::window_title(path))
            .default_width(DEFAULT_WIDTH)
            .default_height(DEFAULT_HEIGHT)
            .css_classes(["reader-window"])
            .build();

        let header = HeaderBar::new();
        let subtitle = Label::builder()
            .label(Self::page_subtitle(0, page_count))
            .css_classes(["placeholder-label"])
            .build();
        header.pack_end(&subtitle);
        window.set_titlebar(Some(&header));

        window.set_child(Some(&Self::page_area(texture.as_ref(), path)));

        Ok(ReaderWindow { window })
    }

    pub fn present(&self) {
        self.window.present();
    }

    fn window_title(path: &Path) -> String {
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "comicrust".into())
    }

    fn page_subtitle(page: usize, page_count: usize) -> String {
        if page_count == 0 {
            "No pages".into()
        } else {
            format!("Page {} of {}", page + 1, page_count)
        }
    }

    /// The centered page area: black background (reader style), the
    /// page scaled to fit (`ContentFit::Contain` matches the C#
    /// default `ImageFitMode.Fit` behavior closely enough for the
    /// skeleton; exact fit-mode math lands with T2).
    fn page_area(texture: Option<&gdk::MemoryTexture>, path: &Path) -> gtk4::Box {
        let area = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        area.set_css_classes(&["reader-page-area"]);
        area.set_hexpand(true);
        area.set_vexpand(true);
        match texture {
            Some(texture) => {
                // `can_shrink` + the Picture default (keep aspect
                // ratio) give fit-inside behavior, the cairo-first
                // stand-in for the C# default `ImageFitMode.Fit`.
                let picture = Picture::for_paintable(texture);
                picture.set_can_shrink(true);
                picture.set_hexpand(true);
                picture.set_vexpand(true);
                area.append(&picture);
            }
            None => {
                let label = Label::builder()
                    .label(format!("Cannot display page 1 of {}", path.display()))
                    .css_classes(["placeholder-label"])
                    .vexpand(true)
                    .valign(gtk4::Align::Center)
                    .build();
                area.append(&label);
            }
        }
        area
    }
}

/// Loads and decodes page `index` into a paintable texture; `None`
/// when the page is missing or not decodable (cr-image reports
/// UnsupportedFormat for HEIF/AVIF/J2K — the placeholder shows).
fn load_page_texture(provider: &ComicProvider, index: usize) -> Option<gdk::MemoryTexture> {
    let bytes = provider.read_page(index)?;
    let image = cr_image::decode::decode(&bytes).ok()?;
    Some(texture_from_image(&image))
}

fn texture_from_image(image: &cr_image::Image) -> gdk::MemoryTexture {
    gdk::MemoryTexture::new(
        image.width as i32,
        image.height as i32,
        gdk::MemoryFormat::R8g8b8a8,
        &glib::Bytes::from(&image.rgba),
        (image.width * 4) as usize,
    )
}
