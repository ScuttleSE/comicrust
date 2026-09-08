//! Headless probe: the T6 Folders tab (the Files view). Seeds an
//! isolated XDG, builds a temp folder with fake comics (one carrying
//! a stored ComicInfo.xml — the first-100 metadata read), and gates
//! the REAL paths:
//! A. the Folders tab shows the folders page (the strip marks it),
//! B. drilling to the folder lists its comics (the provider scan),
//!    the stored series shows for the metadata file,
//! C. Include Sub Folders rescans with the subfolder comic,
//! D. Add To Favorites persists into the Settings,
//! E. back to Library: the library grid returns.
//! Run: Xvfb + `cargo run -p cr-ui --release --example foldersview_probe`
//! with an isolated XDG.
use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::{CrDateTime, CrGuid};
use gtk4::glib;
use gtk4::prelude::*;
use std::path::Path;

fn write_page_zip(path: &Path, extra: &[(&str, &[u8])]) {
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
    for (name, bytes) in extra {
        zip.start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut zip, bytes).unwrap();
    }
    zip.finish().unwrap();
}

fn comic_info_xml(series: &str, number: &str) -> Vec<u8> {
    format!(
        r#"<?xml version="1.0"?>
<ComicInfo xmlns:xsd="http://www.w3.org/2001/XMLSchema" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <Series>{series}</Series>
  <Number>{number}</Number>
</ComicInfo>"#
    )
    .into_bytes()
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
        eprintln!("REFUSED: set XDG_DATA_HOME=/tmp/opencode/<dir> AND XDG_CONFIG_HOME=/tmp/opencode/<dir> (the probe seeds books and writes Config.xml)");
        std::process::exit(1);
    }
    let work = std::path::Path::new("/tmp/opencode/foldersview");
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).unwrap();
    cr_ui::library::initialize().expect("session init");
    // A couple of library books (the Library tab gate).
    for i in 0..3 {
        let mut b = ComicBook {
            id: CrGuid::new_random(),
            file_path: format!("/comics/lib-{i}.cbz"),
            added_time: CrDateTime::now(),
            ..Default::default()
        };
        b.info.series = format!("Lib Series {i}");
        assert!(cr_ui::library::insert_new_book(&b));
    }
    // The scanned folder: two comics at the top, one in a subfolder.
    let folder = work.join("comics");
    let sub = folder.join("Sub");
    std::fs::create_dir_all(&sub).unwrap();
    write_page_zip(
        &folder.join("Info Comic 001.cbz"),
        &[("ComicInfo.xml", &comic_info_xml("Info Comic", "1"))],
    );
    write_page_zip(&folder.join("Plain Comic 002.cbz"), &[]);
    write_page_zip(&sub.join("Sub Comic 003.cbz"), &[]);

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.foldersview-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let app = app.clone();
        let (window, shell) = cr_ui::browser::shell::BrowserShell::create(&app);
        window.present();
        std::mem::forget(shell.clone());

        // A. The Folders tab: visible + click → the folders page +
        //    the strip marks it.
        glib::timeout_add_local(std::time::Duration::from_millis(700), {
            let shell = shell.clone();
            move || {
                let strip = shell.tab_strip_handle();
                let visible = strip.tab_visible(&cr_ui::browser::tabstrip::TabId::Folders);
                strip.click(&cr_ui::browser::tabstrip::TabId::Folders);
                let page = shell.state_folders_page();
                let sel = strip.selected();
                println!("A tab visible={visible} page={page:?} sel={sel:?}");
                let ok = visible
                    && page.as_deref() == Some("folders")
                    && sel == cr_ui::browser::tabstrip::TabId::Folders;
                println!("A ok={ok}");
                glib::ControlFlow::Break
            }
        });

        // B. Drill to the scanned folder → the grid lists the two
        //    top comics; the stored series shows for the metadata
        //    file (the first-100 rule).
        glib::timeout_add_local(std::time::Duration::from_millis(1200), {
            let shell = shell.clone();
            let folder = folder.to_string_lossy().into_owned();
            move || {
                let tree = shell.state_folders_tree();
                tree.drill_to(&folder);
                glib::timeout_add_local(std::time::Duration::from_millis(600), {
                    let shell = shell.clone();
                    move || {
                        let n = shell.state_folders_book_count();
                        let view = shell.state_folders_view_state();
                        let names: Vec<String> = (0..view.len())
                            .map(|i| cr_engine::display_text::caption(view.book(i)))
                            .collect();
                        println!("B count={n} names={names:?}");
                        let ok = n == 2 && names.iter().any(|s| s.contains("Info Comic"));
                        println!("B ok={ok}");
                        glib::ControlFlow::Break
                    }
                });
                glib::ControlFlow::Break
            }
        });

        // C. Include Sub Folders → the rescan lists the sub comic.
        glib::timeout_add_local(std::time::Duration::from_millis(2300), {
            let shell = shell.clone();
            move || {
                cr_ui::browser::folder_tree::click_include_sub(&shell.state_folders_tree());
                glib::timeout_add_local(std::time::Duration::from_millis(600), {
                    let shell = shell.clone();
                    move || {
                        let n = shell.state_folders_book_count();
                        let active = cr_ui::browser::folder_tree::include_sub_active(
                            &shell.state_folders_tree(),
                        );
                        let setting = cr_ui::library::settings()
                            .borrow()
                            .explorer_include_sub_folders;
                        println!("C count={n} toggle={active} setting={setting}");
                        let ok = n == 3 && active && setting;
                        println!("C ok={ok}");
                        glib::ControlFlow::Break
                    }
                });
                glib::ControlFlow::Break
            }
        });

        // D. Add To Favorites → the Settings list carries the folder.
        glib::timeout_add_local(std::time::Duration::from_millis(3300), {
            let shell = shell.clone();
            let folder = folder.to_string_lossy().into_owned();
            move || {
                if let Some(btn) = shell.state_folders_button("add-favorite") {
                    btn.emit_clicked();
                }
                let favs = shell.state_favorite_folders();
                println!("D favorites={favs:?}");
                let ok = favs.contains(&folder);
                println!("D ok={ok}");
                glib::ControlFlow::Break
            }
        });

        // E. Back to the Library tab: the library grid returns.
        glib::timeout_add_local(std::time::Duration::from_millis(3800), {
            let app = app.clone();
            let shell = shell.clone();
            move || {
                let strip = shell.tab_strip_handle();
                strip.click(&cr_ui::browser::tabstrip::TabId::Library);
                let page = shell.state_folders_page();
                let n = shell.state_grid_book_count();
                println!("E page={page:?} library-books={n} (expect 3)");
                let ok = page.as_deref() == Some("browser") && n == 3;
                println!("E ok={ok}");
                println!("FOLDERSVIEW PROBE DONE");
                app.quit();
                glib::ControlFlow::Break
            }
        });

        glib::timeout_add_local(std::time::Duration::from_secs(90), {
            let app = app.clone();
            move || {
                eprintln!("TIMEOUT — probe did not finish");
                app.quit();
                glib::ControlFlow::Break
            }
        });
    });

    app.run();
}
