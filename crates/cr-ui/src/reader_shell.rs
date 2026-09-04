//! The reader shell — the tabbed page-view host (`MainForm` file
//! tabs over the `OpenBooks` slots). Phase 5 docks it into the
//! browser window as a view (the C# main-form shape); the standalone
//! window remains only as the undock target (`D`).
//!
//! Shell duties ported from `MainForm`:
//! - reading-state write-back (`OnBookOpened` stamps
//!   `OpenedTime`/`OpenedCount`; `TrackCurrentPage` mirrors every
//!   page change into `ComicBook.CurrentPage`/`LastPageRead`).
//!   Library comics write back into the database book (Phase 4 T1);
//!   non-library comics keep session-only state (the C# temporary
//!   books are memory-only).
//! - `Tab`/`Shift+Tab` slot switching (`OpenBooks.NextSlot`/
//!   `PreviousSlot`), closable tabs.
//! - `D` undocks the current reader into its own window and back
//!   (`ReaderUndocked`; one undocked reader, like the single
//!   `ReaderForm`). While undocked, the tab strip keeps working —
//!   a divergence from the C#, which shows every slot in the one
//!   shared display; the shared-display architecture is Phase 4.
//! - `K` (`MinimalGui`) pins the chrome hidden; otherwise the header
//!   auto-hides `AutoHideMainMenu`-style (reveals in the top strip).
//! - The fullscreen cursor auto-hide (`HideCursorFullScreen`
//!   default true).

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use anyhow::Context;
use gtk4::prelude::*;
use gtk4::{glib, Application, ApplicationWindow, HeaderBar, Label, Notebook, Orientation};

use cr_core::model::comic_book::ComicBook;
use cr_engine::image_pool::ImagePool;

use crate::library;
use crate::reader::page_view::PageView;

/// Default reader window size (the C# persists its own window layout;
/// workspace persistence arrives in Phase 7).
const DEFAULT_WIDTH: i32 = 1200;
const DEFAULT_HEIGHT: i32 = 800;

/// `ComicBookNavigator.TrackCurrentPage` (the setting flips it; the
/// settings port is still open).
const TRACK_CURRENT_PAGE: bool = true;

/// Pointer distance from the top edge that reveals the chrome
/// (`AutoHideMainMenu` reveal strip).
const CHROME_REVEAL_EDGE: f64 = 16.0;

/// `HideCursorFullScreen` idle delay.
const CURSOR_HIDE_MS: u64 = 1000;

struct ReaderTab {
    /// Stable slot id — callbacks capture this, never a Vec index.
    slot: usize,
    path: PathBuf,
    page_count: usize,
    view: PageView,
    tab_widget: gtk4::Box,
}

/// The undocked reader (`ReaderForm`): one at a time, plain window.
/// The reading state stays in `ShellState.books`, keyed by slot.
struct UndockedTab {
    /// Notebook position to restore on re-dock.
    position: usize,
    window: ApplicationWindow,
    tab: ReaderTab,
}

struct ShellState {
    /// The host window (the browser shell sets it; fullscreen chrome
    /// and the Q exit ride it). `None` until docked.
    host: RefCell<Option<ApplicationWindow>>,
    header: HeaderBar,
    subtitle: Label,
    notebook: Notebook,
    app: Application,
    pool: Arc<ImagePool>,
    tabs: Vec<ReaderTab>,
    /// Reading state per slot — survives undocking and outlives the
    /// tab widget moves.
    books: HashMap<usize, ComicBook>,
    undocked: Option<UndockedTab>,
    minimal_gui: bool,
    cursor_hide_source: Option<glib::SourceId>,
    next_slot: usize,
    /// The host runs this when the last tab closes (the C# `Close`
    /// makes the browser visible again).
    on_last_tab_closed: Option<Box<dyn Fn()>>,
    /// The host runs this when undocking (`reader_visible == false`)
    /// or re-docking (`true`) — the main window swaps its stack page.
    on_view_change: Option<Box<dyn Fn(bool)>>,
    /// The host runs this on every page change of the current slot.
    on_page_change: Option<Box<dyn Fn(usize)>>,
    /// The host runs this when the visible book changes (open or
    /// slot switch) — the Pages panel rebinds.
    on_book_changed: Option<Box<dyn Fn()>>,
}

