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
use gtk4::{glib, Application, ApplicationWindow, HeaderBar, Label, Notebook};

use cr_core::model::comic_book::ComicBook;
use cr_engine::image_pool::ImagePool;

use crate::library;

/// The user settings (`Program.Settings`).
fn cr_ui_settings() -> std::rc::Rc<std::cell::RefCell<cr_core::settings::Settings>> {
    library::settings()
}
use crate::reader::display::ImageFitMode;
use crate::reader::page_view::{DisplayOptions, PageLayoutMode, PageView};
use cr_core::model::enums::ImageRotation;

/// Default reader window size (the C# persists its own window layout;
/// workspace persistence arrives in Phase 7).
const DEFAULT_WIDTH: i32 = 1200;
const DEFAULT_HEIGHT: i32 = 800;

/// Pointer distance from the top edge that reveals the chrome
/// (`AutoHideMainMenu` reveal strip).
const CHROME_REVEAL_EDGE: f64 = 16.0;

struct ReaderTab {
    /// Stable slot id — callbacks capture this, never a Vec index.
    slot: usize,
    path: PathBuf,
    page_count: usize,
    view: PageView,
}

/// The undocked reader (`ReaderForm`): one at a time, plain window.
/// The reading state stays in `ShellState.books`, keyed by slot.
struct UndockedTab {
    /// Notebook position to restore on re-dock.
    position: usize,
    window: ApplicationWindow,
    tab: ReaderTab,
}

/// The Library-group command forwarder (`Rc`: the dispatch clones it
/// out before firing — the handler re-enters this shell).
type LibraryCommandFn = Rc<dyn Fn(&str)>;
/// The tab-close hook (the C# `BookClosing` — the auto Quick Review
/// gate reads the leaving book).
type BookClosingFn = Box<dyn Fn(&ComicBook)>;

/// One open slot as the workspace tab strip renders it.
pub struct TabInfo {
    pub slot: usize,
    pub caption: String,
    pub source: Option<String>,
    /// The front-cover thumbnail key (the C#
    /// `GetFrontCoverThumbnailKey` parity — one shared cache slot
    /// with the browser covers instead of a second decode).
    pub cover_key: Option<cr_image::keys::ThumbnailKey>,
    pub has_book: bool,
    pub current: bool,
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
    /// The host runs this for the Library-group reader commands
    /// (`NextComic`/`PrevComic`/`RandomComic`/`ShowBrowser`) — the
    /// C# handlers live on `MainForm` (the browser list context).
    on_library_command: Option<LibraryCommandFn>,
    /// The chrome widget that rides the reader into the undocked
    /// window (the T5 toolbar) + its docked parent box.
    undock_chrome: Option<gtk4::Widget>,
    undock_chrome_docked_parent: Option<gtk4::Box>,
    /// The host runs this whenever the chrome visibility resolves
    /// (fullscreen enter/leave, MinimalGui) — the T3 menubar rides
    /// the same visibility.
    on_chrome_change: Option<Rc<dyn Fn(bool)>>,
    /// The host runs this whenever the TAB SET changed (open/close/
    /// undock/re-dock/`AddSlot`) — the workspace tab strip rebuilds.
    on_tabs_changed: Option<Box<dyn Fn()>>,
    /// The host runs this when a tab closes, with the book that
    /// leaves (the C# `OnBookClosing` — the auto Quick Review gate).
    on_book_closing: Option<BookClosingFn>,
    /// The tab captions (`Comic.Caption`), cached per slot — the
    /// proposed-name fallback parses file names with regexes and the
    /// strip sync runs on every shell action.
    captions: RefCell<HashMap<usize, String>>,
    /// The persisted reader layout (`DisplayWorkspace.Layout` — the
    /// T14 restore seeds every new view with it; the C# keeps the
    /// values on the workspace and each new `ComicDisplayControl`
    /// copies them).
    seed: RefCell<ReaderSeed>,
}

