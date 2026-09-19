//! The gauge scheduler: keeps the per-list counters
//! (`ListItemBase.book_count/new_book_count/unread_book_count`) fresh
//! and pushes them into the navigator rows.
//!
//! Port shape of the C# machinery (`ComicListItem.CommitCache`,
//! `ComicLibrary.InvalidateComicListCaches`, the browser's
//! `queryCacheTimer`): book and list mutations bump an epoch and
//! rebuild a leaf-first result set. A debounced timer (100 ms, the C#
//! instant-mode commit interval) starts one worker pass over a database
//! snapshot. Folders combine their children's sets
//! (`ComicListItemFolder.OnCacheMatch`). Only smart and ID-list leaves
//! run the matcher pipeline. The GTK pump applies current results to the
//! session database and updates each row label in place.

use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use gtk4::glib;

use cr_core::database::list_items::ComicListItem;
use cr_core::xml::scalar::{CrDateTime, CrGuid};

use crate::library;

type RowHook = Box<dyn Fn(&CrGuid)>;

/// The debounce before a burst of invalidations starts the refresh
/// (the C# instant-mode `queryCacheTimer` interval, 100 ms).
const DEBOUNCE_MS: u64 = 100;
/// The spacing between per-node refresh ticks.
const TICK_MS: u64 = 50;

thread_local! {
    static EPOCH: Cell<u64> = const { Cell::new(0) };
    static PENDING: Cell<usize> = const { Cell::new(0) };
    /// The queue must rebuild from the model before the next tick.
    /// invalidate() runs INSIDE mutation fns that hold the session
    /// borrow, so it only sets this flag — the rebuild happens on the
    /// timer, where no borrow is held.
    static STALE: Cell<bool> = const { Cell::new(false) };
    /// Completed refresh runs (the probe's drain gate: a mutation's
    /// counters are visible once a pass started AFTER the mutation
    /// completes).
    static PASSES: Cell<u64> = const { Cell::new(0) };
    /// The one pending timer (the debounce or the next tick).
    static SOURCE: RefCell<Option<glib::SourceId>> = const { RefCell::new(None) };
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
    static CANCEL: RefCell<Option<Arc<AtomicBool>>> = const { RefCell::new(None) };
    /// The row applier, set by the shell (a Weak-held state handle).
    static ROW_HOOK: RefCell<Option<RowHook>> = RefCell::new(None);
}

/// Installs the row applier: called with a node id after its counters
/// land, it reads the fresh base and updates the navigator row.
pub fn set_row_hook(hook: Option<RowHook>) {
    ROW_HOOK.with(|h| *h.borrow_mut() = hook);
}

/// The nodes still waiting for a refresh (the probe's drain gate).
pub fn pending() -> usize {
    PENDING.with(Cell::get)
}

/// Completed refresh runs (the probe's drain gate).
pub fn passes() -> u64 {
    PASSES.with(Cell::get)
}

/// Marks the gauges stale and (re)starts the refresh loop. Cheap to
/// call per mutation, and SAFE while the session borrow is held: it
/// only bumps the epoch and sets the rebuild flag — the model walk
/// defers to the timer (the C# invalidation is equally decoupled from
/// the commit).
#[track_caller]
pub fn invalidate() {
    let caller = std::panic::Location::caller();
    crate::trace::trace(format!(
        "gauges: invalidate caller={}:{}",
        caller.file(),
        caller.line()
    ));
    EPOCH.with(|e| e.set(e.get().wrapping_add(1)));
    STALE.with(|s| s.set(true));
    CANCEL.with(|slot| {
        if let Some(cancel) = slot.borrow().as_ref() {
            cancel.store(true, Ordering::Relaxed);
        }
    });
    schedule(DEBOUNCE_MS);
}

fn node_count(items: &[ComicListItem]) -> usize {
    items
        .iter()
        .map(|item| {
            1 + match item {
                ComicListItem::Folder(folder) => node_count(&folder.items),
                _ => 0,
            }
        })
        .sum()
}

/// Starts the one pending timer (no double schedules).
fn schedule(delay_ms: u64) {
    if SOURCE.with(|s| s.borrow().is_some()) {
        return;
    }
    let source = glib::timeout_add_local(std::time::Duration::from_millis(delay_ms), || {
        SOURCE.with(|s| *s.borrow_mut() = None);
        step();
        glib::ControlFlow::Break
    });
    SOURCE.with(|s| *s.borrow_mut() = Some(source));
}

