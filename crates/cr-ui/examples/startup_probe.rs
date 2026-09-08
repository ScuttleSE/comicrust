//! T10 timing probe: the staged startup cost. Three scenarios (the
//! first CLI arg picks one):
//! - `fresh`  — no database file: the default tree is created.
//! - `real`   — the real-world 255-book ComicDb.xml (committed).
//! - `big`    — a synthetic 10 000-book database written before the
//!   session opens (the 10k scale).
//!
//! Stages: `library::initialize` (DB load + settings) →
//! `BrowserShell::create` (wire + workspace apply + boot fill =
//! evaluate + set_books + navigator) → present + first frames.
//!
//! Run: Xvfb + `cargo run -p cr-ui --release --example startup_probe -- big`
//! with an isolated XDG pair (the probe writes the database it opens).
use gtk4::glib;
use gtk4::prelude::*;
use std::time::Instant;

const BOOKS: usize = 10_000;

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
        eprintln!("REFUSED: set XDG_DATA_HOME=/tmp/opencode/<dir> AND XDG_CONFIG_HOME=/tmp/opencode/<dir> (the probe writes a database and Config.xml)");
        std::process::exit(1);
    }
    let scenario = std::env::args().nth(1).unwrap_or_else(|| "fresh".into());
    let xdg = std::env::var("XDG_DATA_HOME").unwrap();
    let db_dir = std::path::Path::new(&xdg).join("comicrust").join("ComicDb");
    std::fs::create_dir_all(&db_dir).unwrap();

    match scenario.as_str() {
        "real" => {
            let src = std::path::Path::new("tests/realworld/ComicDb.xml");
            std::fs::copy(src, db_dir.join("ComicDb.xml")).unwrap();
        }
        "big" => {
            // The default list tree (Library + the smart lists) so
            // the boot fill has the Library root to select.
            let mut db = cr_core::database::comic_database::create_new();
            for i in 0..BOOKS {
                let mut b = cr_core::model::comic_book::ComicBook {
                    id: cr_core::xml::scalar::CrGuid::new_random(),
                    file_path: format!("/comics/book{i:05}.cbz"),
                    added_time: cr_core::xml::scalar::CrDateTime::now(),
                    ..Default::default()
                };
                b.info.series = format!("Series {:04}", i % 500);
                b.info.number = format!("{}", 1 + i / 500);
                b.info.page_count = 24;
                db.books.push(b);
            }
            let t = Instant::now();
            cr_core::database::comic_database::save(&db, &db_dir.join("ComicDb.xml")).unwrap();
            println!("SEED big db write: {:?}", t.elapsed());
        }
        _ => {}
    }

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.startup-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let scenario = scenario.clone();
        let t0 = Instant::now();
        cr_ui::library::initialize().expect("session init");
        let t_init = t0.elapsed();

        let t1 = Instant::now();
        let (window, shell) = cr_ui::browser::shell::BrowserShell::create(app);
        let t_create = t1.elapsed();
        window.present();

        println!(
            "STARTUP [{scenario}] init={t_init:?} shell-create={t_create:?} books={}",
            cr_ui::library::session().borrow().database().books.len()
        );

        // A first-frame + settle read (the boot fill evaluates + draws
        // the grid asynchronously through the idle pumps).
        glib::timeout_add_local(std::time::Duration::from_millis(1500), {
            let app = app.clone();
            let shell = shell.clone();
            move || {
                let t2 = t0.elapsed();
                println!(
                    "STARTUP [{scenario}] total-to-settled={t2:?} grid-books={}",
                    shell.state_grid_book_count()
                );
                app.quit();
                glib::ControlFlow::Break
            }
        });
        std::mem::forget(shell.clone());
    });

    // Empty argv: GApplication would treat the scenario arg as a FILE
    // to open ("This application can not open files" + exit).
    app.run_with_args::<&str>(&[]);
}
