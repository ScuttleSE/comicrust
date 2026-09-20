//! The Comic Vine Scraper configuration widgets — the C#
//! `configform.py` port. The API key entry, the scrape-field
//! checkboxes, the behavior checkboxes, and the advanced
//! KEY=VALUE text. The same widget set serves TWO surfaces: the
//! standalone dialog (`show_scrape_config`, the plugin's own
//! config form) and the Comic Vine Scraper page of the Preferences
//! dialog. Both commit through [`ScrapeConfigWidgets::collect`];
//! OK commits, Cancel discards.

use gtk4::prelude::*;
use gtk4::{CheckButton, Dialog, Entry, Grid, Label, ScrolledWindow, TextView};

use cr_scrape::config::Configuration;

/// One checkbox row: the label plus the compile-checked read/write
/// accessors (no string dispatch).
type Flag = (
    &'static str,
    fn(&Configuration) -> bool,
    fn(&mut Configuration, bool),
);

/// The scrape-field flags, in the C# ConfigForm's display order.
const SCRAPE_FLAGS: &[Flag] = &[
    ("Series", |c| c.update_series, |c, v| c.update_series = v),
    ("Volume", |c| c.update_volume, |c, v| c.update_volume = v),
    ("Number", |c| c.update_number, |c, v| c.update_number = v),
    ("Title", |c| c.update_title, |c, v| c.update_title = v),
    (
        "Published",
        |c| c.update_published,
        |c, v| c.update_published = v,
    ),
    (
        "Released",
        |c| c.update_released,
        |c, v| c.update_released = v,
    ),
    (
        "Crossovers",
        |c| c.update_crossovers,
        |c, v| c.update_crossovers = v,
    ),
    ("Writer", |c| c.update_writer, |c, v| c.update_writer = v),
    (
        "Penciller",
        |c| c.update_penciller,
        |c, v| c.update_penciller = v,
    ),
    ("Inker", |c| c.update_inker, |c, v| c.update_inker = v),
    (
        "Cover Artist",
        |c| c.update_cover_artist,
        |c, v| c.update_cover_artist = v,
    ),
    (
        "Colorist",
        |c| c.update_colorist,
        |c, v| c.update_colorist = v,
    ),
    (
        "Letterer",
        |c| c.update_letterer,
        |c, v| c.update_letterer = v,
    ),
    ("Editor", |c| c.update_editor, |c, v| c.update_editor = v),
    ("Summary", |c| c.update_summary, |c, v| c.update_summary = v),
    (
        "Publisher",
        |c| c.update_publisher,
        |c, v| c.update_publisher = v,
    ),
    ("Imprint", |c| c.update_imprint, |c, v| c.update_imprint = v),
    (
        "Characters",
        |c| c.update_characters,
        |c, v| c.update_characters = v,
    ),
    ("Teams", |c| c.update_teams, |c, v| c.update_teams = v),
    (
        "Locations",
        |c| c.update_locations,
        |c, v| c.update_locations = v,
    ),
    ("Webpage", |c| c.update_webpage, |c, v| c.update_webpage = v),
];

/// The behavior flags.
const BEHAVIOR_FLAGS: &[Flag] = &[
    (
        "Overwrite Existing",
        |c| c.overwrite_existing,
        |c, v| c.overwrite_existing = v,
    ),
    (
        "Ignore Blanks",
        |c| c.ignore_blanks,
        |c, v| c.ignore_blanks = v,
    ),
    (
        "Convert Imprints",
        |c| c.convert_imprints,
        |c, v| c.convert_imprints = v,
    ),
    (
        "Autochoose Series",
        |c| c.autochoose_series,
        |c, v| c.autochoose_series = v,
    ),
    (
        "Confirm Issues",
        |c| c.confirm_issue,
        |c, v| c.confirm_issue = v,
    ),
    (
        "Download Thumbs",
        |c| c.download_thumbs,
        |c, v| c.download_thumbs = v,
    ),
    (
        "Preserve Thumbs",
        |c| c.preserve_thumbs,
        |c, v| c.preserve_thumbs = v,
    ),
    (
        "Fast Rescrape",
        |c| c.fast_rescrape,
        |c, v| c.fast_rescrape = v,
    ),
    (
        "Rescraping: Notes",
        |c| c.rescrape_notes,
        |c, v| c.rescrape_notes = v,
    ),
    (
        "Rescraping: Tags",
        |c| c.rescrape_tags,
        |c, v| c.rescrape_tags = v,
    ),
    (
        "Summary Dialog",
        |c| c.summary_dialog,
        |c, v| c.summary_dialog = v,
    ),
];

