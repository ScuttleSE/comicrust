//! The Library Organizer run window — the port of `WorkerForm` /
//! `loworkerform.py`: a log of the run, a progress readout, and a
//! Cancel button, with the mover engine on a worker thread. The
//! engine's blocking requests (duplicate conflicts, multi-value
//! selections) arrive over a std mpsc channel and are answered by
//! modal dialogs in the main-loop pump (Rule 6: GTK widgets stay
//! main-thread). The run's session mutations come back in the report
//! and are applied here through `library::apply_edited` /
//! `insert_new_book` / `remove_book`.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use gtk4::glib::ControlFlow;
use gtk4::prelude::*;
use gtk4::{gdk, glib, CheckButton, DropDown, Label, StringList, TextView};

use cr_core::model::comic_book::ComicBook;
use cr_organize::engine::{
    Apply, AuditItem, AuditReport, CoverSource, DuplicateAction, DuplicateAnswer, DuplicateAsk,
    DuplicateBookInfo, LogEntry, OrganizeUi, RunContext, UndoCollection,
};
use cr_organize::profile::Profile;
use cr_organize::template::{MultiValueAnswer, MultiValueAsk, MultiValueAsker};

/// What the worker sends the main thread.
enum UiRequest {
    /// Boxed: the largest variant.
    AskDuplicate(Box<DuplicateAsk>),
    /// Boxed.
    AskMultiValue(Box<MultiValueAsk>),
    Log(LogEntry),
    Progress {
        done: usize,
        total: usize,
    },
    /// Boxed: the report carries the applies and the undo log.
    Done(Box<RunOutcome>),
}

/// The run's end state: the report text and the session mutations
/// (already applied when the pump hands them over).
pub struct RunOutcome {
    pub text: String,
    pub failed_or_skipped: bool,
    pub applies: Vec<Apply>,
    pub residual: Option<UndoCollection>,
}

struct UndoRunOptions {
    effects: Option<Arc<dyn cr_organize::engine::FilesystemEffects>>,
    operation: Option<cr_engine::incoming_transaction::ActiveOperation>,
}

/// The UI's answer to a blocking request.
enum UiAnswer {
    Duplicate(DuplicateAnswer),
    MultiValue(MultiValueAnswer),
}

/// The worker-side UI seam over the channels (the `ChannelUi`
/// pattern of the scrape dialog).
struct ChannelUi {
    tx: std::sync::mpsc::Sender<UiRequest>,
    answer_rx: std::sync::mpsc::Receiver<UiAnswer>,
}

impl MultiValueAsker for ChannelUi {
    fn ask_multi_value(&mut self, ask: MultiValueAsk) -> MultiValueAnswer {
        let _ = self.tx.send(UiRequest::AskMultiValue(Box::new(ask)));
        match self.answer_rx.recv() {
            Ok(UiAnswer::MultiValue(a)) => a,
            _ => MultiValueAnswer::default(),
        }
    }
}

impl OrganizeUi for ChannelUi {
    fn ask_duplicate(&mut self, ask: DuplicateAsk) -> DuplicateAnswer {
        let _ = self.tx.send(UiRequest::AskDuplicate(Box::new(ask)));
        match self.answer_rx.recv() {
            Ok(UiAnswer::Duplicate(a)) => a,
            _ => DuplicateAnswer {
                action: DuplicateAction::Cancel,
                always: false,
            },
        }
    }

    fn log(&mut self, entry: LogEntry) {
        let _ = self.tx.send(UiRequest::Log(entry));
    }

    fn progress(&mut self, done: usize, total: usize) {
        let _ = self.tx.send(UiRequest::Progress { done, total });
    }
}

/// The worker-thread cover reads. The custom-thumbnail folder comes
/// from the image pool (fileless books); every other cover opens the
/// file through the cr-io provider.
struct PoolCover {
    pool: Option<Arc<cr_engine::image_pool::ImagePool>>,
}

impl CoverSource for PoolCover {
    fn fileless_cover(&self, book: &ComicBook) -> Option<cr_image::Image> {
        let pool = self.pool.as_ref()?;
        let key = book.custom_thumbnail_key.as_ref()?;
        let bytes = pool.read_custom_thumbnail(key)?;
        cr_image::decode(&bytes).ok()
    }

    fn duplicate_cover(&self, book: &ComicBook) -> Option<Vec<u8>> {
        if book.file_path.is_empty() {
            let image = self.fileless_cover(book)?;
            let thumb = cr_image::thumbnail_from_image(&image, (image.width, image.height)).ok()?;
            return Some(thumb.to_bytes());
        }
        let provider =
            cr_io::provider::ComicProvider::open(std::path::Path::new(&book.file_path)).ok()?;
        let page = book.info.front_cover_page_index().max(0) as usize;
        let bytes = provider.read_page(page)?;
        let image = cr_image::decode(&bytes).ok()?;
        let thumb = cr_image::thumbnail_from_image(&image, (image.width, image.height)).ok()?;
        Some(thumb.to_bytes())
    }
}

/// The recycle-bin delete (the `ShellFile.DeleteFile` parity guard:
/// an empty path or a non-file never reaches gio).
fn trash_file(path: &str) -> bool {
    if path.is_empty() || !std::path::Path::new(path).is_file() {
        return false;
    }
    std::process::Command::new("gio")
        .args(["trash", path])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn log_line(entry: &LogEntry) -> String {
    let mut line = format!("{}: {}", entry.action, entry.path);
    if !entry.message.is_empty() {
        line.push_str(&format!(": {}", entry.message));
    }
    if !entry.profile.is_empty() {
        line = format!("[{}] {}", entry.profile, line);
    }
    line
}

/// Applies one session mutation on the main thread.
fn apply_to_library(apply: &Apply, operation: &cr_engine::incoming_transaction::ActiveOperation) {
    match apply {
        Apply::Update(book) => {
            crate::library::apply_edited_from_organizer(book, operation);
        }
        Apply::Insert(book) => {
            crate::library::insert_new_book_from_organizer(book, operation);
        }
        Apply::Adopt(book) => {
            crate::library::insert_new_book_from_organizer(book, operation);
        }
        Apply::Remove(id) => {
            crate::library::remove_book_from_organizer(id, operation);
        }
    }
}

/// One book the user chose to fix in the audit results window.
#[derive(Clone, Debug)]
pub struct AuditApply {
    pub book_id: cr_core::xml::scalar::CrGuid,
    /// The losing library book to remove first when the fix takes a
    /// path currently held by another library book.
    pub remove_loser: Option<cr_core::xml::scalar::CrGuid>,
    /// Delete this audited (wrong-place) book instead of moving it —
    /// the existing copy at the planned path won the collision.
    pub delete_audited: bool,
}

/// A colliding row's resolution. `Unresolved` rows are not applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Resolution {
    /// No collision — a plain relocation.
    None,
    /// Move the audited book; remove the losing existing book.
    KeepAudited,
    /// The existing book wins; delete the audited (wrong-place) book.
    KeepExisting,
    /// A collision the user has not decided yet.
    Unresolved,
    /// A bare-file collision — cannot be applied.
    Blocked,
}

