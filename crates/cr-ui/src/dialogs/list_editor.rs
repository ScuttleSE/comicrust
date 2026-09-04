//! The list editor — the `Dialogs/EditListDialog.cs` port.
//!
//! Covers the two item kinds the C# routes here: FOLDERS (name,
//! notes, the combine mode — "All Books from every list" / "Only
//! Books existing in every list" / "Empty list") and READING LISTS
//! (name, notes, QuickOpen). OK writes the fields through the
//! caller's callback; Cancel discards. The C# "Show notes" toggle
//! is a panel-visibility nicety — the port shows the notes row
//! always.

use gtk4::prelude::*;
use gtk4::{Align, ComboBoxText, Dialog, Entry, Grid, Label, ScrolledWindow, TextView};

use cr_core::model::enums::ComicFolderCombineMode;

/// The kind selector the caller passes (which rows show).
#[derive(Clone, Copy, PartialEq)]
pub enum ListKind {
    Folder,
    ReadingList,
}

/// The committed fields.
pub struct ListEditResult {
    pub name: String,
    pub description: String,
    pub quick_open: bool,
    pub combine_mode: Option<ComicFolderCombineMode>,
}

/// Opens the modal editor. `on_done` runs once with the committed
/// fields (OK) or `None` (Cancel). The `done` Cell guards the
/// re-entrant close response (the smart-list lesson).
pub fn show_list_editor(
    parent: &impl IsA<gtk4::Window>,
    kind: ListKind,
    name: &str,
    description: &str,
    quick_open: bool,
    combine_mode: ComicFolderCombineMode,
    on_done: impl Fn(Option<ListEditResult>) + 'static,
) {
    let dialog = Dialog::builder()
        .title(if kind == ListKind::Folder {
            "Edit Folder"
        } else {
            "Edit Reading List"
        })
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

    let grid = Grid::new();
    grid.set_row_spacing(4);
    grid.set_column_spacing(8);

    let name_entry = Entry::builder().text(name).hexpand(true).build();
    grid.attach(&Label::new(Some("Name")), 0, 0, 1, 1);
    grid.attach(&name_entry, 1, 0, 1, 1);

    let notes_view = TextView::builder().wrap_mode(gtk4::WrapMode::Word).build();
    notes_view.buffer().set_text(description);
    let notes_scroll = ScrolledWindow::builder()
        .child(&notes_view)
        .height_request(60)
        .build();
    grid.attach(&Label::new(Some("Notes")), 0, 1, 1, 1);
    grid.attach(&notes_scroll, 1, 1, 1, 1);

    let mut quick_open_check = None;
    let mut combine_combo = None;
    let row = 2;
    if kind == ListKind::ReadingList {
        let check = gtk4::CheckButton::with_label("Show in Quick Open");
        check.set_active(quick_open);
        grid.attach(&check, 1, row, 1, 1);
        quick_open_check = Some(check);
    }
    if kind == ListKind::Folder {
        let combo = ComboBoxText::new();
        for (id, label) in [
            ("0", "All Books from every list"),
            ("1", "Only Books existing in every list"),
            ("2", "Empty list"),
        ] {
            combo.append(Some(id), label);
        }
        combo.set_active(Some(combine_mode as u32));
        grid.attach(&Label::new(Some("Combine")), 0, row, 1, 1);
        grid.attach(&combo, 1, row, 1, 1);
        combine_combo = Some(combo);
    }

    content.append(&grid);
    dialog.add_button("OK", gtk4::ResponseType::Ok);
    dialog.add_button("Cancel", gtk4::ResponseType::Cancel);

    let done = std::rc::Rc::new(std::cell::RefCell::new(false));
    {
        let done = std::rc::Rc::clone(&done);
        dialog.connect_response(move |dlg, response| {
            if done.replace(true) {
                return;
            }
            match response {
                gtk4::ResponseType::Ok => {
                    let text = |tv: &TextView| {
                        let buf = tv.buffer();
                        buf.text(&buf.start_iter(), &buf.end_iter(), false)
                            .trim()
                            .to_string()
                    };
                    let combine_mode = match &combine_combo {
                        Some(c) => match c.active() {
                            Some(1) => Some(ComicFolderCombineMode::And),
                            Some(2) => Some(ComicFolderCombineMode::Empty),
                            _ => Some(ComicFolderCombineMode::Or),
                        },
                        None => None,
                    };
                    let result = ListEditResult {
                        name: name_entry.text().trim().to_string(),
                        description: text(&notes_view),
                        quick_open: quick_open_check.as_ref().is_some_and(|c| c.is_active()),
                        combine_mode,
                    };
                    dlg.close();
                    on_done(Some(result));
                }
                _ => {
                    dlg.close();
                    on_done(None);
                }
            }
        });
    }

    let _ = Align::Start;
    dialog.present();
}
