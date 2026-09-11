//! The Tasks dialog — the `Dialogs/TasksDialog.cs` port.
//!
//! The C# dialog lists `QueueManager.GetQueues()` — one group per
//! background queue, up to 10 pending items each, a "{0} Tasks are
//! pending" counter, a 1 s update timer, and an "Abort all User
//! Tasks" split button. The Server Statistics tab is omitted (the
//! remote server is ADR-024 territory).
//!
//! Ported queue sources: the ImagePool page/thumbnail queues, the
//! session QueueManager read/export queues, the debounced write-back
//! timers (the port's `WriteComicBookInfoFileQueue` equivalent), and
//! the scan worker. The Update Web Comics / Device Sync queues are
//! omitted (the ADR-024 omissions + the Phase 1 web-comic gap).
//!
//! Deviations (recorded): the write rows show the file path (the C#
//! shows the book caption); the list scroll position is not preserved
//! across refreshes. (The scan abort closed 2026-09-10 —
//! `Scanner.Stop(clearQueue: true)` ported as `library::abort_scan`.)

use std::path::Path;
use std::sync::Arc;

use gtk4::prelude::*;
use gtk4::{
    glib, Align, Button, CellRendererText, Label, Orientation, ScrolledWindow, TreeView,
    TreeViewColumn, Window,
};

use cr_engine::image_pool::ImagePool;
use cr_engine::queue_manager::{BookRef, QueueManager};

/// The row cap per queue (`pendingItems.Take(10)`).
pub const MAX_ROWS_PER_QUEUE: usize = 10;

/// The state texts (the C# `TR.Default` Waiting/Running; no ported
/// queue item carries a `ProgressState`).
pub const WAITING: &str = "Waiting";
pub const RUNNING: &str = "Running";

/// One pending-task row.
pub struct TaskEntry {
    pub text: String,
    pub state: &'static str,
}

/// One queue's pending block (the C# `QueueManager.IPendingTasks`).
pub struct PendingTasks {
    pub group: &'static str,
    pub tasks: Vec<TaskEntry>,
    /// Rows hidden behind the cap (rendered as "{0} more...").
    pub more: usize,
    /// The abort command text (None = not abortable).
    pub abort: Option<&'static str>,
}

impl PendingTasks {
    /// The queue's full pending count (rows + hidden).
    pub fn pending_count(&self) -> usize {
        self.tasks.len() + self.more
    }
}

/// The abort command texts (the C# `TR.Messages` defaults).
pub const ABORT_COVER: &str = "Abort Cover Generation";
pub const ABORT_UPDATE: &str = "Abort Update";
pub const ABORT_EXPORT: &str = "Abort Export";
/// The scan row (`scanComicAbortText`, QueueManager.cs:679).
pub const ABORT_SCAN: &str = "Abort Scanning";

/// The snapshot sources (`QueueManager.GetQueues` inputs).
pub struct TaskSnapshot<'a> {
    pub pool: &'a ImagePool,
    pub queues: &'a QueueManager,
    /// The pending write-back file paths (the port's debounced
    /// write timers).
    pub write_files: Vec<String>,
    /// The scan walk location while a scan runs (None = idle).
    pub scan_location: Option<String>,
}

/// The claimed-item rule: the first queued item of an ACTIVE queue is
/// the one running (the port marks the claim inside the queue lock);
/// everything else waits.
fn entries(texts: Vec<String>, active: bool) -> Vec<TaskEntry> {
    texts
        .into_iter()
        .enumerate()
        .map(|(i, text)| TaskEntry {
            text,
            state: if active && i == 0 { RUNNING } else { WAITING },
        })
        .collect()
}

fn file_name(location: &str) -> String {
    Path::new(location)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| location.to_string())
}

/// The C# `StringUtility.Format` two-placeholder shape (the Rust
/// `format!` needs literals; the templates carry two `{}` slots).
fn format_two(template: &str, a: &str, b: &str) -> String {
    template.replacen("{}", a, 1).replacen("{}", b, 1)
}

fn format_one(template: &str, a: &str) -> String {
    template.replacen("{}", a, 1)
}

