//! The bulk editor — the `Dialogs/MultipleComicBooksDialog.cs` port.
//!
//! The C# model: one check box per field. Unchecked = leave the
//! field alone on every book; Checked = apply the typed value to
//! every book (the C# tri-state indeterminate is its list-merge
//! mode — deferred, recorded in the kickoff). The gray cue shows
//! the COMMON value (`GetSameValue`: identical across the selection
//! or empty). OK applies only the checked fields through the same
//! registry machinery as the single editor, marks every touched
//! book's file info dirty (the write-back flow decides about the
//! file), and the Books list refreshes.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{
    Align, CheckButton, ComboBoxText, Dialog, Entry, Grid, Label, Notebook, ScrolledWindow,
    TextView,
};

use cr_core::model::comic_book::ComicBook;
use cr_core::registry::{self, PropValue};

use crate::dialogs::book_editor::{
    manga_from_combo, number_from_text, proposed_placeholders, rating_from_text, real_from_text,
    yesno_from_combo, CATALOG_ROWS, DETAIL_ROWS, DETAIL_ROWS_2, MANGA_ITEMS, NUM_ROWS, PLOT_ROWS,
    PROPOSED_ITEMS, YES_NO_ITEMS,
};

/// One bulk row: the "Set" check + the widget.
struct BulkRow {
    check: CheckButton,
    entry: Option<Entry>,
    text_view: Option<TextView>,
    combo: Option<ComboBoxText>,
}

#[derive(Default)]
struct BulkFields {
    rows: HashMap<&'static str, BulkRow>,
}

type FieldsRef = Rc<RefCell<BulkFields>>;

