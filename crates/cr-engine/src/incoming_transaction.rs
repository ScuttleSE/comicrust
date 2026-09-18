//! Durable transactions for Incoming catalog mutations.

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, TryLockError};

use cr_core::durable::{durable_remove, durable_replace};
use cr_core::paths::{incoming_transaction_file, Paths};
use serde::{Deserialize, Serialize};
use sha1::{Digest, Sha1};

static MUTATION_LOCK: Mutex<()> = Mutex::new(());
static DATABASE_EPOCH: AtomicU64 = AtomicU64::new(0);
static ACTIVE_OPERATIONS: AtomicU64 = AtomicU64::new(0);

pub fn database_epoch() -> u64 {
    DATABASE_EPOCH.load(Ordering::Acquire)
}

#[track_caller]
pub fn advance_database_epoch() {
    let caller = std::panic::Location::caller();
    let previous = DATABASE_EPOCH.fetch_add(1, Ordering::AcqRel);
    crate::trace::trace(format!(
        "incoming epoch advanced {} -> {} caller={}:{}",
        previous,
        previous.wrapping_add(1),
        caller.file(),
        caller.line()
    ));
}

/// Advances the epoch and returns the value that identifies the new state.
#[track_caller]
pub fn commit_database_epoch() -> u64 {
    let caller = std::panic::Location::caller();
    let previous = DATABASE_EPOCH
        .fetch_add(1, Ordering::AcqRel)
        .wrapping_add(1);
    crate::trace::trace(format!(
        "incoming epoch committed {} -> {previous} caller={}:{}",
        previous.wrapping_sub(1),
        caller.file(),
        caller.line()
    ));
    previous
}

#[track_caller]
pub fn begin_operation() -> bool {
    let caller = std::panic::Location::caller();
    let started = ACTIVE_OPERATIONS
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .is_ok();
    crate::trace::trace(format!(
        "incoming operation begin started={started} active={} caller={}:{}",
        ACTIVE_OPERATIONS.load(Ordering::Acquire),
        caller.file(),
        caller.line()
    ));
    started
}