fn page_texts(keys: &[cr_image::keys::PageKey], template: &str) -> Vec<String> {
    keys.iter()
        .map(|k| {
            format_two(
                template,
                &(k.key.index + 1).to_string(),
                &file_name(&k.key.location),
            )
        })
        .collect()
}

fn thumb_texts(keys: &[cr_image::keys::ThumbnailKey], template: &str) -> Vec<String> {
    keys.iter()
        .map(|k| {
            format_two(
                template,
                &(k.key.index + 1).to_string(),
                &file_name(&k.key.location),
            )
        })
        .collect()
}

fn book_texts(items: &[BookRef], template: &str) -> Vec<String> {
    items
        .iter()
        .map(|b| {
            let caption = cr_engine::display_text::caption(b);
            format_one(template, &caption)
        })
        .collect()
}

/// The queue snapshot — the ported queues in the C# `GetQueues`
/// order (the C# formats each pending item through its queue
/// message; the messages here are the C# defaults verbatim).
pub fn pending_tasks(snapshot: &TaskSnapshot) -> Vec<PendingTasks> {
    let pool = snapshot.pool;
    let queues = snapshot.queues;
    let mut out = Vec::new();
    let mut push =
        |group: &'static str, texts: Vec<String>, active: bool, abort: Option<&'static str>| {
            let more = texts.len().saturating_sub(MAX_ROWS_PER_QUEUE);
            let mut tasks = entries(texts, active);
            tasks.truncate(MAX_ROWS_PER_QUEUE);
            out.push(PendingTasks {
                group,
                tasks,
                more,
                abort,
            });
        };
    push(
        "Load Thumbnails",
        thumb_texts(
            &pool.fast_thumbnail_queue.pending_items(),
            "Retrieve cached thumbnail for page {} in file '{}'",
        ),
        pool.fast_thumbnail_queue.is_active(),
        None,
    );
    push(
        "Create Thumbnails",
        thumb_texts(
            &pool.slow_thumbnail_queue.pending_items(),
            "Create thumbnail for page {} in file '{}'",
        ),
        pool.slow_thumbnail_queue.is_active(),
        None,
    );
    push(
        "Create Thumbnails",
        pool.slow_thumbnail_queue_unlimited
            .pending_items()
            .iter()
            .map(|k| {
                format!(
                    "Create thumbnail for cover in file '{}'",
                    file_name(&k.key.location)
                )
            })
            .collect(),
        pool.slow_thumbnail_queue_unlimited.is_active(),
        Some(ABORT_COVER),
    );
    push(
        "Create Pages",
        page_texts(
            &pool.slow_page_queue.pending_items(),
            "Get page {} in file '{}'",
        ),
        pool.slow_page_queue.is_active(),
        None,
    );
    push(
        "Load Pages",
        page_texts(
            &pool.fast_page_queue.pending_items(),
            "Get page {} in file '{}'",
        ),
        pool.fast_page_queue.is_active(),
        None,
    );
    push(
        "Read Info",
        book_texts(
            &queues.read_comic_book_info_file_queue.pending_items(),
            "Refresh information for Book '{}'",
        ),
        queues.read_comic_book_info_file_queue.is_active(),
        None,
    );
    push(
        "Write Info",
        snapshot
            .write_files
            .iter()
            .map(|f| format!("Write information to Book file '{f}'"))
            .collect(),
        !snapshot.write_files.is_empty(),
        Some(ABORT_UPDATE),
    );
    push(
        "Export Books",
        book_texts(
            &queues.export_comics_queue.pending_items(),
            "Export Book '{}'",
        ),
        queues.export_comics_queue.is_active(),
        Some(ABORT_EXPORT),
    );
    if let Some(location) = &snapshot.scan_location {
        push(
            "Scanning",
            vec![format!("Scanning '{location}'")],
            true,
            Some(ABORT_SCAN),
        );
    }
    out
}

/// The total pending count (the "{0} Tasks are pending" label).
pub fn total_pending(tasks: &[PendingTasks]) -> usize {
    tasks.iter().map(PendingTasks::pending_count).sum()
}

/// The Tasks dialog window (single instance — the shell keeps it and
/// re-presents; the C# `ShowPendingTasks` activates the open dialog).
pub struct TasksDialog {
    pub window: Window,
}

