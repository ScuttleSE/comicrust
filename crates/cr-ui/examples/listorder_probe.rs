//! Headless gate: an imported reading list displays in its STORED
//! order in the real grid (the C# `OnGetBooks` list-order parity).
//! Seeds an isolated library in deliberately shuffled order, imports
//! a 4-item `.cbl` (all books solved — no question dialog), selects
//! it, and gates the ItemView's display order. This caught the
//! 2026-09-06 bug: the unsorted view fell back to Guid order and
//! shuffled every imported list.
//! Run with an isolated XDG: XDG_DATA_HOME=/tmp/opencode/... cargo
//! run -p cr-ui --example listorder_probe
use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::{CrDateTime, CrGuid};
use gtk4::glib;
use gtk4::prelude::*;

/// (series, number) in the intended `.cbl` file order.
const LIST_ITEMS: &[(&str, &str)] = &[
    ("The Amazing Spider-Man", "296"),
    ("The Amazing Spider-Man", "297"),
    ("Web of Spider-Man", "35"),
    ("Web of Spider-Man", "36"),
];

fn seed_book(series: &str, number: &str, file: &str) -> ComicBook {
    let mut book = ComicBook {
        id: CrGuid::new_random(),
        file_path: file.into(),
        added_time: CrDateTime::now(),
        ..Default::default()
    };
    book.info.series = series.into();
    book.info.number = number.into();
    book
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
    let work = std::path::Path::new("/tmp/opencode/listorder");
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).unwrap();

    // Seed the library in a DELIBERATELY SHUFFLED order (the list
    // order must win over the library order).
    let (mut lib, _) = cr_engine::library::Library::open_at_default_location().unwrap();
    lib.database_mut()
        .books
        .push(seed_book("Web of Spider-Man", "36", "/comics/wosm 036.cbz"));
    lib.database_mut().books.push(seed_book(
        "The Amazing Spider-Man",
        "297",
        "/comics/asm 297.cbz",
    ));
    lib.database_mut()
        .books
        .push(seed_book("Web of Spider-Man", "35", "/comics/wosm 035.cbz"));
    lib.database_mut().books.push(seed_book(
        "The Amazing Spider-Man",
        "296",
        "/comics/asm 296.cbz",
    ));
    lib.save().unwrap();
    cr_ui::library::initialize().expect("session init");

    // The `.cbl` in the intended order; every book resolves by
    // series+number → no missing-books dialog.
    let items: Vec<String> = LIST_ITEMS
        .iter()
        .map(|(s, n)| {
            format!(
                r#"    <Book Series="{}" Number="{}">
      <Id>00000000-0000-0000-0000-000000000000</Id>
    </Book>"#,
                s, n
            )
        })
        .collect();
    let cbl = work.join("order probe.cbl");
    std::fs::write(
        &cbl,
        format!(
            r#"<?xml version="1.0"?>
<ReadingList xmlns:xsd="http://www.w3.org/2001/XMLSchema" xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <Name>Order Probe</Name>
  <Books>
{}
  </Books>
  <Matchers />
</ReadingList>"#,
            items.join("\n")
        ),
    )
    .unwrap();

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.listorder-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();

    app.connect_activate(move |app| {
        let app = app.clone();
        let (window, shell) = cr_ui::browser::shell::BrowserShell::create(&app);
        window.present();
        std::mem::forget(shell.clone());

        // Import through the real flow (lands in Temporary Lists,
        // auto-selected by the flow).
        glib::timeout_add_local(std::time::Duration::from_millis(400), {
            let shell = shell.clone();
            let cbl = cbl.clone();
            let window = window.clone();
            move || {
                let nav = shell.navigator();
                cr_ui::dialogs::import_list::import_list_file(&window, &cbl, None, &nav, |_| {});
                glib::ControlFlow::Break
            }
        });

        // Past the import + the selection debounce: read the grid.
        glib::timeout_add_local(std::time::Duration::from_millis(1400), move || {
            let view = shell.item_view_state();
            let n = view.len();
            let names: Vec<String> = (0..n)
                .map(|i| {
                    let b = view.book(i);
                    format!("{} #{}", b.info.series, b.info.number)
                })
                .collect();
            let expected: Vec<String> = LIST_ITEMS
                .iter()
                .map(|(s, n)| format!("{s} #{n}"))
                .collect();
            let order_ok = names == expected;
            println!("grid: {n} books");
            for name in &names {
                println!("  {name}");
            }
            println!("order-ok={order_ok}");
            println!("PROBE COMPLETE");
            app.quit();
            glib::ControlFlow::Break
        });
    });

    app.run();
}
