//! Headless probe: the "no metadata" tag (the port addition — no C#
//! counterpart; user request). Gates: a file-backed comic whose
//! descriptive fields are all empty draws ONE tag in the grid while
//! its metadata-carrying neighbor draws none, and the tag disappears
//! once an editor commit (`apply_edited` — the same funnel the Comic
//! Vine scrape lands in) fills a key field.
//! Run: Xvfb + `cargo run -p cr-ui --example metadatatag_probe` with
//! an isolated XDG pair.
use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::{CrDateTime, CrGuid};
use gtk4::glib;
use gtk4::prelude::*;

fn write_page_zip(path: &std::path::Path) {
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file("00000.jpg", zip::write::SimpleFileOptions::default())
        .unwrap();
    // A 1x1 JPEG placeholder (the decoder accepts it as a page).
    std::io::Write::write_all(
        &mut zip,
        &[
            0xFF, 0xD8, 0xFF, 0xDB, 0x00, 0x43, 0x00, 0x08, 0x06, 0x06, 0x07, 0x06, 0x05, 0x08,
            0x07, 0x07, 0x09, 0x09, 0x08, 0x0A, 0x0C, 0x14, 0x0D, 0x0C, 0x0B, 0x0B, 0x0C, 0x19,
            0x12, 0x13, 0x0F, 0x14, 0x1D, 0x1A, 0x1F, 0x1E, 0x1D, 0x1A, 0x1C, 0x1C, 0x20, 0x24,
            0x2E, 0x27, 0x20, 0x22, 0x2C, 0x23, 0x1C, 0x1E, 0x23, 0x27, 0x29, 0x2B, 0x2E, 0x27,
            0x2C, 0x2A, 0x2D, 0x2F, 0x29, 0x2B, 0xFF, 0xD9,
        ],
    )
    .unwrap();
    zip.finish().unwrap();
}

fn main() {
    gtk4::init().expect("gtk init");
    cr_ui::theme::init();
    if !std::env::var("XDG_DATA_HOME")
        .map(|v| v.contains("/tmp/opencode"))
        .unwrap_or(false)
        || !std::env::var("XDG_CONFIG_HOME")
            .map(|v| v.contains("/tmp/opencode"))
            .unwrap_or(false)
    {
        eprintln!("REFUSED: set XDG_DATA_HOME=/tmp/opencode/<dir> AND XDG_CONFIG_HOME=/tmp/opencode/<dir> (the probe writes the database)");
        std::process::exit(1);
    }
    let work = std::path::Path::new("/tmp/opencode/metadatatag");
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).unwrap();

    // Two file-backed books: one with series metadata, one without.
    let empty_path = work.join("No Info 001.cbz");
    let full_path = work.join("Full Info 001.cbz");
    write_page_zip(&empty_path);
    write_page_zip(&full_path);
    for (path, series) in [(&empty_path, ""), (&full_path, "Full Series")] {
        let provider = cr_io::ComicProvider::open(path).unwrap();
        let mut book = ComicBook {
            id: CrGuid::new_random(),
            file_path: path.to_string_lossy().into_owned(),
            added_time: CrDateTime::now(),
            ..Default::default()
        };
        book.info.series = series.into();
        book.info.page_count = provider.page_count() as i32;
        let (mut lib, _) = cr_engine::library::Library::open_at_default_location().unwrap();
        lib.database_mut().books.push(book);
        lib.save().unwrap();
    }
    cr_ui::library::initialize().expect("session init");

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.metadatatag-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let (window, shell) = cr_ui::browser::shell::BrowserShell::create(app);
        window.present();
        std::mem::forget(shell.clone());
        let empty_path = empty_path.clone();

        // Settle past the boot fill's 200 ms debounced Library
        // evaluation AND the first thumb decode (the badge draws only
        // on a ready thumb) before the first read.
        glib::timeout_add_local(std::time::Duration::from_millis(900), {
            let shell = shell.clone();
            move || {
                // A. Exactly ONE tag: the metadataless book draws it,
                //    the metadata-carrying neighbor draws none.
                let drawn = shell.state_grid_metadata_badge_draws();
                assert_eq!(
                    drawn, 1,
                    "A FAIL: expected exactly 1 metadata tag drawn, got {drawn}"
                );
                println!("A ok: the metadataless book draws the tag ({drawn})");

                // B. The manual-edit path: `apply_edited` (the editor
                //    commit; the Comic Vine scrape funnels through the
                //    same fields) fills a key field → the tag clears.
                let lib = cr_ui::library::session();
                let mut book = {
                    let lib = lib.borrow();
                    lib.find_book(&empty_path.to_string_lossy())
                        .expect("B: the empty book is in the library")
                        .clone()
                };
                book.info.series = "Edited Series".into();
                assert!(cr_ui::library::apply_edited(&book), "B: the edit lands");
                shell.refresh_after_data_change();
                glib::timeout_add_local(std::time::Duration::from_millis(600), {
                    let shell = shell.clone();
                    move || {
                        let drawn = shell.state_grid_metadata_badge_draws();
                        assert_eq!(
                            drawn, 0,
                            "B FAIL: the tag must clear once the metadata is edited, got {drawn}"
                        );
                        println!("B ok: the edited book no longer draws the tag");
                        std::process::exit(0);
                    }
                });
                glib::ControlFlow::Break
            }
        });
        let _ = window;
    });

    let _ = app.run();
}
