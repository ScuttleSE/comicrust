//! Headless: PageView::open_with_sequence with [1,0,2,...] — the
//! display must show archive page 1 at position 0.
use std::path::Path;
use std::sync::Arc;

use gtk4::prelude::*;

fn main() {
    gtk4::init().expect("gtk init");
    let comic = "tests/testfiles/Absolute Flash (2025) Volume 01 Issue 009.cbz";
    let provider = cr_io::ComicProvider::open(Path::new(comic)).expect("provider");
    let seq: Vec<usize> = (0..provider.page_count())
        .map(|i| {
            if i == 0 {
                1
            } else if i == 1 {
                0
            } else {
                i
            }
        })
        .collect();
    println!("sequence: {:?}", &seq[..4]);

    let win = gtk4::Window::new();
    win.set_default_size(800, 1000);
    let view = cr_ui::reader::page_view::PageView::new(Arc::new(
        cr_engine::image_pool::ImagePool::new(None),
    ));
    win.set_child(Some(view.widget()));
    win.present();
    view.open_with_sequence(provider, Path::new(comic), Some(seq), 0, 0)
        .expect("open");

    // Pump the main loop for ~8 s, then leave the window up for the
    // screenshot (the shell script kills us).
    let loop_ = gtk4::glib::MainLoop::new(None, false);
    let quit = loop_.clone();
    gtk4::glib::timeout_add_local(std::time::Duration::from_secs(60), move || {
        quit.quit();
        gtk4::glib::ControlFlow::Break
    });
    loop_.run();
}
