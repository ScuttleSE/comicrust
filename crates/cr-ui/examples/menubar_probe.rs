//! Headless probe: the T3 menubar. The six menus build from the
//! table, the bar mounts and follows the visibility rule, real row
//! clicks fire actions (the round-2 gate), the active-panel
//! highlight moves (the C# highlights the Library/Pages row instead
//! of a check mark), and top-menu switching stays one-grab-safe.
//! Run: Xvfb + `cargo run -p cr-ui --example menubar_probe` with an
//! isolated XDG.
use gtk4::gio;
use gtk4::glib;
use gtk4::prelude::*;
use std::collections::VecDeque;

/// Opens one top menu per step, waits for the popover to map, and
/// prints the sync (dynamic fill + popup) and map halves of the open
/// cost. Runs the remaining steps 30 ms apart; each open starts from
/// an all-closed bar (the user clicks one menu at a time). When the
/// steps run out the probe completes.
fn open_steps(
    menubar: cr_ui::browser::menubar::MenubarWidget,
    steps: VecDeque<(&'static str, usize, &'static str)>,
    app: gtk4::Application,
) {
    let mut steps = Some(steps);
    let Some((name, index, pass)) = steps.as_mut().unwrap().pop_front() else {
        println!("PROBE COMPLETE");
        app.quit();
        return;
    };
    for p in menubar.top_popovers() {
        p.popdown();
    }
    // The open waits 100 ms: the user's click-away closes the old
    // menu well before the click on the next one. A shorter gap did
    // not help — the first File open closed itself 2-3 ms after
    // mapping even from a separate idle, so the trigger window is
    // longer than one iteration.
    let open_menubar = menubar.clone_handle();
    glib::timeout_add_local_once(std::time::Duration::from_millis(100), move || {
        let t0 = std::time::Instant::now();
        open_menubar.open_top(index);
        let sync = t0.elapsed();
        let poll = open_menubar.clone_handle();
        let deadline = t0 + std::time::Duration::from_secs(2);
        glib::timeout_add_local(std::time::Duration::from_millis(2), move || {
            let mapped = poll.top_popovers()[index].is_mapped();
            if !mapped && std::time::Instant::now() < deadline {
                return glib::ControlFlow::Continue;
            }
            println!(
                "OPEN top={index} {name} {pass} sync {:.2}ms map {:.2}ms mapped={mapped}",
                sync.as_secs_f64() * 1000.0,
                t0.elapsed().as_secs_f64() * 1000.0,
            );
            let next = poll.clone_handle();
            let steps = steps.take().expect("the remaining open steps");
            let app = app.clone();
            glib::timeout_add_local_once(std::time::Duration::from_millis(30), move || {
                open_steps(next, steps, app);
            });
            glib::ControlFlow::Break
        });
    });
}

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
        // The app keeps its shell in a thread-local (app.rs); the
        // probe must do the same — every action handler holds a
        // Weak<ShellState>, and a dropped shell turns each dispatch
        // into a silent no-op (the probe lesson).
        std::mem::forget(shell.clone());

        // 1. The table: six menus.
        println!("MENUS {}", cr_ui::browser::menubar::MENUS.len());

        // 2. The mounted menubar follows the startup rule.
        let mounted = shell.menubar().widget().is_visible();
        let rule =
            cr_ui::browser::menubar::menubar_visible(false, false, false, false, true, true, false);
        println!(
            "MENUBAR VISIBLE {mounted} RULE {rule} AGREE {}",
            mounted == rule
        );

        // 2b. The T1 gates: no `&` reaches any rendered label (the
        //     WinForms mnemonics strip at render) and the top-menu
        //     popovers carry no pointing arrow (the C# MenuStrip
        //     drop shape).
        {
            let menubar = shell.menubar();
            let amps: Vec<String> = menubar
                .all_row_labels()
                .into_iter()
                .filter(|l| l.contains('&'))
                .collect();
            let arrows: Vec<bool> = menubar
                .top_popovers()
                .iter()
                .map(|p| p.has_arrow())
                .collect();
            println!("AMP labels-with-amp {} (expect 0)", amps.len());
            println!("ARROW top-popovers {arrows:?} (expect all false)");
        }

        // 3. The proofs at 1200 ms: direct activation + a REAL row
        //    click for the stateful check, then the panel highlight
        //    via a row click on view-library.
        glib::timeout_add_local(std::time::Duration::from_millis(1200), {
            let menubar = shell.menubar().clone_handle();
            let window = window.clone();
            move || {
                let read_state = || cr_ui::library::settings().borrow().track_current_page;
                // The stateful check action (the round-2 gate).
                let before = read_state();
                let fired = gtk4::prelude::WidgetExt::activate_action(
                    &window,
                    "win.track-current-page",
                    None,
                );
                let direct = read_state();
                println!("TRACK direct fired={fired:?} {before:?}->{direct:?}");
                menubar.click_row("win.track-current-page");
                let after = read_state();
                println!(
                    "TRACK click {direct:?}->{after:?} flipped {}",
                    direct != after
                );

                // The stateless panel actions + the highlight (the
                // round-3 gate). The panel starts on "library", so
                // click view-pages and expect ONLY pages highlighted.
                menubar.click_row("win.view-pages");
                let lib = menubar.is_row_highlighted("win.view-library");
                let pages = menubar.is_row_highlighted("win.view-pages");
                println!("HIGHLIGHT view-library={lib} view-pages={pages}");

                // The dark-mode toggle: a REAL row click flips the
                // global AND the GTK dark preference (the check
                // derives from the global, the whole shell re-styles).
                let prefer = || {
                    gtk4::Settings::default()
                        .map(|s| s.is_gtk_application_prefer_dark_theme())
                        .unwrap_or(false)
                };
                let dark_before = prefer();
                menubar.click_row("win.dark-mode");
                let dark_after = prefer();
                println!(
                    "DARK {dark_before}->{dark_after} flipped {}",
                    dark_before != dark_after
                );
                glib::ControlFlow::Break
            }
        });

        // 4. The switching path: switch top menus while one is open,
        //    then close (the Wayland-grab regression class).
        glib::timeout_add_local(std::time::Duration::from_millis(1500), {
            let menubar = shell.menubar().clone_handle();
            move || {
                println!("SWITCH to Edit");
                menubar.open_top(1);
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(1900), {
            let menubar = shell.menubar().clone_handle();
            move || {
                println!("SWITCH to Help");
                menubar.open_top(5);
                // The nested fills rebuild at the SUBMENU's open (the
                // C# DropDownOpening wiring) — refresh_dyn_slot is the
                // exact call the child popover map runs. Re-run the
                // no-`&` gate over the filled rows.
                menubar.refresh_dyn_slot("page-type");
                menubar.refresh_dyn_slot("page-rotation");
                menubar.refresh_dyn_slot("bookmarks");
                let amps: Vec<String> = menubar
                    .all_row_labels()
                    .into_iter()
                    .filter(|l| l.contains('&'))
                    .collect();
                println!("AMP-FILLED labels-with-amp {} (expect 0)", amps.len());
                glib::ControlFlow::Break
            }
        });

        // 5. The open-cost gates: the top menus open twice each,
        //    from an all-closed bar, 30 ms apart; the probe prints
        //    the sync (dynamic fill + popup) and map halves. File
        //    carries the two dynamic slots; Edit/Browse/Display
        //    carry none.
        glib::timeout_add_local_once(std::time::Duration::from_millis(2400), {
            let menubar = shell.menubar().clone_handle();
            let app = app.clone();
            move || {
                open_steps(
                    menubar.clone(),
                    [
                        ("File", 0, "p1"),
                        ("Edit", 1, "p1"),
                        ("Browse", 2, "p1"),
                        ("Display", 4, "p1"),
                        ("File", 0, "p2"),
                        ("Edit", 1, "p2"),
                    ]
                    .into_iter()
                    .collect(),
                    app,
                );
            }
        });
    });
    app.run();
}
