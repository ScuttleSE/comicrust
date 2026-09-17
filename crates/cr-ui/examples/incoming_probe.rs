//! Release probe for the separate Incoming catalog and its browser views.
//!
//! Run under Xvfb with `XDG_DATA_HOME`, `XDG_CONFIG_HOME`, and
//! `INCOMING_PROBE_WORK` set to separate directories below `/tmp/opencode`.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::AtomicBool;

use cr_core::database::comic_database::ComicDatabase;
use cr_core::database::list_items::{ComicListItem, IdListItem};
use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::CrGuid;
use cr_engine::incoming::{IncomingCatalog, IncomingConfig};
use cr_engine::incoming_transaction::{
    FileSnapshot, IncomingTransaction, TransactionEngine, TransactionFiles, TransactionKind,
    TransactionStage,
};
use cr_organize::engine::{
    Apply, CoverSource, DuplicateAction, DuplicateAnswer, DuplicateAsk, LogEntry, MoveLanding,
    OrganizeUi, RunContext,
};
use cr_organize::profile::{Profile, MODE_MOVE, MODE_SIMULATE};
use cr_organize::template::{MultiValueAnswer, MultiValueAsk, MultiValueAsker};
use gtk4::glib::{self, ControlFlow};
use gtk4::prelude::*;

const ROOT: &str = "/tmp/opencode";

fn isolated_path(name: &str) -> PathBuf {
    let value = std::env::var_os(name).unwrap_or_else(|| {
        eprintln!("REFUSED: {name} is not set");
        std::process::exit(1);
    });
    let path = PathBuf::from(value);
    let valid_components = path
        .components()
        .all(|part| !matches!(part, Component::ParentDir | Component::CurDir));
    if !path.is_absolute()
        || !valid_components
        || path == Path::new(ROOT)
        || !path.starts_with(ROOT)
    {
        eprintln!("REFUSED: {name} must be a strict descendant of {ROOT}");
        std::process::exit(1);
    }
    path
}

fn fixed_id(last: u8) -> CrGuid {
    CrGuid::parse(&format!("00000000-0000-0000-0000-{last:012}")).unwrap()
}

fn book(id: u8, path: &Path, series: &str, number: &str) -> ComicBook {
    let mut book = ComicBook {
        id: fixed_id(id),
        file_path: path.to_string_lossy().into_owned(),
        enable_proposed: false,
        ..ComicBook::default()
    };
    book.info.series = series.into();
    book.info.number = number.into();
    book.info.volume = 1;
    book.info.format = "Digital".into();
    book.info.language_iso = "en".into();
    book
}

fn write_cbz(path: &Path, series: &str, number: &str) {
    let file = std::fs::File::create(path).expect("create CBZ");
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file("000.jpg", zip::write::SimpleFileOptions::default())
        .expect("start page");
    std::io::Write::write_all(&mut zip, &[0xff, 0xd8, 0xff, 0xd9]).expect("write page");
    zip.start_file("ComicInfo.xml", zip::write::SimpleFileOptions::default())
        .expect("start metadata");
    let metadata = format!(
        "<?xml version=\"1.0\"?>\r\n<ComicInfo><Series>{series}</Series><Number>{number}</Number><Volume>1</Volume><Format>Digital</Format><LanguageISO>en</LanguageISO></ComicInfo>"
    );
    std::io::Write::write_all(&mut zip, metadata.as_bytes()).expect("write metadata");
    zip.finish().expect("finish CBZ");
}

fn snapshot(path: PathBuf, after: Vec<u8>) -> FileSnapshot {
    FileSnapshot {
        before: std::fs::read(&path).ok(),
        path,
        after,
        remove_after: false,
    }
}

fn id_set<'a>(
    books: &'a [ComicBook],
    classes: impl Iterator<Item = &'a cr_engine::incoming::IncomingClassification>,
) -> BTreeSet<String> {
    classes
        .map(|class| books[class.index].id.to_d_string())
        .collect()
}

