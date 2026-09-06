//! The browser shell — the app's main window (the C# `MainForm`):
//! the navigator pane + ItemView in a paned container with a status
//! bar, the quick search, the view-mode/sort/group/size/columns
//! commands, and the reader docked as a view (the C# reader replaces
//! the browser view; `D` undocks it into its own window).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;

use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{gio, Application, ApplicationWindow, Button, Entry, Label, Paned, Stack};

use cr_core::model::comic_book::ComicBook;
use cr_core::xml::scalar::CrGuid;
use cr_engine::image_pool::ImagePool;
use cr_engine::matcher::tree::Matcher;

use crate::library;
use crate::reader::display::ImageFitMode;
use crate::reader::page_view::PageLayoutMode;

/// The user settings (`Program.Settings`).
fn cr_ui_settings() -> std::rc::Rc<std::cell::RefCell<cr_core::settings::Settings>> {
    library::settings()
}
use crate::reader_shell::ReaderShell;

use super::columns::default_columns;
use super::item_view::ItemView;
use super::layout::ItemViewMode;
use super::navigator::Navigator;
use super::pages_view::PagesPanel;
use super::status_bar;
use super::tabstrip::TabId;

/// The browser workspace tabs (the C# `tsbLibrary`/`tsbPages`).
#[derive(Clone, Copy, PartialEq)]
enum Workspace {
    Library,
    Pages,
}

/// The search debounce (`UpdateSearch` on text change; large sets
/// re-filter).
const SEARCH_DEBOUNCE_MS: u64 = 300;

/// The thumbnail size range (`Program` limits: 96..512).
const MIN_THUMB: f64 = 96.0;
const MAX_THUMB: f64 = 512.0;
const THUMB_STEP: f64 = 16.0;

/// The clone-able handle (the app's session slot; the closures keep
/// the window alive through the state's widgets).
#[derive(Clone)]
pub struct BrowserShell {
    window: ApplicationWindow,
    state: Rc<ShellState>,
}

struct ShellState {
    window: ApplicationWindow,
    stack: Stack,
    /// The multi-panel status bar (the T8 `statusStrip`).
    status_bar: super::status_bar::StatusBar,
    navigator: Rc<Navigator>,
    item_view: ItemView,
    quick_view: ItemView,
    pages: PagesPanel,
    /// The full-window Pages workspace page (the probe measures its
    /// allocation — the full-window layout gate).
    pages_page: gtk4::Box,
    /// The navigator pane host — the Sidebar toggle target (the C#
    /// `tbSidebar` collapses the left pane).
    nav_box: gtk4::Box,
    /// The workspace tab strip (`MainView.tabStrip`): Library |
    /// Pages | the comic tabs | `+`, with the reader toolbar docked
    /// at the right end.
    tab_strip: super::tabstrip::TabStrip,
    /// The last browser workspace (the C# `lastBrowser` —
    /// `ShowLast`/ToggleBrowser return to it).
    last_browser: Cell<u8>,
    /// The quick-search entry (the FocusQuickSearch command target).
    search: Entry,
    reader: ReaderShell,
    app: Application,
    /// The current navigator selection (refreshes after mutations).
    current_list: RefCell<Option<CrGuid>>,
    /// The current list's name (the status-bar selection panel; set
    /// on the navigator selection + refreshes).
    current_list_name: RefCell<String>,
    /// The `win.` action group members by name (the enable-state
    /// sync reaches them here). A RefCell: the map fills while the
    /// state itself already lives in its Rc.
    actions: RefCell<HashMap<&'static str, gio::SimpleAction>>,
    /// The list browsing history (the C# `ILibraryBrowser` back /
    /// forward chain; Previous/Next List + the T6 toolbar buttons).
    list_history: RefCell<Vec<CrGuid>>,
    list_history_pos: Cell<usize>,
    /// The random-book walk state (`OpenNextComic` random mode: no
    /// repeats until the list changed or the cycle wrapped).
    random_list: RefCell<Vec<CrGuid>>,
    random_picked: RefCell<Vec<CrGuid>>,
    /// The main-window menubar (Phase 5.5 T3; the T14
    /// layout persistence and the probes reach it here).
    menubar: super::menubar::MenubarWidget,
    /// The reader toolbar (the T5 `mainToolStrip`).
    toolbar: super::toolbar::ReaderToolbar,
    /// The reader page box (the toolbar's docked parent — the
    /// undock moves the toolbar in and out of it).
    reader_page_box: gtk4::Box,
    /// The browser toolbar (the T6 `ComicBrowserControl.toolStrip`).
    browser_toolbar: super::browser_toolbar::BrowserToolbar,
    /// The live quick-search text (the composed filter reads it).
    search_text: RefCell<String>,
    /// The composed filter (quick search + the view filters) — the
    /// Duplicate List source (`GetCurrentMatcher`).
    current_filter: RefCell<Option<Matcher>>,
    /// The Detail header column chooser (the C#
    /// `autoHeaderContextMenuStrip`): a FRESH plain popover per open;
    /// the last one is kept for the probe.
    columns_drop: RefCell<Option<gtk4::Popover>>,
    /// The app image pool (the C# `Program.ImagePool`): the Tasks
    /// dialog queue snapshot and the Quick Rating cover load.
    pool: Arc<ImagePool>,
    /// The single Tasks dialog instance (`ShowPendingTasks`
    /// re-presents it).
    tasks_window: RefCell<Option<gtk4::Window>>,
    /// The navigator/item split (the `BrowserSplit` persistence
    /// reads the position, the restore sets it).
    paned: Paned,
}

impl ShellState {
    /// Opens a comic into the docked reader and shows it (the C#
    /// `OpenComic`; the reader tab selects and the workspace swaps).
    fn open_comic(&self, path: &Path) {
        // The C# `Open(ComicBook)` gate (NavigatorManager.cs): a
        // fileless book (`!IsLinked`) never opens a reader slot.
        if path.as_os_str().is_empty() {
            return;
        }
        match self.reader.open_comic(path) {
            Ok(()) => {
                self.stack.set_visible_child_name("reader");
                self.window.present();
            }
            Err(err) => {
                show_error_dialog(&self.app, &path.to_string_lossy(), &format!("{err:#}"));
            }
        }
    }

    fn show_browser(&self) {
        self.stack.set_visible_child_name("browser");
    }

    /// Selects a browser workspace tab (`ShowView(tsbLibrary)`/
    /// `ShowView(tsbPages)`); the reader hides behind it.
    fn select_workspace(&self, ws: Workspace) {
        match ws {
            Workspace::Library => {
                self.last_browser.set(0);
                self.stack.set_visible_child_name("browser");
            }
            Workspace::Pages => {
                self.last_browser.set(1);
                self.stack.set_visible_child_name("pages");
            }
        }
    }

    /// `ShowLast` — the last browser workspace tab.
    fn select_last_browser(&self) {
        let ws = if self.last_browser.get() == 1 {
            Workspace::Pages
        } else {
            Workspace::Library
        };
        self.select_workspace(ws);
    }

    /// `OpenBooks_Clicked` / a comic tab click: the slot selects and
    /// the reader workspace shows (`ShowView(i)` → the comic viewer
    /// covers the browser).
    fn activate_slot(&self, slot: usize) {
        self.reader.switch_to_slot(slot);
        self.stack.set_visible_child_name("reader");
    }

    /// The workspace tab strip click (`Selected`/`CaptionClick`):
    /// selecting another item swaps the workspace; re-clicking the
    /// SELECTED item toggles the browser (the C# `tab_CaptionClick`
    /// → `ToggleBrowser`).
    fn on_tab_select(&self, id: &TabId) {
        let visible = self
            .stack
            .visible_child_name()
            .map(|s| s.to_string())
            .unwrap_or_default();
        match id {
            TabId::Library => {
                if visible == "browser" {
                    self.toggle_browser();
                } else {
                    self.select_workspace(Workspace::Library);
                }
            }
            TabId::Pages => {
                if visible == "pages" {
                    self.toggle_browser();
                } else {
                    self.select_workspace(Workspace::Pages);
                }
            }
            TabId::Comic(slot) => {
                // The C# wires CaptionClick only on the WORKSPACE
                // items (`MainView.cs:161-163` — Library/Folders/
                // Pages); comic file tabs never toggle: a re-click on
                // the current comic's tab stays on the page (the C#
                // `ShowView` re-selects the viewer, no toggle).
                self.activate_slot(*slot);
            }
            TabId::Plus => {
                // `OpenBooks.AddSlot` + `CurrentSlot = last`: the new
                // empty slot selects and shows (blank reader view).
                self.reader.add_empty_slot();
                self.stack.set_visible_child_name("reader");
            }
        }
        // The strip state renders from the workspace (the T6
        // lesson) — re-sync after every click.
        self.sync_enabled();
    }

    /// Pushes the open slots into the strip and derives its
    /// selection from the visible workspace (the T6 lesson: state
    /// renders from the source of truth, never from the click).
    fn sync_tabs(&self) {
        let infos = self.reader.tab_infos();
        self.tab_strip.set_tabs(&infos);
        // `tsbPages.Visible = OpenBooks.CurrentBook != null`.
        self.tab_strip
            .set_pages_visible(self.reader.has_current_book());
        // `fileTab.Visible = BrowserDock == Fill && !ReaderUndocked`.
        self.tab_strip
            .set_comic_tabs_visible(!self.reader.is_undocked());
        let selected = match self.stack.visible_child_name().as_deref() {
            Some("browser") => TabId::Library,
            Some("pages") => TabId::Pages,
            _ => self
                .reader
                .current_slot_id()
                .map(TabId::Comic)
                .unwrap_or(TabId::Library),
        };
        self.tab_strip.set_selected(&selected);
    }

    /// The QuickOpen empty state (`UpdateQuickList`: visible when no
    /// book is open, `ShowQuickOpen`, and the database has books).
    fn show_quick_open(&self) {
        if !cr_ui_settings().borrow().show_quick_open {
            self.show_browser();
            return;
        }
        let lists = library::quick_open_lists();
        let total: usize = lists.iter().map(|(_, b)| b.len()).sum();
        if total == 0 {
            self.show_browser();
            return;
        }
        let mut books: Vec<ComicBook> = Vec::new();
        for (_, group) in lists {
            books.extend(group);
        }
        self.quick_view.set_books(books);
        self.stack.set_visible_child_name("quickopen");
    }

    fn refresh_view_from_list(&self) {
        let id = *self.current_list.borrow();
        if let Some(id) = id {
            if let Some((name, books)) = library::evaluate_books(&id) {
                // The list name feeds the status panel (a rename
                // shows on the next refresh without a re-select).
                *self.current_list_name.borrow_mut() = name;
                // The C# refresh updates the items in place — the
                // selection survives (the My Rating check reads the
                // selection right after the rating commit).
                let selected = self.item_view.selection_ids();
                self.item_view.set_books(books);
                if !selected.is_empty() {
                    self.item_view.reselect(&selected);
                }
            }
        }
    }

    /// The status-bar panels (`OnUpdateGui`'s strip updates fold
    /// into the same sync the actions ride): the selection info, the
    /// book caption, the page + count, and the thumb slider.
    fn update_status_panels(&self) {
        // The selection info (the C# `SelectionInfo`).
        let count = self.item_view.book_count();
        let total = self.item_view.total_count();
        let total_size = self.item_view.visible_size();
        let selected = self.item_view.selection_len();
        let selected_size = self.item_view.selected_size();
        let selected_path = if selected == 1 {
            self.item_view
                .selection_ids()
                .first()
                .and_then(library::book_path)
        } else {
            None
        };
        let list_name = self.current_list_name.borrow().clone();
        // The C# reads the ACTIVE browser service: with the reader
        // or QuickOpen showing, `FindActiveService<IComicBrowser>`
        // returns null and the panel goes EMPTY (the "Ready" text is
        // only the Designer default).
        let browser_visible = self.stack.visible_child_name().as_deref() == Some("browser");
        let info = if browser_visible {
            status_bar::selection_info(
                &list_name,
                count,
                total,
                total_size,
                selected,
                selected_size,
                selected_path.as_deref(),
            )
        } else {
            String::new()
        };
        self.status_bar.set_selection_info(&info);

        // The open book (the caption ellipsized in the C# to 60 —
        // the label caps at the same width).
        let caption = self
            .reader
            .tab_infos()
            .into_iter()
            .find(|t| t.current && t.has_book)
            .map(|t| t.caption);
        self.status_bar.set_book(caption.as_deref());

        // The current page + count (the page panel is 1-based;
        // "NA"/"None" without a book).
        let has_book = self.reader.has_current_book();
        let page = if has_book {
            self.reader.current_display_page()
        } else {
            None
        };
        let track = cr_ui_settings().borrow().track_current_page;
        self.status_bar.set_page(page, track);
        let page_count = self.reader.current_book().map(|(_, c)| c).unwrap_or(0);
        self.status_bar
            .set_page_count(&status_bar::page_count_text(page_count));

        // The thumb slider: the browser workspace only (the C#
        // `mainViewContainer.Expanded`), range/value per mode.
        let size = self.item_view.item_size();
        self.status_bar.sync_slider(size, browser_visible);
    }
}

impl BrowserShell {
    /// Builds the main window (`MainForm`): the browser view + the
    /// docked reader + the header commands.
    pub fn create(app: &Application) -> (ApplicationWindow, BrowserShell) {
        let window = ApplicationWindow::builder()
            .application(app)
            .title("comicrust")
            .default_width(1280)
            .default_height(800)
            .build();
        // F10 = MinimalGui (the C# command). GTK's built-in
        // `handle-menubar-accel` (a CAPTURE-phase F10 shortcut since
        // 4.2) consumes the key to focus a model menubar — our
        // menubar is the custom T3 widget, so the accel never fired
        // (the user report). The window keeps its own F10 meaning.
        window.set_handle_menubar_accel(false);

        // One pool for the whole app (the C# `Program.ImagePool` is
        // global). The `CacheManager` construction: disk caches +
        // memory capacities from the settings, and the cache-event
        // sink that writes decoded page sizes back into the books.
        let pool = Arc::new(ImagePool::with_config(&library::image_pool_config()));
        library::install_cache_events(&pool);
        let (reader, reader_widgets) = ReaderShell::new(app, Arc::clone(&pool));
        let navigator = Navigator::new();
        let super::item_view::ItemViewWidgets {
            scroller: item_scroller,
            view: item_view,
        } = ItemView::create(Arc::clone(&pool));
        // The QuickOpen covers (captionless — `HideCaptions`).
        let super::item_view::ItemViewWidgets {
            scroller: quick_scroller,
            view: quick_view,
        } = ItemView::create(Arc::clone(&pool));
        quick_view.configure(|c| c.hide_captions = true);
        let super::pages_view::PagesPanelWidgets {
            widget: pages_widget,
            panel: pages,
        } = super::pages_view::PagesPanel::create(Arc::clone(&pool), &window);

        // The header commands (the handlers wire in `wire`, where
        // the shared state exists). The T6 reorg: Open/Add
        // Folder/Preferences/View/Sort/Group moved into the menubar
        // and the browser toolbar row — the header carries the
        // reader's page display only.
        let header = gtk4::HeaderBar::new();
        // The reader's "Page X of Y" lives in the main window header
        // (the C# main form shows it in the title area).
        header.pack_end(&reader_widgets.subtitle());
        window.set_titlebar(Some(&header));

        // The browser toolbar row (the C# `ComicBrowserControl.
        // toolStrip`): Sidebar, Browse prev/next, Views, Group,
        // Arrange, then the right-aligned Quick Search, List Layouts
        // (disabled), Duplicate List.
        let search = Entry::new();
        let browser_toolbar = super::browser_toolbar::BrowserToolbar::create(&window, &search);

        // The browser page: the navigator pane left, the toolbar +
        // ItemView pane right. The status label moved below the
        // workspace stack (the C# status strip is form-wide).
        let nav_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        nav_box.append(navigator.widget());

        let paned = Paned::new(gtk4::Orientation::Horizontal);
        paned.set_start_child(Some(&nav_box));
        paned.set_shrink_start_child(false);
        paned.set_position(280);
        paned.set_vexpand(true);
        // The status bar sits below the workspace stack (the C#
        // `statusStrip` is form-wide). The panels fill at the first
        // `sync_enabled`.
        let super::status_bar::StatusBarWidgets {
            widget: status_widget,
            bar: status_bar,
        } = super::status_bar::StatusBar::create();
        // The browser toolbar rides the ITEM VIEW pane (the C#
        // toolStrip spans the ComicBrowserControl's list area — the
        // user report: it must start at the left edge of the RIGHT
        // view window, not cover the navigator).
        let item_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        item_box.append(browser_toolbar.widget());
        item_box.append(&item_scroller);
        paned.set_end_child(Some(&item_box));
        paned.set_shrink_end_child(false);
        let browser_page = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        browser_page.append(&paned);

        // The quick-open page (the C# reader-area overlay shown when
        // no book is open and `ShowQuickOpen`): the recent lists as
        // captionless covers.
        let quick_page = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
        let quick_label = Label::builder()
            .label("Quick Open")
            .halign(gtk4::Align::Start)
            .margin_top(8)
            .margin_start(8)
            .build();
        quick_page.append(&quick_label);
        quick_page.append(&quick_scroller);
        quick_scroller.set_vexpand(true);

        // The workspace stack — the full-window tab contents (the C#
        // `MainView.ShowView`): quick open ⇄ browser ⇄ Pages ⇄
        // reader. The Pages workspace is a full-window tab now (the
        // `ComicPagesView` shape), not a left-panel mini tab. The
        // stack EXPANDS: it owns the window below the bars (the T9
        // user test: the Pages page collapsed to its toolbar
        // without this).
        let stack = Stack::new();
        stack.set_vhomogeneous(false);
        stack.set_hhomogeneous(false);
        stack.set_vexpand(true);
        stack.set_hexpand(true);
        stack.add_named(&quick_page, Some("quickopen"));
        stack.add_named(&browser_page, Some("browser"));
        let pages_page = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        pages_page.append(&pages_widget);
        stack.add_named(&pages_page, Some("pages"));

        // The menubar (the C# `mainMenuStrip`) rides above the
        // content — the T3 custom bar (GTK4 model menus cannot show
        // the C# menu-item icons).
        let menubar = super::menubar::create_menubar(&window);
        // The workspace tab strip (the T9 `MainView.tabStrip`): the
        // row under the menubar with Library | Pages | the comic tabs
        // | `+`. The reader toolbar (the T5 `mainToolStrip`) docks
        // into its right end (the C# Fill rule:
        // `MainToolStripVisible = false` → the strip lives inside the
        // tab row; Tools/Fullscreen stay reachable from the library
        // view).
        let toolbar = super::toolbar::ReaderToolbar::create(&window);
        let tab_strip = super::tabstrip::TabStrip::create(Arc::clone(&pool));
        tab_strip.host().append(toolbar.widget());
        let reader_page = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        reader_page.append(&reader_widgets.notebook());
        stack.add_named(&reader_page, Some("reader"));
        let content = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        content.append(menubar.widget());
        content.append(tab_strip.widget());
        content.append(&stack);
        content.append(&status_widget);
        window.set_child(Some(&content));

        let state = Rc::new(ShellState {
            window: window.clone(),
            stack: stack.clone(),
            status_bar,
            navigator: Rc::clone(&navigator),
            item_view,
            quick_view,
            pages,
            pages_page: pages_page.clone(),
            nav_box,
            reader_page_box: tab_strip.host().clone(),
            tab_strip,
            last_browser: Cell::new(0),
            search: search.clone(),
            reader,
            app: app.clone(),
            current_list: RefCell::new(None),
            current_list_name: RefCell::new(String::new()),
            actions: RefCell::new(HashMap::new()),
            list_history: RefCell::new(Vec::new()),
            list_history_pos: Cell::new(0),
            random_list: RefCell::new(Vec::new()),
            random_picked: RefCell::new(Vec::new()),
            menubar,
            toolbar,
            browser_toolbar,
            search_text: RefCell::new(String::new()),
            current_filter: RefCell::new(None),
            // The column chooser builds a FRESH popover per open
            // (the exact shape of the proven book context menu; the
            // last one stays here for the probe).
            columns_drop: RefCell::new(None),
            pool,
            tasks_window: RefCell::new(None),
            paned: paned.clone(),
        });
        let shell = BrowserShell {
            window: window.clone(),
            state: Rc::clone(&state),
        };
        shell.wire(&search);
        // The persisted workspace restores (the C# `MainForm.Load`
        // applies `Settings.CurrentWorkspace` before the first
        // show). A missing element keeps the defaults.
        if let Some(ws) = cr_ui_settings().borrow().current_workspace.clone() {
            shell.state.apply_workspace(&ws);
        }
        (window, shell)
    }

