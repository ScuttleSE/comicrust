//! The Comic Vine cache manager (ADR-064).

use std::rc::Rc;
use std::sync::Arc;

use cr_scrape::cache::manage::{UpdateMode, UpdateReport};
use cr_scrape::cache::{ManagedVolume, SqliteCache};
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    ApplicationWindow, Box as GtkBox, Button, Dialog, Entry, Grid, Label, Orientation,
    ScrolledWindow, TextView,
};

pub type UpdateDone = Box<dyn Fn(Result<UpdateReport, String>)>;
pub type UpdateStarter = Rc<dyn Fn(i64, UpdateMode, UpdateDone) -> bool>;

/// A small test seam for the isolated release probe.
#[derive(Clone)]
pub struct CacheManagerHandle {
    dialog: Dialog,
    widgets: Widgets,
    search: Button,
}

impl CacheManagerHandle {
    pub fn set_series_id(&self, id: i64) {
        self.widgets.id.set_text(&id.to_string());
    }

    pub fn search(&self) {
        self.search.emit_clicked();
    }

    pub fn set_metadata(&self, name: &str, publisher: &str, year: &str) {
        self.widgets.name.set_text(name);
        self.widgets.publisher.set_text(publisher);
        self.widgets.year.set_text(year);
    }

    pub fn save(&self) {
        self.widgets.save.emit_clicked();
    }

    pub fn name(&self) -> String {
        self.widgets.name.text().to_string()
    }

    pub fn publisher(&self) -> String {
        self.widgets.publisher.text().to_string()
    }

    pub fn issue_text(&self) -> String {
        let buffer = self.widgets.issues.buffer();
        buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), false)
            .to_string()
    }

    pub fn status(&self) -> String {
        self.widgets.status.text().to_string()
    }

    pub fn close(&self) {
        self.dialog.close();
    }
}

#[derive(Clone)]
struct Widgets {
    id: Entry,
    name: Entry,
    publisher: Entry,
    year: Entry,
    issue_count: Label,
    pending: Label,
    status: Label,
    raw: TextView,
    issues: TextView,
    save: Button,
    update: Button,
    complete: Button,
}

struct DisplayRecord {
    name: String,
    publisher: String,
    year: String,
    issue_count: usize,
    pending: usize,
    raw: String,
    issues: String,
}

impl Widgets {
    fn volume_id(&self) -> Result<i64, String> {
        let id = self
            .id
            .text()
            .trim()
            .parse::<i64>()
            .map_err(|_| "Enter a numeric Comic Vine series ID.".to_string())?;
        if id <= 0 {
            return Err("Enter a positive Comic Vine series ID.".to_string());
        }
        Ok(id)
    }

    fn set_busy(&self, busy: bool) {
        self.save.set_sensitive(!busy);
        self.update.set_sensitive(!busy);
        self.complete.set_sensitive(!busy);
    }

    fn show_record(&self, volume_id: i64, record: Option<DisplayRecord>) {
        match record {
            Some(record) => {
                self.name.set_text(&record.name);
                self.publisher.set_text(&record.publisher);
                self.year.set_text(&record.year);
                self.issue_count
                    .set_text(&format!("{} cached issue(s)", record.issue_count));
                self.pending
                    .set_text(&format!("{} issue detail(s) remain", record.pending));
                self.raw.buffer().set_text(&record.raw);
                self.issues.buffer().set_text(&record.issues);
                self.status
                    .set_text(&format!("Loaded series ID {volume_id}."));
            }
            None => {
                self.name.set_text("");
                self.publisher.set_text("");
                self.year.set_text("");
                self.issue_count.set_text("0 cached issues");
                self.pending.set_text("0 issue details remain");
                self.raw
                    .buffer()
                    .set_text("This series ID is not in the local cache.");
                self.issues
                    .buffer()
                    .set_text("Issue ID\tNumber\tTitle\tCover date\n");
                self.status.set_text(
                    "No cached record. Save metadata or select an API update to create it.",
                );
            }
        }
    }
}

