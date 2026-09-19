//! The book mover (the port of `BookMover` in lobookmover.py).
//!
//! The run has two phases: the plan (`create_book_paths`) resolves
//! every book's destination under every profile — non-copy profiles
//! claim a book (later profiles are then skipped for it) while copy
//! profiles always produce a copy — and the process phase performs
//! the moves/copies with progress, cancel, duplicate resolution and
//! the undo log.

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::CrGuid;

use crate::engine::{
    Apply, CoverSource, DuplicateAction, DuplicateAsk, DuplicateBookInfo, FilesystemEffects,
    LogEntry, OrganizeUi,
};
use crate::fields;
use crate::profile::{Profile, MODE_COPY, MODE_MOVE};
use crate::rules;
use crate::series::SeriesIndex;
use crate::template::{file_directory, path_combine, path_extension, MultiValueState, TokenCtx};

/// The undo record collection (`UndoCollection` in locommon.py): the
/// original path, the current path, and the profile per moved book.
/// Books moved more than once keep only the ORIGINAL undo path.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UndoCollection {
    pub undo_paths: Vec<String>,
    pub current_paths: Vec<String>,
    pub profile_names: Vec<String>,
}

/// One correlated entry in an [`UndoCollection`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UndoEntry<'a> {
    pub undo_path: &'a str,
    pub current_path: &'a str,
    pub profile_name: &'a str,
}

/// Adoption state stored beside `undo.dat`. The undo file itself stays unchanged.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AdoptionManifest {
    pub entries: Vec<AdoptionManifestEntry>,
}

/// One adopted ID and the exact undo entry that owns it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AdoptionManifestEntry {
    pub id: String,
    pub profile_name: String,
    pub current_path: String,
    pub undo_path: String,
}

/// Returns the companion path without changing the `undo.dat` path.
pub fn adoption_manifest_path(undo_path: &Path) -> std::path::PathBuf {
    let mut name = undo_path.as_os_str().to_os_string();
    name.push(".adoption.json");
    name.into()
}

impl AdoptionManifest {
    pub fn from_adoptions(undo: &UndoCollection, applies: &[Apply]) -> Self {
        let ids: HashMap<&str, String> = applies
            .iter()
            .filter_map(|apply| match apply {
                Apply::Adopt(book) => Some((book.file_path.as_str(), book.id.to_d_string())),
                _ => None,
            })
            .collect();
        Self {
            entries: undo
                .entries()
                .filter_map(|entry| {
                    ids.get(entry.current_path).map(|id| AdoptionManifestEntry {
                        id: id.clone(),
                        profile_name: entry.profile_name.to_string(),
                        current_path: entry.current_path.to_string(),
                        undo_path: entry.undo_path.to_string(),
                    })
                })
                .collect(),
        }
    }

    pub fn load(path: &Path) -> std::io::Result<Self> {
        let bytes = std::fs::read(path)?;
        serde_json::from_slice(&bytes)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let bytes = self.to_bytes()?;
        atomic_write(path, &bytes)
    }

    pub fn to_bytes(&self) -> std::io::Result<Vec<u8>> {
        serde_json::to_vec_pretty(self)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
    }

    pub fn matches(&self, undo: &UndoCollection) -> bool {
        if self.entries.len() > undo.len() {
            return false;
        }
        let mut ids = std::collections::HashSet::new();
        let mut identities = std::collections::HashSet::new();
        for entry in &self.entries {
            if CrGuid::parse(&entry.id).is_err()
                || !ids.insert(entry.id.as_str())
                || !identities.insert((
                    entry.profile_name.as_str(),
                    entry.current_path.as_str(),
                    entry.undo_path.as_str(),
                ))
            {
                return false;
            }
        }
        for manifest in &self.entries {
            if !undo.entries().any(|entry| {
                manifest.profile_name == entry.profile_name
                    && manifest.current_path == entry.current_path
                    && manifest.undo_path == entry.undo_path
            }) {
                return false;
            }
        }
        true
    }

    pub fn entry_for_current(&self, current_path: &str) -> Option<&AdoptionManifestEntry> {
        self.entries
            .iter()
            .find(|entry| entry.current_path == current_path)
    }

    pub fn retain_undo(&mut self, undo: &UndoCollection) {
        self.entries.retain(|manifest| {
            undo.entries().any(|entry| {
                manifest.profile_name == entry.profile_name
                    && manifest.current_path == entry.current_path
                    && manifest.undo_path == entry.undo_path
            })
        });
    }
}

impl UndoCollection {
    pub fn len(&self) -> usize {
        self.current_paths.len()
    }

    pub fn is_empty(&self) -> bool {
        self.current_paths.is_empty()
    }

    /// Returns all entries with their three fields kept together.
    pub fn entries(&self) -> impl ExactSizeIterator<Item = UndoEntry<'_>> {
        self.undo_paths
            .iter()
            .zip(&self.current_paths)
            .zip(&self.profile_names)
            .map(|((undo_path, current_path), profile_name)| UndoEntry {
                undo_path,
                current_path,
                profile_name,
            })
    }

    /// Returns the entry for its current path.
    pub fn entry(&self, current: &str) -> Option<UndoEntry<'_>> {
        self.entries().find(|entry| entry.current_path == current)
    }

    pub fn append(&mut self, undo_path: &str, new_path: &str, profile_name: &str) {
        if let Some(i) = self.current_paths.iter().position(|p| p == undo_path) {
            // Moved more than once: the ORIGINAL undo path stays.
            self.current_paths[i] = new_path.to_string();
        } else {
            self.undo_paths.push(undo_path.to_string());
            self.current_paths.push(new_path.to_string());
            self.profile_names.push(profile_name.to_string());
        }
    }

    fn remove_current(&mut self, current: &str) {
        if let Some(index) = self.current_paths.iter().position(|path| path == current) {
            self.undo_paths.remove(index);
            self.current_paths.remove(index);
            self.profile_names.remove(index);
        }
    }

    /// `undo_path(path)`.
    pub fn undo_path(&self, current: &str) -> Option<&str> {
        self.entry(current).map(|entry| entry.undo_path)
    }

    pub fn profile(&self, current: &str) -> Option<&str> {
        self.entry(current).map(|entry| entry.profile_name)
    }

    /// The `profile|current|undo` line file (undo.dat).
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        self.save_ordered(path, false)
    }

    /// Saves a collection returned by `load` so another load keeps its order.
    pub fn save_for_retry(&self, path: &Path) -> std::io::Result<()> {
        self.save_ordered(path, true)
    }

    pub fn to_bytes(&self, reverse: bool) -> std::io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        self.write_ordered(&mut bytes, reverse)?;
        Ok(bytes)
    }

    fn save_ordered(&self, path: &Path, reverse: bool) -> std::io::Result<()> {
        let mut bytes = Vec::new();
        self.write_ordered(&mut bytes, reverse)?;
        atomic_write(path, &bytes)
    }

    fn write_ordered(&self, bytes: &mut Vec<u8>, reverse: bool) -> std::io::Result<()> {
        let indexes: Box<dyn Iterator<Item = usize>> = if reverse {
            Box::new((0..self.current_paths.len()).rev())
        } else {
            Box::new(0..self.current_paths.len())
        };
        for i in indexes {
            writeln!(
                bytes,
                "{}|{}|{}",
                self.profile_names[i], self.current_paths[i], self.undo_paths[i]
            )?;
        }
        Ok(())
    }

    /// Loads the line file, reversed so multi-move runs unwind in
    /// order.
    pub fn load(path: &Path) -> UndoCollection {
        let mut out = UndoCollection::default();
        let Ok(text) = std::fs::read_to_string(path) else {
            return out;
        };
        for line in text.lines() {
            let line = line.trim_end();
            if line.is_empty() {
                continue;
            }
            let parts: Vec<&str> = line.splitn(3, '|').collect();
            if parts.len() != 3 {
                continue;
            }
            out.append(parts[2], parts[1], parts[0]);
        }
        out.undo_paths.reverse();
        out.current_paths.reverse();
        out.profile_names.reverse();
        out
    }
}