/// One persisted reader layout family (the cr-ui types behind
/// `ReaderLayoutState`).
#[derive(Clone, Copy, Default)]
pub struct ReaderSeed {
    pub fit: Option<ImageFitMode>,
    pub layout: Option<PageLayoutMode>,
    pub rtl: Option<bool>,
    pub zoom: Option<f32>,
    pub rotation: Option<ImageRotation>,
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
        // The docked reader carries NO tabs of its own: the one
        // workspace tab strip under the menubar is the only tab UI
        // (the C# Fill shape — `MainView.tabStrip` holds the file
        // tabs; `MainToolStripVisible=false` docks the toolbar there
        // too).
        notebook.set_show_tabs(false);
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
                on_library_command: None,
                on_chrome_change: None,
                undock_chrome: None,
                undock_chrome_docked_parent: None,
                on_tabs_changed: None,
                on_book_closing: None,
                captions: RefCell::new(HashMap::new()),
                seed: RefCell::new(ReaderSeed::default()),
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
        // decorations — docked, that is the HOST header bar.
        // `AutoMinimalGui` also toggles the minimal user interface
        // with the fullscreen state (the C# `MainForm` fullscreen
        // toggle).
        {
            let st = Rc::downgrade(&self.state);
            window.connect_notify_local(Some("fullscreened"), move |win, _| {
                let Some(rc) = st.upgrade() else {
                    return;
                };
                let fullscreen = win.is_fullscreen();
                let auto_minimal = cr_ui_settings().borrow().auto_minimal_gui;
                let visible = {
                    let mut sh = rc.borrow_mut();
                    if auto_minimal {
                        sh.minimal_gui = fullscreen;
                    }
                    !fullscreen && !sh.minimal_gui
                };
                ReaderShell::apply_chrome_visibility(&rc, visible);
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

    /// The host hook: a Library-group reader command fired (the
    /// C# `OpenNextComic`/`ToggleBrowserFromReader` on MainForm).
    pub fn set_on_library_command<F: Fn(&str) + 'static>(&self, f: F) {
        self.state.borrow_mut().on_library_command = Some(Rc::new(f));
    }

    /// The host hook: a tab is closing and its book leaves (the C#
    /// `BookClosing` event; the auto Quick Review gate reads the
    /// book).
    pub fn set_on_book_closing<F: Fn(&ComicBook) + 'static>(&self, f: F) {
        self.state.borrow_mut().on_book_closing = Some(Box::new(f));
    }

    /// The number of open reader slots (`OpenBooks.Slots.Count`).
    pub fn tab_count(&self) -> usize {
        self.state.borrow().tabs.len()
    }

    /// Whether the CURRENT slot holds a book (`ComicDisplay.Book !=
    /// null` — an `AddSlot` slot stays empty and gates the reader
    /// commands off).
    pub fn has_current_book(&self) -> bool {
        let s = self.state.borrow();
        match s.current_slot() {
            Some(slot) => s.books.contains_key(&slot),
            None => false,
        }
    }

    /// The number of slots that hold a book (`OpenBooks.OpenCount` —
    /// the `flag2` of the C# `OnGuiVisibilities` rules).
    pub fn open_book_count(&self) -> usize {
        let s = self.state.borrow();
        s.tabs
            .iter()
            .filter(|t| s.books.contains_key(&t.slot))
            .count()
    }

    /// The open books' file paths (`books.OpenFiles` — the exit-time
    /// `Settings.LastOpenFiles` source). Empty slots are skipped.
    pub fn open_files(&self) -> Vec<String> {
        self.state
            .borrow()
            .tabs
            .iter()
            .filter(|t| !t.path.as_os_str().is_empty())
            .map(|t| t.path.to_string_lossy().into_owned())
            .collect()
    }

    /// One strip row per open slot (the workspace tab strip's comic
    /// tabs): caption, cover source, bold rule.
    pub fn tab_infos(&self) -> Vec<TabInfo> {
        let s = self.state.borrow();
        let current = s.current_slot();
        s.tabs
            .iter()
            .map(|t| {
                let book = s.books.get(&t.slot);
                let has_book = book.is_some();
                let mut captions = s.captions.borrow_mut();
                let caption = match (has_book, captions.get(&t.slot)) {
                    (true, Some(text)) => text.clone(),
                    (true, None) => {
                        let text = book
                            .map(cr_engine::display_text::caption)
                            .unwrap_or_default();
                        captions.insert(t.slot, text.clone());
                        text
                    }
                    (false, _) => String::new(),
                };
                TabInfo {
                    slot: t.slot,
                    caption,
                    source: (!t.path.as_os_str().is_empty())
                        .then(|| t.path.to_string_lossy().into_owned()),
                    cover_key: book.and_then(|b| {
                        (!b.file_path.is_empty())
                            .then(|| cr_engine::image_pool::front_cover_thumbnail_key(b))
                    }),
                    has_book,
                    current: current == Some(t.slot),
                }
            })
            .collect()
    }

    /// Forwards a shell command into the CURRENT reader view
    /// (`ComicDisplay` command parity — the shell actions route
    /// here). The view handle clones out first: the dispatch can
    /// re-enter this shell (the RefCell lesson).
    pub fn dispatch_current(&self, id: &str) {
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
        view.run_command(id);
    }

    /// Closes the current tab (`OpenBooks.Close` — the Ctrl+X
    /// command; the tab close button routes to the same slot close).
    pub fn close_current_tab(&self) {
        let slot = {
            let s = self.state.borrow();
            match s.current_slot() {
                Some(slot) => slot,
                None => return,
            }
        };
        ReaderShell::close_tab(&self.state, slot);
    }

    /// Closes ONE slot (the workspace tab strip's close button —
    /// `btn_CloseClick`).
    pub fn close_slot(&self, slot: usize) {
        ReaderShell::close_tab(&self.state, slot);
    }

    /// `OpenBooks.NextSlot`/`PreviousSlot` (the Tab commands).
    /// Returns whether the slot switched (the shell reveals the
    /// reader then — the C# `ShowView(i)` shows the comic viewer).
    pub fn cycle_slot(&self, dir: i32) -> bool {
        let next = {
            let st = self.state.borrow();
            if st.undocked.is_some() || st.tabs.len() < 2 {
                return false;
            }
            let current = st.notebook.current_page().map(|p| p as i32).unwrap_or(0);
            ((current + dir).rem_euclid(st.tabs.len() as i32)) as u32
        };
        if self.state.borrow().notebook.current_page() == Some(next) {
            return false;
        }
        // Outside the borrow: the switch-page handler borrows the
        // shell.
        let notebook = self.state.borrow().notebook.clone();
        notebook.set_current_page(Some(next));
        true
    }

    /// `OpenBooks.AddSlot` + `CurrentSlot = last` (the `+` tab): a
    /// new EMPTY slot selects and shows a blank reader view. No
    /// dialog, no overlay (the ported behavior — recorded deviation
    /// from the C#, whose empty slot hosts QuickOpen).
    pub fn add_empty_slot(&self) {
        let (slot, view);
        {
            let mut st = self.state.borrow_mut();
            view = PageView::new(Arc::clone(&st.pool));
            {
                let s = cr_ui_settings();
                let (wheel, browse, wall) = {
                    let b = s.borrow();
                    (
                        b.mouse_wheel_speed,
                        b.scrolling_does_browse,
                        b.page_change_delay,
                    )
                };
                drop(s);
                view.apply_display_settings(wheel, browse, wall);
                // The persisted layout seeds the fresh view (the T14
                // `DisplayWorkspace.Layout` copy).
                let seed = *st.seed.borrow();
                Self::apply_seed(&view, &seed);
            }
            slot = st.next_slot;
            st.next_slot += 1;
            st.tabs.push(ReaderTab {
                slot,
                path: PathBuf::new(),
                page_count: 0,
                view: view.clone(),
            });
        }
        // Notebook mutations run outside the state borrow (the
        // switch-page handler borrows the shell).
        let notebook = self.state.borrow().notebook.clone();
        let last = (self.state.borrow().tabs.len() as u32).saturating_sub(1);
        notebook.append_page(view.widget(), None::<&gtk4::Widget>);
        notebook.set_current_page(Some(last));
        Self::fire_tabs_changed(&self.state);
    }

    /// Closes every tab (`OpenBooks.CloseAll`); the last close hands
    /// the view back to the browser through the host callback.
    pub fn close_all_tabs(&self) {
        while !self.is_empty() {
            self.close_current_tab();
        }
    }

    /// The current view's fit mode (the shell radio sync).
    pub fn current_fit_mode(&self) -> Option<ImageFitMode> {
        let s = self.state.borrow();
        let current = s.notebook.current_page()?;
        Some(s.tabs.get(current as usize)?.view.fit_mode())
    }

    /// The current view's page layout (the shell radio sync).
    pub fn current_page_layout(&self) -> Option<PageLayoutMode> {
        let s = self.state.borrow();
        let current = s.notebook.current_page()?;
        Some(s.tabs.get(current as usize)?.view.page_layout())
    }

    /// The current view's RTL state (the shell check sync).
    pub fn current_rtl(&self) -> Option<bool> {
        let s = self.state.borrow();
        let current = s.notebook.current_page()?;
        Some(s.tabs.get(current as usize)?.view.rtl())
    }

    /// The current view's auto-scroll state (the shell check sync).
    pub fn current_auto_scrolling(&self) -> Option<bool> {
        let s = self.state.borrow();
        let current = s.notebook.current_page()?;
        Some(s.tabs.get(current as usize)?.view.auto_scrolling())
    }

    /// The current view's Two Page Auto Scrolling state.
    pub fn current_two_page_navigation(&self) -> Option<bool> {
        let s = self.state.borrow();
        let current = s.notebook.current_page()?;
        Some(s.tabs.get(current as usize)?.view.two_page_navigation())
    }

    /// The current view's Autorotate state.
    pub fn current_auto_rotate(&self) -> Option<bool> {
        let s = self.state.borrow();
        let current = s.notebook.current_page()?;
        Some(s.tabs.get(current as usize)?.view.auto_rotate())
    }

    /// The current view's zoom (`ComicDisplay.ImageZoom`).
    pub fn current_zoom(&self) -> Option<f32> {
        let s = self.state.borrow();
        let current = s.notebook.current_page()?;
        Some(s.tabs.get(current as usize)?.view.zoom())
    }

    /// The current view's rotation.
    pub fn current_rotation(&self) -> Option<cr_core::model::enums::ImageRotation> {
        let s = self.state.borrow();
        let current = s.notebook.current_page()?;
        Some(s.tabs.get(current as usize)?.view.rotation())
    }

    /// The current view's magnifier state.
    pub fn current_magnifier(&self) -> Option<bool> {
        let s = self.state.borrow();
        let current = s.notebook.current_page()?;
        Some(s.tabs.get(current as usize)?.view.magnifier_visible())
    }

    /// `MainForm.MinimalGui`.
    pub fn is_minimal_gui(&self) -> bool {
        self.state.borrow().minimal_gui
    }

    /// `MainForm.ReaderUndocked`.
    pub fn is_undocked(&self) -> bool {
        self.state.borrow().undocked.is_some()
    }

    /// The fullscreen state of the window the visible view lives in
    /// (an undocked reader fullscreens its own window).
    pub fn is_fullscreen(&self) -> bool {
        let s = self.state.borrow();
        let window = if let Some(undocked) = s.undocked.as_ref() {
            Some(undocked.window.clone())
        } else {
            s.host.borrow().clone()
        };
        drop(s);
        window.map(|w| w.is_fullscreen()).unwrap_or(false)
    }

    /// The host runs this whenever the chrome visibility resolves —
    /// the T3 menubar rides the same visibility.
    pub fn set_on_chrome_change<F: Fn(bool) + 'static>(&self, f: F) {
        self.state.borrow_mut().on_chrome_change = Some(Rc::new(f));
    }

