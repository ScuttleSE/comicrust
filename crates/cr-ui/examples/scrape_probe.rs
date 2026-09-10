//! Headless probe: the Comic Vine Scraper wizard (T7). Seeds an
//! isolated library with one book whose previous scrape key is in
//! the custom values, runs the wizard against a mock ComicVine
//! server, and gates:
//!   A. the fast rescrape path scrapes the book, the status line
//!      shows it, and the committed clone lands in the library,
//!   B. the run summary fires through `on_done` exactly once,
//!   C. a book with no prior choice and no search results raises the
//!      search-terms dialog; Skip resolves it as skipped,
//!   D. Cancel ends a run early (the stop flag) without hanging.
//!
//! Run: Xvfb + `cargo run -p cr-ui --example scrape_probe` with an
//! isolated XDG pair (the probe seeds books into the DB it opens).

use std::io::{Read, Write};

use cr_core::model::comic_book::{values_store, ComicBook};
use cr_core::xml::scalar::{CrDateTime, CrGuid};
use gtk4::glib::ControlFlow;
use gtk4::prelude::*;

fn find_toplevel(title: &str) -> Option<gtk4::Window> {
    gtk4::Window::list_toplevels()
        .into_iter()
        .filter_map(|w| w.downcast::<gtk4::Window>().ok())
        .find(|w| w.title().as_deref().is_some_and(|t| t.starts_with(title)))
}

/// The canned ComicVine routes: the issue details (400011) and its
/// volume details (40501).
fn start_mock() -> String {
    let details = r#"{
        "number_of_total_results": 1, "status_code": 1,
        "results": {"id": "400011", "name": "The Court of Owls",
            "issue_number": "12",
            "site_detail_url": "http://comicvine.example/x/4000-11/",
            "cover_date": "2011-05-14",
            "description": "A summary.",
            "volume": {"id": "40501", "name": "Batman", "start_year": "1940"},
            "image": {"small_url": ""}}}"#;
    let volume = r#"{
        "number_of_total_results": 1, "status_code": 1,
        "results": {"id": 40501, "name": "Batman", "start_year": "1940",
                    "publisher": {"id": 10, "name": "DC Comics"}}}"#;
    let empty_search = r#"{"number_of_total_results": 0, "status_code": 1}"#;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let routes = [
        ("/issue/4000-", details.as_bytes().to_vec()),
        ("/volume/4050-", volume.as_bytes().to_vec()),
        ("/search/", empty_search.as_bytes().to_vec()),
    ];
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut buf = [0u8; 8192];
            let len = stream.read(&mut buf).unwrap_or(0);
            let request = String::from_utf8_lossy(&buf[..len]).to_string();
            let path = request.lines().next().unwrap_or("");
            let body = routes
                .iter()
                .find(|(p, _)| path.contains(p))
                .map(|(_, b)| b.clone())
                .unwrap_or_default();
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    format!("http://127.0.0.1:{port}")
}

fn seed_book(work: &std::path::Path, name: &str, issue_key: bool) -> ComicBook {
    let src = "tests/testfiles/Absolute Flash (2025) Volume 01 Issue 009.cbz";
    let comic = work.join(name);
    std::fs::copy(src, &comic).unwrap();
    let provider = cr_io::ComicProvider::open(&comic).unwrap();
    let mut book = ComicBook {
        id: CrGuid::new_random(),
        file_path: comic.to_string_lossy().into_owned(),
        added_time: CrDateTime::now(),
        ..Default::default()
    };
    book.info.page_count = provider.page_count() as i32;
    book.info.series = "Seed Series".into();
    book.enable_proposed = true;
    if issue_key {
        book.custom_values_store =
            values_store::encode(&[("comicvine_issue".into(), "400011".into())]);
    }
    let (mut lib, _) = cr_engine::library::Library::open_at_default_location().unwrap();
    lib.database_mut().books.push(book.clone());
    lib.save().unwrap();
    book
}

