//! Headless gate: the boot with a persisted DETAIL workspace must
//! not abort. The user's 2026-09-06 crash: the persisted Detail mode
//! makes the status-bar slider's first `sync_slider` clamp its fresh
//! value (96) into the Detail range (12..48) — `set_range` emitted
//! `value_changed` OUTSIDE the sync guard, the handler re-entered
//! `set_item_size` while the selection notify still held the
//! ItemView borrow → "RefCell already borrowed" abort at boot.
//! The probe writes a Detail-mode Config.xml + seeds books into an
//! isolated XDG, then boots the real shell — an abort IS the gate
//! failure.
//! Run with an isolated XDG: XDG_DATA_HOME=/tmp/opencode/... cargo
//! run -p cr-ui --example bootreentry_probe
use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::{CrDateTime, CrGuid};
use gtk4::prelude::*;

fn main() {
    gtk4::init().expect("gtk init");
    cr_ui::theme::init();
    if !std::env::var("XDG_DATA_HOME")
        .map(|v| v.contains("/tmp/opencode"))
        .unwrap_or(false)
    {
        eprintln!("REFUSED: set XDG_DATA_HOME=/tmp/opencode/<dir> (the probe seeds books into the DB it opens)");
        std::process::exit(1);
    }
    let work = std::path::Path::new("/tmp/opencode/bootreentry");
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).unwrap();

    // Three books so the boot list selection has content.
    for name in ["probe a.cbz", "probe b.cbz", "probe c.cbz"] {
        let comic = work.join(name);
        std::fs::write(&comic, b"not a real archive").ok();
        let mut book = ComicBook {
            id: CrGuid::new_random(),
            file_path: comic.to_string_lossy().into_owned(),
            added_time: CrDateTime::now(),
            ..Default::default()
        };
        book.info.series = format!("Series {}", name);
        book.info.page_count = 1;
        let (mut lib, _) = cr_engine::library::Library::open_at_default_location().unwrap();
        lib.database_mut().books.push(book);
        lib.save().unwrap();
    }

    // The persisted DETAIL workspace (the crash precondition), saved
    // through the real settings writer.
    cr_ui::library::initialize().expect("session init");
    let mut ws = cr_core::settings::workspace::WorkspaceState::default();
    ws.view.mode = cr_core::model::enums::ItemViewMode::Detail;
    ws.view.row_height = 32;
    {
        let settings = cr_ui::library::settings();
        settings.borrow_mut().current_workspace = Some(ws);
    }
    cr_ui::library::save_settings();

    // The shell applies `current_workspace` at create (the T14
    // restore point); the boot list selection then fires the notify
    // chain — the old code aborted inside it.
    let app = gtk4::Application::builder()
        .application_id("org.comicrust.bootreentry-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let app = app.clone();
        let (_window, shell) = cr_ui::browser::shell::BrowserShell::create(&app);
        std::mem::forget(shell.clone());

        gtk4::glib::timeout_add_local(std::time::Duration::from_millis(1500), move || {
            let view = shell.item_view_state();
            println!(
                "boot grid: {} books mode={}",
                view.len(),
                shell.state_grid_mode()
            );
            println!("PROBE COMPLETE");
            app.quit();
            gtk4::glib::ControlFlow::Break
        });
    });

    app.run();
}