/// Opens the cache manager. All cache reads and writes run on workers.
pub fn show(
    parent: &ApplicationWindow,
    cache: Arc<SqliteCache>,
    start_update: UpdateStarter,
) -> CacheManagerHandle {
    let dialog = Dialog::builder()
        .title("Manage Comic Vine Cache")
        .transient_for(parent)
        .modal(false)
        .default_width(820)
        .default_height(700)
        .build();
    dialog.add_button("Close", gtk4::ResponseType::Close);

    let root = GtkBox::new(Orientation::Vertical, 8);
    root.set_margin_top(10);
    root.set_margin_bottom(10);
    root.set_margin_start(10);
    root.set_margin_end(10);
    let search_row = GtkBox::new(Orientation::Horizontal, 8);
    let id = Entry::builder()
        .placeholder_text("Comic Vine series ID")
        .hexpand(true)
        .input_purpose(gtk4::InputPurpose::Digits)
        .build();
    let search = Button::with_label("Search");
    search_row.append(&Label::new(Some("Comic Vine Series ID")));
    search_row.append(&id);
    search_row.append(&search);
    root.append(&search_row);

    let grid = Grid::new();
    grid.set_column_spacing(8);
    grid.set_row_spacing(6);
    let name = Entry::new();
    let publisher = Entry::new();
    let year = Entry::new();
    year.set_input_purpose(gtk4::InputPurpose::Digits);
    grid.attach(&Label::new(Some("Name")), 0, 0, 1, 1);
    grid.attach(&name, 1, 0, 1, 1);
    grid.attach(&Label::new(Some("Publisher")), 0, 1, 1, 1);
    grid.attach(&publisher, 1, 1, 1, 1);
    grid.attach(&Label::new(Some("Start year")), 0, 2, 1, 1);
    grid.attach(&year, 1, 2, 1, 1);
    name.set_hexpand(true);
    root.append(&grid);

    let action_row = GtkBox::new(Orientation::Horizontal, 8);
    let save = Button::with_label("Save Metadata");
    let update = Button::with_label("Update from API");
    let complete = Button::with_label("Complete Update from API");
    action_row.append(&save);
    action_row.append(&update);
    action_row.append(&complete);
    root.append(&action_row);

    let summary_row = GtkBox::new(Orientation::Horizontal, 16);
    let issue_count = Label::new(Some("0 cached issues"));
    let pending = Label::new(Some("0 issue details remain"));
    summary_row.append(&issue_count);
    summary_row.append(&pending);
    root.append(&summary_row);

    root.append(&Label::new(Some("Volume metadata from the API")));
    let raw = TextView::new();
    raw.set_editable(false);
    raw.set_monospace(true);
    raw.set_wrap_mode(gtk4::WrapMode::WordChar);
    let raw_scroll = ScrolledWindow::builder()
        .child(&raw)
        .min_content_height(180)
        .vexpand(true)
        .build();
    root.append(&raw_scroll);

    root.append(&Label::new(Some("Issues")));
    let issues = TextView::new();
    issues.set_editable(false);
    issues.set_monospace(true);
    let issue_scroll = ScrolledWindow::builder()
        .child(&issues)
        .min_content_height(220)
        .vexpand(true)
        .build();
    root.append(&issue_scroll);
    let status = Label::new(Some("Enter a Comic Vine series ID."));
    status.set_xalign(0.0);
    status.set_wrap(true);
    root.append(&status);
    dialog.content_area().append(&root);

    let widgets = Widgets {
        id,
        name,
        publisher,
        year,
        issue_count,
        pending,
        status,
        raw,
        issues,
        save,
        update,
        complete,
    };

    {
        let widgets = widgets.clone();
        let cache = Arc::clone(&cache);
        search.connect_clicked(move |_| search_cache(&widgets, Arc::clone(&cache)));
    }
    {
        let widgets = widgets.clone();
        let cache = Arc::clone(&cache);
        widgets
            .id
            .clone()
            .connect_activate(move |_| search_cache(&widgets, Arc::clone(&cache)));
    }
    {
        let widgets = widgets.clone();
        let cache = Arc::clone(&cache);
        widgets.save.clone().connect_clicked(move |_| {
            let volume_id = match widgets.volume_id() {
                Ok(id) => id,
                Err(error) => {
                    widgets.status.set_text(&error);
                    return;
                }
            };
            let year_text = widgets.year.text().trim().to_string();
            let start_year = if year_text.is_empty() {
                None
            } else {
                match year_text.parse::<i32>() {
                    Ok(year) if year > 0 => Some(year),
                    _ => {
                        widgets.status.set_text("Enter a positive start year.");
                        return;
                    }
                }
            };
            let name = optional_text(&widgets.name);
            let publisher = optional_text(&widgets.publisher);
            widgets.set_busy(true);
            widgets.status.set_text("Saving metadata...");
            let (tx, rx) = std::sync::mpsc::channel();
            let worker_cache = Arc::clone(&cache);
            std::thread::spawn(move || {
                let result = worker_cache
                    .update_volume_metadata(
                        volume_id,
                        name.as_deref(),
                        publisher.as_deref(),
                        start_year,
                    )
                    .map_err(|error| error.to_string());
                let _ = tx.send(result);
            });
            let widgets = widgets.clone();
            let cache = Arc::clone(&cache);
            glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
                let result = rx.try_recv();
                match result {
                    Ok(Ok(())) => {
                        widgets.set_busy(false);
                        search_cache(&widgets, Arc::clone(&cache));
                        glib::ControlFlow::Break
                    }
                    Ok(Err(error)) => {
                        widgets.set_busy(false);
                        widgets.status.set_text(&format!("Save failed: {error}"));
                        glib::ControlFlow::Break
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        widgets.set_busy(false);
                        widgets.status.set_text("The cache worker stopped.");
                        glib::ControlFlow::Break
                    }
                }
            });
        });
    }
    connect_update(
        &widgets.update,
        widgets.clone(),
        Arc::clone(&cache),
        Rc::clone(&start_update),
        UpdateMode::Summary,
    );
    connect_update(
        &widgets.complete,
        widgets.clone(),
        Arc::clone(&cache),
        start_update,
        UpdateMode::Complete,
    );

    dialog.connect_response(|dialog, _| dialog.close());
    dialog.present();
    CacheManagerHandle {
        dialog,
        widgets,
        search,
    }
}

