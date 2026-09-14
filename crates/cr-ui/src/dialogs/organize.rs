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
    Apply, CoverSource, DuplicateAction, DuplicateAnswer, DuplicateAsk, DuplicateBookInfo,
    LogEntry, OrganizeUi, RunContext, UndoCollection,
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
struct RunOutcome {
    text: String,
    failed_or_skipped: bool,
    applies: Vec<Apply>,
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
fn apply_to_library(apply: &Apply) {
    match apply {
        Apply::Update(book) => {
            crate::library::apply_edited(book);
        }
        Apply::Insert(book) => {
            crate::library::insert_new_book(book);
        }
        Apply::Remove(id) => {
            crate::library::remove_book(id);
        }
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
) {
    if books.is_empty() || selected.is_empty() || profiles.is_empty() {
        return;
    }
    let worker_cover = Arc::new(PoolCover { pool: pool.clone() });
    let worker_cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let run = move |ui: &mut dyn OrganizeUi| -> (String, bool, Vec<Apply>) {
        let trash = |path: &str| trash_file(path);
        let ctx = RunContext {
            books: &books,
            selected: &selected,
            profiles: &profiles,
            trash: &trash,
            cover: worker_cover.as_ref(),
            undo_path: undo_path.clone(),
            cancel: &worker_cancel,
        };
        let report = cr_organize::engine::organize(ctx, ui);
        (report.text, report.failed_or_skipped, report.applies)
    };
    let done = move |text: &str, _failed: bool| on_done(text);
    show_window(parent, "Library Organizer", run, done)
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
) {
    if collection.is_empty() {
        return;
    }
    let worker_cover = Arc::new(PoolCover { pool: pool.clone() });
    let worker_cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let run = move |ui: &mut dyn OrganizeUi| -> (String, bool, Vec<Apply>) {
        let trash = |path: &str| trash_file(path);
        let ctx = RunContext {
            books: &books,
            selected: &[],
            profiles: &[],
            trash: &trash,
            cover: worker_cover.as_ref(),
            undo_path: None,
            cancel: &worker_cancel,
        };
        let report = cr_organize::engine::undo(ctx, &collection, &profiles, ui);
        (report.text, report.failed_or_skipped, report.applies)
    };
    let done = move |text: &str, _failed: bool| on_done(text);
    show_window(parent, "Library Organizer — Undo", run, done)
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
    run: impl FnOnce(&mut dyn OrganizeUi) -> (String, bool, Vec<Apply>) + Send + 'static,
    on_done: impl Fn(&str, bool) + 'static,
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

    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
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
            let (text, failed_or_skipped, applies) = run(&mut ui);
            let _ = done_tx.send(UiRequest::Done(Box::new(RunOutcome {
                text,
                failed_or_skipped,
                applies,
            })));
        })
        .expect("spawn the organizer worker");

    let window_pump = window;
    let log_pump = log_view;
    let progress_pump = progress_label;
    let answer_tx_pump = answer_tx;
    let parent_pump = parent.upcast_ref::<gtk4::Window>().clone();
    let on_done_pump = on_done;
    glib::timeout_add_local(Duration::from_millis(50), move || {
        while let Ok(request) = rx.try_recv() {
            match request {
                UiRequest::Log(entry) => {
                    let buffer = log_pump.buffer();
                    let mut end = buffer.end_iter();
                    buffer.insert(&mut end, &format!("{}\n", log_line(&entry)));
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
                UiRequest::Done(outcome) => {
                    for apply in &outcome.applies {
                        apply_to_library(apply);
                    }
                    let text = outcome.text.clone();
                    let failed_or_skipped = outcome.failed_or_skipped;
                    window_pump.close();
                    on_done_pump(&text, failed_or_skipped);
                    return ControlFlow::Break;
                }
            }
        }
        ControlFlow::Continue
    });
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
