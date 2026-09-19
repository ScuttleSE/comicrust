//! Release probe for the separate Incoming catalog and its browser views.
//!
//! Run under Xvfb with `XDG_DATA_HOME`, `XDG_CONFIG_HOME`, and
//! `INCOMING_PROBE_WORK` set to separate directories below `/tmp/opencode`.

use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};
use std::rc::Rc;
use std::sync::atomic::AtomicBool;

use cr_core::database::comic_database::ComicDatabase;
use cr_core::database::list_items::{
    ComicBookMatcher, ComicListItem, IdListItem, ListItemBase, SmartListItem, ValueMatcher,
};
use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::CrGuid;
use cr_engine::incoming::{IncomingCatalog, IncomingConfig, IncomingLists};
use cr_engine::incoming_transaction::{
    FileSnapshot, IncomingTransaction, ReplacementTransaction, TransactionEngine, TransactionFiles,
    TransactionKind, TransactionStage,
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
    zip.start_file("000.png", zip::write::SimpleFileOptions::default())
        .expect("start page");
    std::io::Write::write_all(&mut zip, include_bytes!("../assets/icons/List.png"))
        .expect("write page");
    zip.start_file("ComicInfo.xml", zip::write::SimpleFileOptions::default())
        .expect("start metadata");
    let metadata = format!(
        "<?xml version=\"1.0\"?>\r\n<ComicInfo><Series>{series}</Series><Number>{number}</Number><Volume>1</Volume><Format>Digital</Format><LanguageISO>en</LanguageISO></ComicInfo>"
    );
    std::io::Write::write_all(&mut zip, metadata.as_bytes()).expect("write metadata");
    zip.finish().expect("finish CBZ");
}

fn find_toplevel(title: &str) -> Option<gtk4::Window> {
    gtk4::Window::list_toplevels()
        .into_iter()
        .filter_map(|widget| widget.downcast::<gtk4::Window>().ok())
        .find(|window| window.title().as_deref() == Some(title))
}

fn walk(widget: &gtk4::Widget, output: &mut Vec<gtk4::Widget>) {
    output.push(widget.clone());
    let mut child = widget.first_child();
    while let Some(widget) = child {
        walk(&widget, output);
        child = widget.next_sibling();
    }
}

fn widgets(window: &gtk4::Window) -> Vec<gtk4::Widget> {
    let mut output = Vec::new();
    if let Some(child) = window.child() {
        walk(&child, &mut output);
    }
    output
}

fn labels(window: &gtk4::Window) -> Vec<String> {
    widgets(window)
        .into_iter()
        .filter_map(|widget| widget.downcast::<gtk4::Label>().ok())
        .map(|label| label.text().to_string())
        .collect()
}

fn button(window: &gtk4::Window, text: &str) -> gtk4::Button {
    widgets(window)
        .into_iter()
        .filter_map(|widget| widget.downcast::<gtk4::Button>().ok())
        .find(|button| button.label().as_deref() == Some(text))
        .unwrap_or_else(|| panic!("button {text}"))
}