/// Opens the bulk editor over `books` (clones). `commit` runs once
/// per changed book (the same shape as the single editor's commit).
pub fn show(
    parent: &impl IsA<gtk4::Window>,
    books: Vec<ComicBook>,
    commit: Rc<dyn Fn(&ComicBook)>,
) {
    if books.is_empty() {
        return;
    }
    let count = books.len();
    let dialog = Dialog::builder()
        .title(format!("Edit {} Books", count))
        .transient_for(parent)
        .modal(true)
        .default_width(860)
        .default_height(620)
        .build();
    let content = dialog.content_area();
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    content.set_margin_start(8);
    content.set_margin_end(8);
    content.set_spacing(6);

    let hint = Label::new(Some(
        "Only the fields with a checked “Set” box apply to every book.",
    ));
    hint.set_halign(Align::Start);
    content.append(&hint);

    let fields: FieldsRef = Rc::new(RefCell::new(BulkFields::default()));
    let notebook = Notebook::new();

    // A row builder: the check + label + widget.
    let add_row = |fields: &FieldsRef,
                   grid: &Grid,
                   row: i32,
                   caption: &str,
                   prop: &'static str,
                   widget: BulkRowWidget| {
        let check = CheckButton::with_label("Set");
        grid.attach(&check, 0, row, 1, 1);
        grid.attach(&Label::new(Some(caption)), 1, row, 1, 1);
        match widget {
            BulkRowWidget::Entry(e) => {
                e.set_hexpand(true);
                grid.attach(&e, 2, row, 1, 1);
                fields.borrow_mut().rows.insert(
                    prop,
                    BulkRow {
                        check,
                        entry: Some(e),
                        text_view: None,
                        combo: None,
                    },
                );
            }
            BulkRowWidget::Text(tv) => {
                let scroll = ScrolledWindow::builder()
                    .child(&tv)
                    .height_request(44)
                    .build();
                grid.attach(&scroll, 2, row, 1, 1);
                fields.borrow_mut().rows.insert(
                    prop,
                    BulkRow {
                        check,
                        entry: None,
                        text_view: Some(tv),
                        combo: None,
                    },
                );
            }
            BulkRowWidget::Combo(c) => {
                grid.attach(&c, 2, row, 1, 1);
                fields.borrow_mut().rows.insert(
                    prop,
                    BulkRow {
                        check,
                        entry: None,
                        text_view: None,
                        combo: Some(c),
                    },
                );
            }
        }
    };

    let make_entry = || Entry::new();

    // ----- Details -----
    {
        let grid = Grid::new();
        grid.set_row_spacing(4);
        grid.set_column_spacing(8);
        grid.set_margin_top(8);
        grid.set_margin_bottom(8);
        grid.set_margin_start(8);
        grid.set_margin_end(8);
        // Rating rows (float).
        {
            let rating = make_entry();
            add_row(
                &fields,
                &grid,
                0,
                "Rating",
                "Rating",
                BulkRowWidget::Entry(rating),
            );
            let cr = make_entry();
            add_row(
                &fields,
                &grid,
                1,
                "Community Rating",
                "CommunityRating",
                BulkRowWidget::Entry(cr),
            );
        }
        let mut row = 2;
        for tr in DETAIL_ROWS {
            add_row(
                &fields,
                &grid,
                row,
                tr.caption,
                tr.property,
                BulkRowWidget::Entry(make_entry()),
            );
            row += 1;
        }
        for (caption, prop) in NUM_ROWS {
            add_row(
                &fields,
                &grid,
                row,
                caption,
                prop,
                BulkRowWidget::Entry(make_entry()),
            );
            row += 1;
        }
        for tr in DETAIL_ROWS_2 {
            add_row(
                &fields,
                &grid,
                row,
                tr.caption,
                tr.property,
                BulkRowWidget::Entry(make_entry()),
            );
            row += 1;
        }
        // The combos.
        let combo_row = |fields: &FieldsRef,
                         grid: &Grid,
                         row: i32,
                         caption: &str,
                         prop: &'static str,
                         items: &[&str]| {
            let combo = ComboBoxText::new();
            for item in items {
                combo.append_text(item);
            }
            add_row(
                fields,
                grid,
                row,
                caption,
                prop,
                BulkRowWidget::Combo(combo),
            );
        };
        combo_row(
            &fields,
            &grid,
            row,
            "Series Complete",
            "SeriesComplete",
            &YES_NO_ITEMS,
        );
        row += 1;
        combo_row(&fields, &grid, row, "Manga", "Manga", &MANGA_ITEMS);
        row += 1;
        combo_row(
            &fields,
            &grid,
            row,
            "Black and White",
            "BlackAndWhite",
            &YES_NO_ITEMS,
        );
        row += 1;
        combo_row(
            &fields,
            &grid,
            row,
            "Enable Proposed",
            "EnableProposed",
            &PROPOSED_ITEMS,
        );
        let scroll = ScrolledWindow::builder()
            .child(&grid)
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vexpand(true)
            .build();
        notebook.append_page(&scroll, Some(&Label::new(Some("Details"))));
    }

    // ----- Plot -----
    {
        let grid = Grid::new();
        grid.set_row_spacing(4);
        grid.set_column_spacing(8);
        grid.set_margin_top(8);
        grid.set_margin_bottom(8);
        grid.set_margin_start(8);
        grid.set_margin_end(8);
        let mut row = 0;
        for (caption, prop) in [
            ("Summary", "Summary"),
            ("Notes", "Notes"),
            ("Review", "Review"),
        ] {
            let label = Label::new(Some(caption));
            label.set_halign(Align::Start);
            label.set_valign(Align::Start);
            grid.attach(&label, 0, row, 1, 1);
            let label = Label::new(Some(caption));
            label.set_halign(Align::Start);
            label.set_valign(Align::Start);
            grid.attach(&label, 0, row, 1, 1);
            let tv = TextView::new();
            tv.set_wrap_mode(gtk4::WrapMode::Word);
            tv.set_hexpand(true);
            add_row(&fields, &grid, row, caption, prop, BulkRowWidget::Text(tv));
            row += 1;
        }
        for tr in PLOT_ROWS {
            add_row(
                &fields,
                &grid,
                row,
                tr.caption,
                tr.property,
                BulkRowWidget::Entry(make_entry()),
            );
            row += 1;
        }
        let scroll = ScrolledWindow::builder()
            .child(&grid)
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vexpand(true)
            .build();
        notebook.append_page(&scroll, Some(&Label::new(Some("Plot"))));
    }

    // ----- Catalog -----
    {
        let grid = Grid::new();
        grid.set_row_spacing(4);
        grid.set_column_spacing(8);
        grid.set_margin_top(8);
        grid.set_margin_bottom(8);
        grid.set_margin_start(8);
        grid.set_margin_end(8);
        for (i, tr) in CATALOG_ROWS.iter().enumerate() {
            add_row(
                &fields,
                &grid,
                i as i32,
                tr.caption,
                tr.property,
                BulkRowWidget::Entry(make_entry()),
            );
        }
        let scroll = ScrolledWindow::builder()
            .child(&grid)
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vexpand(true)
            .build();
        notebook.append_page(&scroll, Some(&Label::new(Some("Catalog"))));
    }

    // The gray cue = the common value (`GetSameValue`).
    for (prop, row) in fields.borrow().rows.iter() {
        let common = same_value(&books, prop);
        let text = match &common {
            Some(v) => v.clone(),
            None => String::new(),
        };
        if let Some(e) = &row.entry {
            e.set_placeholder_text(Some(&text));
        } else if let Some(tv) = &row.text_view {
            tv.buffer().set_text(&text);
        }
    }
    // The proposed placeholders follow the first book (the C# gray
    // texts read `Proposed*` of the first book too).
    {
        let f = fields.borrow();
        for (prop, value) in proposed_placeholders(&books[0]) {
            if let Some(row) = f.rows.get(prop) {
                if let Some(e) = &row.entry {
                    e.set_placeholder_text(Some(value.as_str()));
                }
            }
        }
    }

    content.append(&notebook);
    dialog.add_button("OK", gtk4::ResponseType::Ok);
    dialog.add_button("Cancel", gtk4::ResponseType::Cancel);

    dialog.connect_response(move |dlg, response| {
        if response == gtk4::ResponseType::Ok {
            let f = fields.borrow();
            for mut book in books.iter().cloned() {
                let mut changed = false;
                for (prop, row) in f.rows.iter() {
                    if !row.check.is_active() {
                        continue;
                    }
                    if apply_row(prop, row, &mut book) {
                        changed = true;
                    }
                }
                if changed {
                    commit(&book);
                }
            }
        }
        dlg.close();
    });

    dialog.present();
}

