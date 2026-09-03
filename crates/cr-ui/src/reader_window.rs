//! The reader window shell around the page views — one window, one
//! tab per open comic (the C# `MainForm` file tabs over the
//! `OpenBooks` slots; the browser arrives in Phase 4).
//!
//! Shell duties ported from `MainForm`:
//! - reading-state write-back (`OnBookOpened` stamps
//!   `OpenedTime`/`OpenedCount`; `TrackCurrentPage` mirrors every
//!   page change into `ComicBook.CurrentPage`/`LastPageRead`). State
//!   lives in the session — the ComicDb wiring is Phase 4.
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
    window: ApplicationWindow,
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
pub struct ReaderWindow {
    state: Rc<RefCell<ShellState>>,
}

impl ReaderWindow {
    /// Opens `path` as the first tab. Errors surface to the caller
    /// (the app shows a dialog) — the C# treats an unopenable comic
    /// the same way.
    pub fn open(app: &Application, path: &Path) -> anyhow::Result<ReaderWindow> {
        let window = ApplicationWindow::builder()
            .application(app)
            .title(Self::window_title(path))
            .default_width(DEFAULT_WIDTH)
            .default_height(DEFAULT_HEIGHT)
            .css_classes(["reader-window"])
            .build();

        let header = HeaderBar::new();
        let subtitle = Label::builder().css_classes(["placeholder-label"]).build();
        header.pack_end(&subtitle);
        window.set_titlebar(Some(&header));

        let notebook = Notebook::new();
        notebook.set_vexpand(true);
        notebook.set_hexpand(true);
        window.set_child(Some(&notebook));

        // One render pool per reader shell (memory-only until the
        // settings port decides the cache location) — the C#
        // `Program.ImagePool` is global; per-window is the Phase 3
        // scope.
        let pool = Arc::new(ImagePool::new(None));

        let shell = ReaderWindow {
            state: Rc::new(RefCell::new(ShellState {
                window: window.clone(),
                header,
                subtitle,
                notebook: notebook.clone(),
                app: app.clone(),
                pool,
                tabs: Vec::new(),
                books: HashMap::new(),
                undocked: None,
                minimal_gui: false,
                cursor_hide_source: None,
                next_slot: 0,
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
                ReaderWindow::refresh_chrome(&sh, page_num as usize);
            });
        }

        // The initial grab_focus often runs while the window is not
        // yet active (late WM focus — sway, or no WM) — GTK then
        // ignores it and keys never reach the reader. Re-grab when
        // the toplevel becomes active.
        {
            let st = Rc::downgrade(&shell.state);
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

        // Fullscreen chrome: the header hides with the decorations
        // (`AutoMinimalGui` is false by default, so the C# keeps the
        // menu; the reveal strip below still applies).
        {
            let st = Rc::downgrade(&shell.state);
            window.connect_notify_local(Some("fullscreened"), move |win, _| {
                let Some(sh) = st.upgrade() else {
                    return;
                };
                let s = sh.borrow_mut();
                let fullscreen = win.is_fullscreen();
                s.header.set_visible(!fullscreen && !s.minimal_gui);
            });
        }

        // Closing the main window with an undocked reader: the C#
        // `ReaderFormFormClosing` re-docks, then the main form closes
        // — everything goes together.
        {
            let st = Rc::downgrade(&shell.state);
            window.connect_close_request(move |_| {
                if let Some(sh) = st.upgrade() {
                    let mut s = sh.borrow_mut();
                    if let Some(undocked) = s.undocked.take() {
                        undocked.window.close();
                    }
                }
                glib::Propagation::Proceed
            });
        }

        shell.open_comic(path)?;
        Ok(shell)
    }

    /// Adds a comic as a new tab (the C# `OpenComic` into a free
    /// slot).
    pub fn open_comic(&self, path: &Path) -> anyhow::Result<()> {
        let provider = cr_io::ComicProvider::open(path)
            .with_context(|| format!("Unsupported or unreadable comic: {}", path.display()))?;
        let page_count = provider.page_count();

        // The session book — the C# keeps these in the library
        // database (`Program.Library`); persistence arrives in
        // Phase 4.
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
        // `ProviderIndexRetrievalCompleted`: resume position and
        // read-progress clamp to the real page count.
        let resume = book.current_page.clamp(0, page_count as i32 - 1).max(0) as usize;
        book.last_page_read = book.last_page_read.clamp(0, page_count as i32 - 1);
        let last_read = book.last_page_read.max(0) as usize;

        let (slot, view, tab_widget);
        {
            let mut st = self.state.borrow_mut();
            view = PageView::new(Arc::clone(&st.pool));
            slot = st.next_slot;
            st.next_slot += 1;

            // Reading-state write-back: every logical page change
            // lands in the book (`ComicBookNavigator.CurrentPage`
            // setter).
            {
                let st_weak = Rc::downgrade(&self.state);
                view.set_page_callback(Some(Box::new(move |page, count| {
                    let Some(sh) = st_weak.upgrade() else {
                        return;
                    };
                    let mut s = sh.borrow_mut();
                    if let Some(book) = s.books.get_mut(&slot) {
                        book.set_current_page(page as i32);
                    }
                    if s.current_slot() == Some(slot) {
                        s.subtitle.set_text(&page_subtitle(page, count));
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
                        "NextTab" => ReaderWindow::switch_slot(&sh, 1),
                        "PrevTab" => ReaderWindow::switch_slot(&sh, -1),
                        "ToggleUndockReader" => ReaderWindow::toggle_undock(&sh),
                        "ToggleMenu" => ReaderWindow::toggle_minimal_gui(&sh),
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
                        let window = sh.borrow().window.clone();
                        window.close();
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
                    ReaderWindow::on_pointer_moved(&sh, area.upcast_ref(), y);
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
                ReaderWindow::close_tab(&sh, slot);
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
        let st = state.borrow();
        if let Some(tab) = st.tabs.get(page_num) {
            let book = st.books.get(&tab.slot);
            let page = book.map(|b| b.current_page).unwrap_or(0).max(0) as usize;
            st.subtitle.set_text(&page_subtitle(page, tab.page_count));
            st.window.set_title(Some(&Self::window_title(&tab.path)));
            tab.view.widget().grab_focus();
        }
    }

    /// Closes one tab (`OpenBooks.Close`); the last close closes the
    /// window.
    fn close_tab(state: &Rc<RefCell<ShellState>>, slot: usize) {
        let (pos, close_window, notebook) = {
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
        if close_window {
            // The main window's close-request handler takes the
            // undocked reader with it.
            let window = state.borrow().window.clone();
            window.close();
        } else {
            let current = state.borrow().notebook.current_page().unwrap_or(0) as usize;
            ReaderWindow::refresh_chrome(state, current);
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
            ReaderWindow::refresh_chrome(state, current);
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
        undocked_window.present();
        view.widget().grab_focus();
    }

    /// `MainForm.MinimalGui` (the K command): pins the chrome hidden.
    fn toggle_minimal_gui(state: &Rc<RefCell<ShellState>>) {
        let mut st = state.borrow_mut();
        st.minimal_gui = !st.minimal_gui;
        st.header
            .set_visible(!st.minimal_gui && !st.window.is_fullscreen());
    }

    /// `AutoHideMainMenu` (default true): the chrome reveals while
    /// the pointer is in the top strip and slides away elsewhere.
    /// The fullscreen cursor hides after the idle delay.
    fn on_pointer_moved(state: &Rc<RefCell<ShellState>>, area: &gtk4::Widget, y: f64) {
        let mut st = state.borrow_mut();
        if st.window.is_fullscreen() {
            // Cursor auto-hide: reset the idle timer on every motion.
            if let Some(source) = st.cursor_hide_source.take() {
                source.remove();
            }
            area.set_cursor_from_name(None);
            let area = area.clone();
            st.cursor_hide_source = Some(glib::timeout_add_local(
                std::time::Duration::from_millis(CURSOR_HIDE_MS),
                move || {
                    area.set_cursor_from_name(Some("none"));
                    glib::ControlFlow::Break
                },
            ));
        }
        let reveal = y <= CHROME_REVEAL_EDGE;
        st.header.set_visible(reveal && !st.minimal_gui);
    }

    pub fn present(&self) {
        self.state.borrow().window.present();
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