/// Returns true when a missing adoption manifest makes ordinary undo unsafe.
pub fn missing_manifest_is_unsafe(undo: &UndoCollection, incoming_roots: &[String]) -> bool {
    undo.entries().any(|entry| {
        incoming_roots
            .iter()
            .any(|root| path_is_below(entry.undo_path, root))
    })
}

fn path_is_below(path: &str, root: &str) -> bool {
    let components = |value: &str| {
        value
            .split(['/', '\\'])
            .filter(|part| !part.is_empty())
            .map(str::to_ascii_lowercase)
            .collect::<Vec<_>>()
    };
    let path = components(path);
    let root = components(root);
    !root.is_empty() && path.len() > root.len() && path.starts_with(&root)
}

static TEMPORARY_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
    loop {
        let sequence = TEMPORARY_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(
            ".{file_name}.{}.{}.tmp",
            std::process::id(),
            sequence
        ));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        match options.open(&temporary) {
            Ok(mut file) => {
                let result = (|| {
                    file.write_all(bytes)?;
                    file.sync_all()?;
                    drop(file);
                    std::fs::rename(&temporary, path)
                })();
                if result.is_err() {
                    let _ = std::fs::remove_file(&temporary);
                }
                return result;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
}

fn remove_if_present(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// The per-profile counter block (`ProfileReport`).
#[derive(Clone, Debug)]
pub struct ProfileReport {
    pub name: String,
    pub mode: String,
    pub total: usize,
    pub success: usize,
    pub failed: usize,
    pub skipped: usize,
}

impl ProfileReport {
    fn mode_past(mode: &str) -> &'static str {
        match mode {
            MODE_COPY => "copied",
            MODE_MOVE => "moved",
            _ => "moved (simulated)",
        }
    }

    fn mode_present(mode: &str) -> &'static str {
        match mode {
            MODE_COPY => "copying",
            MODE_MOVE => "moving",
            _ => "moving (simulated)",
        }
    }

    /// `get_report(cancelled)` — on cancel the skipped count is the
    /// remainder.
    pub fn report(&self, cancelled: bool) -> String {
        let skipped = if cancelled {
            self.total.saturating_sub(self.success + self.failed)
        } else {
            self.skipped
        };
        format!(
            "{}:\nSuccessfully {}: {}\tSkipped: {}\tFailed: {}",
            self.name,
            Self::mode_past(&self.mode),
            self.success,
            skipped,
            self.failed
        )
    }
}

/// The run outcome (`OrganizeReport`): the report text per profile,
/// the session mutations to apply, and the undo log to persist.
#[derive(Clone, Debug, Default)]
pub struct OrganizeReport {
    pub text: String,
    pub failed_or_skipped: bool,
    pub applies: Vec<Apply>,
    pub undo: UndoCollection,
    /// A failure to replace the undo and adoption recovery pair.
    pub persistence_error: Option<String>,
}

/// One planned move (`BookToMove`).
#[derive(Clone, Debug)]
struct BookToMove {
    book_index: usize,
    path: String,
    profile_index: usize,
    failed_fields: Vec<String>,
}

enum MoveOutcome {
    Success,
    Failed,
    Skipped,
    Duplicate,
}

/// The run context (what the UI hands the engine).
pub struct RunContext<'a> {
    /// The full library snapshot (series lookups scan it).
    pub books: &'a [ComicBook],
    /// The indexes of the books to organize.
    pub selected: &'a [usize],
    /// The profiles to run, in order.
    pub profiles: &'a [Profile],
    /// How a successful Move enters its destination collection.
    pub move_landing: MoveLanding,
    /// Recycle-bin delete (`gio trash` on the UI side).
    pub trash: &'a dyn Fn(&str) -> bool,
    /// Optional durable notifications. Direct organizer runs use `None`.
    pub filesystem_effects: Option<&'a dyn FilesystemEffects>,
    /// Cover reads for fileless export and the duplicate dialog.
    pub cover: &'a dyn CoverSource,
    /// Where the undo log is written (None = no undo record).
    pub undo_path: Option<std::path::PathBuf>,
    /// The cancel flag.
    pub cancel: &'a std::sync::atomic::AtomicBool,
}

/// The destination-collection operation for a successful Move.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MoveLanding {
    #[default]
    UpdateExisting,
    InsertPreservingId,
}

pub fn run(ctx: RunContext, ui: &mut dyn OrganizeUi) -> OrganizeReport {
    let mut mover = Mover::new(ctx, ui);
    let move_landing = mover.move_landing;
    let mut report = mover.process_books();
    if let Some(path) = mover.undo_path {
        if !report.undo.is_empty() {
            let manifest_path = adoption_manifest_path(&path);
            let mut errors = Vec::new();
            if let Err(error) = report.undo.save(&path) {
                errors.push(format!("undo.dat: {error}"));
            }
            if move_landing == MoveLanding::InsertPreservingId {
                let manifest = AdoptionManifest::from_adoptions(&report.undo, &report.applies);
                if let Err(error) = manifest.save(&manifest_path) {
                    errors.push(format!("adoption manifest: {error}"));
                }
            } else if let Err(error) = remove_if_present(&manifest_path) {
                errors.push(format!("adoption manifest: {error}"));
            }
            if !errors.is_empty() {
                report.persistence_error = Some(errors.join("; "));
            }
        }
    }
    report
}

struct Mover<'a> {
    books: &'a [ComicBook],
    selected: &'a [usize],
    profiles: &'a [Profile],
    move_landing: MoveLanding,
    ui: &'a mut dyn OrganizeUi,
    trash: &'a dyn Fn(&str) -> bool,
    filesystem_effects: Option<&'a dyn FilesystemEffects>,
    cover: &'a dyn CoverSource,
    undo_path: Option<std::path::PathBuf>,
    cancel: &'a std::sync::atomic::AtomicBool,
    reports: Vec<ProfileReport>,
    moved_books: Vec<String>,
    created_paths: Vec<String>,
    undo: UndoCollection,
    applies: Vec<Apply>,
    always_duplicate: bool,
    duplicate_action: DuplicateAction,
    /// Book clones mutated mid-run (the overwrite read-percentage
    /// carry); move_book picks these up instead of the snapshot.
    pending_books: HashMap<usize, ComicBook>,
    counter: Option<i64>,
    multi: MultiValueState,
    held_count: usize,
    failed_or_skipped: bool,
}

impl<'a> Mover<'a> {
    fn rename(&self, source: &str, destination: &str) -> Result<(), String> {
        if let Some(effects) = self.filesystem_effects {
            effects
                .before_rename(source, destination)
                .map_err(|error| error.to_string())?;
        }
        let result = std::fs::rename(source, destination).map_err(|error| error.to_string());
        if let Some(effects) = self.filesystem_effects {
            effects
                .after_rename(source, destination, result.is_ok())
                .map_err(|error| error.to_string())?;
        }
        result
    }

    fn delete(&self, path: &str) -> Result<(), String> {
        if let Some(effects) = self.filesystem_effects {
            effects
                .before_delete(path)
                .map_err(|error| error.to_string())?;
        }
        let succeeded = (self.trash)(path);
        if let Some(effects) = self.filesystem_effects {
            effects
                .after_delete(path, succeeded)
                .map_err(|error| error.to_string())?;
        }
        succeeded
            .then_some(())
            .ok_or_else(|| format!("Failed to delete {path}."))
    }

