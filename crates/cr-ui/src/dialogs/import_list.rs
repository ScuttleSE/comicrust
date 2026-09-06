//! The `.cbl` reading-list import — the `ComicListLibraryBrowser
//! .ImportList` port (ComicListLibraryBrowser.cs:1464-1556) plus the
//! missing-books question (`QuestionDialog.AskQuestion` with the
//! "Add missing Books to Library" option).
//!
//! Flow: parse the file → matchers → a smart list, else the library
//! match (`cr_engine::reading_list`) → the unsolved-books question →
//! insert (target container, or the Temporary Lists folder) → tree
//! refill + selection.

use std::path::Path;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{MessageDialog, MessageType, ResponseType, Window};

use cr_core::database::list_items::{ComicListItem, IdListItem, ListItemBase, SmartListItem};
use cr_core::database::reading_list::ReadingListContainer;
use cr_core::xml::scalar::CrGuid;

use crate::browser::navigator::Navigator;
use crate::library;

/// `UnsolvedBookItems`: up to 25 `'caption'` lines, `...` beyond,
/// then the still-import question.
fn missing_books_message(new_books: &[cr_core::model::comic_book::ComicBook]) -> String {
    let mut list = String::new();
    for (i, cb) in new_books.iter().enumerate().take(25) {
        if i != 0 {
            list.push('\n');
        }
        list.push('\'');
        list.push_str(&cr_engine::display_text::caption(cb));
        list.push('\'');
    }
    if new_books.len() > 25 {
        list.push_str("\n...");
    }
    format!(
        "The following Books were not found in the Library:\n{list}\nDo you still want to import the Reading List?"
    )
}

fn show_import_error(parent: &impl IsA<Window>, file: &Path) {
    let dlg = MessageDialog::builder()
        .title("comicrust")
        .transient_for(parent)
        .modal(true)
        .message_type(MessageType::Error)
        .text(format!(
            "There was an error importing the Reading List '{}'",
            file.file_name().and_then(|n| n.to_str()).unwrap_or("")
        ))
        .buttons(gtk4::ButtonsType::Ok)
        .build();
    dlg.connect_response(|d, _| d.close());
    dlg.present();
}

/// The question dialog: `Import` (Ok), `Add missing Books to Library`
/// (the option), `Cancel`. `f(ok, add_missing)` runs once.
fn ask_missing_books(
    parent: &impl IsA<Window>,
    message: &str,
    f: impl FnOnce(bool, bool) + 'static,
) {
    let dlg = MessageDialog::builder()
        .title("Import")
        .transient_for(parent)
        .modal(true)
        .message_type(MessageType::Question)
        .text(message)
        .build();
    dlg.add_button("Import", ResponseType::Ok);
    dlg.add_button("Add missing Books to Library", ResponseType::Apply);
    dlg.add_button("Cancel", ResponseType::Cancel);
    let done = Rc::new(std::cell::Cell::new(false));
    let f = std::cell::RefCell::new(Some(f));
    {
        let done = Rc::clone(&done);
        dlg.connect_response(move |d, response| {
            if done.replace(true) {
                return;
            }
            d.close();
            let ok = response == ResponseType::Ok;
            let add = response == ResponseType::Apply;
            if let Some(f) = f.borrow_mut().take() {
                f(ok, add);
            }
        });
    }
    dlg.present();
}

/// Inserts the built item, refills + selects the tree, then reports.
fn finish_insert(
    item: ComicListItem,
    target: Option<CrGuid>,
    nav: &Rc<Navigator>,
    done: impl FnOnce(Option<CrGuid>) + 'static,
) {
    let id = match target {
        Some(t) => library::import_list_item(Some(&t), item),
        None => library::import_temporary_item(item),
    };
    nav.refill(&library::comic_lists_snapshot());
    nav.select_list(&id);
    done(Some(id));
}

/// The `ImportList` port. `target` is the navigator selection (None
/// = the Temporary Lists folder). `done` receives the new list id,
/// `None` on parse failure or cancel.
pub fn import_list_file(
    parent: &impl IsA<Window>,
    path: &Path,
    target: Option<CrGuid>,
    nav: &Rc<Navigator>,
    done: impl FnOnce(Option<CrGuid>) + 'static,
) {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(_) => {
            show_import_error(parent, path);
            done(None);
            return;
        }
    };
    let container = match ReadingListContainer::parse(&bytes) {
        Ok(c) => c,
        Err(_) => {
            show_import_error(parent, path);
            done(None);
            return;
        }
    };
    let name = container.name.clone().unwrap_or_default();
    if !container.matchers.is_empty() {
        // `new ComicSmartListItem(name, mode, matchers)`.
        let item = ComicListItem::Smart(SmartListItem {
            base: ListItemBase {
                id: CrGuid::new_random(),
                name: Some(name),
                ..Default::default()
            },
            matcher_mode: container.matcher_mode,
            matchers: container.matchers.clone(),
            ..Default::default()
        });
        finish_insert(item, target, nav, done);
        return;
    }
    // The id-list path: match against the library (the C# wraps this
    // in an AutomaticProgressDialog — the in-memory match is fast, so
    // the port runs it synchronously; a recorded deviation).
    let books = library::session().borrow().database().books.clone();
    let m = cr_engine::reading_list::match_container(&books, &container);
    if m.new_books.is_empty() {
        let item = ComicListItem::IdList(IdListItem {
            base: ListItemBase {
                id: CrGuid::new_random(),
                name: Some(name),
                ..Default::default()
            },
            book_ids: m.book_ids,
        });
        finish_insert(item, target, nav, done);
        return;
    }
    let message = missing_books_message(&m.new_books);
    let nav = Rc::clone(nav);
    ask_missing_books(parent, &message, move |import, add_missing| {
        if !import && !add_missing {
            done(None);
            return;
        }
        let mut book_ids = m.book_ids;
        if add_missing {
            // `Library.Books.AddRange(newBooks)` — the placeholders
            // become fileless library books.
            library::add_books(m.new_books);
        } else {
            // Import solved only: the placeholder ids drop.
            book_ids.retain(|id| !m.new_books.iter().any(|cb| cb.id == *id));
        }
        let item = ComicListItem::IdList(IdListItem {
            base: ListItemBase {
                id: CrGuid::new_random(),
                name: Some(name),
                ..Default::default()
            },
            book_ids,
        });
        finish_insert(item, target, &nav, done);
    });
}