/// Starts one worker pass over a database snapshot.
fn step() {
    if ACTIVE.with(Cell::get) || !STALE.with(|stale| stale.replace(false)) {
        return;
    }
    let epoch = EPOCH.with(Cell::get);
    let database = library::session().borrow().database().clone();
    PENDING.with(|pending| pending.set(node_count(&database.comic_lists)));
    let now = CrDateTime::now();
    let recent = cr_core::settings::EngineConfiguration::global().is_recent_in_days;
    let cancel = Arc::new(AtomicBool::new(false));
    CANCEL.with(|slot| *slot.borrow_mut() = Some(Arc::clone(&cancel)));
    ACTIVE.with(|active| active.set(true));
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::Builder::new()
        .name("Library Gauges".into())
        .spawn(move || {
            let result = build_pass(&database, &now, recent, &cancel);
            let _ = tx.send((epoch, now, result));
        })
        .expect("spawn library gauge worker");
    glib::timeout_add_local(
        std::time::Duration::from_millis(TICK_MS),
        move || match rx.try_recv() {
            Ok((completed, now, result)) => {
                ACTIVE.with(|active| active.set(false));
                CANCEL.with(|slot| *slot.borrow_mut() = None);
                let current = EPOCH.with(Cell::get);
                if completed == current {
                    if let Some(result) = result {
                        apply_pass(now, result);
                    }
                }
                if STALE.with(Cell::get) || completed != current {
                    schedule(DEBOUNCE_MS);
                }
                glib::ControlFlow::Break
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => glib::ControlFlow::Continue,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                ACTIVE.with(|active| active.set(false));
                CANCEL.with(|slot| *slot.borrow_mut() = None);
                PENDING.with(|pending| pending.set(0));
                if STALE.with(Cell::get) {
                    schedule(DEBOUNCE_MS);
                }
                glib::ControlFlow::Break
            }
        },
    );
}

type GaugeResult = (CrGuid, cr_engine::gauges::Gauges);

fn build_pass(
    database: &cr_core::database::comic_database::ComicDatabase,
    now: &CrDateTime,
    recent: i32,
    cancel: &AtomicBool,
) -> Option<Vec<GaugeResult>> {
    fn build_item(
        item: &ComicListItem,
        database: &cr_core::database::comic_database::ComicDatabase,
        now: &CrDateTime,
        recent: i32,
        cancel: &AtomicBool,
        result: &mut Vec<GaugeResult>,
    ) -> Option<HashSet<CrGuid>> {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let set = match item {
            ComicListItem::Folder(folder) => {
                let mut children = Vec::with_capacity(folder.items.len());
                for child in &folder.items {
                    children.push(build_item(child, database, now, recent, cancel, result)?);
                }
                cr_engine::gauges::combine_folder_sets(folder.combine_mode, &children)
            }
            _ => cr_engine::gauges::list_book_ids(item, database),
        };
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let gauges = cr_engine::gauges::gauge_counts(
            database.books.iter().filter(|book| set.contains(&book.id)),
            now,
            recent,
        );
        result.push((item.base().id, gauges));
        Some(set)
    }

    let mut result = Vec::new();
    for item in &database.comic_lists {
        build_item(item, database, now, recent, cancel, &mut result)?;
    }
    Some(result)
}

fn apply_pass(now: CrDateTime, result: Vec<GaugeResult>) {
    for (id, gauges) in result {
        let stored = library::store_list_gauges(&id, gauges, now);
        crate::trace::trace(format!(
            "gauges: refresh id={id} total={} new={} unread={} stored={stored}",
            gauges.total, gauges.new, gauges.unread
        ));
        ROW_HOOK.with(|hook| {
            if let Some(apply) = hook.borrow().as_ref() {
                apply(&id);
            }
        });
        PENDING.with(|pending| pending.set(pending.get().saturating_sub(1)));
    }
    PENDING.with(|pending| pending.set(0));
    PASSES.with(|passes| passes.set(passes.get() + 1));
    if STALE.with(Cell::get) {
        schedule(DEBOUNCE_MS);
    }
}

#[cfg(test)]
mod worker_tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn worker_pass_returns_children_before_their_folder() {
        let database = cr_core::database::load(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../tests/realworld/ComicDb.xml"
            )
            .as_ref(),
        )
        .unwrap();
        let result =
            build_pass(&database, &CrDateTime::now(), 14, &AtomicBool::new(false)).unwrap();
        let positions: HashMap<CrGuid, usize> = result
            .iter()
            .enumerate()
            .map(|(index, (id, _))| (*id, index))
            .collect();
        for item in &database.comic_lists {
            if let ComicListItem::Folder(folder) = item {
                for child in &folder.items {
                    assert!(positions[&child.base().id] < positions[&folder.base.id]);
                }
            }
        }
    }

    #[test]
    fn cancelled_worker_pass_returns_no_partial_results() {
        let database = cr_core::database::load(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../tests/realworld/ComicDb.xml"
            )
            .as_ref(),
        )
        .unwrap();
        assert!(build_pass(&database, &CrDateTime::now(), 14, &AtomicBool::new(true)).is_none());
    }
}