impl ShellState {
    fn position_of(&self, slot: usize) -> Option<usize> {
        self.tabs.iter().position(|t| t.slot == slot)
    }

    fn current_slot(&self) -> Option<usize> {
        self.notebook
            .current_page()
            .and_then(|pos| self.tabs.get(pos as usize))
            .map(|t| t.slot)
    }
}

/// Clone-able handle around the shell state. Callbacks hold a
/// `Weak` so the tab's `PageView` closures never form a cycle.
#[derive(Clone)]
pub struct ReaderShell {
    state: Rc<RefCell<ShellState>>,
}

/// The reader's visible pieces — the browser shell parents the
/// notebook into its reader view and packs the header's subtitle
/// where the C# shows it (the main window title area).
pub struct ReaderShellWidgets {
    notebook: Notebook,
    pub header: HeaderBar,
    subtitle: Label,
}

impl ReaderShellWidgets {
    pub fn notebook(&self) -> Notebook {
        self.notebook.clone()
    }

    /// The "Page X of Y" label — the host packs it where the C#
    /// shows it (the main window title area).
    pub fn subtitle(&self) -> Label {
        self.subtitle.clone()
    }
}

impl ReaderShell {
    /// Builds the reader pane (no window — the host docks it).
    pub fn new(app: &Application, pool: Arc<ImagePool>) -> (ReaderShell, ReaderShellWidgets) {
        // The header stays unparented when docked (the undocked
        // window is chrome-less) — the HOST packs the subtitle label
        // where the C# shows it. Do not pack it here: a widget
        // packed twice keeps its first parent.
        let header = HeaderBar::new();
        let subtitle = Label::builder().css_classes(["placeholder-label"]).build();

        let notebook = Notebook::new();
        notebook.set_vexpand(true);
        notebook.set_hexpand(true);

        let shell = ReaderShell {
            state: Rc::new(RefCell::new(ShellState {
                host: RefCell::new(None),
                header: header.clone(),
                subtitle: subtitle.clone(),
                notebook: notebook.clone(),
                app: app.clone(),
                pool,
                tabs: Vec::new(),
                books: HashMap::new(),
                undocked: None,
                minimal_gui: false,
                cursor_hide_source: None,
                next_slot: 0,
                on_last_tab_closed: None,
                on_view_change: None,
                on_page_change: None,
                on_book_changed: None,
            })),
        };

        // Tab switching (click or the Tab commands) refreshes the
        // chrome for the new slot.
        {
            let st = Rc::downgrade(&shell.state);
            notebook.connect_switch_page(move |_, _, page_num| {
                let Some(sh) = st.upgrade() else {
                    return;
                };
                ReaderShell::refresh_chrome(&sh, page_num as usize);
            });
        }

        let widgets = ReaderShellWidgets {
            notebook,
            header,
            subtitle,
        };
        (shell, widgets)
    }

    /// The browser shell docks the reader: the host window drives
    /// the fullscreen chrome, the title, and the Q exit.
    pub fn set_host(&self, window: &ApplicationWindow) {
        *self.state.borrow().host.borrow_mut() = Some(window.clone());

        // Fullscreen chrome: the reader header hides with the
        // decorations (`AutoMinimalGui` is false by default).
        {
            let st = Rc::downgrade(&self.state);
            window.connect_notify_local(Some("fullscreened"), move |win, _| {
                let Some(sh) = st.upgrade() else {
                    return;
                };
                let fullscreen = win.is_fullscreen();
                let minimal = sh.borrow().minimal_gui;
                sh.borrow().header.set_visible(!fullscreen && !minimal);
            });
        }

        // The initial grab_focus often runs while the window is not
        // yet active (late WM focus — sway, or no WM) — GTK then
        // ignores it and keys never reach the reader. Re-grab when
        // the toplevel becomes active.
        {
            let st = Rc::downgrade(&self.state);
            window.connect_notify_local(Some("is-active"), move |win, _| {
                if !win.is_active() {
                    return;
                }
                let Some(sh) = st.upgrade() else {
                    return;
                };
                let s = sh.borrow();
                let current = s.notebook.current_page();
                if let Some(tab) = s.tabs.get(current.unwrap_or(0) as usize) {
                    tab.view.widget().grab_focus();
                }
            });
        }
    }

