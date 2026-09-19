//! Release probe for Missing Issues > Find in Incoming (ADR-061).
//!
//! Run under Xvfb with isolated XDG_DATA_HOME and XDG_CONFIG_HOME paths.

use std::path::{Path, PathBuf};

use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::{CrDateTime, CrGuid};
use cr_engine::incoming::{IncomingCatalog, IncomingConfig};
use cr_organize::profile::{PluginSettings, Profile, MODE_MOVE};
use cr_scrape::cache::{CvCache, IssueSkeleton, SqliteCache};
use gtk4::glib;
use gtk4::prelude::*;

type Shell = cr_ui::browser::shell::BrowserShell;

fn isolated(name: &str) -> PathBuf {
    let path = PathBuf::from(std::env::var_os(name).expect("set isolated XDG path"));
    assert!(path.starts_with("/tmp/opencode/"));
    path
}

fn book(id: u8, path: &Path, number: &str) -> ComicBook {
    let mut book = ComicBook {
        id: CrGuid::parse(&format!("00000000-0000-0000-0000-{id:012}")).unwrap(),
        file_path: path.to_string_lossy().into_owned(),
        added_time: CrDateTime::now(),
        ..Default::default()
    };
    book.info.series = "Alpha".into();
    book.info.volume = 2020;
    book.info.number = number.into();
    book
}

fn write_cbz(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = std::fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file("000.png", zip::write::SimpleFileOptions::default())
        .unwrap();
    std::io::Write::write_all(&mut zip, include_bytes!("../assets/icons/List.png")).unwrap();
    zip.finish().unwrap();
}

fn find_dialog(title: &str) -> Option<gtk4::Dialog> {
    gtk4::Window::list_toplevels()
        .into_iter()
        .filter_map(|window| window.downcast::<gtk4::Window>().ok())
        .find(|window| window.title().as_deref() == Some(title))
        .and_then(|window| window.downcast::<gtk4::Dialog>().ok())
}

fn dialog_labels(dialog: &gtk4::Dialog) -> Vec<String> {
    fn walk(widget: &gtk4::Widget, labels: &mut Vec<String>) {
        if let Ok(label) = widget.clone().downcast::<gtk4::Label>() {
            labels.push(label.text().to_string());
        }
        let mut child = widget.first_child();
        while let Some(widget) = child {
            walk(&widget, labels);
            child = widget.next_sibling();
        }
    }
    let mut labels = Vec::new();
    if let Some(child) = dialog.child() {
        walk(&child, &mut labels);
    }
    labels
}

fn find_report(heading: &str) -> Option<gtk4::Dialog> {
    gtk4::Window::list_toplevels()
        .into_iter()
        .filter_map(|window| window.downcast::<gtk4::Dialog>().ok())
        .find(|dialog| dialog_labels(dialog).iter().any(|text| text == heading))
}

fn wait_for_grid(shell: Shell, expected: usize, then: impl FnOnce(Shell) + 'static, tick: u32) {
    if shell.state_grid_book_count() == expected {
        then(shell);
        return;
    }
    assert!(tick < 200, "grid did not reach {expected} rows");
    let mut then = Some(then);
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        wait_for_grid(shell.clone(), expected, then.take().unwrap(), tick + 1);
        glib::ControlFlow::Break
    });
}

fn wait_for_preview(shell: Shell, incoming_id: CrGuid, incoming_path: PathBuf, tick: u32) {
    if let Some(dialog) = find_report("Preview Adoption") {
        assert!(incoming_path.exists(), "preview moved the Incoming file");
        assert_eq!(cr_ui::library::incoming_books_snapshot().len(), 2);
        assert_eq!(cr_ui::library::session().borrow().database().books.len(), 1);
        dialog.response(gtk4::ResponseType::Ok);
        shell.state_select_books(&[incoming_id]);
        shell.state_run_incoming_adoption(false);
        wait_for_adopt_confirmation(shell, 0);
        return;
    }
    assert!(
        find_dialog("Library Organizer — Profiles").is_none(),
        "Gap Fills preview opened the general profile selector"
    );
    if tick >= 200 {
        let titles: Vec<String> = gtk4::Window::list_toplevels()
            .into_iter()
            .filter_map(|window| window.downcast::<gtk4::Window>().ok())
            .filter_map(|window| window.title().map(|title| title.to_string()))
            .collect();
        panic!(
            "Gap Fills preview report did not appear; selection={} operation_active={} windows={titles:?}",
            shell.state_selection_len(),
            cr_engine::incoming_transaction::operation_active()
        );
    }
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        wait_for_preview(shell.clone(), incoming_id, incoming_path.clone(), tick + 1);
        glib::ControlFlow::Break
    });
}