    fn new(ctx: RunContext<'a>, ui: &'a mut dyn OrganizeUi) -> Self {
        Mover {
            books: ctx.books,
            selected: ctx.selected,
            profiles: ctx.profiles,
            move_landing: ctx.move_landing,
            ui,
            trash: ctx.trash,
            filesystem_effects: ctx.filesystem_effects,
            cover: ctx.cover,
            undo_path: ctx.undo_path,
            cancel: ctx.cancel,
            reports: Vec::new(),
            moved_books: Vec::new(),
            created_paths: Vec::new(),
            undo: UndoCollection::default(),
            applies: Vec::new(),
            always_duplicate: false,
            duplicate_action: DuplicateAction::Cancel,
            pending_books: HashMap::new(),
            counter: None,
            multi: MultiValueState::default(),
            held_count: 0,
            failed_or_skipped: false,
        }
    }

    fn log(&mut self, profile: Option<&str>, action: &str, path: &str, message: &str) {
        self.ui.log(LogEntry {
            profile: profile.unwrap_or_default().to_string(),
            action: action.to_string(),
            path: path.to_string(),
            message: message.to_string(),
        });
    }

    fn log_operation(&mut self, book_index: usize, path: &str, profile: &Profile, done: bool) {
        let source = if self.books[book_index].file_path.is_empty() {
            self.book_report_name(book_index)
        } else {
            self.books[book_index].file_path.clone()
        };
        let action = if done {
            ProfileReport::mode_past(&profile.mode)
        } else {
            ProfileReport::mode_present(&profile.mode)
        };
        self.log(Some(&profile.name), action, &source, &format!("to: {path}"));
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn process_books(&mut self) -> OrganizeReport {
        self.reports = self
            .profiles
            .iter()
            .map(|p| ProfileReport {
                name: p.name.clone(),
                mode: p.mode.clone(),
                total: self.selected.len(),
                success: 0,
                failed: 0,
                skipped: 0,
            })
            .collect();

        self.log(
            None,
            "Preparing",
            &format!("{} book(s)", self.selected.len()),
            "Calculating destinations",
        );
        let plan = self.create_book_paths();

        let mut held: Vec<BookToMove> = Vec::new();
        let mut done = 0usize;
        let total = plan.len();

        for item in plan {
            if self.cancelled() {
                let n = total - done;
                self.log(
                    None,
                    "Canceled",
                    &format!("{n} operations"),
                    "User cancelled the script",
                );
                return self.finish(true);
            }
            done += 1;
            self.ui.progress(done, total);

            let profile_index = item.profile_index;
            let outcome = self.process_book(&item);
            match outcome {
                MoveOutcome::Duplicate => {
                    done -= 1;
                    held.push(item);
                }
                MoveOutcome::Skipped => {
                    self.failed_or_skipped = true;
                    self.reports[profile_index].skipped += 1;
                }
                MoveOutcome::Failed => {
                    self.failed_or_skipped = true;
                    self.reports[profile_index].failed += 1;
                }
                MoveOutcome::Success => {
                    self.reports[profile_index].success += 1;
                }
            }
        }

        // The held duplicates are asked once at the end.
        self.held_count = held.len();
        for item in held {
            if self.cancelled() {
                let n = total - done;
                self.log(
                    None,
                    "Canceled",
                    &format!("{n} operations"),
                    "User cancelled the script",
                );
                return self.finish(true);
            }
            done += 1;
            self.ui.progress(done, total);
            self.held_count -= 1;
            let profile_index = item.profile_index;
            let outcome = self.process_duplicate_book(&item);
            match outcome {
                MoveOutcome::Skipped => {
                    self.failed_or_skipped = true;
                    self.reports[profile_index].skipped += 1;
                }
                MoveOutcome::Failed => {
                    self.failed_or_skipped = true;
                    self.reports[profile_index].failed += 1;
                }
                MoveOutcome::Success => {
                    self.reports[profile_index].success += 1;
                }
                MoveOutcome::Duplicate => {}
            }
        }

        self.finish(false)
    }

    fn finish(&mut self, cancelled: bool) -> OrganizeReport {
        let text = self
            .reports
            .iter()
            .map(|r| r.report(cancelled))
            .collect::<Vec<_>>()
            .join("\n\n");
        OrganizeReport {
            text,
            failed_or_skipped: self.failed_or_skipped,
            applies: std::mem::take(&mut self.applies),
            undo: std::mem::take(&mut self.undo),
            persistence_error: None,
        }
    }

    // ------------------------------------------------------------------
    // Planning
    // ------------------------------------------------------------------

    fn create_book_paths(&mut self) -> Vec<BookToMove> {
        let mut plan: Vec<BookToMove> = Vec::new();
        for &book_index in self.selected {
            let mut path = String::new();
            let mut profile_index: Option<usize> = None;
            let mut failed_fields: Vec<String> = Vec::new();

            for index in 0..self.profiles.len() {
                let (result, logs, failed_fields_out) = self.create_book_path(book_index, index);
                for entry in logs {
                    self.ui.log(entry);
                }
                match result {
                    PlanResult::Skipped => {
                        self.failed_or_skipped = true;
                        self.reports[index].skipped += 1;
                        continue;
                    }
                    PlanResult::Failed => {
                        self.failed_or_skipped = true;
                        self.reports[index].failed += 1;
                        continue;
                    }
                    PlanResult::Path(new_path) => {
                        let profile = &self.profiles[index];
                        if profile.mode == MODE_COPY {
                            plan.push(BookToMove {
                                book_index,
                                path: new_path,
                                profile_index: index,
                                failed_fields: failed_fields_out,
                            });
                            continue;
                        }
                        if !path.is_empty() {
                            // An earlier non-copy profile claimed the
                            // book; the addon marks THAT profile's
                            // plan skipped ("moved by a later
                            // profile") and the later profile wins.
                            let prev = profile_index.expect("claimed path");
                            self.reports[prev].skipped += 1;
                            let name = self.profiles[prev].name.clone();
                            self.log(
                                Some(&name),
                                "Skipped",
                                &self.book_report_name(book_index),
                                "The book is moved by a later profile",
                            );
                            self.failed_or_skipped = true;
                        }
                        path = new_path;
                        profile_index = Some(index);
                        failed_fields = failed_fields_out;
                    }
                }
            }

            let Some(profile_index) = profile_index else {
                continue;
            };
            let full_path = path;
            let file_name = Path::new(&full_path)
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default();
            if self.check_path_problems(book_index, &file_name, &full_path, profile_index) {
                continue;
            }
            plan.push(BookToMove {
                book_index,
                path: full_path,
                profile_index,
                failed_fields,
            });
        }
        plan
    }

    /// `create_book_path`: the destination for one book under one
    /// profile. Logs are returned (the token evaluation holds the
    /// worker's mutable state while it runs).
    fn create_book_path(
        &mut self,
        book_index: usize,
        index: usize,
    ) -> (PlanResult, Vec<LogEntry>, Vec<String>) {
        let profile = self.profiles[index].clone();
        let report_name = self.book_report_name(book_index);
        let mut logs: Vec<LogEntry> = Vec::new();

        let book = &self.books[book_index];
        if !book.file_path.is_empty() && !Path::new(&book.file_path).exists() {
            logs.push(LogEntry {
                profile: String::new(),
                action: "Failed".into(),
                path: report_name,
                message: "The file does not exist".into(),
            });
            return (PlanResult::Failed, logs, Vec::new());
        }

        // The exclude rules (own series-index borrow).
        let qualifies = {
            let mut series = SeriesIndex::new(self.books);
            rules::book_qualifies(book, book_index, &profile, &mut series)
        };
        if !qualifies {
            logs.push(LogEntry {
                profile: String::new(),
                action: "Skipped".into(),
                path: report_name,
                message: "The book qualified under the exclude rules".into(),
            });
            return (PlanResult::Skipped, logs, Vec::new());
        }

        if book.file_path.is_empty() {
            if !profile.move_fileless {
                logs.push(LogEntry {
                    profile: String::new(),
                    action: "Skipped".into(),
                    path: report_name,
                    message: "The book is fileless and fileless images are not being created"
                        .into(),
                });
                return (PlanResult::Skipped, logs, Vec::new());
            }
            if book.custom_thumbnail_key.is_none() {
                logs.push(LogEntry {
                    profile: String::new(),
                    action: "Failed".into(),
                    path: report_name,
                    message: "The fileless book does not have a custom thumbnail".into(),
                });
                return (PlanResult::Failed, logs, Vec::new());
            }
        }

        // The token evaluation scope: working state restored after.
        enum Out {
            Path(String),
            BlankName,
        }
        let (out, failed, failed_fields, counter, multi) = {
            let mut series = SeriesIndex::new(self.books);
            let mut failed_fields: Vec<String> = Vec::new();
            let mut failed = false;
            let mut counter = std::mem::take(&mut self.counter);
            let mut multi = std::mem::take(&mut self.multi);
            let book = &self.books[book_index];
            let mut ctx = TokenCtx::new(
                book,
                book_index,
                &profile,
                &mut series,
                &mut failed_fields,
                &mut failed,
                &mut counter,
                &mut multi,
                self.ui,
            );
            let (folder_path, file_name, failed) =
                ctx.make_path(&profile.folder_template, &profile.file_template);
            let full_path = path_combine(&folder_path, &file_name);
            let out = if file_name.is_empty() {
                Out::BlankName
            } else {
                Out::Path(full_path)
            };
            (out, failed, failed_fields, counter, multi)
        };
        self.counter = counter;
        self.multi = multi;

        if failed {
            self.failed_or_skipped = true;
            if !profile.move_failed {
                let fields = failed_fields.join(", ");
                let verb = if failed_fields.len() == 1 {
                    " is"
                } else {
                    " are"
                };
                logs.push(LogEntry {
                    profile: String::new(),
                    action: "Failed".into(),
                    path: self.book_report_name(book_index),
                    message: format!("{fields}{verb} empty."),
                });
                return (PlanResult::Failed, logs, failed_fields);
            }
        }
        match out {
            Out::BlankName => {
                logs.push(LogEntry {
                    profile: String::new(),
                    action: "Failed".into(),
                    path: self.book_report_name(book_index),
                    message: "The created filename was blank".into(),
                });
                (PlanResult::Failed, logs, failed_fields)
            }
            Out::Path(p) => (PlanResult::Path(p), logs, failed_fields),
        }
    }

    fn check_path_problems(
        &mut self,
        book_index: usize,
        file_name: &str,
        full_path: &str,
        profile_index: usize,
    ) -> bool {
        let book = &self.books[book_index];
        let report_name = self.book_report_name(book_index);
        if full_path == book.file_path {
            self.log(
                None,
                "Skipped",
                &report_name,
                "The book is already located at the calculated path",
            );
            self.failed_or_skipped = true;
            self.reports[profile_index].skipped += 1;
            return true;
        }
        if full_path.to_lowercase() == book.file_path.to_lowercase() {
            // A case-only difference: rename the file in place.
            if self.profiles[profile_index].is_simulate() {
                self.log(None, "Renaming", &report_name, &format!("to: {full_path}"));
            } else {
                self.rename_file_in_place(book_index, file_name);
            }
            self.log(
                None,
                "Skipped",
                &report_name,
                "The book is already located at the calculated path",
            );
            self.failed_or_skipped = true;
            self.reports[profile_index].skipped += 1;
            return true;
        }
        false
    }

    /// `book.RenameFile(file_name)` — rename in the same directory
    /// and update the book.
    fn rename_file_in_place(&mut self, book_index: usize, file_name: &str) {
        let book = self.books[book_index].clone();
        let Some(parent) = Path::new(&book.file_path).parent() else {
            return;
        };
        let new_path = parent.join(file_name);
        if self
            .rename(&book.file_path, &new_path.to_string_lossy())
            .is_ok()
        {
            let mut updated = book;
            updated.file_path = new_path.to_string_lossy().into_owned();
            self.applies.push(Apply::Update(updated));
        }
    }

    fn book_report_name(&self, book_index: usize) -> String {
        let book = &self.books[book_index];
        if !book.file_path.is_empty() {
            book.file_path.clone()
        } else {
            // The caption stand-in: series with number.
            let mut text = book.info.series.clone();
            if !book.info.number.is_empty() {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(&book.info.number);
            }
            if text.is_empty() {
                text = format!("Book {}", book.id.to_d_string());
            }
            text
        }
    }

    // ------------------------------------------------------------------
    // Processing
    // ------------------------------------------------------------------

    fn process_book(&mut self, item: &BookToMove) -> MoveOutcome {
        let book_index = item.book_index;
        let full_path = item.path.clone();
        let report_name = self.book_report_name(book_index);
        let profile = self.profiles[item.profile_index].clone();

        if Path::new(&full_path).exists() || self.moved_books.contains(&full_path) {
            return MoveOutcome::Duplicate;
        }

        self.log_operation(book_index, &full_path, &profile, false);

        let old_folder = file_directory(&self.books[book_index]);
        let folder_path = Path::new(&full_path)
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();

        if !self.create_folder(&folder_path, &report_name, &profile) {
            return MoveOutcome::Failed;
        }

        let outcome = if self.books[book_index].file_path.is_empty() {
            self.create_fileless_image(book_index, &full_path, &profile)
        } else {
            self.move_book(book_index, &full_path, &profile)
        };

        if profile.remove_empty_folder
            && profile.mode == MODE_MOVE
            && self.filesystem_effects.is_none()
        {
            if !old_folder.is_empty() {
                self.remove_empty_folders(Path::new(&old_folder));
            }
            self.remove_empty_folders(Path::new(&folder_path));
        }

        if !item.failed_fields.is_empty() && matches!(outcome, MoveOutcome::Success) {
            let fields = item.failed_fields.join(",");
            let verb = if item.failed_fields.len() == 1 {
                " is"
            } else {
                " are"
            };
            let past = ProfileReport::mode_past(&profile.mode);
            if self.move_landing == MoveLanding::InsertPreservingId && profile.mode == MODE_MOVE {
                let original = Path::new(&self.books[book_index].file_path);
                let parent_ready = original
                    .parent()
                    .is_none_or(|parent| std::fs::create_dir_all(parent).is_ok());
                if parent_ready && self.rename(&full_path, &original.to_string_lossy()).is_ok() {
                    self.applies.retain(|apply| !matches!(apply, Apply::Adopt(book) if book.id == self.books[book_index].id));
                    self.undo.remove_current(&full_path);
                    if profile.remove_empty_folder {
                        if let Some(parent) = Path::new(&full_path).parent() {
                            self.remove_empty_folders(parent);
                        }
                    }
                    self.log(
                        None,
                        "Failed",
                        &report_name,
                        &format!("{fields}{verb} empty. The move was rolled back."),
                    );
                    return MoveOutcome::Failed;
                }
                self.log(
                    None,
                    "Warning",
                    &report_name,
                    &format!("{fields}{verb} empty. {past} to {full_path}"),
                );
                return MoveOutcome::Success;
            }
            self.log(
                None,
                "Failed",
                &report_name,
                &format!("{fields}{verb} empty. {past} to {full_path}"),
            );
            return MoveOutcome::Failed;
        }
        if matches!(outcome, MoveOutcome::Success) {
            self.log_operation(book_index, &full_path, &profile, true);
        }
        outcome
    }

    /// `move_book`.
    fn move_book(&mut self, book_index: usize, path: &str, profile: &Profile) -> MoveOutcome {
        let book = match self.pending_books.remove(&book_index) {
            Some(pending) => pending,
            None => self.books[book_index].clone(),
        };
        let report_name = self.book_report_name(book_index);
        let result: Result<(), String> = if profile.is_simulate() {
            self.moved_books.push(path.to_string());
            Ok(())
        } else if profile.is_copy() {
            std::fs::copy(&book.file_path, path)
                .map(|_| ())
                .map_err(|e| e.to_string())
        } else {
            self.rename(&book.file_path, path)
        };

        match result {
            Ok(()) => {
                if profile.is_simulate() {
                    return MoveOutcome::Success;
                }
                if profile.is_copy() {
                    if profile.copy_mode {
                        let mut new_book = ComicBook {
                            id: CrGuid::new_random(),
                            file_path: path.to_string(),
                            ..ComicBook::default()
                        };
                        copy_data(&book, &mut new_book);
                        self.applies.push(Apply::Insert(new_book));
                    }
                    return MoveOutcome::Success;
                }
                // Move: update the book and record the undo entry.
                let mut updated = book.clone();
                updated.file_path = path.to_string();
                self.undo.append(&book.file_path, path, &profile.name);
                self.applies.push(match self.move_landing {
                    MoveLanding::UpdateExisting => Apply::Update(updated),
                    MoveLanding::InsertPreservingId => Apply::Adopt(updated),
                });
                MoveOutcome::Success
            }
            Err(e) => {
                self.log(
                    None,
                    "Failed",
                    &report_name,
                    &format!("because an error occured. The error was: {e}"),
                );
                MoveOutcome::Failed
            }
        }
    }

    /// `create_fileless_image`: writes the cover image in the
    /// profile's format.
    fn create_fileless_image(
        &mut self,
        book_index: usize,
        path: &str,
        profile: &Profile,
    ) -> MoveOutcome {
        let book = self.books[book_index].clone();
        let report_name = self.book_report_name(book_index);
        let format = match profile.fileless_format.as_str() {
            ".jpg" => Some(cr_image::ImageFormat::Jpeg),
            ".png" => Some(cr_image::ImageFormat::Png),
            ".bmp" => Some(cr_image::ImageFormat::Bmp),
            _ => None,
        };
        let result: Result<(), String> = if profile.is_simulate() {
            self.moved_books.push(path.to_string());
            Ok(())
        } else {
            match (self.cover.fileless_cover(&book), format) {
                (Some(image), Some(format)) => {
                    let bytes = if format == cr_image::ImageFormat::Jpeg {
                        cr_image::encode_jpeg(&image, 90)
                    } else {
                        cr_image::decode::encode_image(&image, format)
                    };
                    match bytes {
                        Ok(bytes) => std::fs::write(path, bytes)
                            .map(|_| ())
                            .map_err(|e| e.to_string()),
                        Err(e) => Err(e.to_string()),
                    }
                }
                (None, _) => Err("the cover image could not be loaded".to_string()),
                (_, None) => Err(format!(
                    "unsupported image format {}",
                    profile.fileless_format
                )),
            }
        };
        match result {
            Ok(()) => MoveOutcome::Success,
            Err(e) => {
                self.log(
                    None,
                    "Failed",
                    &report_name,
                    &format!(
                        "Failed to create the image because an error occured. The error was: {e}"
                    ),
                );
                MoveOutcome::Failed
            }
        }
    }

    /// `create_folder`.
    fn create_folder(&mut self, folder_path: &str, report_name: &str, profile: &Profile) -> bool {
        if Path::new(folder_path).exists() {
            return true;
        }
        if profile.is_simulate() {
            if !self.created_paths.iter().any(|p| p == folder_path) {
                self.log(None, "Created Folder", folder_path, "");
                self.created_paths.push(folder_path.to_string());
            }
            return true;
        }
        match std::fs::create_dir_all(folder_path) {
            Ok(()) => true,
            Err(e) => {
                self.log(
                    None,
                    "Failed to create folder",
                    folder_path,
                    &format!("Book {report_name} was not moved.\nThe error was: {e}"),
                );
                false
            }
        }
    }

    /// `create_rename_path` — `base (1)`, `base (2)`, …
    fn create_rename_path(&self, path: &str) -> Option<String> {
        static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
        let re = RE.get_or_init(|| regex::Regex::new(r" \([0-9]\)$").expect("rename regex"));
        let extension = path_extension(path);
        let base = &path[..path.len() - extension.len()];
        let base = re.replace_all(base, "").into_owned();
        for i in 1..=100 {
            let newpath = format!("{base} ({i}){extension}");
            if self.moved_books.contains(&newpath) {
                continue;
            }
            if Path::new(&newpath).exists() {
                continue;
            }
            return Some(newpath);
        }
        None
    }

    /// `process_duplicate_book`.
    fn process_duplicate_book(&mut self, item: &BookToMove) -> MoveOutcome {
        let book_index = item.book_index;
        let full_path = item.path.clone();
        let report_name = self.book_report_name(book_index);
        let profile = self.profiles[item.profile_index].clone();

        if Path::new(&full_path).exists() || self.moved_books.contains(&full_path) {
            // The existing book in the library, if any.
            let old_index = self.books.iter().position(|b| b.file_path == full_path);
            let rename_path = self.create_rename_path(&full_path);

            if !self.always_duplicate {
                let ask = DuplicateAsk {
                    mode: profile.mode.clone(),
                    rename_filename: rename_path
                        .as_ref()
                        .map(|p| file_name_of(p))
                        .unwrap_or_default(),
                    held_count: self.held_count,
                    new_book: Some(self.duplicate_info(book_index)),
                    old_book: Some(match old_index {
                        Some(i) => self.duplicate_info(i),
                        None => duplicate_info_of_file(&full_path),
                    }),
                };
                let answer = self.ui.ask_duplicate(ask);
                self.duplicate_action = answer.action;
                if answer.always {
                    self.always_duplicate = true;
                }
            }

            match self.duplicate_action {
                DuplicateAction::Cancel => {
                    let kind = if !self.books[book_index].file_path.is_empty() {
                        "A file already exists at"
                    } else {
                        "The image already exists at"
                    };
                    self.log(
                        None,
                        "Skipped",
                        &report_name,
                        &format!("{kind}: {full_path} and the user declined to overwrite it"),
                    );
                    return MoveOutcome::Skipped;
                }
                DuplicateAction::Rename => {
                    let Some(rename_path) = rename_path else {
                        self.log(
                            None,
                            "Failed",
                            &report_name,
                            "no free rename name was found",
                        );
                        return MoveOutcome::Failed;
                    };
                    let next = BookToMove {
                        book_index,
                        path: rename_path,
                        profile_index: item.profile_index,
                        failed_fields: item.failed_fields.clone(),
                    };
                    return self.process_duplicate_book(&next);
                }
                DuplicateAction::Overwrite => {
                    if profile.is_simulate() {
                        // No files change in simulate mode (the
                        // addon's simulate guard against its own
                        // rename loop).
                        self.log(None, "Deleted (simulated)", &full_path, "");
                        if !self.books[book_index].file_path.is_empty() {
                            self.log(
                                None,
                                ProfileReport::mode_past(&profile.mode),
                                &self.books[book_index].file_path,
                                &format!("to: {full_path}"),
                            );
                        } else {
                            self.log(None, "Created image", &full_path, "");
                        }
                        self.moved_books.push(full_path);
                        return MoveOutcome::Success;
                    }
                    if profile.copy_read_percentage {
                        if let Some(old_index) = old_index {
                            // Carries the read state of the replaced
                            // book onto the moved book.
                            let old = self.books[old_index].clone();
                            let mut updated = self
                                .pending_books
                                .get(&book_index)
                                .cloned()
                                .unwrap_or_else(|| self.books[book_index].clone());
                            updated.last_page_read = old.last_page_read;
                            self.pending_books.insert(book_index, updated);
                        }
                    }
                    if self.delete(&full_path).is_err() {
                        self.log(
                            None,
                            "Failed",
                            &report_name,
                            &format!("Failed to overwrite {full_path}."),
                        );
                        return MoveOutcome::Failed;
                    }
                    // Remove the replaced book from the library (only
                    // for file books).
                    if !self.books[book_index].file_path.is_empty() {
                        if let Some(old_index) = old_index {
                            let old = self.books[old_index].clone();
                            self.applies.push(Apply::Remove(old.id));
                        }
                    }
                    return self.process_duplicate_book(item);
                }
            }
        }

        let old_folder = file_directory(&self.books[book_index]);
        self.log_operation(book_index, &full_path, &profile, false);
        let outcome = if self.books[book_index].file_path.is_empty() {
            let folder_path = Path::new(&full_path)
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            if !self.create_folder(&folder_path, &report_name, &profile) {
                return MoveOutcome::Failed;
            }
            self.create_fileless_image(book_index, &full_path, &profile)
        } else {
            self.move_book(book_index, &full_path, &profile)
        };

        if profile.remove_empty_folder
            && profile.mode == MODE_MOVE
            && self.filesystem_effects.is_none()
        {
            if !old_folder.is_empty() {
                self.remove_empty_folders(Path::new(&old_folder));
            }
            if let Some(dir) = Path::new(&full_path).parent() {
                self.remove_empty_folders(dir);
            }
        }

        if !item.failed_fields.is_empty() && matches!(outcome, MoveOutcome::Success) {
            let fields = item.failed_fields.join(",");
            let verb = if item.failed_fields.len() == 1 {
                " is"
            } else {
                " are"
            };
            let present = ProfileReport::mode_present(&profile.mode);
            if self.move_landing == MoveLanding::InsertPreservingId && profile.mode == MODE_MOVE {
                if self
                    .rename(&full_path, &self.books[book_index].file_path)
                    .is_ok()
                {
                    self.applies.retain(|apply| !matches!(apply, Apply::Adopt(book) if book.id == self.books[book_index].id));
                    self.undo.remove_current(&full_path);
                    self.log(
                        None,
                        "Failed",
                        &report_name,
                        &format!("{fields}{verb} empty. The move was rolled back."),
                    );
                    return MoveOutcome::Failed;
                }
                self.log(
                    None,
                    "Warning",
                    &report_name,
                    &format!("{fields}{verb} empty. {present} to {full_path}"),
                );
                return MoveOutcome::Success;
            }
            self.log(
                None,
                "Failed",
                &report_name,
                &format!("{fields}{verb} empty. {present} to {full_path}"),
            );
            return MoveOutcome::Failed;
        }
        if matches!(outcome, MoveOutcome::Success) {
            self.log_operation(book_index, &full_path, &profile, true);
        }
        outcome
    }

    fn duplicate_info(&self, book_index: usize) -> DuplicateBookInfo {
        let book = &self.books[book_index];
        let prop = cr_engine::matcher::book_view::proposed_cached(book);
        DuplicateBookInfo {
            in_library: true,
            series: cr_engine::matcher::book_view::shadow_series(book, &prop).to_string(),
            volume: cr_engine::matcher::book_view::shadow_volume(book, &prop),
            number: cr_engine::matcher::book_view::shadow_number(book, &prop).to_string(),
            page_count: book.info.page_count,
            file_size_text: file_size_text(book.file_size),
            published_text: published_text(book),
            added_text: fields::date_display(&book.added_time),
            path: book.file_path.clone(),
            scan_info: book.info.scan_information.clone(),
            cover: self.cover.duplicate_cover(book),
        }
    }

    /// `remove_empty_folders`: recursive prune until a non-empty
    /// directory or an excluded path.
    fn remove_empty_folders(&mut self, directory: &Path) {
        if !directory.exists() {
            return;
        }
        let excluded = self
            .profiles
            .iter()
            .flat_map(|p| p.excluded_empty_folder.iter())
            .any(|p| *p == directory.to_string_lossy());
        let is_empty = std::fs::read_dir(directory)
            .map(|mut d| d.next().is_none())
            .unwrap_or(false);
        if is_empty && !excluded {
            let parent = directory.parent().map(|p| p.to_path_buf());
            if std::fs::remove_dir(directory).is_ok() {
                if let Some(parent) = parent {
                    self.remove_empty_folders(&parent);
                }
            }
        }
    }
}

enum PlanResult {
    Path(String),
    Failed,
    Skipped,
}

fn file_name_of(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The existing-file info pane for the duplicate dialog
/// (the addon's `FileInfo` branch).
fn duplicate_info_of_file(path: &str) -> DuplicateBookInfo {
    let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
    DuplicateBookInfo {
        in_library: false,
        series: "Comic not in Library".to_string(),
        volume: -1,
        number: String::new(),
        page_count: 0,
        file_size_text: format!("{:.2} MB", size as f64 / 1048576.0),
        published_text: String::new(),
        added_text: String::new(),
        path: path.to_string(),
        scan_info: String::new(),
        cover: None,
    }
}

fn file_size_text(size: i64) -> String {
    // The C# FileLengthFormat: Bytes / kB / MB / GB, two decimals.
    if size < 0 {
        return "Unknown".to_string();
    }
    if size < 1024 {
        return format!("{size} Bytes");
    }
    if size < 1048576 {
        return format!("{:.2} kB", size as f64 / 1024.0);
    }
    if size < 1073741824 {
        return format!("{:.2} MB", size as f64 / 1024.0 / 1024.0);
    }
    format!("{:.2} GB", size as f64 / 1024.0 / 1024.0 / 1024.0)
}

fn published_text(book: &ComicBook) -> String {
    let month = if book.info.month != -1 {
        book.info.month.to_string()
    } else {
        String::new()
    };
    let year = if book.info.year != -1 {
        book.info.year.to_string()
    } else {
        String::new()
    };
    format!("{month}, {year}")
}

/// `CopyData` — the metadata the copy-mode insert carries
/// (lobookmover.py `CopyData`).
fn copy_data(book: &ComicBook, new_book: &mut ComicBook) {
    new_book.info.series = book.info.series.clone();
    new_book.info.number = book.info.number.clone();
    new_book.info.count = book.info.count;
    new_book.info.month = book.info.month;
    new_book.info.year = book.info.year;
    new_book.info.format = book.info.format.clone();
    new_book.info.title = book.info.title.clone();
    new_book.info.publisher = book.info.publisher.clone();
    new_book.info.alternate_series = book.info.alternate_series.clone();
    new_book.info.alternate_number = book.info.alternate_number.clone();
    new_book.info.alternate_count = book.info.alternate_count;
    new_book.info.imprint = book.info.imprint.clone();
    new_book.info.writer = book.info.writer.clone();
    new_book.info.penciller = book.info.penciller.clone();
    new_book.info.inker = book.info.inker.clone();
    new_book.info.colorist = book.info.colorist.clone();
    new_book.info.letterer = book.info.letterer.clone();
    new_book.info.cover_artist = book.info.cover_artist.clone();
    new_book.info.editor = book.info.editor.clone();
    new_book.info.age_rating = book.info.age_rating.clone();
    new_book.info.manga = book.info.manga;
    new_book.info.language_iso = book.info.language_iso.clone();
    new_book.info.black_and_white = book.info.black_and_white;
    new_book.info.genre = book.info.genre.clone();
    new_book.info.tags = book.info.tags.clone();
    new_book.series_complete = book.series_complete;
    new_book.info.summary = book.info.summary.clone();
    new_book.info.characters = book.info.characters.clone();
    new_book.info.teams = book.info.teams.clone();
    new_book.info.locations = book.info.locations.clone();
    new_book.info.notes = book.info.notes.clone();
    new_book.info.web = book.info.web.clone();
    new_book.info.scan_information = book.info.scan_information.clone();
    new_book.info.day = book.info.day;
}

// ---------------------------------------------------------------------------
// Undo
// ---------------------------------------------------------------------------

/// The undo run report (`UndoMover.process_books`).
#[derive(Clone, Debug, Default)]
pub struct UndoReport {
    pub text: String,
    pub failed_or_skipped: bool,
    pub applies: Vec<Apply>,
    /// Entries that did not complete and can be retried.
    pub residual: UndoCollection,
}

/// Moves the books in the undo collection back (`UndoMover`).
pub fn run_undo(
    ctx: RunContext,
    undo: &UndoCollection,
    profiles: &HashMap<String, Profile>,
    ui: &mut dyn OrganizeUi,
) -> UndoReport {
    let mut u = UndoMover {
        ctx,
        undo,
        profiles,
        ui,
        applies: Vec::new(),
        success: 0,
        failed: 0,
        skipped: 0,
        count: 0,
        residual: undo.clone(),
    };
    u.process_books()
}

struct UndoMover<'a> {
    ctx: RunContext<'a>,
    undo: &'a UndoCollection,
    profiles: &'a HashMap<String, Profile>,
    ui: &'a mut dyn OrganizeUi,
    applies: Vec<Apply>,
    success: usize,
    failed: usize,
    skipped: usize,
    count: usize,
    residual: UndoCollection,
}

impl<'a> UndoMover<'a> {
    fn rename(&self, source: &str, destination: &str) -> Result<(), String> {
        if let Some(effects) = self.ctx.filesystem_effects {
            effects
                .before_rename(source, destination)
                .map_err(|error| error.to_string())?;
        }
        let result = std::fs::rename(source, destination).map_err(|error| error.to_string());
        if let Some(effects) = self.ctx.filesystem_effects {
            effects
                .after_rename(source, destination, result.is_ok())
                .map_err(|error| error.to_string())?;
        }
        result
    }