    /// The navigator pane handle (the list-command host).
    pub fn navigator(&self) -> Rc<Navigator> {
        Rc::clone(&self.state.navigator)
    }

    /// The main-window menubar (the T3 custom bar; the T14
    /// layout persistence and the probes reach it here).
    pub fn menubar(&self) -> &super::menubar::MenubarWidget {
        &self.state.menubar
    }

    /// The workspace tab strip (the T9 bar; the probes reach it
    /// here).
    pub fn tabstrip(&self) -> super::tabstrip::TabStrip {
        self.state.tab_strip.clone()
    }

    /// The status bar handle (the T8 probe gates the panels).
    pub fn statusbar(&self) -> super::status_bar::StatusBar {
        self.state.status_bar.clone()
    }

    /// The browser grid's current thumb height (the slider resize
    /// gate).
    pub fn state_grid_thumb_height(&self) -> f64 {
        self.state.item_view.thumb_height()
    }

    /// Probe: the browser view mode (the T14 restore gate).
    pub fn state_grid_mode(&self) -> &'static str {
        match self.state.item_view.mode() {
            ItemViewMode::Thumbnail => "thumbnail",
            ItemViewMode::Tile => "tile",
            ItemViewMode::Detail => "detail",
        }
    }

    /// Probe: the navigator pane visibility + split (the T14 restore
    /// gate).
    pub fn state_sidebar(&self) -> (bool, i32) {
        (self.state.nav_box.is_visible(), self.state.paned.position())
    }

    /// Probe: the T14 exit snapshot (the collect against the live
    /// widgets; the reader family falls back to the saved one).
    pub fn state_collect_workspace(&self) -> cr_core::settings::workspace::WorkspaceState {
        let prev = cr_ui_settings().borrow().current_workspace.clone();
        self.state.collect_workspace(prev.as_ref())
    }

    /// Probe: the T14 startup restore.
    pub fn state_apply_workspace(&self, ws: &cr_core::settings::workspace::WorkspaceState) {
        self.state.apply_workspace(ws);
    }

    /// Probe: moves the navigator/item split (the T14 collect gate
    /// needs a non-default position to prove the persistence).
    pub fn state_set_paned(&self, position: i32) {
        self.state.paned.set_position(position);
    }

    /// Probe: the Detail column set (id, name, visible) — the T14
    /// restore gate.
    pub fn state_detail_columns(&self) -> Vec<(i32, String, bool)> {
        self.state.item_view.detail_columns_snapshot()
    }

    /// The browser grid's item-size triple (the slider sync gate).
    pub fn state_grid_item_size(&self) -> Option<(f64, f64, f64)> {
        self.state.item_view.item_size()
    }

    /// Dispatches a READER command through the current view (the
    /// page-click path minus the mouse gesture — the ShowBrowser →
    /// ToggleBrowserFromReader gate).
    pub fn state_reader_dispatch(&self, id: &str) {
        self.state.reader.dispatch_current(id);
    }

    /// Probe: the allocated heights of the workspace stack and the
    /// Pages page (the full-window layout gate — the T9 user test
    /// caught the Pages page at its toolbar's height).
    pub fn state_workspace_heights(&self) -> (i32, i32) {
        (self.state.stack.height(), self.state.pages_page.height())
    }

    /// The main window handle.
    pub fn window(&self) -> ApplicationWindow {
        self.window.clone()
    }

    fn wire(&self, search: &Entry) {
        let state = &self.state;

        // The reader docks: the host window drives the fullscreen
        // chrome and the Q exit.
        state.reader.set_host(&self.window);

        // The last reader tab closes → the Library workspace shows
        // again (the C# `Close` → `ShowLibrary`; the strip loses the
        // comic tabs and the Pages tab).
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .reader
                .set_on_last_tab_closed(move || {
                    if let Some(sh) = state.upgrade() {
                        sh.pages.clear_book();
                        sh.select_workspace(Workspace::Library);
                        sh.sync_enabled();
                    }
                });
        }

        // The tab set changed (open/close/undock/re-dock/AddSlot) —
        // the strip rebuilds with the sync.
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .reader
                .set_on_tabs_changed(move || {
                    if let Some(sh) = state.upgrade() {
                        sh.sync_enabled();
                    }
                });
        }

        // A tab closes → the auto Quick Review gate (the C#
        // `OnBookClosing`: AutoShowQuickReview && HasBeenRead &&
        // Rating == 0 — the book leaves, the dialog opens over the
        // library entry).
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .reader
                .set_on_book_closing(move |book| {
                    let auto_show = cr_ui_settings().borrow().auto_show_quick_review;
                    if !crate::dialogs::quick_rating::should_auto_show(book, auto_show) {
                        return;
                    }
                    if let Some(sh) = state.upgrade() {
                        // The rating edits the LIBRARY copy (the
                        // session copy is gone with the closed tab).
                        let Some(current) = library::session()
                            .borrow()
                            .find_book(&book.file_path)
                            .cloned()
                        else {
                            return;
                        };
                        let show_when_read = cr_ui_settings().borrow().auto_show_quick_review;
                        let state2 = Rc::downgrade(&sh);
                        let pool = Arc::clone(&sh.pool);
                        crate::dialogs::quick_rating::show_quick_rating(
                            &sh.window,
                            &current,
                            show_when_read,
                            pool,
                            move |result| {
                                let Some(result) = result else {
                                    return;
                                };
                                cr_ui_settings().borrow_mut().auto_show_quick_review =
                                    result.show_when_read;
                                if let Some(sh) = state2.upgrade() {
                                    sh.set_quick_rating_fields(
                                        &current.id,
                                        result.rating,
                                        &result.review,
                                    );
                                    sh.sync_enabled();
                                }
                            },
                        );
                    }
                });
        }

        // Library-group reader commands (`NextComic`/`PrevComic`/
        // `RandomComic`/`ShowBrowser`) — the C# handlers live on
        // MainForm; the shell owns the list context.
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .reader
                .set_on_library_command(move |id| {
                    if let Some(sh) = state.upgrade() {
                        match id {
                            "NextComic" => sh.open_next_book(1),
                            "PrevComic" => sh.open_next_book(-1),
                            "RandomComic" => sh.open_next_book(0),
                            "ShowBrowser" => {
                                // `ToggleBrowserFromReader`: in Fill
                                // mode (our only mode) the reader's
                                // MouseLeft/Escape command flips
                                // MINIMAL UI, not the browser — the
                                // browser branch runs only with the
                                // MouseSwitchesToFullLibrary
                                // extended setting (MainForm.cs:
                                // 2133-2144).
                                if !cr_core::settings::ExtendedSettings::global()
                                    .mouse_switches_to_full_library
                                {
                                    sh.reader.dispatch_current("ToggleMenu");
                                } else {
                                    sh.toggle_browser();
                                }
                            }
                            _ => {}
                        }
                        sh.sync_enabled();
                    }
                });
        }

        // Undock → the main window reveals the browser (the C#
        // `ReaderUndocked` leaves the main form with its browser);
        // re-dock → the reader page shows again.
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .reader
                .set_on_view_change(move |reader_visible| {
                    if let Some(sh) = state.upgrade() {
                        if reader_visible {
                            sh.stack.set_visible_child_name("reader");
                        } else {
                            sh.select_last_browser();
                        }
                        // Undock/re-dock changes the menubar rule and
                        // hides the comic tabs (the C#
                        // `fileTab.Visible = Fill && !ReaderUndocked`).
                        sh.sync_enabled();
                    }
                });
        }

        // The Pages panel: rebinds on every visible-book change (the
        // C# `Viewer_BookChanged` → `pagesView.Book`; an empty slot
        // clears it), follows the bound book's page turns, and
        // navigates on double-click.
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .reader
                .set_on_book_changed(move || {
                    if let Some(sh) = state.upgrade() {
                        match sh.reader.current_comic_book() {
                            Some(book) => {
                                let page = book.current_page.max(0) as usize;
                                sh.pages.set_book(book);
                                sh.pages.set_current_page(page);
                            }
                            None => sh.pages.clear_book(),
                        }
                        sh.sync_enabled();
                    }
                });
        }
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .reader
                .set_on_page_change(move |page| {
                    if let Some(sh) = state.upgrade() {
                        sh.pages.set_current_page(page);
                        // The status-bar page panel follows every
                        // turn (wheel/click turns dispatch no action,
                        // so the sync never sees them). The hook runs
                        // INSIDE the reader-state borrow — no reader
                        // access here, only the page value.
                        let track = cr_ui_settings().borrow().track_current_page;
                        sh.status_bar.set_page(Some(page), track);
                    }
                });
        }
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .pages
                .connect_activate(move |page| {
                    if let Some(sh) = state.upgrade() {
                        sh.reader.navigate_current(page);
                        // `ShowComic()`: the double-click reveals the
                        // comic (the reader page wins over the
                        // browser).
                        sh.stack.set_visible_child_name("reader");
                        sh.sync_enabled();
                    }
                });
        }

        // The workspace tab strip: item clicks select the workspace
        // (a re-click on the selected item toggles the browser — the
        // C# `tab_CaptionClick`); the close buttons close the slot.
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .tab_strip
                .connect_select(move |id| {
                    if let Some(sh) = state.upgrade() {
                        sh.on_tab_select(id);
                    }
                });
        }
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .tab_strip
                .connect_close(move |slot| {
                    if let Some(sh) = state.upgrade() {
                        sh.reader.close_slot(slot);
                        sh.sync_enabled();
                    }
                });
        }

        // The Pages tab became visible — reflow with the real
        // allocation (the first show after a hidden binding).
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .stack
                .connect_visible_child_notify(move |_stack| {
                    if let Some(sh) = state.upgrade() {
                        if sh.stack.visible_child_name().as_deref() == Some("pages") {
                            sh.pages.reflow();
                        }
                    }
                });
        }

        // The QuickOpen covers: double-click opens the comic
        // (`QuickOpenBookActivated`).
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .quick_view
                .connect_activate(move |id| {
                    if let Some(sh) = state.upgrade() {
                        if let Some(path) = library::book_path(id) {
                            sh.open_comic(Path::new(&path));
                        }
                    }
                });
        }

        // The navigator selection → the ItemView book set (debounced
        // inside the widget) + the status bar count.
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .navigator
                .connect_selected(move |id, _name| {
                    if let Some(sh) = state.upgrade() {
                        *sh.current_list.borrow_mut() = Some(*id);
                        // The list history (`BrowsePrevious` chain): a
                        // history walk lands on the entry at the walk
                        // position and does not append; a new
                        // selection drops the forward entries.
                        {
                            let mut h = sh.list_history.borrow_mut();
                            let pos = sh.list_history_pos.get();
                            if h.get(pos) != Some(id) {
                                h.truncate(pos + 1);
                                h.push(*id);
                                sh.list_history_pos.set(h.len() - 1);
                            }
                        }
                        if let Some((name, books)) = library::evaluate_books(id) {
                            // The list name feeds the status-bar
                            // selection panel (`BookList.Name`).
                            *sh.current_list_name.borrow_mut() = name;
                            sh.item_view.set_books(books);
                        }
                        sh.sync_enabled();
                    }
                });
        }

        // The navigator Refresh button: refill the tree from the
        // library snapshot and re-evaluate the current list (the
        // C# `FillListTree` + `UpdateBookList` refresh shape).
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .navigator
                .connect_refresh(move || {
                    if let Some(sh) = state.upgrade() {
                        sh.navigator.refill(&library::comic_lists_snapshot());
                        sh.refresh_view_from_list();
                    }
                });
        }

        // The selection change feeds the status-bar selection panel
        // + the enable-state (both ride the sync).
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .item_view
                .connect_selection_changed(move |_selected| {
                    if let Some(sh) = state.upgrade() {
                        sh.sync_enabled();
                    }
                });
        }

        // Double-click / Enter → open in the (docked) reader.
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .item_view
                .connect_activate(move |id| {
                    if let Some(sh) = state.upgrade() {
                        if let Some(path) = library::book_path(id) {
                            sh.open_comic(Path::new(&path));
                        }
                    }
                });
        }

        // The right-click context menu (open / reveal / remove /
        // properties stub).
        {
            state.item_view.connect_context({
                let state = Rc::downgrade(state);
                move |id, x, y| {
                    show_context_menu(&state, id, x, y);
                }
            });
        }

        // The quick search (`UpdateQuickFilter`): the composed filter
        // (scope + the view filters + the text, or a MATCH/NOT
        // query). Debounced.
        {
            let state = Rc::downgrade(state);
            search.connect_changed(move |entry| {
                let text = entry.text().to_string();
                let state = state.clone();
                glib::timeout_add_local(
                    std::time::Duration::from_millis(SEARCH_DEBOUNCE_MS),
                    move || {
                        if let Some(sh) = state.upgrade() {
                            *sh.search_text.borrow_mut() = text.clone();
                            sh.rebuild_filter();
                        }
                        glib::ControlFlow::Break
                    },
                );
            });
        }

        // The Detail header right-click → the column chooser (the
        // C# `autoHeaderContextMenuStrip`).
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .item_view
                .connect_header_context(move |wx, wy| {
                    if let Some(sh) = state.upgrade() {
                        sh.popup_column_chooser(wx, wy);
                    }
                });
        }

        // The window-activation focus: the browser page grabs the
        // ItemView, the reader page its PageView (the Phase 3
        // dead-first-keypress fix, now per view).
        {
            let state = Rc::downgrade(state);
            self.window
                .connect_notify_local(Some("is-active"), move |win, _| {
                    if !win.is_active() {
                        return;
                    }
                    if let Some(sh) = state.upgrade() {
                        if sh.stack.visible_child_name().as_deref() == Some("reader") {
                            crate::trace::trace("is-active: re-grab reader focus");
                            sh.reader.focus_current();
                        } else {
                            let scroll_before = sh.item_view.scroll_value();
                            crate::trace::trace(format!(
                                "is-active: re-grab item-view focus (scroll was {scroll_before})"
                            ));
                            sh.item_view.grab_focus();
                            // The Wayland popover grab flaps the
                            // window activation; the re-grab is the
                            // dead-first-keypress fix, but grab_focus
                            // on the virtual-size canvas makes the
                            // ScrolledWindow's scroll-to-focus jump to
                            // the origin (the right-click report).
                            // That scroll runs in an IDLE — a
                            // synchronous compare here reads the old
                            // value and misses it (the user trace: the
                            // restore line never fired, the jump
                            // happened anyway). Restore on an idle.
                            let view = sh.item_view.clone();
                            glib::idle_add_local_once(move || {
                                let scroll_after = view.scroll_value();
                                if scroll_after != scroll_before {
                                    crate::trace::trace(format!(
                                        "is-active: grab moved the scroll {scroll_before} -> {scroll_after}; restored"
                                    ));
                                    view.set_scroll_value(scroll_before);
                                }
                            });
                        }
                    }
                });
        }

        // Closing the main window: dock the undocked reader back and
        // save (`MainFormFormClosed` → `CleanUp`; the C# exit also
        // stores `Settings.QuickOpenThumbnailSize` and saves the
        // settings file).
        {
            let state = Rc::downgrade(state);
            self.window.connect_close_request(move |_| {
                if let Some(sh) = state.upgrade() {
                    sh.reader.shutdown();
                    // `Program.Settings.QuickOpenThumbnailSize = quickOpenView.ThumbnailSize`.
                    let size = sh.quick_view.thumb_height() as i32;
                    {
                        // The workspace snapshot lands BEFORE the
                        // save (the C# `CleanUp` copy). The reader
                        // keeps the layout family from the previous
                        // save — shutdown closed the views.
                        let prev = cr_ui_settings().borrow().current_workspace.clone();
                        let ws = sh.collect_workspace(prev.as_ref());
                        cr_ui_settings().borrow_mut().current_workspace = Some(ws);
                    }
                    cr_ui_settings().borrow_mut().quick_open_thumbnail_size = size;
                }
                if let Err(err) = library::save() {
                    eprintln!("library save failed: {err}");
                }
                library::save_settings();
                glib::Propagation::Proceed
            });
        }

        // The view commands (mode/size/sort/group).
        self.install_actions();

        // The status bar's clicks: the page panel toggles
        // TrackCurrentPage (the C# `tsCurrentPage_Click` — the
        // stateful action flips the setting and re-syncs); the lamps
        // open the Tasks dialog (T13; the disabled stub no-ops).
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .status_bar
                .connect_page_click(move || {
                    if let Some(sh) = state.upgrade() {
                        // The disabled stub would return Err — the
                        // activation is best-effort by design.
                        let _ = gtk4::prelude::WidgetExt::activate_action(
                            &sh.window,
                            "win.track-current-page",
                            None,
                        );
                    }
                });
        }
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .status_bar
                .connect_lamp_click(move || {
                    if let Some(sh) = state.upgrade() {
                        // The Tasks dialog is a disabled stub until
                        // T13 — the activation is best-effort.
                        let _ = gtk4::prelude::WidgetExt::activate_action(
                            &sh.window,
                            "win.tasks",
                            None,
                        );
                    }
                });
        }
        // The slider drag → `SetItemSize` (the C# `TrackBar.Scroll`).
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .status_bar
                .connect_slider(move |value| {
                    if let Some(sh) = state.upgrade() {
                        sh.item_view.set_item_size(value);
                    }
                });
        }
        // The 1 s activity poll (`updateActivityTimer`): the lamps
        // follow the scan/write/export activity.
        {
            let state = Rc::downgrade(state);
            status_bar::start_activity_timer(move || {
                if let Some(sh) = state.upgrade() {
                    sh.status_bar.update_lamps(
                        library::is_scanning(),
                        library::writes_pending() > 0,
                        library::export_in_flight(),
                    );
                }
            });
        }

        // The initial fill. The startup view: the QuickOpen covers
        // when the database has books (the C#
        // `OpenCount == 0 && ShowQuickOpen`), the browser otherwise.
        state.navigator.refill(&library::comic_lists_snapshot());
        // `UpdateSettings` applies the stored QuickOpen thumbnail size.
        {
            let size = cr_ui_settings().borrow().quick_open_thumbnail_size as f64;
            state.quick_view.configure(|c| c.thumb_height = size);
            let _ = &size;
        }
        state.show_quick_open();
        // The strip renders its startup state with the sync.
        state.sync_enabled();
    }

    fn install_actions(&self) {
        ShellState::install_commands(&self.state);
        ShellState::install_menubar_keys(&self.state);
        ShellState::install_dyn_fills(&self.state);
    }

    /// Opens a comic into the docked reader (the app's `open_reader`
    /// path).
    pub fn open_comic(&self, path: &Path) {
        self.state.open_comic(path);
    }

    pub fn present(&self) {
        self.window.present();
    }

    // ----- probe accessors (headless gates; not app paths) -----

    /// The open reader tab count.
    pub fn state_reader_tab_count(&self) -> usize {
        self.state.reader.tab_count()
    }

    /// The current reader slot id.
    pub fn state_reader_slot(&self) -> Option<usize> {
        self.state.reader.current_slot_id()
    }

    /// The slot of the FIRST open tab (the Open Books first row).
    pub fn state_first_open_slot(&self) -> Option<usize> {
        self.state.reader.open_tabs().first().map(|(s, _)| *s)
    }

    /// Whether the CURRENT reader page carries a bookmark.
    pub fn state_current_page_bookmark(&self) -> bool {
        self.state.current_page_has_bookmark()
    }

    /// Selects the current view's first book (the rating path).
    pub fn state_select_first_book(&self) {
        let view = self.state.item_view.view_state();
        if let Some(first) = view.books().first() {
            self.state.item_view.select_book(&first.id);
            self.state.sync_enabled();
        }
    }

    /// Fires a detailed action on the window (the dispatch path).
    pub fn state_dispatch(&self, action: &str) -> bool {
        gtk4::prelude::WidgetExt::activate_action(&self.window, action, None).is_ok()
    }

    /// Fires an action with an explicit string parameter (bare name
    /// + variant).
    pub fn state_dispatch_param(&self, action: &str, value: &str) -> bool {
        gtk4::prelude::WidgetExt::activate_action(&self.window, action, Some(&value.to_variant()))
            .is_ok()
    }

    /// The state of a rating check action.
    pub fn state_rating_checked(&self, n: u32) -> bool {
        self.state
            .action(&format!("rating-{n}"))
            .and_then(|a| a.state())
            .and_then(|v| v.get::<bool>())
            .unwrap_or(false)
    }

    /// Whether an action is enabled (the probe).
    pub fn state_action_enabled(&self, name: &str) -> bool {
        self.state.action(name).is_some_and(|a| a.is_enabled())
    }

    /// The grid's book count (the probe).
    pub fn state_grid_book_count(&self) -> usize {
        self.state.item_view.book_count()
    }

    /// The grid's selection length (the probe).
    pub fn state_grid_selection_len(&self) -> usize {
        self.state.item_view.selection_len()
    }

    /// The grid's selection ids (the probe).
    pub fn state_grid_selection_ids(&self) -> Vec<CrGuid> {
        self.state.item_view.selection_ids()
    }

    /// The visible workspace stack page name (the probe).
    pub fn state_stack_page(&self) -> String {
        self.state
            .stack
            .visible_child_name()
            .map(|n| n.to_string())
            .unwrap_or_default()
    }

    /// The current page image's size via `create_page_image` (the
    /// T6 probe).
    pub fn state_page_image_size(&self) -> Option<(i32, i32)> {
        self.state
            .reader
            .current_view()
            .and_then(|v| v.create_page_image())
            .map(|s| (s.width(), s.height()))
    }

    /// Scrolls the book grid (the context-menu probe).
    pub fn state_item_scroll_to(&self, y: f64) -> f64 {
        self.state.item_view.probe_scroll_to(y)
    }

    /// The book grid's live scroll value (the context-menu probe).
    pub fn state_item_scroll_value(&self) -> f64 {
        self.state.item_view.probe_scroll_value()
    }

    /// Fires the right-click hook through the shared gesture body
    /// (the context-menu probe).
    pub fn state_trigger_context(&self, x: f64, y: f64) {
        self.state.item_view.probe_context(x, y);
    }

    /// The selected book's rating in the library (the probe).
    pub fn state_selected_book_rating(&self) -> f32 {
        let ids = self.state.item_view.selection_ids();
        if ids.is_empty() {
            return f32::NAN;
        }
        let lib = library::session();
        let l = lib.borrow();
        l.database()
            .books
            .iter()
            .find(|b| ids.contains(&b.id))
            .map(|b| b.rating)
            .unwrap_or(f32::NAN)
    }

    /// The current reader page's type (the probe).
    pub fn state_current_page_type(&self) -> Option<i16> {
        let book = self.state.reader.current_comic_book()?;
        let display = self.state.reader.current_display_page()?;
        let provider = self.state.reader.provider_index_of_display(display)?;
        book.info.pages.get(provider).map(|p| p.page_type.0 as i16)
    }

    /// The reader toolbar handle (the probe).
    pub fn toolbar_widget(&self) -> gtk4::Widget {
        self.state.toolbar.widget().clone().upcast()
    }

    /// The browser toolbar dropdown by name (the probe).
    pub fn browserbar_dropdown(&self, name: &str) -> Option<crate::browser::menubar::Dropdown> {
        self.state.browser_toolbar.dropdown(name)
    }

    /// Opens a browser toolbar dropdown through its real anchor.
    pub fn browserbar_open_dropdown(&self, name: &str) -> bool {
        self.state.browser_toolbar.open_dropdown(name)
    }

    /// Whether a browser toolbar dropdown's popover is mapped.
    pub fn browserbar_drop_mapped(&self, name: &str) -> bool {
        self.state.browser_toolbar.drop_mapped(name)
    }

    /// Closes one browser toolbar dropdown.
    pub fn browserbar_close_dropdown(&self, name: &str) {
        self.state.browser_toolbar.close_dropdown(name);
    }

    /// A stateful action's string state (the probe).
    pub fn state_action_string(&self, name: &str) -> Option<String> {
        self.state
            .action(name)
            .and_then(|a| a.state())
            .and_then(|v| v.get::<String>())
    }

    /// A stateful action's bool state (the probe).
    pub fn state_action_bool(&self, name: &str) -> Option<bool> {
        self.state
            .action(name)
            .and_then(|a| a.state())
            .and_then(|v| v.get::<bool>())
    }

    /// The search box cue text (the scope probe).
    pub fn state_search_placeholder(&self) -> String {
        self.state
            .search
            .placeholder_text()
            .unwrap_or_default()
            .to_string()
    }

    /// The Detail column snapshot (id, name, visible).
    pub fn state_columns_snapshot(&self) -> Vec<(i32, String, bool)> {
        self.state.item_view.detail_columns_snapshot()
    }

    /// The visible stack page name (the probe).
    pub fn state_visible_page(&self) -> Option<String> {
        self.state.stack.visible_child_name().map(|s| s.to_string())
    }

    /// The Tasks dialog's single-instance visibility (the probe).
    pub fn state_tasks_window_visible(&self) -> bool {
        self.state
            .tasks_window
            .borrow()
            .as_ref()
            .is_some_and(|w| w.is_visible())
    }

    /// The current reader zoom (the Custom Zoom gate).
    pub fn state_current_zoom(&self) -> Option<f32> {
        self.state.reader.current_zoom()
    }

    /// Opens the column chooser through the real hook path (the
    /// probe's OPEN gate).
    pub fn state_open_column_chooser(&self, wx: f64, wy: f64) -> bool {
        self.state.popup_column_chooser(wx, wy);
        self.state
            .columns_drop
            .borrow()
            .as_ref()
            .is_some_and(|p| p.is_mapped())
    }

    /// The chooser popover's child natural height (the probe: the
    /// list must be taller than a couple of rows — the "two lines
    /// high" report).
    pub fn state_column_chooser_height(&self) -> i32 {
        self.state
            .columns_drop
            .borrow()
            .as_ref()
            .and_then(|p| p.child())
            .map(|c| c.measure(gtk4::Orientation::Vertical, -1).1)
            .unwrap_or(0)
    }

    /// The browser toolbar's Group/Arrange label texts (the probe).
    pub fn browserbar_labels(&self) -> (String, String) {
        self.state.browser_toolbar.label_texts()
    }

    // --- T7 probe accessors (the navigator + Pages toolbars) ---

    /// Clicks a navigator toolbar button through the real handler.
    pub fn nav_click_button(&self, name: &str) -> bool {
        self.state.navigator.click_button(name)
    }

    /// Whether the navigator search box shows.
    pub fn nav_search_visible(&self) -> bool {
        self.state.navigator.search_visible()
    }

    /// Sets the navigator search text (the typing path — `set_text`
    /// fires the same changed signal).
    pub fn nav_set_search_text(&self, text: &str) {
        self.state.navigator.set_search_text(text);
    }

    /// The navigator tree row count (the filter evidence).
    pub fn nav_row_count(&self) -> usize {
        self.state.navigator.row_count()
    }

    /// The expanded navigator rows (the expand/collapse-all evidence).
    pub fn nav_expanded_count(&self) -> usize {
        self.state.navigator.expanded_count()
    }

    /// The Pages grid mode.
    pub fn pages_mode(&self) -> super::pages_view::PagesMode {
        self.state.pages.mode()
    }

    /// Opens the Pages Views drop through its real anchor (the OPEN
    /// gate).
    pub fn pages_open_views(&self) -> bool {
        self.state.pages.open_views()
    }

    /// Closes the Pages Views drop (the probe cleanup).
    pub fn pages_close_views(&self) {
        self.state.pages.close_views();
    }

    /// Clicks a Pages Views radio row through the real handler.
    pub fn pages_click_view(&self, action: &str) -> bool {
        self.state.pages.click_view(action)
    }

    /// Clicks the Pages Views MAIN part (the mode cycle — the C#
    /// `tbbView_ButtonClick`).
    pub fn pages_click_main(&self) {
        self.state.pages.click_main();
    }

    /// Sets the search text through the composed-filter path (the
    /// probe; the entry typing itself is a user-test matter).
    pub fn state_set_search_text(&self, text: &str) {
        *self.state.search_text.borrow_mut() = text.to_string();
        self.state.rebuild_filter();
    }

    /// The toolbar's zoom state text (the probe).
    pub fn toolbar_zoom_text(&self) -> String {
        self.state.toolbar.zoom_text()
    }

    /// The toolbar's rotation state text (the probe).
    pub fn toolbar_rotate_label(&self) -> String {
        self.state.toolbar.rotate_text()
    }

    /// The toolbar dropdown by name (the probe).
    pub fn toolbar_dropdown(&self, name: &str) -> Option<crate::browser::menubar::Dropdown> {
        self.state.toolbar.dropdown(name)
    }

    /// Opens a toolbar dropdown through its real anchor (the probe).
    pub fn toolbar_open_dropdown(&self, name: &str) -> bool {
        self.state.toolbar.open_dropdown(name)
    }

    /// Closes one toolbar dropdown (the probe).
    pub fn toolbar_close_dropdown(&self, name: &str) {
        self.state.toolbar.close_dropdown(name);
    }

    /// Whether a toolbar dropdown's popover is mapped (the probe).
    pub fn toolbar_drop_mapped(&self, name: &str) -> bool {
        self.state
            .toolbar
            .dropdown(name)
            .is_some_and(|d| d.popover().is_mapped())
    }

    /// The current fit mode as the action name (the probe).
    pub fn reader_current_fit_name(&self) -> Option<&'static str> {
        self.state.reader.current_fit_mode().map(fit_action_name)
    }

    /// Sets a bookmark on a provider page of the CURRENT book,
    /// bypassing the prompt (the probe).
    pub fn state_set_bookmark_silent(&self, provider: usize, name: &str) {
        let book = self.state.reader.edit_current_book(|b| {
            if let Some(p) = b.info.pages.get_mut(provider) {
                p.bookmark = Some(name.to_string());
            }
        });
        if let Some(book) = book {
            library::apply_edited(&book);
        }
    }
}