/// Runs a read-only audit on a worker thread, then opens the results
/// window. Each mismatch has a checkbox; `on_apply` receives the book
/// indexes the user checked (deduplicated) so the caller can run the
/// normal Organizer over exactly those books.
pub fn show_audit_dialog(
    parent: &impl IsA<gtk4::Window>,
    books: Vec<ComicBook>,
    selected: Vec<usize>,
    profiles: Vec<Profile>,
    pool: Option<Arc<cr_engine::image_pool::ImagePool>>,
    rules: cr_engine::duplicates::DuplicateRules,
    on_apply: impl Fn(Vec<AuditApply>, Vec<Profile>) + 'static,
) -> bool {
    if books.is_empty() || selected.is_empty() || profiles.is_empty() {
        return false;
    }
    let worker_cover = Arc::new(PoolCover { pool: pool.clone() });
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancel);
    let profiles_run = profiles.clone();

    let (tx, rx) = std::sync::mpsc::channel::<AuditRequest>();
    let (answer_tx, answer_rx) = std::sync::mpsc::channel::<UiAnswer>();

    std::thread::Builder::new()
        .name("Library Organizer Audit".into())
        .spawn(move || {
            let done_tx = tx.clone();
            let mut ui = ChannelUi {
                tx: audit_bridge(tx),
                answer_rx,
            };
            let trash = |_p: &str| false;
            let ctx = RunContext {
                books: &books,
                selected: &selected,
                profiles: &profiles,
                trash: &trash,
                filesystem_effects: None,
                cover: worker_cover.as_ref(),
                undo_path: None,
                cancel: &worker_cancel,
                move_landing: cr_organize::engine::MoveLanding::UpdateExisting,
            };
            let report = cr_organize::engine::audit(ctx, &mut ui);
            let _ = done_tx.send(AuditRequest::Done(Box::new((report, books))));
        })
        .expect("spawn the organizer audit worker");

    // A small progress window while the audit runs.
    let window = gtk4::Window::builder()
        .title("Library Organizer — Audit")
        .transient_for(parent)
        .default_width(420)
        .default_height(120)
        .build();
    let progress_label = Label::new(Some("Auditing…"));
    progress_label.set_halign(gtk4::Align::Start);
    progress_label.set_margin_top(12);
    progress_label.set_margin_bottom(12);
    progress_label.set_margin_start(12);
    progress_label.set_margin_end(12);
    let cancel_btn = gtk4::Button::with_label("Cancel");
    cancel_btn.set_margin_bottom(12);
    cancel_btn.set_margin_end(12);
    cancel_btn.set_halign(gtk4::Align::End);
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    content.append(&progress_label);
    content.append(&cancel_btn);
    window.set_child(Some(&content));
    window.present();
    {
        let stop = Arc::clone(&cancel);
        cancel_btn.connect_clicked(move |_| stop.store(true, Ordering::Relaxed));
    }
    {
        let stop = Arc::clone(&cancel);
        window.connect_close_request(move |_w| {
            stop.store(true, Ordering::Relaxed);
            glib::Propagation::Proceed
        });
    }

    let window_pump = window;
    let progress_pump = progress_label;
    let answer_tx_pump = answer_tx;
    let parent_pump = parent.upcast_ref::<gtk4::Window>().clone();
    let profiles_pump = profiles_run;
    let pool_pump = pool;
    let rules_pump = rules;
    let mut on_apply = Some(on_apply);
    glib::timeout_add_local(Duration::from_millis(50), move || {
        loop {
            let request = match rx.try_recv() {
                Ok(request) => request,
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    window_pump.close();
                    return ControlFlow::Break;
                }
            };
            match request {
                AuditRequest::Progress { done, total } => {
                    progress_pump.set_text(&format!("Auditing… {done} of {total}"));
                }
                AuditRequest::AskMultiValue(ask) => {
                    ask_multi_value(&window_pump, *ask, &answer_tx_pump);
                }
                AuditRequest::Done(payload) => {
                    let (report, books) = *payload;
                    window_pump.close();
                    if let Some(cb) = on_apply.take() {
                        show_audit_results(
                            &parent_pump,
                            report,
                            books,
                            profiles_pump.clone(),
                            pool_pump.clone(),
                            rules_pump.clone(),
                            cb,
                        );
                    }
                    return ControlFlow::Break;
                }
            }
        }
        ControlFlow::Continue
    });
    true
}

/// The audit worker's channel messages.
enum AuditRequest {
    AskMultiValue(Box<MultiValueAsk>),
    Progress {
        done: usize,
        total: usize,
    },
    /// The report and the book snapshot (for display names).
    Done(Box<(AuditReport, Vec<ComicBook>)>),
}

/// Adapts an `AuditRequest` sender to the `UiRequest` sender the shared
/// `ChannelUi` expects. Only `Log`, `Progress` and `AskMultiValue`
/// arrive during an audit; log lines are dropped (the results window
/// shows the outcome, not the trace).
fn audit_bridge(tx: std::sync::mpsc::Sender<AuditRequest>) -> std::sync::mpsc::Sender<UiRequest> {
    let (bridge_tx, bridge_rx) = std::sync::mpsc::channel::<UiRequest>();
    std::thread::Builder::new()
        .name("audit-bridge".into())
        .spawn(move || {
            while let Ok(request) = bridge_rx.recv() {
                match request {
                    UiRequest::Progress { done, total } => {
                        if tx.send(AuditRequest::Progress { done, total }).is_err() {
                            break;
                        }
                    }
                    UiRequest::AskMultiValue(ask) => {
                        if tx.send(AuditRequest::AskMultiValue(ask)).is_err() {
                            break;
                        }
                    }
                    UiRequest::Log(_) | UiRequest::AskDuplicate(_) => {}
                    UiRequest::Done(_) => break,
                }
            }
        })
        .expect("spawn the audit bridge");
    bridge_tx
}

