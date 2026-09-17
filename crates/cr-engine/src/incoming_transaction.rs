//! Durable transactions for Incoming catalog mutations.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, TryLockError};

use cr_core::durable::{durable_remove, durable_replace};
use cr_core::paths::{incoming_transaction_file, Paths};
use serde::{Deserialize, Serialize};

static MUTATION_LOCK: Mutex<()> = Mutex::new(());
static DATABASE_EPOCH: AtomicU64 = AtomicU64::new(0);
static ACTIVE_OPERATIONS: AtomicU64 = AtomicU64::new(0);

pub fn database_epoch() -> u64 {
    DATABASE_EPOCH.load(Ordering::Acquire)
}

pub fn advance_database_epoch() {
    DATABASE_EPOCH.fetch_add(1, Ordering::AcqRel);
}

/// Advances the epoch and returns the value that identifies the new state.
pub fn commit_database_epoch() -> u64 {
    DATABASE_EPOCH
        .fetch_add(1, Ordering::AcqRel)
        .wrapping_add(1)
}

pub fn begin_operation() -> bool {
    ACTIVE_OPERATIONS
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
}

pub fn end_operation() {
    let previous = ACTIVE_OPERATIONS.fetch_sub(1, Ordering::AcqRel);
    debug_assert!(previous > 0);
}

pub fn operation_active() -> bool {
    ACTIVE_OPERATIONS.load(Ordering::Acquire) != 0
}

/// Owns one exclusive operation from launch through main-thread landing.
pub struct ActiveOperation(());

impl ActiveOperation {
    /// Runs the main-thread landing before this operation becomes idle.
    pub fn finish(self, landing: impl FnOnce(&Self)) {
        landing(&self);
    }
}

impl Drop for ActiveOperation {
    fn drop(&mut self) {
        end_operation();
    }
}

/// Starts an operation only when no other operation is active.
pub fn try_begin_operation() -> Option<ActiveOperation> {
    begin_operation().then(|| ActiveOperation(()))
}