impl ShellState {
    fn action(&self, name: &str) -> Option<gio::SimpleAction> {
        self.actions.borrow().get(name).cloned()
    }

    fn set_action_enabled(&self, name: &str, enabled: bool) {
        if let Some(a) = self.actions.borrow().get(name) {
            a.set_enabled(enabled);
        }
    }

    /// Registers one parameterless action with a `&ShellState`
    /// handler (the `CommandMapper.Add` one-command-one-handler
    /// shape).
    fn add_simple<F: Fn(&Rc<ShellState>) + 'static>(
        self: &Rc<ShellState>,
        group: &gio::SimpleActionGroup,
        name: &'static str,
        f: F,
    ) {
        let action = gio::SimpleAction::new(name, None);
        let state = Rc::downgrade(self);
        action.connect_activate(move |_, _| {
            crate::trace::trace(format!("action {name} activated"));
            if let Some(sh) = state.upgrade() {
                f(&sh);
                // The C# re-syncs the command enable/check states on
                // every menu operation (`CommandMapper` idle update);
                // every action dispatch refreshes ours.
                sh.sync_enabled();
            } else {
                crate::trace::trace(format!("action {name}: shell gone — silent no-op"));
            }
        });
        group.add_action(&action);
        self.actions.borrow_mut().insert(name, action);
    }

    /// The enable-state sync (`CommandMapper` idle update parity):
    /// reader commands need an open book, the edit commands a
    /// selection, Previous/Next List a walkable history. The
    /// radio/check actions take their state from the reader.
    fn sync_enabled(&self) {
        // The reader commands gate on the CURRENT slot's book (the
        // C# `ComicDisplay.Book != null` — an AddSlot slot stays
        // empty).
        let has_book = self.reader.has_current_book();
        let slots = self.reader.tab_count();
        let selected = self.item_view.selection_len();
        let (can_prev, can_next) = {
            let h = self.list_history.borrow();
            let pos = self.list_history_pos.get();
            (pos > 0, pos + 1 < h.len())
        };
        // Reader commands (`ComicDisplay.Book != null`).
        for name in [
            "close",
            "close-all",
            "first-page",
            "prev-page",
            "next-page",
            "last-page",
            "prev-bookmark",
            "next-bookmark",
            "last-page-read",
            "auto-scroll",
            "double-auto-scroll",
            "show-in-browser",
            "prev-book",
            "next-book",
            "random-book",
            "full-screen",
            "magnifier",
            "minimal-gui",
            "undock-reader",
            "copy-page",
            "export-page",
        ] {
            self.set_action_enabled(name, has_book);
        }
        self.set_action_enabled("prev-tab", slots > 1);
        self.set_action_enabled("next-tab", slots > 1);
        // Selection commands (`GetBookList(Selected)` non-empty).
        for name in [
            "info",
            "rating-0",
            "rating-1",
            "rating-2",
            "rating-3",
            "rating-4",
            "rating-5",
            "quick-rating",
        ] {
            self.set_action_enabled(name, selected > 0);
        }
        // Bookmark commands (`CanBookmark`/`CanNavigateBookmark`/
        // the current-page bookmark check).
        self.set_action_enabled("set-bookmark", has_book);
        self.set_action_enabled(
            "remove-bookmark",
            has_book && self.current_page_has_bookmark(),
        );
        self.set_action_enabled(
            "prev-bookmark",
            has_book && self.reader.can_navigate_bookmark(-1),
        );
        self.set_action_enabled(
            "next-bookmark",
            has_book && self.reader.can_navigate_bookmark(1),
        );
        // The My Rating check states (`Math.Round(GetRating()) == N`
        // — the selection's COMMON rating, -1 = mixed).
        let common = self.selection_common_rating();
        for (n, name) in [
            (0u32, "rating-0"),
            (1, "rating-1"),
            (2, "rating-2"),
            (3, "rating-3"),
            (4, "rating-4"),
            (5, "rating-5"),
        ] {
            if let Some(a) = self.action(name) {
                let checked = common >= 0.0 && common.round() == n as f32;
                a.set_state(&checked.to_variant());
            }
        }
        self.set_action_enabled("prev-list", can_prev);
        self.set_action_enabled("next-list", can_next);
        // Radio/check state follows the reader (`IsPageFitBest`,
        // `IsPageSingle`, `RightToLeftReading` checks).
        if let Some(fit) = self.reader.current_fit_mode() {
            if let Some(a) = self.action("page-fit") {
                a.set_state(&fit_action_name(fit).to_variant());
            }
        }
        if let Some(layout) = self.reader.current_page_layout() {
            if let Some(a) = self.action("page-layout") {
                a.set_state(&layout_action_name(layout).to_variant());
            }
        }
        if let Some(rtl) = self.reader.current_rtl() {
            if let Some(a) = self.action("right-to-left") {
                a.set_state(&rtl.to_variant());
            }
        }
        // The check states (`CommandMapper` check lambdas):
        // The view-mode radio state follows the VIEW (the source of
        // truth — the T6 report: the Views check never moved).
        if let Some(a) = self.action("view-mode") {
            let name = match self.item_view.mode() {
                ItemViewMode::Thumbnail => "thumbnail",
                ItemViewMode::Tile => "tile",
                ItemViewMode::Detail => "detail",
            };
            a.set_state(&name.to_variant());
        }
        // `() => BrowserVisible`, `() => Program.Settings.AutoScrolling`
        // (the view mirrors it), `() => ComicDisplay.TwoPageNavigation`,
        // MinimalGui / FullScreen / Autorotate. `BrowserVisible` is
        // true on BOTH browser workspaces (the C# browser container
        // holds the Library and Pages views).
        if let Some(a) = self.action("toggle-browser") {
            let visible = matches!(
                self.stack.visible_child_name().as_deref(),
                Some("browser") | Some("pages")
            );
            a.set_state(&visible.to_variant());
        }
        if let Some(v) = self.reader.current_auto_scrolling() {
            if let Some(a) = self.action("auto-scroll") {
                a.set_state(&v.to_variant());
            }
        }
        if let Some(v) = self.reader.current_two_page_navigation() {
            if let Some(a) = self.action("double-auto-scroll") {
                a.set_state(&v.to_variant());
            }
        }
        if let Some(v) = self.reader.current_auto_rotate() {
            if let Some(a) = self.action("auto-rotate") {
                a.set_state(&v.to_variant());
            }
        }
        if let Some(a) = self.action("minimal-gui") {
            a.set_state(&self.reader.is_minimal_gui().to_variant());
        }
        if let Some(a) = self.action("full-screen") {
            a.set_state(&self.reader.is_fullscreen().to_variant());
        }
        // `tbShowMainMenu`: checked while the menu is NOT
        // auto-hidden.
        if let Some(a) = self.action("show-main-menu") {
            let checked = !cr_ui_settings().borrow().auto_hide_main_menu;
            a.set_state(&checked.to_variant());
        }
        // The navigator search toggle: the check = the box visibility
        // (`() => quickSearchPanel.Visible`).
        if let Some(a) = self.action("toggle-navigator-search") {
            a.set_state(&self.navigator.search_visible().to_variant());
        }
        // The Pages grid mode radio (the source of truth is the
        // panel — the main click cycles through the action).
        if let Some(a) = self.action("pages-view-mode") {
            a.set_state(&self.pages.mode().action_name().to_variant());
        }
        // `() => Program.Settings.TrackCurrentPage` — the page-panel
        // lock icon + the menu check derive from the setting.
        if let Some(a) = self.action("track-current-page") {
            a.set_state(&cr_ui_settings().borrow().track_current_page.to_variant());
        }
        // The workspace tab strip (tabs, selection, Pages visibility).
        self.sync_tabs();
        self.update_menubar();
        // The status panels ride the same sync (the `OnUpdateGui`
        // strip updates run in the same idle pass).
        self.update_status_panels();
        self.sync_menubar();
    }

    /// Applies the chrome visibility rules plus the T5 toolbar
    /// visibility. The MENUBAR shows ALWAYS in the normal windowed
    /// state (user decision 2026-09-05: the C# `AutoHideMainMenu`
    /// auto-hide and the Alt-alone reveal are not ported — the C#
    /// rule lives on in `menubar::menubar_visible` + its tests for
    /// the record); MinimalGui/fullscreen chrome still hide it.
    fn update_menubar(&self) {
        let minimal = self.reader.is_minimal_gui();
        let undocked = self.reader.is_undocked();
        let is_comic_viewer = self.stack.visible_child_name().as_deref() == Some("reader");
        let open_books = self.reader.open_book_count();
        let show_no_comic = cr_ui_settings().borrow().show_main_menu_no_comic_open;
        self.menubar.widget().set_visible(!minimal);
        // The tab strip + the status strip ride the Fill-mode `flag4`
        // (`OnGuiVisibilities`: `mainView.TabBarVisible` and
        // `statusStripVisibility.Visible` share it).
        let strip_visible = super::tabstrip::tabstrip_visible(
            minimal,
            undocked,
            is_comic_viewer,
            open_books,
            show_no_comic,
        );
        self.tab_strip.widget().set_visible(strip_visible);
        self.status_bar.widget().set_visible(strip_visible);
        // The toolbar: visible while the reader view shows and
        // MinimalGui is off (the C# `MainToolStripVisible`); the
        // reader-only buttons gate on the current book (`OnUpdateGui`).
        self.toolbar
            .sync_visibility(self.reader.has_current_book(), !minimal);
    }

    /// Pushes the current action states into the menubar rows and
    /// the toolbar dropdowns (check/radio marks + disabled graying +
    /// the hide rules — the custom bars have no model-driven state
    /// rendering).
    fn sync_menubar(&self) {
        let actions = self.actions.borrow();
        // The active-panel emphasis (the C# highlights the
        // miViewLibrary/miViewPages row of the shown workspace — no
        // checkbox on those items).
        let panel = match self.stack.visible_child_name().as_deref() {
            Some("browser") => "library",
            Some("pages") => "pages",
            _ => "",
        };
        // `fileMenu_DropDownOpening`: "Update all Book Files" hides
        // while `AutoUpdateComicsFiles` is on.
        let update_files_visible = !cr_ui_settings().borrow().auto_update_comics_files;
        let resolve = |base: &str| {
            let action = actions.get(base)?;
            let highlight = match base {
                "view-library" => panel == "library",
                "view-pages" => panel == "pages",
                _ => false,
            };
            let visible = match base {
                "update-book-files" => update_files_visible,
                _ => true,
            };
            // The dark-mode check derives from the ExtendedSettings
            // global (the source of truth — the T6 lesson: never
            // trust the click side to have updated the state).
            let state = if base == "dark-mode" {
                Some(
                    (cr_core::settings::ExtendedSettings::global().effective_theme()
                        == cr_core::settings::enums::Themes::Dark)
                        .to_variant(),
                )
            } else {
                action.state()
            };
            Some(super::menubar::ActionState {
                enabled: action.is_enabled(),
                state,
                highlight,
                visible,
            })
        };
        self.menubar.sync(&resolve);
        self.toolbar.sync(&resolve);
        // The Pages panel's Views drop (the T7 mode radios).
        self.pages.sync(&resolve);
        // The browser toolbar (the T6 strip): the enable states +
        // the Group/Arrange label texts (`OnIdle` tbbSort/tbbGroup).
        self.browser_toolbar.sync(&resolve);
        let (sort_col, sort_desc, grouper) = self.item_view.sort_group_summary();
        let sort_label = sort_col.and_then(|p| {
            default_columns()
                .iter()
                .find(|c| c.property == p)
                .map(|c| c.name.to_string())
        });
        self.browser_toolbar
            .sync_labels(sort_label, sort_desc, grouper);
        // The state text/icons (`viewer_PageDisplayModeChanged`).
        self.toolbar.sync_state(
            self.reader.current_zoom(),
            self.reader.current_rotation(),
            self.reader.current_fit_mode(),
            self.reader.current_page_layout(),
            self.reader.current_rtl(),
            self.reader.current_magnifier(),
            self.reader.current_auto_rotate(),
        );
        // The submenu PARENT enables (`OnGuiVisibilities` +
        // `DropDownOpening` rules).
        self.menubar
            .set_sub_enabled("Open Books", self.reader.tab_count() > 0);
        let recent_count = library::recent_books(20)
            .iter()
            .filter(|b| Path::new(&b.file_path).exists())
            .count();
        self.menubar
            .set_sub_enabled("Recent Books", recent_count > 0);
        let has_book = self.reader.has_current_book();
        self.menubar.set_sub_enabled("Page Type", has_book);
        self.menubar.set_sub_enabled("Page Rotation", has_book);
    }

    /// `OpenNextComic(relative)`: the neighbor book in the current
    /// list's view order; `relative == 0` picks a random book without
    /// repeats until the cycle wraps (`lastRandomList`/
    /// `randomSelectedComics` parity). The current book must be part
    /// of the viewed list — the C# resolves the book's own browser
    /// container, we use the active view.
    fn open_next_book(&self, relative: i32) {
        let Some(current) = self.reader.current_comic_book() else {
            return;
        };
        let view = self.item_view.view_state();
        let ids: Vec<CrGuid> = view
            .display_order()
            .iter()
            .map(|&i| view.books()[i].id)
            .collect();
        let Some(pos) = ids.iter().position(|id| *id == current.id) else {
            return;
        };
        let next = if relative == 0 {
            let reset = {
                let mut list = self.random_list.borrow_mut();
                if *list != ids {
                    *list = ids.clone();
                    self.random_picked.borrow_mut().clear();
                }
                let mut picked = self.random_picked.borrow_mut();
                if picked.len() >= ids.len() {
                    picked.clear();
                }
                let remaining: Vec<CrGuid> = ids
                    .iter()
                    .filter(|id| !picked.contains(id))
                    .copied()
                    .collect();
                // `new Random().Next(0, remaining)` — the C# uses an
                // unseeded Random; a time-seeded one matches the
                // behavior class.
                let seed = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.subsec_nanos() as i32)
                    .unwrap_or(0);
                let choice =
                    remaining[cr_engine::sort::DotNetRandom::new(seed).next(remaining.len())];
                picked.push(choice);
                choice
            };
            Some(reset)
        } else {
            let idx = pos as i32 + relative;
            if idx >= 0 && (idx as usize) < ids.len() {
                Some(ids[idx as usize])
            } else {
                None
            }
        };
        if let Some(id) = next {
            if let Some(path) = library::book_path(&id) {
                self.open_comic(Path::new(&path));
            }
        }
    }

    /// Walks the list browsing history (Previous/Next List).
    fn browse_history(&self, dir: i32) {
        let next = self.list_history_pos.get() as i64 + dir as i64;
        let id = {
            let h = self.list_history.borrow();
            if next < 0 || next as usize >= h.len() {
                return;
            }
            h[next as usize]
        };
        self.list_history_pos.set(next as usize);
        self.navigator.select_list(&id);
        self.sync_enabled();
    }

    /// `ShowInfo` (Ctrl+I): the editor over the selection — the bulk
    /// editor for several books (`MultipleComicBooksDialog`), the
    /// book editor otherwise.
    fn show_info(self: &Rc<ShellState>) {
        let ids = self.item_view.selection_ids();
        if ids.is_empty() {
            return;
        }
        let books = Self::books_by_ids(&ids);
        if books.is_empty() {
            return;
        }
        if books.len() > 1 {
            self.open_bulk_editor(books);
        } else {
            self.open_editor(books);
        }
    }

    fn books_by_ids(ids: &[CrGuid]) -> Vec<ComicBook> {
        let lib = library::session();
        let l = lib.borrow();
        l.database()
            .books
            .iter()
            .filter(|b| ids.contains(&b.id))
            .cloned()
            .collect()
    }

    /// The editor commit: `apply_edited` (the library replace + the
    /// dirty mark + the debounced file write) and a grid refresh.
    fn editor_commit(self: &Rc<ShellState>) -> crate::dialogs::book_editor::CommitFn {
        let state = Rc::downgrade(self);
        Rc::new(move |edited| {
            library::apply_edited(edited);
            if let Some(sh) = state.upgrade() {
                sh.refresh_view_from_list();
            }
        })
    }

    fn open_editor(self: &Rc<ShellState>, books: Vec<ComicBook>) {
        let commit = self.editor_commit();
        // The editor shares the app pool (the C# `Program.ImagePool`
        // is global — a private pool would re-decode every cover).
        crate::dialogs::book_editor::show(&self.window, books, commit, Arc::clone(&self.pool));
    }

    /// `AddNewBook(showDialog: true)` (MainForm.cs:1879): a fresh
    /// fileless book (no file path, `AddedTime = now`, a new id)
    /// opens the book editor. The commit inserts it into the database
    /// on the first save point (the C# adds after the dialog's OK;
    /// the port inserts idempotently so later Apply commits degrade
    /// to `apply_edited`). Cancel closes without a commit.
    fn open_new_book_editor(self: &Rc<ShellState>) {
        let book = crate::dialogs::new_book_series::new_fileless_book();
        let state = Rc::downgrade(self);
        let commit: crate::dialogs::book_editor::CommitFn = Rc::new(move |edited| {
            let inserted = library::insert_new_book(edited);
            if !inserted {
                library::apply_edited(edited);
            }
            if let Some(sh) = state.upgrade() {
                sh.refresh_view_from_list();
            }
        });
        crate::dialogs::book_editor::show(&self.window, vec![book], commit, Arc::clone(&self.pool));
    }

    /// The NewComics.py port: the dialog creates N = to-from+1
    /// fileless books (`Number = str(n)`, the shared series/volume)
    /// and selects them (the script's `Browser.SelectComics`). A
    /// range over 100 aborts silently — the script's sanity check.
    fn new_book_series(self: &Rc<ShellState>) {
        let state = Rc::downgrade(self);
        crate::dialogs::new_book_series::show(&self.window, move |series, volume, first, last| {
            if last - first > 100 {
                return;
            }
            let mut ids = Vec::with_capacity((last - first + 1).max(0) as usize);
            for n in first..=last {
                let mut book = crate::dialogs::new_book_series::new_fileless_book();
                book.info.series = series.clone();
                book.info.number = n.to_string();
                book.info.volume = volume;
                library::insert_new_book(&book);
                ids.push(book.id);
            }
            if let Some(sh) = state.upgrade() {
                sh.refresh_view_from_list();
                sh.item_view.reselect(&ids);
            }
        });
    }

    fn open_bulk_editor(self: &Rc<ShellState>, books: Vec<ComicBook>) {
        let commit = self.editor_commit();
        crate::dialogs::bulk_edit::show(&self.window, books, commit);
    }

    /// `SetRating(n)` over the selection (the My Rating menu).
    fn set_rating(&self, rating: f32) {
        let ids = self.item_view.selection_ids();
        if ids.is_empty() {
            return;
        }
        for mut book in Self::books_by_ids(&ids) {
            book.rating = rating;
            library::apply_edited(&book);
        }
        self.refresh_view_from_list();
    }

    /// `RatingEditor.GetRating`: the selection's rating when all
    /// selected books agree, else -1 (mixed / empty).
    fn selection_common_rating(&self) -> f32 {
        let ids = self.item_view.selection_ids();
        if ids.is_empty() {
            return -1.0;
        }
        let lib = library::session();
        let l = lib.borrow();
        let mut num = -1.0f32;
        for book in l.database().books.iter().filter(|b| ids.contains(&b.id)) {
            if num == -1.0 {
                num = book.rating;
            } else if num != book.rating {
                return -1.0;
            }
        }
        num
    }

    /// Whether the CURRENT reader page carries a bookmark
    /// (`RemoveBookmarkAvailable`).
    fn current_page_has_bookmark(&self) -> bool {
        let Some(book) = self.reader.current_comic_book() else {
            return false;
        };
        let Some(display) = self.reader.current_display_page() else {
            return false;
        };
        let Some(provider) = self.reader.provider_index_of_display(display) else {
            return false;
        };
        book.info
            .pages
            .get(provider)
            .and_then(|p| p.bookmark.as_deref())
            .is_some_and(|b| !b.is_empty())
    }

    /// The current provider page of the open book (the page-edit
    /// target). `None` without an open comic.
    fn current_provider_page(&self) -> Option<(usize, usize)> {
        let display = self.reader.current_display_page()?;
        let provider = self.reader.provider_index_of_display(display)?;
        Some((display, provider))
    }

    /// Applies an edit to the CURRENT reader book: the session copy
    /// mutates, the library entry replaces (`apply_edited` — the
    /// dirty mark + the gated file write), and the Pages panel
    /// rebinds. `None` without an open comic.
    fn edit_open_book<F: FnOnce(&mut ComicBook)>(&self, f: F) -> Option<ComicBook> {
        let book = self.reader.edit_current_book(f)?;
        library::apply_edited(&book);
        self.pages.set_book(book.clone());
        Some(book)
    }

    /// The dynamic fill provider (the `DropDownOpening` parity):
    /// every menu open rebuilds the dynamic slots from the live
    /// book/tabs state. The check/disabled state is baked here —
    /// the C# also refreshes at `DropDownOpening`, not through the
    /// command states.
    fn install_dyn_fills(self: &Rc<ShellState>) {
        let state = Rc::downgrade(self);
        let fill: crate::browser::menubar::DynFillFn = Rc::new(move |id| match state.upgrade() {
            Some(sh) => sh.dyn_fill(id),
            None => Vec::new(),
        });
        self.menubar.set_dyn_fill(Rc::clone(&fill));
        self.toolbar.set_dyn_fill(fill.clone());
        // The browser toolbar's Duplicate List drop shares it.
        self.browser_toolbar.set_dyn_fill(fill);
        // The toolbar rides into the undocked window (the T5
        // chrome).
        self.reader.set_undock_chrome(
            self.toolbar.widget().clone().upcast(),
            self.reader_page_box.clone(),
        );
    }

    fn dyn_fill(&self, id: &str) -> Vec<super::menubar::DynNode> {
        use super::menubar::{DynItem, DynNode};
        match id {
            // File ▸ Open Books: one row per open tab, checked on
            // the current, Ctrl+Alt+F1..F12 on the first 12.
            "open-books" => {
                let current = self.reader.current_slot_id();
                self.reader
                    .open_tabs()
                    .into_iter()
                    .enumerate()
                    .map(|(i, (slot, caption))| {
                        let accel = if i < 12 {
                            format!("<Control><Alt>F{}", i + 1)
                        } else {
                            String::new()
                        };
                        let detailed = format!("win.open-tab::{slot}");
                        if !accel.is_empty() {
                            self.app.set_accels_for_action(&detailed, &[accel.as_str()]);
                        }
                        DynNode::Item(DynItem {
                            label: caption,
                            action: detailed,
                            accel,
                            icon: "",
                            checked: current == Some(slot),
                            enabled: true,
                        })
                    })
                    .collect()
            }
            // File ▸ Recent Books: numbered file names, existing
            // files only (`RecentFilesMenuOpening`).
            "recent-books" => {
                let mut out = Vec::new();
                let mut n = 0usize;
                for book in library::recent_books(20) {
                    if !Path::new(&book.file_path).exists() {
                        continue;
                    }
                    n += 1;
                    let name = Path::new(&book.file_path)
                        .file_name()
                        .map(|f| f.to_string_lossy().into_owned())
                        .unwrap_or_else(|| book.file_path.clone());
                    out.push(DynNode::Item(DynItem {
                        label: format!("{n} - {name}"),
                        // The raw path rides the detailed name's
                        // value (split_once takes everything after
                        // the first "::" — colons in paths survive).
                        action: format!("win.recent-book::{}", book.file_path),
                        accel: String::new(),
                        icon: "",
                        checked: false,
                        enabled: true,
                    }));
                }
                out
            }
            // Edit ▸ Bookmarks: the per-page list (the C# "bm"
            // items — disabled on the current page).
            "bookmarks" => {
                let Some(book) = self.reader.current_comic_book() else {
                    return Vec::new();
                };
                let Some(current) = self.current_provider_page().map(|(_, provider)| provider)
                else {
                    return Vec::new();
                };
                book.info
                    .pages
                    .iter()
                    .enumerate()
                    .filter(|(_, p)| p.bookmark.as_deref().is_some_and(|b| !b.is_empty()))
                    .map(|(i, p)| {
                        let name = p.bookmark.clone().unwrap_or_default();
                        DynNode::Item(DynItem {
                            label: format!("{name} (Page {})", i + 1),
                            action: format!("win.open-bookmark::{i}"),
                            accel: String::new(),
                            icon: "",
                            checked: false,
                            enabled: i != current,
                        })
                    })
                    .collect()
            }
            // The TOOLBAR bookmark drops (`tbPrevPage`/
            // `tbNextPage_DropDownOpening` → `UpdateBookmarkMenu(
            // direction)`): the bookmarks BEFORE (-1) / AFTER (+1)
            // the current page; all rows clickable.
            "bookmarks-prev" | "bookmarks-next" => {
                let dir = if id == "bookmarks-prev" { -1 } else { 1 };
                let Some(book) = self.reader.current_comic_book() else {
                    return Vec::new();
                };
                let Some(current) = self.current_provider_page().map(|(_, provider)| provider)
                else {
                    return Vec::new();
                };
                let mut rows: Vec<_> = book
                    .info
                    .pages
                    .iter()
                    .enumerate()
                    .filter(|(i, p)| {
                        p.bookmark.as_deref().is_some_and(|b| !b.is_empty())
                            && if dir < 0 { *i < current } else { *i > current }
                    })
                    .map(|(i, p)| {
                        let name = p.bookmark.clone().unwrap_or_default();
                        DynNode::Item(DynItem {
                            label: format!("{name} (Page {})", i + 1),
                            action: format!("win.open-bookmark::{i}"),
                            accel: String::new(),
                            icon: "",
                            checked: false,
                            enabled: true,
                        })
                    })
                    .collect();
                // The C# reverses for the backward direction (the
                // nearest bookmark first).
                if dir < 0 {
                    rows.reverse();
                }
                rows
            }
            // Edit ▸ Page Type: the enum radio over the CURRENT
            // page (all rows disabled without a book — the C#
            // `pageEditor.IsValid` rule).
            "page-type" => {
                let has_book = !self.reader.is_empty();
                let current = self.current_provider_page().and_then(|(_, provider)| {
                    self.reader
                        .current_comic_book()
                        .and_then(|b| b.info.pages.get(provider).map(|p| p.page_type))
                });
                crate::dialogs::book_editor::PAGE_TYPE_ITEMS
                    .iter()
                    .map(|(label, v)| {
                        DynNode::Item(DynItem {
                            label: (*label).to_string(),
                            action: format!("win.page-type::{}", v.0),
                            accel: String::new(),
                            icon: "",
                            checked: current.is_some_and(|c| c == *v),
                            enabled: has_book,
                        })
                    })
                    .collect()
            }
            // Edit ▸ Page Rotation: the rotation radio with the C#
            // Permanent icons (`EnumMenuUtility` images dict).
            "page-rotation" => {
                let has_book = !self.reader.is_empty();
                let current = self
                    .reader
                    .current_view()
                    .map(|v| v.page_rotation_of(v.current_page()));
                const NONE: cr_core::model::enums::ImageRotation =
                    cr_core::model::enums::ImageRotation::None;
                [
                    ("None", NONE, "Rotate0Permanent"),
                    (
                        "90\u{b0}",
                        cr_core::model::enums::ImageRotation::Rotate90,
                        "Rotate90Permanent",
                    ),
                    (
                        "180\u{b0}",
                        cr_core::model::enums::ImageRotation::Rotate180,
                        "Rotate180Permanent",
                    ),
                    (
                        "270\u{b0}",
                        cr_core::model::enums::ImageRotation::Rotate270,
                        "Rotate270Permanent",
                    ),
                ]
                .into_iter()
                .map(|(label, rot, icon)| {
                    DynNode::Item(DynItem {
                        label: label.to_string(),
                        action: format!(
                            "win.page-rotation::{}",
                            match rot {
                                NONE => "none",
                                cr_core::model::enums::ImageRotation::Rotate90 => "90",
                                cr_core::model::enums::ImageRotation::Rotate180 => "180",
                                cr_core::model::enums::ImageRotation::Rotate270 => "270",
                            }
                        ),
                        accel: String::new(),
                        icon,
                        checked: current.is_some_and(|c| c == rot),
                        enabled: has_book,
                    })
                })
                .collect()
            }
            // The Duplicate List drop (`tbbDuplicateList_
            // DropDownOpening`): every folder of the tree, an indent
            // per child level; an empty tree shows a disabled None.
            "duplicate-list" => {
                let folders = library::list_folders();
                if folders.is_empty() {
                    return vec![DynNode::Item(DynItem {
                        label: "None".into(),
                        action: String::new(),
                        accel: String::new(),
                        icon: "",
                        checked: false,
                        enabled: false,
                    })];
                }
                folders
                    .into_iter()
                    .map(|(id, level, name)| {
                        DynNode::Item(DynItem {
                            label: format!("{}{}", " ".repeat(level * 4), name),
                            action: format!("win.duplicate-list::{}", id),
                            accel: String::new(),
                            icon: "",
                            checked: false,
                            enabled: true,
                        })
                    })
                    .collect()
            }
            _ => Vec::new(),
        }
    }

    /// `RefreshDisplay` (F5): the tree re-fills and the current list
    /// re-evaluates.
    fn refresh_view(&self) {
        self.navigator.refill(&library::comic_lists_snapshot());
        self.refresh_view_from_list();
    }

    /// `UpdateQuickFilter` + `UpdateSearch`: rebuild the composed
    /// filter (the quick-search text + the view filters + duplicates)
    /// and apply it to the grid. The result stays in
    /// `current_filter` — the Duplicate List source.
    fn rebuild_filter(&self) {
        let text = self.search_text.borrow().clone();
        let state_str = |name: &str, default: &str| -> String {
            self.action(name)
                .and_then(|a| a.state())
                .and_then(|v| v.get::<String>())
                .unwrap_or_else(|| default.to_string())
        };
        let scope = state_str("search-scope", "all");
        let show = state_str("view-filter", "all");
        let ctype = state_str("comic-type", "all");
        let dups = self
            .action("duplicates-only")
            .and_then(|a| a.state())
            .and_then(|v| v.get::<bool>())
            .unwrap_or(false);
        let matcher = compose_quick_filter(&text, &scope, &show, &ctype, dups);
        *self.current_filter.borrow_mut() = matcher.clone();
        self.item_view.set_filter(matcher);
        // The filter changed the visible set — the selection-info
        // panel follows without an action dispatch.
        self.update_status_panels();
    }

    /// The Detail header column chooser: a PLAIN popover of check-
    /// rows built fresh per open — the Wayland-proven shape of the
    /// book context menu. The T5/T6 `build_dropdown` popover
    /// (has_arrow off + submenu child popovers) fails to MAP when
    /// parented to the top-level window on Wayland; a plain popover
    /// maps fine.
    fn popup_column_chooser(self: &Rc<ShellState>, wx: f64, wy: f64) {
        let popover = gtk4::Popover::new();
        let list = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        list.set_margin_top(4);
        list.set_margin_bottom(4);
        list.set_margin_start(4);
        list.set_margin_end(4);
        let scroller = gtk4::ScrolledWindow::builder()
            .propagate_natural_width(true)
            // Without natural-height propagation the scroller
            // collapses to ~2 rows — request the list's full height
            // up to the cap (the user report).
            .propagate_natural_height(true)
            .max_content_height(480)
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .child(&list)
            .build();
        for (id, name, visible) in self.item_view.detail_columns_snapshot() {
            let check = gtk4::CheckButton::with_label(&name);
            check.set_active(visible);
            let state = Rc::downgrade(self);
            let popover_ref = popover.clone();
            check.connect_toggled(move |_| {
                if let Some(sh) = state.upgrade() {
                    sh.item_view.toggle_column_visible(id);
                    sh.sync_enabled();
                }
                let _ = &popover_ref;
            });
            list.append(&check);
        }
        popover.set_child(Some(&scroller));
        popover.set_parent(&self.window);
        popover.connect_closed(|p| p.unparent());
        let rect = gtk4::gdk::Rectangle::new(wx as i32, wy as i32 + 4, 1, 1);
        popover.set_pointing_to(Some(&rect));
        *self.columns_drop.borrow_mut() = Some(popover.clone());
        popover.popup();
    }

    /// The Preferences dialog (shared by the header button and the
    /// action): a modal settings clone committed on OK; the open
    /// reader views and the QuickOpen grid re-apply.
    fn show_preferences(self: &Rc<ShellState>) {
        let window = self.window.clone();
        let state = Rc::downgrade(self);
        crate::settings::preferences::show_preferences(&window, move || {
            if let Some(sh) = state.upgrade() {
                sh.reader.apply_settings_to_open_views();
                let size = cr_ui_settings().borrow().quick_open_thumbnail_size as f64;
                sh.quick_view.configure(|c| c.thumb_height = size);
                // Settings may move menu-visible state (the
                // update-book-files hide rule reads
                // AutoUpdateComicsFiles) — re-sync now, not on the
                // next unrelated dispatch.
                sh.sync_enabled();
            }
        });
    }

    /// The Tasks dialog (`ShowPendingTasks`): one instance — an open
    /// dialog re-presents (`taskDialog.Activate()`).
    fn show_tasks(self: &Rc<ShellState>) {
        if let Some(window) = self.tasks_window.borrow().as_ref() {
            window.present();
            return;
        }
        let dialog = crate::dialogs::tasks::show_tasks_dialog(&self.window, Arc::clone(&self.pool));
        *self.tasks_window.borrow_mut() = Some(dialog.window);
    }

    /// The About dialog (`ShowAboutDialog` — the splash image with
    /// the version line).
    fn show_about(self: &Rc<ShellState>) {
        crate::dialogs::about::show_about(&self.window);
    }

    /// Quick Rating and Review over the FIRST selected book (the C#
    /// `GetRatingEditor().QuickRatingAndReview()` →
    /// `books.FirstOrDefault()`); OK applies rating + review and
    /// stores the AutoShowQuickReview setting.
    fn show_quick_rating(self: &Rc<ShellState>) {
        let Some(id) = self.item_view.selection_ids().first().cloned() else {
            return;
        };
        let Some(book) = Self::books_by_ids(&[id]).into_iter().next() else {
            return;
        };
        let show_when_read = cr_ui_settings().borrow().auto_show_quick_review;
        let state = Rc::downgrade(self);
        let pool = Arc::clone(&self.pool);
        crate::dialogs::quick_rating::show_quick_rating(
            &self.window,
            &book,
            show_when_read,
            pool,
            move |result| {
                let Some(result) = result else {
                    return;
                };
                cr_ui_settings().borrow_mut().auto_show_quick_review = result.show_when_read;
                if let Some(sh) = state.upgrade() {
                    sh.set_quick_rating_fields(&book.id, result.rating, &result.review);
                    sh.sync_enabled();
                }
            },
        );
    }

    /// Applies the Quick Rating OK fields to one library book (the
    /// C# writes rating + review inside `QuickRatingDialog.Show` on
    /// OK). Books outside the library are skipped (the port keeps no
    /// session store for them after the tab closes).
    fn set_quick_rating_fields(&self, id: &CrGuid, rating: f32, review: &str) {
        let Some(mut book) = Self::books_by_ids(std::slice::from_ref(id))
            .into_iter()
            .next()
        else {
            return;
        };
        book.rating = rating;
        book.info.review = review.to_string();
        library::apply_edited(&book);
        self.refresh_view_from_list();
    }

    /// `ToggleBrowser`: the reader and the last browser workspace
    /// flip. From the QuickOpen page the browser shows (the user
    /// report: Browse ▸ Browser did nothing there); without an open
    /// book the reader side stays on the browser/QuickOpen.
    fn toggle_browser(&self) {
        let visible = self
            .stack
            .visible_child_name()
            .map(|s| s.to_string())
            .unwrap_or_default();
        match visible.as_str() {
            "reader" | "quickopen" => self.select_last_browser(),
            "browser" | "pages" if self.reader.has_current_book() => {
                self.stack.set_visible_child_name("reader");
            }
            _ => {}
        }
    }

    /// Builds the persisted workspace from the live widgets (the
    /// exit path; the C# `MainForm.CleanUp` copies the layout into
    /// `Settings.CurrentWorkspace`). `prev` keeps the reader layout
    /// when no view is open at exit (the display family always reads
    /// the live session copy).
    fn collect_workspace(
        &self,
        prev: Option<&cr_core::settings::workspace::WorkspaceState>,
    ) -> cr_core::settings::workspace::WorkspaceState {
        use crate::workspace::{browser_view_state, display_to_state, fit_name, layout_name};
        let (w, h) = (self.window.width(), self.window.height());
        let (sort_key, descending, grouper) = self.item_view.sort_group_summary();
        // The C# reader layout lives on the workspace whether or not
        // a book is open; without a view the previous save (or the
        // defaults) carries over.
        let prev_reader = prev.map(|p| p.reader.clone());
        let reader = cr_core::settings::workspace::ReaderLayoutState {
            fit: self
                .reader
                .current_fit_mode()
                .map(fit_name)
                .map(|s| s.to_string())
                .or_else(|| prev_reader.as_ref().map(|r| r.fit.clone()))
                .unwrap_or_else(|| "FitWidth".to_string()),
            layout: self
                .reader
                .current_page_layout()
                .map(layout_name)
                .map(|s| s.to_string())
                .or_else(|| prev_reader.as_ref().map(|r| r.layout.clone()))
                .unwrap_or_else(|| "Single".to_string()),
            rotation: self
                .reader
                .current_rotation()
                .or_else(|| prev_reader.as_ref().map(|r| r.rotation))
                .unwrap_or_default(),
            zoom: self
                .reader
                .current_zoom()
                .or_else(|| prev_reader.as_ref().map(|r| r.zoom))
                .unwrap_or(1.0),
            rtl: self
                .reader
                .current_rtl()
                .or_else(|| prev_reader.as_ref().map(|r| r.rtl))
                .unwrap_or(false),
        };
        cr_core::settings::workspace::WorkspaceState {
            width: w,
            height: h,
            maximized: self.window.is_maximized(),
            view: browser_view_state(
                self.nav_box.is_visible(),
                self.paned.position(),
                crate::workspace::BrowserReadouts {
                    mode: self.item_view.mode(),
                    sort_key,
                    descending,
                    grouper,
                    thumb_height: self.item_view.thumb_height(),
                    tile_height: self.item_view.tile_height(),
                    row_height: self.item_view.row_height(),
                    columns: self.item_view.detail_columns_state(),
                },
            ),
            reader,
            display: display_to_state(&crate::reader::page_view::session_display_options()),
        }
    }

    /// Restores the persisted workspace into the widgets (the
    /// startup path; the C# `MainForm.Load` applies
    /// `Settings.CurrentWorkspace`).
    fn apply_workspace(&self, ws: &cr_core::settings::workspace::WorkspaceState) {
        use crate::workspace::{
            display_from_state, fit_from_name, layout_from_name, mode_from_xml, sort_descending,
        };
        self.paned.set_position(ws.view.browser_split);
        self.nav_box.set_visible(ws.view.show_browser);
        if let Some(a) = self.action("sidebar") {
            a.set_state(&ws.view.show_browser.to_variant());
        }
        // Mode first (the item sizes clamp per mode), then the sizes.
        self.item_view
            .configure(|c| c.mode = mode_from_xml(ws.view.mode));
        self.item_view.configure(|c| {
            c.thumb_height = f64::from(ws.view.thumb_height);
            c.tile_size = (
                f64::from(ws.view.tile_height * 2),
                f64::from(ws.view.tile_height),
            );
            c.row_height = f64::from(ws.view.row_height);
        });
        if let Some(key) = &ws.view.sort_key {
            self.item_view.set_sort_column(key);
        }
        self.item_view
            .set_sort_direction(sort_descending(ws.view.sort_order));
        if let Some(g) = &ws.view.grouper {
            // The registry owns the 'static keys — a stored key only
            // applies when it still exists.
            if let Some((key, _)) = cr_engine::group::groupers()
                .iter()
                .find(|(k, _)| k == &g.as_str())
            {
                self.item_view.set_grouper(Some(key));
            }
        }
        let cols: Vec<(i32, bool, i32)> = ws
            .view
            .columns
            .iter()
            .map(|c| (c.id, c.visible, c.width))
            .collect();
        self.item_view.set_detail_columns_state(&cols);
        if ws.width > 0 && ws.height > 0 {
            self.window.set_default_size(ws.width, ws.height);
        }
        if ws.maximized {
            self.window.maximize();
        }
        // The display family rides the session copy: new views seed
        // from it (the T12 shape).
        crate::reader::page_view::set_session_display_options(display_from_state(&ws.display));
        self.reader
            .set_reader_seed(crate::reader_shell::ReaderSeed {
                fit: Some(fit_from_name(&ws.reader.fit)),
                layout: Some(layout_from_name(&ws.reader.layout)),
                rtl: Some(ws.reader.rtl),
                zoom: Some(ws.reader.zoom),
                rotation: Some(ws.reader.rotation),
            });
    }

    /// The shell command registry (`CommandMapper` parity): every
    /// command a `win.` action, accelerators on the application.
    /// Stub actions stay DISABLED until their feature lands — the
    /// task that lands each one is noted.
    fn install_commands(self: &Rc<ShellState>) {
        let group = gio::SimpleActionGroup::new();
        let state = Rc::downgrade(self);

        // --- Existing view commands (Phase 4) ---
        let mode_action = gio::SimpleAction::new_stateful(
            "view-mode",
            Some(glib::VariantTy::STRING),
            &"thumbnail".to_variant(),
        );
        {
            let state = state.clone();
            mode_action.connect_activate(move |_, value| {
                let Some(state) = state.upgrade() else {
                    return;
                };
                let name = value.and_then(|v| v.get::<String>()).unwrap_or_default();
                let mode = match name.as_str() {
                    "tile" => ItemViewMode::Tile,
                    "detail" => ItemViewMode::Detail,
                    _ => ItemViewMode::Thumbnail,
                };
                state.item_view.configure(|c| c.mode = mode);
                // The check state follows the VIEW (the T6 user
                // report: the Views check never moved — the state
                // was set here but the dropdown rows only re-render
                // on the sync, which this handler never ran).
                state.sync_enabled();
            });
        }
        group.add_action(&mode_action);
        self.actions.borrow_mut().insert("view-mode", mode_action);

        // thumb-size: grow / shrink (the C# Ctrl+wheel steps 16).
        for (name, delta) in [("thumb-bigger", THUMB_STEP), ("thumb-smaller", -THUMB_STEP)] {
            let action = gio::SimpleAction::new(name, None);
            let state = state.clone();
            action.connect_activate(move |_, _| {
                if let Some(sh) = state.upgrade() {
                    let current = sh.item_view.thumb_height();
                    let next = (current + delta).clamp(MIN_THUMB, MAX_THUMB);
                    sh.item_view.configure(|c| c.thumb_height = next);
                }
            });
            group.add_action(&action);
        }

        // sort-column (string parameter = the property name; "" =
        // Not Sorted — the Arrange menu's first row). STATEFUL: the
        // check state rides the current sort property.
        let sort_action = gio::SimpleAction::new_stateful(
            "sort-column",
            Some(glib::VariantTy::STRING),
            &"".to_variant(),
        );
        {
            let state = state.clone();
            sort_action.connect_activate(move |action, value| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let name = value.and_then(|v| v.get::<String>()).unwrap_or_default();
                if name.is_empty() {
                    sh.item_view.clear_sort();
                } else {
                    sh.item_view.set_sort_column(&name);
                }
                action.set_state(&name.to_variant());
                sh.sync_enabled();
            });
        }
        group.add_action(&sort_action);
        self.actions.borrow_mut().insert("sort-column", sort_action);

        // sort-direction toggle.
        let dir_action = gio::SimpleAction::new("sort-direction", None);
        {
            let state = state.clone();
            dir_action.connect_activate(move |_, _| {
                if let Some(sh) = state.upgrade() {
                    sh.item_view.toggle_sort_direction();
                }
            });
        }
        group.add_action(&dir_action);

        // group-by (string parameter; "" = none). STATEFUL: the
        // check mark follows the grouper key.
        let group_action = gio::SimpleAction::new_stateful(
            "group-by",
            Some(glib::VariantTy::STRING),
            &"".to_variant(),
        );
        {
            let state = state.clone();
            group_action.connect_activate(move |action, value| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let name = value.and_then(|v| v.get::<String>()).unwrap_or_default();
                let grouper = if name.is_empty() {
                    None
                } else {
                    cr_engine::group::groupers()
                        .iter()
                        .find(|(k, _)| *k == name)
                        .map(|(k, _)| *k)
                };
                sh.item_view.set_grouper(grouper);
                action.set_state(&name.to_variant());
                sh.sync_enabled();
            });
        }
        group.add_action(&group_action);
        self.actions.borrow_mut().insert("group-by", group_action);

        // The view filters (`ComicBookAllPropertiesMatcher.Create`):
        // the read-state radio, the comic-type toggles, duplicates.
        let view_filter = gio::SimpleAction::new_stateful(
            "view-filter",
            Some(glib::VariantTy::STRING),
            &"all".to_variant(),
        );
        {
            let state = state.clone();
            view_filter.connect_activate(move |action, value| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let name = value.and_then(|v| v.get::<String>()).unwrap_or_default();
                if !matches!(name.as_str(), "all" | "unread" | "reading" | "read") {
                    return;
                }
                action.set_state(&name.to_variant());
                sh.rebuild_filter();
                sh.sync_enabled();
            });
        }
        group.add_action(&view_filter);
        self.actions.borrow_mut().insert("view-filter", view_filter);

        let comic_type = gio::SimpleAction::new_stateful(
            "comic-type",
            Some(glib::VariantTy::STRING),
            &"all".to_variant(),
        );
        {
            let state = state.clone();
            comic_type.connect_activate(move |action, value| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let name = value.and_then(|v| v.get::<String>()).unwrap_or_default();
                if !matches!(name.as_str(), "books" | "fileless") {
                    return;
                }
                // The C# rows toggle: the active row returns to All.
                let current = action
                    .state()
                    .and_then(|v| v.get::<String>())
                    .unwrap_or_default();
                let next = if current == name {
                    "all".to_string()
                } else {
                    name
                };
                action.set_state(&next.to_variant());
                sh.rebuild_filter();
                sh.sync_enabled();
            });
        }
        group.add_action(&comic_type);
        self.actions.borrow_mut().insert("comic-type", comic_type);

        let duplicates =
            gio::SimpleAction::new_stateful("duplicates-only", None, &false.to_variant());
        {
            let state = state.clone();
            duplicates.connect_activate(move |action, _| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let next = !action
                    .state()
                    .and_then(|v| v.get::<bool>())
                    .unwrap_or(false);
                action.set_state(&next.to_variant());
                sh.rebuild_filter();
                sh.sync_enabled();
            });
        }
        group.add_action(&duplicates);
        self.actions
            .borrow_mut()
            .insert("duplicates-only", duplicates);

        // The Quick Search scope radio (the cue text follows).
        let scope = gio::SimpleAction::new_stateful(
            "search-scope",
            Some(glib::VariantTy::STRING),
            &"all".to_variant(),
        );
        {
            let state = state.clone();
            scope.connect_activate(move |action, value| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let name = value.and_then(|v| v.get::<String>()).unwrap_or_default();
                if !super::browser_toolbar::SEARCH_SCOPE_LABELS
                    .iter()
                    .any(|(v, _)| *v == name)
                {
                    return;
                }
                action.set_state(&name.to_variant());
                sh.search.set_placeholder_text(Some(
                    super::browser_toolbar::SEARCH_SCOPE_LABELS
                        .iter()
                        .find(|(v, _)| *v == name)
                        .map(|(_, l)| *l)
                        .unwrap_or("Search All"),
                ));
                sh.sync_enabled();
            });
        }
        group.add_action(&scope);
        self.actions.borrow_mut().insert("search-scope", scope);

        // The Detail column chooser rows (toggle one column).
        let toggle_column = gio::SimpleAction::new("toggle-column", Some(glib::VariantTy::STRING));
        {
            let state = state.clone();
            toggle_column.connect_activate(move |_, value| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let Some(text) = value.and_then(|v| v.get::<String>()) else {
                    return;
                };
                let Ok(id) = text.parse::<i32>() else {
                    return;
                };
                sh.item_view.toggle_column_visible(id);
                sh.sync_enabled();
            });
        }
        group.add_action(&toggle_column);

        // Duplicate List (the folder rows; the parameter = the
        // folder id).
        let duplicate = gio::SimpleAction::new("duplicate-list", Some(glib::VariantTy::STRING));
        {
            let state = state.clone();
            duplicate.connect_activate(move |_, value| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let Some(text) = value.and_then(|v| v.get::<String>()) else {
                    return;
                };
                let Ok(folder) = CrGuid::parse(&text) else {
                    return;
                };
                let Some(source) = *sh.current_list.borrow() else {
                    return;
                };
                let filter = sh.current_filter.borrow().clone();
                let Some(filter) = filter else {
                    return;
                };
                match library::duplicate_smart_list(&source, &folder, &filter) {
                    Ok(_) => sh.refresh_view(),
                    Err(err) => eprintln!("duplicate list failed: {err}"),
                }
            });
        }
        group.add_action(&duplicate);

        // list-layouts — the T14 workspace data lands the menus.
        self.add_disabled(&group, "list-layouts");

        // --- File ---
        self.add_simple(&group, "open-file", |sh| {
            let window = sh.window.clone();
            let state = Rc::downgrade(sh);
            open_file_dialog(&window, move |path| {
                if let Some(sh) = state.upgrade() {
                    sh.open_comic(Path::new(&path));
                }
            });
        });
        self.add_simple(&group, "close", |sh| sh.reader.close_current_tab());
        self.add_simple(&group, "close-all", |sh| sh.reader.close_all_tabs());
        // `OpenBooks.AddSlot` + `CurrentSlot = last`: the new EMPTY
        // slot selects and shows (the ported shape — no QuickOpen
        // overlay in the empty slot; recorded deviation).
        self.add_simple(&group, "new-tab", |sh| {
            sh.reader.add_empty_slot();
            sh.stack.set_visible_child_name("reader");
        });
        // The Open Books rows (`OpenBooks_Clicked`: CurrentSlot = i).
        {
            let open_tab = gio::SimpleAction::new("open-tab", Some(glib::VariantTy::STRING));
            let state = state.clone();
            open_tab.connect_activate(move |_, value| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let Some(text) = value.and_then(|v| v.get::<String>()) else {
                    return;
                };
                let Ok(slot) = text.parse::<usize>() else {
                    return;
                };
                // `OpenBooks_Clicked`: `CurrentSlot = i` — the reader
                // workspace shows.
                sh.activate_slot(slot);
                sh.sync_enabled();
            });
            group.add_action(&open_tab);
        }
        // The Recent Books rows (`OnOpenRecent`: open the path).
        {
            let recent = gio::SimpleAction::new("recent-book", Some(glib::VariantTy::STRING));
            let state = state.clone();
            recent.connect_activate(move |_, value| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let Some(path) = value.and_then(|v| v.get::<String>()) else {
                    return;
                };
                sh.open_comic(Path::new(&path));
                sh.sync_enabled();
            });
            group.add_action(&recent);
        }
        self.add_simple(&group, "add-folder", |sh| {
            let window = sh.window.clone();
            crate::app::add_folder_dialog(&window);
            let state = Rc::downgrade(sh);
            glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
                if let Some(sh) = state.upgrade() {
                    sh.navigator.refill(&library::comic_lists_snapshot());
                }
                glib::ControlFlow::Break
            });
        });
        self.add_simple(&group, "scan-folders", |_sh| {
            // `StartFullScan`: re-scan every watch-folder root (the
            // C# `QueueManager.StartScan(all,
            // RemoveMissingFilesOnFullScan)`; the remove-missing flag
            // is not ported — scans flag, never delete).
            let roots: Vec<String> = {
                let lib = library::session();
                let l = lib.borrow();
                l.database()
                    .watch_folders
                    .iter()
                    .map(|w| w.folder.clone())
                    .collect()
            };
            for root in roots {
                if root.is_empty() {
                    continue;
                }
                library::add_folder_to_library(Path::new(&root), |_| {});
            }
        });
        self.add_simple(&group, "update-book-files", |_| {
            library::update_all_book_files();
        });
        // The Tasks dialog (the C# `ShowPendingTasks`; the lamps and
        // the menu open the same single instance).
        self.add_simple(&group, "tasks", ShellState::show_tasks);
        // generate-thumbnails — the C# `CacheThumbnails` queue
        // command: one unlimited-queue warm-up job per library book
        // (the worker skips covers already in the thumbnail disk
        // cache).
        self.add_simple(&group, "generate-thumbnails", |_sh| {
            let pool = Arc::clone(&_sh.pool);
            library::cache_thumbnails(&pool);
        });
        // new-book-entry — the C# `AddNewBook()` (MainForm.cs:1879):
        // a fileless book (no file path) opens the editor; OK inserts
        // it into the database, Cancel discards.
        self.add_simple(&group, "new-book-entry", ShellState::open_new_book_editor);
        // new-book-series — the NewComics.py port ("New fileless Book
        // Series..."): a dialog creates a run of fileless books and
        // selects them (ADR-027 moved the script natively into the
        // app; the C# inserted the script item right after
        // `miNewComic`).
        self.add_simple(&group, "new-book-series", ShellState::new_book_series);
        self.add_simple(&group, "restart", |sh| {
            // `MenuRestart`: save, then re-launch the binary (the C#
            // `Program.Restart` + `Application.Restart`). The
            // workspace snapshot lands first (the restart keeps the
            // layout).
            {
                let prev = cr_ui_settings().borrow().current_workspace.clone();
                let ws = sh.collect_workspace(prev.as_ref());
                cr_ui_settings().borrow_mut().current_workspace = Some(ws);
            }
            if let Err(err) = library::save() {
                eprintln!("library save failed: {err}");
            }
            library::save_settings();
            if let Ok(exe) = std::env::current_exe() {
                let _ = std::process::Command::new(exe).spawn();
            }
            sh.app.quit();
        });
        self.add_simple(&group, "quit", |sh| sh.window.close());

        // --- Edit ---
        self.add_simple(&group, "info", ShellState::show_info);
        for (n, name) in [
            (0u32, "rating-0"),
            (1, "rating-1"),
            (2, "rating-2"),
            (3, "rating-3"),
            (4, "rating-4"),
            (5, "rating-5"),
        ] {
            // STATEFUL (the check state — `Math.Round(GetRating())
            // == N`; sync_enabled writes it).
            let action = gio::SimpleAction::new_stateful(name, None, &false.to_variant());
            let state = state.clone();
            action.connect_activate(move |_, _| {
                if let Some(sh) = state.upgrade() {
                    sh.set_rating(n as f32);
                    sh.sync_enabled();
                }
            });
            group.add_action(&action);
            self.actions.borrow_mut().insert(name, action);
        }
        // Quick Rating and Review — the FIRST selected book (the
        // C# `QuickRatingAndReview` passes `books.FirstOrDefault()`).
        self.add_simple(&group, "quick-rating", ShellState::show_quick_rating);
        // Set Bookmark — the name prompt over the CURRENT page
        // (`SetBookmark`: the proposal is the existing bookmark or
        // the page number; an empty entry clears the bookmark).
        self.add_simple(&group, "set-bookmark", |sh| {
            let Some((_, provider)) = sh.current_provider_page() else {
                return;
            };
            let Some(book) = sh.reader.current_comic_book() else {
                return;
            };
            let existing = book
                .info
                .pages
                .get(provider)
                .and_then(|p| p.bookmark.clone())
                .unwrap_or_default();
            let proposal = if existing.is_empty() {
                format!("Page {}", provider + 1)
            } else {
                existing
            };
            let state = Rc::downgrade(sh);
            crate::dialogs::name_prompt::show_name_prompt(
                &sh.window,
                "Bookmark",
                &proposal,
                move |name| {
                    let Some(sh) = state.upgrade() else {
                        return;
                    };
                    sh.edit_open_book(|b| update_bookmark_entry(b, provider, &name));
                    sh.sync_enabled();
                },
            );
        });
        // Remove Bookmark: clears the CURRENT page's bookmark
        // (`UpdateBookmark(page, "")`).
        self.add_simple(&group, "remove-bookmark", |sh| {
            let Some((_, provider)) = sh.current_provider_page() else {
                return;
            };
            sh.edit_open_book(|b| update_bookmark_entry(b, provider, ""));
            sh.sync_enabled();
        });
        // The Bookmarks list rows (`win.open-bookmark::<provider>`).
        {
            let jump = gio::SimpleAction::new("open-bookmark", Some(glib::VariantTy::STRING));
            let state = state.clone();
            jump.connect_activate(move |_, value| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let Some(text) = value.and_then(|v| v.get::<String>()) else {
                    return;
                };
                let Ok(provider) = text.parse::<usize>() else {
                    return;
                };
                let Some(target) = sh.reader.display_of_provider(provider) else {
                    return;
                };
                sh.reader.navigate_current(target);
                sh.sync_enabled();
            });
            group.add_action(&jump);
        }
        self.add_simple(&group, "prev-bookmark", |sh| {
            sh.reader.dispatch_current("MoveToPrevBookmark")
        });
        self.add_simple(&group, "next-bookmark", |sh| {
            sh.reader.dispatch_current("MoveToNextBookmark")
        });
        self.add_simple(&group, "last-page-read", |sh| {
            // `ComicDisplay.DisplayLastPageRead`.
            if let Some(book) = sh.reader.current_comic_book() {
                let page = book.last_page_read.max(0) as usize;
                sh.reader.navigate_current(page);
            }
        });
        // copy-page — `ComicDisplay.CopyPageToClipboard`
        // (ComicDisplay.cs:1441): the current composed page image onto
        // the clipboard (errors are swallowed in the C# too).
        self.add_simple(&group, "copy-page", |sh| {
            crate::trace::trace("copy-page: handler entered");
            let view = sh.reader.current_view();
            let surface = view.and_then(|v| v.create_page_image());
            crate::trace::trace(format!("copy-page: view+surface ok={}", surface.is_some()));
            let Some(surface) = surface else {
                return;
            };
            copy_surface_to_clipboard(&surface);
        });
        // export-page — `ExportCurrentImage` (MainForm.cs:2326 +
        // `ExportImage` 2185): the "Save Page as" dialog, the name
        // "{Caption} - Page {N}", the 5-format filter, the filter
        // index persisted (`LastExportPageFilterIndex`).
        self.add_simple(&group, "export-page", |sh| {
            crate::trace::trace("export-page: handler entered");
            let view = sh.reader.current_view();
            let caption = sh
                .reader
                .current_comic_book()
                .map(|b| cr_engine::display_text::caption(&b))
                .unwrap_or_default();
            let page = sh.reader.current_display_page().map_or(1, |p| p + 1);
            let surface = view.and_then(|v| v.create_page_image());
            crate::trace::trace(format!(
                "export-page: caption={caption:?} page={page} surface={}",
                surface.is_some()
            ));
            export_page_dialog(&sh.window, &caption, page, surface);
        });
        self.add_simple(&group, "refresh", |sh| sh.refresh_view());
        self.add_simple(&group, "preferences", ShellState::show_preferences);

        // --- Browse ---
        // `ToggleBrowser` with the `() => BrowserVisible` check.
        self.add_check(&group, "toggle-browser", true, |sh| sh.toggle_browser());
        self.add_simple(&group, "view-library", |sh| {
            sh.select_workspace(Workspace::Library);
        });
        self.add_simple(&group, "view-pages", |sh| {
            sh.select_workspace(Workspace::Pages);
        });
        {
            let sidebar = gio::SimpleAction::new_stateful("sidebar", None, &true.to_variant());
            let state = state.clone();
            sidebar.connect_activate(move |action, _| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let visible = !sh.nav_box.is_visible();
                sh.nav_box.set_visible(visible);
                action.set_state(&visible.to_variant());
            });
            group.add_action(&sidebar);
            self.actions.borrow_mut().insert("sidebar", sidebar);
        }
        // small-preview — T11 lands the pane.
        self.add_disabled(&group, "small-preview");
        // Dark mode — NO C# command (recorded deviation): the C#
        // theme (`ExtendedSettings.Theme` + the `-dark` switch) is
        // boot-time only. The global is the source of truth; the
        // sync derives the check from it, the ini keys persist it.
        {
            let initial = cr_core::settings::ExtendedSettings::global().effective_theme()
                == cr_core::settings::enums::Themes::Dark;
            let dark = gio::SimpleAction::new_stateful("dark-mode", None, &initial.to_variant());
            {
                let state = state.clone();
                dark.connect_activate(move |action, _| {
                    let Some(sh) = state.upgrade() else {
                        return;
                    };
                    let next = {
                        let extended = cr_core::settings::ExtendedSettings::global();
                        extended.effective_theme() != cr_core::settings::enums::Themes::Dark
                    };
                    {
                        let mut extended = cr_core::settings::ExtendedSettings::global_mut();
                        extended.theme = if next {
                            cr_core::settings::enums::Themes::Dark
                        } else {
                            cr_core::settings::enums::Themes::Default
                        };
                        // The toggle is the explicit intent: the
                        // `-dark` force must not re-darken the next
                        // boot over the stored Theme.
                        extended.use_dark_mode = false;
                    }
                    crate::theme::set_dark(next);
                    action.set_state(&next.to_variant());
                    library::save_ini_keys(&[
                        ("UseDarkMode", "False"),
                        ("Theme", if next { "Dark" } else { "Default" }),
                    ]);
                    sh.sync_enabled();
                });
            }
            group.add_action(&dark);
            self.actions.borrow_mut().insert("dark-mode", dark);
        }
        self.add_simple(&group, "prev-list", |sh| sh.browse_history(-1));
        self.add_simple(&group, "next-list", |sh| sh.browse_history(1));

        // --- Read ---
        self.add_simple(&group, "first-page", |sh| {
            sh.reader.dispatch_current("MoveToFirstPage")
        });
        self.add_simple(&group, "prev-page", |sh| {
            sh.reader.dispatch_current("MoveToPreviousPage")
        });
        self.add_simple(&group, "next-page", |sh| {
            sh.reader.dispatch_current("MoveToNextPage")
        });
        self.add_simple(&group, "last-page", |sh| {
            sh.reader.dispatch_current("MoveToLastPage")
        });
        self.add_simple(&group, "prev-book", |sh| sh.open_next_book(-1));
        self.add_simple(&group, "next-book", |sh| sh.open_next_book(1));
        self.add_simple(&group, "random-book", |sh| sh.open_next_book(0));
        self.add_simple(&group, "show-in-browser", |sh| {
            // `SyncBrowser`: reveal the browser, select the open book.
            if let Some(book) = sh.reader.current_comic_book() {
                sh.show_browser();
                sh.item_view.select_book(&book.id);
            }
        });
        self.add_simple(&group, "prev-tab", |sh| {
            // `OpenBooks.PreviousSlot`: the switch reveals the reader
            // (`ShowView(i)` shows the comic viewer).
            if sh.reader.cycle_slot(-1) {
                sh.stack.set_visible_child_name("reader");
            }
        });
        self.add_simple(&group, "next-tab", |sh| {
            if sh.reader.cycle_slot(1) {
                sh.stack.set_visible_child_name("reader");
            }
        });
        // `() => Program.Settings.AutoScrolling` — the view field
        // mirrors the C# setting (session-only here; the C# writes
        // Config.xml).
        self.add_check(&group, "auto-scroll", false, |sh| {
            sh.reader.dispatch_current("ToggleAutoScrolling");
        });
        // `() => ComicDisplay.TwoPageNavigation`.
        self.add_check(&group, "double-auto-scroll", true, |sh| {
            sh.reader.dispatch_current("DoublePageAutoScroll");
        });
        {
            // `tsCurrentPage_Click` (`TrackCurrentPage = !…`): the
            // check derives from the SETTING in the sync (the T6
            // source-of-truth rule).
            let track = cr_ui_settings().borrow().track_current_page;
            self.add_check(&group, "track-current-page", track, |_sh| {
                let next = !cr_ui_settings().borrow().track_current_page;
                cr_ui_settings().borrow_mut().track_current_page = next;
            });
            // `tbShowMainMenu` (the Tools menu): CHECKED while the
            // menu is NOT auto-hidden — the C# flips
            // `AutoHideMainMenu` (MainForm.cs:1455-1458).
            let show = gio::SimpleAction::new_stateful(
                "show-main-menu",
                None,
                &(!cr_ui_settings().borrow().auto_hide_main_menu).to_variant(),
            );
            {
                let state = state.clone();
                show.connect_activate(move |action, _| {
                    let Some(sh) = state.upgrade() else {
                        return;
                    };
                    let next = !cr_ui_settings().borrow().auto_hide_main_menu;
                    cr_ui_settings().borrow_mut().auto_hide_main_menu = next;
                    action.set_state(&(!next).to_variant());
                    sh.update_menubar();
                });
            }
            group.add_action(&show);
            self.actions.borrow_mut().insert("show-main-menu", show);
        }

        // --- Display ---
        // display-settings — the `EditWorkspaceDisplaySettings` port
        // (F9): snapshot the current reader view (or the session
        // copy), the dialog edits the copy, OK/Apply push it onto
        // every open view (`SetWorkspaceDisplayOptions`).
        {
            let state = state.clone();
            let action = gio::SimpleAction::new("display-settings", None);
            action.connect_activate(move |_, _| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let window = sh.window.clone();
                let options = sh
                    .reader
                    .current_view()
                    .map(|view| view.display_options())
                    .unwrap_or_else(crate::reader::page_view::session_display_options);
                let reader = sh.reader.clone();
                crate::dialogs::display_settings::show_display_settings(
                    &window,
                    options,
                    move |options| {
                        // The session copy records FIRST (the
                        // workspace write-back shape) so a book
                        // opened later seeds the same options; every
                        // open view re-applies
                        // (`SetWorkspaceDisplayOptions`).
                        crate::reader::page_view::set_session_display_options(options.clone());
                        reader.apply_display_options_all(options);
                    },
                );
            });
            group.add_action(&action);
            self.actions.borrow_mut().insert("display-settings", action);
        }
        let fit_action = gio::SimpleAction::new_stateful(
            "page-fit",
            Some(glib::VariantTy::STRING),
            &"fit-all".to_variant(),
        );
        {
            let state = state.clone();
            fit_action.connect_activate(move |_, value| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let name = value.and_then(|v| v.get::<String>()).unwrap_or_default();
                let id = match name.as_str() {
                    "original" => "Original",
                    "fit-all" => "FitAll",
                    "fit-width" => "FitWidth",
                    "fit-width-adaptive" => "FitWidthAdaptive",
                    "fit-height" => "FitHeight",
                    "fit-best" => "FitBest",
                    _ => return,
                };
                sh.reader.dispatch_current(id);
                sh.sync_enabled();
            });
        }
        group.add_action(&fit_action);
        self.actions.borrow_mut().insert("page-fit", fit_action);

        let layout_action = gio::SimpleAction::new_stateful(
            "page-layout",
            Some(glib::VariantTy::STRING),
            &"single".to_variant(),
        );
        {
            let state = state.clone();
            layout_action.connect_activate(move |_, value| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let name = value.and_then(|v| v.get::<String>()).unwrap_or_default();
                let id = match name.as_str() {
                    "single" => "SinglePage",
                    "double" => "TwoPages",
                    "double-adaptive" => "TwoPagesAdaptive",
                    "continuous" => "Continuous",
                    _ => return,
                };
                sh.reader.dispatch_current(id);
                sh.sync_enabled();
            });
        }
        group.add_action(&layout_action);
        self.actions
            .borrow_mut()
            .insert("page-layout", layout_action);

        let rtl_action =
            gio::SimpleAction::new_stateful("right-to-left", None, &false.to_variant());
        {
            let state = state.clone();
            rtl_action.connect_activate(move |_, _| {
                if let Some(sh) = state.upgrade() {
                    sh.reader.dispatch_current("RightToLeft");
                    sh.sync_enabled();
                }
            });
        }
        group.add_action(&rtl_action);
        self.actions
            .borrow_mut()
            .insert("right-to-left", rtl_action);

        let oversized =
            gio::SimpleAction::new_stateful("only-fit-oversized", None, &false.to_variant());
        {
            let state = state.clone();
            oversized.connect_activate(move |_, _| {
                if let Some(sh) = state.upgrade() {
                    sh.reader.dispatch_current("OnlyFitIfOversized");
                    sh.sync_enabled();
                }
            });
        }
        group.add_action(&oversized);

        self.add_simple(&group, "zoom-in", |sh| sh.reader.dispatch_current("ZoomIn"));
        self.add_simple(&group, "zoom-out", |sh| {
            sh.reader.dispatch_current("ZoomOut")
        });
        // `MainForm.ToggleZoom` (the menu item; the touch binding is
        // a reader-internal no-op).
        self.add_simple(&group, "toggle-zoom", |sh| sh.reader.toggle_zoom_current());
        // The Zoom presets (`ComicDisplay.ImageZoom = v`).
        {
            let zoom = gio::SimpleAction::new("zoom-preset", Some(glib::VariantTy::STRING));
            let state = Rc::downgrade(self);
            zoom.connect_activate(move |_, value| {
                if let Some(sh) = state.upgrade() {
                    let Some(text) = value.and_then(|v| v.get::<String>()) else {
                        return;
                    };
                    let percent: f32 = text.parse().unwrap_or(100.0);
                    sh.reader.zoom_current(percent / 100.0);
                    sh.sync_enabled();
                }
            });
            group.add_action(&zoom);
            self.actions.borrow_mut().insert("zoom-preset", zoom);
        }
        // Custom Zoom (the C# always enables the item — the dialog
        // opens without a book too; OK stores the zoom when a view
        // exists).
        self.add_simple(&group, "zoom-custom", |sh| {
            let zoom = sh.reader.current_zoom().unwrap_or(1.0);
            let state = Rc::downgrade(sh);
            crate::dialogs::zoom::show_zoom_dialog(&sh.window, zoom, move |result| {
                if let (Some(z), Some(sh)) = (result, state.upgrade()) {
                    sh.reader.zoom_current(z);
                    sh.sync_enabled();
                }
            });
        });
        self.add_simple(&group, "rotate-left", |sh| {
            sh.reader.dispatch_current("RotateCC")
        });
        self.add_simple(&group, "rotate-right", |sh| {
            sh.reader.dispatch_current("RotateC")
        });
        self.add_simple(&group, "rotate-0", |sh| {
            sh.reader.dispatch_current("Rotate0")
        });
        self.add_simple(&group, "rotate-90", |sh| {
            sh.reader.dispatch_current("Rotate90")
        });
        self.add_simple(&group, "rotate-180", |sh| {
            sh.reader.dispatch_current("Rotate180")
        });
        self.add_simple(&group, "rotate-270", |sh| {
            sh.reader.dispatch_current("Rotate270")
        });
        // The Page Rotation EDITOR items (`GetPageEditor().Rotation`
        // — the CURRENT PAGE's stored rotation, radio).
        {
            let pr = gio::SimpleAction::new("page-rotation", Some(glib::VariantTy::STRING));
            let state = Rc::downgrade(self);
            pr.connect_activate(move |_, value| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let Some(name) = value.and_then(|v| v.get::<String>()) else {
                    return;
                };
                let rot = match name.as_str() {
                    "none" => cr_core::model::enums::ImageRotation::None,
                    "90" => cr_core::model::enums::ImageRotation::Rotate90,
                    "180" => cr_core::model::enums::ImageRotation::Rotate180,
                    "270" => cr_core::model::enums::ImageRotation::Rotate270,
                    _ => return,
                };
                let Some(view) = sh.reader.current_view() else {
                    return;
                };
                let display = view.current_page();
                view.set_page_rotation_for(display, rot);
                if let Some(provider) = view.provider_index_of(display) {
                    sh.edit_open_book(|b| b.info.update_page_rotation(provider, rot));
                }
                sh.sync_enabled();
            });
            group.add_action(&pr);
        }
        // The Page Type EDITOR items (`GetPageEditor().PageType` —
        // the CURRENT PAGE's type, radio; the parameter is the type
        // VALUE).
        {
            let pt = gio::SimpleAction::new("page-type", Some(glib::VariantTy::STRING));
            let state = Rc::downgrade(self);
            pt.connect_activate(move |_, value| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let Some(name) = value.and_then(|v| v.get::<String>()) else {
                    return;
                };
                let Some((_, pt_value)) = crate::dialogs::book_editor::PAGE_TYPE_ITEMS
                    .iter()
                    .find(|(_, v)| name == v.0.to_string())
                else {
                    return;
                };
                let Some((_, provider)) = sh.current_provider_page() else {
                    return;
                };
                sh.edit_open_book(|b| b.info.update_page_type(provider, *pt_value));
                sh.sync_enabled();
            });
            group.add_action(&pt);
        }
        {
            let auto = gio::SimpleAction::new_stateful("auto-rotate", None, &false.to_variant());
            let state = state.clone();
            auto.connect_activate(move |action, _| {
                if let Some(sh) = state.upgrade() {
                    sh.reader.dispatch_current("AutoRotate");
                    let current = action
                        .state()
                        .and_then(|v| v.get::<bool>())
                        .unwrap_or(false);
                    action.set_state(&(!current).to_variant());
                }
            });
            group.add_action(&auto);
        }
        // MinimalGui / FullScreen checks (the state lives in the
        // reader shell / the root window; sync reads it back).
        self.add_check(&group, "minimal-gui", false, |sh| {
            sh.reader.dispatch_current("ToggleMenu");
        });
        self.add_check(&group, "full-screen", false, |sh| {
            sh.reader.dispatch_current("ToggleFullScreen");
        });
        self.add_simple(&group, "undock-reader", |sh| {
            sh.reader.dispatch_current("ToggleUndockReader")
        });
        self.add_simple(&group, "magnifier", |sh| {
            sh.reader.dispatch_current("ToggleMagnify")
        });

        // --- Help ---
        self.add_simple(&group, "about", ShellState::show_about);

        // --- The mainKeys shell commands ---
        self.add_simple(&group, "focus-search", |sh| {
            sh.search.grab_focus();
        });
        // `tsQuickSearch` (the navigator's own search toggle; the
        // check state = the box visibility — `commands.Add(
        // ToggleQuickSearch, true, () => quickSearchPanel.Visible)`).
        {
            let state = state.clone();
            let action = gio::SimpleAction::new_stateful(
                "toggle-navigator-search",
                None,
                &false.to_variant(),
            );
            action.connect_activate(move |_, _| {
                if let Some(sh) = state.upgrade() {
                    sh.navigator.toggle_search();
                    sh.sync_enabled();
                }
            });
            group.add_action(&action);
            self.actions
                .borrow_mut()
                .insert("toggle-navigator-search", action);
        }

        // --- The Pages panel (the T7 `ComicPagesView.toolStrip`) ---
        // The page-grid mode radios (the Views drop rows).
        let pages_mode = gio::SimpleAction::new_stateful(
            "pages-view-mode",
            Some(glib::VariantTy::STRING),
            &"thumbnail".to_variant(),
        );
        {
            let state = state.clone();
            pages_mode.connect_activate(move |_, value| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let name = value.and_then(|v| v.get::<String>()).unwrap_or_default();
                sh.pages
                    .set_mode(super::pages_view::PagesMode::from_action_name(&name));
                sh.sync_enabled();
            });
        }
        group.add_action(&pages_mode);
        self.actions
            .borrow_mut()
            .insert("pages-view-mode", pages_mode);

        self.window.insert_action_group("win", Some(&group));
        ShellState::register_accels(&self.app);
        ShellState::install_shifted_key_fallback(self);
        self.sync_enabled();
    }

    /// The shifted-symbol key fallback (see
    /// `commands::shifted_symbol_command`): a window key controller
    /// resolves the hardware keycode to its UNSHIFTED keyval and
    /// fires the command the accel table cannot match when Shift
    /// rewrote the symbol (Alt+Shift+4 → '¤', Ctrl+Shift+7 → '/').
    fn install_shifted_key_fallback(self: &Rc<ShellState>) {
        let controller = gtk4::EventControllerKey::new();
        let state = Rc::downgrade(self);
        controller.connect_key_pressed(move |_c, key, keycode, state_bits| {
            let Some(sh) = state.upgrade() else {
                return glib::Propagation::Proceed;
            };
            let mask = gtk4::accelerator_get_default_mod_mask();
            let mods = state_bits & mask;
            let ctrl = mods.contains(gtk4::gdk::ModifierType::CONTROL_MASK);
            let shift = mods.contains(gtk4::gdk::ModifierType::SHIFT_MASK);
            let alt = mods.contains(gtk4::gdk::ModifierType::ALT_MASK);
            // The unshifted keyval: the level-0 mapping of the
            // hardware keycode (group 0 preferred).
            let unshifted = gtk4::prelude::WidgetExt::display(&sh.window)
                .map_keycode(keycode)
                .and_then(|entries| {
                    let pick = |group: Option<i32>| {
                        entries
                            .iter()
                            .find(|(k, _)| k.level() == 0 && group.is_none_or(|g| k.group() == g))
                            .map(|(_, v)| *v)
                    };
                    pick(Some(0)).or_else(|| pick(None))
                });
            let Some(unshifted) = unshifted else {
                return glib::Propagation::Proceed;
            };
            // Only when Shift rewrote the symbol — otherwise the real
            // accelerator handles the combo (never double-fire).
            if unshifted == key {
                return glib::Propagation::Proceed;
            }
            let Some(action) = crate::commands::shifted_symbol_command(ctrl, shift, alt, unshifted)
            else {
                return glib::Propagation::Proceed;
            };
            let _ = gtk4::prelude::WidgetExt::activate_action(
                &sh.window,
                &format!("win.{action}"),
                None,
            );
            sh.sync_enabled();
            glib::Propagation::Stop
        });
        self.window.add_controller(controller);
    }

    /// The menubar wiring (Phase 5.5 T3): the chrome-visibility hook
    /// (fullscreen enter/leave, MinimalGui). The `AutoHideMainMenu`
    /// Alt-alone reveal was REMOVED (the T9 user decision: the
    /// menubar shows always — no reveal shortcut).
    fn install_menubar_keys(self: &Rc<ShellState>) {
        // Chrome changes (fullscreen notify, MinimalGui toggles) →
        // the menubar rule re-evaluates with the sync.
        {
            let state = Rc::downgrade(self);
            self.reader.set_on_chrome_change(move |_visible| {
                if let Some(sh) = state.upgrade() {
                    sh.sync_enabled();
                }
            });
        }
    }

    /// Registers one STATEFUL check action (`CommandMapper.Add(...
    /// checkLambda)` parity): the handler runs, the check state
    /// follows in the next sync (GTK renders stateful-action state
    /// on menu items).
    fn add_check<F: Fn(&Rc<ShellState>) + 'static>(
        self: &Rc<ShellState>,
        group: &gio::SimpleActionGroup,
        name: &'static str,
        initial: bool,
        f: F,
    ) {
        let action = gio::SimpleAction::new_stateful(name, None, &initial.to_variant());
        let state = Rc::downgrade(self);
        action.connect_activate(move |_, _| {
            if let Some(sh) = state.upgrade() {
                f(&sh);
                sh.sync_enabled();
            }
        });
        group.add_action(&action);
        self.actions.borrow_mut().insert(name, action);
    }

    /// A stub action that stays disabled until its feature task
    /// lands (the name is the note).
    fn add_disabled(&self, group: &gio::SimpleActionGroup, name: &'static str) {
        let action = gio::SimpleAction::new(name, None);
        action.set_enabled(false);
        group.add_action(&action);
        self.actions.borrow_mut().insert(name, action);
    }

    /// The accelerator registration (`gtk_application_set_accels_for_
    /// action`); the radio values ride detailed action names.
    fn register_accels(app: &Application) {
        for command in crate::commands::COMMANDS {
            if !command.accels.is_empty() {
                app.set_accels_for_action(&format!("win.{}", command.action), command.accels);
            }
        }
        for (value, accel) in crate::commands::FIT_MODES {
            if !accel.is_empty() {
                app.set_accels_for_action(&format!("win.page-fit::{value}"), &[*accel]);
            }
        }
        for (value, accel) in crate::commands::LAYOUT_MODES {
            if !accel.is_empty() {
                app.set_accels_for_action(&format!("win.page-layout::{value}"), &[*accel]);
            }
        }
    }
}