fn wait_for_adopt_confirmation(shell: Shell, tick: u32) {
    if let Some(dialog) = find_dialog("Gap Fill Adoption") {
        assert!(
            find_dialog("Library Organizer — Profiles").is_none(),
            "Gap Fills adoption opened the general profile selector"
        );
        dialog.response(gtk4::ResponseType::Cancel);
        shell.state_select_list(&cr_ui::browser::navigator::missing_issues_id());
        glib::timeout_add_local(std::time::Duration::from_millis(400), move || {
            shell.state_missing_issues_refresh();
            wait_for_missing(
                shell.clone(),
                1,
                |shell| {
                    let row = cr_ui::library::missing_issues_snapshot().remove(0);
                    shell.state_select_books(&[row.id]);
                    shell.state_find_in_incoming();
                    wait_for_find_dialog(shell, 0);
                },
                0,
            );
            glib::ControlFlow::Break
        });
        return;
    }
    assert!(tick < 100, "Gap Fill adoption confirmation did not appear");
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        wait_for_adopt_confirmation(shell.clone(), tick + 1);
        glib::ControlFlow::Break
    });
}

fn wait_for_missing(shell: Shell, expected: usize, then: impl FnOnce(Shell) + 'static, tick: u32) {
    if shell.state_grid_book_count() == expected {
        then(shell);
        return;
    }
    assert!(tick < 200, "Missing Issues refresh timed out");
    let mut then = Some(then);
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        wait_for_missing(shell.clone(), expected, then.take().unwrap(), tick + 1);
        glib::ControlFlow::Break
    });
}

fn wait_for_find_dialog(shell: Shell, tick: u32) {
    if let Some(dialog) = find_dialog("Find in Incoming") {
        let (labels, combos) = {
            fn walk(
                widget: &gtk4::Widget,
                labels: &mut Vec<String>,
                combos: &mut Vec<gtk4::ComboBoxText>,
            ) {
                if let Ok(label) = widget.clone().downcast::<gtk4::Label>() {
                    labels.push(label.text().to_string());
                }
                if let Ok(combo) = widget.clone().downcast::<gtk4::ComboBoxText>() {
                    combos.push(combo);
                }
                let mut child = widget.first_child();
                while let Some(widget) = child {
                    walk(&widget, labels, combos);
                    child = widget.next_sibling();
                }
            }
            let mut labels = Vec::new();
            let mut combos = Vec::new();
            if let Some(child) = dialog.child() {
                walk(&child, &mut labels, &mut combos);
            }
            (labels, combos)
        };
        assert!(labels
            .iter()
            .any(|text| text.contains("1 issue(s) need a selection")));
        assert!(labels.iter().any(|text| text.contains("Alpha v2020 #2")));
        assert_eq!(combos.len(), 1);
        combos[0].set_active(Some(1));
        dialog.response(gtk4::ResponseType::Ok);
        wait_for_adoption(shell, 0);
        return;
    }
    assert!(tick < 100, "Find in Incoming dialog did not appear");
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        wait_for_find_dialog(shell.clone(), tick + 1);
        glib::ControlFlow::Break
    });
}