    fn delete(&self, path: &str) -> Result<(), String> {
        if let Some(effects) = self.ctx.filesystem_effects {
            effects
                .before_delete(path)
                .map_err(|error| error.to_string())?;
        }
        let succeeded = (self.ctx.trash)(path);
        if let Some(effects) = self.ctx.filesystem_effects {
            effects
                .after_delete(path, succeeded)
                .map_err(|error| error.to_string())?;
        }
        succeeded
            .then_some(())
            .ok_or_else(|| format!("Failed to delete {path}."))
    }

    fn total(&self) -> usize {
        self.undo.current_paths.len()
    }

    fn log(&mut self, profile: &str, action: &str, path: &str, message: &str) {
        self.ui.log(LogEntry {
            profile: profile.to_string(),
            action: action.to_string(),
            path: path.to_string(),
            message: message.to_string(),
        });
    }

    fn cancelled_report(&mut self) -> UndoReport {
        let skipped = self.total().saturating_sub(self.success + self.failed);
        self.log(
            "",
            "Canceled",
            &format!("{skipped} files"),
            "User cancelled the script",
        );
        UndoReport {
            text: format!(
                "Successfully moved: {}\tFailed to move: {}\tSkipped: {}",
                self.success, self.failed, skipped
            ),
            failed_or_skipped: self.failed > 0 || skipped > 0,
            applies: std::mem::take(&mut self.applies),
            residual: std::mem::take(&mut self.residual),
        }
    }