/// `ComicInfo.UpdateBookmark(page, bookmark)`: an empty name clears
/// (the C# writes an empty bookmark; the model stores `None`),
/// a change only when the entry really changes.
fn update_bookmark_entry(book: &mut ComicBook, provider: usize, name: &str) {
    let Some(p) = book.info.pages.get_mut(provider) else {
        return;
    };
    let old = p.bookmark.clone().unwrap_or_default();
    let changed = (!old.is_empty() || !name.is_empty()) && old != name;
    if changed {
        p.bookmark = if name.is_empty() {
            None
        } else {
            Some(name.into())
        };
    }
}

/// `ImageFitMode` → the `win.page-fit` state name.
fn fit_action_name(mode: ImageFitMode) -> &'static str {
    match mode {
        ImageFitMode::Original => "original",
        ImageFitMode::Fit => "fit-all",
        ImageFitMode::FitWidth => "fit-width",
        ImageFitMode::FitWidthAdaptive => "fit-width-adaptive",
        ImageFitMode::FitHeight => "fit-height",
        ImageFitMode::BestFit => "fit-best",
    }
}

/// `PageLayoutMode` → the `win.page-layout` state name.
fn layout_action_name(mode: PageLayoutMode) -> &'static str {
    match mode {
        PageLayoutMode::Single => "single",
        PageLayoutMode::Double => "double",
        PageLayoutMode::DoubleAdaptive => "double-adaptive",
        PageLayoutMode::Continuous => "continuous",
    }
}

