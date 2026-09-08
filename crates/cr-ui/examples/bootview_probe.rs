//! Headless probe: the Phase 8 T2 default view. Gates: the boot
//! lands on the BROWSER workspace with the Library list selected
//! (the C# `OpenCount == 0 && !ShowQuickOpen` shape,
//! MainForm.cs:3140), opening a comic still swaps to the reader,
//! and closing the LAST tab returns to the LAST BROWSER tab
//! (`ShowLast` — the user report 2026-09-08: a comic opened from
//! the Folders view must land back on Folders), while the `+`
//! empty slot shows the QuickOpen covers (the C# empty-reader
//! overlay). Run: Xvfb + `cargo run -p cr-ui --example
//! bootview_probe` with an isolated XDG.
use gtk4::glib;
use gtk4::prelude::*;

fn main() {
    gtk4::init().expect("gtk init");
    cr_ui::theme::init();
    if !std::env::var("XDG_DATA_HOME")
        .map(|v| v.contains("/tmp/opencode"))
        .unwrap_or(false)
    {
        eprintln!("REFUSED: set XDG_DATA_HOME=/tmp/opencode/<dir>");
        std::process::exit(1);
    }
    cr_ui::library::initialize().expect("session init");

    // Seed library books (fileless) so the QuickOpen "Recently
    // Added" group is non-empty for the last-tab-close gate.
    for n in 0..3 {
        let mut book = cr_ui::dialogs::new_book_series::new_fileless_book();
        book.info.series = format!("Boot Probe {n}");
        cr_ui::library::insert_new_book(&book);
    }

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.bootview-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let (window, shell) = cr_ui::browser::shell::BrowserShell::create(app);
        window.present();
        std::mem::forget(shell.clone());

        // A. The boot shape (wire ran synchronously inside create).
        let page = shell.state_stack_page();
        let selected = shell
            .navigator()
            .current_selection()
            .map(|(_, name)| name)
            .unwrap_or_default();
        println!("BOOT page={page} selected={selected:?} (expect browser/Library)");

        // B. Open a comic: the reader swaps in (the open path is
        //    untouched by the boot change).
        glib::timeout_add_local(std::time::Duration::from_millis(800), {
            let shell = shell.clone();
            move || {
                let work = std::path::Path::new("/tmp/opencode/bootview");
                let _ = std::fs::remove_dir_all(work);
                std::fs::create_dir_all(work).unwrap();
                let src = std::path::Path::new(
                    "tests/testfiles/Absolute Flash (2025) Volume 01 Issue 009.cbz",
                );
                let comic = work.join("probe.cbz");
                std::fs::copy(src, &comic).unwrap();
                shell.open_comic(&comic);
                println!("OPEN page={:?} (expect reader)", shell.state_stack_page());
                glib::ControlFlow::Break
            }
        });

        // C. Close the tab: the LAST close returns to the LAST
        //    browser tab (`ShowLast` — here the Library, the only
        //    browser visited).
        glib::timeout_add_local(std::time::Duration::from_millis(3000), {
            let shell = shell.clone();
            move || {
                let fired = shell.state_dispatch("win.close");
                println!(
                    "CLOSE fired={fired} page={:?} (expect browser)",
                    shell.state_stack_page()
                );
                glib::ControlFlow::Break
            }
        });

        // D. QuickOpen lives at the `+` empty slot (the C#
        //    empty-reader overlay): click `+` → the covers (the DB
        //    has the seeded fileless books for the Recently Added
        //    group); toggle-browser leaves again.
        glib::timeout_add_local(std::time::Duration::from_millis(3600), {
            let shell = shell.clone();
            move || {
                shell
                    .tab_strip_handle()
                    .click(&cr_ui::browser::tabstrip::TabId::Plus);
                println!(
                    "PLUS page={:?} (expect quickopen)",
                    shell.state_stack_page()
                );
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(4200), {
            let app = app.clone();
            let shell = shell.clone();
            move || {
                let fired = shell.state_dispatch("win.toggle-browser");
                println!(
                    "BACK fired={fired} page={:?} (expect browser)",
                    shell.state_stack_page()
                );
                println!("BOOTVIEW PROBE DONE");
                app.quit();
                glib::ControlFlow::Break
            }
        });

        glib::timeout_add_local(std::time::Duration::from_secs(30), || {
            eprintln!("TIMEOUT — probe did not finish");
            std::process::exit(2);
        });
    });
    app.run();
}