    /// The host hook: the last tab closed → the browser view shows
    /// again (the C# `Close` reveals the browser).
    pub fn set_on_last_tab_closed<F: Fn() + 'static>(&self, f: F) {
        self.state.borrow_mut().on_last_tab_closed = Some(Box::new(f));
    }

    /// The host hook: the undock state changed — `reader_visible`
    /// tells whether the main window should show the reader page
    /// (after an undock the C# reveals the browser in the main
    /// form; after a re-dock the reader shows again).
    pub fn set_on_view_change<F: Fn(bool) + 'static>(&self, f: F) {
        self.state.borrow_mut().on_view_change = Some(Box::new(f));
    }

    /// The host hook: every page change on the current slot (the
    /// Pages panel's current-page marker).
    pub fn set_on_page_change<F: Fn(usize) + 'static>(&self, f: F) {
        self.state.borrow_mut().on_page_change = Some(Box::new(f));
    }

    /// The host hook: the visible book changed — a comic opened or
    /// the reader slot switched (the C# `ComicDisplay.BookChanged` →
    /// `pagesView.Book` rebind).
    pub fn set_on_book_changed<F: Fn() + 'static>(&self, f: F) {
        self.state.borrow_mut().on_book_changed = Some(Box::new(f));
    }

    fn fire_book_changed(state: &Rc<RefCell<ShellState>>) {
        // Immutable borrows only — the host's rebind handler calls
        // `current_comic_book` (another immutable borrow).
        if let Some(f) = state.borrow().on_book_changed.as_ref() {
            f();
        }
    }

    /// The current slot's book (the Pages panel binding).
    pub fn current_comic_book(&self) -> Option<ComicBook> {
        let s = self.state.borrow();
        let current = s.notebook.current_page()?;
        s.books.get(&(s.tabs.get(current as usize)?.slot)).cloned()
    }

    /// Focuses the current reader tab (the host's is-active handler
    /// calls this before any keypress can land).
    pub fn focus_current(&self) {
        let s = self.state.borrow();
        let current = s.notebook.current_page();
        if let Some(tab) = s.tabs.get(current.unwrap_or(0) as usize) {
            tab.view.widget().grab_focus();
        }
    }

    pub fn is_empty(&self) -> bool {
        self.state.borrow().tabs.is_empty()
    }

    /// The currently visible reader book: (file path, page count).
    /// The Pages panel binds this (the C# `ComicDisplay.Book`).
    pub fn current_book(&self) -> Option<(String, usize)> {
        let s = self.state.borrow();
        let current = s.notebook.current_page()?;
        let tab = s.tabs.get(current as usize)?;
        Some((tab.path.to_string_lossy().into_owned(), tab.page_count))
    }

    /// `ComicBookNavigator.Navigate(page, Absolute)` on the current
    /// reader slot — the Pages panel's double-click. The view handle
    /// clones out FIRST: the navigation fires the page callback,
    /// which re-enters this shell state (the RefCell lesson).
    pub fn navigate_current(&self, page: usize) {
        let view = {
            let s = self.state.borrow();
            let current = match s.notebook.current_page() {
                Some(c) => c,
                None => return,
            };
            match s.tabs.get(current as usize) {
                Some(tab) => tab.view.clone(),
                None => return,
            }
        };
        view.navigate(page);
    }

    /// The host window closes: an undocked reader docks back first
    /// (the C# `ReaderFormFormClosing`).
    pub fn shutdown(&self) {
        let mut s = self.state.borrow_mut();
        if let Some(undocked) = s.undocked.take() {
            undocked.window.close();
        }
    }

    pub fn notebook(&self) -> Notebook {
        self.state.borrow().notebook.clone()
    }

    /// Adds a comic as a new tab (the C# `OpenComic` into a free
    /// slot). A comic already open in another tab is focused instead
    /// (the C# `NavigatorManager.Open` finds the existing slot).
    pub fn open_comic(&self, path: &Path) -> anyhow::Result<()> {
        // Same-path open → switch to the existing slot (the C# `Open`
        // slot lookup by book identity).
        let existing = {
            let st = self.state.borrow();
            st.tabs.iter().position(|t| t.path == path)
        };
        if let Some(pos) = existing {
            let notebook = self.state.borrow().notebook.clone();
            notebook.set_current_page(Some(pos as u32));
            return Ok(());
        }

        let provider = cr_io::ComicProvider::open(path)
            .with_context(|| format!("Unsupported or unreadable comic: {}", path.display()))?;
        let page_count = provider.page_count();

        // The C# `ComicBookFactory.Create` (`AddToLibraryOnOpen`
        // defaults to false): a comic in the library reuses the
        // stored book — the file-info refresh and the open stamps
        // land in the database, so the resume position and read
        // progress survive the save. Anything else stays a temporary
        // session book whose reading state is not persisted (the C#
        // `AddToTemporary` books are memory-only).
        let mut book = match library::open_book(&path.to_string_lossy()) {
            Some(book) => book,
            None => {
                let mut book = ComicBook {
                    file_path: path.to_string_lossy().into_owned(),
                    ..ComicBook::default()
                };
                // `OnBookOpened` + the navigator `Opened` handler
                // (`TrackCurrentPage` gates both stamps).
                if TRACK_CURRENT_PAGE {
                    book.opened_time = cr_core::xml::scalar::CrDateTime::now();
                    book.opened_count += 1;
                    book.new_pages = 0;
                }
                book
            }
        };
        // `ProviderIndexRetrievalCompleted`: the C# fills
        // `ComicBook.Pages` from the provider index when the metadata
        // carries no page list — the Pages panel reads it.
        if book.info.pages.is_empty() {
            book.info.pages = provider
                .pages()
                .iter()
                .map(|p| cr_core::model::comic_page_info::ComicPageInfo {
                    key: Some(p.name.clone()),
                    ..Default::default()
                })
                .collect();
        }
        // Resume position and read-progress clamp to the real page
        // count. An empty page list (a broken archive) must not
        // panic — the display shows the error page instead.
        let max_page = (page_count as i32 - 1).max(0);
        let resume = book.current_page.clamp(0, max_page).max(0) as usize;
        book.last_page_read = book.last_page_read.clamp(0, max_page);
        let last_read = book.last_page_read.max(0) as usize;

        let (slot, view, tab_widget);
        {
            let mut st = self.state.borrow_mut();
            view = PageView::new(Arc::clone(&st.pool));
            slot = st.next_slot;
            st.next_slot += 1;

            // Reading-state write-back: every logical page change
            // lands in the book (`ComicBookNavigator.CurrentPage`
            // setter) and mirrors into the library book by path so
            // the state survives the save. Temporary books have no
            // library entry — the mirror is a no-op for them.
            {
                let st_weak = Rc::downgrade(&self.state);
                view.set_page_callback(Some(Box::new(move |page, count| {
                    let Some(sh) = st_weak.upgrade() else {
                        return;
                    };
                    let mut s = sh.borrow_mut();
                    if let Some(book) = s.books.get_mut(&slot) {
                        book.set_current_page(page as i32);
                        let file = book.file_path.clone();
                        library::record_page_change(&file, page as i32);
                    }
                    if s.current_slot() == Some(slot) {
                        s.subtitle.set_text(&page_subtitle(page, count));
                        // Only the bound book's turns reach the host
                        // (the C# per-item `Navigation` subscription).
                        if let Some(f) = s.on_page_change.as_ref() {
                            f(page);
                        }
                    }
                })));
            }
            // Shell commands (tab slots, undock, minimal GUI).
            {
                let st_weak = Rc::downgrade(&self.state);
                view.set_command_callback(Rc::new(move |command| {
                    let Some(sh) = st_weak.upgrade() else {
                        return;
                    };
                    match command {
                        "NextTab" => ReaderShell::switch_slot(&sh, 1),
                        "PrevTab" => ReaderShell::switch_slot(&sh, -1),
                        "ToggleUndockReader" => ReaderShell::toggle_undock(&sh),
                        "ToggleMenu" => ReaderShell::toggle_minimal_gui(&sh),
                        _ => {}
                    }
                }));
            }
            // The `Exit` command (Q) closes the whole shell — the C#
            // `ControlExit` closes the main form (docking the reader
            // back first via `ReaderFormFormClosing`).
            {
                let st_weak = Rc::downgrade(&self.state);
                view.set_exit_callback(Box::new(move || {
                    if let Some(sh) = st_weak.upgrade() {
                        if let Some(window) = sh.borrow().host.borrow().clone() {
                            window.close();
                        }
                    }
                }));
            }
            // Chrome auto-hide + fullscreen cursor auto-hide ride the
            // pointer (the shell adds its own controller — additive
            // to the widget's input handling).
            {
                let st_weak = Rc::downgrade(&self.state);
                let controller = gtk4::EventControllerMotion::new();
                let area = view.widget().clone();
                controller.connect_motion(move |_c, _x, y| {
                    let Some(sh) = st_weak.upgrade() else {
                        return;
                    };
                    ReaderShell::on_pointer_moved(&sh, area.upcast_ref(), y);
                });
                view.widget().add_controller(controller);
            }

            tab_widget = Self::build_tab_widget(&self.state, slot, &Self::window_title(path));
            let tab = ReaderTab {
                slot,
                path: path.to_path_buf(),
                page_count,
                view: view.clone(),
                tab_widget: tab_widget.clone(),
            };
            st.books.insert(slot, book);
            st.tabs.push(tab);
        }
        // Notebook mutations run outside the state borrow: appending
        // to an empty notebook selects the page synchronously and the
        // switch-page handler borrows the shell.
        let notebook = self.state.borrow().notebook.clone();
        let widget = view.widget().clone();
        notebook.append_page(&widget, Some(&tab_widget));
        view.open_with_state(provider, path, resume, last_read)
            .map_err(|e| anyhow::anyhow!(e))?;
        // Select the new tab outside the state borrow — the
        // switch-page handler borrows the shell itself.
        let last = (self.state.borrow().tabs.len() as u32).saturating_sub(1);
        notebook.set_current_page(Some(last));
        Self::fire_book_changed(&self.state);
        Ok(())
    }

    /// Closable tab caption (the C# `CanClose` file tabs): caption +
    /// close button.
    fn build_tab_widget(state: &Rc<RefCell<ShellState>>, slot: usize, caption: &str) -> gtk4::Box {
        let box_ = gtk4::Box::new(Orientation::Horizontal, 6);
        box_.append(&Label::new(Some(caption)));
        let close = gtk4::Button::from_icon_name("window-close-symbolic");
        let clicked = Rc::downgrade(state);
        close.connect_clicked(move |_| {
            if let Some(sh) = clicked.upgrade() {
                ReaderShell::close_tab(&sh, slot);
            }
        });
        box_.append(&close);
        box_
    }

    fn switch_slot(state: &Rc<RefCell<ShellState>>, dir: i32) {
        let next = {
            let st = state.borrow();
            if st.undocked.is_some() || st.tabs.len() < 2 {
                return;
            }
            let current = st.notebook.current_page().map(|p| p as i32).unwrap_or(0);
            ((current + dir).rem_euclid(st.tabs.len() as i32)) as u32
        };
        // Outside the borrow: the switch-page handler borrows the
        // shell.
        let notebook = state.borrow().notebook.clone();
        notebook.set_current_page(Some(next));
    }

    fn refresh_chrome(state: &Rc<RefCell<ShellState>>, page_num: usize) {
        let (subtitle_text, title, view, _book_page, has_book) = {
            let st = state.borrow();
            match st.tabs.get(page_num) {
                Some(tab) => {
                    let book = st.books.get(&tab.slot);
                    let page = book.map(|b| b.current_page).unwrap_or(0).max(0) as usize;
                    (
                        Some(page_subtitle(page, tab.page_count)),
                        Some(Self::window_title(&tab.path)),
                        Some(tab.view.clone()),
                        page,
                        true,
                    )
                }
                None => (None, None, None, 0, false),
            }
        };
        if let Some(text) = subtitle_text {
            state.borrow().subtitle.set_text(&text);
        }
        if let Some(title) = title {
            if let Some(window) = state.borrow().host.borrow().as_ref() {
                window.set_title(Some(&title));
            }
        }
        if let Some(view) = view {
            view.widget().grab_focus();
        }
        // The visible book changed — the host rebinds the Pages
        // panel (`ComicDisplay.BookChanged`).
        if has_book {
            Self::fire_book_changed(state);
        }
    }

    /// Closes one tab (`OpenBooks.Close`); the last close hands the
    /// view back to the browser (the host callback).
    fn close_tab(state: &Rc<RefCell<ShellState>>, slot: usize) {
        let (pos, last_tab, notebook) = {
            let mut st = state.borrow_mut();
            let Some(pos) = st.position_of(slot) else {
                return;
            };
            let _tab = st.tabs.remove(pos);
            st.books.remove(&slot);
            (pos, st.tabs.is_empty(), st.notebook.clone())
        };
        // Outside the borrow: removing the current page selects a
        // neighbor synchronously and the switch-page handler borrows
        // the shell.
        notebook.remove_page(Some(pos as u32));
        if last_tab {
            if let Some(f) = state.borrow().on_last_tab_closed.as_ref() {
                f();
            }
        } else {
            let current = state.borrow().notebook.current_page().unwrap_or(0) as usize;
            ReaderShell::refresh_chrome(state, current);
        }
    }

    /// `MainForm.ToggleUndockReader` — the current reader moves into
    /// its own window (`ReaderForm`); `D` again re-docks it. One
    /// undocked reader at a time (the C# has one `ReaderForm`).
    fn toggle_undock(state: &Rc<RefCell<ShellState>>) {
        let (notebook, redock, undock) = {
            let mut st = state.borrow_mut();
            let notebook = st.notebook.clone();
            if let Some(undocked) = st.undocked.take() {
                // Re-dock: back into the tab strip at the old
                // position.
                let position = undocked.position.min(st.tabs.len());
                let view = undocked.tab.view.clone();
                let tab_widget = undocked.tab.tab_widget.clone();
                st.tabs.insert(position, undocked.tab);
                (
                    notebook,
                    Some((position, view, tab_widget, undocked.window)),
                    None,
                )
            } else {
                let position = st.notebook.current_page().unwrap_or(0) as usize;
                if position >= st.tabs.len() {
                    return;
                }
                let tab = st.tabs.remove(position);
                let caption = Self::window_title(&tab.path);
                let view = tab.view.clone();
                let tab_widget = tab.tab_widget.clone();
                let app = st.app.clone();
                (
                    notebook,
                    None,
                    Some((position, tab, view, tab_widget, caption, app)),
                )
            }
        };
        if let Some((position, view, tab_widget, window)) = redock {
            // Unparent from the undocked window first
            // (`gtk_notebook.insert_page` asserts on a parented
            // child), then close the bare window.
            window.set_child(None::<&gtk4::Widget>);
            window.close();
            notebook.insert_page(view.widget(), Some(&tab_widget), Some(position as u32));
            notebook.set_current_page(Some(position as u32));
            let current = notebook.current_page().unwrap_or(0) as usize;
            ReaderShell::refresh_chrome(state, current);
            if let Some(f) = state.borrow().on_view_change.as_ref() {
                f(true);
            }
            return;
        }
        let Some((position, tab, view, _tab_widget, caption, app)) = undock else {
            return;
        };
        // Unparent from the notebook first (`gtk_window.set_child`
        // asserts on a parented child). All re-parenting runs outside
        // the shell borrow: the notebook mutations emit switch-page,
        // whose handler borrows the shell.
        notebook.remove_page(Some(position as u32));
        // The undocked reader is chrome-less (`ReaderForm` is a bare
        // form). Q keeps closing the whole shell via the exit
        // callback.
        let undocked_window = ApplicationWindow::builder()
            .application(&app)
            .title(&caption)
            .default_width(DEFAULT_WIDTH)
            .default_height(DEFAULT_HEIGHT)
            .build();
        undocked_window.set_child(Some(view.widget()));
        // The undocked window is not active yet at this point — the
        // grab_focus below would be ignored. Re-grab on activation
        // (same race as the main window).
        {
            let view = view.clone();
            undocked_window.connect_notify_local(Some("is-active"), move |win, _| {
                if win.is_active() {
                    view.widget().grab_focus();
                }
            });
        }
        {
            let mut st = state.borrow_mut();
            st.undocked = Some(UndockedTab {
                position,
                window: undocked_window.clone(),
                tab,
            });
        }
        if let Some(f) = state.borrow().on_view_change.as_ref() {
            f(false);
        }
        undocked_window.present();
        view.widget().grab_focus();
    }

    /// `MainForm.MinimalGui` (the K command): pins the chrome hidden.
    fn toggle_minimal_gui(state: &Rc<RefCell<ShellState>>) {
        let fullscreen = state
            .borrow()
            .host
            .borrow()
            .as_ref()
            .map(|w| w.is_fullscreen())
            .unwrap_or(false);
        let mut st = state.borrow_mut();
        st.minimal_gui = !st.minimal_gui;
        st.header.set_visible(!st.minimal_gui && !fullscreen);
    }

    /// `AutoHideMainMenu` (default true): the chrome reveals while
    /// the pointer is in the top strip and slides away elsewhere.
    /// The fullscreen cursor hides after the idle delay.
    fn on_pointer_moved(state: &Rc<RefCell<ShellState>>, area: &gtk4::Widget, y: f64) {
        let fullscreen = state
            .borrow()
            .host
            .borrow()
            .as_ref()
            .map(|w| w.is_fullscreen())
            .unwrap_or(false);
        let mut st = state.borrow_mut();
        if fullscreen {
            // Cursor auto-hide: reset the idle timer on every motion.
            // A fired one-shot's SourceId must NOT be removed (glib
            // panics on removing a finished source) — the timeout
            // clears its own slot; only a still-pending source gets
            // removed.
            if let Some(source) = st.cursor_hide_source.take() {
                source.remove();
            }
            area.set_cursor_from_name(None);
            area.set_cursor_from_name(None);
            let area = area.clone();
            let state = Rc::downgrade(state);
            st.cursor_hide_source = Some(glib::timeout_add_local(
                std::time::Duration::from_millis(CURSOR_HIDE_MS),
                move || {
                    area.set_cursor_from_name(Some("none"));
                    if let Some(st) = state.upgrade() {
                        st.borrow_mut().cursor_hide_source = None;
                    }
                    glib::ControlFlow::Break
                },
            ));
        }
        let reveal = y <= CHROME_REVEAL_EDGE;
        st.header.set_visible(reveal && !st.minimal_gui);
    }

    fn window_title(path: &Path) -> String {
        path.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "comicrust".into())
    }
}

fn page_subtitle(page: usize, page_count: usize) -> String {
    if page_count == 0 {
        "No pages".into()
    } else {
        format!("Page {} of {}", page + 1, page_count)
    }
}
