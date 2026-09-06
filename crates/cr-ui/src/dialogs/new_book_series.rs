//! The "New fileless Book Series…" dialog — the `NewComics.py` port
//! (the script created a run of fileless books for one series; the
//! `#@Hook NewBooks` item sat directly after the native
//! "New fileless Book Entry..." in the File menu, MainForm.cs:787).
//! ADR-027 moved it natively into the app.

use gtk4::prelude::*;
use gtk4::{Dialog, Entry, Label};

use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::{CrDateTime, CrGuid};

/// The script's `GetNumber`: an int, or -1 on any failure.
fn parse_number(text: &str) -> i32 {
    text.trim().parse::<i32>().unwrap_or(-1)
}

/// Opens the modal dialog (series, volume, number range). `on_ok`
/// runs once with the parsed fields. The OK button enables when the
/// series is non-empty and `start >= 0` and `end >= start` (the
/// script's `InputTextChanged` rule).
pub fn show(parent: &impl IsA<gtk4::Window>, on_ok: impl Fn(String, i32, i32, i32) + 'static) {
    let dialog = Dialog::builder()
        .title("New fileless Book Series")
        .transient_for(parent)
        .modal(true)
        .default_width(360)
        .build();
    let content = dialog.content_area();
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    content.set_margin_start(8);
    content.set_margin_end(8);
    content.set_spacing(6);

    let grid = gtk4::Grid::new();
    grid.set_row_spacing(6);
    grid.set_column_spacing(6);

    let series = Entry::new();
    let volume = Entry::new();
    let from = Entry::new();
    let to = Entry::new();
    for (row, (label, w, entry)) in [
        ("Series:", 240, &series),
        ("Volume:", 60, &volume),
        ("Number from:", 60, &from),
        ("to:", 60, &to),
    ]
    .into_iter()
    .enumerate()
    {
        let lbl = Label::new(Some(label));
        lbl.set_halign(gtk4::Align::Start);
        grid.attach(&lbl, 0, row as i32, 1, 1);
        entry.set_width_request(w);
        entry.set_hexpand(true);
        grid.attach(entry, 1, row as i32, 1, 1);
    }
    content.append(&grid);

    dialog.add_button("OK", gtk4::ResponseType::Ok);
    dialog.add_button("Cancel", gtk4::ResponseType::Cancel);
    let ok_btn = dialog.widget_for_response(gtk4::ResponseType::Ok);
    for e in [&series, &from, &to] {
        e.set_activates_default(true);
    }
    dialog.set_default_response(gtk4::ResponseType::Ok);

    // The OK-enable rule (the script's `InputTextChanged`).
    let valid = {
        let series = series.clone();
        let from = from.clone();
        let to = to.clone();
        move || {
            let start = parse_number(from.text().as_str());
            let end = parse_number(to.text().as_str());
            !series.text().trim().is_empty() && start >= 0 && end >= start
        }
    };
    let sync_ok = {
        let valid = valid.clone();
        move || {
            if let Some(b) = &ok_btn {
                b.set_sensitive(valid());
            }
        }
    };
    for e in [&series, &volume, &from, &to] {
        let sync = sync_ok.clone();
        e.connect_changed(move |_| sync());
    }
    sync_ok();

    // One-shot close guard (the re-entrant response lesson).
    let done = std::rc::Rc::new(std::cell::Cell::new(false));
    {
        let done = std::rc::Rc::clone(&done);
        let series = series.clone();
        let volume = volume.clone();
        let from = from.clone();
        let to = to.clone();
        dialog.connect_response(move |dlg, response| {
            if done.replace(true) {
                return;
            }
            let ok = response == gtk4::ResponseType::Ok;
            let (series, volume, first, last) = (
                series.text().trim().to_string(),
                parse_number(volume.text().as_str()),
                parse_number(from.text().as_str()),
                parse_number(to.text().as_str()),
            );
            dlg.close();
            if ok {
                on_ok(series, volume, first, last);
            }
        });
    }
    dialog.present();
}

/// The book factory both the series dialog and the single entry use:
/// fresh id, `AddedTime = now`, no file path (fileless).
pub fn new_fileless_book() -> ComicBook {
    let mut book = ComicBook {
        id: CrGuid::new_random(),
        ..Default::default()
    };
    book.added_time = CrDateTime::now();
    book
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_number_defaults_to_minus_one() {
        assert_eq!(parse_number("12"), 12);
        assert_eq!(parse_number(" 3 "), 3);
        assert_eq!(parse_number(""), -1);
        assert_eq!(parse_number("x"), -1);
        assert_eq!(parse_number("1.5"), -1);
    }

    #[test]
    fn fileless_books_get_fresh_ids_and_time() {
        let a = new_fileless_book();
        let b = new_fileless_book();
        assert_ne!(a.id, b.id);
        assert_ne!(a.id, CrGuid::EMPTY);
        assert!(a.file_path.is_empty());
        assert!(a.added_time.naive > CrDateTime::min_value().naive);
    }
}