    /// The host runs this whenever the tab set changed (open/close/
    /// undock/re-dock/`AddSlot`) — the workspace tab strip rebuilds.
    pub fn set_on_tabs_changed<F: Fn() + 'static>(&self, f: F) {
        self.state.borrow_mut().on_tabs_changed = Some(Box::new(f));
    }

    fn fire_tabs_changed(state: &Rc<RefCell<ShellState>>) {
        if let Some(f) = state.borrow().on_tabs_changed.as_ref() {
            f();
        }
    }

    /// The chrome widget that rides the reader into the undocked
    /// window (the T5 toolbar — the C# ReaderForm keeps the strip):
    /// `docked_parent` is where it lives while docked; the undock
    /// moves it above the view, the re-dock puts it back.
    pub fn set_undock_chrome(&self, widget: gtk4::Widget, docked_parent: gtk4::Box) {
        let mut s = self.state.borrow_mut();
        s.undock_chrome = Some(widget);
        s.undock_chrome_docked_parent = Some(docked_parent);
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

    /// The open reader slots (`OpenBooks.Slots`) with the C# slot
    /// captions (`GetSlotCaption` → `Comic.Caption`) — the File ▸
    /// Open Books fill. An `AddSlot` slot has no book and no caption.
    pub fn open_tabs(&self) -> Vec<(usize, String)> {
        let s = self.state.borrow();
        s.tabs
            .iter()
            .map(|t| {
                let caption = match s.books.get(&t.slot) {
                    Some(book) => cr_engine::display_text::caption(book),
                    None if t.path.as_os_str().is_empty() => String::new(),
                    None => Self::window_title(&t.path),
                };
                (t.slot, caption)
            })
            .collect()
    }

    /// The current reader slot id (the Open Books check + bookmark
    /// context).
    pub fn current_slot_id(&self) -> Option<usize> {
        self.state.borrow().current_slot()
    }

    /// `OpenBooks.CurrentSlot = i` — switches the notebook to the
    /// tab with that slot id.
    pub fn switch_to_slot(&self, slot: usize) {
        let (pos, notebook) = match self.state.borrow().position_of(slot) {
            Some(pos) => (pos as u32, self.state.borrow().notebook.clone()),
            None => return,
        };
        notebook.set_current_page(Some(pos));
    }

    /// The current view's display page (the bookmark/menu context).
    pub fn current_display_page(&self) -> Option<usize> {
        let s = self.state.borrow();
        let current = s.notebook.current_page()?;
        Some(s.tabs.get(current as usize)?.view.current_page())
    }

    /// The provider page index behind a display position of the
    /// CURRENT slot's view (`ComicBookNavigator.CurrentPage` space).
    pub fn provider_index_of_display(&self, display: usize) -> Option<usize> {
        let s = self.state.borrow();
        let current = s.notebook.current_page()?;
        s.tabs
            .get(current as usize)?
            .view
            .provider_index_of(display)
    }

    /// The display position that shows a provider page.
    pub fn display_of_provider(&self, provider: usize) -> Option<usize> {
        let s = self.state.borrow();
        let current = s.notebook.current_page()?;
        s.tabs
            .get(current as usize)?
            .view
            .display_of_provider(provider)
    }

    /// Mutates the CURRENT slot's book in place (`Book.Comic` edits —
    /// the page editor family) and returns the edited clone for the
    /// host. `None` without an open comic.
    pub fn edit_current_book<F: FnOnce(&mut ComicBook)>(&self, f: F) -> Option<ComicBook> {
        let mut s = self.state.borrow_mut();
        let current = s.notebook.current_page()?;
        let slot = s.tabs.get(current as usize)?.slot;
        // The editor can rename the comic — the cached tab caption
        // re-computes on the next strip sync. (Before the books
        // borrow: the RefMut guard deref blocks field-precise
        // borrows.)
        s.captions.borrow_mut().remove(&slot);
        let book = s.books.get_mut(&slot)?;
        f(book);
        Some(book.clone())
    }

    /// `ComicDisplay.DisplayPreviousBookmarkedPage`/
    /// `DisplayNextBookmarkedPage`: seek over the CURRENT book's
    /// bookmarked pages (provider space — the C#
    /// `ComicBookNavigator.SeekBookmark` walks `Comic.Pages`) and
    /// navigate through the display sequence. A bookmark on a
    /// Deleted page has no display position and is skipped (the
    /// C# navigates it in provider space — recorded deviation).
    pub fn bookmark_nav(&self, dir: i32) {
        let Some(book) = self.current_comic_book() else {
            return;
        };
        let Some(display) = self.current_display_page() else {
            return;
        };
        let Some(current) = self.provider_index_of_display(display) else {
            return;
        };
        let next = book.info.seek_bookmark(current as i32 + dir, dir);
        if next < 0 {
            return;
        }
        let Some(target) = self.display_of_provider(next as usize) else {
            return;
        };
        self.navigate_current(target);
    }

    /// Whether a bookmark exists before (`dir < 0`) / after (`dir >
    /// 0`) the current page (`CanNavigateBookmark` parity).
    pub fn can_navigate_bookmark(&self, dir: i32) -> bool {
        let Some(book) = self.current_comic_book() else {
            return false;
        };
        let Some(display) = self.current_display_page() else {
            return false;
        };
        let Some(current) = self.provider_index_of_display(display) else {
            return false;
        };
        book.info.seek_bookmark(current as i32 + dir, dir) >= 0
    }

    /// The Y page-rotation commands (`GetPageEditor().Rotation`
    /// write-through): the view applies + re-decodes, the session
    /// book and the library entry mirror (`apply_edited` gates the
    /// file write), and the Pages panel rebinds.
    pub fn page_rotate_current(&self, right: bool) {
        let Some(view) = self.current_view() else {
            return;
        };
        view.page_rotate(right);
        let display = view.current_page();
        let (Some(provider), rot) = (
            view.provider_index_of(display),
            view.page_rotation_of(display),
        ) else {
            return;
        };
        if let Some(book) = self.edit_current_book(|b| b.info.update_page_rotation(provider, rot)) {
            library::apply_edited(&book);
        }
        ReaderShell::fire_book_changed(&self.state);
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

    /// Re-applies the display settings to every open reader view
    /// (the C# `UpdateSettings` runs on `SettingsChanged` after OK).
    pub fn apply_settings_to_open_views(&self) {
        let s = self.state.borrow();
        let (wheel, browse, wall) = {
            let set = cr_ui_settings();
            let b = set.borrow();
            (
                b.mouse_wheel_speed,
                b.scrolling_does_browse,
                b.page_change_delay,
            )
        };
        for tab in &s.tabs {
            tab.view.apply_display_settings(wheel, browse, wall);
        }
    }

    /// `SetWorkspaceDisplayOptions` parity: push the workspace
    /// display options onto every open view (the C# has one
    /// `ComicDisplay`; the port has one view per book slot — the
    /// options are workspace-scoped, so all of them follow). Each
    /// apply also records the session copy.
    pub fn apply_display_options_all(&self, opts: &DisplayOptions) {
        let s = self.state.borrow();
        for tab in &s.tabs {
            tab.view.apply_display_options(opts);
        }
    }

    /// Stores the persisted reader layout (the T14 restore; every
    /// view created AFTER this seeds with it — the C# workspace
    /// `BookPageLayout` shape).
    pub fn set_reader_seed(&self, seed: ReaderSeed) {
        self.state.borrow_mut().seed.replace(seed);
    }

    /// Applies the seed to a fresh view (both creation paths call
    /// this right after `apply_display_settings`).
    fn apply_seed(view: &PageView, seed: &ReaderSeed) {
        if let Some(fit) = seed.fit {
            view.set_fit_mode(fit);
        }
        if let Some(layout) = seed.layout {
            view.set_page_layout(layout);
        }
        if let Some(rtl) = seed.rtl {
            view.set_rtl(rtl);
        }
        if let Some(zoom) = seed.zoom {
            view.zoom_to(zoom.clamp(
                crate::reader::page_view::MINIMUM_ZOOM,
                crate::reader::page_view::MAXIMUM_ZOOM,
            ));
        }
        if let Some(rotation) = seed.rotation {
            view.set_rotation(rotation);
        }
    }

    /// The currently visible reader book: (file path, page count).
    /// The Pages panel binds this (the C# `ComicDisplay.Book`).
    pub fn current_book(&self) -> Option<(String, usize)> {
        let s = self.state.borrow();
        let current = s.notebook.current_page()?;
        let tab = s.tabs.get(current as usize)?;
        Some((tab.path.to_string_lossy().into_owned(), tab.page_count))
    }

    /// `MainForm.ToggleZoom` on the current reader slot.
    pub fn toggle_zoom_current(&self) {
        let Some(view) = self.current_view() else {
            return;
        };
        view.toggle_zoom();
    }

    /// A Zoom preset on the current reader slot (`ImageZoom = v`).
    pub fn zoom_current(&self, zoom: f32) {
        let Some(view) = self.current_view() else {
            return;
        };
        view.zoom_to(zoom);
    }

    /// The current slot's view handle (cloned out before any
    /// callback fires — the RefCell lesson).
    pub fn current_view(&self) -> Option<PageView> {
        let s = self.state.borrow();
        let current = s.notebook.current_page()?;
        s.tabs.get(current as usize).map(|t| t.view.clone())
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
        self.open_comic_at(path, false, 0)
    }

    /// The `books.Open(file, newSlot, page)` knobs: `new_slot` skips
    /// the same-path slot focus and always opens a fresh tab (the
    /// second-instance handoff `newSlot: true`), and a positive
    /// `page` (0-based) opens there instead of the resume position.
    pub fn open_comic_at(&self, path: &Path, new_slot: bool, page: i32) -> anyhow::Result<()> {
        // Same-path open → switch to the existing slot (the C# `Open`
        // slot lookup by book identity).
        if !new_slot {
            let existing = {
                let st = self.state.borrow();
                st.tabs.iter().position(|t| t.path == path)
            };
            if let Some(pos) = existing {
                let notebook = self.state.borrow().notebook.clone();
                notebook.set_current_page(Some(pos as u32));
                return Ok(());
            }
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
                // `CreateBookOption.AddToTemporary` → `ComicBook
                // .Create(file, options)` → `RefreshInfoFromFile` (the
                // C# ComicBookFactory.cs:95 shape): the temporary book
                // carries the ComicInfo.xml/MetronInfo.xml metadata
                // too — the open provider serves the read.
                cr_engine::scanner::apply_info_chain(&mut book, &provider);
                // `OnBookOpened` + the navigator `Opened` handler
                // (`TrackCurrentPage` gates both stamps — the setting).
                let track = cr_ui_settings().borrow().track_current_page;
                if track {
                    book.opened_time = cr_core::xml::scalar::CrDateTime::now();
                    book.opened_count += 1;
                    book.new_pages = 0;
                }
                book
            }
        };
        // `ProviderIndexRetrievalCompleted`: PageCount comes from the
        // provider and the stored page entries OVERLAY it (a partial
        // metadata list does not shrink the display — the C#
        // `GetPage(i)` returns the entry or a default).
        book.info.pages = crate::pages::merged_page_entries(&book, &provider);
        // The display sequence (the C# `GetPageList` with the default
        // PageFilter = All: Deleted pages drop; reads resolve by the
        // entry ImageIndex). Books without page entries keep the
        // 1:1 sequence.
        let sequence: Option<Vec<usize>> = if book.info.pages.is_empty() {
            None
        } else {
            let seq: Vec<usize> = book
                .info
                .pages
                .iter()
                .enumerate()
                .filter(|(_, p)| p.page_type != cr_core::model::enums::ComicPageType(1024))
                .map(|(i, p)| {
                    let idx = p.image_index();
                    if idx >= 0 {
                        idx as usize
                    } else {
                        i
                    }
                })
                .collect();
            // All-deleted edge: fall back to 1:1 (the C# tolerates
            // empty, our reader shows the error page per the T5
            // lesson — keep a page).
            if seq.is_empty() {
                None
            } else {
                Some(seq)
            }
        };
        let display_count = sequence.as_ref().map_or(page_count, |s| s.len());
        // Resume position and read-progress clamp to the DISPLAY
        // count. An empty page list (a broken archive) must not
        // panic — the display shows the error page instead. The
        // `books.Open` page parameter replaces the resume position
        // (the C# passes `Math.Max(0, page - 1)` from the `-p`
        // switch).
        let max_page = (display_count as i32 - 1).max(0);
        let resume = if page > 0 {
            page.clamp(0, max_page) as usize
        } else {
            book.current_page.clamp(0, max_page).max(0) as usize
        };
        book.last_page_read = book.last_page_read.clamp(0, max_page);
        let last_read = book.last_page_read.max(0) as usize;

        // Seed the stored per-page rotations (`ComicPageInfo.Rotation`
        // — the C# render pipeline reads them per page): DISPLAY-keyed
        // through the sequence.
        let stored_rotations: HashMap<usize, cr_core::model::enums::ImageRotation> = book
            .info
            .pages
            .iter()
            .enumerate()
            .filter(|(_, p)| p.rotation != cr_core::model::enums::ImageRotation::None)
            .filter_map(|(i, p)| {
                let display = match &sequence {
                    Some(seq) => seq.iter().position(|&x| x == i)?,
                    None => i,
                };
                Some((display, p.rotation))
            })
            .collect();

        let (slot, view);
        {
            let mut st = self.state.borrow_mut();
            view = PageView::new(Arc::clone(&st.pool));
            slot = st.next_slot;
            st.next_slot += 1;

            // The `MainForm.UpdateSettings` display copy: wheel speed,
            // browse-on-scroll, and the page wall.
            {
                let s = cr_ui_settings();
                let (wheel, browse, wall) = {
                    let b = s.borrow();
                    (
                        b.mouse_wheel_speed,
                        b.scrolling_does_browse,
                        b.page_change_delay,
                    )
                };
                drop(s);
                view.apply_display_settings(wheel, browse, wall);
                // The book's color adjustment rides the page keys
                // (`ComicDisplay` renders with `book.ColorAdjustment`).
                view.set_base_adjustment(book.color_adjustment);
                // The persisted layout seeds the fresh view (the T14
                // `DisplayWorkspace.Layout` copy).
                let seed = *st.seed.borrow();
                Self::apply_seed(&view, &seed);
            }

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
                        // Bookmark navigation — the book copy lives
                        // in this shell (`Comic.Pages` seek + the
                        // display-sequence navigation).
                        "MoveToPrevBookmark" | "MoveToNextBookmark" => {
                            let dir = if command == "MoveToNextBookmark" {
                                1
                            } else {
                                -1
                            };
                            ReaderShell { state: sh.clone() }.bookmark_nav(dir);
                        }
                        // The Y page rotations write through into the
                        // book (`GetPageEditor().Rotation` setter
                        // parity) — the view applies + the session/
                        // library copies mirror.
                        "PageRotateC" | "PageRotateCC" => {
                            ReaderShell { state: sh.clone() }
                                .page_rotate_current(command == "PageRotateC");
                        }
                        // Library-group commands — the C# handlers
                        // are MainForm methods (the browser list
                        // context); the host (browser shell) owns
                        // them.
                        "NextComic" | "PrevComic" | "RandomComic" | "ShowBrowser" => {
                            let f = sh.borrow().on_library_command.clone();
                            if let Some(f) = f {
                                f(command);
                            }
                        }
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

            let tab = ReaderTab {
                slot,
                path: path.to_path_buf(),
                page_count,
                view: view.clone(),
            };
            st.books.insert(slot, book);
            st.tabs.push(tab);
        }
        // Notebook mutations run outside the state borrow: appending
        // to an empty notebook selects the page synchronously and the
        // switch-page handler borrows the shell.
        let notebook = self.state.borrow().notebook.clone();
        let widget = view.widget().clone();
        // The tab label is the strip's job now — the notebook tabs
        // stay hidden.
        notebook.append_page(&widget, None::<&gtk4::Widget>);
        view.open_with_sequence(provider, path, sequence, resume, last_read)
            .map_err(|e| anyhow::anyhow!(e))?;
        // Apply the stored rotations (after the open — the map keys
        // are the DISPLAY positions the view tracks).
        view.set_stored_page_rotations(stored_rotations);
        // Select the new tab outside the state borrow — the
        // switch-page handler borrows the shell itself.
        let last = (self.state.borrow().tabs.len() as u32).saturating_sub(1);
        notebook.set_current_page(Some(last));
        Self::fire_tabs_changed(&self.state);
        Self::fire_book_changed(&self.state);
        Ok(())
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
        let (subtitle_text, title, view) = {
            let st = state.borrow();
            match st.tabs.get(page_num) {
                Some(tab) => match st.books.get(&tab.slot) {
                    Some(book) => {
                        let page = book.current_page.max(0) as usize;
                        (
                            Some(page_subtitle(page, tab.page_count)),
                            Some(Self::window_title(&tab.path)),
                            Some(tab.view.clone()),
                        )
                    }
                    // The `AddSlot` empty slot: no book, no subtitle.
                    None => (
                        Some(String::new()),
                        Some("comicrust".to_string()),
                        Some(tab.view.clone()),
                    ),
                },
                None => (None, None, None),
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
        // The visible book changed (possibly to none) — the host
        // rebinds the Pages panel (`ComicDisplay.BookChanged`).
        Self::fire_book_changed(state);
    }

    /// Closes one tab (`OpenBooks.Close`); the last close hands the
    /// view back to the browser (the host callback).
    fn close_tab(state: &Rc<RefCell<ShellState>>, slot: usize) {
        let (pos, last_tab, notebook, closing_book) = {
            let mut st = state.borrow_mut();
            let Some(pos) = st.position_of(slot) else {
                return;
            };
            let _tab = st.tabs.remove(pos);
            let closing_book = st.books.get(&slot).cloned();
            st.books.remove(&slot);
            st.captions.borrow_mut().remove(&slot);
            (pos, st.tabs.is_empty(), st.notebook.clone(), closing_book)
        };
        // Outside the borrow: removing the current page selects a
        // neighbor synchronously and the switch-page handler borrows
        // the shell.
        notebook.remove_page(Some(pos as u32));
        // The BookClosing hook fires after the state borrow drops
        // (the handler may re-enter the shell — the RefCell lesson).
        if let Some(book) = closing_book {
            if let Some(f) = state.borrow().on_book_closing.as_ref() {
                f(&book);
            }
        }
        Self::fire_tabs_changed(state);
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
                st.tabs.insert(position, undocked.tab);
                (notebook, Some((position, view, undocked.window)), None)
            } else {
                let position = st.notebook.current_page().unwrap_or(0) as usize;
                if position >= st.tabs.len() {
                    return;
                }
                let tab = st.tabs.remove(position);
                let caption = Self::window_title(&tab.path);
                let view = tab.view.clone();
                let app = st.app.clone();
                (notebook, None, Some((position, tab, view, caption, app)))
            }
        };
        if let Some((position, view, window)) = redock {
            // Unparent the chrome (the T5 toolbar) from the undocked
            // box back into the docked parent first, then unparent
            // the view (`gtk_notebook.insert_page` asserts on a
            // parented child) and close the bare window.
            let (chrome, docked_parent) = {
                let st = state.borrow();
                (
                    st.undock_chrome.clone(),
                    st.undock_chrome_docked_parent.clone(),
                )
            };
            if let (Some(chrome), Some(docked_parent)) = (chrome, docked_parent) {
                if let Some(box_) = window.child().and_downcast::<gtk4::Box>() {
                    box_.remove(&chrome);
                }
                docked_parent.append(&chrome);
            }
            window.set_child(None::<&gtk4::Widget>);
            window.close();
            notebook.insert_page(view.widget(), None::<&gtk4::Widget>, Some(position as u32));
            notebook.set_current_page(Some(position as u32));
            let current = notebook.current_page().unwrap_or(0) as usize;
            ReaderShell::refresh_chrome(state, current);
            Self::fire_tabs_changed(state);
            if let Some(f) = state.borrow().on_view_change.as_ref() {
                f(true);
            }
            return;
        }
        let Some((position, tab, view, caption, app)) = undock else {
            return;
        };
        // Unparent from the notebook first (`gtk_window.set_child`
        // asserts on a parented child). All re-parenting runs outside
        // the shell borrow: the notebook mutations emit switch-page,
        // whose handler borrows the shell.
        notebook.remove_page(Some(position as u32));
        // The chrome (the T5 toolbar) rides into the undocked window
        // above the view (the C# ReaderForm keeps the strip).
        let undocked_window = ApplicationWindow::builder()
            .application(&app)
            .title(&caption)
            .default_width(DEFAULT_WIDTH)
            .default_height(DEFAULT_HEIGHT)
            .build();
        let undocked_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        let (chrome, docked_parent) = {
            let st = state.borrow();
            (
                st.undock_chrome.clone(),
                st.undock_chrome_docked_parent.clone(),
            )
        };
        if let (Some(chrome), Some(docked_parent)) = (chrome, docked_parent) {
            docked_parent.remove(&chrome);
            undocked_box.append(&chrome);
        }
        undocked_box.append(view.widget());
        undocked_window.set_child(Some(&undocked_box));
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
        Self::fire_tabs_changed(state);
        if let Some(f) = state.borrow().on_view_change.as_ref() {
            f(false);
        }
        undocked_window.present();
        view.widget().grab_focus();
    }

    /// `MainForm.MinimalGui` (the K command): pins the chrome hidden.
    /// Docked, the chrome is the HOST window's header bar (the
    /// reader's own header only exists inside the undocked shape);
    /// the C# hides menubar + tab bars + status bar
    /// (`MainForm.cs:3658-3716`) — the T3/T8 bars join when they
    /// exist.
    fn toggle_minimal_gui(state: &Rc<RefCell<ShellState>>) {
        let fullscreen = state
            .borrow()
            .host
            .borrow()
            .as_ref()
            .map(|w| w.is_fullscreen())
            .unwrap_or(false);
        let visible = {
            let mut st = state.borrow_mut();
            st.minimal_gui = !st.minimal_gui;
            !st.minimal_gui && !fullscreen
        };
        ReaderShell::apply_chrome_visibility(state, visible);
    }

    /// Applies one visibility state to the reader header and — when
    /// docked — the host window's header bar. The chrome-change
    /// callback fires AFTER the state borrow drops (the callback
    /// re-enters this state — the Phase 3 lesson).
    fn apply_chrome_visibility(state: &Rc<RefCell<ShellState>>, visible: bool) {
        let (header, host, docked, callback) = {
            let st = state.borrow();
            let host = st.host.borrow().clone();
            (
                st.header.clone(),
                host,
                st.undocked.is_none(),
                st.on_chrome_change.clone(),
            )
        };
        header.set_visible(visible);
        if docked {
            if let Some(bar) = host.as_ref().and_then(|w| w.titlebar()) {
                bar.set_visible(visible);
            }
        }
        if let Some(f) = callback {
            f(visible);
        }
    }

    /// `AutoHideMainMenu` (default true): the chrome reveals while
    /// the pointer is in the top strip and slides away elsewhere.
    /// The fullscreen cursor hides after the idle delay
    /// (`Settings.HideCursorFullScreen` gates it; the delay is
    /// `ExtendedSettings.AutoHideCursorDuration`).
    fn on_pointer_moved(state: &Rc<RefCell<ShellState>>, area: &gtk4::Widget, y: f64) {
        // The fullscreen state of the window the view lives in (the
        // undocked reader fullscreens its own window, not the host).
        let fullscreen = area
            .root()
            .and_downcast::<gtk4::Window>()
            .map(|w| w.is_fullscreen())
            .unwrap_or(false);
        if fullscreen {
            let hide_cursor = cr_ui_settings().borrow().hide_cursor_full_screen;
            let hide_ms =
                cr_core::settings::ExtendedSettings::global().auto_hide_cursor_duration as u64;
            if hide_cursor {
                let source = state.borrow_mut().cursor_hide_source.take();
                // Cursor auto-hide: reset the idle timer on every
                // motion. A fired one-shot's SourceId must NOT be
                // removed (glib panics on removing a finished source)
                // — the timeout clears its own slot; only a
                // still-pending source gets removed.
                if let Some(source) = source {
                    source.remove();
                }
                area.set_cursor_from_name(None);
                area.set_cursor_from_name(None);
                let area = area.clone();
                let weak = Rc::downgrade(state);
                let source =
                    glib::timeout_add_local(std::time::Duration::from_millis(hide_ms), move || {
                        area.set_cursor_from_name(Some("none"));
                        if let Some(st) = weak.upgrade() {
                            st.borrow_mut().cursor_hide_source = None;
                        }
                        glib::ControlFlow::Break
                    });
                state.borrow_mut().cursor_hide_source = Some(source);
            }
            // The top strip reveals the chrome (docked: the host
            // header bar); a PINNED MinimalGui stays hidden (the C#
            // reveal serves the auto-hide menu, not MinimalGui).
            let minimal = state.borrow().minimal_gui;
            ReaderShell::apply_chrome_visibility(state, y <= CHROME_REVEAL_EDGE && !minimal);
        } else {
            let st = state.borrow_mut();
            let reveal = y <= CHROME_REVEAL_EDGE;
            st.header.set_visible(reveal && !st.minimal_gui);
        }
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