    fn process_books(&mut self) -> UndoReport {
        self.log(
            "",
            "Preparing",
            &format!("{} file(s)", self.total()),
            "Calculating restore operations",
        );
        // Library books first, then the not-found paths.
        let mut items: Vec<(usize, String)> = Vec::new();
        let mut not_found: Vec<String> = Vec::new();
        for current in &self.undo.current_paths {
            match self.ctx.books.iter().position(|b| &b.file_path == current) {
                Some(i) => items.push((i, current.clone())),
                None => not_found.push(current.clone()),
            }
        }

        let mut held: Vec<HeldUndo> = Vec::new();

        for (book_index, current) in items {
            if self.ctx.cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return self.cancelled_report();
            }
            self.count += 1;
            self.ui.progress(self.count, self.total());
            self.process_item(Some(book_index), &current, &mut held);
        }
        for current in not_found {
            if self.ctx.cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return self.cancelled_report();
            }
            self.count += 1;
            self.ui.progress(self.count, self.total());
            self.process_item(None, &current, &mut held);
        }

        // The held duplicates go through the duplicate dialog.
        for h in held {
            if self.ctx.cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return self.cancelled_report();
            }
            self.count += 1;
            self.ui.progress(self.count, self.total());
            self.process_duplicate(h);
        }

        UndoReport {
            text: format!(
                "Successfully moved: {}\tFailed to move: {}\tSkipped: {}",
                self.success, self.failed, self.skipped
            ),
            failed_or_skipped: self.failed > 0 || self.skipped > 0,
            applies: std::mem::take(&mut self.applies),
            residual: std::mem::take(&mut self.residual),
        }
    }

    fn profile_for(&self, current: &str) -> Profile {
        self.undo
            .profile(current)
            .and_then(|name| self.profiles.get(name))
            .cloned()
            .unwrap_or_default()
    }

    fn process_item(&mut self, book_index: Option<usize>, current: &str, held: &mut Vec<HeldUndo>) {
        let undo_path = match self.undo.undo_path(current) {
            Some(p) => p.to_string(),
            None => return,
        };
        let profile = self.profile_for(current);
        let report_name = book_index
            .map(|i| self.ctx.books[i].file_path.clone())
            .unwrap_or_else(|| current.to_string());

        if !Path::new(current).exists() {
            self.log(
                &profile.name,
                "Failed",
                &report_name,
                "The file does not exist",
            );
            self.failed += 1;
            return;
        }
        if undo_path == current {
            self.log(
                &profile.name,
                "Skipped",
                &report_name,
                "The book is already located at the calculated path",
            );
            self.skipped += 1;
            return;
        }
        if Path::new(&undo_path).exists() {
            held.push(HeldUndo {
                book_index,
                current: current.to_string(),
                undo_path,
            });
            self.count -= 1;
            return;
        }
        let parent = Path::new(&undo_path).parent().unwrap_or(Path::new(""));
        if !parent.exists() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                self.log(
                    &profile.name,
                    "Failed",
                    &undo_path,
                    &format!("The error was: {e}"),
                );
                self.failed += 1;
                return;
            }
        }
        let old_folder = book_index
            .map(|i| file_directory(&self.ctx.books[i]))
            .unwrap_or_else(|| {
                Path::new(current)
                    .parent()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });
        let source = book_index
            .map(|i| self.ctx.books[i].file_path.as_str())
            .unwrap_or(current)
            .to_string();
        self.log(
            &profile.name,
            "restoring",
            &source,
            &format!("to: {undo_path}"),
        );
        let result = self.rename(&source, &undo_path);
        match result {
            Ok(()) => {
                if let Some(i) = book_index {
                    let mut updated = self.ctx.books[i].clone();
                    updated.file_path = undo_path.clone();
                    self.applies.push(Apply::Update(updated));
                }
                if profile.remove_empty_folder && self.ctx.filesystem_effects.is_none() {
                    prune_empty(Path::new(&old_folder), &profile);
                    prune_empty(parent, &profile);
                }
                self.success += 1;
                self.remove_residual(current);
                self.log(
                    &profile.name,
                    "restored",
                    &source,
                    &format!("to: {undo_path}"),
                );
            }
            Err(e) => {
                self.log(
                    &profile.name,
                    "Failed",
                    &report_name,
                    &format!("because an error occured. The error was: {e}"),
                );
                self.failed += 1;
            }
        }
    }

    fn process_duplicate(&mut self, held: HeldUndo) {
        let profile = self.profile_for(&held.current);
        let ask = DuplicateAsk {
            mode: profile.mode.clone(),
            rename_filename: String::new(),
            held_count: 0,
            new_book: None,
            old_book: None,
        };
        let answer = self.ui.ask_duplicate(ask);
        match answer.action {
            DuplicateAction::Cancel => {
                self.log(
                    &profile.name,
                    "Skipped",
                    &held.current,
                    &format!(
                        "A file already exists at: {} and the user declined to overwrite it",
                        held.undo_path
                    ),
                );
                self.skipped += 1;
                return;
            }
            DuplicateAction::Rename => {
                self.log(
                    &profile.name,
                    "Failed",
                    &held.current,
                    "the undo run cannot rename; remove the file at the destination first",
                );
                self.failed += 1;
                return;
            }
            DuplicateAction::Overwrite => {}
        }

        let source = held
            .book_index
            .map(|i| self.ctx.books[i].file_path.as_str())
            .unwrap_or(&held.current)
            .to_string();
        self.log(
            &profile.name,
            "restoring",
            &source,
            &format!("to: {}", held.undo_path),
        );
        if self.delete(&held.undo_path).is_err() {
            self.log(
                &profile.name,
                "Failed",
                &held.current,
                &format!("Failed to overwrite {}.", held.undo_path),
            );
            self.failed += 1;
            return;
        }
        let mut landing_book = held.book_index.map(|i| self.ctx.books[i].clone());
        if held.book_index.is_some() {
            if let Some(existing) = self
                .ctx
                .books
                .iter()
                .position(|b| b.file_path == held.undo_path)
            {
                let old_book = self.ctx.books[existing].clone();
                if let Some(updated) = &mut landing_book {
                    updated.last_page_read = old_book.last_page_read;
                }
                self.applies.push(Apply::Remove(old_book.id));
            }
        }

        let parent = Path::new(&held.undo_path)
            .parent()
            .unwrap_or(Path::new(""))
            .to_path_buf();
        if !parent.exists() {
            let _ = std::fs::create_dir_all(&parent);
        }
        let result = self.rename(&source, &held.undo_path);
        match result {
            Ok(()) => {
                if let Some(mut updated) = landing_book {
                    updated.file_path = held.undo_path.clone();
                    self.applies.push(Apply::Update(updated));
                }
                if profile.remove_empty_folder && self.ctx.filesystem_effects.is_none() {
                    let old_folder = held
                        .book_index
                        .map(|i| file_directory(&self.ctx.books[i]))
                        .unwrap_or_else(|| {
                            Path::new(&held.current)
                                .parent()
                                .map(|p| p.to_string_lossy().into_owned())
                                .unwrap_or_default()
                        });
                    prune_empty(Path::new(&old_folder), &profile);
                    prune_empty(&parent, &profile);
                }
                self.success += 1;
                self.remove_residual(&held.current);
                self.log(
                    &profile.name,
                    "restored",
                    &source,
                    &format!("to: {}", held.undo_path),
                );
            }
            Err(e) => {
                self.log(
                    &profile.name,
                    "Failed",
                    &held.current,
                    &format!("because an error occured. The error was: {e}"),
                );
                self.failed += 1;
            }
        }
    }

    fn remove_residual(&mut self, current: &str) {
        self.residual.remove_current(current);
    }
}

