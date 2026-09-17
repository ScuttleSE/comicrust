use std::path::{Path, PathBuf};

use cr_core::durable::durable_replace;
use cr_engine::incoming_transaction::{
    acquire_mutation_guard, commit_database_epoch, database_epoch, operation_active,
    try_acquire_mutation_guard, try_begin_operation, CloseBarrier, CloseDecision,
    ExternalActionStatus, ExternalFileAction, FileSnapshot, IncomingTransaction, RecoveryResult,
    ReplacementStage, ReplacementTransaction, TransactionEngine, TransactionError,
    TransactionFiles, TransactionKind, TransactionStage,
};

struct TestDir(PathBuf);

impl TestDir {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "comicrust-transaction-{name}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn snapshot(path: PathBuf, before: &[u8], after: &[u8]) -> FileSnapshot {
    FileSnapshot {
        path,
        before: Some(before.to_vec()),
        after: after.to_vec(),
        remove_after: false,
    }
}

fn transaction(
    root: &TestDir,
    kind: TransactionKind,
    stage: TransactionStage,
) -> IncomingTransaction {
    let catalog = snapshot(
        root.path("IncomingDb.xml"),
        b"incoming-before",
        b"incoming-after",
    );
    let database = snapshot(
        root.path("ComicDb.xml"),
        b"database-before",
        b"database-after",
    );
    let config = snapshot(
        root.path("comicrust.toml"),
        b"config-before",
        b"config-after",
    );
    IncomingTransaction {
        kind,
        stage,
        files: TransactionFiles {
            incoming_catalog: Some(catalog),
            comic_database: matches!(
                kind,
                TransactionKind::Adoption
                    | TransactionKind::Undo
                    | TransactionKind::FolderConversion
            )
            .then_some(database),
            config: (kind == TransactionKind::FolderConversion).then_some(config),
            auxiliary: Vec::new(),
        },
        external_actions: Vec::new(),
    }
}

fn seed(engine: &TransactionEngine, transaction: &IncomingTransaction) {
    if matches!(
        transaction.stage,
        TransactionStage::DestinationSaved
            | TransactionStage::SourceSaved
            | TransactionStage::AuxiliarySaved
            | TransactionStage::Committed
    ) {
        let destination = match transaction.kind {
            TransactionKind::Adoption => transaction.files.comic_database.as_ref(),
            _ => transaction.files.incoming_catalog.as_ref(),
        };
        if let Some(snapshot) = destination {
            durable_replace(&snapshot.path, &snapshot.after).unwrap();
        }
    }
    if matches!(
        transaction.stage,
        TransactionStage::SourceSaved
            | TransactionStage::AuxiliarySaved
            | TransactionStage::Committed
    ) {
        let source = match transaction.kind {
            TransactionKind::Adoption => transaction.files.incoming_catalog.as_ref(),
            _ => transaction.files.comic_database.as_ref(),
        };
        if let Some(snapshot) = source {
            durable_replace(&snapshot.path, &snapshot.after).unwrap();
        }
    }
    if matches!(
        transaction.stage,
        TransactionStage::AuxiliarySaved | TransactionStage::Committed
    ) {
        if let Some(snapshot) = &transaction.files.config {
            durable_replace(&snapshot.path, &snapshot.after).unwrap();
        }
        for snapshot in &transaction.files.auxiliary {
            durable_replace(&snapshot.path, &snapshot.after).unwrap();
        }
    }
    engine.begin(transaction).unwrap();
}

fn assert_after(root: &TestDir, kind: TransactionKind) {
    assert_eq!(
        std::fs::read(root.path("IncomingDb.xml")).unwrap(),
        b"incoming-after"
    );
    if matches!(
        kind,
        TransactionKind::Adoption | TransactionKind::Undo | TransactionKind::FolderConversion
    ) {
        assert_eq!(
            std::fs::read(root.path("ComicDb.xml")).unwrap(),
            b"database-after"
        );
    }
    if kind == TransactionKind::FolderConversion {
        assert_eq!(
            std::fs::read(root.path("comicrust.toml")).unwrap(),
            b"config-after"
        );
    }
}