/// The selection ids plus the right-clicked row when it is outside
/// the selection (the context-menu targeting rule).
fn selection_ids_with_target(sh: &ShellState, target: Option<CrGuid>) -> Vec<CrGuid> {
    let mut ids = sh.item_view.selection_ids();
    if let Some(id) = target {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}

fn show_context_menu(state: &std::rc::Weak<ShellState>, target: Option<CrGuid>, x: f64, y: f64) {
    let popover = gtk4::Popover::new();
    let box_ = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    box_.set_margin_top(4);
    box_.set_margin_bottom(4);
    box_.set_margin_start(4);
    box_.set_margin_end(4);

    let window = state
        .upgrade()
        .map(|sh| sh.window.clone())
        .expect("shell alive while the menu opens");
    let add_item = |box_: &gtk4::Box, label: &str, action: &'static str| {
        let popover = popover.clone();
        let state = state.clone();
        let window = window.clone();
        let button = Button::with_label(label);
        button.set_has_frame(false);
        button.set_halign(gtk4::Align::Fill);
        button.connect_clicked(move |_| {
            popover.popdown();
            let Some(sh) = state.upgrade() else {
                return;
            };
            match action {
                "open" => {
                    if let Some(id) = target {
                        if let Some(path) = library::book_path(&id) {
                            sh.open_comic(Path::new(&path));
                        }
                    }
                }
                "reveal" => {
                    if let Some(id) = target {
                        if let Some(path) = library::book_path(&id) {
                            let _ = std::process::Command::new("xdg-open")
                                .arg(Path::new(&path).parent().unwrap_or(Path::new("/")))
                                .spawn();
                        }
                    }
                }
                "edit" => {
                    // The bulk editor over the selection (the C#
                    // `MultipleComicBooksDialog`).
                    let ids = selection_ids_with_target(&sh, target);
                    if ids.is_empty() {
                        return;
                    }
                    let books = ShellState::books_by_ids(&ids);
                    if books.is_empty() {
                        return;
                    }
                    sh.open_bulk_editor(books);
                }
                "update-file" => {
                    // The manual write (the C# `AddBookToFileUpdate(cb,
                    // alwaysWrite: true)`): the UpdateComicFiles gate
                    // still applies.
                    let selection = sh.item_view.view_state().selection_snapshot();
                    let mut ids: Vec<CrGuid> = selection.into_iter().collect();
                    if let Some(id) = target {
                        if !ids.contains(&id) {
                            ids.push(id);
                        }
                    }
                    let mut errors: Vec<String> = Vec::new();
                    let mut written = 0usize;
                    for id in &ids {
                        match library::update_book_file(id, true) {
                            Ok(true) => written += 1,
                            Ok(false) => {}
                            Err(e) => errors.push(e),
                        }
                    }
                    if let Some(last) = errors.last() {
                        show_error_dialog(
                            &sh.app,
                            "Update Book Files",
                            &format!("{}/{} written. Last error: {}", written, ids.len(), last),
                        );
                    }
                    sh.refresh_view_from_list();
                }
                "export" => {
                    // The export dialog over the selection (the C#
                    // `ConvertComic`).
                    let selection = sh.item_view.view_state().selection_snapshot();
                    let mut ids: Vec<CrGuid> = selection.into_iter().collect();
                    if let Some(id) = target {
                        if !ids.contains(&id) {
                            ids.push(id);
                        }
                    }
                    if ids.is_empty() {
                        return;
                    }
                    let books: Vec<cr_core::model::comic_book::ComicBook> = {
                        let lib = library::session();
                        let l = lib.borrow();
                        l.database()
                            .books
                            .iter()
                            .filter(|b| ids.contains(&b.id))
                            .cloned()
                            .collect()
                    };
                    if books.is_empty() {
                        return;
                    }
                    let captions: Vec<String> =
                        books.iter().map(cr_engine::display_text::caption).collect();
                    // `Program.Settings.CurrentExportSetting` —
                    // session-only here (the persistence joins when
                    // the settings schema carries the export block).
                    let session_default = library::last_export_setting().unwrap_or_default();
                    let refresh_state = state.clone();
                    crate::dialogs::export::show_export_dialog(
                        &window,
                        books,
                        captions,
                        session_default,
                        move |result| {
                            if let Some(r) = result {
                                library::remember_export_setting(r.setting);
                                if let Some(sh) = refresh_state.upgrade() {
                                    sh.refresh_view_from_list();
                                }
                            }
                        },
                    );
                }
                "remove" => {
                    // The C# remove flow asks: remove from the list
                    // only, or from the Library, and whether to move
                    // the files to the trash.
                    let selection = sh.item_view.view_state().selection_snapshot();
                    let mut ids: Vec<CrGuid> = selection.into_iter().collect();
                    if let Some(id) = target {
                        if !ids.contains(&id) {
                            ids.push(id);
                        }
                    }
                    if ids.is_empty() {
                        return;
                    }
                    let count = ids.len();
                    let confirm = gtk4::MessageDialog::builder()
                        .transient_for(&window)
                        .modal(true)
                        .title("Remove Books")
                        .text(format!("Remove {count} book(s) from the current list?"))
                        .message_type(gtk4::MessageType::Question)
                        .buttons(gtk4::ButtonsType::OkCancel)
                        .build();
                    let also_files =
                        gtk4::CheckButton::with_label("Also delete the files (moved to the trash)");
                    // The MessageDialog message_area is a Box; reach
                    // it through the child hierarchy.
                    let area = confirm
                        .child()
                        .and_downcast::<gtk4::Box>()
                        .and_then(|vbox| vbox.first_child().and_downcast::<gtk4::Box>());
                    if let Some(area) = area {
                        area.append(&also_files);
                    }
                    let refresh_state = state.clone();
                    let ids_for_ok = ids.clone();
                    confirm.connect_response(move |dlg, resp| {
                        let remove_files = also_files.is_active();
                        dlg.destroy();
                        if resp != gtk4::ResponseType::Ok {
                            return;
                        }
                        let Some(sh) = refresh_state.upgrade() else {
                            return;
                        };
                        for id in &ids_for_ok {
                            if remove_files {
                                if let Some(path) = library::book_path(id) {
                                    // ADR-006: the recycle bin → GIO
                                    // trash (the `gio` CLI; a libgio
                                    // binding is Phase 7 polish).
                                    let _ = std::process::Command::new("gio")
                                        .args(["trash", &path])
                                        .status();
                                }
                            }
                            library::remove_book(id);
                        }
                        sh.refresh_view_from_list();
                    });
                    confirm.present();
                }
                "properties" => {
                    // The selection (plus the right-clicked row when
                    // it is outside it) — the C# opens the dialog
                    // over the selected books (prev/next when > 1).
                    let ids = selection_ids_with_target(&sh, target);
                    if ids.is_empty() {
                        return;
                    }
                    // List order for the prev/next walk.
                    let books = ShellState::books_by_ids(&ids);
                    if books.is_empty() {
                        return;
                    }
                    sh.open_editor(books);
                }
                _ => {}
            }
        });
        box_.append(&button);
    };
    add_item(&box_, "Open", "open");
    add_item(&box_, "Reveal in File Manager", "reveal");
    add_item(&box_, "Edit…", "edit");
    add_item(&box_, "Update Book File(s)", "update-file");
    add_item(&box_, "Export…", "export");
    add_item(&box_, "Remove from Library", "remove");
    add_item(&box_, "Properties…", "properties");
    popover.set_child(Some(&box_));
    popover.set_parent(&window);
    popover.connect_closed(|p| p.unparent());
    let rect = gtk4::gdk::Rectangle::new(x as i32, y as i32 + 8, 1, 1);
    popover.set_pointing_to(Some(&rect));
    crate::trace::trace(format!(
        "context: popup at ({x}, {y}) — scroll before popup {}",
        state
            .upgrade()
            .map(|sh| sh.item_view.probe_scroll_value())
            .unwrap_or(-1.0)
    ));
    popover.popup();
}

