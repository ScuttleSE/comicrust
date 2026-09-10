//! The Comic Vine Scraper wizard — the C# `comicform.py` +
//! `scrapeengine.py` UI shape: a NON-MODAL status window listing the
//! books and their states, with the engine running on a worker
//! thread. The engine's `ScrapeUi` requests (search terms, series
//! pick, issue pick) arrive over a std mpsc channel and are answered
//! by modal dialogs presented in the main-loop pump (Rule 6: the
//! session is thread-local, GTK widgets stay main-thread). Scraped
//! books are committed per book through `library::apply_edited`
//! (the C# writes through the queue as books complete).
//!
//! Cancellation: the window's Cancel button (and close) raises the
//! shared stop flag; the engine exits at its next check and the run
//! ends with the summary.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gtk4::glib::ControlFlow;
use gtk4::prelude::*;
use gtk4::{glib, Dialog, Label, ListBoxRow};

use cr_core::model::comic_book::ComicBook;
use cr_scrape::config::Configuration;
use cr_scrape::cv::connection::CvClient;
use cr_scrape::cv::models::{IssueRef, SeriesRef};
use cr_scrape::cv::queries::Cv;
use cr_scrape::engine::{
    BookStatus, IssueResult, ProgressKind, ScrapeEngine, ScrapeUi, SeriesResult,
};

/// The issue-dialog payload (boxed: the largest UiRequest variant).
struct PickIssueRequest {
    caption: String,
    series: SeriesRef,
    issues: Vec<IssueRef>,
    force: bool,
}

/// The per-book status map (caption -> state).
type BookStates = Vec<(String, Option<BookStatus>)>;

/// What the worker sends the main thread (one variant per ScrapeUi
/// event).
enum UiRequest {
    SearchTerms {
        caption: String,
        failed: String,
    },
    PickSeries {
        caption: String,
        terms: String,
        refs: Vec<SeriesRef>,
    },
    /// Boxed: the largest variant by far.
    PickIssue(Box<PickIssueRequest>),
    NoIssues {
        series: String,
    },
    Started {
        caption: String,
        remaining: usize,
    },
    Finished {
        caption: String,
        status: BookStatus,
    },
    /// Boxed: ComicBook is large.
    Scraped(Box<ComicBook>),
    Progress {
        text: String,
    },
    Done {
        scraped: usize,
        skipped: usize,
        chosen: Vec<String>,
    },
}

/// The UI's answer to a blocking request.
enum UiAnswer {
    Terms(Option<String>),
    Series(SeriesResult),
    Issue(IssueResult),
}

/// The worker-side ScrapeUi: every call ships a request to the main
/// thread and blocks for the answer (the C# modal-dialog semantics,
/// inverted for GTK threading).
struct ChannelUi {
    tx: std::sync::mpsc::Sender<UiRequest>,
    answer_rx: std::sync::mpsc::Receiver<UiAnswer>,
}

impl ScrapeUi for ChannelUi {
    fn request_search_terms(&mut self, caption: &str, failed: &str) -> Option<String> {
        let _ = self.tx.send(UiRequest::SearchTerms {
            caption: caption.to_string(),
            failed: failed.to_string(),
        });
        match self.answer_rx.recv() {
            Ok(UiAnswer::Terms(t)) => t,
            _ => None,
        }
    }

    fn request_series(&mut self, caption: &str, terms: &str, refs: &[SeriesRef]) -> SeriesResult {
        let _ = self.tx.send(UiRequest::PickSeries {
            caption: caption.to_string(),
            terms: terms.to_string(),
            refs: refs.to_vec(),
        });
        match self.answer_rx.recv() {
            Ok(UiAnswer::Series(r)) => r,
            _ => SeriesResult::Cancel,
        }
    }

    fn request_issue(
        &mut self,
        caption: &str,
        series: &SeriesRef,
        issues: &[IssueRef],
        _hint: Option<&IssueRef>,
        force: bool,
    ) -> IssueResult {
        let _ = self.tx.send(UiRequest::PickIssue(Box::new(PickIssueRequest {
            caption: caption.to_string(),
            series: series.clone(),
            issues: issues.to_vec(),
            force,
        })));
        match self.answer_rx.recv() {
            Ok(UiAnswer::Issue(r)) => r,
            _ => IssueResult::Cancel,
        }
    }