#[test]
fn adoption_and_undo_recover_from_every_stage() {
    let stages = [
        TransactionStage::Prepared,
        TransactionStage::ExternalApplied,
        TransactionStage::DestinationSaved,
        TransactionStage::SourceSaved,
        TransactionStage::AuxiliarySaved,
        TransactionStage::Committed,
    ];
    for kind in [TransactionKind::Adoption, TransactionKind::Undo] {
        for stage in stages {
            let root = TestDir::new(&format!("{kind:?}-{stage:?}"));
            let engine = TransactionEngine::from_journal_path(root.path("current.json"));
            let transaction = transaction(&root, kind, stage);
            seed(&engine, &transaction);

            assert_eq!(engine.recover().unwrap(), RecoveryResult::Recovered);
            assert_after(&root, kind);
            assert!(!engine.journal_path().exists());
        }
    }
}

#[test]
fn conversion_recovery_installs_catalog_database_and_config() {
    let root = TestDir::new("conversion");
    let engine = TransactionEngine::from_journal_path(root.path("current.json"));
    let transaction = transaction(
        &root,
        TransactionKind::FolderConversion,
        TransactionStage::DestinationSaved,
    );
    seed(&engine, &transaction);

    assert_eq!(engine.recover().unwrap(), RecoveryResult::Recovered);
    assert_after(&root, TransactionKind::FolderConversion);
}

#[test]
fn scan_recovery_commits_the_incoming_catalog() {
    let root = TestDir::new("scan");
    let engine = TransactionEngine::from_journal_path(root.path("current.json"));
    seed(
        &engine,
        &transaction(&root, TransactionKind::Scan, TransactionStage::Prepared),
    );

    assert_eq!(engine.recover().unwrap(), RecoveryResult::Recovered);
    assert_after(&root, TransactionKind::Scan);
}

#[test]
fn discard_with_an_absent_file_removes_the_record() {
    let root = TestDir::new("discard");
    let engine = TransactionEngine::from_journal_path(root.path("current.json"));
    let deleted = root.path("discarded.cbz");
    let mut transaction = transaction(&root, TransactionKind::Discard, TransactionStage::Prepared);
    transaction
        .external_actions
        .push(ExternalFileAction::Delete {
            source: deleted,
            status: ExternalActionStatus::Pending,
        });
    seed(&engine, &transaction);

    assert_eq!(engine.recover().unwrap(), RecoveryResult::Recovered);
    assert_eq!(
        std::fs::read(root.path("IncomingDb.xml")).unwrap(),
        b"incoming-after"
    );
}

#[test]
fn recovery_does_not_repeat_a_pending_delete() {
    let root = TestDir::new("delete-present");
    let engine = TransactionEngine::from_journal_path(root.path("current.json"));
    let deleted = root.path("discarded.cbz");
    std::fs::write(&deleted, b"comic").unwrap();
    let mut transaction = transaction(&root, TransactionKind::Discard, TransactionStage::Prepared);
    transaction
        .external_actions
        .push(ExternalFileAction::Delete {
            source: deleted.clone(),
            status: ExternalActionStatus::Pending,
        });
    seed(&engine, &transaction);

    assert!(matches!(
        engine.recover(),
        Err(TransactionError::Conflict(_))
    ));
    assert_eq!(std::fs::read(deleted).unwrap(), b"comic");
    assert!(engine.journal_path().exists());
}

#[test]
fn ambiguous_rename_paths_keep_the_journal() {
    for (source_exists, destination_exists) in [(true, true), (false, false)] {
        let root = TestDir::new("rename-conflict");
        let engine = TransactionEngine::from_journal_path(root.path("current.json"));
        let source = root.path("source.cbz");
        let destination = root.path("destination.cbz");
        if source_exists {
            std::fs::write(&source, b"source").unwrap();
        }
        if destination_exists {
            std::fs::write(&destination, b"destination").unwrap();
        }
        let mut transaction =
            transaction(&root, TransactionKind::Adoption, TransactionStage::Prepared);
        transaction
            .external_actions
            .push(ExternalFileAction::Rename {
                source,
                destination,
                status: ExternalActionStatus::Pending,
            });
        seed(&engine, &transaction);

        assert!(matches!(
            engine.recover(),
            Err(TransactionError::Conflict(_))
        ));
        assert!(engine.journal_path().exists());
        assert!(!root.path("IncomingDb.xml").exists());
        assert!(!root.path("ComicDb.xml").exists());
    }
}