/// The audit results window: one row per mismatch with a checkbox, the
/// current and planned paths, and — for a collision — a Duplicate tag,
/// a per-row resolution, and a Compare button. Apply Selected hands the
/// chosen fixes back to the caller.
fn show_audit_results(
    parent: &impl IsA<gtk4::Window>,
    report: AuditReport,
    books: Vec<ComicBook>,
    profiles: Vec<Profile>,
    pool: Option<Arc<cr_engine::image_pool::ImagePool>>,
    rules: cr_engine::duplicates::DuplicateRules,
    on_apply: impl Fn(Vec<AuditApply>, Vec<Profile>) + 'static,
) {
    let window = gtk4::Window::builder()
        .title("Library Organizer — Audit Results")
        .transient_for(parent)
        .modal(true)
        .default_width(760)
        .default_height(520)
        .build();

    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    content.set_margin_start(8);
    content.set_margin_end(8);

    let collisions = report
        .items
        .iter()
        .filter(|i| i.collision.is_some())
        .count();
    let summary = Label::new(Some(&format!(
        "{} of {} book(s) do not match the selected profile(s). {} duplicate collision(s).",
        report.items.len(),
        report.scanned,
        collisions,
    )));
    summary.set_halign(gtk4::Align::Start);
    content.append(&summary);

    let list_box = gtk4::ListBox::new();
    list_box.set_selection_mode(gtk4::SelectionMode::None);

    // Per-row widgets and state, in report order.
    struct Row {
        check: CheckButton,
        resolution: std::rc::Rc<std::cell::Cell<Resolution>>,
        status: Label,
    }
    let rows: std::rc::Rc<std::cell::RefCell<Vec<Row>>> =
        std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let items = std::rc::Rc::new(report.items);
    let books = std::rc::Rc::new(books);
    let pool = std::rc::Rc::new(pool);
    let rules = std::rc::Rc::new(rules);

    let multi_profile = profiles.len() > 1;
    for (row_index, item) in items.iter().enumerate() {
        let row_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
        row_box.set_margin_top(4);
        row_box.set_margin_bottom(4);
        row_box.set_margin_start(4);
        row_box.set_margin_end(4);
        let check = CheckButton::new();
        check.set_valign(gtk4::Align::Center);
        row_box.append(&check);

        let text_box = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
        let name = book_display_name(&books, item);
        let title = if multi_profile {
            format!("{name}  [{}]", item.profile_name)
        } else {
            name
        };
        let title_label = Label::new(Some(&title));
        title_label.set_halign(gtk4::Align::Start);
        title_label.set_wrap(true);
        text_box.append(&title_label);
        let detail = Label::new(Some(&format!(
            "current: {}\nplanned: {}",
            display_path(&item.current_path),
            item.planned_path,
        )));
        detail.set_halign(gtk4::Align::Start);
        detail.set_wrap(true);
        detail.add_css_class("dim-label");
        text_box.append(&detail);

        let status = Label::new(None);
        status.set_halign(gtk4::Align::Start);
        text_box.append(&status);
        row_box.append(&text_box);

        // The resolution seed and the row controls.
        let resolution = std::rc::Rc::new(std::cell::Cell::new(match &item.collision {
            None => Resolution::None,
            Some(cr_organize::engine::AuditCollision::BareFile) => Resolution::Blocked,
            Some(_) => Resolution::Unresolved,
        }));

        match resolution.get() {
            Resolution::None => {
                check.set_active(true);
                status.set_text("Will move to the planned path.");
            }
            Resolution::Blocked => {
                check.set_active(false);
                check.set_sensitive(false);
                status.set_text(
                    "Duplicate: the destination holds a file that is not in the library — skipped.",
                );
                status.add_css_class("warning");
            }
            Resolution::Unresolved => {
                check.set_active(false);
                check.set_sensitive(false);
                status.set_text("Duplicate: choose which copy to keep.");
                status.add_css_class("warning");
            }
            _ => {}
        }

        // A Compare button on a collision that has another book.
        let compare_other = match &item.collision {
            Some(cr_organize::engine::AuditCollision::LibraryBook { book_index })
            | Some(cr_organize::engine::AuditCollision::AnotherAudited { book_index }) => {
                Some(*book_index)
            }
            _ => None,
        };
        if let Some(book_index) = compare_other {
            let compare = gtk4::Button::with_label("Compare…");
            compare.set_valign(gtk4::Align::Center);
            let items2 = std::rc::Rc::clone(&items);
            let books2 = std::rc::Rc::clone(&books);
            let pool2 = std::rc::Rc::clone(&pool);
            let rules2 = std::rc::Rc::clone(&rules);
            let resolution2 = std::rc::Rc::clone(&resolution);
            let check2 = check.clone();
            let status2 = status.clone();
            let window2 = window.clone();
            compare.connect_clicked(move |_| {
                let audited = books2[items2[row_index].book_index].clone();
                let existing = books2[book_index].clone();
                let resolution3 = std::rc::Rc::clone(&resolution2);
                let check3 = check2.clone();
                let status3 = status2.clone();
                show_audit_compare(
                    &window2,
                    audited,
                    existing,
                    (*pool2).clone(),
                    (*rules2).clone(),
                    move |chosen| {
                        apply_resolution(&resolution3, &check3, &status3, chosen);
                    },
                );
            });
            row_box.append(&compare);
        }

        list_box.append(&row_box);
        rows.borrow_mut().push(Row {
            check,
            resolution,
            status,
        });
    }

    let scroll = gtk4::ScrolledWindow::builder()
        .child(&list_box)
        .vexpand(true)
        .hexpand(true)
        .build();
    content.append(&scroll);

    let button_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    let select_all = gtk4::Button::with_label("Select All");
    let select_none = gtk4::Button::with_label("Select None");
    let select_worst = gtk4::Button::with_label("Select Worst Duplicates");
    let spacer = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    let apply_btn = gtk4::Button::with_label("Apply Selected");
    apply_btn.add_css_class("suggested-action");
    let close_btn = gtk4::Button::with_label("Close");
    button_row.append(&select_all);
    button_row.append(&select_none);
    button_row.append(&select_worst);
    button_row.append(&spacer);
    button_row.append(&close_btn);
    button_row.append(&apply_btn);
    content.append(&button_row);

    apply_btn.set_sensitive(!items.is_empty());
    {
        let rows = std::rc::Rc::clone(&rows);
        select_all.connect_clicked(move |_| {
            for row in rows.borrow().iter() {
                if row.check.is_sensitive() {
                    row.check.set_active(true);
                }
            }
        });
    }
    {
        let rows = std::rc::Rc::clone(&rows);
        select_none.connect_clicked(move |_| {
            for row in rows.borrow().iter() {
                row.check.set_active(false);
            }
        });
    }
    {
        // Resolve every unresolved/collision row by the duplicate rules.
        let rows = std::rc::Rc::clone(&rows);
        let items = std::rc::Rc::clone(&items);
        let books = std::rc::Rc::clone(&books);
        let rules = std::rc::Rc::clone(&rules);
        select_worst.connect_clicked(move |_| {
            let rows_ref = rows.borrow();
            for (row_index, item) in items.iter().enumerate() {
                let other = match &item.collision {
                    Some(cr_organize::engine::AuditCollision::LibraryBook { book_index })
                    | Some(cr_organize::engine::AuditCollision::AnotherAudited { book_index }) => {
                        *book_index
                    }
                    _ => continue,
                };
                let audited = &books[item.book_index];
                let existing = &books[other];
                let pair = [audited, existing];
                let worst = cr_engine::duplicates::worst_duplicate_ids(&pair, &rules);
                let chosen = if worst.len() == 1 && worst[0] == existing.id {
                    Resolution::KeepAudited
                } else if worst.len() == 1 && worst[0] == audited.id {
                    Resolution::KeepExisting
                } else {
                    continue;
                };
                let row = &rows_ref[row_index];
                apply_resolution(&row.resolution, &row.check, &row.status, chosen);
            }
        });
    }
    {
        let window = window.clone();
        close_btn.connect_clicked(move |_| window.close());
    }
    {
        let rows = std::rc::Rc::clone(&rows);
        let items = std::rc::Rc::clone(&items);
        let books = std::rc::Rc::clone(&books);
        let window = window.clone();
        apply_btn.connect_clicked(move |_| {
            let mut applies: Vec<AuditApply> = Vec::new();
            let mut seen: std::collections::HashSet<cr_core::xml::scalar::CrGuid> =
                std::collections::HashSet::new();
            for (row_index, row) in rows.borrow().iter().enumerate() {
                if !row.check.is_active() {
                    continue;
                }
                let item = &items[row_index];
                let book_id = books[item.book_index].id;
                let (remove_loser, delete_audited) = match row.resolution.get() {
                    Resolution::None => (None, false),
                    Resolution::KeepAudited => match &item.collision {
                        Some(cr_organize::engine::AuditCollision::LibraryBook { book_index }) => {
                            (Some(books[*book_index].id), false)
                        }
                        // AnotherAudited: no library loser to remove.
                        _ => (None, false),
                    },
                    // The existing copy won: delete the wrong-place book.
                    Resolution::KeepExisting => (None, true),
                    // Unresolved / Blocked never apply.
                    _ => continue,
                };
                if seen.insert(book_id) {
                    applies.push(AuditApply {
                        book_id,
                        remove_loser,
                        delete_audited,
                    });
                }
            }
            window.close();
            if !applies.is_empty() {
                on_apply(applies, profiles.clone());
            }
        });
    }

    window.set_child(Some(&content));
    window.present();
}