enum BulkRowWidget {
    Entry(Entry),
    Text(TextView),
    Combo(ComboBoxText),
}

/// `GetSameValue`: the field's value when identical across the
/// selection, `Some("")` when they differ, `None` when unset
/// everywhere (the cue stays empty either way).
fn same_value(books: &[ComicBook], prop: &str) -> Option<String> {
    let mut current: Option<String> = None;
    for b in books {
        let v = match registry::get(b, prop) {
            Some(PropValue::Str(s)) => s,
            Some(PropValue::Int(i)) if i >= 0 => i.to_string(),
            Some(PropValue::Float(f)) if f > 0.0 => f.to_string(),
            _ => String::new(),
        };
        match &current {
            None => current = Some(v),
            Some(c) if *c == v => {}
            _ => return Some(String::new()),
        }
    }
    current
}

/// Applies one checked row to a book. Returns whether the value
/// changed.
fn apply_row(prop: &str, row: &BulkRow, book: &mut ComicBook) -> bool {
    let before = registry::get(book, prop);
    if let Some(entry) = &row.entry {
        let text = entry.text().trim().to_string();
        match prop {
            "Rating" => book.rating = rating_from_text(&text, book.rating),
            "CommunityRating" => {
                book.info.community_rating = rating_from_text(&text, book.info.community_rating)
            }
            "BookPrice" => book.book_price = real_from_text(&text),
            _ => {
                if row_is_int(prop) {
                    let v = number_from_text(&text);
                    let _ = registry::set(book, prop, &PropValue::Int(v as i64));
                } else {
                    let _ = registry::set(book, prop, &PropValue::Str(text));
                }
            }
        }
    } else if let Some(tv) = &row.text_view {
        let buf = tv.buffer();
        let text = buf
            .text(&buf.start_iter(), &buf.end_iter(), false)
            .trim()
            .to_string();
        let _ = registry::set(book, prop, &PropValue::Str(text));
    } else if let Some(combo) = &row.combo {
        match prop {
            "SeriesComplete" => book.series_complete = yesno_from_combo(combo.active()),
            "Manga" => book.info.manga = manga_from_combo(combo.active()),
            "BlackAndWhite" => book.info.black_and_white = yesno_from_combo(combo.active()),
            "EnableProposed" => book.enable_proposed = matches!(combo.active(), Some(1)),
            _ => {}
        }
    }
    registry::get(book, prop) != before
}

fn row_is_int(prop: &str) -> bool {
    NUM_ROWS.iter().any(|(_, p)| *p == prop)
}
