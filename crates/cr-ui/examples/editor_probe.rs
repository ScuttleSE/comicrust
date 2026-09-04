//! Headless probe: opens the book editor over synthetic books.
use cr_ui::dialogs::book_editor;
use gtk4::prelude::*;

fn main() {
    gtk4::init().expect("gtk init");
    let win = gtk4::Window::new();
    win.set_title(Some("editor-probe"));
    win.set_default_size(920, 680);

    let mut books = Vec::new();
    if let Some(mut b) = real_book("tests/testfiles/Absolute Flash (2025) Volume 01 Issue 009.cbz")
    {
        // The user's failing shape: NO stored page list.
        b.info.pages.clear();
        books.push(b);
    }
    for i in 1..=2 {
        let pages: Vec<cr_core::model::comic_page_info::ComicPageInfo> = (0..4)
            .map(|p| cr_core::model::comic_page_info::ComicPageInfo {
                page_type: if p == 0 {
                    cr_core::model::enums::ComicPageType(1) // FrontCover
                } else {
                    Default::default()
                },
                key: Some(format!("{p:04}.jpg")),
                ..Default::default()
            })
            .map(|mut pg| {
                pg.set_image_index(pages_index(pg.key.as_deref()));
                pg
            })
            .collect();
        books.push(cr_core::model::comic_book::ComicBook {
            id: cr_core::xml::scalar::CrGuid::new_random(),
            file_path: format!("/tmp/opencode/probe-book-{i}.cbz"),
            info: cr_core::model::comic_info::ComicInfo {
                series: "Probe Series".into(),
                number: format!("{i:03}"),
                title: format!("Title {i}"),
                writer: "Writer Name".into(),
                page_count: 4,
                pages,
                ..Default::default()
            },
            ..Default::default()
        });
    }

    let commit: book_editor::CommitFn = std::rc::Rc::new(|_edited| {});
    win.present();
    book_editor::show(&win, books, commit);
    gtk4::glib::MainLoop::new(None, false).run();
}
/// Builds one editor book from a real comic (the probe's preview +
/// page-list proof); falls back to the synthetic book.
fn real_book(path: &str) -> Option<cr_core::model::comic_book::ComicBook> {
    let provider = cr_io::ComicProvider::open(std::path::Path::new(path)).ok()?;
    let count = provider.page_count();
    let mut b = cr_core::model::comic_book::ComicBook {
        id: cr_core::xml::scalar::CrGuid::new_random(),
        file_path: path.to_string(),
        ..Default::default()
    };
    b.info.page_count = count as i32;
    b.info.pages = provider
        .pages()
        .iter()
        .enumerate()
        .map(|(i, p)| cr_core::model::comic_page_info::ComicPageInfo {
            key: Some(p.name.clone()),
            page_type: if i == 0 {
                cr_core::model::enums::ComicPageType(1)
            } else {
                Default::default()
            },
            ..Default::default()
        })
        .map(|mut pg| {
            let idx = pg
                .key
                .as_deref()
                .and_then(|k| k.split('.').next())
                .and_then(|k| k.parse().ok())
                .unwrap_or(0);
            pg.set_image_index(idx);
            pg
        })
        .collect();
    b.info.series = "Absolute Flash".into();
    b.info.number = "009".into();
    Some(b)
}

fn pages_index(key: Option<&str>) -> i32 {
    key.and_then(|k| k.split('.').next())
        .and_then(|k| k.parse().ok())
        .unwrap_or(0)
}
