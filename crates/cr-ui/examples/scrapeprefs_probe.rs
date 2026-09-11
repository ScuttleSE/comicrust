//! Headless probe: the Comic Vine Scraper page of the Preferences
//! dialog. Gates:
//!   A. `show_preferences(None)` opens on the Reader page (the
//!      default child).
//!   B. `show_preferences(Some("scraper"))` opens on the Comic Vine
//!      Scraper page (the no-API-key flow).
//!   C. The scraper page edits (the API key + a flag) commit into the
//!      plugin settings.json on OK.
//!
//! Run: Xvfb + `cargo run -p cr-ui --example scrapeprefs_probe` with
//! an isolated XDG pair.

use gtk4::prelude::*;
use gtk4::{Dialog, Stack};

fn find_toplevel(title: &str) -> Option<gtk4::Window> {
    gtk4::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk4::Window>().ok())
        .find(|w| w.title().as_deref().is_some_and(|t| t.starts_with(title)))
}

fn walk(widget: &gtk4::Widget, out: &mut Vec<gtk4::Widget>) {
    out.push(widget.clone());
    let mut child = widget.first_child();
    while let Some(c) = child {
        walk(&c, out);
        child = c.next_sibling();
    }
}

fn widgets_of(window: &gtk4::Window) -> Vec<gtk4::Widget> {
    let mut widgets = Vec::new();
    if let Some(child) = window.child() {
        walk(&child, &mut widgets);
    }
    widgets
}

fn find_stack(window: &gtk4::Window) -> Stack {
    widgets_of(window)
        .into_iter()
        .find_map(|w| w.downcast::<Stack>().ok())
        .expect("the Preferences stack")
}

fn find_entry(window: &gtk4::Window) -> gtk4::Entry {
    widgets_of(window)
        .into_iter()
        .find_map(|w| w.downcast::<gtk4::Entry>().ok())
        .expect("the API key entry")
}

fn find_check(window: &gtk4::Window, label: &str) -> gtk4::CheckButton {
    widgets_of(window)
        .into_iter()
        .filter_map(|w| w.downcast::<gtk4::CheckButton>().ok())
        .find(|c| c.label().map(|l| l.as_str() == label).unwrap_or(false))
        .expect("the flag checkbox")
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
        eprintln!("REFUSED: set XDG_DATA_HOME and XDG_CONFIG_HOME to /tmp/opencode/<dir> (an isolated pair)");
        std::process::exit(1);
    }
    cr_ui::library::initialize().expect("session");

    let host = gtk4::Window::new();
    host.set_title(Some("scrapeprefs-probe-host"));
    host.present();

    // Gate A: the default page is the Reader page.
    cr_ui::settings::show_preferences(&host, None, || {});
    let Some(dialog) = find_toplevel("Preferences") else {
        eprintln!("FAIL: the Preferences dialog did not open");
        std::process::exit(1);
    };
    let stack = find_stack(&dialog);
    if stack.visible_child_name().map(|n| n.to_string()).as_deref() != Some("reader") {
        eprintln!(
            "FAIL: the default page is {:?} (expect reader)",
            stack.visible_child_name().map(|n| n.to_string())
        );
        std::process::exit(1);
    }
    println!("GATE A OK: the Preferences dialog opens on the Reader page");
    dialog.clone().downcast::<Dialog>().unwrap().close();

    // Gate B: the no-API-key flow opens on the scraper page.
    cr_ui::settings::show_preferences(&host, Some("scraper"), || {});
    let Some(dialog) = find_toplevel("Preferences") else {
        eprintln!("FAIL: the second Preferences dialog did not open");
        std::process::exit(1);
    };
    let stack = find_stack(&dialog);
    if stack.visible_child_name().map(|n| n.to_string()).as_deref() != Some("scraper") {
        eprintln!(
            "FAIL: the scraper page did not open (visible {:?})",
            stack.visible_child_name().map(|n| n.to_string())
        );
        std::process::exit(1);
    }
    println!("GATE B OK: the scraper page opens on request");

    // Gate C: the edits commit into the plugin settings.json on OK.
    let entry = find_entry(&dialog);
    entry.set_text("prefs-key-789");
    let series = find_check(&dialog, "Series");
    series.set_active(false);
    dialog
        .clone()
        .downcast::<Dialog>()
        .expect("the Preferences dialog is a Dialog")
        .response(gtk4::ResponseType::Ok);
    let config = cr_ui::library::scraper_config();
    if config.api_key != "prefs-key-789" {
        eprintln!("FAIL: the API key did not commit: {:?}", config.api_key);
        std::process::exit(1);
    }
    if config.update_series {
        eprintln!("FAIL: the Series flag did not commit");
        std::process::exit(1);
    }
    println!("GATE C OK: OK commits the scraper settings into the unified config");
    std::process::exit(0);
}
