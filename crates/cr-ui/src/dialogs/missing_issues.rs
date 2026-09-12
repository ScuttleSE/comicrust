//! "Fill Missing Issues" — create fileless books for the issues of a
//! series that the library does not hold (Phase 15 T7).
//!
//! The Comic Vine cache skeleton knows every issue of a volume. The
//! difference against the owned issue numbers is the gap. The user
//! ticks the rows to create, and each new book carries the series, the
//! volume, the issue number, and the Comic Vine issue id, so a later
//! scrape resolves it with no search.
//!
//! The lookup can reach the network, so it runs on a worker thread
//! (Rule 9) and reports back over a channel.

use std::sync::mpsc;
use std::time::Duration;

use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{CheckButton, Dialog, Label, ResponseType, ScrolledWindow};

use cr_scrape::cache::freshness::{self, FreshnessPolicy};
use cr_scrape::cache::missing::{missing_issues, MissingIssue};
use cr_scrape::cache::{CvCache, SqliteCache};
use cr_scrape::cv::connection::CvClient;

/// What the worker sends back.
enum Found {
    /// The gap, and the number of issues the volume holds.
    Gap {
        missing: Vec<MissingIssue>,
        total: usize,
    },
    Failed(String),
}

/// What the caller must supply about the selected books.
pub struct Request {
    /// The series name the new books carry.
    pub series: String,
    /// The volume number the new books carry.
    pub volume: i32,
    /// The Comic Vine volume id.
    pub volume_id: i64,
    /// The issue numbers the library already holds.
    pub owned_numbers: Vec<String>,
    /// The API key. An empty key still allows a cache-only lookup.
    pub api_key: String,
}

/// The books the dialog asks the caller to create.
pub type CreateFn = Box<dyn Fn(&[MissingIssue])>;

/// Opens the dialog. `on_create` runs once with the ticked rows.
pub fn show(parent: &impl IsA<gtk4::Window>, request: Request, on_create: CreateFn) {
    let dialog = Dialog::builder()
        .title("Fill Missing Issues")
        .transient_for(parent)
        .modal(true)
        .default_width(520)
        .default_height(460)
        .build();
    let content = dialog.content_area();
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    content.set_margin_start(8);
    content.set_margin_end(8);
    content.set_spacing(6);

    let heading = Label::new(Some(&format!(
        "{} — Comic Vine volume {}",
        request.series, request.volume_id
    )));
    heading.set_halign(gtk4::Align::Start);
    heading.add_css_class("heading");
    content.append(&heading);

    let status = Label::new(Some("Reading the issue list…"));
    status.set_halign(gtk4::Align::Start);
    status.set_wrap(true);
    content.append(&status);

    let list = gtk4::ListBox::new();
    list.set_selection_mode(gtk4::SelectionMode::None);
    let scroller = ScrolledWindow::builder()
        .child(&list)
        .vexpand(true)
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .build();
    content.append(&scroller);

    let toggles = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    let check_all = gtk4::Button::with_label("Check All");
    let uncheck_all = gtk4::Button::with_label("Uncheck All");
    check_all.set_sensitive(false);
    uncheck_all.set_sensitive(false);
    toggles.append(&check_all);
    toggles.append(&uncheck_all);
    content.append(&toggles);

    dialog.add_button("Create Books", ResponseType::Ok);
    dialog.add_button("Cancel", ResponseType::Cancel);
    let create_btn = dialog.widget_for_response(ResponseType::Ok);
    if let Some(b) = &create_btn {
        b.set_sensitive(false);
    }
    dialog.set_default_response(ResponseType::Ok);

    // The rows, in the order the list shows them. They are main
    // thread only: a `CheckButton` is neither `Send` nor `Sync`.
    let rows: std::rc::Rc<std::cell::RefCell<Vec<(MissingIssue, CheckButton)>>> =
        std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));

    let (tx, rx) = mpsc::channel::<Found>();
    let policy = FreshnessPolicy::default();
    let volume_id = request.volume_id;
    let owned = request.owned_numbers.clone();
    let api_key = request.api_key.clone();
    std::thread::Builder::new()
        .name("Missing Issues".into())
        .spawn(move || {
            let _ = tx.send(look_up(volume_id, &owned, &api_key, &policy));
        })
        .expect("spawn the missing-issue worker");

    let rows_pump = std::rc::Rc::clone(&rows);
    let list_pump = list.clone();
    let status_pump = status.clone();
    let check_all_pump = check_all.clone();
    let uncheck_all_pump = uncheck_all.clone();
    glib::timeout_add_local(Duration::from_millis(50), move || {
        let Ok(found) = rx.try_recv() else {
            return glib::ControlFlow::Continue;
        };
        match found {
            Found::Failed(reason) => {
                status_pump.set_text(&format!("The issue list could not be read: {reason}"));
            }
            Found::Gap { missing, total } => {
                if missing.is_empty() {
                    status_pump.set_text(&format!(
                        "The library holds every one of the {total} issues of this volume."
                    ));
                } else {
                    status_pump.set_text(&format!(
                        "{} of the {total} issues are missing. Tick the ones to create.",
                        missing.len()
                    ));
                }
                let mut store = rows_pump.borrow_mut();
                for issue in missing {
                    let check = CheckButton::with_label(&row_text(&issue));
                    check.set_active(true);
                    list_pump.append(&check);
                    store.push((issue, check));
                }
                let any = !store.is_empty();
                drop(store);
                check_all_pump.set_sensitive(any);
                uncheck_all_pump.set_sensitive(any);
                if let Some(b) = &create_btn {
                    b.set_sensitive(any);
                }
            }
        }
        glib::ControlFlow::Break
    });

    for (button, active) in [(&check_all, true), (&uncheck_all, false)] {
        let rows = std::rc::Rc::clone(&rows);
        button.connect_clicked(move |_| {
            for (_, check) in rows.borrow().iter() {
                check.set_active(active);
            }
        });
    }

    // One-shot close guard (the re-entrant response lesson).
    let done = std::rc::Rc::new(std::cell::Cell::new(false));
    dialog.connect_response(move |dlg, response| {
        if done.replace(true) {
            return;
        }
        let ok = response == ResponseType::Ok;
        let picked: Vec<MissingIssue> = rows
            .borrow()
            .iter()
            .filter(|(_, check)| check.is_active())
            .map(|(issue, _)| issue.clone())
            .collect();
        dlg.close();
        if ok && !picked.is_empty() {
            on_create(&picked);
        }
    });
    dialog.present();
}