#[test]
fn corrupt_journal_changes_no_files() {
    let root = TestDir::new("corrupt");
    let journal = root.path("current.json");
    let catalog = root.path("IncomingDb.xml");
    let database = root.path("ComicDb.xml");
    std::fs::write(&catalog, b"catalog-original").unwrap();
    std::fs::write(&database, b"database-original").unwrap();
    durable_replace(&journal, b"{not json").unwrap();
    let engine = TransactionEngine::from_journal_path(journal);

    assert!(matches!(engine.recover(), Err(TransactionError::Json(_))));
    assert_eq!(std::fs::read(catalog).unwrap(), b"catalog-original");
    assert_eq!(std::fs::read(database).unwrap(), b"database-original");
    assert!(engine.journal_path().exists());
}

#[test]
fn comic_database_bytes_are_installed_without_reformatting() {
    let root = TestDir::new("opaque-database");
    let engine = TransactionEngine::from_journal_path(root.path("current.json"));
    let mut transaction = transaction(&root, TransactionKind::Adoption, TransactionStage::Prepared);
    let exact = b"<?xml version='1.0'?><ComicDatabase>\r\n  unusual bytes\r\n</ComicDatabase>";
    transaction.files.comic_database.as_mut().unwrap().after = exact.to_vec();
    seed(&engine, &transaction);

    assert_eq!(engine.recover().unwrap(), RecoveryResult::Recovered);
    assert_eq!(std::fs::read(root.path("ComicDb.xml")).unwrap(), exact);
}

#[test]
fn commit_applies_a_rename_after_the_journal_exists() {
    let root = TestDir::new("commit");
    let engine = TransactionEngine::from_journal_path(root.path("current.json"));
    let source = root.path("source.cbz");
    let destination = root.path("library/book.cbz");
    std::fs::write(&source, b"comic").unwrap();
    let mut transaction = transaction(&root, TransactionKind::Adoption, TransactionStage::Prepared);
    transaction
        .external_actions
        .push(ExternalFileAction::Rename {
            source: source.clone(),
            destination: destination.clone(),
            status: ExternalActionStatus::Pending,
        });
    engine.begin(&transaction).unwrap();

    engine.commit(&mut transaction).unwrap();
    assert!(!source.exists());
    assert_eq!(std::fs::read(destination).unwrap(), b"comic");
    assert_after(&root, TransactionKind::Adoption);
    assert!(!engine.journal_path().exists());
}

#[test]
fn mutation_guard_serializes_process_local_callers() {
    let guard = acquire_mutation_guard();
    assert!(guard.is_held());
    assert!(try_acquire_mutation_guard().is_none());
    drop(guard);
    assert!(try_acquire_mutation_guard().is_some());
}

#[test]
fn close_barrier_stops_once_and_proceeds_after_the_coordinator_is_idle() {
    let mut barrier = CloseBarrier::default();
    assert_eq!(barrier.request(), CloseDecision::StartWait);
    assert_eq!(barrier.request(), CloseDecision::Stop);
    assert!(barrier.coordinator_became_idle());
    assert_eq!(barrier.request(), CloseDecision::Proceed);
}

#[test]
fn idle_close_barrier_waits_for_the_completion_signal() {
    let mut barrier = CloseBarrier::default();
    assert_eq!(barrier.request(), CloseDecision::StartWait);
    assert!(barrier.coordinator_became_idle());
    assert_eq!(barrier.request(), CloseDecision::Proceed);
}

#[test]
fn failed_close_save_returns_the_barrier_to_idle() {
    let mut barrier = CloseBarrier::default();
    assert_eq!(barrier.request(), CloseDecision::StartWait);
    barrier.save_failed();
    assert_eq!(barrier.request(), CloseDecision::StartWait);
}

#[test]
fn operation_stays_active_through_landing_and_blocks_another_start() {
    assert!(!operation_active());
    let operation = try_begin_operation().expect("start operation");
    assert!(operation_active());
    assert!(try_begin_operation().is_none());
    assert!(operation_active());
    assert!(try_begin_operation().is_none());
    assert!(operation_active());
    let mut landed_while_active = false;
    operation.finish(|_| {
        landed_while_active = operation_active();
        assert!(try_begin_operation().is_none());
    });
    assert!(landed_while_active);
    assert!(!operation_active());
}

