//! Mover integration tests on real temp directories: the move/copy/
//! simulate flows, the exclude rules, duplicate handling, the
//! empty-folder cleanup, the fileless export, and the undo round
//! trip.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use cr_core::model::comic_book::ComicBook;
use cr_organize::engine::{
    adoption_manifest_path, AdoptionManifest, AdoptionManifestEntry, Apply, DuplicateAction,
    DuplicateAnswer, DuplicateAsk, FilesystemEffects, LogEntry, MoveLanding, OrganizeReport,
    OrganizeUi, RunContext,
};
use cr_organize::mover::{run_undo, UndoCollection};
use cr_organize::profile::Profile;
use cr_organize::template::{MultiValueAnswer, MultiValueAsk, MultiValueAsker};

static CANCEL: AtomicBool = AtomicBool::new(false);
static STUB_COVER: StubCover = StubCover;

struct StubCover;
impl cr_organize::engine::CoverSource for StubCover {
    fn fileless_cover(&self, _book: &ComicBook) -> Option<cr_image::Image> {
        Some(cr_image::Image::new(1, 1, vec![255, 0, 0, 255]).unwrap())
    }

    fn duplicate_cover(&self, _book: &ComicBook) -> Option<Vec<u8>> {
        None
    }
}

struct StubUi {
    log: Mutex<Vec<LogEntry>>,
    duplicate: Mutex<Option<DuplicateAnswer>>,
    asks: Mutex<Vec<DuplicateAsk>>,
}

impl StubUi {
    fn new() -> StubUi {
        StubUi {
            log: Mutex::new(Vec::new()),
            duplicate: Mutex::new(None),
            asks: Mutex::new(Vec::new()),
        }
    }

    fn actions(&self, action: &str) -> usize {
        self.log
            .lock()
            .unwrap()
            .iter()
            .filter(|e| e.action == action)
            .count()
    }

    fn action_names(&self) -> Vec<String> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .map(|entry| entry.action.clone())
            .collect()
    }
}

impl MultiValueAsker for StubUi {
    fn ask_multi_value(&mut self, _ask: MultiValueAsk) -> MultiValueAnswer {
        MultiValueAnswer::default()
    }
}

impl OrganizeUi for StubUi {
    fn ask_duplicate(&mut self, ask: DuplicateAsk) -> DuplicateAnswer {
        self.asks.lock().unwrap().push(ask);
        self.duplicate.lock().unwrap().unwrap_or(DuplicateAnswer {
            action: DuplicateAction::Cancel,
            always: false,
        })
    }

    fn log(&mut self, entry: LogEntry) {
        self.log.lock().unwrap().push(entry);
    }

    fn progress(&mut self, _done: usize, _total: usize) {}
}

fn book(series: &str, number: &str, volume: i32, path: &str) -> ComicBook {
    let mut b = ComicBook::default();
    b.info.series = series.into();
    b.info.number = number.into();
    b.info.volume = volume;
    b.file_path = path.into();
    b.enable_proposed = false;
    b
}

fn profile(name: &str, mode: &str, base: &Path) -> Profile {
    let mut p = Profile::builtin_default();
    p.name = name.into();
    p.mode = mode.into();
    p.base_folder = base.to_string_lossy().into_owned();
    p
}

fn ctx<'a>(
    books: &'a [ComicBook],
    selected: &'a [usize],
    profiles: &'a [Profile],
    trash: &'a impl Fn(&str) -> bool,
) -> RunContext<'a> {
    RunContext {
        books,
        selected,
        profiles,
        move_landing: MoveLanding::default(),
        trash,
        filesystem_effects: None,
        cover: &STUB_COVER,
        undo_path: None,
        cancel: &CANCEL,
    }
}