/// The config-form widget set (shared by the standalone dialog and
/// the Preferences page). `collect` reads the widgets back into a
/// `Configuration` (the OK path).
pub struct ScrapeConfigWidgets {
    /// The layout grid (margins free — the host adds its own).
    pub grid: Grid,
    base: Configuration,
    api_entry: Entry,
    checks: Vec<CheckButton>,
    /// The offline switch of the advanced settings (ADR-071).
    offline_check: CheckButton,
    /// The auto refresh switch of the advanced settings (ADR-071).
    auto_refresh_check: CheckButton,
    advanced_view: TextView,
}

impl ScrapeConfigWidgets {
    /// Builds the widgets prefilled from `config`.
    pub fn build(config: &Configuration) -> Self {
        let grid = Grid::new();
        grid.set_row_spacing(4);
        grid.set_column_spacing(8);

        // the API key
        let api_entry = Entry::builder().text(&config.api_key).hexpand(true).build();
        grid.attach(&Label::new(Some("API Key")), 0, 0, 1, 1);
        grid.attach(&api_entry, 1, 0, 1, 1);

        // the checkbox grid: the scrape fields, then the behavior flags
        let mut checks: Vec<CheckButton> = Vec::new();
        for (row, (label, read, _)) in SCRAPE_FLAGS.iter().chain(BEHAVIOR_FLAGS.iter()).enumerate()
        {
            let check = CheckButton::with_label(label);
            check.set_active(read(config));
            let column = (row % 2) as i32;
            let grid_row = (row as i32) / 2 + 1;
            grid.attach(&check, column, grid_row, 1, 1);
            checks.push(check);
        }
        // Check All / Uncheck All over the SCRAPE checkboxes (the C#
        // ConfigForm buttons)
        let check_all = gtk4::Button::with_label("Check All");
        let uncheck_all = gtk4::Button::with_label("Uncheck All");
        {
            let checks = checks.clone();
            let count = SCRAPE_FLAGS.len();
            check_all.connect_clicked(move |_| {
                for check in checks.iter().take(count) {
                    check.set_active(true);
                }
            });
        }
        {
            let checks = checks.clone();
            let count = SCRAPE_FLAGS.len();
            uncheck_all.connect_clicked(move |_| {
                for check in checks.iter().take(count) {
                    check.set_active(false);
                }
            });
        }
        let buttons_row = (SCRAPE_FLAGS.len() as i32) / 2 + 2;
        grid.attach(&check_all, 0, buttons_row, 1, 1);
        grid.attach(&uncheck_all, 1, buttons_row, 1, 1);

        // The two cache switches of the advanced settings (ADR-071).
        // They read the parsed defaults and write back into the
        // KEY=VALUE text on collect, so a hand-edited line and the
        // check box cannot disagree.
        let advanced = config.advanced();
        let offline_check = CheckButton::with_label("Offline Mode (Cache Only)");
        offline_check.set_active(advanced.cache_offline_only);
        let auto_refresh_check = CheckButton::with_label("Auto Cache Refresh");
        auto_refresh_check.set_active(advanced.cache_refresh_auto);
        grid.attach(&offline_check, 0, buttons_row + 1, 1, 1);
        grid.attach(&auto_refresh_check, 1, buttons_row + 1, 1, 1);

        // the advanced settings text, verbatim
        let advanced_view = TextView::builder().monospace(true).build();
        advanced_view.buffer().set_text(&config.advanced_settings);
        let advanced_scroll = ScrolledWindow::builder()
            .child(&advanced_view)
            .vexpand(true)
            .height_request(90)
            .build();
        grid.attach(
            &Label::new(Some("Advanced settings (KEY=VALUE lines)")),
            0,
            buttons_row + 2,
            2,
            1,
        );
        grid.attach(&advanced_scroll, 0, buttons_row + 3, 2, 1);

        ScrapeConfigWidgets {
            grid,
            base: config.clone(),
            api_entry,
            checks,
            offline_check,
            auto_refresh_check,
            advanced_view,
        }
    }

