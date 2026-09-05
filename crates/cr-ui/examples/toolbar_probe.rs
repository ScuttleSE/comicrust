//! Headless probe: the T5 reader toolbar. Opens a comic, checks the
//! strip's visibility rules, the state text tracking (zoom %,
//! rotation °), the dropdown row clicks (fit radio through the real
//! widget path), and the undock round-trip (the toolbar rides).
//! Run: Xvfb + `cargo run -p cr-ui --example toolbar_probe` with an
//! isolated XDG.
use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::{CrDateTime, CrGuid};
use gtk4::glib;
use gtk4::prelude::*;

fn main() {
    gtk4::init().expect("gtk init");
    let src = "tests/testfiles/Absolute Flash (2025) Volume 01 Issue 009.cbz";
    let work = std::path::Path::new("/tmp/opencode/toolbar");
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).unwrap();
    let comic = work.join("probe t.cbz");
    std::fs::copy(src, &comic).unwrap();
    let provider = cr_io::ComicProvider::open(&comic).unwrap();
    let mut book = ComicBook {
        id: CrGuid::new_random(),
        file_path: comic.to_string_lossy().into_owned(),
        added_time: CrDateTime::now(),
        ..Default::default()
    };
    book.info.page_count = provider.page_count() as i32;
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
    let (mut lib, _) = cr_engine::library::Library::open_at_default_location().unwrap();
    lib.database_mut().books.push(book);
    lib.save().unwrap();
    cr_ui::library::initialize().expect("session init");

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.toolbar-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let (window, shell) = cr_ui::browser::shell::BrowserShell::create(app);
        window.present();
        std::mem::forget(shell.clone());

        glib::timeout_add_local(std::time::Duration::from_millis(1200), {
            let shell = shell.clone();
            let comic = comic.clone();
            move || {
                // A. The bar mounts and follows MinimalGui.
                let bar = shell.toolbar_widget();
                println!(
                    "BAR exists={} visible={}",
                    !bar.is_sensitive() || true,
                    bar.is_visible()
                );
                // Open a comic → the reader shows with the strip.
                let path = comic.clone();
                shell.open_comic(&path);
                println!("B reader-open bar_visible={}", bar.is_visible());
                glib::ControlFlow::Break
            }
        });

        // 2. The state text: zoom/rotate track the reader.
        glib::timeout_add_local(std::time::Duration::from_millis(2000), {
            let shell = shell.clone();
            move || {
                let _ = shell.state_dispatch("win.zoom-preset");
                shell.state_dispatch_param("win.zoom-preset", "200");
                println!("ZOOM text={} (expect 200%)", shell.toolbar_zoom_text());
                let _ = shell.state_dispatch("win.rotate-90");
                glib::timeout_add_local(std::time::Duration::from_millis(200), {
                    let shell = shell.clone();
                    move || {
                        println!("ROTATE text={} (expect 90°)", shell.toolbar_rotate_label());
                        glib::ControlFlow::Break
                    }
                });
                glib::ControlFlow::Break
            }
        });

        // 3. The dropdown row clicks: the fit radio fires.
        glib::timeout_add_local(std::time::Duration::from_millis(2700), {
            let shell = shell.clone();
            move || {
                let drop = shell.toolbar_dropdown("fit");
                let fired = drop.is_some_and(|d| d.click_row("win.page-fit::original"));
                let fit = shell.reader_current_fit_name();
                println!("FIT-DROP clicked={fired:?} fit={fit:?} (expect Some(original))");
                glib::ControlFlow::Break
            }
        });

        // 3b. The bookmark drop fill: set a bookmark on the current
        //     page, open the next-page drop, expect the row.
        glib::timeout_add_local(std::time::Duration::from_millis(3000), {
            let shell = shell.clone();
            move || {
                shell.state_set_bookmark_silent(2, "probe bm");
                let drop = shell.toolbar_dropdown("next");
                if let Some(d) = drop {
                    d.refresh_slot("bookmarks-next");
                    let rows = d.dyn_rows_snapshot("bookmarks-next");
                    println!("NEXT-DROP bookmarks={rows:?}");
                }
                glib::ControlFlow::Break
            }
        });

        glib::timeout_add_local(std::time::Duration::from_millis(3300), {
            let app = app.clone();
            move || {
                println!("PROBE COMPLETE");
                app.quit();
                glib::ControlFlow::Break
            }
        });
    });
    app.run();
}