/// Writes a chosen resolution onto a row's controls.
fn apply_resolution(
    resolution: &std::rc::Rc<std::cell::Cell<Resolution>>,
    check: &CheckButton,
    status: &Label,
    chosen: Resolution,
) {
    resolution.set(chosen);
    status.remove_css_class("warning");
    match chosen {
        Resolution::KeepAudited => {
            check.set_sensitive(true);
            check.set_active(true);
            status.set_text("Duplicate: keep this copy; remove the existing one.");
        }
        Resolution::KeepExisting => {
            check.set_sensitive(true);
            check.set_active(true);
            status.set_text("Duplicate: remove this wrong-place copy; keep the existing one.");
        }
        _ => {}
    }
}

/// A two-pane compare of the audited book and the existing library
/// book with covers, details, and a Select Worst pick. `on_choose`
/// receives the chosen resolution (KeepAudited or KeepExisting).
fn show_audit_compare(
    parent: &impl IsA<gtk4::Window>,
    audited: ComicBook,
    existing: ComicBook,
    pool: Option<Arc<cr_engine::image_pool::ImagePool>>,
    rules: cr_engine::duplicates::DuplicateRules,
    on_choose: impl Fn(Resolution) + 'static,
) {
    let window = gtk4::Window::builder()
        .title("Audit — Compare Duplicates")
        .transient_for(parent)
        .modal(true)
        .default_width(900)
        .default_height(620)
        .build();

    let grid = gtk4::Grid::builder()
        .column_spacing(16)
        .column_homogeneous(true)
        .hexpand(true)
        .build();
    let (left_box, left_pic, left_status) =
        compare_pane("Audited book (would move here)", &audited);
    let (right_box, right_pic, right_status) = compare_pane("Existing book at the path", &existing);
    grid.attach(&left_box, 0, 0, 1, 1);
    grid.attach(&right_box, 1, 0, 1, 1);

    // The rule recommendation.
    let pair = [&audited, &existing];
    let worst = cr_engine::duplicates::worst_duplicate_ids(&pair, &rules);
    let recommendation = if worst.len() == 1 && worst[0] == existing.id {
        "Rules recommend: keep the audited copy."
    } else if worst.len() == 1 && worst[0] == audited.id {
        "Rules recommend: keep the existing copy."
    } else {
        "Rules find no clear worst copy."
    };
    let rec_label = Label::new(Some(recommendation));
    rec_label.set_halign(gtk4::Align::Start);

    let keep_audited = gtk4::Button::with_label("Keep Audited (remove existing)");
    keep_audited.add_css_class("suggested-action");
    let keep_existing = gtk4::Button::with_label("Keep Existing (delete this copy)");
    let cancel = gtk4::Button::with_label("Cancel");
    let button_row = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    button_row.append(&keep_audited);
    button_row.append(&keep_existing);
    let spacer = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    spacer.set_hexpand(true);
    button_row.append(&spacer);
    button_row.append(&cancel);

    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 12);
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.append(&grid);
    content.append(&rec_label);
    content.append(&button_row);
    let scroll = gtk4::ScrolledWindow::builder().child(&content).build();
    window.set_child(Some(&scroll));

    let on_choose = std::rc::Rc::new(on_choose);
    {
        let window = window.clone();
        let on_choose = std::rc::Rc::clone(&on_choose);
        keep_audited.connect_clicked(move |_| {
            on_choose(Resolution::KeepAudited);
            window.close();
        });
    }
    {
        let window = window.clone();
        let on_choose = std::rc::Rc::clone(&on_choose);
        keep_existing.connect_clicked(move |_| {
            on_choose(Resolution::KeepExisting);
            window.close();
        });
    }
    {
        let window = window.clone();
        cancel.connect_clicked(move |_| window.close());
    }

    // Load covers on the pool worker and paint them.
    load_compare_cover(&pool, &audited, &left_pic, &left_status);
    load_compare_cover(&pool, &existing, &right_pic, &right_status);

    window.present();
}

/// One compare pane: a heading, a cover slot, and the book details.
fn compare_pane(heading: &str, book: &ComicBook) -> (gtk4::Box, gtk4::Picture, Label) {
    let pane = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    let head = Label::new(Some(heading));
    head.set_halign(gtk4::Align::Start);
    head.add_css_class("title-4");
    pane.append(&head);
    let picture = gtk4::Picture::new();
    picture.set_size_request(240, 360);
    let status = Label::new(Some("Loading cover…"));
    pane.append(&picture);
    pane.append(&status);
    let details = Label::new(Some(&crate::dialogs::incoming_compare::book_details(book)));
    details.set_halign(gtk4::Align::Start);
    details.set_wrap(true);
    details.set_selectable(true);
    pane.append(&details);
    (pane, picture, status)
}