/// `ComicBookAllPropertiesMatcher.Create` for the quick search: a
/// contains over the All field set; MATCH/NOT text parses as a full
/// query (`UpdateQuickFilter`).
/// One registered value matcher as raw XML data (the C#
/// `new ComicBookXMatcher { ... }` object initializers).
fn raw_value_matcher(
    type_name: &str,
    op: i32,
    value: &str,
    value2: &str,
    not: bool,
    option: Option<&str>,
) -> cr_core::database::list_items::ComicBookMatcher {
    cr_core::database::list_items::ComicBookMatcher::Value(
        cr_core::database::list_items::ValueMatcher {
            type_name: type_name.into(),
            not,
            match_operator: op,
            match_value: value.into(),
            match_value_2: value2.into(),
            option: option.map(Into::into),
            ..Default::default()
        },
    )
}

/// `ComicBookAllPropertiesMatcher.Create(text, 3, option, show,
/// comic)` + the `ShowOnlyDuplicates` extra of `GetCurrentMatcher` —
/// the composed browser filter (`UpdateQuickFilter` parity):
///
/// - a MATCH/NOT text parses as a full query, but ONLY for the All
///   scope (the C# `UpdateQuickFilter` gate); the view filters do
///   NOT apply to a parsed query (they live in the Create path).
/// - otherwise: [read-state filter][comic-type filter][text
///   matcher] as an And group (each part only when active).
/// - duplicates-only adds a `ComicBookDuplicateMatcher` on top
///   (always applied — it is a GetCurrentMatcher member, not a
///   quickFilter member).
/// - all inactive + no text → no filter (the C# `Create` returns
///   null).
fn compose_quick_filter(
    text: &str,
    scope: &str,
    show: &str,
    ctype: &str,
    dups: bool,
) -> Option<Matcher> {
    use cr_core::model::enums::MatcherMode;
    let engine = cr_core::settings::EngineConfiguration::global();
    let read_at = engine.is_read_completion_percentage.to_string();
    let not_read_at = engine.is_not_read_completion_percentage.to_string();
    let reading_from = (engine.is_not_read_completion_percentage + 1).to_string();
    let reading_to = (engine.is_read_completion_percentage - 1).to_string();

    let mut quick: Option<Matcher> = None;
    let trimmed = text.trim();
    let upper = trimmed.to_ascii_uppercase();
    if scope == "all" && (upper.starts_with("NOT") || upper.starts_with("MATCH")) {
        let mut t = cr_engine::tokenizer::Tokenizer::new(text);
        if let Ok(group) = cr_engine::matcher::query::parse_group_query(&mut t) {
            quick = Some(Matcher::Group(group));
        }
    }
    if quick.is_none() {
        let mut list: Vec<cr_core::database::list_items::ComicBookMatcher> = Vec::new();
        match show {
            "read" => list.push(raw_value_matcher(
                "ComicBookReadPercentageMatcher",
                cr_engine::matcher::spec::ops::NUM_GREATER as i32,
                &read_at,
                "",
                false,
                None,
            )),
            "reading" => list.push(raw_value_matcher(
                "ComicBookReadPercentageMatcher",
                cr_engine::matcher::spec::ops::NUM_IN_RANGE as i32,
                &reading_from,
                &reading_to,
                false,
                None,
            )),
            "unread" => list.push(raw_value_matcher(
                "ComicBookReadPercentageMatcher",
                cr_engine::matcher::spec::ops::NUM_LESSER as i32,
                &not_read_at,
                "",
                false,
                None,
            )),
            _ => {}
        }
        match ctype {
            "books" => list.push(raw_value_matcher(
                "ComicBookFileMatcher",
                cr_engine::matcher::spec::ops::STR_EQUALS as i32,
                "",
                "",
                true,
                None,
            )),
            "fileless" => list.push(raw_value_matcher(
                "ComicBookFileMatcher",
                cr_engine::matcher::spec::ops::STR_EQUALS as i32,
                "",
                "",
                false,
                None,
            )),
            _ => {}
        }
        if !trimmed.is_empty() {
            // The C# `Create` passes operator 3 (ContainsAll) and the
            // RAW (untrimmed) search text. The option carries the C#
            // enum name (the action value is the lowercase id).
            let option = match scope {
                "series" => "Series",
                "writer" => "Writer",
                "artists" => "Artists",
                "descriptive" => "Descriptive",
                "catalog" => "Catalog",
                "file" => "File",
                _ => "All",
            };
            list.push(raw_value_matcher(
                "ComicBookAllPropertiesMatcher",
                cr_engine::matcher::spec::ops::STR_CONTAINS_ALL as i32,
                text,
                "",
                false,
                Some(option),
            ));
        }
        quick = match list.len() {
            0 => None,
            1 => Matcher::from_raw(&list[0]),
            _ => {
                let raws = list.iter().filter_map(Matcher::from_raw).collect();
                Some(Matcher::Group(cr_engine::matcher::tree::GroupMatcher {
                    matchers: raws,
                    matcher_mode: MatcherMode::And,
                    ..Default::default()
                }))
            }
        };
    }
    let mut parts: Vec<Matcher> = Vec::new();
    if let Some(q) = quick {
        parts.push(q);
    }
    if dups {
        if let Some(d) = Matcher::from_raw(&raw_value_matcher(
            "ComicBookDuplicateMatcher",
            0,
            "",
            "",
            false,
            None,
        )) {
            parts.push(d);
        }
    }
    match parts.len() {
        0 => None,
        1 => {
            // The C# wraps quickFilter in the GetCurrentMatcher
            // group, whose Match applies a child's `Not`
            // (`ComicBookValueMatcher.Match` ignores it). A bare Not
            // value matcher as the single part (e.g. Show only Books)
            // needs that wrapper — the set evaluator skips the root
            // matcher's own Not.
            let part = parts.pop().unwrap();
            if matches!(part, Matcher::Value(ref v) if v.not) {
                Some(Matcher::Group(cr_engine::matcher::tree::GroupMatcher {
                    matcher_mode: MatcherMode::And,
                    matchers: vec![part],
                    ..Default::default()
                }))
            } else {
                Some(part)
            }
        }
        _ => Some(Matcher::Group(cr_engine::matcher::tree::GroupMatcher {
            matcher_mode: MatcherMode::And,
            matchers: parts,
            ..Default::default()
        })),
    }
}