#[test]
fn filesystem_effects_wrap_a_move() {
    struct Effects(Mutex<Vec<String>>);
    impl FilesystemEffects for Effects {
        fn before_rename(&self, source: &str, destination: &str) -> std::io::Result<()> {
            self.0
                .lock()
                .unwrap()
                .push(format!("before:{source}:{destination}"));
            Ok(())
        }

        fn after_rename(
            &self,
            source: &str,
            destination: &str,
            succeeded: bool,
        ) -> std::io::Result<()> {
            self.0
                .lock()
                .unwrap()
                .push(format!("after:{source}:{destination}:{succeeded}"));
            Ok(())
        }
    }

    let tmp = std::env::temp_dir().join(format!("lo-effects-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let source = tmp.join("source.cbz");
    write(&source);
    let books = vec![book("B", "1", 1, &source.to_string_lossy())];
    let selected = vec![0];
    let profiles = vec![profile("Move", "Move", &tmp.join("library"))];
    let effects = Effects(Mutex::new(Vec::new()));
    let trash = |_path: &str| false;
    let context = RunContext {
        books: &books,
        selected: &selected,
        profiles: &profiles,
        move_landing: MoveLanding::default(),
        trash: &trash,
        filesystem_effects: Some(&effects),
        cover: &STUB_COVER,
        undo_path: None,
        cancel: &CANCEL,
    };
    let mut ui = StubUi::new();

    let report = cr_organize::engine::organize(context, &mut ui);

    assert!(!report.applies.is_empty());
    let calls = effects.0.lock().unwrap();
    assert_eq!(calls.len(), 2);
    assert!(calls[0].starts_with("before:"));
    assert!(calls[1].ends_with(":true"));
    let _ = std::fs::remove_dir_all(&tmp);
}

fn no_trash(_path: &str) -> bool {
    false
}

fn delete_trash(path: &str) -> bool {
    std::fs::remove_file(path).is_ok()
}

fn write(path: &Path) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, b"x").unwrap();
}