#[test]
fn dropping_operation_without_landing_releases_exclusive_start() {
    assert!(!operation_active());
    let operation = try_begin_operation().expect("start operation");
    drop(operation);
    assert!(!operation_active());
    drop(try_begin_operation().expect("start operation after disconnect"));
    assert!(!operation_active());
}

#[test]
fn journal_path_uses_the_private_incoming_directory() {
    let root = TestDir::new("path");
    let paths = cr_core::paths::Paths::from_xdg_root(Path::new(&root.0));
    let engine = TransactionEngine::new(&paths);
    assert_eq!(
        engine.journal_path(),
        root.0.join("comicrust/Incoming/Transactions/current.json")
    );
}

#[test]
fn recovery_removes_an_auxiliary_file_marked_for_removal() {
    let root = TestDir::new("remove-auxiliary");
    let engine = TransactionEngine::from_journal_path(root.path("current.json"));
    let undo = root.path("undo.dat");
    std::fs::write(&undo, b"old undo").unwrap();
    let mut transaction = transaction(&root, TransactionKind::Undo, TransactionStage::Prepared);
    transaction.files.auxiliary.push(FileSnapshot {
        path: undo.clone(),
        before: Some(b"old undo".to_vec()),
        after: Vec::new(),
        remove_after: true,
    });
    seed(&engine, &transaction);

    assert_eq!(engine.recover().unwrap(), RecoveryResult::Recovered);
    assert!(!undo.exists());
    assert!(!engine.journal_path().exists());
}

#[test]
fn abort_prepared_removes_only_an_untouched_prepared_journal() {
    let root = TestDir::new("abort-prepared");
    let engine = TransactionEngine::from_journal_path(root.path("current.json"));
    let transaction = transaction(&root, TransactionKind::Scan, TransactionStage::Prepared);
    engine.begin(&transaction).unwrap();
    engine.abort_prepared().unwrap();
    assert!(!engine.journal_path().exists());

    let mut applied = transaction;
    applied.external_actions.push(ExternalFileAction::Delete {
        source: root.path("gone.cbz"),
        status: ExternalActionStatus::Applied,
    });
    engine.begin(&applied).unwrap();
    assert!(matches!(
        engine.abort_prepared(),
        Err(TransactionError::Invalid(_))
    ));
    assert!(engine.journal_path().exists());

    durable_replace(engine.journal_path(), b"").unwrap();
    let mut pending =
        crate::transaction(&root, TransactionKind::Discard, TransactionStage::Prepared);
    pending.external_actions.push(ExternalFileAction::Delete {
        source: root.path("pending.cbz"),
        status: ExternalActionStatus::Pending,
    });
    durable_replace(
        engine.journal_path(),
        &serde_json::to_vec_pretty(&pending).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        engine.abort_prepared(),
        Err(TransactionError::Invalid(_))
    ));
}

#[test]
fn epoch_compare_and_commit_excludes_mutation_until_commit_finishes() {
    let root = TestDir::new("epoch-commit");
    let engine = TransactionEngine::from_journal_path(root.path("current.json"));
    let mut transaction = transaction(&root, TransactionKind::Scan, TransactionStage::Prepared);
    engine.begin(&transaction).unwrap();
    let expected = database_epoch();
    let guard = acquire_mutation_guard();

    let blocked = std::thread::spawn(|| try_acquire_mutation_guard().is_none());
    assert!(blocked.join().unwrap());
    let committed = guard
        .commit_if_epoch(expected, &engine, &mut transaction)
        .unwrap();
    assert_ne!(committed, expected);
    assert!(try_acquire_mutation_guard().is_none());
    drop(guard);
    assert!(try_acquire_mutation_guard().is_some());
}

#[test]
fn replacement_epoch_invalidates_a_rescan_waiting_for_the_mutation_guard() {
    let replacement_guard = acquire_mutation_guard();
    let scan_epoch = database_epoch();
    let (waiting_tx, waiting_rx) = std::sync::mpsc::channel();
    let scan = std::thread::spawn(move || {
        waiting_tx.send(()).unwrap();
        let _scan_guard = acquire_mutation_guard();
        database_epoch()
    });

    waiting_rx.recv().unwrap();
    let replacement_epoch = commit_database_epoch();
    drop(replacement_guard);
    let scan_start_epoch = scan.join().unwrap();

    assert_ne!(replacement_epoch, scan_epoch);
    assert_eq!(scan_start_epoch, replacement_epoch);
    assert_ne!(scan_start_epoch, scan_epoch);
}

