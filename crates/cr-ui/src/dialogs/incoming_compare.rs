//! Side-by-side comparison for selected Incoming books and their duplicates.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;

use cr_core::model::comic_book::ComicBook;
use cr_engine::duplicates::DuplicateRules;
use cr_engine::image_pool::{front_cover_thumbnail_key, ImagePool};
use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MatchSource {
    Incoming,
    Library,
}

impl MatchSource {
    fn label(self) -> &'static str {
        match self {
            Self::Incoming => "Incoming duplicate",
            Self::Library => "Library duplicate",
        }
    }
}

#[derive(Clone, Debug)]
pub struct DuplicateMatch {
    pub source: MatchSource,
    pub book: ComicBook,
}

#[derive(Clone, Debug)]
pub struct BookComparison {
    pub selected: ComicBook,
    pub matches: Vec<DuplicateMatch>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompareAction {
    ReplaceLibraryCopy,
    DeleteSelectedIncomingCopy,
    DeleteMatchingIncomingCopy,
}

pub type ActionExecutor =
    Rc<dyn Fn(CompareAction, CrGuidPair, Box<dyn FnOnce(Result<(), String>)>)>;

#[derive(Clone, Copy, Debug)]
pub struct CrGuidPair {
    pub selected: cr_core::xml::scalar::CrGuid,
    pub matched: cr_core::xml::scalar::CrGuid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeepSide {
    Left,
    Right,
}

/// One accepted Keep action that waits for serial execution.
struct QueuedAction {
    selected: ComicBook,
    duplicate: DuplicateMatch,
    action: CompareAction,
    ids: CrGuidPair,
}

/// The serial action queue. The transaction layer permits one active
/// operation, so the dialog starts one queued action at a time and
/// continues review while an action runs in the background.
#[derive(Default)]
struct ActionQueue {
    pending: std::collections::VecDeque<QueuedAction>,
    running: bool,
    completed: usize,
    failures: Vec<String>,
}

pub fn keep_action(source: MatchSource, side: KeepSide) -> CompareAction {
    match (source, side) {
        (MatchSource::Library, KeepSide::Left) => CompareAction::ReplaceLibraryCopy,
        (MatchSource::Library, KeepSide::Right) | (MatchSource::Incoming, KeepSide::Right) => {
            CompareAction::DeleteSelectedIncomingCopy
        }
        (MatchSource::Incoming, KeepSide::Left) => CompareAction::DeleteMatchingIncomingCopy,
    }
}

pub fn revalidate_pair(
    selected: &ComicBook,
    duplicate: &DuplicateMatch,
    incoming_books: &[ComicBook],
    library_books: &[ComicBook],
) -> Result<(), String> {
    let live_selected = incoming_books
        .iter()
        .find(|book| book.id == selected.id)
        .ok_or_else(|| "The selected Incoming copy is no longer available.".to_string())?;
    let source_books = match duplicate.source {
        MatchSource::Incoming => incoming_books,
        MatchSource::Library => library_books,
    };
    let live_duplicate = source_books
        .iter()
        .find(|book| book.id == duplicate.book.id)
        .ok_or_else(|| "The matching copy is no longer available.".to_string())?;
    if live_selected.file_path != selected.file_path
        || live_duplicate.file_path != duplicate.book.file_path
    {
        return Err("A file path changed. Review the current copies before you continue.".into());
    }
    let pair = [live_selected, live_duplicate];
    if cr_engine::matcher::eval::grouped_duplicate_indexes(&pair).is_empty() {
        return Err("These copies are no longer duplicates.".into());
    }
    Ok(())
}

pub fn recommended_action(
    selected: &ComicBook,
    duplicate: &DuplicateMatch,
    rules: &DuplicateRules,
) -> Option<CompareAction> {
    let members = [selected, &duplicate.book];
    let worst = cr_engine::duplicates::worst_duplicate_ids(&members, rules);
    if worst.len() != 1 {
        return None;
    }
    if worst[0] == selected.id {
        Some(CompareAction::DeleteSelectedIncomingCopy)
    } else if worst[0] == duplicate.book.id {
        Some(match duplicate.source {
            MatchSource::Incoming => CompareAction::DeleteMatchingIncomingCopy,
            MatchSource::Library => CompareAction::ReplaceLibraryCopy,
        })
    } else {
        None
    }
}

/// Builds the stable comparison sequence used by the dialog.
pub fn build_comparisons(
    selected: &[ComicBook],
    incoming_books: &[ComicBook],
    library_books: &[ComicBook],
) -> Vec<BookComparison> {
    let all: Vec<&ComicBook> = incoming_books.iter().chain(library_books).collect();
    let groups = cr_engine::matcher::eval::grouped_duplicate_indexes(&all);
    let incoming_count = incoming_books.len();
    selected
        .iter()
        .map(|selected| {
            let group = incoming_books
                .iter()
                .position(|candidate| candidate.id == selected.id)
                .and_then(|index| groups.iter().find(|group| group.contains(&index)));
            let mut matches = Vec::new();
            if let Some(group) = group {
                for index in group {
                    if *index < incoming_count {
                        if let Some(book) = incoming_books
                            .get(*index)
                            .filter(|candidate| candidate.id != selected.id)
                        {
                            matches.push(DuplicateMatch {
                                source: MatchSource::Incoming,
                                book: book.clone(),
                            });
                        }
                    } else if let Some(book) = library_books.get(*index - incoming_count) {
                        matches.push(DuplicateMatch {
                            source: MatchSource::Library,
                            book: book.clone(),
                        });
                    }
                }
            }
            BookComparison {
                selected: selected.clone(),
                matches,
            }
        })
        .collect()
}

struct Pane {
    container: gtk4::Box,
    heading: gtk4::Label,
    cover_stack: gtk4::Stack,
    picture: gtk4::Picture,
    cover_status: gtk4::Label,
    details: gtk4::Label,
    keep: gtk4::Button,
}

struct DialogState {
    comparisons: RefCell<Vec<BookComparison>>,
    book_index: Cell<usize>,
    match_indexes: RefCell<Vec<usize>>,
    generation: Cell<u64>,
    pool: Arc<ImagePool>,
    left: Pane,
    right: Pane,
    book_position: gtk4::Label,
    match_position: gtk4::Label,
    previous_book: gtk4::Button,
    next_book: gtk4::Button,
    previous_match: gtk4::Button,
    next_match: gtk4::Button,
    select_worst: gtk4::Button,
    status: gtk4::Label,
    rules: DuplicateRules,
    executor: ActionExecutor,
    queue: RefCell<ActionQueue>,
    window: glib::WeakRef<gtk4::Window>,
}

#[derive(Clone, Copy)]
enum CoverSide {
    Left,
    Right,
}

struct CoverResult {
    generation: u64,
    side: CoverSide,
    bytes: Option<Vec<u8>>,
}

pub fn show(
    parent: &impl IsA<gtk4::Window>,
    pool: Arc<ImagePool>,
    comparisons: Vec<BookComparison>,
    rules: DuplicateRules,
    executor: ActionExecutor,
) {
    if comparisons.is_empty() {
        return;
    }
    let window = gtk4::Window::builder()
        .title("Compare Incoming")
        .transient_for(parent)
        .modal(true)
        .default_width(1100)
        .default_height(800)
        .build();

    let previous_book = gtk4::Button::with_label("Previous Book");
    let next_book = gtk4::Button::with_label("Next Book");
    let book_position = gtk4::Label::new(None);
    let book_nav = navigation_row(&previous_book, &book_position, &next_book);

    let left = comparison_pane("Selected Incoming book");
    let right = comparison_pane("Matching duplicate");
    let grid = gtk4::Grid::builder()
        .column_spacing(24)
        .column_homogeneous(true)
        .hexpand(true)
        .build();
    grid.attach(pane_widget(&left), 0, 0, 1, 1);
    grid.attach(pane_widget(&right), 1, 0, 1, 1);

    let previous_match = gtk4::Button::with_label("Previous Match");
    let next_match = gtk4::Button::with_label("Next Match");
    let match_position = gtk4::Label::new(None);
    let match_nav = navigation_row(&previous_match, &match_position, &next_match);

    let select_worst = gtk4::Button::with_label("Select Worst Duplicates");
    select_worst.set_halign(gtk4::Align::Center);
    let status = gtk4::Label::new(Some("Ready"));
    status.set_wrap(true);
    status.set_selectable(true);

    let close = gtk4::Button::with_label("Close");
    close.set_halign(gtk4::Align::End);
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.append(&book_nav);
    content.append(&grid);
    content.append(&match_nav);
    content.append(&select_worst);
    content.append(&status);
    content.append(&close);
    let scroll = gtk4::ScrolledWindow::builder()
        .hscrollbar_policy(gtk4::PolicyType::Automatic)
        .vscrollbar_policy(gtk4::PolicyType::Automatic)
        .child(&content)
        .build();
    window.set_child(Some(&scroll));

    let (cover_tx, cover_rx) = std::sync::mpsc::channel();
    let state = Rc::new(DialogState {
        match_indexes: RefCell::new(vec![0; comparisons.len()]),
        comparisons: RefCell::new(comparisons),
        book_index: Cell::new(0),
        generation: Cell::new(0),
        pool,
        left,
        right,
        book_position,
        match_position,
        previous_book,
        next_book,
        previous_match,
        next_match,
        select_worst,
        status,
        rules,
        executor,
        queue: RefCell::new(ActionQueue::default()),
        window: window.downgrade(),
    });
    update_dialog(&state, &cover_tx);

    connect_navigation(&state, &cover_tx);
    connect_actions(&state, &cover_tx);
    {
        let window = window.clone();
        close.connect_clicked(move |_| window.close());
    }
    let state = Rc::clone(&state);
    let weak_window = window.downgrade();
    glib::timeout_add_local(std::time::Duration::from_millis(30), move || {
        let Some(window) = weak_window.upgrade() else {
            return glib::ControlFlow::Break;
        };
        if !window.is_visible() {
            return glib::ControlFlow::Break;
        }
        while let Ok(result) = cover_rx.try_recv() {
            if result.generation != state.generation.get() {
                continue;
            }
            let pane = match result.side {
                CoverSide::Left => &state.left,
                CoverSide::Right => &state.right,
            };
            if let Some(texture) = result.bytes.as_deref().and_then(texture_from_thumb_blob) {
                pane.picture.set_paintable(Some(&texture));
                pane.cover_stack.set_visible_child(&pane.picture);
            } else {
                pane.cover_status.set_text("No cover available");
                pane.cover_stack.set_visible_child(&pane.cover_status);
            }
        }
        glib::ControlFlow::Continue
    });
    window.present();
}

fn navigation_row(
    previous: &gtk4::Button,
    position: &gtk4::Label,
    next: &gtk4::Button,
) -> gtk4::Box {
    let row = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    row.set_halign(gtk4::Align::Center);
    row.append(previous);
    row.append(position);
    row.append(next);
    row
}

fn comparison_pane(heading: &str) -> Pane {
    let heading = gtk4::Label::new(Some(heading));
    heading.add_css_class("title-3");
    heading.set_halign(gtk4::Align::Start);
    let picture = gtk4::Picture::new();
    picture.set_content_fit(gtk4::ContentFit::Contain);
    picture.set_size_request(260, 360);
    let cover_status = gtk4::Label::new(Some("Loading cover..."));
    cover_status.set_size_request(260, 360);
    let cover_stack = gtk4::Stack::new();
    cover_stack.add_child(&picture);
    cover_stack.add_child(&cover_status);
    cover_stack.set_visible_child(&cover_status);
    let details = gtk4::Label::new(None);
    details.set_halign(gtk4::Align::Start);
    details.set_valign(gtk4::Align::Start);
    details.set_xalign(0.0);
    details.set_wrap(true);
    details.set_selectable(true);
    let keep = gtk4::Button::with_label("Keep This Copy");
    keep.set_hexpand(true);
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    container.set_hexpand(true);
    container.add_css_class("compare-pane");
    container.append(&heading);
    container.append(&cover_stack);
    container.append(&details);
    container.append(&keep);
    Pane {
        container,
        heading,
        cover_stack,
        picture,
        cover_status,
        details,
        keep,
    }
}

fn pane_widget(pane: &Pane) -> &gtk4::Box {
    &pane.container
}

fn connect_navigation(state: &Rc<DialogState>, cover_tx: &std::sync::mpsc::Sender<CoverResult>) {
    {
        let button = state.previous_book.clone();
        let state = Rc::downgrade(state);
        let cover_tx = cover_tx.clone();
        button.connect_clicked(move |_| {
            let Some(state) = state.upgrade() else {
                return;
            };
            state
                .book_index
                .set(state.book_index.get().saturating_sub(1));
            update_dialog(&state, &cover_tx);
        });
    }
    {
        let button = state.next_book.clone();
        let state = Rc::downgrade(state);
        let cover_tx = cover_tx.clone();
        button.connect_clicked(move |_| {
            let Some(state) = state.upgrade() else {
                return;
            };
            let last = state.comparisons.borrow().len().saturating_sub(1);
            state.book_index.set((state.book_index.get() + 1).min(last));
            update_dialog(&state, &cover_tx);
        });
    }
    {
        let button = state.previous_match.clone();
        let state = Rc::downgrade(state);
        let cover_tx = cover_tx.clone();
        button.connect_clicked(move |_| {
            let Some(state) = state.upgrade() else {
                return;
            };
            let book_index = state.book_index.get();
            let mut indexes = state.match_indexes.borrow_mut();
            indexes[book_index] = indexes[book_index].saturating_sub(1);
            drop(indexes);
            update_dialog(&state, &cover_tx);
        });
    }
    {
        let button = state.next_match.clone();
        let state = Rc::downgrade(state);
        let cover_tx = cover_tx.clone();
        button.connect_clicked(move |_| {
            let Some(state) = state.upgrade() else {
                return;
            };
            let book_index = state.book_index.get();
            let last = state.comparisons.borrow()[book_index]
                .matches
                .len()
                .saturating_sub(1);
            let mut indexes = state.match_indexes.borrow_mut();
            indexes[book_index] = (indexes[book_index] + 1).min(last);
            drop(indexes);
            update_dialog(&state, &cover_tx);
        });
    }
}

fn connect_actions(state: &Rc<DialogState>, cover_tx: &std::sync::mpsc::Sender<CoverResult>) {
    {
        let button = state.select_worst.clone();
        let state = Rc::downgrade(state);
        button.connect_clicked(move |_| {
            let Some(state) = state.upgrade() else { return };
            // The recommendation already shows on the panes and the
            // status line. The button re-applies it and re-focuses
            // the recommended Keep button.
            if let Some(side) = apply_recommendation(&state) {
                keep_button(&state, side).grab_focus();
            }
            update_status(&state);
        });
    }
    for (button, side) in [
        (state.left.keep.clone(), KeepSide::Left),
        (state.right.keep.clone(), KeepSide::Right),
    ] {
        let state = Rc::downgrade(state);
        let cover_tx = cover_tx.clone();
        button.connect_clicked(move |_| {
            let Some(state) = state.upgrade() else { return };
            enqueue_keep_action(&state, side, &cover_tx);
        });
    }
}

fn recommended_keep_side(action: CompareAction) -> KeepSide {
    match action {
        CompareAction::ReplaceLibraryCopy | CompareAction::DeleteMatchingIncomingCopy => {
            KeepSide::Left
        }
        CompareAction::DeleteSelectedIncomingCopy => KeepSide::Right,
    }
}

fn keep_button(state: &DialogState, side: KeepSide) -> &gtk4::Button {
    match side {
        KeepSide::Left => &state.left.keep,
        KeepSide::Right => &state.right.keep,
    }
}

fn clear_recommendation(state: &DialogState) {
    state.left.keep.remove_css_class("suggested-action");
    state.right.keep.remove_css_class("suggested-action");
    state
        .left
        .container
        .remove_css_class("compare-pane-preferred");
    state.left.container.remove_css_class("compare-pane-worse");
    state
        .right
        .container
        .remove_css_class("compare-pane-preferred");
    state.right.container.remove_css_class("compare-pane-worse");
}

/// Ranks the displayed pair with the duplicate rules and marks the
/// preferred pane green and the worse pane red. A tie or a missing
/// match leaves both panes plain. Returns the recommended keep side,
/// when the rules choose one.
fn apply_recommendation(state: &DialogState) -> Option<KeepSide> {
    clear_recommendation(state);
    let book_index = state.book_index.get();
    let match_index = state.match_indexes.borrow()[book_index];
    let comparisons = state.comparisons.borrow();
    let duplicate = comparisons[book_index].matches.get(match_index)?;
    let action = recommended_action(&comparisons[book_index].selected, duplicate, &state.rules)?;
    let side = recommended_keep_side(action);
    let (preferred, worse) = match side {
        KeepSide::Left => (&state.left, &state.right),
        KeepSide::Right => (&state.right, &state.left),
    };
    preferred.container.add_css_class("compare-pane-preferred");
    worse.container.add_css_class("compare-pane-worse");
    keep_button(state, side).add_css_class("suggested-action");
    Some(side)
}

/// Accepts a Keep action for the displayed pair, then moves to the
/// next selected book at once. The action runs in the background
/// through the serial queue. The dialog does not wait.
fn enqueue_keep_action(
    state: &Rc<DialogState>,
    side: KeepSide,
    cover_tx: &std::sync::mpsc::Sender<CoverResult>,
) {
    let book_index = state.book_index.get();
    let match_index = state.match_indexes.borrow()[book_index];
    let comparisons = state.comparisons.borrow();
    let Some(duplicate) = comparisons[book_index].matches.get(match_index).cloned() else {
        return;
    };
    let selected = comparisons[book_index].selected.clone();
    drop(comparisons);
    let action = keep_action(duplicate.source, side);
    let ids = CrGuidPair {
        selected: selected.id,
        matched: duplicate.book.id,
    };
    crate::trace::trace(format!(
        "compare Keep queued side={side:?} match_source={:?} action={action:?} selected_id={} matched_id={}",
        duplicate.source,
        selected.id.to_d_string(),
        duplicate.book.id.to_d_string(),
    ));
    // Mark this selected book as accepted so it leaves the review
    // sequence at once, and move to the next selected book.
    mark_book_accepted(state, book_index);
    state.queue.borrow_mut().pending.push_back(QueuedAction {
        selected,
        duplicate,
        action,
        ids,
    });
    process_queue(state, cover_tx);
    refresh_after_model_change(state, cover_tx);
}

/// Removes the accepted selected book from the review sequence and
/// advances to the next book in browser order.
fn mark_book_accepted(state: &DialogState, book_index: usize) {
    let removed_id = {
        let comparisons = state.comparisons.borrow();
        comparisons[book_index].selected.id
    };
    remove_selected(state, removed_id);
}

/// Removes every comparison whose selected book matches `id`, keeps
/// the match indexes aligned, and clamps the visible index.
fn remove_selected(state: &DialogState, id: cr_core::xml::scalar::CrGuid) {
    let mut comparisons = state.comparisons.borrow_mut();
    let mut book_index = state.book_index.get();
    let mut indexes = state.match_indexes.borrow_mut();
    drop_selected_book(&mut comparisons, &mut indexes, &mut book_index, id);
    drop(indexes);
    drop(comparisons);
    state.book_index.set(book_index);
}

/// Pure model transform for [`remove_selected`]: drops every
/// comparison whose selected book is `id`, keeps the visible index on
/// the same surviving book, and rebuilds the match-index vector.
fn drop_selected_book(
    comparisons: &mut Vec<BookComparison>,
    indexes: &mut Vec<usize>,
    book_index: &mut usize,
    id: cr_core::xml::scalar::CrGuid,
) {
    let removed_before = comparisons
        .iter()
        .take(*book_index)
        .filter(|c| c.selected.id == id)
        .count();
    comparisons.retain(|comparison| comparison.selected.id != id);
    let new_len = comparisons.len();
    *book_index = book_index
        .saturating_sub(removed_before)
        .min(new_len.saturating_sub(1));
    indexes.clear();
    indexes.resize(new_len, 0);
}

/// Starts the next queued action when none runs. Runs one action at a
/// time because the transaction layer permits one active operation.
fn process_queue(state: &Rc<DialogState>, cover_tx: &std::sync::mpsc::Sender<CoverResult>) {
    {
        let queue = state.queue.borrow();
        if queue.running || queue.pending.is_empty() {
            return;
        }
    }
    let job = state.queue.borrow_mut().pending.pop_front();
    let Some(job) = job else { return };
    state.queue.borrow_mut().running = true;
    // A strong clone keeps the queue alive until every accepted action
    // finishes, even after the window closes.
    let owner = Rc::clone(state);
    let cover_tx = cover_tx.clone();
    if crate::library::is_scanning() {
        crate::library::abort_scan();
        update_status(&owner);
        let cover_tx2 = cover_tx.clone();
        glib::timeout_add_local(std::time::Duration::from_millis(50), move || {
            if crate::library::is_scanning() {
                return glib::ControlFlow::Continue;
            }
            run_job(&owner, job_clone(&job), &cover_tx2);
            glib::ControlFlow::Break
        });
    } else {
        run_job(&owner, job, &cover_tx);
    }
}

fn job_clone(job: &QueuedAction) -> QueuedAction {
    QueuedAction {
        selected: job.selected.clone(),
        duplicate: job.duplicate.clone(),
        action: job.action,
        ids: job.ids,
    }
}

/// Revalidates the pair, then dispatches the durable action. On
/// completion it records the result and starts the next queued action.
fn run_job(
    owner: &Rc<DialogState>,
    job: QueuedAction,
    cover_tx: &std::sync::mpsc::Sender<CoverResult>,
) {
    let incoming = crate::library::incoming_books_snapshot();
    let library = crate::library::session().borrow().database().books.clone();
    if let Err(error) = revalidate_pair(&job.selected, &job.duplicate, &incoming, &library) {
        finish_job(owner, &job, Err(error), cover_tx);
        return;
    }
    crate::trace::trace(format!(
        "compare action dispatch source={:?} action={:?} selected_path='{}' matched_path='{}'",
        job.duplicate.source, job.action, job.selected.file_path, job.duplicate.book.file_path
    ));
    let owner_weak = Rc::downgrade(owner);
    let strong = Rc::clone(owner);
    let cover_tx = cover_tx.clone();
    let action = job.action;
    let ids = job.ids;
    let job_for_result = job_clone(&job);
    (owner.executor)(
        action,
        ids,
        Box::new(move |result| {
            // Keep the queue alive through completion.
            let _ = &owner_weak;
            finish_job(&strong, &job_for_result, result, &cover_tx);
        }),
    );
}

fn finish_job(
    owner: &Rc<DialogState>,
    job: &QueuedAction,
    result: Result<(), String>,
    cover_tx: &std::sync::mpsc::Sender<CoverResult>,
) {
    {
        let mut queue = owner.queue.borrow_mut();
        queue.running = false;
        match &result {
            Ok(()) => queue.completed += 1,
            Err(error) => queue.failures.push(format!(
                "{}: {error}",
                cr_engine::display_text::caption(&job.selected)
            )),
        }
    }
    if result.is_ok() {
        remove_resolved(owner, job.action, job.ids);
    }
    process_queue(owner, cover_tx);
    refresh_after_model_change(owner, cover_tx);
}

/// Refreshes the view after the model changes. Closes the window only
/// when nothing remains to review and no action is pending.
fn refresh_after_model_change(
    state: &Rc<DialogState>,
    cover_tx: &std::sync::mpsc::Sender<CoverResult>,
) {
    // The queue can outlive the window. Skip every UI touch once the
    // window is gone; the queue still finishes its accepted actions.
    let window_open = state
        .window
        .upgrade()
        .map(|w| w.is_visible())
        .unwrap_or(false);
    if !window_open {
        return;
    }
    let empty_model = state.comparisons.borrow().is_empty();
    let idle = {
        let queue = state.queue.borrow();
        !queue.running && queue.pending.is_empty()
    };
    if empty_model {
        if idle {
            if let Some(window) = state.window.upgrade() {
                window.close();
            }
        } else {
            // Nothing to show yet, but actions still run.
            state
                .status
                .set_text(&format!("Working...{}", batch_suffix(state)));
        }
        return;
    }
    update_dialog(state, cover_tx);
}

/// Removes the resolved records from the comparison model without
/// moving the visible book. The batch flow already advanced the view.
fn remove_resolved(state: &DialogState, action: CompareAction, ids: CrGuidPair) {
    let mut comparisons = state.comparisons.borrow_mut();
    let mut book_index = state.book_index.get();
    let mut indexes = state.match_indexes.borrow_mut();
    drop_resolved_records(&mut comparisons, &mut indexes, &mut book_index, action, ids);
    drop(indexes);
    drop(comparisons);
    state.book_index.set(book_index);
}

/// Pure model transform for [`remove_resolved`]: removes the records
/// the completed `action` resolved, keeps the visible index on the
/// same surviving book when possible, and aligns the match indexes.
fn drop_resolved_records(
    comparisons: &mut Vec<BookComparison>,
    indexes: &mut Vec<usize>,
    book_index: &mut usize,
    action: CompareAction,
    ids: CrGuidPair,
) {
    let visible_id = comparisons.get(*book_index).map(|c| c.selected.id);
    match action {
        CompareAction::ReplaceLibraryCopy | CompareAction::DeleteSelectedIncomingCopy => {
            comparisons.retain(|comparison| comparison.selected.id != ids.selected);
        }
        CompareAction::DeleteMatchingIncomingCopy => {
            for comparison in comparisons.iter_mut() {
                comparison
                    .matches
                    .retain(|duplicate| duplicate.book.id != ids.matched);
            }
            comparisons.retain(|comparison| {
                comparison.selected.id != ids.matched && !comparison.matches.is_empty()
            });
        }
    }
    let new_len = comparisons.len();
    *book_index = visible_id
        .and_then(|id| comparisons.iter().position(|c| c.selected.id == id))
        .unwrap_or_else(|| (*book_index).min(new_len.saturating_sub(1)));
    if indexes.len() != new_len {
        indexes.clear();
        indexes.resize(new_len, 0);
    }
    for index in indexes.iter_mut() {
        if *index >= new_len {
            *index = 0;
        }
    }
}

fn update_dialog(state: &Rc<DialogState>, cover_tx: &std::sync::mpsc::Sender<CoverResult>) {
    let book_index = state.book_index.get();
    let comparisons = state.comparisons.borrow();
    let comparison = &comparisons[book_index];
    let match_index = state.match_indexes.borrow()[book_index];
    let generation = state.generation.get().wrapping_add(1);
    state.generation.set(generation);

    state
        .book_position
        .set_text(&format!("Book {} of {}", book_index + 1, comparisons.len()));
    clear_recommendation(state);
    state.left.heading.set_text("Selected Incoming book");
    set_book(&state.left, &comparison.selected);
    queue_cover(
        state,
        CoverSide::Left,
        &comparison.selected,
        generation,
        cover_tx,
    );

    if let Some(duplicate) = comparison.matches.get(match_index) {
        state.right.heading.set_text(duplicate.source.label());
        set_book(&state.right, &duplicate.book);
        state.match_position.set_text(&format!(
            "Match {} of {}",
            match_index + 1,
            comparison.matches.len()
        ));
        queue_cover(
            state,
            CoverSide::Right,
            &duplicate.book,
            generation,
            cover_tx,
        );
        state.left.keep.set_visible(true);
        state.right.keep.set_visible(true);
    } else {
        state.right.heading.set_text("No matching duplicate");
        state.right.details.set_text("");
        state.right.picture.set_paintable(gdk::Paintable::NONE);
        state.right.cover_status.set_text("No matching duplicate");
        state
            .right
            .cover_stack
            .set_visible_child(&state.right.cover_status);
        state.match_position.set_text("No matches");
        state.left.keep.set_visible(false);
        state.right.keep.set_visible(false);
    }
    drop(comparisons);
    update_dialog_sensitivity(state);
    let has_match = apply_recommendation(state).is_some();
    let _ = has_match;
    update_status(state);
}

/// Shows the batch status line: the recommendation or tie hint for the
/// current pair, plus any pending, completed, or failed counts.
fn update_status(state: &DialogState) {
    let book_index = state.book_index.get();
    let match_index = state.match_indexes.borrow()[book_index];
    let comparisons = state.comparisons.borrow();
    let base = match comparisons.get(book_index).and_then(|c| {
        c.matches
            .get(match_index)
            .map(|d| recommended_action(&c.selected, d, &state.rules))
    }) {
        Some(Some(action)) => match recommended_keep_side(action) {
            KeepSide::Left => "Recommendation: keep the left copy.".to_string(),
            KeepSide::Right => "Recommendation: keep the right copy.".to_string(),
        },
        Some(None) => "The copies tie. Choose the copy to keep.".to_string(),
        None => "No matching duplicate.".to_string(),
    };
    drop(comparisons);
    state
        .status
        .set_text(&format!("{base}{}", batch_suffix(state)));
}

/// The queue progress suffix. Empty when nothing is pending, running,
/// done, or failed.
fn batch_suffix(state: &DialogState) -> String {
    let queue = state.queue.borrow();
    let mut parts = Vec::new();
    if queue.running {
        parts.push("1 running".to_string());
    }
    if !queue.pending.is_empty() {
        parts.push(format!("{} queued", queue.pending.len()));
    }
    if queue.completed > 0 {
        parts.push(format!("{} done", queue.completed));
    }
    if !queue.failures.is_empty() {
        parts.push(format!("{} failed", queue.failures.len()));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("  [{}]", parts.join(", "))
    }
}

fn update_dialog_sensitivity(state: &DialogState) {
    let book_index = state.book_index.get();
    let comparisons = state.comparisons.borrow();
    let Some(comparison) = comparisons.get(book_index) else {
        return;
    };
    let match_index = state.match_indexes.borrow()[book_index];
    state.previous_book.set_sensitive(book_index > 0);
    state
        .next_book
        .set_sensitive(book_index + 1 < comparisons.len());
    state.previous_match.set_sensitive(match_index > 0);
    state
        .next_match
        .set_sensitive(match_index + 1 < comparison.matches.len());
}

fn set_book(pane: &Pane, book: &ComicBook) {
    pane.details.set_text(&book_details(book));
    pane.picture.set_paintable(gdk::Paintable::NONE);
    pane.cover_status.set_text("Loading cover...");
    pane.cover_stack.set_visible_child(&pane.cover_status);
}

fn book_details(book: &ComicBook) -> String {
    let published = match (book.info.year, book.info.month, book.info.day) {
        (year, month, day) if year > 0 && month > 0 && day > 0 => {
            format!("{year:04}-{month:02}-{day:02}")
        }
        (year, month, _) if year > 0 && month > 0 => format!("{year:04}-{month:02}"),
        (year, _, _) if year > 0 => year.to_string(),
        _ => "Unknown".to_string(),
    };
    format!(
        "{}\nSeries: {}\nVolume: {}\nNumber: {}\nPages: {}\nFile size: {}\nPublished: {}\nPath: {}",
        cr_engine::display_text::caption(book),
        book.info.series,
        book.info.volume,
        book.info.number,
        book.info.page_count,
        cr_engine::display_text::file_size_as_text(book.file_size),
        published,
        book.file_path
    )
}

fn queue_cover(
    state: &Rc<DialogState>,
    side: CoverSide,
    book: &ComicBook,
    generation: u64,
    cover_tx: &std::sync::mpsc::Sender<CoverResult>,
) {
    if book.file_path.is_empty() && book.custom_thumbnail_key.is_none() {
        let _ = cover_tx.send(CoverResult {
            generation,
            side,
            bytes: None,
        });
        return;
    }
    let key = front_cover_thumbnail_key(book);
    let pool = Arc::clone(&state.pool);
    let render = Arc::clone(&pool);
    let tx = cover_tx.clone();
    pool.add_thumb_to_queue(key, None, move |key| {
        let _ = tx.send(CoverResult {
            generation,
            side,
            bytes: render.render_thumbnail(key),
        });
    });
}

fn texture_from_thumb_blob(bytes: &[u8]) -> Option<gdk::MemoryTexture> {
    let mut surface = crate::bitmap::surface_from_thumb_blob(bytes)?;
    let width = surface.width();
    let height = surface.height();
    let stride = surface.stride();
    let data = surface.data().ok()?;
    Some(gdk::MemoryTexture::new(
        width,
        height,
        gdk::MemoryFormat::B8g8r8a8Premultiplied,
        &glib::Bytes::from(&*data),
        stride as usize,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use cr_core::xml::scalar::CrGuid;

    fn book(id: u8, series: &str, number: &str, path: &str) -> ComicBook {
        let mut book = ComicBook {
            id: CrGuid::parse(&format!("00000000-0000-0000-0000-{id:012}")).unwrap(),
            file_path: path.into(),
            ..Default::default()
        };
        book.info.series = series.into();
        book.info.number = number.into();
        book.info.year = 2026;
        book
    }

    #[test]
    fn comparison_model_keeps_selection_match_source_and_order() {
        let first = book(1, "Alpha", "1", "/incoming/a.cbz");
        let incoming_match = book(2, "Alpha", "1", "/incoming/b.cbz");
        let library_match = book(3, "Alpha", "1", "/library/a.cbz");
        let second = book(4, "Beta", "1", "/incoming/c.cbz");
        let library_second = book(5, "Beta", "1", "/library/b.cbz");

        let comparisons = build_comparisons(
            &[second.clone(), first.clone()],
            &[first, incoming_match, second],
            &[library_match, library_second],
        );

        assert_eq!(comparisons[0].selected.id, fixed(4));
        assert_eq!(comparisons[0].matches.len(), 1);
        assert_eq!(comparisons[0].matches[0].source, MatchSource::Library);
        assert_eq!(comparisons[1].selected.id, fixed(1));
        assert_eq!(comparisons[1].matches.len(), 2);
        assert_eq!(comparisons[1].matches[0].source, MatchSource::Incoming);
        assert_eq!(comparisons[1].matches[0].book.id, fixed(2));
        assert_eq!(comparisons[1].matches[1].source, MatchSource::Library);
        assert_eq!(comparisons[1].matches[1].book.id, fixed(3));
    }

    #[test]
    fn comparison_model_excludes_self_and_keeps_no_match_selections() {
        let selected = book(1, "Alpha", "1", "/incoming/a.cbz");
        let unmatched = book(2, "Beta", "1", "/incoming/b.cbz");

        let comparisons = build_comparisons(
            &[selected.clone(), unmatched.clone()],
            &[selected, unmatched],
            &[],
        );

        assert_eq!(comparisons.len(), 2);
        assert!(comparisons
            .iter()
            .all(|comparison| comparison.matches.is_empty()));
    }

    #[test]
    fn recommendation_selects_the_worse_copy_and_leaves_a_tie_unselected() {
        let mut selected = book(1, "Alpha", "1", "/incoming/a.cbz");
        selected.file_size = 200;
        selected.info.page_count = 20;
        let mut incoming = book(2, "Alpha", "1", "/incoming/b.cbz");
        incoming.file_size = 100;
        incoming.info.page_count = 10;
        let rules = DuplicateRules {
            cbr_worse_than_cbz: false,
            smaller_file_worse: true,
            fewer_pages_worse: true,
            older_file_worse: false,
        };
        let duplicate = DuplicateMatch {
            source: MatchSource::Incoming,
            book: incoming,
        };
        assert_eq!(
            recommended_action(&selected, &duplicate, &rules),
            Some(CompareAction::DeleteMatchingIncomingCopy)
        );

        let tied = DuplicateMatch {
            source: MatchSource::Library,
            book: selected.clone(),
        };
        assert_eq!(recommended_action(&selected, &tied, &rules), None);
    }

    #[test]
    fn recommendation_replaces_a_worse_library_match() {
        let mut selected = book(1, "Alpha", "1", "/incoming/a.cbz");
        selected.file_size = 200;
        let mut library = book(2, "Alpha", "1", "/library/a.cbz");
        library.file_size = 100;
        let rules = DuplicateRules {
            cbr_worse_than_cbz: false,
            smaller_file_worse: true,
            fewer_pages_worse: false,
            older_file_worse: false,
        };
        let duplicate = DuplicateMatch {
            source: MatchSource::Library,
            book: library,
        };
        assert_eq!(
            recommended_action(&selected, &duplicate, &rules),
            Some(CompareAction::ReplaceLibraryCopy)
        );
    }

    #[test]
    fn keep_buttons_map_to_source_specific_actions() {
        assert_eq!(
            keep_action(MatchSource::Library, KeepSide::Left),
            CompareAction::ReplaceLibraryCopy
        );
        assert_eq!(
            keep_action(MatchSource::Library, KeepSide::Right),
            CompareAction::DeleteSelectedIncomingCopy
        );
        assert_eq!(
            keep_action(MatchSource::Incoming, KeepSide::Left),
            CompareAction::DeleteMatchingIncomingCopy
        );
        assert_eq!(
            keep_action(MatchSource::Incoming, KeepSide::Right),
            CompareAction::DeleteSelectedIncomingCopy
        );
    }

    #[test]
    fn revalidation_checks_source_paths_and_duplicate_relation() {
        let selected = book(1, "Alpha", "1", "/incoming/a.cbz");
        let matched = book(2, "Alpha", "1", "/library/a.cbz");
        let duplicate = DuplicateMatch {
            source: MatchSource::Library,
            book: matched.clone(),
        };
        assert_eq!(
            revalidate_pair(
                &selected,
                &duplicate,
                std::slice::from_ref(&selected),
                std::slice::from_ref(&matched)
            ),
            Ok(())
        );

        let mut changed_path = matched.clone();
        changed_path.file_path = "/library/new.cbz".into();
        assert!(revalidate_pair(
            &selected,
            &duplicate,
            std::slice::from_ref(&selected),
            &[changed_path]
        )
        .is_err());

        let not_duplicate = book(2, "Beta", "1", "/library/a.cbz");
        assert!(revalidate_pair(
            &selected,
            &duplicate,
            std::slice::from_ref(&selected),
            &[not_duplicate]
        )
        .is_err());
    }

    fn fixed(id: u8) -> CrGuid {
        CrGuid::parse(&format!("00000000-0000-0000-0000-{id:012}")).unwrap()
    }

    fn ids(selected: u8, matched: u8) -> CrGuidPair {
        CrGuidPair {
            selected: fixed(selected),
            matched: fixed(matched),
        }
    }

    #[test]
    fn dropping_the_accepted_book_advances_to_the_next_selected_book() {
        // Three selected books, the first visible. Accepting the first
        // removes it and keeps index 0 on what is now the second book.
        let a = book(1, "A", "1", "/incoming/a.cbz");
        let b = book(2, "B", "1", "/incoming/b.cbz");
        let c = book(3, "C", "1", "/incoming/c.cbz");
        let mut comparisons = vec![
            BookComparison {
                selected: a.clone(),
                matches: vec![],
            },
            BookComparison {
                selected: b.clone(),
                matches: vec![],
            },
            BookComparison {
                selected: c.clone(),
                matches: vec![],
            },
        ];
        let mut indexes = vec![0, 0, 0];
        let mut index = 0;
        drop_selected_book(&mut comparisons, &mut indexes, &mut index, fixed(1));
        assert_eq!(comparisons.len(), 2);
        assert_eq!(comparisons[0].selected.id, fixed(2));
        assert_eq!(index, 0);
        assert_eq!(indexes.len(), 2);
    }

    #[test]
    fn dropping_an_earlier_book_keeps_the_visible_book() {
        // The second book is visible. Removing the first keeps the
        // view on the same (now first) book.
        let a = book(1, "A", "1", "/incoming/a.cbz");
        let b = book(2, "B", "1", "/incoming/b.cbz");
        let mut comparisons = vec![
            BookComparison {
                selected: a,
                matches: vec![],
            },
            BookComparison {
                selected: b.clone(),
                matches: vec![],
            },
        ];
        let mut indexes = vec![0, 0];
        let mut index = 1;
        drop_selected_book(&mut comparisons, &mut indexes, &mut index, fixed(1));
        assert_eq!(comparisons.len(), 1);
        assert_eq!(comparisons[0].selected.id, fixed(2));
        assert_eq!(index, 0);
    }

    #[test]
    fn resolving_a_matching_incoming_copy_prunes_other_books() {
        // Book 1 keeps its copy; the matched incoming copy (id 2) is
        // also selected book 2, so book 2 leaves review, and the same
        // copy drops from book 3's matches.
        let one = book(1, "S", "1", "/incoming/1.cbz");
        let two = book(2, "S", "1", "/incoming/2.cbz");
        let three = book(3, "S", "1", "/incoming/3.cbz");
        let mut comparisons = vec![
            BookComparison {
                selected: one.clone(),
                matches: vec![DuplicateMatch {
                    source: MatchSource::Incoming,
                    book: two.clone(),
                }],
            },
            BookComparison {
                selected: two.clone(),
                matches: vec![DuplicateMatch {
                    source: MatchSource::Incoming,
                    book: one.clone(),
                }],
            },
            BookComparison {
                selected: three.clone(),
                matches: vec![DuplicateMatch {
                    source: MatchSource::Incoming,
                    book: two.clone(),
                }],
            },
        ];
        let mut indexes = vec![0, 0, 0];
        let mut index = 0;
        drop_resolved_records(
            &mut comparisons,
            &mut indexes,
            &mut index,
            CompareAction::DeleteMatchingIncomingCopy,
            ids(1, 2),
        );
        // Book 2 (selected == matched) is gone. Book 1 and book 3
        // each lose their only match (id 2) and drop out too.
        assert_eq!(comparisons.len(), 0);
        assert_eq!(index, 0);
    }

    #[test]
    fn recommendation_side_maps_from_action() {
        assert_eq!(
            recommended_keep_side(CompareAction::ReplaceLibraryCopy),
            KeepSide::Left
        );
        assert_eq!(
            recommended_keep_side(CompareAction::DeleteMatchingIncomingCopy),
            KeepSide::Left
        );
        assert_eq!(
            recommended_keep_side(CompareAction::DeleteSelectedIncomingCopy),
            KeepSide::Right
        );
    }
}
