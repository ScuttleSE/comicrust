//! ProcessingQueue / QueueManager concurrency tests: ordering, dedup,
//! capacity drops, idle detection, graceful shutdown — real threads,
//! short timeouts, no sleep-based assertions.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::time::Duration;

use cr_core::model::comic_book::ComicBook;
use cr_engine::queue::{AddMode, ProcessingQueue, ThreadPriority};
use cr_engine::queue_manager::{BookRef, QueueManager};

fn wait_for<F: Fn() -> bool>(timeout: Duration, check: F) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if check() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    check()
}

#[test]
fn add_to_top_processes_front_first() {
    // The first item blocks the single worker while the test queues the
    // other two, making the processing order deterministic.
    let order = Arc::new(Mutex::new(Vec::new()));
    let queue: ProcessingQueue<usize> =
        ProcessingQueue::new_single("t", ThreadPriority::BelowNormal, usize::MAX);
    let barrier = Arc::new(Barrier::new(2));
    let b0 = Arc::clone(&barrier);
    queue.add_item(0, move |_| {
        b0.wait();
    });
    let o1 = Arc::clone(&order);
    queue.add_item(1, move |_| {
        o1.lock().unwrap().push(1);
    });
    let o2 = Arc::clone(&order);
    queue.add_item_with_key(
        2,
        None,
        move |_| {
            o2.lock().unwrap().push(2);
        },
        AddMode::AddToTop,
    );
    barrier.wait(); // release item 0
    assert!(wait_for(Duration::from_secs(5), || {
        queue.count() == 0 && !queue.is_active()
    }));
    assert_eq!(*order.lock().unwrap(), vec![2, 1]);
}

#[test]
fn duplicate_add_does_not_requeue() {
    let runs = Arc::new(AtomicUsize::new(0));
    let queue: ProcessingQueue<usize> =
        ProcessingQueue::new_single("t", ThreadPriority::BelowNormal, usize::MAX);
    let r1 = Arc::clone(&runs);
    queue.add_item_with_key(
        7,
        Some("k"),
        move |_| {
            r1.fetch_add(1, Ordering::SeqCst);
        },
        AddMode::AddToBottom,
    );
    // Same key: no second callback registration (C# AddCallback).
    let r2 = Arc::clone(&runs);
    queue.add_item_with_key(
        7,
        Some("k"),
        move |_| {
            r2.fetch_add(1, Ordering::SeqCst);
        },
        AddMode::AddToTop,
    );
    assert!(wait_for(Duration::from_secs(5), || {
        queue.count() == 0 && !queue.is_active()
    }));
    assert_eq!(runs.load(Ordering::SeqCst), 1);
}

#[test]
fn capacity_drops_from_back() {
    let done = Arc::new(Mutex::new(Vec::new()));
    let queue: ProcessingQueue<usize> =
        ProcessingQueue::new_single("t", ThreadPriority::BelowNormal, 2);
    let barrier = Arc::new(Barrier::new(2));
    let b = Arc::clone(&barrier);
    queue.add_item(1, move |_| {
        b.wait();
    });
    // Queue is at capacity 1/2; adding two bottoms overflows and trims
    // the last added from the back.
    let d1 = Arc::clone(&done);
    queue.add_item_with_key(
        2,
        None,
        move |_| {
            d1.lock().unwrap().push(2);
        },
        AddMode::AddToBottom,
    );
    let d2 = Arc::clone(&done);
    queue.add_item_with_key(
        3,
        None,
        move |_| {
            d2.lock().unwrap().push(3);
        },
        AddMode::AddToBottom,
    );
    assert_eq!(queue.pending_items(), vec![1, 2]);
    barrier.wait();
    assert!(wait_for(Duration::from_secs(5), || {
        queue.count() == 0 && !queue.is_active()
    }));
    assert_eq!(*done.lock().unwrap(), vec![2]);
}

#[test]
fn graceful_stop_drains_all_pending() {
    let processed = Arc::new(AtomicUsize::new(0));
    let queue: ProcessingQueue<usize> =
        ProcessingQueue::new_single("t", ThreadPriority::BelowNormal, usize::MAX);
    for i in 0..8 {
        let p = Arc::clone(&processed);
        queue.add_item(i, move |_| {
            p.fetch_add(1, Ordering::SeqCst);
        });
    }
    queue.stop(false);
    assert_eq!(processed.load(Ordering::SeqCst), 8);
    assert_eq!(queue.count(), 0);
}

#[test]
fn abort_stop_discards_pending_after_current_item() {
    let processed = Arc::new(AtomicUsize::new(0));
    let queue: ProcessingQueue<usize> =
        ProcessingQueue::new_single("t", ThreadPriority::BelowNormal, usize::MAX);
    // The current item sleeps; the abort is requested while it runs.
    queue.add_item(0, |_| {
        std::thread::sleep(Duration::from_millis(150));
    });
    for i in 0..4 {
        let p = Arc::clone(&processed);
        queue.add_item_with_key(
            1000 + i,
            None,
            move |_| {
                p.fetch_add(1, Ordering::SeqCst);
            },
            AddMode::AddToBottom,
        );
    }
    queue.stop(true);
    // The pending ones never ran and the worker has exited (stop joins
    // it). Whether the sleeping item was claimed before the abort is a
    // race — only assert the pending items never ran.
    assert_eq!(processed.load(Ordering::SeqCst), 0);
}

#[test]
fn is_active_toggles_and_idle_settles() {
    let queue: ProcessingQueue<usize> =
        ProcessingQueue::new(4, "idle", ThreadPriority::BelowNormal, usize::MAX);
    let barrier = Arc::new(Barrier::new(3));
    for i in 0..2 {
        let b = Arc::clone(&barrier);
        queue.add_item(i, move |_| {
            b.wait();
        });
    }
    assert!(wait_for(Duration::from_secs(5), || queue.is_active()));
    barrier.wait(); // 2 workers + test = 3 participants
    assert!(wait_for(Duration::from_secs(5), || !queue.is_active()
        && queue.count() == 0));
}

#[test]
fn queue_manager_report_flags() {
    let mgr = QueueManager::new();
    let barrier = Arc::new(Barrier::new(2));
    let b = Arc::clone(&barrier);
    let item = BookRef(Arc::new(ComicBook::default()));
    mgr.write_comic_book_info_file_queue
        .add_item(item, move |_| {
            b.wait();
        });
    assert!(wait_for(Duration::from_secs(5), || mgr.is_in_comic_file_update()));
    barrier.wait();
    assert!(wait_for(Duration::from_secs(5), || !mgr.is_in_comic_file_update()));
}
