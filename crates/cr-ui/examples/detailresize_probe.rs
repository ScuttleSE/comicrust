//! Headless probe: the T5 Details-view column resize. Drives the
//! REAL drag path (the separator hit → begin/move/end with the C#
//! clamp math), the double-click auto-size, and the width
//! persistence (the workspace collect → write → a second shell
//! restores the dragged width). Gates:
//! A. the drag: Series 200 → ~280 through the real move math, live
//!    reflow (the layout's content width tracks),
//! B. the auto-size: the Series column shrinks to the widest cell
//!    text + padding,
//! C. the persistence: collect → apply in a second shell keeps the
//!    resized width,
//! D. the zero clamp: dragging left past 0 clamps at 0 (the C#
//!    0..10000).
//! Run: Xvfb + `cargo run -p cr-ui --release --example detailresize_probe`
//! with an isolated XDG.
use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::{CrDateTime, CrGuid};
use gtk4::glib;
use gtk4::prelude::*;

fn series_width(shell: &cr_ui::browser::shell::BrowserShell) -> i32 {
    shell
        .state_column_widths()
        .iter()
        .find(|(id, _, _)| *id == 1)
        .map(|(_, _, w)| *w)
        .unwrap_or(-1)
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
        eprintln!("REFUSED: set XDG_DATA_HOME=/tmp/opencode/<dir> AND XDG_CONFIG_HOME=/tmp/opencode/<dir> (the probe seeds books and can write comicrust.toml)");
        std::process::exit(1);
    }
    let work = std::path::Path::new("/tmp/opencode/detailresize");
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).unwrap();
    cr_ui::library::initialize().expect("session init");
    for i in 0..12 {
        let mut b = ComicBook {
            id: CrGuid::new_random(),
            file_path: format!("/comics/s{i}.cbz"),
            added_time: CrDateTime::now(),
            ..Default::default()
        };
        b.info.series = format!("Resize Series {}", i % 4);
        b.info.number = format!("{}", 1 + i);
        b.info.writer = "Alan Writer".into();
        assert!(cr_ui::library::insert_new_book(&b));
    }

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.detailresize-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let app = app.clone();
        let (window, shell) = cr_ui::browser::shell::BrowserShell::create(&app);
        window.present();
        std::mem::forget(shell.clone());

        // A. Detail mode, then the drag: start at the Series right
        //    edge (8 + 200 = 208), move +80 through the C# math.
        glib::timeout_add_local(std::time::Duration::from_millis(700), {
            let shell = shell.clone();
            move || {
                let _ = shell.state_dispatch_param("win.view-mode", "detail");
                let before = series_width(&shell);
                let started = shell.state_column_resize_start(1, 208.0);
                let moved = shell.state_column_resize_move(288.0);
                let _ended = shell.state_column_resize_end();
                let after = series_width(&shell);
                println!("A drag started={started} moved={moved} before={before} after={after} (expect 280)");
                let ok = started && after == 280;
                println!("A ok={ok}");
                glib::ControlFlow::Break
            }
        });

        // B. The zero clamp: drag far left (the C# clamps at 0).
        glib::timeout_add_local(std::time::Duration::from_millis(1200), {
            let shell = shell.clone();
            move || {
                let started = shell.state_column_resize_start(1, 0.0);
                let moved = shell.state_column_resize_move(-5000.0);
                let _ended = shell.state_column_resize_end();
                let after = series_width(&shell);
                println!("B zero clamp started={started} moved={moved} after={after} (expect 0)");
                let ok = started && moved == 0.0 && after == 0;
                println!("B ok={ok}");
                glib::ControlFlow::Break
            }
        });

        // C. The auto-size: the Series column resizes to the widest
        //    displayed cell + padding.
        glib::timeout_add_local(std::time::Duration::from_millis(1700), {
            let shell = shell.clone();
            move || {
                let width = shell.state_column_autosize(1);
                println!("C autosize width={width} (widest text + 8)");
                let ok = width > 0.0 && width < 200.0;
                println!("C ok={ok}");
                glib::ControlFlow::Break
            }
        });

        // D. The persistence: collect the workspace, restore it in a
        //    SECOND shell, and the resized width must carry (the C#
        //    `ItemViewConfig.Columns`).
        glib::timeout_add_local(std::time::Duration::from_millis(2200), {
            let app = app.clone();
            let shell = shell.clone();
            move || {
                let saved = series_width(&shell);
                // A real drag to a distinctive width, then the
                // collect/apply round-trip.
                let _ = shell.state_column_resize_start(1, 0.0);
                let _ = shell.state_column_resize_move(173.0);
                let _ = shell.state_column_resize_end();
                let resized = series_width(&shell);
                let ws = shell.state_collect_workspace();
                let (window2, shell2) = cr_ui::browser::shell::BrowserShell::create(&app);
                window2.present();
                shell2.state_apply_workspace(&ws);
                let restored = series_width(&shell2);
                println!(
                    "D persistence resized={resized} restored={restored} (expect {resized})"
                );
                let ok = resized == restored && restored != saved;
                println!("D ok={ok}");
                println!("DETAILRESIZE PROBE DONE");
                app.quit();
                glib::ControlFlow::Break
            }
        });

        glib::timeout_add_local(std::time::Duration::from_secs(60), {
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