struct ProbeUi;

impl MultiValueAsker for ProbeUi {
    fn ask_multi_value(&mut self, _ask: MultiValueAsk) -> MultiValueAnswer {
        MultiValueAnswer::default()
    }
}

impl OrganizeUi for ProbeUi {
    fn ask_duplicate(&mut self, _ask: DuplicateAsk) -> DuplicateAnswer {
        DuplicateAnswer {
            action: DuplicateAction::Cancel,
            always: false,
        }
    }

    fn log(&mut self, _entry: LogEntry) {}

    fn progress(&mut self, _done: usize, _total: usize) {}
}

struct ProbeCover;

impl CoverSource for ProbeCover {
    fn fileless_cover(&self, _book: &ComicBook) -> Option<cr_image::Image> {
        None
    }

    fn duplicate_cover(&self, _book: &ComicBook) -> Option<Vec<u8>> {
        None
    }
}

fn main() {
    let data = isolated_path("XDG_DATA_HOME");
    let config = isolated_path("XDG_CONFIG_HOME");
    let work = isolated_path("INCOMING_PROBE_WORK");
    if data == config || data == work || config == work {
        eprintln!("REFUSED: XDG and work paths must be separate");
        std::process::exit(1);
    }
    for path in [&data, &config, &work] {
        let _ = std::fs::remove_dir_all(path);
        std::fs::create_dir_all(path).expect("create isolated probe directory");
    }

    let incoming_root = work.join("incoming");
    let library_root = work.join("library");
    std::fs::create_dir_all(&incoming_root).expect("create Incoming root");
    std::fs::create_dir_all(&library_root).expect("create library root");
    let scanned_path = incoming_root.join("Scanned 001.cbz");
    write_cbz(&scanned_path, "Scanned", "1");

    let paths = Rc::new(cr_core::paths::Paths::from_roots(&data, &config));
    let database_path = cr_core::paths::database_file(&paths);
    let seed_database = ComicDatabase {
        books: vec![
            book(101, &library_root.join("Alpha 001.cbz"), "Alpha", "1"),
            book(103, &library_root.join("Alpha 003.cbz"), "Alpha", "3"),
        ],
        ..Default::default()
    };
    cr_core::database::comic_database::save(&seed_database, &database_path)
        .expect("seed ComicDb.xml");
    let comic_db_before_scan = std::fs::read(&database_path).expect("read seeded ComicDb.xml");
    println!("GATE A OK: isolated paths, valid CBZ seed, and fixed IDs are ready");

    gtk4::init().expect("GTK init");
    cr_ui::theme::init();
    cr_ui::library::initialize().expect("library session");
    let incoming_config = IncomingConfig {
        incoming_folders: vec![incoming_root.to_string_lossy().into_owned()],
        last_organizer_profile: "Probe Move".into(),
    };
    cr_ui::library::store_incoming_config(&incoming_config);
    assert_eq!(cr_ui::library::incoming_config(), incoming_config);
    let config_path = cr_core::paths::config_file(&paths);
    let config_text = std::fs::read_to_string(&config_path).expect("read config");
    assert!(config_text.contains("[plugins.incoming]"));
    assert!(config_text.contains("last_organizer_profile = \"Probe Move\""));
    println!("GATE C OK: Incoming roots and remembered profile round-trip in config");

    let app = gtk4::Application::builder()
        .application_id("org.comicrust.incoming-probe")
        .flags(gtk4::gio::ApplicationFlags::NON_UNIQUE)
        .build();
    let paths_for_activate = Rc::clone(&paths);
    app.connect_activate(move |app| {
        let (window, shell) = cr_ui::browser::shell::BrowserShell::create(app);
        window.present();
        let navigator = shell.navigator();
        let names: Vec<String> = navigator
            .row_labels()
            .into_iter()
            .map(|(name, _)| name)
            .collect();
        let incoming_index = names
            .iter()
            .position(|name| name == "Incoming")
            .expect("Incoming navigator root");
        assert_eq!(
            &names[incoming_index..incoming_index + 6],
            [
                "Incoming",
                "All",
                "Gap Fills",
                "Duplicates",
                "New Series",
                "Needs Review",
            ]
        );
        shell.state_select_list(&cr_ui::browser::navigator::IncomingView::All.id());
        assert!(shell
            .nav_expanded_dump()
            .iter()
            .any(|(name, expanded)| name == "Incoming" && *expanded));
        println!("GATE B OK: Incoming hierarchy order is correct and selection expands its root");

        let watchdog = window.clone();
        glib::timeout_add_local(std::time::Duration::from_secs(45), move || {
            eprintln!("FAIL WATCHDOG: Incoming probe exceeded 45 seconds");
            watchdog.close();
            std::process::exit(2);
        });

        let shell = Rc::new(shell);
        let scanned_path = scanned_path.clone();
        let comic_db_before_scan = comic_db_before_scan.clone();
        let work = work.clone();
        let incoming_root_for_scan = incoming_root.clone();
        cr_ui::library::add_folder_to_library(&incoming_root, {
            let shell = Rc::clone(&shell);
            let paths = Rc::clone(&paths_for_activate);
            move |result| {
                assert_eq!(result.added.len(), 1, "Incoming scan added count");
                let live = cr_ui::library::incoming_books_snapshot();
                assert_eq!(live.len(), 1);
                assert_eq!(live[0].file_path, scanned_path.to_string_lossy());
                let persisted = IncomingCatalog::load(&paths).expect("load Incoming after scan");
                assert_eq!(persisted.books.len(), 1);
                assert_eq!(persisted.books[0].id, live[0].id);
                assert_eq!(persisted.books[0].file_path, live[0].file_path);
                assert_eq!(persisted.books[0].info, live[0].info);
                assert_eq!(persisted.books[0].file_size, live[0].file_size);
                assert_eq!(
                    persisted.to_bytes().unwrap(),
                    IncomingCatalog {
                        books: live.clone(),
                    }
                    .to_bytes()
                    .unwrap(),
                    "persisted catalog matches the XML-compatible live record"
                );
                assert!(!cr_ui::library::session()
                    .borrow()
                    .database()
                    .books
                    .iter()
                    .any(|book| book.file_path == scanned_path.to_string_lossy()));
                let persisted_main =
                    cr_core::database::comic_database::load(&cr_core::paths::database_file(&paths))
                        .expect("load main database after scan");
                assert!(!persisted_main
                    .books
                    .iter()
                    .any(|book| book.file_path == scanned_path.to_string_lossy()));
                assert_eq!(
                    std::fs::read(cr_core::paths::database_file(&paths)).unwrap(),
                    comic_db_before_scan
                );
                println!("GATE D OK: scan routed only to persisted and live Incoming state");
                start_classification(shell, paths, work.clone(), incoming_root_for_scan.clone());
            }
        });
    });
    let _ = app.run();
}

