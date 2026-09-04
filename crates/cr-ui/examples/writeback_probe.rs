//! Headless: the file write-back flow. Seeds an isolated library
//! with a COPY of the test comic, turns the update settings on via
//! the isolated Config.xml, applies an edit through `apply_edited`,
//! and verifies the file's ComicInfo.xml changed.
use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::{CrDateTime, CrGuid};

fn main() {
    gtk4::init().expect("gtk init");
    let src = "tests/testfiles/Absolute Flash (2025) Volume 01 Issue 009.cbz";
    let work = std::path::Path::new("/tmp/opencode/writeback");
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).unwrap();
    let comic = work.join("probe comic.cbz");
    std::fs::copy(src, &comic).unwrap();

    // The isolated settings: the auto write-back on.
    let paths = cr_core::paths::Paths::new_default();
    let settings = cr_core::settings::Settings {
        update_comic_files: true,
        auto_update_comics_files: true,
        ..Default::default()
    };
    settings
        .save(&cr_core::paths::settings_file(&paths))
        .unwrap();

    // Seed the library with the comic copy.
    let (mut lib, _) = cr_engine::library::Library::open_at_default_location().unwrap();
    let provider = cr_io::ComicProvider::open(&comic).unwrap();
    let mut book = ComicBook {
        id: CrGuid::new_random(),
        file_path: comic.to_string_lossy().into_owned(),
        added_time: CrDateTime::now(),
        ..Default::default()
    };
    book.info.page_count = provider.page_count() as i32;
    book.info.series = "Seed Series".into();
    book.info.pages = provider
        .pages()
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mut pg = cr_core::model::comic_page_info::ComicPageInfo {
                key: Some(p.name.clone()),
                ..Default::default()
            };
            pg.set_image_index(i as i32);
            pg
        })
        .collect();
    lib.database_mut().books.push(book);
    lib.save().unwrap();

    // The session (loads the settings + the DB).
    cr_ui::library::initialize().expect("session");

    // Edit a field through the commit path.
    let mut edited = {
        let lib = cr_ui::library::session();
        let l = lib.borrow();
        l.database().books[0].clone()
    };
    edited.info.series = "Edited Series".into();
    assert!(cr_ui::library::apply_edited(&edited), "the book applied");

    // The write rides the 100 ms debounce + the pool queues; drive
    // the main loop for 3 s, then verify.
    let loop_ = gtk4::glib::MainLoop::new(None, false);
    let quit = loop_.clone();
    gtk4::glib::timeout_add_local(std::time::Duration::from_secs(3), move || {
        quit.quit();
        gtk4::glib::ControlFlow::Break
    });
    loop_.run();

    // Verify: the archive's ComicInfo.xml carries the edit.
    let file = std::fs::File::open(&comic).unwrap();
    let mut archive = zip::ZipArchive::new(file).unwrap();
    use std::io::Read;
    let mut entry = archive.by_name("ComicInfo.xml").unwrap();
    let mut info = Vec::new();
    entry.read_to_end(&mut info).unwrap();
    let text = String::from_utf8_lossy(&info);
    println!(
        "RESULT series-edited={} book-info-dirty={}",
        text.contains("Edited Series"),
        {
            let lib = cr_ui::library::session();
            let l = lib.borrow();
            l.database().books[0].comic_info_is_dirty
        }
    );
    // The book list should also flip: the ModifiedInfo matcher reads
    // the flag (cleared after the write).
    let clean = !text.contains("Seed Series");
    println!("RESULT seed-gone={clean}");
}