#[test]
fn move_renames_files_and_updates_books() {
    let tmp = std::env::temp_dir().join(format!("lo-move-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let src = tmp.join("src");
    let dst = tmp.join("dst");
    write(&src.join("Batman 005.cbz"));

    let books = vec![book(
        "Batman",
        "5",
        1,
        &src.join("Batman 005.cbz").to_string_lossy(),
    )];
    let p = profile("Move", "Move", &dst);
    let selected = vec![0];

    let mut ui = StubUi::new();
    let report = cr_organize::engine::organize(
        ctx(&books, &selected, std::slice::from_ref(&p), &no_trash),
        &mut ui,
    );
    assert_eq!(
        report.text,
        "Move:\nSuccessfully moved: 1\tSkipped: 0\tFailed: 0"
    );
    assert!(!report.failed_or_skipped);
    assert_eq!(ui.action_names(), ["Preparing", "moving", "moved"]);

    // The file landed under the base folder in the folder template
    // (default: the series), renamed by the file template.
    let expected = dst.join("Batman").join("Batman Vol.1 #05.cbz");
    assert!(expected.exists());
    assert!(!src.join("Batman 005.cbz").exists());

    // One Update apply with the new path.
    assert_eq!(report.applies.len(), 1);
    match &report.applies[0] {
        Apply::Update(b) => {
            assert_eq!(b.file_path, expected.to_string_lossy());
        }
        other => panic!("expected an update, got {other:?}"),
    }
    // The undo record maps the original to the new path.
    assert_eq!(report.undo.len(), 1);
    assert_eq!(report.undo.undo_paths[0], books[0].file_path);
    assert_eq!(report.undo.current_paths[0], expected.to_string_lossy());

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn move_can_adopt_the_complete_book_with_its_id() {
    let tmp = std::env::temp_dir().join(format!("lo-adopt-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let source = tmp.join("src").join("Incoming.cbz");
    let destination = tmp.join("library");
    write(&source);

    let mut original = book("Batman", "5", 1, &source.to_string_lossy());
    original.info.summary = "Complete incoming metadata".into();
    original.info.tags = "incoming, reviewed".into();
    original.last_page_read = 7;
    let books = vec![original.clone()];
    let selected = vec![0];
    let profile = profile("Adopt", "Move", &destination);
    let mut context = ctx(&books, &selected, std::slice::from_ref(&profile), &no_trash);
    context.move_landing = MoveLanding::InsertPreservingId;

    let mut ui = StubUi::new();
    let report = cr_organize::engine::organize(context, &mut ui);
    assert_eq!(report.applies.len(), 1);
    let Apply::Adopt(adopted) = &report.applies[0] else {
        panic!("expected an adopt apply, got {:?}", report.applies[0]);
    };
    let mut expected = original;
    expected.file_path = destination
        .join("Batman")
        .join("Batman Vol.1 #05.cbz")
        .to_string_lossy()
        .into_owned();
    assert_eq!(adopted, &expected);
    assert_eq!(
        report.undo.entry(&adopted.file_path).unwrap().undo_path,
        source.to_string_lossy()
    );

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn failed_adoption_validation_rolls_back_and_stays_incoming() {
    let tmp = std::env::temp_dir().join(format!("lo-adopt-validation-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let source = tmp.join("incoming").join("Incoming.cbz");
    write(&source);
    let books = vec![book("Batman", "5", 1, &source.to_string_lossy())];
    let selected = [0];
    let mut profile = profile("Adopt", "Move", &tmp.join("library"));
    profile.fail_empty_values = true;
    profile.move_failed = true;
    profile.failed_folder = tmp.join("failed").to_string_lossy().into_owned();
    profile.failed_fields = vec!["ShadowTitle".into()];
    profile.file_template = "{<title>}{<series>}".into();
    let profiles = [profile];
    let mut context = ctx(&books, &selected, &profiles, &no_trash);
    context.move_landing = MoveLanding::InsertPreservingId;

    let mut ui = StubUi::new();
    let report = cr_organize::engine::organize(context, &mut ui);

    assert!(source.exists());
    assert!(report.applies.is_empty());
    assert!(report.undo.is_empty());
    assert!(report.text.contains("Failed: 1"));
    assert_eq!(ui.actions("Failed"), 1);
    assert_eq!(ui.actions("moved"), 0);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn copy_keeps_the_source_and_inserts_a_book() {
    let tmp = std::env::temp_dir().join(format!("lo-copy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let src = tmp.join("src");
    let dst = tmp.join("dst");
    write(&src.join("Batman 005.cbz"));

    let books = vec![book(
        "Batman",
        "5",
        1,
        &src.join("Batman 005.cbz").to_string_lossy(),
    )];
    let mut p = profile("Copy", "Copy", &dst);
    p.file_template = "{<series>}{ #<number>}".into();
    p.folder_template = "{<series>}".into();
    let selected = vec![0];

    let mut ui = StubUi::new();
    let report = cr_organize::engine::organize(ctx(&books, &selected, &[p], &no_trash), &mut ui);
    assert_eq!(report.applies.len(), 1);
    match &report.applies[0] {
        Apply::Insert(new_book) => {
            let expected = dst.join("Batman").join("Batman #5.cbz");
            assert_eq!(new_book.file_path, expected.to_string_lossy());
            assert_eq!(new_book.info.series, "Batman");
            assert!(expected.exists());
            // The source file stays.
            assert!(src.join("Batman 005.cbz").exists());
            // Copies are never undo records.
            assert!(report.undo.is_empty());
        }
        other => panic!("expected an insert, got {other:?}"),
    }
    assert_eq!(ui.action_names(), ["Preparing", "copying", "copied"]);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn insert_preserving_id_does_not_change_copy_or_simulate() {
    let tmp = std::env::temp_dir().join(format!("lo-landing-modes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let source = tmp.join("src").join("Batman.cbz");
    write(&source);
    let books = vec![book("Batman", "5", 1, &source.to_string_lossy())];
    let selected = vec![0];

    let copy_profile = profile("Copy", "Copy", &tmp.join("copy"));
    let copy_profiles = [copy_profile];
    let mut copy_context = ctx(&books, &selected, &copy_profiles, &no_trash);
    copy_context.move_landing = MoveLanding::InsertPreservingId;
    let mut ui = StubUi::new();
    let copy_report = cr_organize::engine::organize(copy_context, &mut ui);
    assert!(matches!(copy_report.applies.as_slice(), [Apply::Insert(_)]));

    let simulate_profile = profile("Simulate", "Simulate", &tmp.join("simulate"));
    let simulate_profiles = [simulate_profile];
    let mut simulate_context = ctx(&books, &selected, &simulate_profiles, &no_trash);
    simulate_context.move_landing = MoveLanding::InsertPreservingId;
    let mut ui = StubUi::new();
    let simulate_report = cr_organize::engine::organize(simulate_context, &mut ui);
    assert!(simulate_report.applies.is_empty());
    assert!(source.exists());

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn simulate_touches_nothing() {
    let tmp = std::env::temp_dir().join(format!("lo-sim-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let src = tmp.join("src");
    let dst = tmp.join("dst");
    write(&src.join("Batman 005.cbz"));

    let books = vec![book(
        "Batman",
        "5",
        1,
        &src.join("Batman 005.cbz").to_string_lossy(),
    )];
    let p = profile("Sim", "Simulate", &dst);
    let selected = vec![0];

    let mut ui = StubUi::new();
    let report = cr_organize::engine::organize(ctx(&books, &selected, &[p], &no_trash), &mut ui);
    assert_eq!(report.applies.len(), 0);
    assert_eq!(ui.actions("moved (simulated)"), 1);
    assert_eq!(ui.actions("moving (simulated)"), 1);
    assert_eq!(ui.actions("Created Folder"), 1);
    // Nothing on disk but the original file.
    assert!(src.join("Batman 005.cbz").exists());
    assert!(!dst.exists());
    assert!(report.undo.is_empty());
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn exclude_rules_and_folders_skip_books() {
    let tmp = std::env::temp_dir().join(format!("lo-rules-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let src = tmp.join("src");
    let dst = tmp.join("dst");
    let read_path = src.join("read").join("B 1.cbz");
    let incoming_path = src.join("incoming").join("B 2.cbz");
    write(&read_path);
    write(&incoming_path);

    let mut b1 = book("B", "1", 1, &read_path.to_string_lossy());
    b1.info.page_count = 10;
    b1.last_page_read = 9; // read
    let b2 = book("B", "2", 1, &incoming_path.to_string_lossy());
    let books = vec![b1, b2];

    let mut p = profile("Rules", "Move", &dst);
    // Only read books move.
    p.exclude_mode = "Only".into();
    p.exclude_rules.push(cr_organize::profile::RuleNode::Rule(
        cr_organize::profile::ExcludeRule {
            field: "Read Percentage".into(),
            operator: "greater than".into(),
            value: "0".into(),
        },
    ));
    // The incoming folder is never touched.
    p.exclude_folders.push("/incoming".into());

    let selected = vec![0, 1];
    let mut ui = StubUi::new();
    let report = cr_organize::engine::organize(ctx(&books, &selected, &[p], &no_trash), &mut ui);

    // Book 1 (read, not in an excluded folder) moved; book 2 skipped.
    assert!(report.text.contains("Successfully moved: 1"));
    assert!(report.text.contains("Skipped: 1"));
    assert_eq!(report.applies.len(), 1);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn duplicate_destination_renames_with_user_consent() {
    let tmp = std::env::temp_dir().join(format!("lo-dup-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let src = tmp.join("src");
    let dst = tmp.join("dst");
    // The destination already holds a file with the target name.
    let existing = dst.join("Batman").join("Batman Vol.1 #05.cbz");
    write(&existing);
    let src_file = src.join("Batman 005.cbz");
    write(&src_file);

    let books = vec![book("Batman", "5", 1, &src_file.to_string_lossy())];
    let p = profile("Move", "Move", &dst);
    let selected = vec![0];

    // Rename: the file lands as `Batman #05 (1).cbz`.
    let mut ui = StubUi::new();
    *ui.duplicate.lock().unwrap() = Some(DuplicateAnswer {
        action: DuplicateAction::Rename,
        always: false,
    });
    let report = cr_organize::engine::organize(
        ctx(&books, &selected, std::slice::from_ref(&p), &no_trash),
        &mut ui,
    );
    assert_eq!(report.applies.len(), 1);
    let renamed = dst.join("Batman").join("Batman Vol.1 #05 (1).cbz");
    assert!(renamed.exists(), "renamed file missing");
    assert_eq!(ui.actions("Skipped"), 0);
    assert_eq!(ui.actions("Skipped"), 0);

    // Cancel: nothing moves.
    let _ = std::fs::remove_dir_all(&dst);
    write(&src_file);
    write(&existing);
    let mut ui = StubUi::new();
    *ui.duplicate.lock().unwrap() = Some(DuplicateAnswer {
        action: DuplicateAction::Cancel,
        always: false,
    });
    let report = cr_organize::engine::organize(
        ctx(&books, &selected, std::slice::from_ref(&p), &no_trash),
        &mut ui,
    );
    assert!(report.applies.is_empty());
    assert_eq!(ui.actions("Skipped"), 1);
    assert!(existing.exists());

    // Overwrite with the destination NOT a library book: the trash
    // callback removes the file, no library removal.
    write(&src_file);
    let mut ui = StubUi::new();
    *ui.duplicate.lock().unwrap() = Some(DuplicateAnswer {
        action: DuplicateAction::Overwrite,
        always: false,
    });
    let report = cr_organize::engine::organize(
        ctx(&books, &selected, std::slice::from_ref(&p), &delete_trash),
        &mut ui,
    );
    let overwrote = dst.join("Batman").join("Batman Vol.1 #05.cbz");
    assert!(overwrote.exists());
    assert!(report.applies.iter().all(|a| matches!(a, Apply::Update(_))));

    // Overwrite with the destination a LIBRARY book: the read
    // percentage carries over and the replaced book is removed.
    write(&src_file);
    write(&existing);
    let mut replaced = book("Batman", "5", 1, &existing.to_string_lossy());
    replaced.last_page_read = 7;
    let books = vec![books[0].clone(), replaced];
    let selected = vec![0];
    let mut ui = StubUi::new();
    *ui.duplicate.lock().unwrap() = Some(DuplicateAnswer {
        action: DuplicateAction::Overwrite,
        always: true,
    });
    let report =
        cr_organize::engine::organize(ctx(&books, &selected, &[p], &delete_trash), &mut ui);
    assert!(overwrote.exists());
    let removes = report
        .applies
        .iter()
        .filter(|a| matches!(a, Apply::Remove(_)))
        .count();
    assert_eq!(removes, 1);
    let carried = report
        .applies
        .iter()
        .find_map(|a| match a {
            Apply::Update(b) => Some(b.last_page_read),
            _ => None,
        })
        .unwrap_or(0);
    assert_eq!(carried, 7);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn empty_folders_are_pruned_after_a_move() {
    let tmp = std::env::temp_dir().join(format!("lo-prune-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let src = tmp.join("src").join("Writer").join("Series");
    let dst = tmp.join("dst");
    write(&src.join("B 1.cbz"));

    let books = vec![book("B", "1", 1, &src.join("B 1.cbz").to_string_lossy())];
    let mut p = profile("Move", "Move", &dst);
    p.file_template = "{<series>}.cbz".into();
    let selected = vec![0];

    let mut ui = StubUi::new();
    let _ = cr_organize::engine::organize(ctx(&books, &selected, &[p], &no_trash), &mut ui);
    // The emptied source tree is gone.
    assert!(!tmp.join("src").join("Writer").exists());
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn excluded_empty_folders_survive_the_prune() {
    let tmp = std::env::temp_dir().join(format!("lo-prune2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let src = tmp.join("src").join("Writer");
    let dst = tmp.join("dst");
    write(&src.join("B 1.cbz"));

    let books = vec![book("B", "1", 1, &src.join("B 1.cbz").to_string_lossy())];
    let mut p = profile("Move", "Move", &dst);
    p.file_template = "{<series>}.cbz".into();
    p.excluded_empty_folder
        .push(src.to_string_lossy().into_owned());
    let selected = vec![0];

    let mut ui = StubUi::new();
    let _ = cr_organize::engine::organize(ctx(&books, &selected, &[p], &no_trash), &mut ui);
    // The excluded folder stays despite being empty.
    assert!(src.exists());
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn fileless_books_export_their_cover() {
    let tmp = std::env::temp_dir().join(format!("lo-fileless-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let dst = tmp.join("dst");

    let mut b = ComicBook::default();
    b.info.series = "Fileless".into();
    b.info.number = "1".into();
    b.enable_proposed = false;
    b.custom_thumbnail_key = Some("abc".into());
    let books = vec![b];

    let mut p = profile("Move", "Move", &dst);
    p.move_fileless = true;
    p.fileless_format = ".png".into();
    p.folder_template = String::new();
    p.file_template = "{<series>} {<number>}".into();
    let selected = vec![0];

    let mut ui = StubUi::new();
    let report = cr_organize::engine::organize(ctx(&books, &selected, &[p], &no_trash), &mut ui);
    assert!(report.text.contains("Successfully moved: 1"));
    let out = dst.join("Fileless 1.png");
    assert!(out.exists(), "fileless cover missing");
    let bytes = std::fs::read(&out).unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn fileless_without_move_fileless_is_skipped() {
    let tmp = std::env::temp_dir().join(format!("lo-fileless2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let dst = tmp.join("dst");

    let mut b = ComicBook::default();
    b.info.series = "Fileless".into();
    b.enable_proposed = false;
    b.custom_thumbnail_key = Some("abc".into());
    let books = vec![b];
    let p = profile("Move", "Move", &dst);
    let selected = vec![0];

    let mut ui = StubUi::new();
    let report = cr_organize::engine::organize(ctx(&books, &selected, &[p], &no_trash), &mut ui);
    assert!(report.text.contains("Skipped: 1"));
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn blank_filename_fails() {
    let tmp = std::env::temp_dir().join(format!("lo-blank-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let dst = tmp.join("dst");
    let src = tmp.join("src");
    write(&src.join("B 1.cbz"));

    let books = vec![book("B", "1", 1, &src.join("B 1.cbz").to_string_lossy())];
    let mut p = profile("Move", "Move", &dst);
    p.file_template = String::new();
    let selected = vec![0];

    let mut ui = StubUi::new();
    let report = cr_organize::engine::organize(ctx(&books, &selected, &[p], &no_trash), &mut ui);
    assert!(report.text.contains("Failed: 1"));
    assert_eq!(ui.actions("Failed"), 1);
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn zero_success_runs_preserve_existing_recovery_files() {
    let tmp = std::env::temp_dir().join(format!("lo-stale-undo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).unwrap();
    let undo_path = tmp.join("undo.dat");
    let old_undo = b"Old|/old/current.cbz|/old/source.cbz\n";
    std::fs::write(&undo_path, old_undo).unwrap();
    let manifest_path = adoption_manifest_path(&undo_path);
    let old_manifest = b"old manifest";
    std::fs::write(&manifest_path, old_manifest).unwrap();

    let source = tmp.join("src").join("B 1.cbz");
    write(&source);
    let books = vec![book("B", "1", 1, &source.to_string_lossy())];
    let selected = vec![0];
    let mut failed_profile = profile("Move", "Move", &tmp.join("dst"));
    failed_profile.file_template.clear();
    let profiles = [failed_profile];
    let mut context = ctx(&books, &selected, &profiles, &no_trash);
    context.undo_path = Some(undo_path.clone());

    let mut ui = StubUi::new();
    let report = cr_organize::engine::organize(context, &mut ui);
    assert!(report.undo.is_empty());
    assert_eq!(std::fs::read(&undo_path).unwrap(), old_undo);
    assert_eq!(std::fs::read(&manifest_path).unwrap(), old_manifest);

    let mut simulate = profile("Simulate", "Simulate", &tmp.join("simulate"));
    simulate.file_template = "{<series>}".into();
    let profiles = [simulate];
    let mut context = ctx(&books, &selected, &profiles, &no_trash);
    context.undo_path = Some(undo_path.clone());
    let mut ui = StubUi::new();
    cr_organize::engine::organize(context, &mut ui);
    assert_eq!(std::fs::read(&undo_path).unwrap(), old_undo);

    let cancel = AtomicBool::new(true);
    let move_profile = profile("Move", "Move", &tmp.join("cancel"));
    let profiles = [move_profile];
    let mut context = ctx(&books, &selected, &profiles, &no_trash);
    context.undo_path = Some(undo_path.clone());
    context.cancel = &cancel;
    let mut ui = StubUi::new();
    cr_organize::engine::organize(context, &mut ui);
    assert_eq!(std::fs::read(&undo_path).unwrap(), old_undo);

    let existing = tmp.join("skip").join("B").join("B Vol.1 #01.cbz");
    write(&existing);
    let skip_profile = profile("Move", "Move", &tmp.join("skip"));
    let profiles = [skip_profile];
    let mut context = ctx(&books, &selected, &profiles, &no_trash);
    context.undo_path = Some(undo_path.clone());
    let mut ui = StubUi::new();
    cr_organize::engine::organize(context, &mut ui);
    assert_eq!(std::fs::read(&undo_path).unwrap(), old_undo);

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn later_profiles_are_skipped_after_a_claim() {
    let tmp = std::env::temp_dir().join(format!("lo-multi-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let src = tmp.join("src");
    let dst1 = tmp.join("dst1");
    let dst2 = tmp.join("dst2");
    write(&src.join("B 1.cbz"));

    let books = vec![book("B", "1", 1, &src.join("B 1.cbz").to_string_lossy())];
    let p1 = profile("First", "Move", &dst1);
    let p2 = profile("Second", "Move", &dst2);
    let selected = vec![0];

    let mut ui = StubUi::new();
    let report =
        cr_organize::engine::organize(ctx(&books, &selected, &[p1, p2], &no_trash), &mut ui);
    assert!(dst2.join("B").join("B Vol.1 #01.cbz").exists());
    assert!(!dst1.join("B").exists());
    // The second profile reports the book as skipped by the claim.
    assert!(report.text.contains("Skipped: 1"));
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn undo_round_trip_restores_the_original_paths_restores_the_original_paths() {
    let tmp = std::env::temp_dir().join(format!("lo-undo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let src = tmp.join("src");
    let dst = tmp.join("dst");
    let original = src.join("Writer").join("B 1.cbz");
    write(&original);

    let books = vec![book("B", "1", 1, &original.to_string_lossy())];
    let p = profile("Move", "Move", &dst);
    let selected = vec![0];

    let mut ui = StubUi::new();
    let report: OrganizeReport =
        cr_organize::engine::organize(ctx(&books, &selected, &[p], &no_trash), &mut ui);
    assert_eq!(report.undo.len(), 1);

    // Save + reload the undo log (the UI's undo.dat cycle).
    let undo_file = tmp.join("undo.dat");
    report.undo.save(&undo_file).unwrap();
    let loaded = UndoCollection::load(&undo_file);
    assert_eq!(loaded, report.undo);

    // The snapshot after the move: the book at its new path.
    let moved_path = report
        .applies
        .iter()
        .find_map(|a| match a {
            Apply::Update(b) => Some(b.file_path.clone()),
            _ => None,
        })
        .unwrap();
    let moved_books = vec![book("B", "1", 1, &moved_path)];
    let selected = vec![0];
    let mut profiles = std::collections::HashMap::new();
    profiles.insert("Move".to_string(), profile("Move", "Move", &dst));

    let mut ui = StubUi::new();
    let ctx2 = RunContext {
        books: &moved_books,
        selected: &selected,
        profiles: &[],
        move_landing: MoveLanding::default(),
        trash: &no_trash,
        filesystem_effects: None,
        cover: &STUB_COVER,
        undo_path: None,
        cancel: &CANCEL,
    };
    let undo_report = run_undo(ctx2, &loaded, &profiles, &mut ui);
    assert_eq!(
        undo_report.text,
        "Successfully moved: 1\tFailed to move: 0\tSkipped: 0"
    );
    assert!(undo_report.residual.is_empty());
    assert_eq!(ui.action_names(), ["Preparing", "restoring", "restored"]);
    assert!(original.exists(), "the file did not come back");
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn undo_report_retains_failed_skipped_and_unprocessed_entries() {
    let tmp = std::env::temp_dir().join(format!("lo-undo-residual-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let restored = tmp.join("original").join("restored.cbz");
    let current = tmp.join("moved").join("restored.cbz");
    let missing_current = tmp.join("moved").join("missing.cbz");
    let skipped = tmp.join("same.cbz");
    write(&current);
    write(&skipped);

    let books = vec![
        book("B", "1", 1, &current.to_string_lossy()),
        book("B", "2", 1, &skipped.to_string_lossy()),
    ];
    let mut undo = UndoCollection::default();
    undo.append(
        &restored.to_string_lossy(),
        &current.to_string_lossy(),
        "Move",
    );
    undo.append(
        &tmp.join("original").join("missing.cbz").to_string_lossy(),
        &missing_current.to_string_lossy(),
        "Move",
    );
    undo.append(
        &skipped.to_string_lossy(),
        &skipped.to_string_lossy(),
        "Move",
    );
    let selected = vec![];
    let profiles = std::collections::HashMap::new();
    let mut ui = StubUi::new();

    let report = run_undo(
        ctx(&books, &selected, &[], &no_trash),
        &undo,
        &profiles,
        &mut ui,
    );

    assert!(restored.exists());
    assert_eq!(report.residual.len(), 2);
    assert!(report
        .residual
        .entry(&missing_current.to_string_lossy())
        .is_some());
    assert!(report.residual.entry(&skipped.to_string_lossy()).is_some());
    assert!(report.residual.entry(&current.to_string_lossy()).is_none());

    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn canceled_undo_retains_unprocessed_entries() {
    let tmp = std::env::temp_dir().join(format!("lo-undo-cancel-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let original = tmp.join("original.cbz");
    let current = tmp.join("current.cbz");
    write(&current);
    let books = vec![book("B", "1", 1, &current.to_string_lossy())];
    let mut undo = UndoCollection::default();
    undo.append(
        &original.to_string_lossy(),
        &current.to_string_lossy(),
        "Move",
    );
    let cancel = AtomicBool::new(true);
    let context = RunContext {
        books: &books,
        selected: &[],
        profiles: &[],
        move_landing: MoveLanding::default(),
        trash: &no_trash,
        filesystem_effects: None,
        cover: &STUB_COVER,
        undo_path: None,
        cancel: &cancel,
    };
    let mut ui = StubUi::new();

    let report = run_undo(context, &undo, &std::collections::HashMap::new(), &mut ui);

    assert_eq!(report.residual, undo);
    assert!(current.exists());
    assert!(!original.exists());
    let _ = std::fs::remove_dir_all(&tmp);
}

#[test]
fn multi_move_runs_keep_the_original_undo_path() {
    let mut undo = UndoCollection::default();
    undo.append("/a/one.cbz", "/b/one.cbz", "P1");
    undo.append("/b/one.cbz", "/c/one.cbz", "P2");
    // A book moved twice keeps ONE record: the original undo path and
    // the current path.
    assert_eq!(undo.len(), 1);
    assert_eq!(undo.undo_path("/c/one.cbz"), Some("/a/one.cbz"));
    assert_eq!(undo.profile("/c/one.cbz"), Some("P1"));
    // The load collapses the same way; the reversal only matters for
    // independent entries.
    let text = "P1|/b/one.cbz|/a/one.cbz\nP2|/c/one.cbz|/b/one.cbz\n";
    let path = std::env::temp_dir().join(format!("lo-undo2-{}.dat", std::process::id()));
    std::fs::write(&path, text).unwrap();
    let loaded = UndoCollection::load(&path);
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded.undo_path("/c/one.cbz"), Some("/a/one.cbz"));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn undo_serialization_bytes_stay_profile_current_undo() {
    let path = std::env::temp_dir().join(format!("lo-undo-bytes-{}.dat", std::process::id()));
    let mut undo = UndoCollection::default();
    undo.append("/incoming/one.cbz", "/library/one.cbz", "Adopt");
    undo.append("/incoming/two.cbz", "/library/two.cbz", "Move");

    undo.save(&path).unwrap();

    assert_eq!(
        std::fs::read(&path).unwrap(),
        b"Adopt|/library/one.cbz|/incoming/one.cbz\nMove|/library/two.cbz|/incoming/two.cbz\n"
    );
    let entries: Vec<_> = undo.entries().collect();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[1].profile_name, "Move");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn retry_save_keeps_loaded_undo_order_stable() {
    let path = std::env::temp_dir().join(format!("lo-undo-retry-{}.dat", std::process::id()));
    std::fs::write(
        &path,
        b"P1|/current/one|/undo/one\nP2|/current/two|/undo/two\n",
    )
    .unwrap();

    let first = UndoCollection::load(&path);
    first.save_for_retry(&path).unwrap();
    let second = UndoCollection::load(&path);
    second.save_for_retry(&path).unwrap();
    let third = UndoCollection::load(&path);

    assert_eq!(first, second);
    assert_eq!(second, third);
    let _ = std::fs::remove_file(path);
}

#[test]
fn manifest_matching_rejects_duplicate_and_malformed_entries() {
    let mut undo = UndoCollection::default();
    undo.append("/incoming/a", "/library/a", "Move");
    undo.append("/incoming/b", "/library/b", "Move");
    let entry = |id: &str, current: &str, original: &str| AdoptionManifestEntry {
        id: id.into(),
        profile_name: "Move".into(),
        current_path: current.into(),
        undo_path: original.into(),
    };
    let valid = AdoptionManifest {
        entries: vec![
            entry(
                "00000000-0000-0000-0000-000000000001",
                "/library/a",
                "/incoming/a",
            ),
            entry(
                "00000000-0000-0000-0000-000000000002",
                "/library/b",
                "/incoming/b",
            ),
        ],
    };
    assert!(valid.matches(&undo));

    let mut malformed = valid.clone();
    malformed.entries[0].id = "not-a-guid".into();
    assert!(!malformed.matches(&undo));

    let mut duplicate_id = valid.clone();
    duplicate_id.entries[1].id = duplicate_id.entries[0].id.clone();
    assert!(!duplicate_id.matches(&undo));

    let mut duplicate_identity = valid;
    duplicate_identity.entries[1] = duplicate_identity.entries[0].clone();
    assert!(!duplicate_identity.matches(&undo));
}

#[test]
fn organize_reports_recovery_pair_save_failures() {
    let tmp = std::env::temp_dir().join(format!("lo-persist-fail-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let source = tmp.join("incoming").join("B.cbz");
    write(&source);
    let undo_path = tmp.join("undo.dat");
    std::fs::create_dir_all(adoption_manifest_path(&undo_path)).unwrap();
    let books = vec![book("B", "1", 1, &source.to_string_lossy())];
    let selected = [0];
    let profiles = [profile("Adopt", "Move", &tmp.join("library"))];
    let mut context = ctx(&books, &selected, &profiles, &no_trash);
    context.move_landing = MoveLanding::InsertPreservingId;
    context.undo_path = Some(undo_path.clone());

    let mut ui = StubUi::new();
    let report = cr_organize::engine::organize(context, &mut ui);

    assert!(undo_path.is_file());
    assert!(report
        .persistence_error
        .as_deref()
        .is_some_and(|error| error.contains("adoption manifest")));
    let _ = std::fs::remove_dir_all(tmp);
}
