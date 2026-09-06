//! Headless probe: the T2 `.cbl` reading-list import (the
//! `ComicListLibraryBrowser.ImportList` port). Gates: (A) the
//! missing-books question appears and "Add missing Books to Library"
//! adds the placeholder to the library, (B) the list lands in the
//! Temporary Lists folder and the tree selects it, (C) solved-by-id
//! and solved-by-file-name matching, (D) the "Import" (solved-only)
//! answer drops the unsolved id, (E) a matchers-only `.cbl` lands as
//! a smart list.
//! Run with an isolated XDG (the probe seeds books into the DB it
//! opens): XDG_DATA_HOME=/tmp/opencode/... cargo run -p cr-ui
//! --example importlist_probe
use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::{CrDateTime, CrGuid};
use gtk4::prelude::*;
use gtk4::{glib, Dialog};

fn find_toplevel(title: &str) -> Option<gtk4::Window> {
    gtk4::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk4::Window>().ok())
        .find(|w| w.title().as_deref().is_some_and(|t| t.starts_with(title)))
}

fn seed_book(id: CrGuid, series: &str, file: &str) -> ComicBook {
    let mut book = ComicBook {
        id,
        file_path: file.into(),
        added_time: CrDateTime::now(),
        ..Default::default()
    };
    book.info.series = series.into();
    book
}

/// The list item (by name, recursive — imports land inside the
/// Temporary Lists folder): (id, evaluated book ids, is-smart).
fn list_by_name(name: &str) -> Option<(CrGuid, Vec<CrGuid>, bool)> {
    fn walk<'a>(
        items: &'a [cr_core::database::list_items::ComicListItem],
        name: &str,
    ) -> Option<&'a cr_core::database::list_items::ComicListItem> {
        for item in items {
            if item.base().name.as_deref() == Some(name) {
                return Some(item);
            }
            if let cr_core::database::list_items::ComicListItem::Folder(f) = item {
                if let Some(found) = walk(&f.items, name) {
                    return Some(found);
                }
            }
        }
        None
    }
    let lib = cr_ui::library::session();
    let l = lib.borrow();
    let item = walk(&l.database().comic_lists, name)?;
    let (smart, ids) = match item {
        cr_core::database::list_items::ComicListItem::Smart(_) => (true, Vec::new()),
        cr_core::database::list_items::ComicListItem::IdList(_) => {
            let books = cr_engine::lists::evaluate_list(item, l.database());
            (false, books.iter().map(|b| b.id).collect())
        }
        _ => (false, Vec::new()),
    };
    Some((item.base().id, ids, smart))
}

fn temporary_folder_exists() -> bool {
    let lib = cr_ui::library::session();
    let l = lib.borrow();
    l.database().comic_lists.iter().any(
        |i| matches!(i, cr_core::database::list_items::ComicListItem::Folder(f) if f.temporary),
    )
}