    fn no_issues_available(&mut self, series_name: &str) {
        let _ = self.tx.send(UiRequest::NoIssues {
            series: series_name.to_string(),
        });
    }

    fn book_started(&mut self, caption: &str, remaining: usize) {
        let _ = self.tx.send(UiRequest::Started {
            caption: caption.to_string(),
            remaining,
        });
    }

    fn book_finished(&mut self, caption: &str, status: BookStatus) {
        let _ = self.tx.send(UiRequest::Finished {
            caption: caption.to_string(),
            status,
        });
    }

    fn book_scraped(&mut self, book: &ComicBook) {
        let _ = self.tx.send(UiRequest::Scraped(Box::new(book.clone())));
    }

    fn progress(&mut self, kind: ProgressKind, value: f64) {
        let text = match kind {
            ProgressKind::SeriesSearch => format!("Searching… {value} results"),
            ProgressKind::IssueList => format!("Loading issues… {}%", (value * 100.0) as u32),
        };
        let _ = self.tx.send(UiRequest::Progress { text });
    }
}

/// The scrape summary handed to `on_done`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScrapeSummary {
    pub scraped: usize,
    pub skipped: usize,
}

/// Loads the prior-series keys (`prior_series.json` under the plugin
/// config dir); a missing or broken file means an empty set.
fn load_prior_series() -> std::collections::HashSet<String> {
    let dir = cr_scrape::config::default_config_dir();
    let Ok(bytes) = std::fs::read(dir.join("prior_series.json")) else {
        return Default::default();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

/// Merges the run's chosen keys into `prior_series.json`.
fn save_prior_series(chosen: &[String]) {
    if chosen.is_empty() {
        return;
    }
    let dir = cr_scrape::config::default_config_dir();
    let mut all: std::collections::BTreeSet<String> = load_prior_series().into_iter().collect();
    all.extend(chosen.iter().cloned());
    let list: Vec<String> = all.into_iter().collect();
    if let Ok(bytes) = serde_json::to_vec_pretty(&list) {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(dir.join("prior_series.json"), bytes);
    }
}

/// The C# SearchForm: the series search terms. `None` = cancel the
/// whole scrape; `Some("")` = skip this book; `Some(terms)` = search.
fn ask_search_terms(
    parent: &impl IsA<gtk4::Window>,
    caption: &str,
    failed: &str,
    answer: &std::sync::mpsc::Sender<UiAnswer>,
) -> Dialog {
    let dialog = Dialog::builder()
        .title("Scrape: search terms")
        .transient_for(parent)
        .modal(true)
        .default_width(420)
        .build();
    let content = dialog.content_area();
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    content.set_margin_start(8);
    content.set_margin_end(8);
    content.set_spacing(6);
    content.append(&Label::new(Some(&format!(
        "No series matched for {caption}."
    ))));
    if !failed.is_empty() {
        content.append(&Label::new(Some(&format!("Failed terms: {failed}"))));
    }
    let entry = gtk4::Entry::new();
    entry.set_activates_default(true);
    content.append(&entry);
    dialog.add_button("Search", gtk4::ResponseType::Ok);
    dialog.add_button("Skip", gtk4::ResponseType::Reject);
    dialog.add_button("Cancel", gtk4::ResponseType::Cancel);
    dialog.set_default_response(gtk4::ResponseType::Ok);
    let answer = answer.clone();
    dialog.connect_response(move |dlg, response| {
        let value = match response {
            gtk4::ResponseType::Ok => UiAnswer::Terms(Some(entry.text().trim().to_string())),
            gtk4::ResponseType::Reject => UiAnswer::Terms(Some(String::new())),
            _ => UiAnswer::Terms(None),
        };
        dlg.close();
        let _ = answer.send(value);
    });
    dialog.present();
    dialog
}

/// The series row text (the C# SeriesForm list text).
fn series_row_text(series_ref: &SeriesRef) -> String {
    let year = if series_ref.volume_year > 0 {
        format!(" ({})", series_ref.volume_year)
    } else {
        String::new()
    };
    let publisher = if series_ref.publisher.is_empty() {
        String::new()
    } else {
        format!(" \u{2014} {}", series_ref.publisher)
    };
    format!(
        "{}{} [{} issues]{}",
        series_ref.series_name(),
        year,
        series_ref.issue_count,
        publisher
    )
}

/// The C# SeriesForm: the series selection with the Search Again /
/// Skip / Cancel outcomes. OK resolves through the selection.
fn ask_series(
    parent: &impl IsA<gtk4::Window>,
    caption: &str,
    terms: &str,
    refs: &[SeriesRef],
    answer: &std::sync::mpsc::Sender<UiAnswer>,
) -> Dialog {
    let dialog = Dialog::builder()
        .title("Scrape: pick the series")
        .transient_for(parent)
        .modal(true)
        .default_width(520)
        .default_height(420)
        .build();
    let content = dialog.content_area();
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    content.set_margin_start(8);
    content.set_margin_end(8);
    content.set_spacing(6);
    content.append(&Label::new(Some(&format!(
        "{caption}: {} series match \u{201C}{terms}\u{201D}",
        refs.len()
    ))));

    let list = gtk4::ListBox::new();
    for series_ref in refs {
        let row = ListBoxRow::new();
        row.set_child(Some(&Label::new(Some(&series_row_text(series_ref)))));
        row.set_tooltip_text(Some(&format!("series key {}", series_ref.series_key)));
        list.append(&row);
    }
    let scroll = gtk4::ScrolledWindow::builder()
        .child(&list)
        .vexpand(true)
        .hexpand(true)
        .build();
    content.append(&scroll);
    dialog.add_button("Select Series", gtk4::ResponseType::Ok);
    dialog.add_button("Show Issues", gtk4::ResponseType::Apply);
    dialog.add_button("Search Again", gtk4::ResponseType::Reject);
    dialog.add_button("Skip", gtk4::ResponseType::No);
    dialog.add_button("Cancel", gtk4::ResponseType::Cancel);
    dialog.set_default_response(gtk4::ResponseType::Ok);

    let refs = std::rc::Rc::new(refs.to_vec());
    let list = std::rc::Rc::new(list);
    let answer = answer.clone();
    dialog.connect_response(move |dlg, response| {
        let index = list.selected_row().map(|r| r.index() as usize);
        let value = match response {
            gtk4::ResponseType::Ok => match index {
                Some(i) if i < refs.len() => UiAnswer::Series(SeriesResult::Ok(refs[i].clone())),
                _ => UiAnswer::Series(SeriesResult::Skip),
            },
            gtk4::ResponseType::Apply => match list.selected_row().map(|r| r.index() as usize) {
                Some(i) if i < refs.len() => UiAnswer::Series(SeriesResult::Show(refs[i].clone())),
                _ => UiAnswer::Series(SeriesResult::Skip),
            },
            gtk4::ResponseType::Reject => UiAnswer::Series(SeriesResult::Search),
            gtk4::ResponseType::No => UiAnswer::Series(SeriesResult::Skip),
            _ => UiAnswer::Series(SeriesResult::Cancel),
        };
        dlg.close();
        let _ = answer.send(value);
    });
    dialog.present();
    dialog
}

/// The issue row text (the C# IssueForm list text).
fn issue_row_text(issue_ref: &IssueRef) -> String {
    if issue_ref.title.is_empty() {
        format!("Issue #{}", issue_ref.issue_num)
    } else {
        format!(
            "Issue #{} \u{2014} {}",
            issue_ref.issue_num, issue_ref.title
        )
    }
}

/// The C# IssueForm: the issue selection with the Back / Skip /
/// Cancel outcomes. OK resolves through the selection.
fn ask_issue(
    parent: &impl IsA<gtk4::Window>,
    caption: &str,
    series: &SeriesRef,
    issues: &[IssueRef],
    force: bool,
    answer: &std::sync::mpsc::Sender<UiAnswer>,
) -> Dialog {
    let title = if force {
        "Scrape: confirm the issue"
    } else {
        "Scrape: pick the issue"
    };
    let dialog = Dialog::builder()
        .title(title)
        .transient_for(parent)
        .modal(true)
        .default_width(520)
        .default_height(420)
        .build();
    let content = dialog.content_area();
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    content.set_margin_start(8);
    content.set_margin_end(8);
    content.set_spacing(6);
    content.append(&Label::new(Some(&format!(
        "{caption} in {} ({} issues)",
        series.series_name(),
        issues.len()
    ))));

    let list = gtk4::ListBox::new();
    for issue_ref in issues {
        let row = ListBoxRow::new();
        row.set_child(Some(&Label::new(Some(&issue_row_text(issue_ref)))));
        list.append(&row);
    }
    let scroll = gtk4::ScrolledWindow::builder()
        .child(&list)
        .vexpand(true)
        .hexpand(true)
        .build();
    content.append(&scroll);
    dialog.add_button("Select Issue", gtk4::ResponseType::Ok);
    dialog.add_button("Back", gtk4::ResponseType::Reject);
    dialog.add_button("Skip", gtk4::ResponseType::No);
    dialog.add_button("Cancel", gtk4::ResponseType::Cancel);
    dialog.set_default_response(gtk4::ResponseType::Ok);

    let issues = std::rc::Rc::new(issues.to_vec());
    let list = std::rc::Rc::new(list);
    let answer = answer.clone();
    dialog.connect_response(move |dlg, response| {
        let index = list.selected_row().map(|r| r.index() as usize);
        let value = match response {
            gtk4::ResponseType::Ok => match index {
                Some(i) if i < issues.len() => UiAnswer::Issue(IssueResult::Ok(issues[i].clone())),
                _ => UiAnswer::Issue(IssueResult::Skip),
            },
            gtk4::ResponseType::Reject => UiAnswer::Issue(IssueResult::Back),
            gtk4::ResponseType::No => UiAnswer::Issue(IssueResult::Skip),
            _ => UiAnswer::Issue(IssueResult::Cancel),
        };
        dlg.close();
        let _ = answer.send(value);
    });
    dialog.present();
    dialog
}

/// The status lines (one per book: "caption — state").
fn status_text(states: &[(String, Option<BookStatus>)]) -> String {
    states
        .iter()
        .map(|(caption, status)| {
            let word = match status {
                None => "pending",
                Some(BookStatus::Scraped) => "scraped",
                Some(BookStatus::Skipped) => "skipped",
                Some(BookStatus::Unscraped) => "unresolved",
                Some(BookStatus::Delayed) => "delayed",
            };
            format!("{caption} \u{2014} {word}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Opens the non-modal scrape window and starts the engine worker.
/// `config` is the loaded plugin configuration (the caller owns the
/// load/save); `base_url` overrides the ComicVine API root (the
/// mock-server seam). `on_done` runs on the main thread when the run
/// ends (cancellation included); `on_scraped` per committed book
/// (the caller's view refresh).
pub fn show_scrape_dialog(
    parent: &impl IsA<gtk4::Window>,
    config: &Configuration,
    books: Vec<ComicBook>,
    base_url: Option<String>,
    on_done: impl Fn(Option<ScrapeSummary>) + 'static,
    on_scraped: impl Fn() + 'static,
) {
    if books.is_empty() {
        return;
    }
    let window = gtk4::Window::builder()
        .title("Comic Vine Scraper")
        .transient_for(parent)
        .default_width(460)
        .default_height(420)
        .build();

    // The status list (one line per book), the progress, the cancel.
    let states: Arc<Mutex<BookStates>> = Arc::default();
    let status_label = Label::new(None);
    status_label.set_halign(gtk4::Align::Start);
    status_label.set_valign(gtk4::Align::Start);
    status_label.set_xalign(0.0);
    let scroll = gtk4::ScrolledWindow::builder()
        .child(&status_label)
        .vexpand(true)
        .hexpand(true)
        .build();
    let progress_label = Label::new(Some("Starting…"));
    let cancel = gtk4::Button::with_label("Cancel");
    let bottom = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    bottom.append(&progress_label);
    bottom.append(&cancel);

    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    content.set_margin_start(8);
    content.set_margin_end(8);
    content.append(&scroll);
    content.append(&bottom);
    window.set_child(Some(&content));

    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    {
        let stop = std::sync::Arc::clone(&stop);
        cancel.connect_clicked(move |_| {
            stop.store(true, Ordering::Relaxed);
        });
    }
    {
        let stop = std::sync::Arc::clone(&stop);
        window.connect_close_request(move |_w| {
            stop.store(true, Ordering::Relaxed);
            glib::Propagation::Proceed
        });
    }

    // The channels.
    let (tx, rx) = std::sync::mpsc::channel::<UiRequest>();
    let (answer_tx, answer_rx) = std::sync::mpsc::channel::<UiAnswer>();

    // The worker: the engine over the channel UI.
    let worker_config = config.clone();
    let worker_stop = std::sync::Arc::clone(&stop);
    let prior = load_prior_series();
    std::thread::Builder::new()
        .name("Comic Vine Scraper".into())
        .spawn(move || {
            let done_tx = tx.clone();
            let mut ui = ChannelUi { tx, answer_rx };
            let client = match &base_url {
                Some(url) => CvClient::with_delays(
                    &worker_config.api_key,
                    url,
                    Duration::from_millis(0),
                    Duration::from_millis(0),
                ),
                None => CvClient::new(&worker_config.api_key),
            };
            let mut cv = Cv::new(client);
            let engine = ScrapeEngine::new(worker_config, worker_stop, prior);
            let (scraped, skipped) = engine.scrape(books, &mut ui, &mut cv);
            let chosen = engine.chosen();
            let _ = done_tx.send(UiRequest::Done {
                scraped,
                skipped,
                chosen,
            });
        })
        .expect("spawn the scraper worker");

    // The main-thread pump: drain the worker's requests, update the
    // status list, present the modal dialogs (which answer over the
    // response channel), commit scraped books, finish on Done.
    let window_pump = window;
    let status_pump = status_label;
    let progress_pump = progress_label;
    let states_pump = states;
    let answer_tx_pump = answer_tx;
    let parent_pump = parent.upcast_ref::<gtk4::Window>().clone();
    let on_done_pump = on_done;
    let on_scraped_pump = on_scraped;
    glib::timeout_add_local(Duration::from_millis(50), move || {
        while let Ok(request) = rx.try_recv() {
            match request {
                UiRequest::SearchTerms { caption, failed } => {
                    ask_search_terms(&parent_pump, &caption, &failed, &answer_tx_pump);
                }
                UiRequest::PickSeries {
                    caption,
                    terms,
                    refs,
                } => {
                    ask_series(&parent_pump, &caption, &terms, &refs, &answer_tx_pump);
                }
                UiRequest::PickIssue(payload) => {
                    ask_issue(
                        &parent_pump,
                        &payload.caption,
                        &payload.series,
                        &payload.issues,
                        payload.force,
                        &answer_tx_pump,
                    );
                }
                UiRequest::NoIssues { series } => {
                    progress_pump.set_text(&format!("No issues in {series}"));
                }
                UiRequest::Started { caption, remaining } => {
                    states_pump.lock().unwrap().push((caption, None));
                    let _ = remaining;
                    status_label_set(&status_pump, &states_pump);
                    progress_pump.set_text("Choosing the series…");
                }
                UiRequest::Finished { caption, status } => {
                    let mut states = states_pump.lock().unwrap();
                    if let Some(slot) = states
                        .iter_mut()
                        .find(|(c, s)| c == &caption && s.is_none())
                    {
                        slot.1 = Some(status);
                    } else {
                        states.push((caption, Some(status)));
                    }
                    drop(states);
                    status_label_set(&status_pump, &states_pump);
                }
                UiRequest::Scraped(book) => {
                    crate::library::apply_edited(&book);
                    on_scraped_pump();
                }
                UiRequest::Progress { text } => {
                    progress_pump.set_text(&text);
                }
                UiRequest::Done {
                    scraped,
                    skipped,
                    chosen,
                } => {
                    save_prior_series(&chosen);
                    window_pump.close();
                    on_done_pump(Some(ScrapeSummary { scraped, skipped }));
                    return ControlFlow::Break;
                }
            }
        }
        ControlFlow::Continue
    });
}

/// Re-renders the per-book status lines.
fn status_label_set(label: &Label, states: &Mutex<BookStates>) {
    let states = states.lock().unwrap();
    label.set_text(&status_text(&states));
}