fn start_classification(
    shell: Rc<cr_ui::browser::shell::BrowserShell>,
    paths: Rc<cr_core::paths::Paths>,
    work: PathBuf,
    incoming_root: PathBuf,
) {
    let fixtures = [
        (1, "Alpha", "1"),
        (2, "Alpha", "2"),
        (3, "Beta", "1"),
        (4, "Alpha", "9"),
        (5, "Beta", "1"),
    ];
    let books: Vec<ComicBook> = fixtures
        .iter()
        .map(|(id, series, number)| {
            let path = incoming_root.join(format!("{series}-{number}-{id}.cbz"));
            write_cbz(&path, series, number);
            book(*id, &path, series, number)
        })
        .collect();
    cr_ui::library::save_incoming_catalog_async(
        IncomingCatalog {
            books: books.clone(),
        },
        move |r| {
            let catalog = r.expect("save classification catalog");
            cr_ui::library::replace_incoming_catalog(catalog);
            wait_for_classification(shell, paths, work, books, 0);
        },
    );
}

fn wait_for_classification(
    shell: Rc<cr_ui::browser::shell::BrowserShell>,
    paths: Rc<cr_core::paths::Paths>,
    work: PathBuf,
    books: Vec<ComicBook>,
    ticks: u32,
) {
    let Some(snapshot) = cr_ui::library::incoming_classification_snapshot() else {
        assert!(ticks < 100, "classification worker timed out");
        glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            wait_for_classification(
                shell.clone(),
                paths.clone(),
                work.clone(),
                books.clone(),
                ticks + 1,
            );
            ControlFlow::Break
        });
        return;
    };
    if snapshot.books != books {
        assert!(ticks < 100, "classification snapshot stayed stale");
        glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            wait_for_classification(
                shell.clone(),
                paths.clone(),
                work.clone(),
                books.clone(),
                ticks + 1,
            );
            ControlFlow::Break
        });
        return;
    }

    let classes = &snapshot.classifications;
    let ids = |predicate: fn(&cr_engine::incoming::IncomingClassification) -> bool| {
        id_set(&books, classes.iter().filter(|class| predicate(class)))
    };
    assert_eq!(ids(|class| class.gap_fill), set(&[2]));
    assert_eq!(ids(|class| class.duplicate), set(&[1, 3, 5]));
    assert_eq!(ids(|class| class.new_series), set(&[3, 5]));
    assert_eq!(ids(|class| class.needs_review), set(&[4]));

    let views = [
        (cr_ui::browser::navigator::IncomingView::All, 5usize),
        (cr_ui::browser::navigator::IncomingView::GapFills, 1),
        (cr_ui::browser::navigator::IncomingView::Duplicates, 3),
        (cr_ui::browser::navigator::IncomingView::NewSeries, 2),
        (cr_ui::browser::navigator::IncomingView::NeedsReview, 1),
    ];
    check_view(shell, paths, work, views, 0);
}