/// Holds the process-local right to mutate Incoming state.
pub struct MutationGuard(MutexGuard<'static, ()>);

/// Waits until this process has no other active Incoming mutation.
pub fn acquire_mutation_guard() -> MutationGuard {
    MutationGuard(
        MUTATION_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    )
}

/// Tries to acquire the process-local Incoming mutation guard.
pub fn try_acquire_mutation_guard() -> Option<MutationGuard> {
    match MUTATION_LOCK.try_lock() {
        Ok(guard) => Some(MutationGuard(guard)),
        Err(TryLockError::Poisoned(error)) => Some(MutationGuard(error.into_inner())),
        Err(TryLockError::WouldBlock) => None,
    }
}

impl MutationGuard {
    /// Keeps the guard value observable without exposing the mutex internals.
    pub fn is_held(&self) -> bool {
        let _ = &self.0;
        true
    }

    /// Commits only when `expected_epoch` still identifies the live catalogs.
    pub fn commit_if_epoch(
        &self,
        expected_epoch: u64,
        engine: &TransactionEngine,
        transaction: &mut IncomingTransaction,
    ) -> Result<u64, TransactionError> {
        if database_epoch() != expected_epoch {
            return Err(TransactionError::EpochChanged);
        }
        engine.commit(transaction)?;
        Ok(commit_database_epoch())
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CloseBarrier {
    #[default]
    Idle,
    Waiting,
    Ready,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseDecision {
    StartWait,
    Stop,
    Proceed,
}

impl CloseBarrier {
    pub fn request(&mut self) -> CloseDecision {
        match *self {
            Self::Ready => CloseDecision::Proceed,
            Self::Idle => {
                *self = Self::Waiting;
                CloseDecision::StartWait
            }
            Self::Waiting => CloseDecision::Stop,
        }
    }

    pub fn coordinator_became_idle(&mut self) -> bool {
        if *self == Self::Waiting {
            *self = Self::Ready;
            true
        } else {
            false
        }
    }

    pub fn save_failed(&mut self) {
        *self = Self::Idle;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionKind {
    Adoption,
    Undo,
    Discard,
    Scan,
    FolderConversion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionStage {
    Prepared,
    ExternalApplied,
    DestinationSaved,
    SourceSaved,
    AuxiliarySaved,
    Committed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileSnapshot {
    pub path: PathBuf,
    pub before: Option<Vec<u8>>,
    pub after: Vec<u8>,
    #[serde(default)]
    pub remove_after: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransactionFiles {
    pub incoming_catalog: Option<FileSnapshot>,
    pub comic_database: Option<FileSnapshot>,
    pub config: Option<FileSnapshot>,
    #[serde(default)]
    pub auxiliary: Vec<FileSnapshot>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExternalActionStatus {
    Pending,
    Applied,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation")]
pub enum ExternalFileAction {
    Rename {
        source: PathBuf,
        destination: PathBuf,
        status: ExternalActionStatus,
    },
    Delete {
        source: PathBuf,
        status: ExternalActionStatus,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncomingTransaction {
    pub kind: TransactionKind,
    pub stage: TransactionStage,
    pub files: TransactionFiles,
    #[serde(default)]
    pub external_actions: Vec<ExternalFileAction>,
}

#[derive(Debug)]
pub enum TransactionError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Active,
    EpochChanged,
    Invalid(String),
    Conflict(String),
}

impl std::fmt::Display for TransactionError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "Incoming transaction I/O error: {error}"),
            Self::Json(error) => write!(formatter, "Incoming transaction JSON error: {error}"),
            Self::Active => formatter.write_str("an Incoming transaction is already active"),
            Self::EpochChanged => {
                formatter.write_str("the library changed during the Incoming transaction")
            }
            Self::Invalid(message) => write!(formatter, "invalid Incoming transaction: {message}"),
            Self::Conflict(message) => {
                write!(formatter, "Incoming transaction conflict: {message}")
            }
        }
    }
}

impl std::error::Error for TransactionError {}

impl From<std::io::Error> for TransactionError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for TransactionError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

/// Result of checking the one journal during startup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryResult {
    NoJournal,
    Recovered,
}

pub struct TransactionEngine {
    journal_path: PathBuf,
}

impl TransactionEngine {
    pub fn new(paths: &Paths) -> Self {
        Self {
            journal_path: incoming_transaction_file(paths),
        }
    }

    pub fn from_journal_path(journal_path: PathBuf) -> Self {
        Self { journal_path }
    }

    pub fn journal_path(&self) -> &Path {
        &self.journal_path
    }

    /// Creates the durable journal before the caller performs external work.
    pub fn begin(&self, transaction: &IncomingTransaction) -> Result<(), TransactionError> {
        validate(transaction)?;
        if self.journal_path.exists() {
            return Err(TransactionError::Active);
        }
        self.write_journal(transaction)
    }

    /// Replaces the durable journal with the caller's current state.
    pub fn update(&self, transaction: &IncomingTransaction) -> Result<(), TransactionError> {
        validate(transaction)?;
        if !self.journal_path.exists() {
            return Err(TransactionError::Invalid(
                "the current journal does not exist".into(),
            ));
        }
        self.write_journal(transaction)
    }

    /// Removes a journal only when no external action or after-image was applied.
    pub fn abort_prepared(&self) -> Result<(), TransactionError> {
        let bytes = match std::fs::read(&self.journal_path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        let transaction: IncomingTransaction = serde_json::from_slice(&bytes)?;
        if transaction.stage != TransactionStage::Prepared
            || !transaction.external_actions.is_empty()
        {
            return Err(TransactionError::Invalid(
                "only an untouched Prepared journal can be aborted".into(),
            ));
        }
        durable_remove(&self.journal_path)?;
        Ok(())
    }

    /// Applies external actions and installs all after-images in kind-specific order.
    pub fn commit(&self, transaction: &mut IncomingTransaction) -> Result<(), TransactionError> {
        if !self.journal_path.exists() {
            return Err(TransactionError::Invalid(
                "the current journal does not exist".into(),
            ));
        }
        validate(transaction)?;
        self.write_journal(transaction)?;
        self.apply_external_actions(transaction)?;
        self.install_after_images(transaction)
    }

    /// Rolls the current journal forward. Conflicts leave the journal unchanged.
    pub fn recover(&self) -> Result<RecoveryResult, TransactionError> {
        let bytes = match std::fs::read(&self.journal_path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(RecoveryResult::NoJournal)
            }
            Err(error) => return Err(error.into()),
        };
        let mut transaction: IncomingTransaction = serde_json::from_slice(&bytes)?;
        validate(&transaction)?;
        if transaction.stage == TransactionStage::Committed {
            durable_remove(&self.journal_path)?;
            return Ok(RecoveryResult::Recovered);
        }
        reconcile_external_actions(&mut transaction)?;
        self.write_journal(&transaction)?;
        transaction.stage = TransactionStage::ExternalApplied;
        self.write_journal(&transaction)?;
        self.install_after_images(&mut transaction)?;
        Ok(RecoveryResult::Recovered)
    }

    fn apply_external_actions(
        &self,
        transaction: &mut IncomingTransaction,
    ) -> Result<(), TransactionError> {
        for index in 0..transaction.external_actions.len() {
            let action = &mut transaction.external_actions[index];
            match action {
                ExternalFileAction::Rename {
                    source,
                    destination,
                    status,
                } => {
                    if *status == ExternalActionStatus::Pending {
                        if !source.exists() || destination.exists() {
                            return Err(rename_conflict(source, destination));
                        }
                        let parent = destination.parent().ok_or_else(|| {
                            TransactionError::Invalid("rename destination has no parent".into())
                        })?;
                        std::fs::create_dir_all(parent)?;
                        std::fs::rename(&*source, &*destination)?;
                        sync_parent(source)?;
                        if source.parent() != destination.parent() {
                            sync_parent(destination)?;
                        }
                        *status = ExternalActionStatus::Applied;
                    }
                }
                ExternalFileAction::Delete { source, status } => {
                    if *status == ExternalActionStatus::Pending {
                        durable_remove(source)?;
                        *status = ExternalActionStatus::Applied;
                    }
                }
            }
            self.write_journal(transaction)?;
        }
        transaction.stage = TransactionStage::ExternalApplied;
        self.write_journal(transaction)
    }

    fn install_after_images(
        &self,
        transaction: &mut IncomingTransaction,
    ) -> Result<(), TransactionError> {
        let (destination, source) = catalog_order(transaction);
        let destination = destination.cloned();
        let source = source.cloned();
        if let Some(snapshot) = destination {
            install_snapshot(&snapshot)?;
        }
        transaction.stage = TransactionStage::DestinationSaved;
        self.write_journal(transaction)?;

        if let Some(snapshot) = source {
            install_snapshot(&snapshot)?;
        }
        transaction.stage = TransactionStage::SourceSaved;
        self.write_journal(transaction)?;

        if let Some(snapshot) = &transaction.files.config {
            install_snapshot(snapshot)?;
        }
        for snapshot in &transaction.files.auxiliary {
            install_snapshot(snapshot)?;
        }
        transaction.stage = TransactionStage::AuxiliarySaved;
        self.write_journal(transaction)?;

        transaction.stage = TransactionStage::Committed;
        self.write_journal(transaction)?;
        durable_remove(&self.journal_path)?;
        Ok(())
    }

    fn write_journal(&self, transaction: &IncomingTransaction) -> Result<(), TransactionError> {
        let bytes = serde_json::to_vec_pretty(transaction)?;
        durable_replace(&self.journal_path, &bytes)?;
        Ok(())
    }
}

fn install_snapshot(snapshot: &FileSnapshot) -> Result<(), TransactionError> {
    if snapshot.remove_after {
        durable_remove(&snapshot.path)?;
    } else {
        durable_replace(&snapshot.path, &snapshot.after)?;
    }
    Ok(())
}

fn validate(transaction: &IncomingTransaction) -> Result<(), TransactionError> {
    let catalog = transaction.files.incoming_catalog.is_some();
    let database = transaction.files.comic_database.is_some();
    let config = transaction.files.config.is_some();
    let valid = match transaction.kind {
        TransactionKind::Adoption | TransactionKind::Undo => catalog && database,
        TransactionKind::FolderConversion => catalog && database && config,
        TransactionKind::Discard | TransactionKind::Scan => catalog,
    };
    if valid {
        Ok(())
    } else {
        Err(TransactionError::Invalid(format!(
            "missing required snapshots for {:?}",
            transaction.kind
        )))
    }
}

fn catalog_order(
    transaction: &IncomingTransaction,
) -> (Option<&FileSnapshot>, Option<&FileSnapshot>) {
    match transaction.kind {
        TransactionKind::Adoption => (
            transaction.files.comic_database.as_ref(),
            transaction.files.incoming_catalog.as_ref(),
        ),
        TransactionKind::Undo
        | TransactionKind::FolderConversion
        | TransactionKind::Discard
        | TransactionKind::Scan => (
            transaction.files.incoming_catalog.as_ref(),
            transaction.files.comic_database.as_ref(),
        ),
    }
}

fn reconcile_external_actions(
    transaction: &mut IncomingTransaction,
) -> Result<(), TransactionError> {
    for action in &mut transaction.external_actions {
        match action {
            ExternalFileAction::Rename {
                source,
                destination,
                status,
            } if *status == ExternalActionStatus::Applied => {}
            ExternalFileAction::Rename {
                source,
                destination,
                status,
            } => match (source.exists(), destination.exists()) {
                (true, false) => {
                    let parent = destination.parent().ok_or_else(|| {
                        TransactionError::Invalid("rename destination has no parent".into())
                    })?;
                    std::fs::create_dir_all(parent)?;
                    std::fs::rename(&*source, &*destination)?;
                    sync_parent(source)?;
                    if source.parent() != destination.parent() {
                        sync_parent(destination)?;
                    }
                    *status = ExternalActionStatus::Applied;
                }
                (false, true) => *status = ExternalActionStatus::Applied,
                _ => return Err(rename_conflict(source, destination)),
            },
            ExternalFileAction::Delete { status, .. }
                if *status == ExternalActionStatus::Applied => {}
            ExternalFileAction::Delete { source, status } => {
                if source.exists() {
                    return Err(TransactionError::Conflict(format!(
                        "delete source still exists: {}",
                        source.display()
                    )));
                }
                *status = ExternalActionStatus::Applied;
            }
        }
    }
    Ok(())
}

fn rename_conflict(source: &Path, destination: &Path) -> TransactionError {
    TransactionError::Conflict(format!(
        "rename paths are ambiguous: {} and {}",
        source.display(),
        destination.display()
    ))
}

fn sync_parent(path: &Path) -> std::io::Result<()> {
    File::open(path.parent().unwrap_or_else(|| Path::new(".")))?.sync_all()
}
