//! The "Link Series from Cache" volume-pick dialog: a small, one-shot
//! version of the scrape wizard's series picker
//! (`dialogs/scrape.rs::ask_series`), trimmed to the two outcomes this
//! flow needs (pick a row, or cancel). Always shown, even for a single
//! search result — this command writes real custom values onto real
//! books, so the user confirms what they're linking to before anything
//! is written.

use gtk4::prelude::*;
use gtk4::{Dialog, Label, ListBoxRow, Window};

use cr_scrape::cv::models::SeriesRef;

/// A left-aligned label in a fixed-width column (mirrors
/// `dialogs/scrape.rs::cell_label`).
fn cell_label(text: &str, width_chars: i32, ellipsize: bool, expand: bool) -> Label {
    let label = Label::new(Some(text));
    label.set_halign(gtk4::Align::Start);
    label.set_xalign(0.0);
    if width_chars > 0 {
        label.set_width_chars(width_chars);
    }
    if ellipsize {
        label.set_ellipsize(gtk4::pango::EllipsizeMode::End);
    }
    if expand {
        label.set_hexpand(true);
    }
    label
}

/// One row's worth of columns: name / year / issue count / publisher
/// (mirrors `dialogs/scrape.rs::series_grid`).
fn series_grid(texts: [&str; 4], header: bool) -> gtk4::Grid {
    let grid = gtk4::Grid::new();
    grid.set_column_spacing(12);
    let name = cell_label(texts[0], 0, true, true);
    let year = cell_label(texts[1], 6, false, false);
    let issues = cell_label(texts[2], 9, false, false);
    let publisher = cell_label(texts[3], 24, true, false);
    if header {
        for label in [&name, &year, &issues, &publisher] {
            label.add_css_class("heading");
        }
    }
    grid.attach(&name, 0, 0, 1, 1);
    grid.attach(&year, 1, 0, 1, 1);
    grid.attach(&issues, 2, 0, 1, 1);
    grid.attach(&publisher, 3, 0, 1, 1);
    grid
}

/// Shows the candidate volumes for `series_name` and calls `on_pick`
/// exactly once with the chosen `SeriesRef`, or `None` on Cancel/close.
pub fn show(
    parent: &impl IsA<Window>,
    series_name: &str,
    refs: &[SeriesRef],
    on_pick: Box<dyn Fn(Option<SeriesRef>)>,
) {
    let dialog = Dialog::builder()
        .title("Link Series from Cache: pick the volume")
        .transient_for(parent)
        .modal(true)
        .default_width(640)
        .default_height(420)
        .build();
    let content = dialog.content_area();
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    content.set_margin_start(8);
    content.set_margin_end(8);
    content.set_spacing(6);
    let headline = Label::new(Some(&format!(
        "{} Comic Vine volume{} match \u{201C}{series_name}\u{201D}",
        refs.len(),
        if refs.len() == 1 { "" } else { "s" }
    )));
    headline.set_xalign(0.0);
    content.append(&headline);

    content.append(&series_grid(
        ["Series", "Year", "Issues", "Publisher"],
        true,
    ));
    let list = gtk4::ListBox::new();
    list.set_activate_on_single_click(false);
    list.set_selection_mode(gtk4::SelectionMode::Single);
    for series_ref in refs {
        let row = ListBoxRow::new();
        let year = if series_ref.volume_year > 0 {
            format!("({})", series_ref.volume_year)
        } else {
            String::new()
        };
        row.set_child(Some(&series_grid(
            [
                series_ref.series_name(),
                &year,
                &series_ref.issue_count.to_string(),
                &series_ref.publisher,
            ],
            false,
        )));
        row.set_tooltip_text(Some(&format!("series key {}", series_ref.series_key)));
        list.append(&row);
    }
    list.select_row(list.row_at_index(0).as_ref());
    let scroll = gtk4::ScrolledWindow::builder()
        .child(&list)
        .vexpand(true)
        .hexpand(true)
        .build();
    content.append(&scroll);
    dialog.add_button("Link this Volume", gtk4::ResponseType::Ok);
    dialog.add_button("Cancel", gtk4::ResponseType::Cancel);
    dialog.set_default_response(gtk4::ResponseType::Ok);

    let refs = std::rc::Rc::new(refs.to_vec());
    let list = std::rc::Rc::new(list);
    let on_pick = std::rc::Rc::new(on_pick);
    let done = std::rc::Rc::new(std::cell::Cell::new(false));
    let finish = std::rc::Rc::new({
        let dialog = dialog.clone();
        let done = std::rc::Rc::clone(&done);
        let on_pick = std::rc::Rc::clone(&on_pick);
        move |value: Option<SeriesRef>| {
            if done.replace(true) {
                return;
            }
            dialog.close();
            on_pick(value);
        }
    });
    {
        let finish = std::rc::Rc::clone(&finish);
        let refs = std::rc::Rc::clone(&refs);
        list.connect_row_activated(move |_, row| {
            let i = row.index() as usize;
            if i < refs.len() {
                finish(Some(refs[i].clone()));
            }
        });
    }
    {
        let finish = std::rc::Rc::clone(&finish);
        let refs = std::rc::Rc::clone(&refs);
        let list = std::rc::Rc::clone(&list);
        dialog.connect_response(move |_, response| {
            let value = match response {
                gtk4::ResponseType::Ok => list
                    .selected_row()
                    .map(|r| r.index() as usize)
                    .and_then(|i| refs.get(i).cloned()),
                _ => None,
            };
            finish(value);
        });
    }
    dialog.present();
}
