//! Headless probe: Copy Page / Export Page (Phase 6 T6, ADR-027).
//! Gates: the reader-gated enable-state (no book → disabled, open →
//! enabled, the C# `ComicDisplay.Book != null` rule), the composed
//! page image (`create_page_image` — natural size), and the
//! "Save Page as" native chooser (open + initial name + cancel).
//! Run: Xvfb + `cargo run -p cr-ui --example exportpage_probe` with
//! an isolated XDG. The comic comes from tests/testfiles (copied to
//! /tmp — never seeded into the DB).
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

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.exportpage-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let (window, shell) = cr_ui::browser::shell::BrowserShell::create(app);
        window.present();
        std::mem::forget(shell.clone());

        // A. No book: the reader-gated actions sit disabled.
        glib::timeout_add_local(std::time::Duration::from_millis(800), {
            let shell = shell.clone();
            move || {
                let copy = shell.state_action_enabled("copy-page");
                let export = shell.state_action_enabled("export-page");
                println!("ENABLED no-book copy={copy} export={export} (expect false/false)");
                glib::ControlFlow::Break
            }
        });

        // B. Open a comic; the actions enable and the page image
        //    composes at natural size.
        glib::timeout_add_local(std::time::Duration::from_millis(1000), {
            let shell = shell.clone();
            move || {
                let work = std::path::Path::new("/tmp/opencode/exportpage");
                let _ = std::fs::remove_dir_all(work);
                std::fs::create_dir_all(work).unwrap();
                let src = std::path::Path::new(
                    "tests/testfiles/Absolute Flash (2025) Volume 01 Issue 009.cbz",
                );
                let comic = work.join("probe.cbz");
                std::fs::copy(src, &comic).unwrap();
                shell.open_comic(&comic);
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(7000), {
            let shell = shell.clone();
            move || {
                let copy = shell.state_action_enabled("copy-page");
                let export = shell.state_action_enabled("export-page");
                println!("ENABLED open copy={copy} export={export} (expect true/true)");
                let size = shell.state_page_image_size();
                println!("PAGE IMAGE size={size:?} (expect Some with both > 0)");
                glib::ControlFlow::Break
            }
        });

        // C. The export dialog: open, the initial name, cancel.
        glib::timeout_add_local(std::time::Duration::from_millis(7500), {
            let window = window.clone();
            move || {
                let _ = gtk4::prelude::WidgetExt::activate_action(&window, "win.export-page", None);
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(8200), {
            move || {
                // The native chooser's internal dialog carries no
                // window title here — find it by type.
                let chooser = gtk4::Window::list_toplevels()
                    .into_iter()
                    .find_map(|w| w.downcast::<gtk4::FileChooserDialog>().ok());
                match chooser {
                    Some(dlg) => {
                        let name = dlg.current_name().unwrap_or_default();
                        println!("EXPORT OPEN name={name:?} (expect \"probe - Page 1.jpg\")");
                        dlg.response(gtk4::ResponseType::Cancel);
                    }
                    None => println!("EXPORT OPEN MISSING"),
                }
                glib::ControlFlow::Break
            }
        });

        glib::timeout_add_local(std::time::Duration::from_millis(8800), {
            let shell = shell.clone();
            move || {
                println!(
                    "STACK after cancel {} (expect reader — cancel saves nothing)",
                    shell.state_stack_page()
                );
                println!("EXPORTPAGE PROBE DONE");
                glib::ControlFlow::Break
            }
        });

        glib::timeout_add_local(std::time::Duration::from_secs(25), || {
            eprintln!("TIMEOUT — probe did not finish");
            std::process::exit(2);
        });
    });

    app.run();
}