#[test]
fn conversion_and_scan_epoch_rejections_remove_the_prepared_journal() {
    for kind in [TransactionKind::FolderConversion, TransactionKind::Scan] {
        let root = TestDir::new(&format!("epoch-reject-{kind:?}"));
        let engine = TransactionEngine::from_journal_path(root.path("current.json"));
        let mut transaction = transaction(&root, kind, TransactionStage::Prepared);
        engine.begin(&transaction).unwrap();
        let stale_epoch = database_epoch().wrapping_sub(1);
        let guard = acquire_mutation_guard();

        assert!(matches!(
            guard.commit_if_epoch(stale_epoch, &engine, &mut transaction),
            Err(TransactionError::EpochChanged)
        ));
        engine.abort_prepared().unwrap();
        assert!(!engine.journal_path().exists());
    }
}

#[test]
fn recovery_accepts_applied_delete_then_applied_rename_to_the_same_path_at_every_stage() {
    for stage in [
        TransactionStage::Prepared,
        TransactionStage::ExternalApplied,
        TransactionStage::DestinationSaved,
        TransactionStage::SourceSaved,
        TransactionStage::AuxiliarySaved,
        TransactionStage::Committed,
    ] {
        let root = TestDir::new(&format!("overwrite-{stage:?}"));
        let engine = TransactionEngine::from_journal_path(root.path("current.json"));
        let source = root.path("source.cbz");
        let destination = root.path("destination.cbz");
        std::fs::write(&destination, b"replacement").unwrap();
        let mut transaction = transaction(&root, TransactionKind::Adoption, stage);
        transaction.external_actions = vec![
            ExternalFileAction::Delete {
                source: destination.clone(),
                status: ExternalActionStatus::Applied,
            },
            ExternalFileAction::Rename {
                source,
                destination: destination.clone(),
                status: ExternalActionStatus::Applied,
            },
        ];
        seed(&engine, &transaction);

        assert_eq!(engine.recover().unwrap(), RecoveryResult::Recovered);
        assert_eq!(std::fs::read(destination).unwrap(), b"replacement");
        assert_after(&root, TransactionKind::Adoption);
    }
}

fn replacement(root: &TestDir) -> ReplacementTransaction {
    let source = root.path("incoming/book.cbz");
    let old_library = root.path("library/book.cbr");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::create_dir_all(old_library.parent().unwrap()).unwrap();
    std::fs::write(&source, b"replacement comic bytes").unwrap();
    std::fs::write(&old_library, b"old library comic").unwrap();
    ReplacementTransaction::prepare(
        source,
        root.path("library/.book.cbz.incoming-stage"),
        root.path("library/book.cbz"),
        old_library,
        snapshot(
            root.path("ComicDb.xml"),
            b"database-before",
            b"database-after",
        ),
        snapshot(
            root.path("IncomingDb.xml"),
            b"incoming-before",
            b"incoming-after",
        ),
    )
    .unwrap()
}

fn write_replacement_journal(engine: &TransactionEngine, transaction: &ReplacementTransaction) {
    durable_replace(
        engine.journal_path(),
        &serde_json::to_vec_pretty(transaction).unwrap(),
    )
    .unwrap();
}

fn fake_trash(root: &TestDir) -> impl Fn(&Path) -> std::io::Result<()> + '_ {
    move |path| {
        let trash = root.path("trash");
        std::fs::create_dir_all(&trash)?;
        std::fs::rename(path, trash.join(path.file_name().unwrap()))
    }
}

