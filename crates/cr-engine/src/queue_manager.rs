//! `QueueManager` — the ComicBook-level background queues of the C#
//! `QueueManager` (the device-sync queue is a Phase 7 concern).
//!
//! C# construction:
//!
//! ```text
//! updateThreads = ProcessorCount.Clamp(1, MaximumUpdateThreads /* 2 */)
//! UpdateComicBookDynamicQueue  1            "Update Dynamic Books"       Lowest  AddToTop
//! ExportComicsQueue            1            "Export Books"               Lowest  AddToBottom
//! ReadComicBookInfoFileQueue   1            "Read Book File Information" Lowest  AddToBottom
//! WriteComicBookInfoFileQueue  updateThreads "Write Book File Information" Lowest AddToTop
//! ```
//!
//! Queue items are `ComicBook` instances, de-duplicated by reference —
//! `BookRef` wraps the book in an `Arc` and compares by pointer.

use std::sync::Arc;

use cr_core::model::comic_book::ComicBook;

use crate::queue::{ProcessingQueue, ThreadPriority};

/// A reference-identity queue item (`ComicBook` — the C# de-duplicates
/// by instance).
#[derive(Clone)]
pub struct BookRef(pub Arc<ComicBook>);

impl PartialEq for BookRef {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for BookRef {}
impl std::hash::Hash for BookRef {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        Arc::as_ptr(&self.0).hash(state);
    }
}
impl std::ops::Deref for BookRef {
    type Target = ComicBook;
    fn deref(&self) -> &ComicBook {
        &self.0
    }
}

/// The ComicBook-level queues.
pub struct QueueManager {
    /// `UpdateComicBookDynamicQueue` — web-comic refresh.
    pub update_comic_book_dynamic_queue: ProcessingQueue<BookRef>,
    /// `ExportComicsQueue`.
    pub export_comics_queue: ProcessingQueue<BookRef>,
    /// `ReadComicBookInfoFileQueue`.
    pub read_comic_book_info_file_queue: ProcessingQueue<BookRef>,
    /// `WriteComicBookInfoFileQueue`.
    pub write_comic_book_info_file_queue: ProcessingQueue<BookRef>,
}

impl Default for QueueManager {
    fn default() -> Self {
        Self::new()
    }
}

impl QueueManager {
    pub fn new() -> Self {
        let update_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
            .clamp(1, 2);
        let mut dynamic =
            ProcessingQueue::new_single("Update Dynamic Books", ThreadPriority::Lowest, usize::MAX);
        dynamic.set_default_mode(crate::queue::AddMode::AddToTop);
        let mut write = ProcessingQueue::new(
            update_threads,
            "Write Book File Information",
            ThreadPriority::Lowest,
            usize::MAX,
        );
        write.set_default_mode(crate::queue::AddMode::AddToTop);
        QueueManager {
            update_comic_book_dynamic_queue: dynamic,
            export_comics_queue: ProcessingQueue::new_single(
                "Export Books",
                ThreadPriority::Lowest,
                usize::MAX,
            ),
            read_comic_book_info_file_queue: ProcessingQueue::new_single(
                "Read Book File Information",
                ThreadPriority::Lowest,
                usize::MAX,
            ),
            write_comic_book_info_file_queue: write,
        }
    }

    /// `IsInComicFileRefresh`.
    pub fn is_in_comic_file_refresh(&self) -> bool {
        self.read_comic_book_info_file_queue.is_active()
            || self.update_comic_book_dynamic_queue.is_active()
    }

    /// `IsInComicFileUpdate`.
    pub fn is_in_comic_file_update(&self) -> bool {
        self.write_comic_book_info_file_queue.is_active()
    }

    /// `IsActive` (the C# also folds in comic conversion and device
    /// sync; conversion runs through `export_comics_queue` here).
    pub fn is_active(&self) -> bool {
        self.is_in_comic_file_update() || self.export_comics_queue.is_active()
    }
}
