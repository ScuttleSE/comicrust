//! Headless probe: the T14 layout persistence (`Settings.
//! CurrentWorkspace` — the `[settings.current_workspace]` tables of
//! the unified config, ADR-033). Gates: the exit snapshot collects
//! the live browser layout (the sidebar visibility + split, view
//! mode, sort key, thumb size, the Detail column set, the window
//! size), comicrust.toml carries the C#-named members, and a SECOND
//! shell (the startup path) restores the layout through
//! `apply_workspace`.
//! Run: Xvfb + `cargo run -p cr-ui --example workspace_probe` with
//! isolated XDG dirs (fresh DB → the default list tree; the probe
//! writes comicrust.toml).
use gtk4::glib;
use gtk4::prelude::*;
use std::cell::Cell;

thread_local! {
    /// The expected thumb size (the default + 5 wheel steps — the
    /// probe avoids hardcoding the LayoutConfig default).
    static EXPECT_THUMB: Cell<f64> = const { Cell::new(0.0) };
}

fn main() {
    gtk4::init().expect("gtk init");
    cr_ui::theme::init();
    // The probe writes comicrust.toml and opens a DB — refuse a real
    // home (the accidental-run lesson).
    let isolated_data = std::env::var("XDG_DATA_HOME")
        .map(|v| v.contains("/tmp/opencode"))
        .unwrap_or(false);
    let isolated_config = std::env::var("XDG_CONFIG_HOME")
        .map(|v| v.contains("/tmp/opencode"))
        .unwrap_or(false);
    if !isolated_data || !isolated_config {
        eprintln!(
            "REFUSED: set XDG_DATA_HOME=/tmp/opencode/<dir> and XDG_CONFIG_HOME=/tmp/opencode/<dir> (the probe seeds a DB and writes comicrust.toml)"
        );
        std::process::exit(1);
    }
    let _ = std::fs::remove_dir_all("/tmp/opencode/workspace");
    std::fs::create_dir_all("/tmp/opencode/workspace").unwrap();
    cr_ui::library::initialize().expect("session init");

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.workspace-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let (window, shell) = cr_ui::browser::shell::BrowserShell::create(app);
        window.present();
        std::mem::forget(shell.clone());

        // A. Mutate the layout (the browser workspace + Detail view
        //    + a sort + hidden sidebar + a moved split + bigger
        //    thumbs + a hidden Detail column).
        glib::timeout_add_local(std::time::Duration::from_millis(800), {
            let shell = shell.clone();
            move || {
                let _ = shell.state_dispatch("win.view-library");
                let thumb0 = shell.state_grid_thumb_height();
                shell.state_set_paned(340);
                // The detailed-name-with-parameter rule: the BARE
                // name + the explicit variant (the T4 lesson).
                shell.state_dispatch_param("win.view-mode", "detail");
                shell.state_dispatch_param("win.sort-column", "Writer");
                shell.state_dispatch("win.sidebar");
                shell.state_dispatch_param("win.toggle-column", "5");
                for _ in 0..5 {
                    shell.state_dispatch("win.thumb-bigger");
                }
                EXPECT_THUMB.with(|t| t.set(thumb0 + 80.0));
                glib::ControlFlow::Break
            }
        });

        // B. The exit snapshot → the settings → comicrust.toml. The
        //    C#-member evidence prints.
        glib::timeout_add_local(std::time::Duration::from_millis(1500), {
            let shell = shell.clone();
            move || {
                let expected = EXPECT_THUMB.with(|t| t.get());
                println!(
                    "A sidebar(visible={:?}) mode={} thumb={} (expect visible=false, detail, {expected})",
                    shell.state_sidebar(),
                    shell.state_grid_mode(),
                    shell.state_grid_thumb_height(),
                );
                let ws = shell.state_collect_workspace();
                assert!(!ws.view.show_browser, "the sidebar state saves");
                assert_eq!(ws.view.mode, cr_core::model::enums::ItemViewMode::Detail);
                assert_eq!(ws.view.sort_key.as_deref(), Some("Writer"));
                assert_eq!(ws.view.browser_split, 340);
                assert_eq!(ws.view.thumb_height, expected as i32);
                assert!(
                    ws.view.columns.iter().any(|c| c.id == 5 && !c.visible),
                    "the hidden column saves"
                );
                assert!(ws.width > 0 && ws.height > 0, "the window size saves");
                cr_ui::library::settings().borrow_mut().current_workspace = Some(ws);
                cr_ui::library::save_settings();
                let file = cr_core::paths::config_file(&cr_core::paths::Paths::new_default());
                let text = std::fs::read_to_string(&file).unwrap();
                println!(
                    "B comicrust.toml carries the workspace: show_browser={} detail={} sort={} split={} (expect true x4)",
                    text.contains("ShowBrowser = false"),
                    text.contains("Mode = \"Detail\""),
                    text.contains("SortKey = \"Writer\""),
                    text.contains("BrowserSplit = 340"),
                );
                assert!(text.contains("[settings.CurrentWorkspace]"));
                assert!(text.contains("[settings.CurrentWorkspace.View]"));
                assert!(text.contains("ShowBrowser = false"));
                assert!(text.contains("Mode = \"Detail\""));
                assert!(text.contains("SortKey = \"Writer\""));
                assert!(text.contains("BrowserSplit = 340"));
                assert!(text.contains("Fit = \"FitWidth\""));
                glib::ControlFlow::Break
            }
        });

        // C. A fresh shell (the startup path): the restore reads the
        //    saved workspace in `create`.
        glib::timeout_add_local(std::time::Duration::from_millis(2100), {
            let app = app.clone();
            move || {
                let (window2, shell2) = cr_ui::browser::shell::BrowserShell::create(&app);
                window2.present();
                let expected = EXPECT_THUMB.with(|t| t.get());
                let (visible, split) = shell2.state_sidebar();
                let (w, h) = window2.default_size();
                println!(
                    "C restored sidebar(visible={visible}, split={split}) mode={} thumb={} size={w}x{h} (expect false, 340, detail, {expected})",
                    shell2.state_grid_mode(),
                    shell2.state_grid_thumb_height(),
                );
                assert!(!visible, "the hidden sidebar restores");
                assert_eq!(split, 340, "the split restores");
                assert_eq!(shell2.state_grid_mode(), "detail", "the view mode restores");
                assert_eq!(shell2.state_grid_thumb_height(), expected, "the thumb size restores");
                assert!(w > 0 && h > 0, "the window size restores");
                let cols = shell2
                    .state_detail_columns()
                    .into_iter()
                    .find(|(id, _, _)| *id == 5)
                    .unwrap();                // The snapshot is (id, name, visible).
                assert!(!cols.2, "the hidden column restores");
                println!("T14 restore gates: PASS");
                // D. The real exit path: closing the window runs the
                //    close-request collect + save (`CleanUp`).
                window2.close();
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(2700), {
            let app = app.clone();
            move || {
                let file = cr_core::paths::config_file(&cr_core::paths::Paths::new_default());
                let text = std::fs::read_to_string(&file).unwrap();
                println!(
                    "D close-path save: detail={} split={} (expect true x2)",
                    text.contains("Mode = \"Detail\""),
                    text.contains("BrowserSplit = 340"),
                );
                assert!(
                    text.contains("Mode = \"Detail\""),
                    "the close-path collect kept the layout"
                );
                assert!(text.contains("BrowserSplit = 340"));
                println!("T14 workspace probe: ALL PASS");
                app.quit();
                glib::ControlFlow::Break
            }
        });
    });

    app.run();
}
