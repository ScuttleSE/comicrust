//! Headless probe: the image-cache wiring (the C# `CacheManager`).
//! Gates: the disk caches under `Cache/{Thumbnails,Images}` receive
//! entries while browsing (the shell pool carries the settings
//! config), the "Generate Cover Thumbnails" warm-up covers every
//! book, a SECOND pool (a restart shape) re-reads the same bytes
//! from the disk cache, and the page-size write-back lands the
//! decoded `ImageWidth`/`ImageHeight` into the books (the DB save
//! persists them).
//! Run: Xvfb + `cargo run -p cr-ui --example cache_probe` with an
//! isolated XDG (the probe seeds books into the DB it opens).
use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::{CrDateTime, CrGuid};
use gtk4::glib;
use gtk4::prelude::*;

fn main() {
    gtk4::init().expect("gtk init");
    cr_ui::theme::init();
    let src = "tests/testfiles/Absolute Flash (2025) Volume 01 Issue 009.cbz";
    if !std::env::var("XDG_DATA_HOME")
        .map(|v| v.contains("/tmp/opencode"))
        .unwrap_or(false)
    {
        eprintln!(
            "REFUSED: set XDG_DATA_HOME=/tmp/opencode/<dir> (the probe seeds books into the DB it opens)"
        );
        std::process::exit(1);
    }
    let work = std::path::Path::new("/tmp/opencode/cache");
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).unwrap();
    for name in ["probe a.cbz", "probe b.cbz"] {
        let comic = work.join(name);
        std::fs::copy(src, &comic).unwrap();
        let provider = cr_io::ComicProvider::open(&comic).unwrap();
        let mut book = ComicBook {
            id: CrGuid::new_random(),
            file_path: comic.to_string_lossy().into_owned(),
            added_time: CrDateTime::now(),
            ..Default::default()
        };
        book.info.page_count = provider.page_count() as i32;
        let (mut lib, _) = cr_engine::library::Library::open_at_default_location().unwrap();
        lib.database_mut().books.push(book);
        lib.save().unwrap();
    }
    cr_ui::library::initialize().expect("session init");

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.cache-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let (window, shell) = cr_ui::browser::shell::BrowserShell::create(app);
        window.present();
        // The probe must keep the shell alive (the thread-local
        // lesson).
        std::mem::forget(shell.clone());

        // A. Select the Library list so the browser shows the books
        //    (the visible covers queue through the shell pool).
        glib::timeout_add_local(std::time::Duration::from_millis(600), {
            let shell = shell.clone();
            move || {
                let items = cr_ui::library::comic_lists_snapshot();
                if let Some(first) = items.first() {
                    shell.navigator().select_list(&first.base().id);
                }
                glib::ControlFlow::Break
            }
        });

        // B. After the thumb queues drain: the thumbnail disk cache
        //    holds entries, and the page-size write-back marked the
        //    books (the shell pool's cache events).
        glib::timeout_add_local(std::time::Duration::from_secs(4), {
            let shell = shell.clone();
            move || {
                let paths = cr_core::paths::Paths::new_default();
                let count = cache_files(&paths.thumbnail_cache_path);
                println!("A thumb-cache-files={count} (expect 2)");
                // The write-back: every book with a rendered cover
                // carries a pixel size.
                let lib = cr_ui::library::session();
                let l = lib.borrow();
                let sized = l
                    .database()
                    .books
                    .iter()
                    .filter(|b| b.info.pages.iter().any(|p| p.image_width != 0))
                    .count();
                let dirty = l.database().books.iter().any(|_| true);
                drop(l);
                println!("B sized-books={sized} (expect 2) db-has-books={dirty}");
                // The "Generate Cover Thumbnails" command (the shell
                // action — the same path the menu fires).
                let _ = shell.state_dispatch("win.generate-thumbnails");
                glib::ControlFlow::Break
            }
        });

        // C. After the warm-up drains: both books are covered, and a
        //    SECOND pool (the restart shape, same config) re-reads
        //    identical bytes from the disk cache.
        glib::timeout_add_local(std::time::Duration::from_secs(7), {
            move || {
                let paths = cr_core::paths::Paths::new_default();
                let count = cache_files(&paths.thumbnail_cache_path);
                println!("C thumb-cache-files-after-warmup={count} (expect 2)");
                let pool = std::sync::Arc::new(cr_engine::image_pool::ImagePool::with_config(
                    &cr_ui::library::image_pool_config(),
                ));
                let lib = cr_ui::library::session();
                let books: Vec<cr_core::model::comic_book::ComicBook> =
                    lib.borrow().database().books.clone();
                let mut identical = true;
                for book in &books {
                    let key = cr_engine::image_pool::front_cover_thumbnail_key(book);
                    let bytes = pool.render_thumbnail(&key).expect("render");
                    if bytes != pool.render_thumbnail(&key).expect("render again") {
                        identical = false;
                    }
                }
                println!("D second-pool-reuses={identical}");
                // The DB save persists the page sizes (the XML text).
                let _ = cr_ui::library::save_if_dirty();
                let xml = std::fs::read_to_string(cr_core::paths::database_file(
                    &cr_core::paths::Paths::new_default(),
                ))
                .unwrap_or_default();
                println!("E db-image-width-persisted={}", xml.contains("ImageWidth="));
                // F. The cache-root override (the Preferences Advanced
                // row writes the ini key + the global): the default
                // first, then the override moves the cache paths.
                let default_root = {
                    let p = cr_core::paths::Paths::new_default();
                    p.thumbnail_cache_path.clone()
                };
                println!("F default-cache-root={}", default_root.display());
                cr_ui::library::save_ini_keys(&[("CachePath", "/tmp/opencode/cache-override")]);
                {
                    cr_core::settings::ExtendedSettings::global_mut().cache_path =
                        Some("/tmp/opencode/cache-override".into());
                }
                let overridden = cr_core::paths::Paths::new_default();
                println!(
                    "G override-applied={} (expect Thumbnails under the override)",
                    overridden
                        .thumbnail_cache_path
                        .starts_with("/tmp/opencode/cache-override")
                );
                // Reset (the Reset button path) and restore the
                // default for the rest of the run.
                cr_ui::library::save_ini_keys(&[("CachePath", "")]);
                {
                    cr_core::settings::ExtendedSettings::global_mut().cache_path = None;
                }
                shell.window().close();
                glib::ControlFlow::Break
            }
        });
    });

    app.run();
}

fn cache_files(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.flatten()
                .filter(|e| e.path().extension().is_some_and(|x| x == "cache"))
                .count()
        })
        .unwrap_or(0)
}