fn check_view(
    shell: Rc<cr_ui::browser::shell::BrowserShell>,
    paths: Rc<cr_core::paths::Paths>,
    work: PathBuf,
    views: [(cr_ui::browser::navigator::IncomingView, usize); 5],
    index: usize,
) {
    if index == views.len() {
        println!("GATE E OK: exact memberships and asynchronous UI projections match");
        gate_f_to_i(paths, work);
        println!("INCOMING PROBE DONE");
        std::process::exit(0);
    }
    let (view, expected) = views[index];
    shell.state_select_list(&view.id());
    glib::timeout_add_local(std::time::Duration::from_millis(350), move || {
        assert_eq!(
            shell.state_grid_book_count(),
            expected,
            "{} UI projection",
            view.name()
        );
        check_view(shell.clone(), paths.clone(), work.clone(), views, index + 1);
        ControlFlow::Break
    });
}

fn set(ids: &[u8]) -> BTreeSet<String> {
    ids.iter().map(|id| fixed_id(*id).to_d_string()).collect()
}

fn gate_f_to_i(paths: Rc<cr_core::paths::Paths>, work: PathBuf) {
    let conversion_root = work.join("convert");
    std::fs::create_dir_all(&conversion_root).expect("create conversion root");
    let conversion_path = conversion_root.join("Complete.cbz");
    write_cbz(&conversion_path, "Complete", "7");
    let mut complete = book(20, &conversion_path, "Complete", "7");
    complete.info.summary = "Complete conversion metadata".into();
    complete.info.tags = "incoming, probe".into();
    complete.book_notes = "Keep this note".into();
    complete.rating = 4.5;
    complete.last_page_read = 7;
    let list = ComicListItem::IdList(IdListItem {
        book_ids: vec![complete.id],
        ..Default::default()
    });
    let mut database = ComicDatabase {
        books: vec![complete.clone()],
        comic_lists: vec![list.clone()],
        ..Default::default()
    };
    let mut incoming = IncomingCatalog::default();
    let roles = [cr_engine::incoming::FolderRole {
        folder: conversion_root.to_string_lossy().into_owned(),
        incoming: true,
        watch: true,
    }];
    assert_eq!(
        cr_engine::incoming::transfer_new_incoming_records(
            &mut database,
            &mut incoming,
            &IncomingConfig::default(),
            &roles,
        ),
        1
    );
    assert!(database.books.is_empty());
    assert_eq!(incoming.books, [complete.clone()]);
    assert_eq!(database.comic_lists.as_slice(), std::slice::from_ref(&list));

    let final_config = IncomingConfig {
        incoming_folders: vec![conversion_root.to_string_lossy().into_owned()],
        last_organizer_profile: "Converted Move".into(),
    };
    let table = cr_core::settings::unified::serialize_plugin(&final_config).unwrap();
    let settings = cr_ui::library::settings();
    let config_bytes = cr_core::settings::unified::save_bytes_with_plugin_tables(
        &settings.borrow(),
        &[(cr_ui::library::INCOMING_PLUGIN.to_string(), table)],
    )
    .unwrap();
    let database_path = cr_core::paths::database_file(&paths);
    let incoming_path = cr_core::paths::incoming_file(&paths);
    let config_path = cr_core::paths::config_file(&paths);
    let mut transaction = IncomingTransaction {
        kind: TransactionKind::FolderConversion,
        stage: TransactionStage::Prepared,
        files: TransactionFiles {
            incoming_catalog: Some(snapshot(
                incoming_path.clone(),
                incoming.to_bytes().unwrap(),
            )),
            comic_database: Some(snapshot(
                database_path.clone(),
                cr_core::database::comic_database::save_bytes(&database).unwrap(),
            )),
            config: Some(snapshot(config_path.clone(), config_bytes)),
            auxiliary: Vec::new(),
        },
        external_actions: Vec::new(),
    };
    let engine = TransactionEngine::new(&paths);
    engine
        .begin(&transaction)
        .expect("begin conversion transaction");
    engine
        .commit(&mut transaction)
        .expect("commit conversion transaction");
    assert!(!engine.journal_path().exists());
    assert_eq!(
        IncomingCatalog::load(&paths).unwrap().books,
        [complete.clone()]
    );
    let converted_db = cr_core::database::comic_database::load(&database_path).unwrap();
    assert!(converted_db.books.is_empty());
    let [ComicListItem::IdList(converted_list)] = converted_db.comic_lists.as_slice() else {
        panic!("converted database did not preserve the ID list");
    };
    assert_eq!(converted_list.book_ids, [complete.id]);
    cr_core::settings::unified::load(&config_path);
    assert_eq!(
        cr_core::settings::unified::get_plugin::<IncomingConfig>(cr_ui::library::INCOMING_PLUGIN),
        Some(final_config.clone())
    );
    println!("GATE F OK: complete conversion and three-file transaction round-trip");

    gate_g(&paths, &work);

    assert!(!cr_engine::incoming_transaction::operation_active());
    let operation =
        cr_engine::incoming_transaction::try_begin_operation().expect("first operation");
    assert!(cr_engine::incoming_transaction::try_begin_operation().is_none());
    operation.finish(|_| {
        assert!(cr_engine::incoming_transaction::operation_active());
        assert!(cr_engine::incoming_transaction::try_begin_operation().is_none());
    });
    assert!(!cr_engine::incoming_transaction::operation_active());
    println!("GATE H OK: a second operation stays blocked through landing");

    let reloaded_incoming = IncomingCatalog::load(&paths).expect("final Incoming reload");
    let reloaded_database =
        cr_core::database::comic_database::load(&database_path).expect("final ComicDb reload");
    cr_core::settings::unified::load(&config_path);
    let reloaded_config: IncomingConfig =
        cr_core::settings::unified::get_plugin(cr_ui::library::INCOMING_PLUGIN)
            .expect("final Incoming config reload");
    assert_eq!(reloaded_incoming.books, [complete]);
    assert!(reloaded_database.books.is_empty());
    assert_eq!(reloaded_config, final_config);
    assert!(!engine.journal_path().exists());
    println!("GATE I OK: both catalogs and config reload after all operations");
}

