//! The gauge scheduler: keeps the per-list counters
//! (`ListItemBase.book_count/new_book_count/unread_book_count`) fresh
//! and pushes them into the navigator rows.
//!
//! Port shape of the C# machinery (`ComicListItem.CommitCache`,
//! `ComicLibrary.InvalidateComicListCaches`, the browser's
//! `queryCacheTimer`): book and list mutations bump an epoch and
//! rebuild a leaf-first work queue; a debounced timer (100 ms, the C#
//! instant-mode commit interval) then refreshes ONE tree node per
//! tick (50 ms apart), so the GTK main thread never blocks longer
//! than one list evaluation. Folders combine their children's cached
//! sets (`ComicListItemFolder.OnCacheMatch`) — only smart/id-list
//! leaves re-run the matcher pipeline. Results write into the
//! session database (they persist to ComicDb.xml, like the C#) and
//! update the row label in place (the C# repaint-only path — no
//! refill, no selection churn).

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

use gtk4::glib;

use cr_core::database::list_items::ComicListItem;
use cr_core::xml::scalar::{CrDateTime, CrGuid};

use crate::library;

type RowHook = Box<dyn Fn(&CrGuid)>;
type SetCache = HashMap<CrGuid, (u64, HashSet<CrGuid>)>;

/// The debounce before a burst of invalidations starts the refresh
/// (the C# instant-mode `queryCacheTimer` interval, 100 ms).
const DEBOUNCE_MS: u64 = 100;
/// The spacing between per-node refresh ticks.
const TICK_MS: u64 = 50;

thread_local! {
    static EPOCH: Cell<u64> = const { Cell::new(0) };
    /// Per-node cached book-id sets, stamped with the epoch they were
    /// built at. Membership-only: classification reads live fields.
    static SETS: RefCell<SetCache> = RefCell::new(HashMap::new());
    /// The nodes to refresh, children before parents.
    static QUEUE: RefCell<VecDeque<CrGuid>> = const { RefCell::new(VecDeque::new()) };
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
    QUEUE.with(|q| q.borrow().len())
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
pub fn invalidate() {
    EPOCH.with(|e| e.set(e.get().wrapping_add(1)));
    STALE.with(|s| s.set(true));
    schedule(DEBOUNCE_MS);
}

/// Rebuilds the work queue from the ComicLists model, children before
/// parents (a folder tick finds fresh child sets).
fn rebuild_queue() {
    fn walk(items: &[ComicListItem], q: &mut VecDeque<CrGuid>) {
        for item in items {
            if let ComicListItem::Folder(f) = item {
                walk(&f.items, q);
            }
            q.push_back(item.base().id);
        }
    }
    let mut q = VecDeque::new();
    walk(&library::comic_lists_snapshot(), &mut q);
    QUEUE.with(|c| *c.borrow_mut() = q);
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

/// One tick: rebuild the queue when stale, refresh the next node, and
/// keep ticking while work remains.
fn step() {
    if STALE.with(|s| s.replace(false)) {
        rebuild_queue();
    }
    let next = QUEUE.with(|q| q.borrow_mut().pop_front());
    let Some(id) = next else {
        return;
    };
    refresh_node(&id);
    if QUEUE.with(|q| q.borrow().is_empty()) {
        // The run completed — every node of this rebuild refreshed.
        PASSES.with(|p| p.set(p.get() + 1));
    } else {
        schedule(TICK_MS);
    }
}

/// Refreshes one node: its book set (leaves evaluate, folders combine
/// children), the counters over the live book fields, the session
/// fields, and the row.
fn refresh_node(id: &CrGuid) {
    let lib = library::session();
    let Some(item) = library::find_list_item_any(id) else {
        return;
    };
    let epoch = EPOCH.with(Cell::get);
    // 1. The node's book set.
    let set = match &item {
        ComicListItem::Folder(folder) => {
            let children: Vec<HashSet<CrGuid>> = folder
                .items
                .iter()
                .map(|child| child_set(child, &lib, epoch))
                .collect();
            cr_engine::gauges::combine_folder_sets(folder.combine_mode, &children)
        }
        _ => {
            let l = lib.borrow();
            cr_engine::gauges::list_book_ids(&item, l.database())
        }
    };
    SETS.with(|c| c.borrow_mut().insert(*id, (epoch, set.clone())));
    // 2. The counters over the LIVE book fields (the cached sets hold
    // membership only — a read-progress change reclassifies without a
    // membership change).
    let now = CrDateTime::now();
    let recent = cr_core::settings::EngineConfiguration::global().is_recent_in_days;
    let gauges = {
        let l = lib.borrow();
        cr_engine::gauges::gauge_counts(
            l.database().books.iter().filter(|b| set.contains(&b.id)),
            &now,
            recent,
        )
    };
    // 3. The session fields (persist to ComicDb.xml).
    let stored = library::store_list_gauges(id, gauges, now);
    crate::trace::trace(format!(
        "gauges: refresh id={id} total={} new={} unread={} stored={stored}",
        gauges.total, gauges.new, gauges.unread
    ));
    // 4. The row (in place — no refill).
    ROW_HOOK.with(|h| {
        if let Some(f) = h.borrow().as_ref() {
            f(id);
        }
    });
}

/// A folder child's fresh set: the cache when current, otherwise
/// evaluated right away (nested folders combine their own children).
fn child_set(
    child: &ComicListItem,
    lib: &Rc<RefCell<cr_engine::library::Library>>,
    epoch: u64,
) -> HashSet<CrGuid> {
    let id = child.base().id;
    if let Some((e, s)) = SETS.with(|c| c.borrow().get(&id).map(|(e, s)| (*e, s.clone()))) {
        if e == epoch {
            return s;
        }
    }
    match child {
        ComicListItem::Folder(folder) => {
            let children: Vec<HashSet<CrGuid>> = folder
                .items
                .iter()
                .map(|c| child_set(c, lib, epoch))
                .collect();
            let set = cr_engine::gauges::combine_folder_sets(folder.combine_mode, &children);
            SETS.with(|c| c.borrow_mut().insert(id, (epoch, set.clone())));
            set
        }
        _ => {
            let l = lib.borrow();
            cr_engine::gauges::list_book_ids(child, l.database())
        }
    }
}