fn seed_replacement_stage(
    root: &TestDir,
    transaction: &mut ReplacementTransaction,
    stage: ReplacementStage,
) {
    transaction.stage = stage;
    if matches!(
        stage,
        ReplacementStage::Staged | ReplacementStage::TrashPending
    ) {
        std::fs::copy(&transaction.source, &transaction.staging).unwrap();
    }
    if matches!(
        stage,
        ReplacementStage::OldLibraryTrashed
            | ReplacementStage::Installed
            | ReplacementStage::DatabaseSaved
            | ReplacementStage::IncomingSaved
            | ReplacementStage::SourceRemoved
            | ReplacementStage::Committed
    ) {
        let trash = fake_trash(root);
        trash(&transaction.old_library).unwrap();
    }
    if matches!(
        stage,
        ReplacementStage::OldLibraryTrashed
            | ReplacementStage::Installed
            | ReplacementStage::DatabaseSaved
            | ReplacementStage::IncomingSaved
            | ReplacementStage::SourceRemoved
            | ReplacementStage::Committed
    ) {
        if stage == ReplacementStage::OldLibraryTrashed {
            std::fs::copy(&transaction.source, &transaction.staging).unwrap();
        } else {
            std::fs::copy(&transaction.source, &transaction.destination).unwrap();
        }
    }
    if matches!(
        stage,
        ReplacementStage::DatabaseSaved
            | ReplacementStage::IncomingSaved
            | ReplacementStage::SourceRemoved
            | ReplacementStage::Committed
    ) {
        durable_replace(
            &transaction.comic_database.path,
            &transaction.comic_database.after,
        )
        .unwrap();
    }
    if matches!(
        stage,
        ReplacementStage::IncomingSaved
            | ReplacementStage::SourceRemoved
            | ReplacementStage::Committed
    ) {
        durable_replace(
            &transaction.incoming_catalog.path,
            &transaction.incoming_catalog.after,
        )
        .unwrap();
    }
    if matches!(
        stage,
        ReplacementStage::SourceRemoved | ReplacementStage::Committed
    ) {
        std::fs::remove_file(&transaction.source).unwrap();
    }
}

#[test]
fn replacement_recovers_from_every_persisted_stage() {
    for stage in [
        ReplacementStage::Prepared,
        ReplacementStage::Staged,
        ReplacementStage::TrashPending,
        ReplacementStage::OldLibraryTrashed,
        ReplacementStage::Installed,
        ReplacementStage::DatabaseSaved,
        ReplacementStage::IncomingSaved,
        ReplacementStage::SourceRemoved,
        ReplacementStage::Committed,
    ] {
        let root = TestDir::new(&format!("replacement-{stage:?}"));
        let engine = TransactionEngine::from_journal_path(root.path("current.json"));
        let mut transaction = replacement(&root);
        seed_replacement_stage(&root, &mut transaction, stage);
        write_replacement_journal(&engine, &transaction);

        assert_eq!(
            engine.recover_with_trash(&fake_trash(&root)).unwrap(),
            RecoveryResult::Recovered
        );
        assert_eq!(
            std::fs::read(&transaction.destination).unwrap(),
            b"replacement comic bytes"
        );
        assert_eq!(
            std::fs::read(&transaction.comic_database.path).unwrap(),
            b"database-after"
        );
        assert_eq!(
            std::fs::read(&transaction.incoming_catalog.path).unwrap(),
            b"incoming-after"
        );
        assert!(!transaction.source.exists());
        assert!(!transaction.old_library.exists());
        assert!(!transaction.staging.exists());
        assert!(!engine.journal_path().exists());
    }
}