fn buttons(window: &gtk4::Window, text: &str) -> Vec<gtk4::Button> {
    widgets(window)
        .into_iter()
        .filter_map(|widget| widget.downcast::<gtk4::Button>().ok())
        .filter(|button| button.label().as_deref() == Some(text))
        .collect()
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
    write_cbz(&library_root.join("Alpha 001.cbz"), "Alpha", "1");
    write_cbz(&library_root.join("Alpha 003.cbz"), "Alpha", "3");
    write_cbz(&library_root.join("Beta 001.cbz"), "Beta", "1");

    let paths = Rc::new(cr_core::paths::Paths::from_roots(&data, &config));
    let database_path = cr_core::paths::database_file(&paths);
    let seed_database = ComicDatabase {
        books: vec![
            book(101, &library_root.join("Alpha 001.cbz"), "Alpha", "1"),
            book(103, &library_root.join("Alpha 003.cbz"), "Alpha", "3"),
            book(105, &library_root.join("Beta 001.cbz"), "Beta", "1"),
        ],
        ..Default::default()
    };
    cr_core::database::comic_database::save(&seed_database, &database_path)
        .expect("seed ComicDb.xml");
    let comic_db_before_lists = std::fs::read(&database_path).expect("read ComicDb before lists");
    let incoming_list_id = fixed_id(200);
    IncomingLists {
        lists: vec![SmartListItem {
            base: ListItemBase {
                id: incoming_list_id,
                name: Some("Incoming Alpha".into()),
                ..Default::default()
            },
            matchers: vec![ComicBookMatcher::Value(ValueMatcher {
                type_name: "ComicBookSeriesMatcher".into(),
                match_value: "Alpha".into(),
                match_operator: 0,
                ..Default::default()
            })],
            ..Default::default()
        }],
    }
    .save(&paths)
    .expect("seed IncomingLists.xml");
    assert_eq!(
        std::fs::read(&database_path).expect("read ComicDb after lists"),
        comic_db_before_lists,
        "Incoming list save must not change ComicDb.xml"
    );
    let comic_db_before_scan = std::fs::read(&database_path).expect("read seeded ComicDb.xml");
    println!("GATE A OK: isolated paths, valid CBZ seed, and fixed IDs are ready");

    gate_compare_actions(&work);

    gtk4::init().expect("GTK init");
    cr_ui::theme::init();
    cr_ui::library::initialize().expect("library session");
    let incoming_config = IncomingConfig {
        incoming_folders: vec![incoming_root.to_string_lossy().into_owned()],
        last_organizer_profile: "Probe Move".into(),
        find_in_incoming_profile: "Probe Move".into(),
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
            &names[incoming_index..incoming_index + 10],
            [
                "Incoming",
                "All",
                "Gap Fills",
                "Duplicates",
                "Library Duplicates",
                "Incoming Duplicates",
                "New Series",
                "Needs Review",
                "Smart Lists",
                "Incoming Alpha",
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

fn gate_compare_actions(work: &Path) {
    use cr_ui::dialogs::incoming_compare::{
        recommended_action, CompareAction, DuplicateMatch, MatchSource,
    };

    let root = work.join("compare-actions");
    let incoming_path = root.join("incoming/Replacement.cbz");
    let old_library_path = root.join("library/Library Name.cbr");
    std::fs::create_dir_all(incoming_path.parent().unwrap()).expect("create action Incoming root");
    std::fs::create_dir_all(old_library_path.parent().unwrap())
        .expect("create action Library root");
    write_cbz(&incoming_path, "Replacement", "1");
    let source_bytes = std::fs::read(&incoming_path).expect("read replacement source");
    std::fs::write(&old_library_path, b"old library bytes").expect("write old Library file");

    let mut incoming = book(40, &incoming_path, "Replacement", "1");
    incoming.file_size = 200;
    let mut library = book(41, &old_library_path, "Replacement", "1");
    library.file_size = 100;
    let duplicate = DuplicateMatch {
        source: MatchSource::Library,
        book: library.clone(),
    };
    let rules = cr_engine::duplicates::DuplicateRules {
        cbr_worse_than_cbz: false,
        smaller_file_worse: true,
        fewer_pages_worse: false,
        older_file_worse: false,
    };
    if recommended_action(&incoming, &duplicate, &rules) != Some(CompareAction::ReplaceLibraryCopy)
    {
        eprintln!("GATE A2 FAILED: the worse Library copy was not recommended for replacement");
        std::process::exit(1);
    }

    let destination = old_library_path.with_extension("cbz");
    let staging = root.join("library/.Library Name.cbz.incoming-stage");
    let database_path = root.join("ComicDb.xml");
    let incoming_catalog_path = root.join("IncomingDb.xml");
    let journal_path = root.join("transaction.json");
    let mut transaction = ReplacementTransaction::prepare(
        incoming_path.clone(),
        staging,
        destination.clone(),
        old_library_path.clone(),
        snapshot(database_path.clone(), b"database after".to_vec()),
        snapshot(incoming_catalog_path.clone(), b"incoming after".to_vec()),
    )
    .expect("prepare replacement probe");
    let engine = TransactionEngine::from_journal_path(journal_path);
    engine
        .begin_replacement(&transaction)
        .expect("begin replacement probe");
    let trash_root = root.join("trash");
    engine
        .commit_replacement_with_trash(&mut transaction, &|path| {
            std::fs::create_dir_all(&trash_root)?;
            std::fs::rename(path, trash_root.join(path.file_name().unwrap()))
        })
        .expect("commit replacement probe");
    let replacement_bytes = std::fs::read(&destination).expect("read replacement destination");
    if replacement_bytes != source_bytes
        || incoming_path.exists()
        || old_library_path.exists()
        || std::fs::read(&database_path).ok().as_deref() != Some(b"database after")
        || std::fs::read(&incoming_catalog_path).ok().as_deref() != Some(b"incoming after")
        || engine.journal_path().exists()
    {
        eprintln!("GATE A2 FAILED: the isolated replacement did not commit safely");
        std::process::exit(1);
    }
    println!("GATE A2 OK: Compare recommendation and isolated replacement are safe");
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
        (6, "Gamma", "1"),
        (7, "Gamma", "1"),
    ];
    let mut books: Vec<ComicBook> = fixtures
        .iter()
        .map(|(id, series, number)| {
            let path = incoming_root.join(format!("{series}-{number}-{id}.cbz"));
            write_cbz(&path, series, number);
            book(*id, &path, series, number)
        })
        .collect();
    books[2].file_size = 200;
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
    assert_eq!(ids(|class| class.duplicate), set(&[1, 3, 5, 6, 7]));
    assert_eq!(ids(|class| class.library_duplicate), set(&[1, 3, 5]));
    assert_eq!(ids(|class| class.incoming_duplicate), set(&[3, 5, 6, 7]));
    assert_eq!(ids(|class| class.new_series), set(&[6, 7]));
    assert_eq!(ids(|class| class.needs_review), set(&[4]));

    let views = [
        (cr_ui::browser::navigator::IncomingView::All, 7usize),
        (cr_ui::browser::navigator::IncomingView::GapFills, 1),
        (cr_ui::browser::navigator::IncomingView::Duplicates, 5),
        (
            cr_ui::browser::navigator::IncomingView::LibraryDuplicates,
            3,
        ),
        (
            cr_ui::browser::navigator::IncomingView::IncomingDuplicates,
            4,
        ),
        (cr_ui::browser::navigator::IncomingView::NewSeries, 2),
        (cr_ui::browser::navigator::IncomingView::NeedsReview, 1),
    ];
    check_view(shell, paths, work, views, 0);
}

fn check_view(
    shell: Rc<cr_ui::browser::shell::BrowserShell>,
    paths: Rc<cr_core::paths::Paths>,
    work: PathBuf,
    views: [(cr_ui::browser::navigator::IncomingView, usize); 7],
    index: usize,
) {
    if index == views.len() {
        println!("GATE E OK: exact memberships and asynchronous UI projections match");
        check_incoming_smart_list(shell, paths, work);
        return;
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

fn check_incoming_smart_list(
    shell: Rc<cr_ui::browser::shell::BrowserShell>,
    paths: Rc<cr_core::paths::Paths>,
    work: PathBuf,
) {
    let id = fixed_id(200);
    shell.state_select_list(&id);
    glib::timeout_add_local(std::time::Duration::from_millis(350), move || {
        wait_for_incoming_smart_list(shell.clone(), paths.clone(), work.clone(), id, 0);
        ControlFlow::Break
    });
}

fn wait_for_incoming_smart_list(
    shell: Rc<cr_ui::browser::shell::BrowserShell>,
    paths: Rc<cr_core::paths::Paths>,
    work: PathBuf,
    id: CrGuid,
    ticks: u32,
) {
    if shell.state_grid_book_count() != 3 {
        if ticks >= 100 {
            eprintln!(
                "GATE E3 FAILED: Incoming smart list showed {} books instead of 3",
                shell.state_grid_book_count()
            );
            std::process::exit(1);
        }
        glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            wait_for_incoming_smart_list(shell.clone(), paths.clone(), work.clone(), id, ticks + 1);
            ControlFlow::Break
        });
        return;
    }
    let loaded = IncomingLists::load(&paths).expect("reload IncomingLists.xml");
    if loaded.lists.len() != 1 || loaded.lists[0].base.id != id {
        eprintln!("GATE E3 FAILED: Incoming smart list did not reload with its stable ID");
        std::process::exit(1);
    }
    println!("GATE E3 OK: Incoming smart list persists and evaluates Incoming books only");
    check_incoming_duplicate_menu(shell, paths, work);
}

fn check_incoming_duplicate_menu(
    shell: Rc<cr_ui::browser::shell::BrowserShell>,
    paths: Rc<cr_core::paths::Paths>,
    work: PathBuf,
) {
    shell.state_select_list(&cr_ui::browser::navigator::IncomingView::IncomingDuplicates.id());
    glib::timeout_add_local(std::time::Duration::from_millis(350), move || {
        shell.state_select_first_book();
        let Some((x, y)) = shell.state_item_center(0) else {
            eprintln!("GATE E4 FAILED: Incoming duplicate row has no item center");
            std::process::exit(1);
        };
        if !shell.state_open_context(x, y) {
            eprintln!("GATE E4 FAILED: Incoming duplicate context menu did not open");
            std::process::exit(1);
        }
        let Some(popover) = shell.state_context_popover() else {
            eprintln!("GATE E4 FAILED: Incoming duplicate context menu is absent");
            std::process::exit(1);
        };
        let mut rows = Vec::new();
        if let Some(child) = popover.child() {
            walk(&child, &mut rows);
        }
        let found = rows
            .into_iter()
            .filter_map(|widget| widget.downcast::<gtk4::Button>().ok())
            .any(|button| button.label().as_deref() == Some("Select Worst Duplicates"));
        if !found {
            eprintln!("GATE E4 FAILED: Select Worst Duplicates is absent from Incoming Duplicates");
            std::process::exit(1);
        }
        popover.popdown();
        println!("GATE E4 OK: Incoming Duplicates exposes Select Worst Duplicates");
        open_compare(shell.clone(), paths.clone(), work.clone());
        ControlFlow::Break
    });
}

fn open_compare(
    shell: Rc<cr_ui::browser::shell::BrowserShell>,
    paths: Rc<cr_core::paths::Paths>,
    work: PathBuf,
) {
    shell.state_select_list(&cr_ui::browser::navigator::IncomingView::Duplicates.id());
    glib::timeout_add_local(std::time::Duration::from_millis(350), move || {
        shell.state_reselect(&[fixed_id(1), fixed_id(3)]);
        assert_eq!(shell.state_grid_selection_len(), 2);
        shell.state_compare_incoming();
        wait_for_compare(shell.clone(), paths.clone(), work.clone(), 0);
        ControlFlow::Break
    });
}

fn wait_for_compare(
    shell: Rc<cr_ui::browser::shell::BrowserShell>,
    paths: Rc<cr_core::paths::Paths>,
    work: PathBuf,
    ticks: u32,
) {
    let Some(window) = find_toplevel("Compare Incoming") else {
        assert!(ticks < 100, "Compare Incoming did not open");
        glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            wait_for_compare(shell.clone(), paths.clone(), work.clone(), ticks + 1);
            ControlFlow::Break
        });
        return;
    };
    let text = labels(&window).join("\n");
    if !text.contains("Book 1 of 2") || !text.contains("Match 1 of ") {
        assert!(
            ticks < 100,
            "Compare Incoming labels stayed incomplete: {text}"
        );
        glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            wait_for_compare(shell.clone(), paths.clone(), work.clone(), ticks + 1);
            ControlFlow::Break
        });
        return;
    }
    button(&window, "Next Book").emit_clicked();
    let text = labels(&window).join("\n");
    assert!(text.contains("Book 2 of 2"));
    navigate_to_library_match(window, paths, work);
}