/// Queues a book's front-cover thumbnail on the pool and paints it.
fn load_compare_cover(
    pool: &Option<Arc<cr_engine::image_pool::ImagePool>>,
    book: &ComicBook,
    picture: &gtk4::Picture,
    status: &Label,
) {
    let Some(pool) = pool else {
        status.set_text("No cover available");
        return;
    };
    if book.file_path.is_empty() && book.custom_thumbnail_key.is_none() {
        status.set_text("No cover available");
        return;
    }
    let key = cr_engine::image_pool::front_cover_thumbnail_key(book);
    let render = Arc::clone(pool);
    let (tx, rx) = std::sync::mpsc::channel::<Option<Vec<u8>>>();
    pool.add_thumb_to_queue(key, None, move |key| {
        let _ = tx.send(render.render_thumbnail(key));
    });
    let picture = picture.clone();
    let status = status.clone();
    glib::timeout_add_local(Duration::from_millis(30), move || match rx.try_recv() {
        Ok(bytes) => {
            if let Some(texture) = bytes
                .as_deref()
                .and_then(crate::dialogs::incoming_compare::texture_from_thumb_blob)
            {
                picture.set_paintable(Some(&texture));
                status.set_text("");
            } else {
                status.set_text("No cover available");
            }
            ControlFlow::Break
        }
        Err(std::sync::mpsc::TryRecvError::Empty) => ControlFlow::Continue,
        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
            status.set_text("No cover available");
            ControlFlow::Break
        }
    });
}

/// A book's display name for a results row (its file name, or the
/// series/number stand-in for a fileless book).
fn book_display_name(books: &[ComicBook], item: &AuditItem) -> String {
    let book = &books[item.book_index];
    if !book.file_path.is_empty() {
        return std::path::Path::new(&book.file_path)
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_else(|| book.file_path.clone());
    }
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

fn display_path(path: &str) -> String {
    if path.is_empty() {
        "(fileless)".to_string()
    } else {
        path.to_string()
    }
}

/// Opens the organize run over the selection (`WorkerForm`).
/// `on_done` runs on the main thread when the run ends (applies
/// already committed).
pub fn show_run_dialog(
    parent: &impl IsA<gtk4::Window>,
    books: Vec<ComicBook>,
    selected: Vec<usize>,
    profiles: Vec<Profile>,
    undo_path: Option<std::path::PathBuf>,
    pool: Option<Arc<cr_engine::image_pool::ImagePool>>,
    on_done: impl Fn(&str) + 'static,
) -> bool {
    if books.is_empty() || selected.is_empty() || profiles.is_empty() {
        return false;
    }
    let Some(operation) = cr_engine::incoming_transaction::try_begin_operation() else {
        return false;
    };
    let worker_cover = Arc::new(PoolCover { pool: pool.clone() });
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancel);
    let run = move |ui: &mut dyn OrganizeUi| -> RunOutcome {
        let _guard = cr_engine::incoming_transaction::acquire_mutation_guard();
        let trash = |path: &str| trash_file(path);
        let ctx = RunContext {
            books: &books,
            selected: &selected,
            profiles: &profiles,
            trash: &trash,
            filesystem_effects: None,
            cover: worker_cover.as_ref(),
            undo_path: undo_path.clone(),
            cancel: &worker_cancel,
            move_landing: cr_organize::engine::MoveLanding::UpdateExisting,
        };
        let report = cr_organize::engine::organize(ctx, ui);
        let persistence_error = report.persistence_error;
        RunOutcome {
            text: match &persistence_error {
                Some(error) => format!(
                    "{}\n\nThe undo state could not be saved: {error}",
                    report.text
                ),
                None => report.text,
            },
            failed_or_skipped: report.failed_or_skipped || persistence_error.is_some(),
            applies: report.applies,
            residual: None,
        }
    };
    let done =
        move |outcome: RunOutcome,
              operation: Option<&cr_engine::incoming_transaction::ActiveOperation>| {
            let operation = operation.expect("ordinary organizer operation token");
            for apply in &outcome.applies {
                apply_to_library(apply, operation);
            }
            on_done(&outcome.text);
        };
    show_window(
        parent,
        "Library Organizer",
        cancel,
        run,
        done,
        Some(operation),
    );
    true
}

/// The undo run (`WorkerFormUndo`): the same window shape over
/// `cr_organize::engine::undo`.
pub fn show_undo_dialog(
    parent: &impl IsA<gtk4::Window>,
    books: Vec<ComicBook>,
    collection: UndoCollection,
    profiles: std::collections::HashMap<String, Profile>,
    pool: Option<Arc<cr_engine::image_pool::ImagePool>>,
    on_done: impl Fn(&str) + 'static,
) -> bool {
    show_undo_dialog_outcome(
        parent,
        books,
        collection,
        profiles,
        pool,
        move |outcome, operation| {
            let operation = operation.expect("ordinary organizer undo operation token");
            for apply in &outcome.applies {
                apply_to_library(apply, operation);
            }
            on_done(&outcome.text);
        },
    )
}

/// Opens Undo and returns its complete worker result to a custom landing.
pub fn show_undo_dialog_outcome(
    parent: &impl IsA<gtk4::Window>,
    books: Vec<ComicBook>,
    collection: UndoCollection,
    profiles: std::collections::HashMap<String, Profile>,
    pool: Option<Arc<cr_engine::image_pool::ImagePool>>,
    on_done: impl FnOnce(RunOutcome, Option<&cr_engine::incoming_transaction::ActiveOperation>)
        + 'static,
) -> bool {
    if collection.is_empty() {
        return false;
    }
    let Some(operation) = cr_engine::incoming_transaction::try_begin_operation() else {
        return false;
    };
    show_custom_undo_dialog_outcome_with_operation(
        parent,
        books,
        collection,
        profiles,
        pool,
        on_done,
        UndoRunOptions {
            effects: None,
            operation: Some(operation),
        },
    );
    true
}

/// Opens ordinary Undo with an operation that the launch path already owns.
pub(crate) fn show_undo_dialog_outcome_with_operation(
    parent: &impl IsA<gtk4::Window>,
    books: Vec<ComicBook>,
    collection: UndoCollection,
    profiles: std::collections::HashMap<String, Profile>,
    pool: Option<Arc<cr_engine::image_pool::ImagePool>>,
    operation: cr_engine::incoming_transaction::ActiveOperation,
    on_done: impl FnOnce(RunOutcome, &cr_engine::incoming_transaction::ActiveOperation) + 'static,
) {
    show_custom_undo_dialog_outcome_with_operation(
        parent,
        books,
        collection,
        profiles,
        pool,
        move |outcome, active| {
            on_done(
                outcome,
                active.expect("ordinary organizer undo operation token"),
            )
        },
        UndoRunOptions {
            effects: None,
            operation: Some(operation),
        },
    );
}