struct HeldUndo {
    book_index: Option<usize>,
    current: String,
    undo_path: String,
}

fn prune_empty(directory: &Path, profile: &Profile) {
    if !directory.exists() {
        return;
    }
    let excluded = profile
        .excluded_empty_folder
        .iter()
        .any(|p| *p == directory.to_string_lossy());
    let is_empty = std::fs::read_dir(directory)
        .map(|mut d| d.next().is_none())
        .unwrap_or(false);
    if is_empty && !excluded {
        let parent = directory.parent().map(|p| p.to_path_buf());
        if std::fs::remove_dir(directory).is_ok() {
            if let Some(parent) = parent {
                prune_empty(&parent, profile);
            }
        }
    }
}

#[cfg(test)]
mod adoption_manifest_tests {
    use super::*;

    fn undo() -> UndoCollection {
        let mut undo = UndoCollection::default();
        undo.append("/incoming/a.cbz", "/library/a.cbz", "Move");
        undo.append("/incoming/b.cbz", "/library/b.cbz", "Move");
        undo
    }

    #[test]
    fn manifest_requires_the_exact_undo_identity() {
        let manifest = AdoptionManifest {
            entries: vec![
                AdoptionManifestEntry {
                    id: "00000000-0000-0000-0000-000000000001".into(),
                    profile_name: "Move".into(),
                    current_path: "/library/a.cbz".into(),
                    undo_path: "/incoming/a.cbz".into(),
                },
                AdoptionManifestEntry {
                    id: "00000000-0000-0000-0000-000000000002".into(),
                    profile_name: "Move".into(),
                    current_path: "/library/b.cbz".into(),
                    undo_path: "/incoming/b.cbz".into(),
                },
            ],
        };
        assert!(manifest.matches(&undo()));

        let mut changed = undo();
        changed.profile_names[1] = "Other".into();
        assert!(!manifest.matches(&changed));
    }