fn navigate_to_library_match(
    window: gtk4::Window,
    paths: Rc<cr_core::paths::Paths>,
    work: PathBuf,
) {
    let text = labels(&window).join("\n");
    if text.contains("Library duplicate") {
        wait_for_compare_covers(window, paths, work, 0);
        return;
    }
    let next = button(&window, "Next Match");
    if !next.is_sensitive() {
        eprintln!("GATE E2 FAILED: no Library match is available for recommendation");
        std::process::exit(1);
    }
    next.emit_clicked();
    let text = labels(&window).join("\n");
    if !text.contains("Library duplicate") {
        eprintln!("GATE E2 FAILED: match navigation did not reach a Library duplicate");
        std::process::exit(1);
    }
    wait_for_compare_covers(window, paths, work, 0);
}

fn wait_for_compare_covers(
    window: gtk4::Window,
    paths: Rc<cr_core::paths::Paths>,
    work: PathBuf,
    ticks: u32,
) {
    let painted = widgets(&window)
        .into_iter()
        .filter_map(|widget| widget.downcast::<gtk4::Picture>().ok())
        .filter(|picture| picture.paintable().is_some())
        .count();
    if painted < 2 {
        assert!(ticks < 200, "Compare covers did not load");
        glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            wait_for_compare_covers(window.clone(), paths.clone(), work.clone(), ticks + 1);
            ControlFlow::Break
        });
        return;
    }
    println!("GATE E2 OK: Compare navigates books and matches and loads both covers");
    // The recommendation is automatic: without any click, one Keep
    // button is highlighted and the panes carry the green/red borders.
    let keep = buttons(&window, "Keep This Copy");
    if keep.len() != 2 {
        eprintln!("GATE E2 FAILED: Compare did not show two Keep This Copy buttons");
        std::process::exit(1);
    }
    let auto_highlighted = keep
        .iter()
        .filter(|button| button.has_css_class("suggested-action"))
        .count();
    if auto_highlighted != 1 || !keep[0].has_css_class("suggested-action") {
        eprintln!("GATE E2 FAILED: automatic recommendation did not highlight one Keep button");
        std::process::exit(1);
    }
    let preferred = widgets(&window)
        .into_iter()
        .filter(|w| w.has_css_class("compare-pane-preferred"))
        .count();
    let worse = widgets(&window)
        .into_iter()
        .filter(|w| w.has_css_class("compare-pane-worse"))
        .count();
    if preferred != 1 || worse != 1 {
        eprintln!(
            "GATE E2 FAILED: automatic borders wrong preferred={preferred} worse={worse} (want 1/1)"
        );
        std::process::exit(1);
    }
    // The button re-applies the same recommendation.
    button(&window, "Select Worst Duplicates").emit_clicked();
    let keep = buttons(&window, "Keep This Copy");
    let highlighted = keep
        .iter()
        .filter(|button| button.has_css_class("suggested-action"))
        .count();
    if highlighted != 1 || !keep[0].has_css_class("suggested-action") {
        eprintln!("GATE E2 FAILED: recommendation did not highlight exactly one Keep button");
        std::process::exit(1);
    }
    if !buttons(&window, "Run Selected Action").is_empty() {
        eprintln!("GATE E2 FAILED: obsolete action control is visible");
        std::process::exit(1);
    }
    println!("GATE E2A OK: Compare recommends automatically with green/red borders");
    window.close();
    gate_f_to_i(paths, work);
    println!("INCOMING PROBE DONE");
    std::process::exit(0);
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
        find_in_incoming_profile: "Converted Move".into(),
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