/// The row caption: the issue number, its title, and its cover year.
fn row_text(issue: &MissingIssue) -> String {
    let number = if issue.issue_number.trim().is_empty() {
        "(no number)".to_string()
    } else {
        format!("#{}", issue.issue_number)
    };
    let year = issue
        .cover_date
        .as_deref()
        .and_then(|d| d.get(..4))
        .filter(|y| y.chars().all(|c| c.is_ascii_digit()))
        .map(|y| format!(" ({y})"))
        .unwrap_or_default();
    match issue.name.as_deref().filter(|n| !n.trim().is_empty()) {
        Some(name) => format!("{number}{year} — {name}"),
        None => format!("{number}{year}"),
    }
}

/// The worker body. It opens its own cache handle, because the cache
/// is a file and the worker must not share the main thread's state.
fn look_up(volume_id: i64, owned: &[String], api_key: &str, policy: &FreshnessPolicy) -> Found {
    let path = cr_scrape::cache::default_cache_path();
    let cache = match SqliteCache::open(&path) {
        Ok(c) => c,
        Err(e) => return Found::Failed(e.to_string()),
    };

    // With no API key the cache is all there is. That still works
    // after an MCL import.
    let issues = if api_key.trim().is_empty() {
        match cache.issues_of_volume(volume_id) {
            Ok(issues) => issues,
            Err(e) => return Found::Failed(e.to_string()),
        }
    } else {
        let client = CvClient::new(api_key);
        match freshness::issues_of_volume(
            &client,
            &cache,
            volume_id,
            policy,
            chrono::Utc::now().timestamp(),
        ) {
            Ok((issues, _)) => issues,
            Err(e) => return Found::Failed(e.to_string()),
        }
    };

    if issues.is_empty() {
        return Found::Failed(format!(
            "Comic Vine volume {volume_id} lists no issue. Import an MCL file, or scrape one book of this series first."
        ));
    }
    let total = issues.len();
    Found::Gap {
        missing: missing_issues(&issues, owned),
        total,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn issue(number: &str, year: Option<&str>, name: Option<&str>) -> MissingIssue {
        MissingIssue {
            issue_id: 1,
            issue_number: number.to_string(),
            cover_date: year.map(str::to_string),
            name: name.map(str::to_string),
        }
    }

    #[test]
    fn a_row_shows_the_number_the_year_and_the_title() {
        assert_eq!(
            row_text(&issue("3", Some("2013-06-01"), Some("Amarillo"))),
            "#3 (2013) — Amarillo"
        );
    }

    #[test]
    fn a_row_drops_what_it_does_not_know() {
        assert_eq!(row_text(&issue("3", None, None)), "#3");
        assert_eq!(row_text(&issue("3", Some("bad"), None)), "#3");
        assert_eq!(row_text(&issue("3", None, Some("  "))), "#3");
        assert_eq!(row_text(&issue("", None, None)), "(no number)");
    }
}