/// Opens the non-modal Tasks dialog (`TasksDialog.Show`).
pub fn show_tasks_dialog(parent: &impl IsA<gtk4::Window>, pool: Arc<ImagePool>) -> TasksDialog {
    let window = Window::builder()
        .title("Tasks")
        .transient_for(parent)
        .default_width(632)
        .default_height(499)
        .build();

    let content = gtk4::Box::new(Orientation::Vertical, 6);
    content.set_margin_top(6);
    content.set_margin_bottom(6);
    content.set_margin_start(6);
    content.set_margin_end(6);
    window.set_child(Some(&content));

    // The task list (Task | State; the C# ListView groups render as
    // bold header rows here, the "more..." rows gray — both through
    // the markup column, the C# colors + bold).
    let store = gtk4::ListStore::new(&[String::static_type(), String::static_type()]);
    let view = TreeView::builder()
        .model(&store)
        .headers_visible(true)
        .build();
    view.set_vexpand(true);
    let append_col = |view: &TreeView, title: &str, markup: bool, col: i32| {
        let cell = CellRendererText::new();
        let column = TreeViewColumn::new();
        column.set_title(title);
        column.pack_start(&cell, true);
        if markup {
            column.add_attribute(&cell, "markup", col);
        } else {
            column.add_attribute(&cell, "text", col);
        }
        view.append_column(&column);
    };
    append_col(&view, "Task", true, 0);
    append_col(&view, "State", false, 1);

    let scroller = ScrolledWindow::new();
    scroller.set_child(Some(&view));
    scroller.set_vexpand(true);
    content.append(&scroller);

    // The bottom row: the pending counter, the abort, the close.
    let pending_label = Label::builder()
        .label("0 Tasks are pending")
        .halign(Align::Start)
        .build();
    let abort = Button::with_label("Abort all User Tasks");
    // "Skip current file" (PORT ADDITION, user request 2026-09-11):
    // abandon the file the scan is reading now and carry on. Enabled
    // only while a scan runs.
    let skip_file = Button::with_label("Skip current file");
    skip_file.set_tooltip_text(Some(
        "Abandon the file the scan is reading now and continue with the next one",
    ));
    skip_file.set_sensitive(false);
    let close = Button::with_label("Close");
    let bottom = gtk4::Box::new(Orientation::Horizontal, 6);
    bottom.append(&pending_label);
    let spacer = gtk4::Box::new(Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    bottom.append(&spacer);
    bottom.append(&skip_file);
    bottom.append(&abort);
    bottom.append(&close);
    content.append(&bottom);

    // The refresh (the C# 1 s updateTimer): re-reads the queue
    // snapshot. Enabled state of the abort follows the snapshot
    // (`totalAbortableItems > 0`).
    let refresh: std::rc::Rc<Box<dyn Fn()>> = {
        let store = store.clone();
        let pending_label = pending_label.clone();
        let abort = abort.clone();
        let skip_file = skip_file.clone();
        let pool = Arc::clone(&pool);
        std::rc::Rc::new(Box::new(move || {
            store.clear();
            // Bind the session borrow first (a temporary in the
            // struct literal drops before the snapshot uses it).
            let session = crate::library::session();
            let lib_ref = session.borrow();
            let snapshot = TaskSnapshot {
                pool: &pool,
                queues: lib_ref.queues(),
                write_files: crate::library::pending_write_files(),
                scan_location: crate::library::is_scanning()
                    .then(crate::library::scan_location)
                    .filter(|l| !l.is_empty()),
            };
            let tasks = pending_tasks(&snapshot);
            drop(lib_ref);
            for block in &tasks {
                // The group header row (the C# ListViewGroups).
                store.insert_with_values(
                    None,
                    &[
                        (
                            0,
                            &format!("<b>{}</b>", glib::markup_escape_text(block.group)),
                        ),
                        (1, &"".to_string()),
                    ],
                );
                for entry in &block.tasks {
                    store.insert_with_values(
                        None,
                        &[
                            (0, &glib::markup_escape_text(&entry.text).to_string()),
                            (1, &entry.state.to_string()),
                        ],
                    );
                }
                if block.more > 0 {
                    store.insert_with_values(
                        None,
                        &[
                            (
                                0,
                                &format!("<span foreground=\"gray\">{} more...</span>", block.more),
                            ),
                            (1, &"".to_string()),
                        ],
                    );
                }
            }
            pending_label.set_text(&format!("{} Tasks are pending", total_pending(&tasks)));
            let abortable = tasks
                .iter()
                .any(|b| b.abort.is_some() && b.pending_count() > 0);
            abort.set_sensitive(abortable);
            // Only a running scan has a "current file" to skip.
            skip_file.set_sensitive(crate::library::is_scanning());
        }))
    };

    // Skip current file: the scan abandons the file in flight, marks
    // it, and continues with the next one.
    {
        let refresh = std::rc::Rc::clone(&refresh);
        skip_file.connect_clicked(move |_| {
            crate::library::skip_current_scan_file();
            refresh();
        });
    }

    // Abort all (`btAbort_Click`): every abortable queue drops its
    // pending items. The ported aborts: the cover-generation queue,
    // the export queue, the pending file writes, and the scan
    // (`Scanner.Stop(clearQueue: true)`, QueueManager.cs:707).
    {
        let pool = Arc::clone(&pool);
        let refresh = std::rc::Rc::clone(&refresh);
        abort.connect_clicked(move |_| {
            pool.slow_thumbnail_queue_unlimited.clear();
            crate::library::session()
                .borrow()
                .queues()
                .export_comics_queue
                .clear();
            crate::library::clear_pending_writes();
            crate::library::abort_scan();
            refresh();
        });
    }
    {
        let window = window.clone();
        close.connect_clicked(move |_| window.hide());
    }

    // Show + the 1 s timer (a re-present restarts it; the timer
    // stops when the window hides).
    {
        let connect_window = window.clone();
        let refresh = std::rc::Rc::clone(&refresh);
        connect_window.clone().connect_show(move |_| {
            refresh();
            let window = connect_window.clone();
            let refresh = std::rc::Rc::clone(&refresh);
            glib::timeout_add_local(std::time::Duration::from_secs(1), move || {
                if !window.is_visible() {
                    return glib::ControlFlow::Break;
                }
                refresh();
                glib::ControlFlow::Continue
            });
        });
    }

    window.present();
    TasksDialog { window }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cr_core::model::comic_book::ComicBook;
    use cr_engine::queue::AddMode;
    use cr_image::keys::{ImageKey, PageKey, ThumbnailKey};

    fn page_key(location: &str, index: usize) -> PageKey {
        PageKey::new(
            ImageKey::from_file(
                location,
                Path::new(location),
                index,
                cr_core::model::enums::ImageRotation::None,
            ),
            Default::default(),
        )
    }

    fn thumb_key(location: &str, index: usize) -> ThumbnailKey {
        ThumbnailKey::new(ImageKey::from_file(
            location,
            Path::new(location),
            index,
            cr_core::model::enums::ImageRotation::None,
        ))
    }

    #[test]
    fn active_queue_first_item_runs() {
        let rows = entries(vec!["a".into(), "b".into(), "c".into()], true);
        assert_eq!(rows[0].state, RUNNING);
        assert_eq!(rows[1].state, WAITING);
        assert_eq!(rows[2].state, WAITING);
        let rows = entries(vec!["a".into()], false);
        assert_eq!(rows[0].state, WAITING);
    }

    #[test]
    fn snapshot_lists_queues_in_csharp_order() {
        let pool = Arc::new(ImagePool::new(None));
        let queues = QueueManager::new();
        // The queue workers drain the no-op items instantly — stop
        // them so the items stay queued for the snapshot (the states
        // then all read Waiting; the Running rule is the pure test
        // above).
        pool.fast_page_queue.stop(true);
        pool.slow_thumbnail_queue_unlimited.stop(true);
        for i in 0..3 {
            pool.fast_page_queue
                .add_item(page_key("/books/a.cbz", i), |_| {});
        }
        pool.slow_thumbnail_queue_unlimited
            .add_item(thumb_key("/books/cover.cbz", 0), |_| {});
        let snapshot = TaskSnapshot {
            pool: &pool,
            queues: &queues,
            write_files: vec!["/books/b.cbz".into()],
            scan_location: Some("/watch/root".into()),
        };
        let blocks = pending_tasks(&snapshot);
        let groups: Vec<&str> = blocks.iter().map(|b| b.group).collect();
        assert_eq!(
            groups,
            vec![
                "Load Thumbnails",
                "Create Thumbnails",
                "Create Thumbnails",
                "Create Pages",
                "Load Pages",
                "Read Info",
                "Write Info",
                "Export Books",
                "Scanning",
            ]
        );
        // The page rows carry the 1-based index and the file name.
        // The pools default to AddToTop (the C# parity) — the LAST
        // added item reads first.
        let load = &blocks[4];
        assert_eq!(load.tasks.len(), 3);
        assert_eq!(load.tasks[0].text, "Get page 3 in file 'a.cbz'");
        assert_eq!(load.tasks[0].state, WAITING);
        assert_eq!(load.tasks[1].text, "Get page 2 in file 'a.cbz'");
        assert_eq!(load.tasks[2].text, "Get page 1 in file 'a.cbz'");
        // The cover row is abortable.
        assert_eq!(blocks[2].abort, Some(ABORT_COVER));
        assert_eq!(
            blocks[2].tasks[0].text,
            "Create thumbnail for cover in file 'cover.cbz'"
        );
        // The write row carries the file path.
        assert_eq!(blocks[6].abort, Some(ABORT_UPDATE));
        assert_eq!(
            blocks[6].tasks[0].text,
            "Write information to Book file '/books/b.cbz'"
        );
        assert_eq!(blocks[6].tasks[0].state, RUNNING);
        // The scan row (abortable — QueueManager.cs:705, "Abort
        // Scanning" → `Scanner.Stop(clearQueue: true)`).
        assert_eq!(blocks[8].tasks[0].text, "Scanning '/watch/root'");
        assert_eq!(blocks[8].abort, Some(ABORT_SCAN));
        // The abortable set: cover + write + scan (export is empty).
        assert_eq!(total_pending(&blocks), 3 + 1 + 1 + 1);
    }

    #[test]
    fn rows_cap_at_ten_with_more() {
        let pool = Arc::new(ImagePool::new(None));
        let queues = QueueManager::new();
        // The workers stay out (the snapshot races them otherwise).
        // The UNLIMITED cover queue is the only one whose size never
        // trims (the C# `int.MaxValue`; the page queues cap at
        // `pageCount * 2` = 10 and would drop the excess).
        pool.slow_thumbnail_queue_unlimited.stop(true);
        for i in 0..12 {
            pool.slow_thumbnail_queue_unlimited.add_item_with_key(
                thumb_key("/books/cover.cbz", i),
                None,
                |_| {},
                AddMode::AddToTop,
            );
        }
        let snapshot = TaskSnapshot {
            pool: &pool,
            queues: &queues,
            write_files: Vec::new(),
            scan_location: None,
        };
        let blocks = pending_tasks(&snapshot);
        let create = blocks
            .iter()
            .find(|b| b.group == "Create Thumbnails" && b.abort.is_some())
            .unwrap();
        assert_eq!(create.tasks.len(), MAX_ROWS_PER_QUEUE);
        assert_eq!(create.more, 2);
        assert_eq!(create.pending_count(), 12);
        // The capped page queues hold their size only: 12 adds keep
        // 10 (the C# `AddItem` Trim parity), so `more` stays 0.
        pool.slow_page_queue.stop(true);
        for i in 0..12 {
            pool.slow_page_queue.add_item_with_key(
                page_key("/books/a.cbz", i),
                None,
                |_| {},
                AddMode::AddToBottom,
            );
        }
        let blocks = pending_tasks(&snapshot);
        let create = blocks.iter().find(|b| b.group == "Create Pages").unwrap();
        assert_eq!(create.tasks.len(), 10);
        assert_eq!(create.more, 0);
        assert_eq!(create.pending_count(), 10);
    }

    #[test]
    fn book_rows_use_the_caption() {
        let mut book = ComicBook {
            file_path: "/books/x.cbz".into(),
            ..ComicBook::default()
        };
        book.info.series = "Foo Bar".into();
        let texts = book_texts(
            &[BookRef(std::sync::Arc::new(book))],
            "Refresh information for Book '{}'",
        );
        assert_eq!(texts[0], "Refresh information for Book 'Foo Bar'");
    }
}