pub fn end_operation() {
    let previous = ACTIVE_OPERATIONS.fetch_sub(1, Ordering::AcqRel);
    debug_assert!(previous > 0);
    crate::trace::trace(format!(
        "incoming operation end active={}",
        previous.saturating_sub(1)
    ));
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
#[track_caller]
pub fn acquire_mutation_guard() -> MutationGuard {
    let caller = std::panic::Location::caller();
    let started = std::time::Instant::now();
    let guard = MutationGuard(
        MUTATION_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    crate::trace::trace(format!(
        "incoming mutation guard acquired wait_ms={} epoch={} caller={}:{}",
        started.elapsed().as_millis(),
        database_epoch(),
        caller.file(),
        caller.line()
    ));
    guard
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

/// The on-disk form of one snapshot: metadata plus content-addressed
/// sidecar references. The large `before`/`after` byte arrays live in
/// sidecar files, not inside the journal, so a stage transition that
/// does not change the content rewrites only a small journal.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct SnapshotJournal {
    path: PathBuf,
    before: Option<String>,
    after: String,
    #[serde(default)]
    remove_after: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct FilesJournal {
    incoming_catalog: Option<SnapshotJournal>,
    comic_database: Option<SnapshotJournal>,
    config: Option<SnapshotJournal>,
    #[serde(default)]
    auxiliary: Vec<SnapshotJournal>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
enum IncomingJournalFormat {
    SidecarSnapshotsV1,
}

/// The compact on-disk journal for an `IncomingTransaction`.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct IncomingJournal {
    journal_format: IncomingJournalFormat,
    kind: TransactionKind,
    stage: TransactionStage,
    files: FilesJournal,
    #[serde(default)]
    external_actions: Vec<ExternalFileAction>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplacementKind {
    CrossFilesystemReplacement,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplacementStage {
    Prepared,
    Staged,
    TrashPending,
    OldLibraryTrashed,
    Installed,
    DatabaseSaved,
    IncomingSaved,
    SourceRemoved,
    Committed,
}

/// Durable state for a copy, same-filesystem install, and source removal.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReplacementTransaction {
    pub kind: ReplacementKind,
    pub stage: ReplacementStage,
    pub source: PathBuf,
    pub staging: PathBuf,
    pub destination: PathBuf,
    pub old_library: PathBuf,
    pub expected_len: u64,
    pub sha1: String,
    pub comic_database: FileSnapshot,
    pub incoming_catalog: FileSnapshot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum ReplacementJournalFormat {
    SidecarAfterImagesV1,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ReplacementJournalSnapshot {
    path: PathBuf,
    after_image: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ReplacementJournal {
    journal_format: ReplacementJournalFormat,
    kind: ReplacementKind,
    stage: ReplacementStage,
    source: PathBuf,
    staging: PathBuf,
    destination: PathBuf,
    old_library: PathBuf,
    expected_len: u64,
    sha1: String,
    comic_database: ReplacementJournalSnapshot,
    incoming_catalog: ReplacementJournalSnapshot,
}

impl ReplacementTransaction {
    /// Reads the source identity before a journal permits file mutation.
    pub fn prepare(
        source: PathBuf,
        staging: PathBuf,
        destination: PathBuf,
        old_library: PathBuf,
        comic_database: FileSnapshot,
        incoming_catalog: FileSnapshot,
    ) -> Result<Self, TransactionError> {
        let (expected_len, sha1) = file_identity(&source)?;
        let transaction = Self {
            kind: ReplacementKind::CrossFilesystemReplacement,
            stage: ReplacementStage::Prepared,
            source,
            staging,
            destination,
            old_library,
            expected_len,
            sha1,
            comic_database,
            incoming_catalog,
        };
        validate_replacement(&transaction)?;
        reject_replacement_collisions(&transaction)?;
        Ok(transaction)
    }
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

struct ReplacementSidecars {
    comic_database: PathBuf,
    incoming_catalog: PathBuf,
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

    /// The directory that holds the journal and its content sidecars.
    fn journal_dir(&self) -> &Path {
        self.journal_path.parent().unwrap_or_else(|| Path::new("."))
    }

    fn journal_prefix(&self) -> String {
        self.journal_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("current.json")
            .to_string()
    }

    /// The content-addressed sidecar path for one blob.
    fn snapshot_sidecar(&self, digest: &str) -> PathBuf {
        self.journal_dir()
            .join(format!("{}.snap.{digest}", self.journal_prefix()))
    }

    /// Writes a blob to its content-addressed sidecar and returns the digest.
    /// A sidecar that already holds this content is not rewritten.
    fn store_blob(&self, bytes: &[u8]) -> Result<String, TransactionError> {
        let mut digest = Sha1::new();
        digest.update(bytes);
        let digest = format!("{:x}", digest.finalize());
        let path = self.snapshot_sidecar(&digest);
        if !path.exists() {
            durable_replace(&path, bytes)?;
        }
        Ok(digest)
    }

    fn load_blob(&self, digest: &str) -> Result<Vec<u8>, TransactionError> {
        Ok(std::fs::read(self.snapshot_sidecar(digest))?)
    }

    fn store_snapshot(&self, snapshot: &FileSnapshot) -> Result<SnapshotJournal, TransactionError> {
        let before = match &snapshot.before {
            Some(bytes) => Some(self.store_blob(bytes)?),
            None => None,
        };
        Ok(SnapshotJournal {
            path: snapshot.path.clone(),
            before,
            after: self.store_blob(&snapshot.after)?,
            remove_after: snapshot.remove_after,
        })
    }

    fn load_snapshot(&self, journal: &SnapshotJournal) -> Result<FileSnapshot, TransactionError> {
        let before = match &journal.before {
            Some(digest) => Some(self.load_blob(digest)?),
            None => None,
        };
        Ok(FileSnapshot {
            path: journal.path.clone(),
            before,
            after: self.load_blob(&journal.after)?,
            remove_after: journal.remove_after,
        })
    }

    fn store_files(&self, files: &TransactionFiles) -> Result<FilesJournal, TransactionError> {
        Ok(FilesJournal {
            incoming_catalog: files
                .incoming_catalog
                .as_ref()
                .map(|snapshot| self.store_snapshot(snapshot))
                .transpose()?,
            comic_database: files
                .comic_database
                .as_ref()
                .map(|snapshot| self.store_snapshot(snapshot))
                .transpose()?,
            config: files
                .config
                .as_ref()
                .map(|snapshot| self.store_snapshot(snapshot))
                .transpose()?,
            auxiliary: files
                .auxiliary
                .iter()
                .map(|snapshot| self.store_snapshot(snapshot))
                .collect::<Result<Vec<_>, _>>()?,
        })
    }

    fn load_files(&self, journal: &FilesJournal) -> Result<TransactionFiles, TransactionError> {
        Ok(TransactionFiles {
            incoming_catalog: journal
                .incoming_catalog
                .as_ref()
                .map(|snapshot| self.load_snapshot(snapshot))
                .transpose()?,
            comic_database: journal
                .comic_database
                .as_ref()
                .map(|snapshot| self.load_snapshot(snapshot))
                .transpose()?,
            config: journal
                .config
                .as_ref()
                .map(|snapshot| self.load_snapshot(snapshot))
                .transpose()?,
            auxiliary: journal
                .auxiliary
                .iter()
                .map(|snapshot| self.load_snapshot(snapshot))
                .collect::<Result<Vec<_>, _>>()?,
        })
    }

    /// The sidecar digests the current journal references, if any.
    fn referenced_snapshot_digests(&self) -> Vec<String> {
        let Ok(bytes) = std::fs::read(&self.journal_path) else {
            return Vec::new();
        };
        let Ok(journal) = serde_json::from_slice::<IncomingJournal>(&bytes) else {
            return Vec::new();
        };
        let mut digests = Vec::new();
        let mut collect = |snapshot: &SnapshotJournal| {
            if let Some(before) = &snapshot.before {
                digests.push(before.clone());
            }
            digests.push(snapshot.after.clone());
        };
        if let Some(snapshot) = &journal.files.incoming_catalog {
            collect(snapshot);
        }
        if let Some(snapshot) = &journal.files.comic_database {
            collect(snapshot);
        }
        if let Some(snapshot) = &journal.files.config {
            collect(snapshot);
        }
        for snapshot in &journal.files.auxiliary {
            collect(snapshot);
        }
        digests
    }

    /// Removes the journal and every content sidecar it referenced.
    fn remove_journal_and_sidecars(&self) -> Result<(), TransactionError> {
        let digests = self.referenced_snapshot_digests();
        durable_remove(&self.journal_path)?;
        for digest in digests {
            durable_remove(&self.snapshot_sidecar(&digest))?;
        }
        Ok(())
    }

    /// Reads the current journal back into an in-memory transaction.
    fn read_transaction(&self, bytes: &[u8]) -> Result<IncomingTransaction, TransactionError> {
        if let Ok(journal) = serde_json::from_slice::<IncomingJournal>(bytes) {
            return Ok(IncomingTransaction {
                kind: journal.kind,
                stage: journal.stage,
                files: self.load_files(&journal.files)?,
                external_actions: journal.external_actions.clone(),
            });
        }
        Ok(serde_json::from_slice::<IncomingTransaction>(bytes)?)
    }

    /// Creates the durable journal before the caller performs external work.
    pub fn begin(&self, transaction: &IncomingTransaction) -> Result<(), TransactionError> {
        validate(transaction)?;
        if self.journal_path.exists() {
            return Err(TransactionError::Active);
        }
        self.write_journal(transaction)
    }

    /// Creates a durable journal before the replacement changes any file.
    pub fn begin_replacement(
        &self,
        transaction: &ReplacementTransaction,
    ) -> Result<(), TransactionError> {
        validate_replacement(transaction)?;
        if self.journal_path.exists() {
            return Err(TransactionError::Active);
        }
        if transaction.stage == ReplacementStage::Prepared {
            reject_replacement_collisions(transaction)?;
        }
        self.cleanup_replacement_sidecars()?;
        self.write_replacement_after_images(transaction)?;
        if let Err(error) = self.write_replacement_journal(transaction) {
            let _ = self.cleanup_replacement_sidecars();
            return Err(error);
        }
        Ok(())
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
        let transaction: IncomingTransaction = self.read_transaction(&bytes)?;
        if transaction.stage != TransactionStage::Prepared
            || !transaction.external_actions.is_empty()
        {
            return Err(TransactionError::Invalid(
                "only an untouched Prepared journal can be aborted".into(),
            ));
        }
        self.remove_journal_and_sidecars()?;
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
        let started = std::time::Instant::now();
        crate::trace::trace(format!(
            "incoming transaction commit start kind={:?} stage={:?} actions={}",
            transaction.kind,
            transaction.stage,
            transaction.external_actions.len()
        ));
        self.write_journal(transaction)?;
        crate::trace::trace(format!(
            "incoming transaction commit journal written elapsed_ms={}",
            started.elapsed().as_millis()
        ));
        self.apply_external_actions(transaction)?;
        crate::trace::trace(format!(
            "incoming transaction external actions complete elapsed_ms={}",
            started.elapsed().as_millis()
        ));
        let result = self.install_after_images(transaction);
        crate::trace::trace(format!(
            "incoming transaction commit finish success={} elapsed_ms={}",
            result.is_ok(),
            started.elapsed().as_millis()
        ));
        result
    }

    /// Copies, validates, installs, and removes the source in durable steps.
    pub fn commit_replacement(
        &self,
        transaction: &mut ReplacementTransaction,
    ) -> Result<(), TransactionError> {
        if !self.journal_path.exists() {
            return Err(TransactionError::Invalid(
                "the current journal does not exist".into(),
            ));
        }
        validate_replacement(transaction)?;
        self.write_replacement_journal(transaction)?;
        self.roll_replacement_forward(transaction, &copy_to_staging, &trash_to_desktop)
    }

    /// Rolls the current journal forward. Conflicts leave the journal unchanged.
    pub fn recover(&self) -> Result<RecoveryResult, TransactionError> {
        let bytes = match std::fs::read(&self.journal_path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                self.cleanup_replacement_sidecars()?;
                return Ok(RecoveryResult::NoJournal);
            }
            Err(error) => return Err(error.into()),
        };
        if let Some(mut replacement) = self.load_replacement(&bytes)? {
            if replacement.stage == ReplacementStage::Committed {
                self.finish_replacement()?;
                return Ok(RecoveryResult::Recovered);
            }
            self.ensure_compact_replacement_journal(&replacement, &bytes)?;
            self.roll_replacement_forward(&mut replacement, &copy_to_staging, &trash_to_desktop)?;
            return Ok(RecoveryResult::Recovered);
        }
        let mut transaction: IncomingTransaction = self.read_transaction(&bytes)?;
        validate(&transaction)?;
        if transaction.stage == TransactionStage::Committed {
            self.remove_journal_and_sidecars()?;
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
            crate::trace::trace(format!(
                "incoming transaction destination install start bytes={} path='{}'",
                snapshot.after.len(),
                snapshot.path.display()
            ));
            let started = std::time::Instant::now();
            install_snapshot(&snapshot)?;
            crate::trace::trace(format!(
                "incoming transaction destination install finish elapsed_ms={} path='{}'",
                started.elapsed().as_millis(),
                snapshot.path.display()
            ));
        }
        transaction.stage = TransactionStage::DestinationSaved;
        self.write_journal(transaction)?;

        if let Some(snapshot) = source {
            crate::trace::trace(format!(
                "incoming transaction source install start bytes={} path='{}'",
                snapshot.after.len(),
                snapshot.path.display()
            ));
            let started = std::time::Instant::now();
            install_snapshot(&snapshot)?;
            crate::trace::trace(format!(
                "incoming transaction source install finish elapsed_ms={} path='{}'",
                started.elapsed().as_millis(),
                snapshot.path.display()
            ));
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
        self.remove_journal_and_sidecars()?;
        Ok(())
    }

    fn write_journal(&self, transaction: &IncomingTransaction) -> Result<(), TransactionError> {
        let started = std::time::Instant::now();
        let previous_digests = self.referenced_snapshot_digests();
        let files = self.store_files(&transaction.files)?;
        let journal = IncomingJournal {
            journal_format: IncomingJournalFormat::SidecarSnapshotsV1,
            kind: transaction.kind,
            stage: transaction.stage,
            files,
            external_actions: transaction.external_actions.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&journal)?;
        let serialized_ms = started.elapsed().as_millis();
        durable_replace(&self.journal_path, &bytes)?;
        let current: std::collections::HashSet<String> =
            self.referenced_snapshot_digests().into_iter().collect();
        for digest in previous_digests {
            if !current.contains(&digest) {
                durable_remove(&self.snapshot_sidecar(&digest))?;
            }
        }
        crate::trace::trace(format!(
            "incoming journal write kind={:?} stage={:?} bytes={} serialize_ms={serialized_ms} elapsed_ms={}",
            transaction.kind,
            transaction.stage,
            bytes.len(),
            started.elapsed().as_millis()
        ));
        Ok(())
    }

    fn write_replacement_journal(
        &self,
        transaction: &ReplacementTransaction,
    ) -> Result<(), TransactionError> {
        let started = std::time::Instant::now();
        let sidecars = self.replacement_sidecars();
        if transaction.stage != ReplacementStage::Committed
            && (!sidecars.comic_database.exists() || !sidecars.incoming_catalog.exists())
        {
            self.write_replacement_after_images(transaction)?;
        }
        let journal = ReplacementJournal {
            journal_format: ReplacementJournalFormat::SidecarAfterImagesV1,
            kind: transaction.kind,
            stage: transaction.stage,
            source: transaction.source.clone(),
            staging: transaction.staging.clone(),
            destination: transaction.destination.clone(),
            old_library: transaction.old_library.clone(),
            expected_len: transaction.expected_len,
            sha1: transaction.sha1.clone(),
            comic_database: ReplacementJournalSnapshot {
                path: transaction.comic_database.path.clone(),
                after_image: sidecars.comic_database,
            },
            incoming_catalog: ReplacementJournalSnapshot {
                path: transaction.incoming_catalog.path.clone(),
                after_image: sidecars.incoming_catalog,
            },
        };
        let bytes = serde_json::to_vec_pretty(&journal)?;
        let serialized_ms = started.elapsed().as_millis();
        durable_replace(&self.journal_path, &bytes)?;
        crate::trace::trace(format!(
            "replacement journal write stage={:?} bytes={} serialize_ms={serialized_ms} elapsed_ms={}",
            transaction.stage,
            bytes.len(),
            started.elapsed().as_millis()
        ));
        Ok(())
    }

    fn replacement_sidecars(&self) -> ReplacementSidecars {
        let file_name = self
            .journal_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("current.json");
        ReplacementSidecars {
            comic_database: self
                .journal_path
                .with_file_name(format!("{file_name}.comic-database.after")),
            incoming_catalog: self
                .journal_path
                .with_file_name(format!("{file_name}.incoming-catalog.after")),
        }
    }

    fn write_replacement_after_images(
        &self,
        transaction: &ReplacementTransaction,
    ) -> Result<(), TransactionError> {
        let sidecars = self.replacement_sidecars();
        durable_replace(&sidecars.comic_database, &transaction.comic_database.after)?;
        durable_replace(
            &sidecars.incoming_catalog,
            &transaction.incoming_catalog.after,
        )?;
        Ok(())
    }

    fn cleanup_replacement_sidecars(&self) -> Result<(), TransactionError> {
        let sidecars = self.replacement_sidecars();
        durable_remove(&sidecars.comic_database)?;
        durable_remove(&sidecars.incoming_catalog)?;
        Ok(())
    }

    fn finish_replacement(&self) -> Result<(), TransactionError> {
        self.cleanup_replacement_sidecars()?;
        durable_remove(&self.journal_path)?;
        Ok(())
    }

    fn load_replacement(
        &self,
        bytes: &[u8],
    ) -> Result<Option<ReplacementTransaction>, TransactionError> {
        if let Ok(journal) = serde_json::from_slice::<ReplacementJournal>(bytes) {
            let sidecars = self.replacement_sidecars();
            if journal.comic_database.after_image != sidecars.comic_database
                || journal.incoming_catalog.after_image != sidecars.incoming_catalog
            {
                return Err(TransactionError::Invalid(
                    "replacement journal references unexpected sidecars".into(),
                ));
            }
            let committed = journal.stage == ReplacementStage::Committed;
            let comic_after = if committed {
                Vec::new()
            } else {
                std::fs::read(&sidecars.comic_database)?
            };
            let incoming_after = if committed {
                Vec::new()
            } else {
                std::fs::read(&sidecars.incoming_catalog)?
            };
            return Ok(Some(ReplacementTransaction {
                kind: journal.kind,
                stage: journal.stage,
                source: journal.source,
                staging: journal.staging,
                destination: journal.destination,
                old_library: journal.old_library,
                expected_len: journal.expected_len,
                sha1: journal.sha1,
                comic_database: FileSnapshot {
                    path: journal.comic_database.path,
                    before: None,
                    after: comic_after,
                    remove_after: false,
                },
                incoming_catalog: FileSnapshot {
                    path: journal.incoming_catalog.path,
                    before: None,
                    after: incoming_after,
                    remove_after: false,
                },
            }));
        }
        match serde_json::from_slice::<ReplacementTransaction>(bytes) {
            Ok(transaction) => Ok(Some(transaction)),
            Err(_) => Ok(None),
        }
    }

    fn ensure_compact_replacement_journal(
        &self,
        transaction: &ReplacementTransaction,
        bytes: &[u8],
    ) -> Result<(), TransactionError> {
        if serde_json::from_slice::<ReplacementJournal>(bytes).is_ok() {
            return Ok(());
        }
        self.write_replacement_after_images(transaction)?;
        self.write_replacement_journal(transaction)
    }

    fn roll_replacement_forward(
        &self,
        transaction: &mut ReplacementTransaction,
        copy: &dyn Fn(&ReplacementTransaction) -> Result<(), TransactionError>,
        trash: &dyn Fn(&Path) -> std::io::Result<()>,
    ) -> Result<(), TransactionError> {
        let started = std::time::Instant::now();
        crate::trace::trace(format!(
            "replacement roll-forward start stage={:?} bytes={} source='{}' destination='{}'",
            transaction.stage,
            transaction.expected_len,
            transaction.source.display(),
            transaction.destination.display()
        ));
        if transaction.stage == ReplacementStage::Committed {
            self.finish_replacement()?;
            return Ok(());
        }

        if transaction.stage == ReplacementStage::Prepared {
            if transaction.destination != transaction.old_library
                && transaction.destination.exists()
            {
                return Err(replacement_collision(transaction));
            }
            if transaction.staging.exists() {
                if validate_file(
                    &transaction.staging,
                    transaction.expected_len,
                    &transaction.sha1,
                )
                .is_err()
                {
                    durable_remove(&transaction.staging)?;
                    copy(transaction)?;
                }
            } else {
                copy(transaction)?;
            }
            transaction.stage = ReplacementStage::Staged;
            self.write_replacement_journal(transaction)?;
            trace_replacement_stage(transaction.stage, started);
        }

        if transaction.stage == ReplacementStage::Staged {
            validate_file(
                &transaction.staging,
                transaction.expected_len,
                &transaction.sha1,
            )?;
            if transaction.destination != transaction.old_library
                && transaction.destination.exists()
            {
                return Err(replacement_collision(transaction));
            }
            transaction.stage = ReplacementStage::TrashPending;
            self.write_replacement_journal(transaction)?;
            trace_replacement_stage(transaction.stage, started);
        }

        if transaction.stage == ReplacementStage::TrashPending {
            if transaction.old_library.exists() {
                trash(&transaction.old_library)?;
            }
            if transaction.old_library.exists() {
                return Err(TransactionError::Conflict(format!(
                    "old Library file still exists after trash: {}",
                    transaction.old_library.display()
                )));
            }
            transaction.stage = ReplacementStage::OldLibraryTrashed;
            self.write_replacement_journal(transaction)?;
            trace_replacement_stage(transaction.stage, started);
        }

        if transaction.stage == ReplacementStage::OldLibraryTrashed {
            if transaction.old_library.exists() {
                return Err(TransactionError::Conflict(format!(
                    "old Library file exists after its trash stage: {}",
                    transaction.old_library.display()
                )));
            }
            match (
                transaction.staging.exists(),
                transaction.destination.exists(),
            ) {
                (true, false) => {
                    validate_file(
                        &transaction.staging,
                        transaction.expected_len,
                        &transaction.sha1,
                    )?;
                    let install_started = std::time::Instant::now();
                    if let Err(error) =
                        std::fs::hard_link(&transaction.staging, &transaction.destination)
                    {
                        if error.kind() == std::io::ErrorKind::AlreadyExists {
                            return Err(replacement_collision(transaction));
                        }
                        return Err(error.into());
                    }
                    sync_parent(&transaction.destination)?;
                    durable_remove(&transaction.staging)?;
                    crate::trace::trace(format!(
                        "replacement file install elapsed_ms={} destination='{}'",
                        install_started.elapsed().as_millis(),
                        transaction.destination.display()
                    ));
                }
                (false, true) => validate_file(
                    &transaction.destination,
                    transaction.expected_len,
                    &transaction.sha1,
                )?,
                (true, true) => {
                    if !same_file(&transaction.staging, &transaction.destination)? {
                        return Err(replacement_collision(transaction));
                    }
                    validate_file(
                        &transaction.staging,
                        transaction.expected_len,
                        &transaction.sha1,
                    )?;
                    validate_file(
                        &transaction.destination,
                        transaction.expected_len,
                        &transaction.sha1,
                    )?;
                    durable_remove(&transaction.staging)?;
                }
                _ => return Err(replacement_collision(transaction)),
            }
            transaction.stage = ReplacementStage::Installed;
            self.write_replacement_journal(transaction)?;
            trace_replacement_stage(transaction.stage, started);
        }

        if transaction.stage == ReplacementStage::Installed {
            reject_reappeared_old_library(transaction)?;
            validate_file(
                &transaction.destination,
                transaction.expected_len,
                &transaction.sha1,
            )?;
            trace_install_snapshot("ComicDb", &transaction.comic_database)?;
            transaction.stage = ReplacementStage::DatabaseSaved;
            self.write_replacement_journal(transaction)?;
            trace_replacement_stage(transaction.stage, started);
        }

        if transaction.stage == ReplacementStage::DatabaseSaved {
            reject_reappeared_old_library(transaction)?;
            validate_file(
                &transaction.destination,
                transaction.expected_len,
                &transaction.sha1,
            )?;
            trace_install_snapshot("IncomingDb", &transaction.incoming_catalog)?;
            transaction.stage = ReplacementStage::IncomingSaved;
            self.write_replacement_journal(transaction)?;
            trace_replacement_stage(transaction.stage, started);
        }

        if transaction.stage == ReplacementStage::IncomingSaved {
            reject_reappeared_old_library(transaction)?;
            validate_file(
                &transaction.destination,
                transaction.expected_len,
                &transaction.sha1,
            )?;
            if transaction.source.exists() {
                validate_file(
                    &transaction.source,
                    transaction.expected_len,
                    &transaction.sha1,
                )?;
                let remove_started = std::time::Instant::now();
                durable_remove(&transaction.source)?;
                crate::trace::trace(format!(
                    "replacement source remove elapsed_ms={} path='{}'",
                    remove_started.elapsed().as_millis(),
                    transaction.source.display()
                ));
            }
            transaction.stage = ReplacementStage::SourceRemoved;
            self.write_replacement_journal(transaction)?;
            trace_replacement_stage(transaction.stage, started);
        }

        if transaction.stage == ReplacementStage::SourceRemoved {
            reject_reappeared_old_library(transaction)?;
            if transaction.source.exists() {
                return Err(TransactionError::Conflict(format!(
                    "replacement source still exists: {}",
                    transaction.source.display()
                )));
            }
            validate_file(
                &transaction.destination,
                transaction.expected_len,
                &transaction.sha1,
            )?;
            transaction.stage = ReplacementStage::Committed;
            self.write_replacement_journal(transaction)?;
            trace_replacement_stage(transaction.stage, started);
        }

        self.finish_replacement()?;
        crate::trace::trace(format!(
            "replacement roll-forward finish elapsed_ms={}",
            started.elapsed().as_millis()
        ));
        Ok(())
    }

    #[doc(hidden)]
    pub fn commit_replacement_with_trash(
        &self,
        transaction: &mut ReplacementTransaction,
        trash: &dyn Fn(&Path) -> std::io::Result<()>,
    ) -> Result<(), TransactionError> {
        if !self.journal_path.exists() {
            return Err(TransactionError::Invalid(
                "the current journal does not exist".into(),
            ));
        }
        validate_replacement(transaction)?;
        self.write_replacement_journal(transaction)?;
        self.roll_replacement_forward(transaction, &copy_to_staging, trash)
    }

    #[doc(hidden)]
    pub fn commit_replacement_with_actions(
        &self,
        transaction: &mut ReplacementTransaction,
        copy: &dyn Fn(&ReplacementTransaction) -> Result<(), TransactionError>,
        trash: &dyn Fn(&Path) -> std::io::Result<()>,
    ) -> Result<(), TransactionError> {
        if !self.journal_path.exists() {
            return Err(TransactionError::Invalid(
                "the current journal does not exist".into(),
            ));
        }
        validate_replacement(transaction)?;
        self.write_replacement_journal(transaction)?;
        self.roll_replacement_forward(transaction, copy, trash)
    }

    #[doc(hidden)]
    pub fn recover_with_trash(
        &self,
        trash: &dyn Fn(&Path) -> std::io::Result<()>,
    ) -> Result<RecoveryResult, TransactionError> {
        let bytes = match std::fs::read(&self.journal_path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(RecoveryResult::NoJournal)
            }
            Err(error) => return Err(error.into()),
        };
        let mut transaction = self.load_replacement(&bytes)?.ok_or_else(|| {
            TransactionError::Invalid("the journal is not a replacement transaction".into())
        })?;
        if transaction.stage == ReplacementStage::Committed {
            self.finish_replacement()?;
            return Ok(RecoveryResult::Recovered);
        }
        validate_replacement(&transaction)?;
        self.ensure_compact_replacement_journal(&transaction, &bytes)?;
        self.roll_replacement_forward(&mut transaction, &copy_to_staging, trash)?;
        Ok(RecoveryResult::Recovered)
    }
}

fn copy_to_staging(transaction: &ReplacementTransaction) -> Result<(), TransactionError> {
    let started = std::time::Instant::now();
    if (transaction.destination != transaction.old_library && transaction.destination.exists())
        || transaction.staging.exists()
    {
        return Err(replacement_collision(transaction));
    }
    validate_file(
        &transaction.source,
        transaction.expected_len,
        &transaction.sha1,
    )?;
    let parent = transaction.staging.parent().ok_or_else(|| {
        TransactionError::Invalid("replacement staging path has no parent".into())
    })?;
    std::fs::create_dir_all(parent)?;
    let mut source = File::open(&transaction.source)?;
    let mut staging = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&transaction.staging)?;
    let result = (|| {
        let copy_started = std::time::Instant::now();
        std::io::copy(&mut source, &mut staging)?;
        crate::trace::trace(format!(
            "replacement staging copy bytes={} elapsed_ms={}",
            transaction.expected_len,
            copy_started.elapsed().as_millis()
        ));
        let sync_started = std::time::Instant::now();
        staging.flush()?;
        staging.sync_all()?;
        crate::trace::trace(format!(
            "replacement staging sync elapsed_ms={}",
            sync_started.elapsed().as_millis()
        ));
        drop(staging);
        validate_file(
            &transaction.staging,
            transaction.expected_len,
            &transaction.sha1,
        )?;
        File::open(parent)?.sync_all()?;
        crate::trace::trace(format!(
            "replacement copy-to-staging finish elapsed_ms={}",
            started.elapsed().as_millis()
        ));
        Ok::<(), TransactionError>(())
    })();
    if result.is_err() {
        let _ = durable_remove(&transaction.staging);
    }
    result
}

fn trace_replacement_stage(stage: ReplacementStage, started: std::time::Instant) {
    crate::trace::trace(format!(
        "replacement stage={stage:?} elapsed_ms={}",
        started.elapsed().as_millis()
    ));
}

fn validate_replacement(transaction: &ReplacementTransaction) -> Result<(), TransactionError> {
    if transaction.source == transaction.staging
        || transaction.source == transaction.destination
        || transaction.staging == transaction.destination
        || transaction.source == transaction.old_library
    {
        return Err(TransactionError::Invalid(
            "replacement paths must be distinct".into(),
        ));
    }
    if transaction.staging.parent() != transaction.destination.parent() {
        return Err(TransactionError::Invalid(
            "replacement staging and destination paths must have the same parent".into(),
        ));
    }
    let incoming_extension = transaction
        .source
        .extension()
        .ok_or_else(|| TransactionError::Invalid("replacement source has no extension".into()))?;
    if transaction.destination != transaction.old_library.with_extension(incoming_extension) {
        return Err(TransactionError::Invalid(
            "replacement destination must use the Library basename and Incoming extension".into(),
        ));
    }
    if transaction.comic_database.remove_after || transaction.incoming_catalog.remove_after {
        return Err(TransactionError::Invalid(
            "replacement catalog snapshots cannot remove files".into(),
        ));
    }
    if transaction.sha1.len() != 40
        || !transaction
            .sha1
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(TransactionError::Invalid(
            "replacement SHA-1 is invalid".into(),
        ));
    }
    Ok(())
}

fn reject_replacement_collisions(
    transaction: &ReplacementTransaction,
) -> Result<(), TransactionError> {
    if transaction.staging.exists()
        || (transaction.destination != transaction.old_library && transaction.destination.exists())
        || !transaction.old_library.exists()
    {
        Err(replacement_collision(transaction))
    } else {
        Ok(())
    }
}

fn trash_to_desktop(path: &Path) -> std::io::Result<()> {
    let started = std::time::Instant::now();
    let status = Command::new("gio").arg("trash").arg(path).status()?;
    crate::trace::trace(format!(
        "replacement trash status={status} elapsed_ms={} path='{}'",
        started.elapsed().as_millis(),
        path.display()
    ));
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "gio trash failed for {} with status {status}",
            path.display()
        )))
    }
}

fn replacement_collision(transaction: &ReplacementTransaction) -> TransactionError {
    TransactionError::Conflict(format!(
        "replacement staging or destination exists: {} and {}",
        transaction.staging.display(),
        transaction.destination.display()
    ))
}

fn reject_reappeared_old_library(
    transaction: &ReplacementTransaction,
) -> Result<(), TransactionError> {
    if transaction.old_library != transaction.destination && transaction.old_library.exists() {
        return Err(TransactionError::Conflict(format!(
            "old Library file exists after its trash stage: {}",
            transaction.old_library.display()
        )));
    }
    Ok(())
}

fn validate_file(
    path: &Path,
    expected_len: u64,
    expected_sha1: &str,
) -> Result<(), TransactionError> {
    let started = std::time::Instant::now();
    let (actual_len, actual_sha1) = file_identity(path)?;
    crate::trace::trace(format!(
        "replacement validate bytes={actual_len} elapsed_ms={} path='{}'",
        started.elapsed().as_millis(),
        path.display()
    ));
    if actual_len != expected_len || actual_sha1 != expected_sha1 {
        return Err(TransactionError::Conflict(format!(
            "replacement file validation failed: {}",
            path.display()
        )));
    }
    Ok(())
}

fn file_identity(path: &Path) -> Result<(u64, String), TransactionError> {
    let mut file = File::open(path)?;
    let mut digest = Sha1::new();
    let mut length = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        length += count as u64;
        digest.update(&buffer[..count]);
    }
    Ok((length, format!("{:x}", digest.finalize())))
}

#[cfg(unix)]
fn same_file(left: &Path, right: &Path) -> std::io::Result<bool> {
    let left = left.metadata()?;
    let right = right.metadata()?;
    Ok(left.dev() == right.dev() && left.ino() == right.ino())
}

#[cfg(not(unix))]
fn same_file(_left: &Path, _right: &Path) -> std::io::Result<bool> {
    Ok(false)
}

fn install_snapshot(snapshot: &FileSnapshot) -> Result<(), TransactionError> {
    if snapshot.remove_after {
        durable_remove(&snapshot.path)?;
    } else {
        durable_replace(&snapshot.path, &snapshot.after)?;
    }
    Ok(())
}

fn trace_install_snapshot(label: &str, snapshot: &FileSnapshot) -> Result<(), TransactionError> {
    let started = std::time::Instant::now();
    install_snapshot(snapshot)?;
    crate::trace::trace(format!(
        "replacement catalog install label={label} bytes={} elapsed_ms={} path='{}'",
        snapshot.after.len(),
        started.elapsed().as_millis(),
        snapshot.path.display()
    ));
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