pub fn show_custom_undo_dialog_outcome(
    parent: &impl IsA<gtk4::Window>,
    books: Vec<ComicBook>,
    collection: UndoCollection,
    profiles: std::collections::HashMap<String, Profile>,
    pool: Option<Arc<cr_engine::image_pool::ImagePool>>,
    effects: Option<Arc<dyn cr_organize::engine::FilesystemEffects>>,
    on_done: impl FnOnce(RunOutcome) + 'static,
) {
    show_custom_undo_dialog_outcome_with_operation(
        parent,
        books,
        collection,
        profiles,
        pool,
        move |outcome, _operation| on_done(outcome),
        UndoRunOptions {
            effects,
            operation: None,
        },
    );
}

fn show_custom_undo_dialog_outcome_with_operation(
    parent: &impl IsA<gtk4::Window>,
    books: Vec<ComicBook>,
    collection: UndoCollection,
    profiles: std::collections::HashMap<String, Profile>,
    pool: Option<Arc<cr_engine::image_pool::ImagePool>>,
    on_done: impl FnOnce(RunOutcome, Option<&cr_engine::incoming_transaction::ActiveOperation>)
        + 'static,
    options: UndoRunOptions,
) {
    if collection.is_empty() {
        return;
    }
    let worker_cover = Arc::new(PoolCover { pool: pool.clone() });
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancel);
    let serialize = options.operation.is_some();
    let effects = options.effects;
    let run = move |ui: &mut dyn OrganizeUi| -> RunOutcome {
        let _guard = serialize.then(cr_engine::incoming_transaction::acquire_mutation_guard);
        let trash = |path: &str| trash_file(path);
        let ctx = RunContext {
            books: &books,
            selected: &[],
            profiles: &[],
            trash: &trash,
            filesystem_effects: effects.as_deref(),
            cover: worker_cover.as_ref(),
            undo_path: None,
            cancel: &worker_cancel,
            move_landing: cr_organize::engine::MoveLanding::UpdateExisting,
        };
        let report = cr_organize::engine::undo(ctx, &collection, &profiles, ui);
        RunOutcome {
            text: report.text,
            failed_or_skipped: report.failed_or_skipped,
            applies: report.applies,
            residual: Some(report.residual),
        }
    };
    show_window(
        parent,
        "Library Organizer - Undo",
        cancel,
        run,
        on_done,
        options.operation,
    )
}

/// Loads the undo log (the `undo.dat` line file).
pub fn load_undo_collection(path: &std::path::Path) -> UndoCollection {
    UndoCollection::load(path)
}

/// The shared run-window body: the log view, the progress readout,
/// the cancel, the worker spawn, and the pump.
fn show_window(
    parent: &impl IsA<gtk4::Window>,
    title: &str,
    stop: Arc<std::sync::atomic::AtomicBool>,
    run: impl FnOnce(&mut dyn OrganizeUi) -> RunOutcome + Send + 'static,
    on_done: impl FnOnce(RunOutcome, Option<&cr_engine::incoming_transaction::ActiveOperation>)
        + 'static,
    operation: Option<cr_engine::incoming_transaction::ActiveOperation>,
) {
    let window = gtk4::Window::builder()
        .title(title)
        .transient_for(parent)
        .default_width(560)
        .default_height(380)
        .build();

    let log_view = TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .build();
    let scroll = gtk4::ScrolledWindow::builder()
        .child(&log_view)
        .vexpand(true)
        .hexpand(true)
        .build();
    let progress_label = Label::new(Some("Working…"));
    progress_label.set_halign(gtk4::Align::Start);
    progress_label.set_hexpand(true);
    let cancel = gtk4::Button::with_label("Cancel");
    let bottom = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    bottom.append(&progress_label);
    bottom.append(&cancel);

    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 6);
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    content.set_margin_start(8);
    content.set_margin_end(8);
    content.append(&scroll);
    content.append(&bottom);
    window.set_child(Some(&content));
    // A GTK4 window is invisible until present() (the Phase 15 trap).
    window.present();

    {
        let stop = Arc::clone(&stop);
        cancel.connect_clicked(move |_| {
            stop.store(true, Ordering::Relaxed);
        });
    }
    {
        let stop = Arc::clone(&stop);
        window.connect_close_request(move |_w| {
            stop.store(true, Ordering::Relaxed);
            glib::Propagation::Proceed
        });
    }

    let (tx, rx) = std::sync::mpsc::channel::<UiRequest>();
    let (answer_tx, answer_rx) = std::sync::mpsc::channel::<UiAnswer>();

    std::thread::Builder::new()
        .name("Library Organizer".into())
        .spawn(move || {
            let done_tx = tx.clone();
            let mut ui = ChannelUi { tx, answer_rx };
            let outcome = run(&mut ui);
            let _ = done_tx.send(UiRequest::Done(Box::new(outcome)));
        })
        .expect("spawn the organizer worker");

    let window_pump = window;
    let log_pump = log_view;
    let progress_pump = progress_label;
    let answer_tx_pump = answer_tx;
    let parent_pump = parent.upcast_ref::<gtk4::Window>().clone();
    let mut on_done_pump = Some(on_done);
    let mut operation = operation;
    // The per-operation log lines are shown live in the window, which
    // closes on Done. Collect them here so the completion report can
    // list the planned operations after the window is gone.
    let mut collected_log: Vec<String> = Vec::new();
    glib::timeout_add_local(Duration::from_millis(50), move || {
        loop {
            let request = match rx.try_recv() {
                Ok(request) => request,
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    window_pump.close();
                    operation.take();
                    return ControlFlow::Break;
                }
            };
            match request {
                UiRequest::Log(entry) => {
                    let line = log_line(&entry);
                    let buffer = log_pump.buffer();
                    let mut end = buffer.end_iter();
                    buffer.insert(&mut end, &format!("{line}\n"));
                    collected_log.push(line);
                    // Keep the newest line in view.
                    log_pump.scroll_to_mark(
                        &buffer.create_mark(None, &end, true),
                        0.0,
                        false,
                        0.0,
                        1.0,
                    );
                }
                UiRequest::Progress { done, total } => {
                    progress_pump.set_text(&format!("{done} of {total}"));
                }
                UiRequest::AskDuplicate(ask) => {
                    ask_duplicate(&parent_pump, *ask, &answer_tx_pump);
                }
                UiRequest::AskMultiValue(ask) => {
                    ask_multi_value(&parent_pump, *ask, &answer_tx_pump);
                }
                UiRequest::Done(mut outcome) => {
                    window_pump.close();
                    if !collected_log.is_empty() {
                        outcome.text = format!("{}\n\n{}", outcome.text, collected_log.join("\n"));
                    }
                    if let Some(active) = operation.take() {
                        active.finish(|active| {
                            if let Some(on_done) = on_done_pump.take() {
                                on_done(*outcome, Some(active));
                            }
                        });
                    } else if let Some(on_done) = on_done_pump.take() {
                        on_done(*outcome, None);
                    }
                    return ControlFlow::Break;
                }
            }
        }
        ControlFlow::Continue
    });
}

