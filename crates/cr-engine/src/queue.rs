//! Port of `cYo.Common.Threading.ProcessingQueue<K>`.
//!
//! Named worker threads drain a deduplicated, capacity-bounded queue.
//! The C# semantics that matter for parity:
//!
//! - `AddItem` de-duplicates: an item already queued (or running) is
//!   NOT queued twice; with the same callback key the new callback is
//!   ignored, with a new key it is registered as an extra callback. An
//!   `AddToTop` re-add moves the item to the front.
//! - Workers take the FIRST queue item that is still waiting, process
//!   it to completion, then remove it. The item stays in the queue
//!   while it runs (re-adds during the run only register callbacks;
//!   they are dropped again when the run completes — C# behavior).
//! - `Trim` drops items from the BACK.
//! - `Stop(abort: true)` exits workers even with pending items;
//!   `Stop(abort: false)` drains all waiting items first. There are no
//!   thread aborts in Rust: the current item always finishes (the C#
//!   flags are checked at the same points, so behavior matches).
//! - Improvement (documented deviation): the claimed item is marked
//!   running INSIDE the queue lock. The C# marks it running after the
//!   scan, which leaves a window where two workers could claim the
//!   same item.
//!
//! Thread priorities (`ThreadPriority`) are stored but not applied:
//! Linux has no portable user-space thread priority API that maps the
//! Windows values.

use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};

/// `ProcessingQueueAddMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddMode {
    AddToBottom,
    AddToTop,
}

/// `ThreadPriority` — stored for parity; not applied on Linux.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ThreadPriority {
    Lowest,
    #[default]
    BelowNormal,
    Normal,
    AboveNormal,
    Highest,
}

type Callback<K> = Arc<dyn Fn(&K) + Send + Sync>;

struct Entry<K> {
    callbacks: Vec<Callback<K>>,
    /// Callback keys already registered for this item (C#
    /// `QueueItem.AddCallback` de-duplication).
    keys: HashSet<String>,
    running: bool,
}

struct State<K> {
    queue: VecDeque<K>,
    entries: HashMap<K, Entry<K>>,
    abort: bool,
    stop: bool,
    disposed: bool,
    size: usize,
    default_mode: AddMode,
}

impl<K: Eq + Hash + Clone> State<K> {
    /// Claims the first waiting item: marks it running, returns a clone.
    fn claim_next(&mut self) -> Option<K> {
        for key in self.queue.iter() {
            if let Some(entry) = self.entries.get(key) {
                if !entry.running {
                    let key = key.clone();
                    if let Some(entry) = self.entries.get_mut(&key) {
                        entry.running = true;
                    }
                    return Some(key);
                }
            }
        }
        None
    }

    /// Completion of the claimed item: run bookkeeping — remove it from
    /// the queue and the entry table (dropping any callbacks registered
    /// meanwhile, like the C# `RemoveItem` after `ProcessCallbacks`).
    fn finish(&mut self, key: &K) {
        self.entries.remove(key);
        self.queue.retain(|k| k != key);
    }

    fn remove_item(&mut self, key: &K) {
        self.entries.remove(key);
        self.queue.retain(|k| k != key);
    }

    fn trim(&mut self, size: usize) {
        while self.queue.len() > size {
            if let Some(last) = self.queue.pop_back() {
                self.remove_item(&last);
            }
        }
    }

    fn waiting_count(&self) -> usize {
        self.queue
            .iter()
            .filter(|k| self.entries.get(*k).is_some_and(|e| !e.running))
            .count()
    }
}

struct Shared<K> {
    state: Mutex<State<K>>,
    signal: Condvar,
    active: AtomicUsize,
}

/// Generic worker queue (`ProcessingQueue<K>`). `K` is the item type;
/// de-duplication uses `Eq`/`Hash`. For reference-identity items (the
/// C# queues `ComicBook` instances) wrap them in a pointer-identity
/// newtype.
pub struct ProcessingQueue<K: Eq + Hash + Clone + Send + 'static> {
    shared: Arc<Shared<K>>,
    threads: Mutex<Vec<std::thread::JoinHandle<()>>>,
    name: String,
    priority: ThreadPriority,
}

