//! Headless evidence probe: the menubar visibility across the view
//! states the T4 user test walks (browser ⇄ reader ⇄ browser, books
//! open and closed). Logs VISIBLE plus the rule inputs per stage.
use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::{CrDateTime, CrGuid};
use gtk4::glib;
use gtk4::prelude::*;

fn seed(src: &str, work: &std::path::Path, name: &str) -> ComicBook {
    let comic = work.join(name);
    std::fs::copy(src, &comic).unwrap();
    let provider = cr_io::ComicProvider::open(&comic).unwrap();
    let mut book = ComicBook {
        id: cr_core::xml::scalar::CrGuid::new_random(),
        file_path: comic.to_string_lossy().into_owned(),
        added_time: cr_core::xml::scalar::CrDateTime::now(),
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
    book
}

fn main() {
    gtk4::init().expect("gtk init");
    let src = "tests/testfiles/Absolute Flash (2025) Volume 01 Issue 009.cbz";
    let work = std::path::Path::new("/tmp/opencode/menubarvis");
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).unwrap();
    let (mut lib, _) = cr_engine::library::Library::open_at_default_location().unwrap();
    lib.database_mut()
        .books
        .push(seed(src, work, "probe v.cbz"));
    lib.save().unwrap();
    cr_ui::library::initialize().expect("session init");

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.menubarvis-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let (window, shell) = cr_ui::browser::shell::BrowserShell::create(app);
        window.present();
        std::mem::forget(shell.clone());
        let vis = |tag: &str, shell: &cr_ui::browser::shell::BrowserShell| {
            let m = shell.menubar().widget().is_visible();
            println!("{tag}: menubar_visible={m}");
        };

        glib::timeout_add_local(std::time::Duration::from_millis(1200), {
            let shell = shell.clone();
            move || {
                vis("A startup-browser (no books)", &shell);
                // Open a comic → reader.
                let path = work.join("probe v.cbz");
                shell.open_comic(&path);
                vis("B reader-open", &shell);
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(2000), {
            let shell = shell.clone();
            move || {
                // C: F3 → browser with the book still open.
                let _ = shell.state_dispatch("win.toggle-browser");
                vis("C browser-books-open", &shell);
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(2400), {
            let shell = shell.clone();
            move || {
                println!("D-START close dispatch");
                // D: close the book → browser with NO books.
                let ok = shell.state_dispatch("win.close");
                println!("D-DISPATCH fired={ok:?}");
                vis("D browser-no-books", &shell);
                // E: an action dispatch in the empty browser.
                let ok2 = shell.state_dispatch("win.refresh");
                println!("E-DISPATCH fired={ok2:?}");
                vis("E after-dispatch", &shell);
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(3200), {
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