/// Opens the shared organizer window for a caller-defined worker and landing.
pub fn show_custom_run(
    parent: &impl IsA<gtk4::Window>,
    title: &str,
    run: impl FnOnce(&mut dyn OrganizeUi, &std::sync::atomic::AtomicBool) -> RunOutcome + Send + 'static,
    on_done: impl FnOnce(RunOutcome) + 'static,
) {
    let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancel);
    show_window(
        parent,
        title,
        cancel,
        move |ui| run(ui, &worker_cancel),
        move |outcome, _operation| on_done(outcome),
        None,
    );
}

/// One pane of the duplicate dialog: the book's display lines.
fn duplicate_pane(info: &DuplicateBookInfo) -> gtk4::Box {
    let box_ = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    if info.in_library {
        let series = format!("{} Vol.{} #{}", info.series, info.volume, info.number);
        let mut lines = vec![series];
        lines.push(format!(
            "{} pages",
            if info.page_count > 0 {
                info.page_count.to_string()
            } else {
                "?".to_string()
            }
        ));
        lines.push(info.file_size_text.clone());
        if !info.published_text.is_empty() {
            lines.push(info.published_text.clone());
        }
        if !info.added_text.is_empty() {
            lines.push(info.added_text.clone());
        }
        if !info.scan_info.is_empty() {
            lines.push(info.scan_info.clone());
        }
        let text = Label::new(Some(&lines.join("\n")));
        text.set_halign(gtk4::Align::Start);
        text.set_valign(gtk4::Align::Start);
        text.set_xalign(0.0);
        box_.append(&text);
    } else {
        let text = Label::new(Some(&format!("{}\n{}", info.series, info.file_size_text)));
        text.set_halign(gtk4::Align::Start);
        text.set_valign(gtk4::Align::Start);
        text.set_xalign(0.0);
        box_.append(&text);
    }
    // The cover pane (JPEG bytes from the engine).
    if let Some(bytes) = &info.cover {
        if let Ok(image) = cr_image::decode(bytes) {
            let texture = gdk::MemoryTexture::new(
                image.width as i32,
                image.height as i32,
                gdk::MemoryFormat::R8g8b8a8,
                &glib::Bytes::from_owned(image.rgba),
                (image.width * 4) as usize,
            );
            let picture = gtk4::Picture::new();
            picture.set_paintable(Some(&texture));
            picture.set_size_request(160, 220);
            box_.append(&picture);
        }
    }
    box_
}

/// The duplicate dialog (`DuplicateForm`): the two panes, the three
/// actions, and the "do this for all conflicts" check.
fn ask_duplicate(
    parent: &impl IsA<gtk4::Window>,
    ask: DuplicateAsk,
    answer_tx: &std::sync::mpsc::Sender<UiAnswer>,
) {
    let window = gtk4::Window::builder()
        .title("Library Organizer — Duplicate")
        .transient_for(parent)
        .modal(true)
        .default_width(640)
        .default_height(480)
        .build();

    let (subtitle, header_text, rename_prefix) = match ask.mode.as_str() {
        "Copy" => (
            "Replace the file in the destination folder with the file you are copying:",
            "Copy and Replace",
            "The file you are copying will be renamed: ",
        ),
        "Simulate" => (
            "Click the file you want to keep (simulated, no files will be deleted or moved)",
            "Replace",
            "The file you are moving will be renamed: ",
        ),
        _ => (
            "Replace the file in the destination folder with the file you are moving:",
            "Replace",
            "The file you are moving will be renamed: ",
        ),
    };

    let header = Label::new(Some(header_text));
    header.set_halign(gtk4::Align::Start);
    let sub = Label::new(Some(subtitle));
    sub.set_halign(gtk4::Align::Start);
    sub.set_wrap(true);

    let grid = gtk4::Grid::new();
    grid.set_column_spacing(16);
    grid.set_hexpand(true);
    let new_pane = ask
        .new_book
        .as_ref()
        .map(duplicate_pane)
        .unwrap_or_else(|| gtk4::Box::new(gtk4::Orientation::Vertical, 4));
    let old_pane = ask
        .old_book
        .as_ref()
        .map(duplicate_pane)
        .unwrap_or_else(|| gtk4::Box::new(gtk4::Orientation::Vertical, 4));
    grid.attach(&new_pane, 0, 0, 1, 1);
    grid.attach(&old_pane, 1, 0, 1, 1);

    let rename_label = Label::new(Some(&format!("{}{}", rename_prefix, ask.rename_filename)));
    rename_label.set_halign(gtk4::Align::Start);
    rename_label.set_wrap(true);

    let do_all =
        CheckButton::with_label(&format!("Do this for all conflicts ({})", ask.held_count));
    do_all.set_visible(ask.held_count > 1);

    let cancel_btn = gtk4::Button::with_label("Cancel");
    let rename_btn = gtk4::Button::with_label("Rename");
    let overwrite_btn = gtk4::Button::with_label("Replace");
    let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    buttons.set_halign(gtk4::Align::End);
    buttons.append(&do_all);
    buttons.append(&cancel_btn);
    buttons.append(&rename_btn);
    buttons.append(&overwrite_btn);

    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    content.set_margin_top(10);
    content.set_margin_bottom(10);
    content.set_margin_start(10);
    content.set_margin_end(10);
    content.append(&header);
    content.append(&sub);
    content.append(&grid);
    content.append(&rename_label);
    content.append(&buttons);
    window.set_child(Some(&content));

    fn answer(
        window: &gtk4::Window,
        answer_tx: &std::sync::mpsc::Sender<UiAnswer>,
        action: DuplicateAction,
        always: bool,
    ) {
        let _ = answer_tx.send(UiAnswer::Duplicate(DuplicateAnswer { action, always }));
        window.close();
    }
    {
        let window = window.clone();
        let answer_tx = answer_tx.clone();
        cancel_btn
            .connect_clicked(move |_| answer(&window, &answer_tx, DuplicateAction::Cancel, false));
    }
    {
        let window = window.clone();
        let answer_tx = answer_tx.clone();
        let do_all = do_all.clone();
        rename_btn.connect_clicked(move |_| {
            answer(
                &window,
                &answer_tx,
                DuplicateAction::Rename,
                do_all.is_active(),
            )
        });
    }
    {
        let window = window.clone();
        let answer_tx = answer_tx.clone();
        let do_all = do_all.clone();
        overwrite_btn.connect_clicked(move |_| {
            answer(
                &window,
                &answer_tx,
                DuplicateAction::Overwrite,
                do_all.is_active(),
            )
        });
    }
    {
        let answer_tx = answer_tx.clone();
        window.connect_close_request(move |_w| {
            let _ = answer_tx.send(UiAnswer::Duplicate(DuplicateAnswer {
                action: DuplicateAction::Cancel,
                always: false,
            }));
            glib::Propagation::Proceed
        });
    }
    window.present();
}