/// The page-export filter table (`ExportImage`, MainForm.cs:2190):
/// JPEG | BMP | PNG | GIF | TIFF, filter index 1-based.
const PAGE_EXPORT_FILTERS: &[(&str, &[&str], cr_image::decode::ImageFormat)] = &[
    (
        "JPEG Image",
        &["jpg", "jpeg"],
        cr_image::decode::ImageFormat::Jpeg,
    ),
    (
        "Windows Bitmap Image",
        &["bmp"],
        cr_image::decode::ImageFormat::Bmp,
    ),
    ("PNG Image", &["png"], cr_image::decode::ImageFormat::Png),
    ("GIF Image", &["gif"], cr_image::decode::ImageFormat::Gif),
    ("TIFF Image", &["tif"], cr_image::decode::ImageFormat::Tiff),
];

/// ARGB (premultiplied, cairo stride) → the RGBA currency
/// (`Bitmap.SaveImage` consumes the un-premultiplied form).
/// ARGB (premultiplied, cairo stride) → the RGBA currency
/// (`Bitmap.SaveImage` consumes the un-premultiplied form). Reads via
/// `with_data` — `data()` demands exclusive access (surface refcount
/// 1) and always fails while the caller holds a reference.
fn surface_to_image(surface: &gtk4::cairo::ImageSurface) -> Option<cr_image::Image> {
    surface.flush();
    let width = surface.width() as u32;
    let height = surface.height() as u32;
    let stride = surface.stride() as usize;
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    let read = surface.with_data(|data| {
        for y in 0..height as usize {
            let row = &data[y * stride..y * stride + width as usize * 4];
            for px in row.as_chunks::<4>().0 {
                let a = u32::from(px[3]);
                if a == 0 {
                    rgba.extend_from_slice(&[0, 0, 0, 0]);
                } else {
                    // Un-premultiply (identity for opaque pages).
                    let un = |v: u8| ((u32::from(v) * 255) / a) as u8;
                    rgba.extend_from_slice(&[un(px[2]), un(px[1]), un(px[0]), px[3]]);
                }
            }
        }
    });
    if let Err(err) = read {
        crate::trace::trace(format!("surface_to_image: with_data failed: {err}"));
        return None;
    }
    Some(cr_image::Image {
        width,
        height,
        rgba,
    })
}

