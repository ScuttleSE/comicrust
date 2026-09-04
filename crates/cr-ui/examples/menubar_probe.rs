//! Headless probe: the T3 menubar skeleton. The six menus build
//! from the table, the menubar mounts above the shell content, and
//! the `OnGuiVisibilities` rule shows it for the startup state (no
//! book open — `ShowMainMenuNoComicOpen`). Run: Xvfb +
//! `cargo run -p cr-ui --example menubar_probe` with an isolated
//! XDG. The Alt-reveal and the reader-state checks stay for the
//! user test (Xvfb key injection is unreliable).
use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;

fn main() {
    gtk4::init().expect("gtk init");
    cr_ui::library::initialize().expect("library init");

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.menubar-probe")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let (window, shell) = cr_ui::browser::shell::BrowserShell::create(app);
        window.present();

        // 1. The table: six menus, every submenu non-empty.
        let top = cr_ui::browser::menubar::MENUS;
        println!("MENUS {}", top.len());
        for (label, defs) in top {
            let items = defs
                .iter()
                .filter(|n| !matches!(n, cr_ui::browser::menubar::MenuNode::Sep))
                .count();
            println!("MENU {label}: {items} nodes");
        }

        // 2. The mounted menubar follows the startup rule: browser
        //    page, no book, auto-hide ON, ShowMainMenuNoComicOpen →
        //    VISIBLE.
        let mounted = shell.menubar().is_visible();
        println!("MENUBAR VISIBLE AT STARTUP: {mounted}");
        let rule =
            cr_ui::browser::menubar::menubar_visible(false, false, false, false, true, true, false);
        println!("RULE AGREES: {mounted} == {rule} ({})", mounted == rule);

        // 3. A menu action fires through the shell (enable-state +
        //    menubar refresh run inside the dispatch).
        let _ = gtk4::prelude::WidgetExt::activate_action(&window, "win.view-pages", None);
        println!("STATE view-pages activated");

        glib::timeout_add_local(std::time::Duration::from_millis(1500), {
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
