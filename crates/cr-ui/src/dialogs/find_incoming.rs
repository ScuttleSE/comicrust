//! Selects Incoming copies that fill selected Missing Issues rows.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;

use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::CrGuid;
use gtk4::prelude::*;
use gtk4::{Align, ComboBoxText, Dialog, Grid, Label, ResponseType, ScrolledWindow};

/// One selected Missing Issues row and its Incoming candidates.
pub struct FindIncomingRow {
    pub missing: ComicBook,
    pub candidates: Vec<ComicBook>,
}

type CandidateSelection = (Vec<ComicBook>, Option<ComboBoxText>);

fn issue_text(book: &ComicBook) -> String {
    format!(
        "{} v{} #{}",
        book.info.series, book.info.volume, book.info.number
    )
}

fn candidate_text(book: &ComicBook) -> String {
    book.file_path.clone()
}

/// Shows the match summary and returns one confirmed Incoming copy per match.
pub fn show(
    parent: &impl IsA<gtk4::Window>,
    rows: Vec<FindIncomingRow>,
    on_done: impl FnOnce(Option<Vec<ComicBook>>) + 'static,
) {
    let unique = rows.iter().filter(|row| row.candidates.len() == 1).count();
    let ambiguous = rows.iter().filter(|row| row.candidates.len() > 1).count();
    let unmatched = rows.iter().filter(|row| row.candidates.is_empty()).count();

    let dialog = Dialog::builder()
        .title("Find in Incoming")
        .transient_for(parent)
        .modal(true)
        .default_width(720)
        .default_height(440)
        .build();
    if unique + ambiguous == 0 {
        dialog.add_button("Close", ResponseType::Cancel);
    } else {
        dialog.add_button("Cancel", ResponseType::Cancel);
        dialog.add_button("Adopt Matches", ResponseType::Ok);
    }

    let content = dialog.content_area();
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    content.set_margin_start(8);
    content.set_margin_end(8);
    content.set_spacing(8);
    let summary = Label::new(Some(&format!(
        "{unique} direct match(es), {ambiguous} issue(s) need a selection, {unmatched} issue(s) have no match."
    )));
    summary.set_halign(Align::Start);
    content.append(&summary);

    let grid = Grid::builder().column_spacing(12).row_spacing(6).build();
    grid.attach(&Label::new(Some("Missing issue")), 0, 0, 1, 1);
    grid.attach(&Label::new(Some("Incoming copy")), 1, 0, 1, 1);

    let selections: Rc<RefCell<Vec<CandidateSelection>>> =
        Rc::new(RefCell::new(Vec::with_capacity(rows.len())));
    for (index, row) in rows.into_iter().enumerate() {
        let issue = Label::new(Some(&issue_text(&row.missing)));
        issue.set_halign(Align::Start);
        issue.set_selectable(true);
        grid.attach(&issue, 0, index as i32 + 1, 1, 1);

        let choice = match row.candidates.len() {
            0 => {
                let label = Label::new(Some("No match"));
                label.set_halign(Align::Start);
                grid.attach(&label, 1, index as i32 + 1, 1, 1);
                None
            }
            1 => {
                let label = Label::new(Some(&candidate_text(&row.candidates[0])));
                label.set_halign(Align::Start);
                label.set_selectable(true);
                grid.attach(&label, 1, index as i32 + 1, 1, 1);
                None
            }
            _ => {
                let combo = ComboBoxText::new();
                combo.append_text("Select a file…");
                for candidate in &row.candidates {
                    combo.append_text(&candidate_text(candidate));
                }
                combo.set_active(Some(0));
                combo.set_hexpand(true);
                grid.attach(&combo, 1, index as i32 + 1, 1, 1);
                Some(combo)
            }
        };
        selections.borrow_mut().push((row.candidates, choice));
    }

    let scroll = ScrolledWindow::builder()
        .hexpand(true)
        .vexpand(true)
        .child(&grid)
        .build();
    content.append(&scroll);
    let error = Label::new(None);
    error.add_css_class("error");
    error.set_halign(Align::Start);
    content.append(&error);

    let callback = Rc::new(RefCell::new(Some(on_done)));
    let finished = Rc::new(Cell::new(false));
    dialog.connect_response(move |dialog, response| {
        if finished.get() {
            return;
        }
        if response != ResponseType::Ok {
            finished.set(true);
            dialog.close();
            if let Some(callback) = callback.borrow_mut().take() {
                callback(None);
            }
            return;
        }

        let mut selected = Vec::new();
        let mut selected_ids = HashSet::<CrGuid>::new();
        for (candidates, combo) in selections.borrow().iter() {
            let candidate = match candidates.as_slice() {
                [] => None,
                [candidate] => Some(candidate),
                _ => combo
                    .as_ref()
                    .and_then(|combo| combo.active())
                    .and_then(|index| index.checked_sub(1))
                    .and_then(|index| candidates.get(index as usize)),
            };
            let Some(candidate) = candidate else {
                if candidates.len() > 1 {
                    error
                        .set_text("Select one Incoming copy for each issue with multiple matches.");
                    return;
                }
                continue;
            };
            if !selected_ids.insert(candidate.id) {
                error.set_text("One Incoming copy cannot fill more than one missing issue.");
                return;
            }
            selected.push(candidate.clone());
        }
        if selected.is_empty() {
            error.set_text("No Incoming copies are selected for adoption.");
            return;
        }

        finished.set(true);
        dialog.close();
        if let Some(callback) = callback.borrow_mut().take() {
            callback(Some(selected));
        }
    });
    dialog.present();
}