fn main() {
    gtk4::init().expect("gtk init");
    cr_ui::theme::init();
    if !std::env::var("XDG_DATA_HOME")
        .map(|v| v.contains("/tmp/opencode"))
        .unwrap_or(false)
    {
        eprintln!("REFUSED: set XDG_DATA_HOME=/tmp/opencode/<dir> (the probe seeds books into the DB it opens)");
        std::process::exit(1);
    }
    let work = std::path::Path::new("/tmp/opencode/importlist");
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).unwrap();

    // Two library books: one matched by id, one by file name.
    let id_a = CrGuid::new_random();
    let id_b = CrGuid::new_random();
    let (mut lib, _) = cr_engine::library::Library::open_at_default_location().unwrap();
    lib.database_mut()
        .books
        .push(seed_book(id_a, "Batman", "/comics/Batman 001 (2015).cbz"));
    lib.database_mut().books.push(seed_book(
        id_b,
        "Daredevil",
        "/comics/Daredevil 005 (2014).cbz",
    ));
    lib.save().unwrap();
    cr_ui::library::initialize().expect("session init");

    // The `.cbl` under test: solved-by-id, solved-by-file-name,
    // one unsolved (the file name parses to Watchmen/1/1986).
    let id_a_text = id_a.to_string();
    let cbl = work.join("probe list.cbl");
    std::fs::write(
        &cbl,
        format!(
            r#"<?xml version="1.0"?>
<ReadingList xmlns:xsd="http://www.w3.org/2001/XMLSchema" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <Name>Probe List</Name>
  <Books>
    <Book>
      <Id>{id_a_text}</Id>
    </Book>
    <Book>
      <Id>00000000-0000-0000-0000-000000000000</Id>
      <FileName>Daredevil 005 (2014)</FileName>
    </Book>
    <Book Series="Watchmen" Number="1" Year="1986">
      <Id>00000000-0000-0000-0000-000000000000</Id>
      <FileName>/comics/Watchmen 001 (1986).cbz</FileName>
    </Book>
  </Books>
  <Matchers />
</ReadingList>"#
        ),
    )
    .unwrap();
    // The solved-only `.cbl` for gate D.
    let cbl2 = work.join("probe list2.cbl");
    std::fs::write(
        &cbl2,
        r#"<?xml version="1.0"?>
<ReadingList xmlns:xsd="http://www.w3.org/2001/XMLSchema" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <Name>Probe Solved Only</Name>
  <Books>
    <Book Series="No Such Series" Number="9">
      <Id>00000000-0000-0000-0000-000000000000</Id>
      <FileName>/comics/Ghost 009 (1998).cbz</FileName>
    </Book>
  </Books>
  <Matchers />
</ReadingList>"#,
    )
    .unwrap();
    // The smart-list `.cbl` for gate E.
    let smart_cbl = work.join("probe smart.cbl");
    std::fs::write(
        &smart_cbl,
        r#"<?xml version="1.0"?>
<ReadingList xmlns:xsd="http://www.w3.org/2001/XMLSchema" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <Name>Probe Smart</Name>
  <Books />
  <Matchers>
    <ComicBookMatcher xsi:type="ComicBookSeriesMatcher">
      <MatchValue>Batman</MatchValue>
    </ComicBookMatcher>
  </Matchers>
</ReadingList>"#,
    )
    .unwrap();

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.importlist-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        // The dwell window is an ApplicationWindow (the probe lesson:
        // a plain Window holds nothing and the loop exits).
        let window = gtk4::ApplicationWindow::builder()
            .application(app)
            .title("importlist probe")
            .build();
        window.present();
        let nav = cr_ui::browser::navigator::Navigator::new();
        window.set_child(Some(nav.widget()));
        // Keep the navigator handle alive for the whole run.
        std::mem::forget(nav.clone());

        let results: std::rc::Rc<std::cell::RefCell<Vec<String>>> =
            std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));

        // A/B/C. The id-list import with a missing book → the
        // question dialog; answer "Add missing Books to Library".
        {
            let results = results.clone();
            let nav2 = nav.clone();
            let cbl = cbl.clone();
            let window = window.clone();
            glib::timeout_add_local(std::time::Duration::from_millis(300), move || {
                let results = results.clone();
                cr_ui::dialogs::import_list::import_list_file(
                    &window,
                    &cbl,
                    None,
                    &nav2,
                    move |id| {
                        results
                            .borrow_mut()
                            .push(format!("done id-set={}", id.is_some()));
                    },
                );
                glib::ControlFlow::Break
            });
        }
        // Respond to the question dialog: Apply = add the missing.
        glib::timeout_add_local(std::time::Duration::from_millis(900), {
            let results = results.clone();
            let nav = nav.clone();
            move || {
                let dialog = find_toplevel("Import");
                let opened = dialog.is_some();
                if let Some(d) = dialog {
                    d.downcast::<Dialog>()
                        .unwrap()
                        .response(gtk4::ResponseType::Apply);
                }
                glib::timeout_add_local(std::time::Duration::from_millis(400), {
                    let results = results.clone();
                    let nav = nav.clone();
                    move || {
                        let lib = cr_ui::library::session();
                        let l = lib.borrow();
                        let book_count = l.database().books.len();
                        let watchmen = l.database().books.iter().find(|b| b.info.series == "Watchmen").map(|b| {
                            (b.info.series.clone(), b.info.number.clone(), b.info.year)
                        });
                        let all: Vec<String> = l
                            .database()
                            .books
                            .iter()
                            .map(|b| {
                                format!(
                                    "{}/{}/{}/{}",
                                    b.info.series, b.info.number, b.info.year, b.file_path
                                )
                            })
                            .collect();
                        drop(l);
                        let list = list_by_name("Probe List");
                        let temp = temporary_folder_exists();
                        let selected = nav.current_selection().map(|(id, _)| id);
                        let done = results.borrow().join(";");
                        println!(
                            "A/B/C question={opened} books-3={book_count} watchmen={watchmen:?} all={all:?} temp-folder={temp} list-solved-3={} temp-selected={} done={done}",
                            list.as_ref().is_some_and(|(_, ids, _)| ids.len() == 3),
                            selected.is_some_and(|s| list.as_ref().is_some_and(|(id, ..)| s == *id)),
                        );
                        glib::ControlFlow::Break
                    }
                });
                glib::ControlFlow::Break
            }
        });

        // D. The solved-only import: "Import" drops the unsolved id.
        glib::timeout_add_local(std::time::Duration::from_millis(1800), {
            let nav2 = nav.clone();
            let window = window.clone();
            let cbl2 = cbl2.clone();
            move || {
                cr_ui::dialogs::import_list::import_list_file(
                    &window,
                    &cbl2,
                    None,
                    &nav2,
                    |_| {},
                );
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(2400), {
            let results = results.clone();
            move || {
                let dialog = find_toplevel("Import");
                if let Some(d) = dialog {
                    d.downcast::<Dialog>()
                        .unwrap()
                        .response(gtk4::ResponseType::Ok);
                }
                glib::timeout_add_local(std::time::Duration::from_millis(400), {
                    let results = results.clone();
                    move || {
                        let lib = cr_ui::library::session();
                        let l = lib.borrow();
                        let book_count = l.database().books.len();
                        drop(l);
                        let list = list_by_name("Probe Solved Only");
                        println!(
                            "D books-still-3={book_count} solved-only-empty-ids={}",
                            list.as_ref().is_some_and(|(_, ids, _)| ids.is_empty())
                        );
                        results.borrow_mut().push("D ran".into());
                        glib::ControlFlow::Break
                    }
                });
                glib::ControlFlow::Break
            }
        });

        // E. The matchers-only `.cbl` lands as a smart list (no
        //    question dialog) and evaluates to the Batman book.
        glib::timeout_add_local(std::time::Duration::from_millis(3300), {
            let nav2 = nav.clone();
            let window = window.clone();
            let smart_cbl = smart_cbl.clone();
            move || {
                cr_ui::dialogs::import_list::import_list_file(
                    &window,
                    &smart_cbl,
                    None,
                    &nav2,
                    |_| {},
                );
                glib::ControlFlow::Break
            }
        });
        glib::timeout_add_local(std::time::Duration::from_millis(3700), {
            let results = results.clone();
            move || {
                let list = list_by_name("Probe Smart");
                let lib = cr_ui::library::session();
                let l = lib.borrow();
                let smart_books = list
                    .as_ref()
                    .map(|(id, ..)| {
                        let item =
                            cr_engine::lists::find_list_item(&l.database().comic_lists, id)
                                .expect("list");
                        cr_engine::lists::evaluate_list(&item, l.database()).len()
                    })
                    .unwrap_or(0);
                println!("E smart-list={} evaluates-to-1={smart_books}", list.is_some());
                results.borrow_mut().push("E ran".into());
                glib::ControlFlow::Break
            }
        });

        glib::timeout_add_local(std::time::Duration::from_millis(4200), {
            let app = app.clone();
            move || {
                println!("PROBE COMPLETE");
                app.quit();
                glib::ControlFlow::Break
            }
        });
    });

    app.run();
}