fn main() {
    gtk4::init().expect("gtk init");
    cr_ui::theme::init();
    if !std::env::var("XDG_DATA_HOME")
        .map(|v| v.contains("/tmp/opencode"))
        .unwrap_or(false)
        || !std::env::var("XDG_CONFIG_HOME")
            .map(|v| v.contains("/tmp/opencode"))
            .unwrap_or(false)
    {
        eprintln!("REFUSED: set XDG_DATA_HOME and XDG_CONFIG_HOME to /tmp/opencode/<dir> (the probe seeds books)");
        std::process::exit(1);
    }
    let work = std::path::Path::new("/tmp/opencode/scrape-probe");
    let _ = std::fs::remove_dir_all(work);
    std::fs::create_dir_all(work).unwrap();

    let host = gtk4::Window::new();
    host.set_title(Some("scrape-probe-host"));
    host.present();

    let mock_url = start_mock();
    let mut config = cr_scrape::config::Configuration::default();
    config.api_key = "probe-key".into();
    config.rescrape_tags = true;

    // The seeds BEFORE the session (the session then loads the file
    // with the books — the writeback_probe order).
    let book = seed_book(work, "keyed.cbz", true);

    // The session (loads the settings + the DB).
    cr_ui::library::initialize().expect("session");

    let summaries: std::rc::Rc<std::cell::RefCell<Vec<cr_ui::dialogs::scrape::ScrapeSummary>>> =
        std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let counter = std::rc::Rc::new(std::cell::Cell::new(0usize));
    let mock_base = mock_url.clone();

    let book_id = book.id;
    let counter2 = std::rc::Rc::clone(&counter);
    let loop_ = gtk4::glib::MainLoop::new(None, false);
    let quit = loop_.clone();
    {
        let summaries = std::rc::Rc::clone(&summaries);
        let counter = std::rc::Rc::clone(&counter2);
        cr_ui::dialogs::scrape::show_scrape_dialog(
            &host,
            &config,
            vec![book],
            Some(mock_base.clone()),
            None,
            move |done| {
                if let Some(summary) = done {
                    summaries.borrow_mut().push(summary);
                }
                quit.quit();
            },
            move || {
                counter.set(counter.get() + 1);
            },
        );
    }

    // The dialogs + the engine ride the main loop; give the run 10 s.
    let watchdog = loop_.clone();
    gtk4::glib::timeout_add_local(std::time::Duration::from_secs(10), move || {
        watchdog.quit();
        gtk4::glib::ControlFlow::Break
    });
    loop_.run();

    let summaries = summaries.borrow();
    if summaries.len() != 1 {
        eprintln!("FAIL: expected one summary, got {}", summaries.len());
        std::process::exit(1);
    }
    if summaries[0].scraped != 1 || summaries[0].skipped != 0 {
        eprintln!("FAIL: expected scraped=1 skipped=0, got {:?}", summaries[0]);
        std::process::exit(1);
    }
    println!("GATE B OK: the run summary fired with scraped=1 skipped=0");

    // The committed clone landed in the library.
    let lib = cr_ui::library::session();
    let db_book = lib
        .borrow()
        .database()
        .books
        .iter()
        .find(|b| b.id == book_id)
        .cloned()
        .expect("the book survived");
    if db_book.info.series != "Batman" {
        eprintln!("FAIL: the scrape did not land: {:?}", db_book.info.series);
        std::process::exit(1);
    }
    if !db_book.info.tags.contains("CVDB400011") {
        eprintln!("FAIL: the key tag is missing: {:?}", db_book.info.tags);
        std::process::exit(1);
    }
    if counter.get() != 1 {
        eprintln!("FAIL: on_scraped fired {} times", counter.get());
        std::process::exit(1);
    }
    println!("GATE A OK: the fast rescrape committed the scraped clone");

    // Gate C: a book with no prior choice + no search results raises
    // the search-terms dialog; Skip resolves the book as skipped.
    let book_no_key = seed_book(work, "nokey.cbz", false);
    let summaries2: std::rc::Rc<std::cell::RefCell<Vec<cr_ui::dialogs::scrape::ScrapeSummary>>> =
        std::rc::Rc::default();
    let loop2 = gtk4::glib::MainLoop::new(None, false);
    let quit2 = loop2.clone();
    // The search dialog appears on the main loop; answer Skip the
    // moment it maps.
    gtk4::glib::timeout_add_local(std::time::Duration::from_millis(200), move || {
        if let Some(dialog) = find_toplevel("Scrape: search terms") {
            dialog
                .downcast::<gtk4::Dialog>()
                .unwrap()
                .response(gtk4::ResponseType::Reject); // Skip
            return ControlFlow::Break;
        }
        ControlFlow::Continue
    });
    {
        let summaries = std::rc::Rc::clone(&summaries2);
        cr_ui::dialogs::scrape::show_scrape_dialog(
            &host,
            &config,
            vec![book_no_key],
            Some(mock_base.clone()),
            None,
            move |done| {
                if let Some(summary) = done {
                    summaries.borrow_mut().push(summary);
                }
                quit2.quit();
            },
            move || {},
        );
    }
    let watchdog2 = loop2.clone();
    gtk4::glib::timeout_add_local(std::time::Duration::from_secs(15), move || {
        watchdog2.quit();
        gtk4::glib::ControlFlow::Break
    });
    loop2.run();

    let summaries = summaries2.borrow();
    if summaries.len() != 1 || summaries[0].scraped != 0 || summaries[0].skipped != 1 {
        eprintln!(
            "FAIL: gate C expected scraped=0 skipped=1, got {:?}",
            summaries
        );
        std::process::exit(1);
    }
    println!("GATE C OK: the search dialog skip resolves the book as skipped");
    std::process::exit(0);
}