#[test]
fn replacement_recovers_from_each_action_before_its_stage_update() {
    for crash_point in [
        "partial-copy",
        "copy",
        "trash",
        "install-linked",
        "install",
        "database",
        "incoming",
        "remove",
    ] {
        let root = TestDir::new(&format!("replacement-crash-{crash_point}"));
        let engine = TransactionEngine::from_journal_path(root.path("current.json"));
        let mut transaction = replacement(&root);
        std::fs::create_dir_all(transaction.destination.parent().unwrap()).unwrap();
        match crash_point {
            "partial-copy" => {
                std::fs::write(&transaction.staging, b"partial").unwrap();
            }
            "copy" => {
                std::fs::copy(&transaction.source, &transaction.staging).unwrap();
            }
            "trash" => {
                std::fs::copy(&transaction.source, &transaction.staging).unwrap();
                fake_trash(&root)(&transaction.old_library).unwrap();
                transaction.stage = ReplacementStage::TrashPending;
            }
            "install-linked" => {
                std::fs::copy(&transaction.source, &transaction.staging).unwrap();
                fake_trash(&root)(&transaction.old_library).unwrap();
                std::fs::hard_link(&transaction.staging, &transaction.destination).unwrap();
                transaction.stage = ReplacementStage::OldLibraryTrashed;
            }
            "install" => {
                std::fs::copy(&transaction.source, &transaction.destination).unwrap();
                fake_trash(&root)(&transaction.old_library).unwrap();
                transaction.stage = ReplacementStage::OldLibraryTrashed;
            }
            "database" => {
                std::fs::copy(&transaction.source, &transaction.destination).unwrap();
                fake_trash(&root)(&transaction.old_library).unwrap();
                durable_replace(
                    &transaction.comic_database.path,
                    &transaction.comic_database.after,
                )
                .unwrap();
                transaction.stage = ReplacementStage::Installed;
            }
            "incoming" => {
                std::fs::copy(&transaction.source, &transaction.destination).unwrap();
                fake_trash(&root)(&transaction.old_library).unwrap();
                durable_replace(
                    &transaction.comic_database.path,
                    &transaction.comic_database.after,
                )
                .unwrap();
                durable_replace(
                    &transaction.incoming_catalog.path,
                    &transaction.incoming_catalog.after,
                )
                .unwrap();
                transaction.stage = ReplacementStage::DatabaseSaved;
            }
            "remove" => {
                std::fs::copy(&transaction.source, &transaction.destination).unwrap();
                fake_trash(&root)(&transaction.old_library).unwrap();
                durable_replace(
                    &transaction.comic_database.path,
                    &transaction.comic_database.after,
                )
                .unwrap();
                durable_replace(
                    &transaction.incoming_catalog.path,
                    &transaction.incoming_catalog.after,
                )
                .unwrap();
                std::fs::remove_file(&transaction.source).unwrap();
                transaction.stage = ReplacementStage::IncomingSaved;
            }
            _ => unreachable!(),
        }
        write_replacement_journal(&engine, &transaction);

        assert_eq!(
            engine.recover_with_trash(&fake_trash(&root)).unwrap(),
            RecoveryResult::Recovered
        );
        assert_eq!(
            std::fs::read(&transaction.destination).unwrap(),
            b"replacement comic bytes"
        );
        assert!(!transaction.source.exists());
        assert!(!transaction.staging.exists());
    }
}

#[test]
fn replacement_rejects_a_destination_collision_without_mutation() {
    let root = TestDir::new("replacement-collision");
    let engine = TransactionEngine::from_journal_path(root.path("current.json"));
    let transaction = replacement(&root);
    std::fs::create_dir_all(transaction.destination.parent().unwrap()).unwrap();
    std::fs::write(&transaction.destination, b"existing library comic").unwrap();

    assert!(matches!(
        engine.begin_replacement(&transaction),
        Err(TransactionError::Conflict(_))
    ));
    assert_eq!(
        std::fs::read(&transaction.source).unwrap(),
        b"replacement comic bytes"
    );
    assert_eq!(
        std::fs::read(&transaction.old_library).unwrap(),
        b"old library comic"
    );
    assert_eq!(
        std::fs::read(&transaction.destination).unwrap(),
        b"existing library comic"
    );
    assert!(!transaction.staging.exists());
    assert!(!engine.journal_path().exists());
}

#[test]
fn same_extension_existing_library_destination_is_expected() {
    let root = TestDir::new("replacement-same-extension");
    let engine = TransactionEngine::from_journal_path(root.path("current.json"));
    let source = root.path("incoming/book.cbz");
    let destination = root.path("library/book.cbz");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
    std::fs::write(&source, b"replacement comic bytes").unwrap();
    std::fs::write(&destination, b"old library comic").unwrap();
    let mut transaction = ReplacementTransaction::prepare(
        source,
        root.path("library/.book.cbz.incoming-stage"),
        destination.clone(),
        destination,
        snapshot(root.path("ComicDb.xml"), b"old-db", b"new-db"),
        snapshot(root.path("IncomingDb.xml"), b"old-in", b"new-in"),
    )
    .unwrap();
    engine.begin_replacement(&transaction).unwrap();

    engine
        .commit_replacement_with_trash(&mut transaction, &fake_trash(&root))
        .unwrap();
    assert_eq!(
        std::fs::read(&transaction.destination).unwrap(),
        b"replacement comic bytes"
    );
    assert_eq!(
        std::fs::read(root.path("trash/book.cbz")).unwrap(),
        b"old library comic"
    );
}

