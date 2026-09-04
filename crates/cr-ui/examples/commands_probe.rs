//! Headless: the T1 command layer. Every shell action must resolve
//! on the window, the accelerators must be registered, and the page
//! actions must visibly switch the stack (screenshots come from the
//! driver shell). Run: Xvfb + `cargo run -p cr-ui --example
//! commands_probe` with an isolated XDG (fresh DB).
use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;

fn main() {
    gtk4::init().expect("gtk init");
    cr_ui::library::initialize().expect("library init");

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.probe")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let (window, _shell) = cr_ui::browser::shell::BrowserShell::create(app);
        window.present();

        // 1. The accelerators are registered (spot checks + the
        //    per-value radio names).
        for action in [
            "open-file",
            "close",
            "quit",
            "refresh",
            "toggle-browser",
            "first-page",
            "next-bookmark",
            "rotate-270",
        ] {
            println!(
                "ACCEL win.{action}: {:?}",
                app.accels_for_action(&format!("win.{action}"))
            );
        }
        println!(
            "ACCEL win.page-fit::original: {:?}",
            app.accels_for_action("win.page-fit::original")
        );
        println!(
            "ACCEL win.page-layout::continuous: {:?}",
            app.accels_for_action("win.page-layout::continuous")
        );

        // 2. Every command action resolves on the window. Skip the
        //    side-effecting commands (restart spawns the binary,
        //    quit closes the window) and the two parametered radio
        //    actions (activated explicitly below with targets).
        let total = cr_ui::commands::COMMANDS.len();
        let mut resolved = 0usize;
        for command in cr_ui::commands::COMMANDS {
            if matches!(
                command.action,
                "restart" | "quit" | "page-fit" | "page-layout"
            ) {
                continue;
            }
            let found = gtk4::prelude::WidgetExt::activate_action(
                &window,
                &format!("win.{}", command.action),
                None,
            )
            .is_ok();
            if found {
                resolved += 1;
            } else {
                println!("MISSING ACTION: {}", command.action);
            }
        }
        println!("RESOLVED {resolved}/{} (4 skipped)", total - 4);

        // 3. Radio actions with targets route into the reader
        //    dispatch (no open book — they must not crash).
        let _ = gtk4::prelude::WidgetExt::activate_action(
            &window,
            "win.page-fit",
            Some(&"fit-width".to_variant()),
        );
        let _ = gtk4::prelude::WidgetExt::activate_action(
            &window,
            "win.page-layout",
            Some(&"double".to_variant()),
        );

        // 4. The visible chrome flips (screenshots via the driver).
        //    One chained timeline, then the app quits.
        let _ = gtk4::prelude::WidgetExt::activate_action(&window, "win.view-pages", None);
        println!("STATE view-pages activated");
        glib::timeout_add_local(std::time::Duration::from_millis(1200), {
            let window = window.downgrade();
            move || {
                let Some(window) = window.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                let _ = gtk4::prelude::WidgetExt::activate_action(&window, "win.sidebar", None);
                println!("STATE sidebar toggled off");
                glib::timeout_add_local(std::time::Duration::from_millis(1200), {
                    let window = window.downgrade();
                    move || {
                        let Some(window) = window.upgrade() else {
                            return glib::ControlFlow::Break;
                        };
                        let _ =
                            gtk4::prelude::WidgetExt::activate_action(&window, "win.sidebar", None);
                        println!("STATE sidebar toggled back on");
                        let _ = gtk4::prelude::WidgetExt::activate_action(
                            &window,
                            "win.view-library",
                            None,
                        );
                        println!("STATE view-library activated");
                        glib::ControlFlow::Break
                    }
                });
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_secs(6), {
            let app = app.clone();
            move || {
                println!("PROBE DONE");
                app.quit();
                glib::ControlFlow::Break
            }
        });
    });
    app.run();
}