/// The Multi-Value Selection form: a check list of the field's
/// values, the every-issue/folder options, and the "always use"
/// memory.
fn ask_multi_value(
    parent: &impl IsA<gtk4::Window>,
    ask: MultiValueAsk,
    answer_tx: &std::sync::mpsc::Sender<UiAnswer>,
) {
    let window = gtk4::Window::builder()
        .title(format!(
            "Choose which {} you would like to use",
            plural_field(&ask.field_text)
        ))
        .transient_for(parent)
        .modal(true)
        .default_width(440)
        .default_height(440)
        .build();

    let subtitle = Label::new(Some(&format!(
        "{}: {}",
        if ask.series { "Series" } else { "Book" },
        ask.book_text
    )));
    subtitle.set_halign(gtk4::Align::Start);

    let list = gtk4::ListBox::new();
    let checks: Arc<Mutex<Vec<CheckButton>>> = Arc::default();
    for item in &ask.items {
        let check = CheckButton::with_label(item);
        check.set_active(ask.selected.contains(item));
        let row = gtk4::ListBoxRow::builder()
            .child(&check)
            .selectable(false)
            .build();
        list.append(&row);
        checks.lock().unwrap().push(check);
    }

    let every = CheckButton::with_label("Use the selection for every issue of the series");
    every.set_visible(ask.series);
    let folder = CheckButton::with_label("Use the values as folders");
    let always = CheckButton::with_label("Always use this selection");
    let always_dont_ask = CheckButton::with_label("Do not ask again");
    always_dont_ask.set_sensitive(false);
    {
        let always_dont_ask = always_dont_ask.clone();
        always.connect_toggled(move |b| {
            always_dont_ask.set_sensitive(b.is_active());
        });
    }

    let ok = gtk4::Button::with_label("OK");
    let cancel = gtk4::Button::with_label("Cancel");
    let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
    buttons.set_halign(gtk4::Align::End);
    buttons.append(&ok);
    buttons.append(&cancel);

    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    content.set_margin_top(10);
    content.set_margin_bottom(10);
    content.set_margin_start(10);
    content.set_margin_end(10);
    content.append(&subtitle);
    let scroll = gtk4::ScrolledWindow::builder()
        .child(&list)
        .vexpand(true)
        .build();
    content.append(&scroll);
    content.append(&every);
    content.append(&folder);
    content.append(&always);
    content.append(&always_dont_ask);
    content.append(&buttons);
    window.set_child(Some(&content));

    {
        let window = window.clone();
        let answer_tx = answer_tx.clone();
        let checks = Arc::clone(&checks);
        ok.connect_clicked(move |_| {
            let selection: Vec<String> = checks
                .lock()
                .unwrap()
                .iter()
                .filter(|c| c.is_active())
                .map(|c| c.label().unwrap_or_default().to_string())
                .collect();
            let _ = answer_tx.send(UiAnswer::MultiValue(MultiValueAnswer {
                selection,
                every_issue: every.is_active(),
                folder: folder.is_active(),
                always_use: always.is_active(),
                always_use_dont_ask: always.is_active() && always_dont_ask.is_active(),
            }));
            window.close();
        });
    }
    {
        let window = window.clone();
        let answer_tx = answer_tx.clone();
        cancel.connect_clicked(move |_| {
            let _ = answer_tx.send(UiAnswer::MultiValue(MultiValueAnswer::default()));
            window.close();
        });
    }
    window.present();
}

/// The C# form's field plural ("Writer" → "Writers",
/// "AlternateSeries" stays).
fn plural_field(field: &str) -> String {
    if field == "AlternateSeries" {
        return field.to_string();
    }
    match field.strip_suffix('s') {
        Some(stem) => format!("{stem}s"),
        None => format!("{field}s"),
    }
}

/// The profile selector (`ProfileSelector` in loworkerform.py): one
/// row per profile to use, preselected with the last-used names; OK
/// returns the ordered chosen names (the profiles run in order).
pub fn show_profile_selector(
    parent: &impl IsA<gtk4::Window>,
    names: &[String],
    preselected: &[String],
    on_done: impl Fn(Option<Vec<String>>) + 'static,
) {
    let dialog = gtk4::Dialog::builder()
        .title("Library Organizer — Profiles")
        .transient_for(parent)
        .modal(true)
        .default_width(380)
        .build();
    let content = dialog.content_area();
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    content.set_margin_start(8);
    content.set_margin_end(8);
    content.set_spacing(6);
    content.append(&Label::new(Some("Select the profile(s) to be used:")));

    // One row per selected profile, a drop-down each; Add appends.
    let rows: Arc<Mutex<Vec<DropDown>>> = Arc::default();
    let list_box = gtk4::Box::new(gtk4::Orientation::Vertical, 4);
    content.append(&list_box);

    let add_row = {
        let rows = Arc::clone(&rows);
        let list_box = list_box.clone();
        let names = names.to_vec();
        move |name: Option<String>| {
            let list = StringList::new(&names.iter().map(String::as_str).collect::<Vec<_>>());
            let drop = DropDown::new(Some(list), Option::<&gtk4::Expression>::None);
            if let Some(pos) = name
                .as_ref()
                .and_then(|n| names.iter().position(|x| x == n))
            {
                drop.set_selected(pos as u32);
            }
            list_box.append(&drop);
            rows.lock().unwrap().push(drop);
        }
    };
    if preselected.is_empty() {
        add_row(names.first().cloned());
    } else {
        for name in preselected {
            add_row(Some(name.clone()));
        }
    }

    let add_btn = gtk4::Button::with_label("Add");
    {
        let add_row = add_row;
        let first = names.first().cloned();
        add_btn.connect_clicked(move |_| add_row(first.clone()));
    }
    content.append(&add_btn);

    let names_owned = names.to_vec();
    dialog.add_button("OK", gtk4::ResponseType::Ok);
    dialog.add_button("Cancel", gtk4::ResponseType::Cancel);
    let done = std::rc::Rc::new(std::cell::Cell::new(false));
    {
        let rows = Arc::clone(&rows);
        let done = std::rc::Rc::clone(&done);
        let names_owned = names_owned.clone();
        dialog.connect_response(move |dlg, response| {
            if done.replace(true) {
                return;
            }
            let result = if response == gtk4::ResponseType::Ok {
                Some(
                    rows.lock()
                        .unwrap()
                        .iter()
                        .filter_map(|d| names_owned.get(d.selected() as usize).cloned())
                        .collect::<Vec<_>>(),
                )
            } else {
                None
            };
            dlg.close();
            on_done(result);
        });
    }
    dialog.present();
}