    /// Reads the widgets back into a `Configuration` (the OK path:
    /// the API key, the flags, and the advanced text reparse).
    pub fn collect(&self) -> Configuration {
        let mut result = self.base.clone();
        result.api_key = self.api_entry.text().trim().to_string();
        for (row, (_, _, write)) in SCRAPE_FLAGS.iter().chain(BEHAVIOR_FLAGS.iter()).enumerate() {
            write(&mut result, self.checks[row].is_active());
        }
        let buf = self.advanced_view.buffer();
        let text = buf
            .text(&buf.start_iter(), &buf.end_iter(), false)
            .to_string();
        // The cache switches write into the advanced text, so the
        // text stays the one source of truth (ADR-071).
        let text = set_advanced_line(
            &text,
            "CACHE_OFFLINE_ONLY",
            if self.offline_check.is_active() {
                "true"
            } else {
                "false"
            },
        );
        let text = set_advanced_line(
            &text,
            "CACHE_REFRESH_MODE",
            if self.auto_refresh_check.is_active() {
                "auto"
            } else {
                "manual"
            },
        );
        result.set_advanced_settings(&text);
        result
    }
}

/// Replaces the one `KEY=VALUE` line in the advanced text, or appends
/// it when absent.
fn set_advanced_line(text: &str, key: &str, value: &str) -> String {
    let mut replaced = false;
    let mut lines: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        let matches = trimmed.len() >= key.len()
            && trimmed[..key.len()].eq_ignore_ascii_case(key)
            && trimmed[key.len()..].trim_start().starts_with('=');
        if matches {
            lines.push(format!("{key}={value}"));
            replaced = true;
        } else {
            lines.push(line.to_string());
        }
    }
    if !replaced {
        lines.push(format!("{key}={value}"));
    }
    let mut out = lines.join("\n");
    if text.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Opens the modal config dialog. `on_done` runs once with the
/// committed settings (OK) or `None` (Cancel).
pub fn show_scrape_config(
    parent: &impl IsA<gtk4::Window>,
    config: &Configuration,
    on_done: impl Fn(Option<Configuration>) + 'static,
) {
    let dialog = Dialog::builder()
        .title("Comic Vine Scraper Settings")
        .transient_for(parent)
        .modal(true)
        .default_width(620)
        .default_height(560)
        .build();
    let content = dialog.content_area();
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    content.set_margin_start(8);
    content.set_margin_end(8);
    content.set_spacing(6);

    let widgets = ScrapeConfigWidgets::build(config);
    content.append(&widgets.grid);
    dialog.add_button("OK", gtk4::ResponseType::Ok);
    dialog.add_button("Cancel", gtk4::ResponseType::Cancel);

    let done = std::rc::Rc::new(std::cell::Cell::new(false));
    {
        let done = std::rc::Rc::clone(&done);
        dialog.connect_response(move |dlg, response| {
            if done.replace(true) {
                return;
            }
            let result = if response == gtk4::ResponseType::Ok {
                Some(widgets.collect())
            } else {
                None
            };
            dlg.close();
            on_done(result);
        });
    }
    dialog.present();
}
