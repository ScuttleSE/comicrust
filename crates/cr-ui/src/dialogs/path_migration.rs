//! The Windows-path migration dialog (Phase 8 T11). Shown at boot when
//! the database still carries Windows-style paths (the C# ComicRack CE
//! migration), and re-runnable from File ▸ "Migrate Windows Paths...".
//!
//! One row per collapsed root (`C:\Comics`): the per-family counts, a
//! target entry + chooser, and a live "N of M found" preview. OK
//! applies the mappings through `library::apply_path_migration`
//! (found files re-home with a file-info refresh, missing files
//! become fileless books, watch folders / blacklist rewrite only when
//! the target exists); rows left unmapped stay for the next prompt.

use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{Button, Dialog, Entry, Label, ResponseType, ScrolledWindow, Window};

use cr_engine::path_migration::{preview_books, Mapping, PathRoot};

use crate::library;

/// One dialog row: the root, its counts, the target entry, the live
/// book preview. The widgets clone into the closures (the entry stays
/// the single source of truth for the target).
#[derive(Clone)]
struct Row {
    root: PathRoot,
    entry: Entry,
    preview: Label,
    book_count: usize,
}

impl Row {
    fn counts_line(root: &PathRoot) -> String {
        let mut parts: Vec<String> = Vec::new();
        if root.books > 0 {
            parts.push(format!(
                "{} book{}",
                root.books,
                if root.books == 1 { "" } else { "s" }
            ));
        }
        if root.watch_folders > 0 {
            parts.push(format!(
                "{} watch folder{}",
                root.watch_folders,
                if root.watch_folders == 1 { "" } else { "s" }
            ));
        }
        if root.black_list > 0 {
            parts.push(format!(
                "{} blacklist entr{}",
                root.black_list,
                if root.black_list == 1 { "y" } else { "ies" }
            ));
        }
        if parts.is_empty() {
            "no library members".into()
        } else {
            parts.join(", ")
        }
    }

    /// The live preview text (books found / not-found under the
    /// chosen target).
    fn refresh_preview(&self, books: &[cr_core::model::comic_book::ComicBook]) {
        let target = self.entry.text().trim().to_string();
        if target.is_empty() {
            self.preview.set_text("No target folder chosen.");
            return;
        }
        let (found, missing) = preview_books(books, &self.root.windows_root, &target);
        self.preview.set_text(&format!(
            "{} of {} found; {} not found will become fileless books.",
            found, self.book_count, missing
        ));
    }

    fn mapping(&self) -> Option<Mapping> {
        let target = self.entry.text().trim().to_string();
        if target.is_empty() {
            return None;
        }
        Some(Mapping {
            windows_root: self.root.windows_root.clone(),
            linux_target: target,
        })
    }
}

/// Opens the modal migration dialog. `on_done` runs once after OK
/// (with the apply report) or with None on Cancel / no roots.
pub fn run(
    parent: &impl IsA<Window>,
    on_done: impl FnOnce(Option<cr_engine::path_migration::ApplyReport>) + 'static,
) {
    let roots = library::windows_path_roots();
    if roots.is_empty() {
        on_done(None);
        return;
    }
    let books: Vec<cr_core::model::comic_book::ComicBook> =
        library::session().borrow().database().books.clone();

    let dialog = Dialog::builder()
        .title("Migrate Windows Paths")
        .transient_for(parent)
        .modal(true)
        .build();
    dialog.add_button("Cancel", ResponseType::Cancel);
    dialog.add_button("OK", ResponseType::Ok);
    dialog.set_default_size(720, 420);

    let content = dialog.content_area();
    content.set_spacing(8);
    content.set_margin_top(12);
    content.set_margin_bottom(6);
    content.set_margin_start(12);
    content.set_margin_end(12);

    let intro = Label::new(Some(
        "The library still points to Windows locations. Choose the Linux \
         folder each root maps to — found files re-home, missing files \
         become fileless books (metadata kept). Roots left unmapped \
         prompt again on the next start.",
    ));
    intro.set_wrap(true);
    intro.set_xalign(0.0);
    content.append(&intro);

    let rows = std::rc::Rc::new(std::cell::RefCell::new(Vec::<Row>::new()));
    let scroll = ScrolledWindow::new();
    scroll.set_vexpand(true);
    scroll.set_hexpand(true);
    scroll.set_min_content_height(240);
    let list = gtk4::Box::new(gtk4::Orientation::Vertical, 10);
    list.set_margin_top(4);
    scroll.set_child(Some(&list));
    content.append(&scroll);

    for root in roots {
        let root_books = root.books;
        let row_box = gtk4::Box::new(gtk4::Orientation::Vertical, 4);

        let head = Label::new(None);
        head.set_markup(&format!(
            "<b>{}</b>\n<span foreground=\"gray\" size=\"small\">{}</span>",
            glib::markup_escape_text(&root.windows_root),
            glib::markup_escape_text(&Row::counts_line(&root))
        ));
        head.set_xalign(0.0);
        row_box.append(&head);

        let target_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        let entry = Entry::new();
        entry.set_hexpand(true);
        entry.set_placeholder_text(Some("/home/user/comics"));
        let choose = Button::with_label("Choose…");
        target_row.append(&entry);
        target_row.append(&choose);
        row_box.append(&target_row);

        let preview = Label::new(Some("No target folder chosen."));
        preview.set_xalign(0.0);
        preview.set_wrap(true);
        row_box.append(&preview);

        list.append(&row_box);

        let row = Row {
            root,
            entry: entry.clone(),
            preview: preview.clone(),
            book_count: root_books,
        };

        {
            let row = row.clone();
            let books = books.clone();
            entry.connect_changed(move |_| {
                row.refresh_preview(&books);
            });
        }
        {
            let entry = entry.clone();
            choose.connect_clicked(move |_| {
                let chooser = gtk4::FileChooserNative::new(
                    Some("Select the target folder"),
                    None::<&Window>,
                    gtk4::FileChooserAction::SelectFolder,
                    Some("Select"),
                    Some("Cancel"),
                );
                let entry = entry.clone();
                chooser.connect_response(move |dlg, resp| {
                    if resp == gtk4::ResponseType::Accept {
                        if let Some(path) = dlg.file().and_then(|f| f.path()) {
                            // The entry is the single source of truth
                            // (the export-dialog lesson): the picker
                            // writes into it and the changed handler
                            // recomputes the preview.
                            entry.set_text(&path.to_string_lossy());
                        }
                    }
                });
                chooser.show();
            });
        }

        rows.borrow_mut().push(row);
    }

    // OK applies (the `done` guard: the re-entrant response lesson).
    let done = std::rc::Rc::new(std::cell::Cell::new(false));
    let on_done = std::cell::RefCell::new(Some(on_done));
    {
        let done = std::rc::Rc::clone(&done);
        let rows = std::rc::Rc::clone(&rows);
        dialog.connect_response(move |dlg, response| {
            if response != ResponseType::Ok {
                if !done.replace(true) {
                    dlg.close();
                    if let Some(f) = on_done.borrow_mut().take() {
                        f(None);
                    }
                }
                return;
            }
            if done.replace(true) {
                return;
            }
            let mappings: Vec<Mapping> = rows.borrow().iter().filter_map(|r| r.mapping()).collect();
            if mappings.is_empty() {
                // Nothing mapped: nothing changes; the prompt returns
                // on the next start.
                done.replace(false);
                return;
            }
            let report = library::apply_path_migration(&mappings);
            dlg.close();
            if let Some(f) = on_done.borrow_mut().take() {
                f(Some(report));
            }
        });
    }
    dialog.present();
}