#[test]
fn replacement_copy_failure_does_not_mutate_source_or_destination() {
    let root = TestDir::new("replacement-copy-failure");
    let engine = TransactionEngine::from_journal_path(root.path("current.json"));
    let mut transaction = replacement(&root);
    engine.begin_replacement(&transaction).unwrap();
    let fail_copy = |_transaction: &ReplacementTransaction| {
        Err(TransactionError::Io(std::io::Error::other(
            "test copy failure",
        )))
    };

    assert!(matches!(
        engine.commit_replacement_with_actions(&mut transaction, &fail_copy, &fake_trash(&root)),
        Err(TransactionError::Io(_))
    ));
    assert_eq!(
        std::fs::read(&transaction.source).unwrap(),
        b"replacement comic bytes"
    );
    assert!(!transaction.staging.exists());
    assert!(!transaction.destination.exists());
    assert!(engine.journal_path().exists());
}

#[test]
fn replacement_rejects_an_invalid_staged_copy() {
    let root = TestDir::new("replacement-invalid-staging");
    let engine = TransactionEngine::from_journal_path(root.path("current.json"));
    let mut transaction = replacement(&root);
    std::fs::create_dir_all(transaction.staging.parent().unwrap()).unwrap();
    std::fs::write(&transaction.staging, b"truncated").unwrap();
    transaction.stage = ReplacementStage::Staged;
    write_replacement_journal(&engine, &transaction);

    assert!(matches!(
        engine.recover_with_trash(&fake_trash(&root)),
        Err(TransactionError::Conflict(_))
    ));
    assert_eq!(std::fs::read(&transaction.staging).unwrap(), b"truncated");
    assert!(!transaction.destination.exists());
    assert_eq!(
        std::fs::read(&transaction.source).unwrap(),
        b"replacement comic bytes"
    );
    assert!(engine.journal_path().exists());
}

#[test]
fn replacement_rejects_an_invalid_installed_file_before_catalog_changes() {
    let root = TestDir::new("replacement-invalid-installed");
    let engine = TransactionEngine::from_journal_path(root.path("current.json"));
    let mut transaction = replacement(&root);
    fake_trash(&root)(&transaction.old_library).unwrap();
    std::fs::write(&transaction.destination, b"invalid installed file").unwrap();
    transaction.stage = ReplacementStage::Installed;
    write_replacement_journal(&engine, &transaction);

    assert!(matches!(
        engine.recover_with_trash(&fake_trash(&root)),
        Err(TransactionError::Conflict(_))
    ));
    assert_eq!(
        std::fs::read(&transaction.destination).unwrap(),
        b"invalid installed file"
    );
    assert_eq!(
        std::fs::read(&transaction.source).unwrap(),
        b"replacement comic bytes"
    );
    assert!(!transaction.comic_database.path.exists());
    assert!(!transaction.incoming_catalog.path.exists());
    assert!(engine.journal_path().exists());
}

#[test]
fn replacement_trash_failure_keeps_files_and_catalogs() {
    let root = TestDir::new("replacement-trash-failure");
    let engine = TransactionEngine::from_journal_path(root.path("current.json"));
    let mut transaction = replacement(&root);
    engine.begin_replacement(&transaction).unwrap();
    let fail = |_path: &Path| Err(std::io::Error::other("test trash failure"));

    assert!(matches!(
        engine.commit_replacement_with_trash(&mut transaction, &fail),
        Err(TransactionError::Io(_))
    ));
    assert_eq!(
        std::fs::read(&transaction.old_library).unwrap(),
        b"old library comic"
    );
    assert_eq!(
        std::fs::read(&transaction.source).unwrap(),
        b"replacement comic bytes"
    );
    assert!(!transaction.destination.exists());
    assert!(!transaction.comic_database.path.exists());
    assert!(!transaction.incoming_catalog.path.exists());
    assert!(engine.journal_path().exists());
}

#[test]
fn replacement_recovery_is_idempotent_after_completion() {
    let root = TestDir::new("replacement-idempotent");
    let engine = TransactionEngine::from_journal_path(root.path("current.json"));
    let transaction = replacement(&root);
    engine.begin_replacement(&transaction).unwrap();

    assert_eq!(
        engine.recover_with_trash(&fake_trash(&root)).unwrap(),
        RecoveryResult::Recovered
    );
    assert_eq!(
        engine.recover_with_trash(&fake_trash(&root)).unwrap(),
        RecoveryResult::NoJournal
    );
    assert_eq!(
        std::fs::read(&transaction.destination).unwrap(),
        b"replacement comic bytes"
    );
    assert!(!transaction.source.exists());
}
