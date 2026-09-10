//! Headless probe: the Comic Vine Scraper configuration dialog (T6).
//! Gates:
//!   A. the dialog opens with the API key prefilled and 27 of the 32
//!      checkboxes active (the default-off ignore_blanks/
//!      autochoose_series/confirm_issue/updateTags plus the test's
//!      update_writer off),
//!   B. Uncheck All / Check All flip the scrape-field checkboxes,
//!   C. OK fires the callback once with the edited values,
//!   D. Cancel discards (no second callback).
//!
//! Run: Xvfb + `cargo run -p cr-ui --example scrapeconfig_probe` with
//! an isolated XDG pair.

use gtk4::prelude::*;
use gtk4::{CheckButton, Dialog};

use cr_scrape::config::Configuration;

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

fn find_button(window: &gtk4::Window, label: &str) -> gtk4::Button {
    widgets_of(window)
        .into_iter()
        .filter_map(|w| w.downcast::<gtk4::Button>().ok())
        .find(|b| b.label().as_deref() == Some(label))
        .expect("the button with the label")
}

fn find_checks(window: &gtk4::Window) -> Vec<CheckButton> {
    widgets_of(window)
        .into_iter()
        .filter_map(|w| w.downcast::<CheckButton>().ok())
        .collect()
}

fn find_entry(window: &gtk4::Window) -> gtk4::Entry {
    widgets_of(window)
        .into_iter()
        .find_map(|w| w.downcast::<gtk4::Entry>().ok())
        .expect("the API key entry")
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

    let host = gtk4::Window::new();
    host.set_title(Some("scrape-config-probe-host"));
    host.present();

    let committed: std::rc::Rc<std::cell::RefCell<Vec<Configuration>>> =
        std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));

    // Gate A: the dialog opens with the API key and flags prefilled.
    let mut current = Configuration::default();
    current.api_key = "probe-key-123".into();
    current.update_writer = false;
    {
        let committed = std::rc::Rc::clone(&committed);
        cr_ui::dialogs::scrape_config::show_scrape_config(&host, &current, move |result| {
            if let Some(config) = result {
                committed.borrow_mut().push(config);
            }
        });
    }
    let Some(window) = find_toplevel("Comic Vine Scraper Settings") else {
        eprintln!("FAIL: the config dialog did not open");
        std::process::exit(1);
    };
    let entry = find_entry(&window);
    if entry.text() != "probe-key-123" {
        eprintln!("FAIL: the API key is not prefilled: {:?}", entry.text());
        std::process::exit(1);
    }
    let checks = find_checks(&window);
    if checks.len() != 32 {
        eprintln!("FAIL: expected 32 checkboxes, got {}", checks.len());
        std::process::exit(1);
    }
    let active = checks.iter().filter(|c| c.is_active()).count();
    if active != 27 {
        eprintln!("FAIL: expected 27 active flags, got {active}");
        std::process::exit(1);
    }
    println!("GATE A OK: the dialog opens with the API key and flags prefilled");

    // Gate B: Uncheck All / Check All flip the SCRAPE-field checks
    // (the first 21; the behavior flags are untouched by the buttons).
    find_button(&window, "Uncheck All").emit_clicked();
    if checks.iter().take(21).any(|c| c.is_active()) {
        eprintln!("FAIL: Uncheck All left scrape checkboxes active");
        std::process::exit(1);
    }
    find_button(&window, "Check All").emit_clicked();
    if !checks.iter().take(21).all(|c| c.is_active()) {
        eprintln!("FAIL: Check All left scrape checkboxes inactive");
        std::process::exit(1);
    }
    println!("GATE B OK: Check/Uncheck All flip the scrape-field checkboxes");

    // Gate C: edit the API key + two flags, then OK commits.
    entry.set_text("new-key-456");
    // the behavior block starts after the 21 scrape flags;
    // checks[21] is "Overwrite Existing" -> off
    checks[21].set_active(false);
    // checks[7] is the "Writer" scrape flag (Check All re-enabled it
    // in Gate B) -> back off, matching the settings the probe seeded
    checks[7].set_active(false);
    window
        .clone()
        .downcast::<Dialog>()
        .expect("the config dialog is a Dialog")
        .response(gtk4::ResponseType::Ok);
    if committed.borrow().len() != 1 {
        eprintln!("FAIL: OK did not fire the callback exactly once");
        std::process::exit(1);
    }
    let first = committed.borrow()[0].clone();
    if first.api_key != "new-key-456" || first.overwrite_existing || first.update_writer {
        eprintln!("FAIL: the committed settings lost the edits");
        std::process::exit(1);
    }
    // the other scrape flags survived the round trip
    if !first.update_series || !first.update_title {
        eprintln!("FAIL: the committed settings lost untouched scrape flags");
        std::process::exit(1);
    }
    println!("GATE C OK: OK commits the edited settings through the callback");

    // Gate D: Cancel discards (no second commit).
    let mut current = Configuration::default();
    current.api_key = "second-key".into();
    {
        let committed = std::rc::Rc::clone(&committed);
        cr_ui::dialogs::scrape_config::show_scrape_config(&host, &current, move |result| {
            if result.is_some() {
                committed.borrow_mut().push(Configuration::default());
            }
        });
    }
    let Some(second) = find_toplevel("Comic Vine Scraper Settings") else {
        eprintln!("FAIL: the second dialog did not open");
        std::process::exit(1);
    };
    second
        .clone()
        .downcast::<Dialog>()
        .expect("the config dialog is a Dialog")
        .response(gtk4::ResponseType::Cancel);
    if committed.borrow().len() != 1 {
        eprintln!("FAIL: Cancel must not commit");
        std::process::exit(1);
    }
    println!("GATE D OK: Cancel discards without committing");
    std::process::exit(0);
}
