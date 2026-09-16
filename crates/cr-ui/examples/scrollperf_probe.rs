//! Headless scroll-perf probe: a 2875-book Thumbnail grid (the
//! reading-list scale), scrolled in jumps and small steps while the
//! `CR_TRACE` frame line reports the culling window, drawn item
//! count, and frame cost. Evidence for the T10 scroll slice.
//! Run: Xvfb + `CR_TRACE=1 cargo run -p cr-ui --release --example
//! scrollperf_probe` (release — the draw cost is the subject).
//!
//! `SCROLLPROBE_BIG_DETAIL=1` runs the measurement pass for the
//! 2026-09-16 scroll-ceiling report: 95,302 Detail rows, scrolled to
//! the end, printing the GTK scroll ceiling.
use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::{CrDateTime, CrGuid};
use cr_ui::browser::item_view::ItemView;
use cr_ui::browser::layout::ItemViewMode;
use gtk4::glib;
use gtk4::prelude::*;
use std::sync::Arc;

const BOOKS: usize = 2875;

/// The reported library scale (the 2026-09-16 scroll-ceiling report:
/// 95,302 books, Details, scroll stopped at 43,477 rows under the
/// 1,000,000 px canvas cap).
const BIG_BOOKS: usize = 95_302;

fn book(i: usize, covers: &[String]) -> ComicBook {
    let mut b = ComicBook {
        id: CrGuid::from_bytes([
            (i % 251) as u8,
            (i >> 8) as u8,
            0xA,
            0xB,
            0,
            0,
            0,
            1,
            2,
            3,
            4,
            5,
            6,
            7,
            8,
            9,
        ]),
        file_path: covers[i % covers.len()].clone(),
        added_time: CrDateTime::now(),
        ..Default::default()
    };
    b.info.series = format!("Amazing Chronicles Annual Vol. {}", 1 + i % 42);
    b.info.number = format!("{}", 1 + i % 300);
    b.info.page_count = 24;
    b.info.writer = "John Writer; Jane Penciller".into();
    b.current_page = (i % 24) as i32;
    b.last_page_read = (i % 24) as i32;
    if i.is_multiple_of(7) {
        b.info.community_rating = 4.5;
    }
    if i.is_multiple_of(11) {
        b.rating = 3.5;
    }
    b
}

fn main() {
    gtk4::init().expect("gtk init");
    cr_ui::theme::init();

    let work = std::path::Path::new("/tmp/opencode/scrollperf");
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).unwrap();
    let covers = build_covers(work);
    let big = std::env::var("SCROLLPROBE_BIG_DETAIL").is_ok();
    if big {
        run_big_detail(covers);
        return;
    }

    let pool = Arc::new(cr_engine::image_pool::ImagePool::new(None));
    let widgets = ItemView::create(pool);

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.scrollperf-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let win = gtk4::ApplicationWindow::builder()
            .application(app)
            .title("scrollperf")
            .build();
        win.set_default_size(1400, 900);
        win.set_child(Some(&widgets.scroller));
        win.present();

        widgets
            .view
            .set_books((0..BOOKS).map(|i| book(i, &covers)).collect());

        let iv = widgets.view.clone();
        let app_quit = app.clone();
        let steps = std::rc::Rc::new(std::cell::Cell::new(0u32));
        // Jump pass (big scroll-wheel flicks) then a smooth pass.
        glib::timeout_add_local(std::time::Duration::from_millis(700), move || {
            let n = steps.get();
            if n == 0 {
                let h = iv.thumb_height();
                println!("probe: {BOOKS} books, thumb {h}");
            }
            let jump = n < 8;
            let delta = if jump { 4000.0 } else { 600.0 };
            let y = iv.scroll_value().max(0.0) + delta;
            iv.set_scroll_value(y);
            steps.set(n + 1);
            if n >= 24 {
                println!("probe done");
                app_quit.quit();
                return glib::ControlFlow::Break;
            }
            glib::ControlFlow::Continue
        });
    });
    app.run();
}

/// A 1×1 red PNG, valid bytes, for when the bundled icons are absent.
fn png_fallback() -> Vec<u8> {
    const P: &[u8] = &[
        0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44,
        0x52, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90,
        0x77, 0x53, 0xDE, 0x00, 0x00, 0x00, 0x0C, 0x49, 0x44, 0x41, 0x54, 0x08, 0xD7, 0x63, 0xF8,
        0xCF, 0xC0, 0x00, 0x00, 0x03, 0x01, 0x01, 0x00, 0x18, 0xDD, 0x8D, 0xB0, 0x00, 0x00, 0x00,
        0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE, 0x42, 0x60, 0x82,
    ];
    P.to_vec()
}

fn build_covers(work: &std::path::Path) -> Vec<String> {
    let mut covers: Vec<String> = Vec::new();
    for (n, src) in [
        "cr-ui/assets/icons/Library.png",
        "cr-ui/assets/icons/ComicPage.png",
        "cr-ui/assets/icons/Bookmarks.png",
        "cr-ui/assets/icons/ThemeDark.png",
    ]
    .iter()
    .enumerate()
    {
        let dst = work.join(format!("cov{n}.png"));
        if std::path::Path::new(src).exists() {
            std::fs::copy(src, &dst).unwrap();
        } else {
            std::fs::write(&dst, png_fallback()).unwrap();
        }
        covers.push(dst.to_string_lossy().into_owned());
    }
    covers
}

/// The measurement pass: 95,302 Detail rows, scrolled to the end,
/// printing the GTK scroll ceiling and the final row reach.
fn run_big_detail(covers: Vec<String>) {
    let pool = Arc::new(cr_engine::image_pool::ImagePool::new(None));
    let widgets = ItemView::create(pool);
    widgets.view.configure(|c| c.mode = ItemViewMode::Detail);
    widgets
        .view
        .set_books((0..BIG_BOOKS).map(|i| book(i, &covers)).collect());

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.scrollperf-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let win = gtk4::ApplicationWindow::builder()
            .application(app)
            .title("bigdetail")
            .build();
        win.set_default_size(1400, 900);
        win.set_child(Some(&widgets.scroller));
        win.present();

        let iv = widgets.view.clone();
        let app_quit = app.clone();
        let step = std::rc::Rc::new(std::cell::Cell::new(0u32));
        glib::timeout_add_local(std::time::Duration::from_millis(400), move || {
            let n = step.get();
            step.set(n + 1);
            match n {
                0 => println!("bigdetail: books={}", iv.book_count()),
                1 => {
                    // A request far past any cap: GTK clamps the
                    // value to its upper bound, which is the
                    // measured scroll ceiling.
                    iv.set_scroll_value(100_000_000.0);
                }
                2 => {
                    println!("bigdetail: final scroll value = {}", iv.scroll_value());
                    println!("bigdetail done");
                    app_quit.quit();
                    return glib::ControlFlow::Break;
                }
                _ => {}
            }
            glib::ControlFlow::Continue
        });
    });
    app.run();
}
