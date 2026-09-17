//! The engine-side protocol (the `OrganizeUi` seam) and the run
//! entry points.
//!
//! The UI (cr-ui) drives the mover on a worker thread. Requests that
//! need a person (duplicate conflicts, multi-value selections) flow
//! engine→UI as blocking calls on the worker thread; the UI pumps
//! them onto the main thread over an mpsc pair, the `ScrapeUi`
//! pattern from Phase 12. File work happens in the engine; the
//! session mutations come back as [`Apply`] items that the main
//! thread applies through `library::apply_edited` /
//! `insert_new_book` / `remove_book`.

use cr_core::model::comic_book::ComicBook;

use crate::template::{MultiValueAnswer, MultiValueAsk, MultiValueAsker};

/// One log line (`Logger.Add`): profile, action, path, message.
#[derive(Clone, Debug, PartialEq)]
pub struct LogEntry {
    pub profile: String,
    pub action: String,
    pub path: String,
    pub message: String,
}

/// The duplicate-dialog answer (`DuplicateResult`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DuplicateAction {
    Cancel,
    Rename,
    Overwrite,
}

#[derive(Clone, Copy, Debug)]
pub struct DuplicateAnswer {
    pub action: DuplicateAction,
    pub always: bool,
}

/// One side of the duplicate dialog (`SetupFields` in
/// loduplicate.py): a library book's display fields or the bare-file
/// "Comic not in Library" pane.
#[derive(Clone, Debug)]
pub struct DuplicateBookInfo {
    pub in_library: bool,
    pub series: String,
    pub volume: i32,
    pub number: String,
    pub page_count: i32,
    pub file_size_text: String,
    pub published_text: String,
    pub added_text: String,
    pub path: String,
    pub scan_info: String,
    /// Thumbnail-size JPEG bytes for the cover pane.
    pub cover: Option<Vec<u8>>,
}

/// What the UI is asked when a destination exists.
#[derive(Clone, Debug)]
pub struct DuplicateAsk {
    /// The profile mode (`Move`/`Copy`/`Simulate`) — the dialog text
    /// differs for copy and simulate.
    pub mode: String,
    pub rename_filename: String,
    pub held_count: usize,
    pub new_book: Option<DuplicateBookInfo>,
    pub old_book: Option<DuplicateBookInfo>,
}

/// The session mutations the main thread applies after a run.
#[derive(Clone, Debug)]
pub enum Apply {
    /// `library::apply_edited` — a moved/renamed/re-read book.
    Update(ComicBook),
    /// `library::insert_new_book` — the copy-mode copy.
    Insert(ComicBook),
    /// Insert a moved book without changing its ID or metadata.
    Adopt(ComicBook),
    /// `library::remove_book` — the replaced book of an overwrite.
    Remove(cr_core::xml::scalar::CrGuid),
}

/// Cover reads the engine needs (the UI supplies the closures; they
/// run on the worker thread and touch no session state).
pub trait CoverSource {
    /// The image saved for a fileless book (the C#
    /// `GetComicThumbnail(book, 0)`).
    fn fileless_cover(&self, book: &ComicBook) -> Option<cr_image::Image>;

    /// Thumbnail-size JPEG bytes for a duplicate-dialog cover pane.
    fn duplicate_cover(&self, book: &ComicBook) -> Option<Vec<u8>>;
}

/// The UI seam for a run (the `ScrapeUi` shape).
pub trait OrganizeUi: MultiValueAsker {
    /// A duplicate destination exists.
    fn ask_duplicate(&mut self, ask: DuplicateAsk) -> DuplicateAnswer;
    /// One log line.
    fn log(&mut self, entry: LogEntry);
    /// Progress: `done` of `total` planned operations.
    fn progress(&mut self, done: usize, total: usize);
}

/// Notifications around destructive organizer filesystem effects.
pub trait FilesystemEffects: Send + Sync {
    fn before_rename(&self, _source: &str, _destination: &str) -> std::io::Result<()> {
        Ok(())
    }

    fn after_rename(
        &self,
        _source: &str,
        _destination: &str,
        _succeeded: bool,
    ) -> std::io::Result<()> {
        Ok(())
    }

    fn before_delete(&self, _path: &str) -> std::io::Result<()> {
        Ok(())
    }

    fn after_delete(&self, _path: &str, _succeeded: bool) -> std::io::Result<()> {
        Ok(())
    }
}

pub use crate::mover::{
    adoption_manifest_path, missing_manifest_is_unsafe, run_undo, AdoptionManifest,
    AdoptionManifestEntry, MoveLanding, OrganizeReport, RunContext, UndoCollection, UndoEntry,
    UndoReport,
};

/// Runs the organizer (`WorkerForm`'s worker body). The profiles run
/// in order; returns the report and the session mutations.
pub fn organize(ctx: RunContext, ui: &mut dyn OrganizeUi) -> OrganizeReport {
    crate::mover::run(ctx, ui)
}

/// Runs the undo pass (`WorkerFormUndo`).
pub fn undo(
    ctx: RunContext,
    collection: &UndoCollection,
    profiles: &std::collections::HashMap<String, crate::profile::Profile>,
    ui: &mut dyn OrganizeUi,
) -> UndoReport {
    crate::mover::run_undo(ctx, collection, profiles, ui)
}

// Re-exports the template ask types so cr-ui can name them from one
// place.
pub use crate::template::{
    MultiValueAnswer as UiMultiValueAnswer, MultiValueAsk as UiMultiValueAsk,
};

impl MultiValueAsker for () {
    fn ask_multi_value(&mut self, _ask: MultiValueAsk) -> MultiValueAnswer {
        MultiValueAnswer::default()
    }
}