fn gate_g(paths: &cr_core::paths::Paths, work: &Path) {
    let source = work.join("adopt-source.cbz");
    write_cbz(&source, "Adopted", "8");
    let mut original = book(30, &source, "Adopted", "8");
    original.info.summary = "Complete adoption metadata".into();
    original.info.tags = "reviewed".into();
    original.book_notes = "Adoption note".into();
    original.rating = 5.0;
    original.last_page_read = 8;
    let destination = work.join("adopt-library");
    let mut profile = Profile::builtin_default();
    profile.name = "Adopt Probe".into();
    profile.mode = MODE_MOVE.into();
    profile.base_folder = destination.to_string_lossy().into_owned();
    profile.folder_template = "{<series>}".into();
    profile.file_template = "{<series>}{ #<number>}".into();
    let books = [original.clone()];
    let selected = [0usize];
    let cancel = AtomicBool::new(false);
    let trash = |_path: &str| false;
    let mut ui = ProbeUi;
    let report = cr_organize::engine::organize(
        RunContext {
            books: &books,
            selected: &selected,
            profiles: std::slice::from_ref(&profile),
            move_landing: MoveLanding::InsertPreservingId,
            trash: &trash,
            filesystem_effects: None,
            cover: &ProbeCover,
            undo_path: None,
            cancel: &cancel,
        },
        &mut ui,
    );
    let [Apply::Adopt(adopted)] = report.applies.as_slice() else {
        panic!("MoveLanding::InsertPreservingId did not return one Adopt apply");
    };
    let mut expected = original;
    expected.file_path = adopted.file_path.clone();
    assert_eq!(adopted, &expected);
    assert_eq!(adopted.id, fixed_id(30));
    assert!(!source.exists());
    assert!(Path::new(&adopted.file_path).exists());

    let simulation_source = work.join("simulate-source.cbz");
    write_cbz(&simulation_source, "Simulated", "9");
    let simulation_book = book(31, &simulation_source, "Simulated", "9");
    let simulation_destination = work.join("simulate-library");
    let mut simulation_profile = profile;
    simulation_profile.name = "Simulation Probe".into();
    simulation_profile.mode = MODE_SIMULATE.into();
    simulation_profile.base_folder = simulation_destination.to_string_lossy().into_owned();
    let catalog_before = std::fs::read(cr_core::paths::incoming_file(paths)).unwrap();
    let simulation_books = [simulation_book];
    let simulation_profiles = [simulation_profile];
    let simulation = cr_organize::engine::organize(
        RunContext {
            books: &simulation_books,
            selected: &selected,
            profiles: &simulation_profiles,
            move_landing: MoveLanding::InsertPreservingId,
            trash: &trash,
            filesystem_effects: None,
            cover: &ProbeCover,
            undo_path: None,
            cancel: &cancel,
        },
        &mut ui,
    );
    assert!(simulation.applies.is_empty());
    assert!(simulation_source.exists());
    assert!(!simulation_destination.exists());
    assert_eq!(
        std::fs::read(cr_core::paths::incoming_file(paths)).unwrap(),
        catalog_before
    );
    assert!(!TransactionEngine::new(paths).journal_path().exists());
    println!("GATE G OK: adoption preserves the record and simulation changes no file or catalog");
}