fn optional_text(entry: &Entry) -> Option<String> {
    let text = entry.text().trim().to_string();
    (!text.is_empty()).then_some(text)
}

fn search_cache(widgets: &Widgets, cache: Arc<SqliteCache>) {
    let volume_id = match widgets.volume_id() {
        Ok(id) => id,
        Err(error) => {
            widgets.status.set_text(&error);
            return;
        }
    };
    widgets.set_busy(true);
    widgets.status.set_text("Reading the local cache...");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let result = cache
            .managed_volume(volume_id)
            .map(|record| record.map(display_record))
            .map_err(|error| error.to_string());
        let _ = tx.send(result);
    });
    let widgets = widgets.clone();
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        match rx.try_recv() {
            Ok(Ok(record)) => {
                widgets.set_busy(false);
                widgets.show_record(volume_id, record);
                glib::ControlFlow::Break
            }
            Ok(Err(error)) => {
                widgets.set_busy(false);
                widgets
                    .status
                    .set_text(&format!("Cache read failed: {error}"));
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                widgets.set_busy(false);
                widgets.status.set_text("The cache worker stopped.");
                glib::ControlFlow::Break
            }
        }
    });
}

fn display_record(record: ManagedVolume) -> DisplayRecord {
    let raw = record
        .detail_json
        .as_deref()
        .and_then(|json| serde_json::from_str::<serde_json::Value>(json).ok())
        .and_then(|value| serde_json::to_string_pretty(&value).ok())
        .unwrap_or_else(|| "No API volume detail is cached.".to_string());
    let mut issues = String::from("Issue ID\tNumber\tTitle\tCover date\n");
    for issue in &record.issues {
        issues.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            issue.issue_id,
            issue.issue_number,
            issue.name.as_deref().unwrap_or(""),
            issue.cover_date.as_deref().unwrap_or("")
        ));
    }
    DisplayRecord {
        name: record.volume.name.unwrap_or_default(),
        publisher: record.volume.publisher.unwrap_or_default(),
        year: record
            .volume
            .start_year
            .map(|year| year.to_string())
            .unwrap_or_default(),
        issue_count: record.issues.len(),
        pending: record.pending_issue_details.len(),
        raw,
        issues,
    }
}

fn connect_update(
    button: &Button,
    widgets: Widgets,
    cache: Arc<SqliteCache>,
    start_update: UpdateStarter,
    mode: UpdateMode,
) {
    button.connect_clicked(move |_| {
        let volume_id = match widgets.volume_id() {
            Ok(id) => id,
            Err(error) => {
                widgets.status.set_text(&error);
                return;
            }
        };
        let label = match mode {
            UpdateMode::Summary => "Updating volume metadata and issue numbers...",
            UpdateMode::Complete => "Updating complete volume and issue details...",
        };
        widgets.set_busy(true);
        widgets.status.set_text(label);
        let done_widgets = widgets.clone();
        let done_cache = Arc::clone(&cache);
        let started = start_update(
            volume_id,
            mode,
            Box::new(move |result| {
                done_widgets.set_busy(false);
                match result {
                    Ok(report) => {
                        let tail = if report.stopped {
                            format!(
                                "Stopped. {} issue detail(s) remain and will resume later.",
                                report.pending_issue_details
                            )
                        } else {
                            format!(
                                "Updated {} issue(s), fetched {} detail record(s), and used {} request(s).",
                                report.issues, report.issue_details, report.requests
                            )
                        };
                        done_widgets.status.set_text(&tail);
                        search_cache(&done_widgets, Arc::clone(&done_cache));
                    }
                    Err(error) => done_widgets
                        .status
                        .set_text(&format!("API update failed: {error}")),
                }
            }),
        );
        if !started {
            widgets.set_busy(false);
        }
    });
}
