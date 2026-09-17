use std::path::{Path, PathBuf};

use cr_core::durable::durable_replace;
use cr_engine::incoming_transaction::{
    acquire_mutation_guard, database_epoch, operation_active, try_acquire_mutation_guard,
    try_begin_operation, CloseBarrier, CloseDecision, ExternalActionStatus, ExternalFileAction,
    FileSnapshot, IncomingTransaction, RecoveryResult, TransactionEngine, TransactionError,
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