/// `Clipboard.SetImage` (ComicDisplay.cs:1441): the composed page
/// image as a texture on the default clipboard.
fn copy_surface_to_clipboard(surface: &gtk4::cairo::ImageSurface) {
    let Some(image) = surface_to_image(surface) else {
        crate::trace::trace("copy-page: surface_to_image returned None");
        return;
    };
    crate::trace::trace(format!(
        "copy-page: page image {}x{}",
        image.width, image.height
    ));
    let Ok(png) = cr_image::decode::encode_image(&image, cr_image::decode::ImageFormat::Png) else {
        crate::trace::trace("copy-page: png encode failed");
        return;
    };
    crate::trace::trace(format!("copy-page: png {} bytes", png.len()));
    let Some(display) = gdk::Display::default() else {
        crate::trace::trace("copy-page: no default display");
        return;
    };
    let provider = gdk::ContentProvider::for_bytes("image/png", &gdk::glib::Bytes::from_owned(png));
    match display.clipboard().set_content(Some(&provider)) {
        Ok(()) => crate::trace::trace("copy-page: clipboard set_content OK"),
        Err(err) => crate::trace::trace(format!("copy-page: set_content failed: {err}")),
    }
}

/// `ExportImage` (MainForm.cs:2185): the "Save Page as" native save
/// dialog. The initial name is "{Caption} - Page {N}" with the saved
/// filter index's extension (the C# `AddExtension`); the accepted
/// path gets the extension when missing, and the chosen filter index
/// persists in the settings.
fn export_page_dialog(
    window: &ApplicationWindow,
    caption: &str,
    page: usize,
    surface: Option<gtk4::cairo::ImageSurface>,
) {
    let Some(surface) = surface else {
        crate::trace::trace("export-page: no page image, dialog skipped");
        return;
    };
    let chooser = gtk4::FileChooserNative::builder()
        .title("Save Page as")
        .action(gtk4::FileChooserAction::Save)
        .transient_for(window)
        .modal(true)
        .build();
    let mut filter_handles: Vec<gtk4::FileFilter> = Vec::new();
    for (name, exts, _) in PAGE_EXPORT_FILTERS {
        let filter = gtk4::FileFilter::new();
        filter.set_name(Some(name));
        for ext in *exts {
            filter.add_pattern(&format!("*.{ext}"));
        }
        chooser.add_filter(&filter);
        filter_handles.push(filter);
    }
    let saved_index = cr_ui_settings().borrow().last_export_page_filter_index;
    let initial = saved_index.clamp(1, PAGE_EXPORT_FILTERS.len() as i32) as usize;
    // Select the saved one (1-based, the C# FilterIndex).
    chooser.set_filter(&filter_handles[initial - 1]);
    let (_, initial_exts, _) = PAGE_EXPORT_FILTERS[initial - 1];
    let name = format!(
        "{} - Page {}.{}",
        cr_io::export::make_valid_filename(caption),
        page,
        initial_exts[0]
    );
    chooser.set_current_name(&name);
    crate::trace::trace(format!("export-page: chooser shown, initial name {name:?}"));
    let app = window.application();
    chooser.connect_response(move |chooser, response| {
        crate::trace::trace(format!(
            "export-page: response {response:?} (accept={:?})",
            gtk4::ResponseType::Accept
        ));
        let _ = &filter_handles;
        if response != gtk4::ResponseType::Accept {
            return;
        }
        let Some(path) = chooser.file().and_then(|f| f.path()) else {
            crate::trace::trace("export-page: response carried no file path");
            return;
        };
        // The chosen filter: position in the kept handle list.
        let selected = chooser.filter();
        let chosen = filter_handles
            .iter()
            .position(|f| selected.as_ref().is_some_and(|sel| sel == f))
            .map_or(initial, |i| i + 1);
        let (_, exts, format) = PAGE_EXPORT_FILTERS[chosen - 1];
        // `AddExtension`: append the filter's extension when missing.
        let mut path = path;
        if path.extension().is_none() {
            path.set_extension(exts[0]);
        }
        crate::trace::trace(format!("export-page: writing {path:?} (format {chosen})"));
        cr_ui_settings().borrow_mut().last_export_page_filter_index = chosen as i32;
        let Some(image) = surface_to_image(&surface) else {
            crate::trace::trace("export-page: surface_to_image returned None");
            return;
        };
        match cr_image::decode::encode_image(&image, format) {
            Ok(bytes) => {
                crate::trace::trace(format!("export-page: encoded {} bytes", bytes.len()));
                if let Err(err) = std::fs::write(&path, bytes) {
                    // `CouldNotSaveImage` parity — an error dialog.
                    crate::trace::trace(format!("export-page: write failed: {err}"));
                    if let Some(app) = &app {
                        show_error_dialog(app, &path.to_string_lossy(), &err.to_string());
                    }
                } else {
                    crate::trace::trace("export-page: file written");
                }
            }
            Err(err) => {
                crate::trace::trace(format!("export-page: encode failed: {err}"));
                if let Some(app) = &app {
                    show_error_dialog(app, &path.to_string_lossy(), &err.to_string());
                }
            }
        }
    });
    chooser.show();
}

fn open_file_dialog(window: &ApplicationWindow, on_open: impl Fn(&str) + 'static) {
    let chooser = gtk4::FileChooserNative::builder()
        .title("Open Comic")
        .action(gtk4::FileChooserAction::Open)
        .transient_for(window)
        .modal(true)
        .build();
    let filter = gtk4::FileFilter::new();
    filter.set_name(Some("Comic files"));
    for ext in crate::app::OPEN_FILTER_EXTS {
        filter.add_pattern(&format!("*.{ext}"));
    }
    chooser.add_filter(&filter);
    chooser.connect_response(move |chooser, response| {
        if response != gtk4::ResponseType::Accept {
            return;
        }
        if let Some(path) = chooser.file().and_then(|f| f.path()) {
            on_open(&path.to_string_lossy());
        }
    });
    chooser.show();
}

fn show_error_dialog(parent: &Application, title: &str, message: &str) {
    let dialog = gtk4::MessageDialog::builder()
        .application(parent)
        .title("comicrust")
        .text(format!("Cannot open {title}"))
        .secondary_text(message.to_string())
        .message_type(gtk4::MessageType::Error)
        .buttons(gtk4::ButtonsType::Close)
        .build();
    dialog.connect_response(|dialog, _| dialog.destroy());
    dialog.present();
}

#[cfg(test)]
mod tests {
    use super::*;
    use cr_core::xml::scalar::CrGuid;

    fn book(series: &str, writer: &str, read_pct: f32, path: &str) -> ComicBook {
        let mut b = ComicBook {
            id: CrGuid::new_random(),
            file_path: path.into(),
            ..Default::default()
        };
        b.info.series = series.into();
        b.info.writer = writer.into();
        if read_pct > 0.0 {
            b.info.page_count = 100;
            b.last_page_read = ((read_pct / 100.0) * 99.0).round() as i32;
        }
        b
    }

    fn eval(matcher: &Matcher, books: &[ComicBook]) -> Vec<usize> {
        let items: Vec<&ComicBook> = books.iter().collect();
        let ctx = cr_engine::matcher::eval::MatchContext::new(&items);
        let pairs = [(cr_core::model::enums::MatcherMode::And, false, matcher)];
        cr_engine::matcher::eval::match_set(&items, &pairs, &ctx)
            .iter()
            .filter_map(|b| books.iter().position(|x| x.id == b.id))
            .collect()
    }

    /// The quick search text contains (all fields) — the Create path
    /// with operator 3 (ContainsAll).
    #[test]
    fn compose_matches_by_text() {
        let books = [
            book("Batman", "Frank Miller", 0.0, "/a.cbz"),
            book("Spider-Man", "Stan Lee", 0.0, "/b.cbz"),
        ];
        let m = compose_quick_filter("batman", "all", "all", "all", false).unwrap();
        let hit = eval(&m, &books);
        assert_eq!(hit, vec![0]);
        // The Writer scope narrows to the writer field.
        let m = compose_quick_filter("stan", "writer", "all", "all", false).unwrap();
        assert_eq!(eval(&m, &books), vec![1]);
        // A different scope misses.
        let m = compose_quick_filter("stan", "series", "all", "all", false).unwrap();
        assert!(eval(&m, &books).is_empty());
    }

    /// The read-state filter (the engine defaults: read >= 95,
    /// unread < 10, reading 11..=94).
    #[test]
    fn compose_applies_the_read_filter() {
        let books = [
            book("a", "w", 100.0, "/a.cbz"),
            book("b", "w", 50.0, "/b.cbz"),
            book("c", "w", 0.0, "/c.cbz"),
        ];
        let m = compose_quick_filter("", "all", "read", "all", false).unwrap();
        assert_eq!(eval(&m, &books), vec![0]);
        let m = compose_quick_filter("", "all", "reading", "all", false).unwrap();
        assert_eq!(eval(&m, &books), vec![1]);
        let m = compose_quick_filter("", "all", "unread", "all", false).unwrap();
        assert_eq!(eval(&m, &books), vec![2]);
        // No filters, no text → no matcher at all (the C# null).
        assert!(compose_quick_filter("", "all", "all", "all", false).is_none());
    }

    /// The comic-type filter: a file path = Books, empty = fileless.
    #[test]
    fn compose_applies_the_comic_type_filter() {
        let books = [book("a", "w", 0.0, "/a.cbz"), book("b", "w", 0.0, "")];
        let m = compose_quick_filter("", "all", "all", "books", false).unwrap();
        assert_eq!(eval(&m, &books), vec![0]);
        let m = compose_quick_filter("", "all", "all", "fileless", false).unwrap();
        assert_eq!(eval(&m, &books), vec![1]);
    }

    /// Duplicates-only rides on top of everything (set-based).
    #[test]
    fn compose_applies_duplicates() {
        let books = [
            book("a", "w", 0.0, "/a.cbz"),
            book("a", "w", 0.0, "/b.cbz"),
            book("c", "w", 0.0, "/c.cbz"),
        ];
        let m = compose_quick_filter("", "all", "all", "all", true).unwrap();
        let hit = eval(&m, &books);
        assert!(hit.contains(&0) && hit.contains(&1) && !hit.contains(&2));
    }

    /// A MATCH query parses only for the All scope, and then the
    /// view filters do NOT apply (the C# UpdateQuickFilter order).
    #[test]
    fn compose_match_query_only_for_all_scope() {
        let books = [
            book("Batman", "w", 100.0, "/a.cbz"),
            book("Superman", "w", 0.0, "/b.cbz"),
        ];
        let m = compose_quick_filter(
            "MATCH [Series] contains \"Batman\"",
            "all",
            "all",
            "all",
            false,
        );
        assert_eq!(eval(m.as_ref().unwrap(), &books), vec![0]);
        // A non-All scope keeps the AllProperties path (the query
        // text searches the scoped fields — no hit on "MATCH ...").
        let m = compose_quick_filter(
            "MATCH [Series] contains \"Batman\"",
            "series",
            "all",
            "all",
            false,
        );
        assert!(eval(m.as_ref().unwrap(), &books).is_empty());
    }
}
