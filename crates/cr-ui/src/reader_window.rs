//! The reader window shell around the page view. The page widget is
//! the `ImageDisplayControl` port (`reader::page_view`); this window
//! supplies the chrome (header, page indicator).

use std::path::Path;
use std::sync::Arc;

use anyhow::Context;
use gtk4::prelude::*;
use gtk4::{Application, ApplicationWindow, HeaderBar, Label};

use cr_engine::image_pool::ImagePool;

use crate::reader::page_view::PageView;

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
        let provider = cr_io::ComicProvider::open(path)
            .with_context(|| format!("Unsupported or unreadable comic: {}", path.display()))?;
        let page_count = provider.page_count();

        let window = ApplicationWindow::builder()
            .application(app)
            .title(Self::window_title(path))
            .default_width(DEFAULT_WIDTH)
            .default_height(DEFAULT_HEIGHT)
            .css_classes(["reader-window"])
            .build();

        let header = HeaderBar::new();
        let subtitle = Label::builder().css_classes(["placeholder-label"]).build();
        header.pack_end(&subtitle);
        window.set_titlebar(Some(&header));

        // One render pool per reader window (memory-only until the
        // settings port decides the cache location).
        let pool = Arc::new(ImagePool::new(None));
        let page_view = PageView::new(pool);
        page_view
            .open(provider, path)
            .map_err(|e| anyhow::anyhow!(e))?;
        {
            let subtitle = subtitle.clone();
            page_view.set_page_callback(Some(Box::new(move |page, count| {
                subtitle.set_text(&page_subtitle(page, count));
            })));
        }
        subtitle.set_text(&page_subtitle(0, page_count));
        // The `Exit` reader command (Q) closes the window — the C#
        // `ControlExit` closes the main form.
        {
            let win = window.clone();
            page_view.set_exit_callback(Box::new(move || win.close()));
        }
        window.set_child(Some(page_view.widget()));
        page_view.widget().grab_focus();

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
}

fn page_subtitle(page: usize, page_count: usize) -> String {
    if page_count == 0 {
        "No pages".into()
    } else {
        format!("Page {} of {}", page + 1, page_count)
    }
}