fn wait_for_adoption(shell: Shell, tick: u32) {
    if !cr_engine::incoming_transaction::operation_active()
        && cr_ui::library::incoming_books_snapshot().len() == 1
        && cr_ui::library::session().borrow().database().books.len() == 2
    {
        wait_for_missing(shell, 0, finish, 0);
        return;
    }
    assert!(tick < 400, "Incoming adoption timed out");
    glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
        wait_for_adoption(shell.clone(), tick + 1);
        glib::ControlFlow::Break
    });
}

fn finish(_shell: Shell) {
    assert_eq!(cr_ui::library::incoming_books_snapshot().len(), 1);
    assert_eq!(cr_ui::library::session().borrow().database().books.len(), 2);
    println!("FIND IN INCOMING PROBE OK: matched, confirmed, adopted, and refreshed");
    std::process::exit(0);
}

fn main() {
    let data = isolated("XDG_DATA_HOME");
    let config = isolated("XDG_CONFIG_HOME");
    let work = data.join("find-in-incoming-work");
    std::fs::create_dir_all(&work).unwrap();
    std::fs::create_dir_all(&config).unwrap();

    let incoming_path = work.join("incoming/Alpha 002.cbz");
    let alternate_path = work.join("incoming-alternate/Alpha 002.cbz");
    let destination = work.join("library");
    write_cbz(&incoming_path);
    write_cbz(&alternate_path);

    let cache = SqliteCache::open(&cr_scrape::cache::default_cache_path()).unwrap();
    cache
        .put_issues(&[
            IssueSkeleton {
                issue_id: 501001,
                volume_id: 501,
                issue_number: "1".into(),
                ..Default::default()
            },
            IssueSkeleton {
                issue_id: 501002,
                volume_id: 501,
                issue_number: "2".into(),
                ..Default::default()
            },
        ])
        .unwrap();

    let (mut library, _) = cr_engine::library::Library::open_at_default_location().unwrap();
    let mut owned = book(1, &destination.join("Alpha 001.cbz"), "1");
    cr_scrape::bookdata::set_custom_value(&mut owned, "comicvine_volume", "501");
    library.database_mut().books.push(owned);
    library.save().unwrap();

    gtk4::init().unwrap();
    cr_ui::theme::init();
    cr_ui::library::initialize().unwrap();
    let incoming = book(2, &incoming_path, "002");
    let alternate = book(3, &alternate_path, "2");
    let catalog = IncomingCatalog {
        books: vec![incoming, alternate],
    };
    catalog.save(&cr_core::paths::Paths::new_default()).unwrap();
    cr_ui::library::replace_incoming_catalog(catalog);

    let mut profile = Profile::builtin_default();
    profile.name = "Find Missing".into();
    profile.mode = MODE_MOVE.into();
    profile.base_folder = destination.to_string_lossy().into_owned();
    profile.folder_template.clear();
    profile.file_template = "{<series>}{ #<number>}".into();
    cr_ui::library::store_organize_settings(&PluginSettings {
        last_used: vec![profile.name.clone()],
        profiles: vec![profile],
    });
    cr_ui::library::store_incoming_config(&IncomingConfig {
        incoming_folders: vec![work.join("incoming").to_string_lossy().into_owned()],
        find_in_incoming_profile: "Find Missing".into(),
        ..Default::default()
    });

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.find-in-incoming-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(move |app| {
        let (window, shell) = cr_ui::browser::shell::BrowserShell::create(app);
        window.present();
        std::mem::forget(shell.clone());
        shell.state_select_list(&cr_ui::browser::navigator::IncomingView::GapFills.id());
        let incoming_path = incoming_path.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(400), move || {
            let incoming_path = incoming_path.clone();
            wait_for_grid(
                shell.clone(),
                2,
                move |shell| {
                    let incoming_id =
                        CrGuid::parse("00000000-0000-0000-0000-000000000002").unwrap();
                    shell.state_select_books(&[incoming_id]);
                    shell.state_run_incoming_adoption(true);
                    wait_for_preview(shell, incoming_id, incoming_path, 0);
                },
                0,
            );
            glib::ControlFlow::Break
        });
    });
    app.run();
}