    #[test]
    fn residual_manifest_keeps_only_failed_entries() {
        let mut manifest = AdoptionManifest {
            entries: vec![
                AdoptionManifestEntry {
                    id: "1".into(),
                    profile_name: "Move".into(),
                    current_path: "/library/a.cbz".into(),
                    undo_path: "/incoming/a.cbz".into(),
                },
                AdoptionManifestEntry {
                    id: "2".into(),
                    profile_name: "Move".into(),
                    current_path: "/library/b.cbz".into(),
                    undo_path: "/incoming/b.cbz".into(),
                },
            ],
        };
        let mut residual = UndoCollection::default();
        residual.append("/incoming/b.cbz", "/library/b.cbz", "Move");

        manifest.retain_undo(&residual);

        assert_eq!(manifest.entries.len(), 1);
        assert_eq!(manifest.entries[0].id, "2");
    }

    #[test]
    fn missing_manifest_blocks_only_undo_paths_below_incoming_roots() {
        let mut undo = UndoCollection::default();
        undo.append("/incoming/series/book.cbz", "/library/book.cbz", "Adopt");
        assert!(missing_manifest_is_unsafe(
            &undo,
            &["/incoming".to_string()]
        ));
        assert!(!missing_manifest_is_unsafe(&undo, &["/other".to_string()]));

        let mut ordinary = UndoCollection::default();
        ordinary.append("/archive/book.cbz", "/library/book.cbz", "Move");
        assert!(!missing_manifest_is_unsafe(
            &ordinary,
            &["/incoming".to_string()]
        ));
    }
}