impl<K: Eq + Hash + Clone + Send + 'static> ProcessingQueue<K> {
    /// `ProcessingQueue(threadCount, name, priority, size)`.
    pub fn new(thread_count: usize, name: &str, priority: ThreadPriority, size: usize) -> Self {
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                queue: VecDeque::new(),
                entries: HashMap::new(),
                abort: false,
                stop: false,
                disposed: false,
                size,
                default_mode: AddMode::AddToBottom,
            }),
            signal: Condvar::new(),
            active: AtomicUsize::new(0),
        });
        let mut threads = Vec::new();
        for i in 0..thread_count {
            let thread_name = if thread_count < 2 {
                name.to_string()
            } else {
                format!("{name} #{}", i + 1)
            };
            let shared = Arc::clone(&shared);
            let handle = std::thread::Builder::new()
                .name(thread_name)
                .spawn(move || worker_loop(shared))
                .expect("spawn worker thread");
            threads.push(handle);
        }
        ProcessingQueue {
            shared,
            threads: Mutex::new(threads),
            name: name.to_string(),
            priority,
        }
    }

    /// Single-threaded queue (`ProcessingQueue(name, priority, size)`).
    pub fn new_single(name: &str, priority: ThreadPriority, size: usize) -> Self {
        Self::new(1, name, priority, size)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn priority(&self) -> ThreadPriority {
        self.priority
    }

    pub fn set_default_mode(&mut self, mode: AddMode) {
        self.shared.state.lock().unwrap().default_mode = mode;
    }

    /// `Size`: the capacity; setting it trims from the back.
    pub fn set_size(&self, size: usize) {
        let mut state = self.shared.state.lock().unwrap();
        state.size = size;
        state.trim(size);
        drop(state);
        self.shared.signal.notify_all();
    }

    /// `Count`: queued items (including the currently running ones —
    /// they stay in the queue until completion).
    pub fn count(&self) -> usize {
        self.shared.state.lock().unwrap().queue.len()
    }

    /// `IsActive`: any worker currently processing.
    pub fn is_active(&self) -> bool {
        self.shared.active.load(Ordering::SeqCst) > 0
    }

    /// `PendingItems`: snapshot of the queue order.
    pub fn pending_items(&self) -> Vec<K> {
        self.shared
            .state
            .lock()
            .unwrap()
            .queue
            .iter()
            .cloned()
            .collect()
    }

    /// `AddItem(item, callbackKey, processCallback, mode)`.
    pub fn add_item_with_key(
        &self,
        item: K,
        callback_key: Option<&str>,
        callback: impl Fn(&K) + Send + Sync + 'static,
        mode: AddMode,
    ) -> bool {
        let mut state = self.shared.state.lock().unwrap();
        if state.disposed {
            return false;
        }
        match state.entries.get_mut(&item) {
            Some(entry) => {
                // Already queued/running: register the callback unless
                // the key was used before (C# `AddCallback`: a null key
                // or the item's own key registers nothing) and move to
                // the front for AddToTop.
                if let Some(key) = callback_key {
                    if !entry.keys.contains(key) {
                        entry.keys.insert(key.to_string());
                        entry.callbacks.push(Arc::new(callback));
                    }
                }
                if mode == AddMode::AddToTop {
                    if let Some(pos) = state.queue.iter().position(|k| *k == item) {
                        let key = state.queue.remove(pos).expect("position exists");
                        state.queue.push_front(key);
                    }
                }
            }
            None => {
                let mut keys = HashSet::new();
                let callbacks = vec![Arc::new(callback) as Callback<K>];
                if let Some(key) = callback_key {
                    keys.insert(key.to_string());
                }
                state.entries.insert(
                    item.clone(),
                    Entry {
                        callbacks,
                        keys,
                        running: false,
                    },
                );
                match mode {
                    AddMode::AddToBottom => state.queue.push_back(item),
                    AddMode::AddToTop => state.queue.push_front(item),
                }
                let size = state.size;
                state.trim(size);
            }
        }
        drop(state);
        self.shared.signal.notify_all();
        true
    }

    /// `AddItem(item, processCallback)` with the default mode.
    pub fn add_item(&self, item: K, callback: impl Fn(&K) + Send + Sync + 'static) -> bool {
        let mode = self.shared.state.lock().unwrap().default_mode;
        self.add_item_with_key(item, None, callback, mode)
    }

    /// `RemoveItem`.
    pub fn remove_item(&self, item: &K) {
        let mut state = self.shared.state.lock().unwrap();
        state.remove_item(item);
        drop(state);
        self.shared.signal.notify_all();
    }

    /// `Trim`.
    pub fn trim(&self, size: usize) {
        self.set_size(size);
    }

    /// `Clear`.
    pub fn clear(&self) {
        self.trim(0);
    }

    /// `Stop(abort, timeOut)`: with `abort` the workers exit even with
    /// pending items; otherwise they drain the waiting items first.
    /// Workers finish their current item either way (no thread aborts).
    pub fn stop(&self, abort: bool) {
        {
            let mut state = self.shared.state.lock().unwrap();
            if abort {
                state.abort = true;
            } else {
                state.stop = true;
            }
        }
        self.shared.signal.notify_all();
        let mut threads = self.threads.lock().unwrap();
        for handle in threads.drain(..) {
            let _ = handle.join();
        }
    }
}

impl<K: Eq + Hash + Clone + Send + 'static> Drop for ProcessingQueue<K> {
    fn drop(&mut self) {
        {
            let mut state = self.shared.state.lock().unwrap();
            state.queue.clear();
            state.entries.clear();
            state.abort = true;
        }
        self.shared.signal.notify_all();
        let mut threads = self.threads.lock().unwrap();
        for handle in threads.drain(..) {
            let _ = handle.join();
        }
    }
}

fn worker_loop<K: Eq + Hash + Clone + Send + 'static>(shared: Arc<Shared<K>>) {
    let mut guard = shared.state.lock().unwrap();
    loop {
        // Exit checks (C# checks `abort` each iteration and `stop` when
        // no waiting item remains).
        if guard.abort {
            return;
        }
        if guard.stop && guard.waiting_count() == 0 {
            return;
        }
        match guard.claim_next() {
            Some(item) => {
                let callbacks = guard
                    .entries
                    .get(&item)
                    .map(|e| e.callbacks.clone())
                    .unwrap_or_default();
                drop(guard);
                shared.active.fetch_add(1, Ordering::SeqCst);
                for callback in &callbacks {
                    callback(&item);
                }
                shared.active.fetch_sub(1, Ordering::SeqCst);
                guard = shared.state.lock().unwrap();
                guard.finish(&item);
                // Free waiting workers (a slot freed up).
                shared.signal.notify_all();
            }
            None => {
                guard = shared
                    .signal
                    .wait(guard)
                    .expect("queue state mutex not poisoned");
            }
        }
    }
}
