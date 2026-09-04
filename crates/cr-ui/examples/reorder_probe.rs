//! Headless reproduction: seeds a library book for the test comic,
//! applies the editor's Move-to-Top op, saves, then the app opens it.
use cr_core::model::comic_book::ComicBook;
use cr_core::model::comic_page_info::ComicPageInfo;
use cr_core::xml::scalar::{CrDateTime, CrGuid};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let comic = args
        .get(1)
        .map(String::as_str)
        .unwrap_or("tests/testfiles/Absolute Flash (2025) Volume 01 Issue 009.cbz");

    // The isolated library (the probe env points the XDG dirs here).
    let mut lib = cr_engine::library::Library::open_at_default_location()
        .expect("open library")
        .0;
    let provider = cr_io::ComicProvider::open(std::path::Path::new(comic)).expect("provider");
    // The real scanner stores absolute paths (and GApplication hands
    // the app absolute paths on open).
    let comic = std::fs::canonicalize(comic).expect("canonicalize");
    let comic = comic.to_string_lossy().into_owned();
    let mut book = ComicBook {
        id: CrGuid::new_random(),
        file_path: comic.clone(),
        added_time: CrDateTime::now(),
        ..Default::default()
    };
    book.info.page_count = provider.page_count() as i32;
    book.info.pages = provider
        .pages()
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mut pg = ComicPageInfo {
                key: Some(p.name.clone()),
                ..Default::default()
            };
            pg.set_image_index(i as i32);
            pg
        })
        .collect();
    println!(
        "before: {:?}",
        book.info
            .pages
            .iter()
            .map(|p| p.image_index())
            .collect::<Vec<_>>()
    );
    // The editor's Move to Top on page 2 (index 1).
    book.info.move_pages(0, &[1]);
    println!(
        "after:  {:?}",
        book.info
            .pages
            .iter()
            .map(|p| p.image_index())
            .collect::<Vec<_>>()
    );
    lib.database_mut().books.push(book);
    lib.save().expect("save");
    println!("seeded");
}
