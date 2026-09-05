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

use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{gio, Application, ApplicationWindow, Button, Entry, Label, MenuButton, Paned, Stack};

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
    status: Label,
    navigator: Rc<Navigator>,
    item_view: ItemView,
    quick_view: ItemView,
    pages: PagesPanel,
    /// The browser-panel tab strip: Library | Pages (`MainView`).
    panel_stack: Stack,
    /// The left panel host (switcher + stack) — the Sidebar toggle.
    panel_box: gtk4::Box,
    /// The quick-search entry (the FocusQuickSearch command target).
    search: Entry,
    reader: ReaderShell,
    app: Application,
    /// The current navigator selection (refreshes after mutations).
    current_list: RefCell<Option<CrGuid>>,
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
    /// The main-window menubar (Phase 5.5 T3; visibility is the
    /// `OnGuiVisibilities` rule).
    menubar: super::menubar::MenubarWidget,
    /// The Alt-reveal override (the `AutoHideMainMenu` toggle).
    menubar_revealed: Cell<bool>,
}

impl ShellState {
    /// Opens a comic into the docked reader and shows it (the C#
    /// `OpenComic`; the browser hides while the reader shows).
    fn open_comic(&self, path: &Path) {
        match self.reader.open_comic(path) {
            Ok(()) => {
                // The Pages panel binds the open comic (the C#
                // `Viewer_BookChanged` → `pagesView.Book`).
                if let Some(book) = self.reader.current_comic_book() {
                    self.pages.set_book(book);
                    self.panel_stack.set_visible_child_name("pages");
                }
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
            if let Some((_name, books)) = library::evaluate_books(&id) {
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

    fn update_status(&self, count: usize, selected: usize) {
        if selected > 0 {
            self.status
                .set_text(&format!("{count} book(s), {selected} selected"));
        } else {
            self.status.set_text(&format!("{count} book(s)"));
        }
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

        // One pool for the whole app (the C# `Program.ImagePool` is
        // global).
        let pool = Arc::new(ImagePool::new(None));
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
            scroller: pages_scroller,
            panel: pages,
        } = super::pages_view::PagesPanel::create(Arc::clone(&pool));

        // The header commands (the handlers wire in `wire`, where
        // the shared state exists).
        let header = gtk4::HeaderBar::new();
        let open_button = Button::with_label("Open…");
        header.pack_start(&open_button);
        let add_folder_button = Button::with_label("Add Folder to Library…");
        header.pack_start(&add_folder_button);
        let search = Entry::builder()
            .placeholder_text("Quick search (or a Match query)")
            .hexpand(true)
            .build();
        header.pack_start(&search);

        // `BrowserVisible` — reveals the browser while the reader is
        // open (the reader page returns on the next open or re-dock).
        let browser_button = Button::with_label("Browser");
        header.pack_end(&browser_button);
        let prefs_button = Button::with_label("Preferences");
        prefs_button.set_css_classes(&["flat"]);
        header.pack_end(&prefs_button);

        let view_button = MenuButton::builder()
            .label("View")
            .css_classes(["flat"])
            .build();
        view_button.set_menu_model(Some(&view_menu_model()));
        header.pack_end(&view_button);
        let sort_button = MenuButton::builder()
            .label("Sort")
            .css_classes(["flat"])
            .build();
        sort_button.set_menu_model(Some(&sort_menu_model()));
        header.pack_end(&sort_button);
        let group_button = MenuButton::builder()
            .label("Group")
            .css_classes(["flat"])
            .build();
        group_button.set_menu_model(Some(&group_menu_model()));
        header.pack_end(&group_button);
        // The reader's "Page X of Y" lives in the main window header
        // (the C# main form shows it in the title area).
        header.pack_end(&reader_widgets.subtitle());
        window.set_titlebar(Some(&header));

        // The browser page: the left panel's tab strip — Library |
        // Pages (`MainView`: tsbLibrary/tsbPages; the Pages tab only
        // exists while a comic is open) — and the ItemView, with the
        // status bar below.
        let panel_stack = Stack::new();
        panel_stack.set_vhomogeneous(false);
        panel_stack.add_titled(navigator.widget(), Some("library"), "Library");
        panel_stack.add_titled(&pages_scroller, Some("pages"), "Pages");

        let switcher = gtk4::StackSwitcher::builder().stack(&panel_stack).build();
        let panel_box = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        panel_box.append(&switcher);
        panel_box.append(&panel_stack);

        let paned = Paned::new(gtk4::Orientation::Horizontal);
        paned.set_start_child(Some(&panel_box));
        paned.set_shrink_start_child(false);
        paned.set_position(280);
        paned.set_end_child(Some(&item_scroller));
        paned.set_shrink_end_child(false);
        let status = Label::builder()
            .halign(gtk4::Align::Start)
            .margin_top(4)
            .margin_bottom(4)
            .margin_start(8)
            .build();
        status.set_text("0 book(s)");
        let browser_page = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        browser_page.append(&paned);
        browser_page.append(&status);

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

        // The stack: quick open ⇄ browser ⇄ reader (`BrowserVisible`
        // + the QuickOpen empty state).
        let stack = Stack::new();
        stack.set_vhomogeneous(false);
        stack.set_hhomogeneous(false);
        stack.add_named(&quick_page, Some("quickopen"));
        stack.add_named(&browser_page, Some("browser"));
        stack.add_named(&reader_widgets.notebook(), Some("reader"));

        // The menubar (the C# `mainMenuStrip`) rides above the
        // content — the T3 custom bar (GTK4 model menus cannot show
        // the C# menu-item icons).
        let menubar = super::menubar::create_menubar(&window);
        let content = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        content.append(menubar.widget());
        content.append(&stack);
        window.set_child(Some(&content));

        let state = Rc::new(ShellState {
            window: window.clone(),
            stack: stack.clone(),
            status,
            navigator: Rc::clone(&navigator),
            item_view,
            quick_view,
            pages,
            panel_stack,
            panel_box: panel_box.clone(),
            search: search.clone(),
            reader,
            app: app.clone(),
            current_list: RefCell::new(None),
            actions: RefCell::new(HashMap::new()),
            list_history: RefCell::new(Vec::new()),
            list_history_pos: Cell::new(0),
            random_list: RefCell::new(Vec::new()),
            random_picked: RefCell::new(Vec::new()),
            menubar,
            menubar_revealed: Cell::new(false),
        });
        let shell = BrowserShell {
            window: window.clone(),
            state: Rc::clone(&state),
        };
        shell.wire(
            &open_button,
            &add_folder_button,
            &search,
            &browser_button,
            &prefs_button,
        );
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

    /// The main window handle.
    pub fn window(&self) -> ApplicationWindow {
        self.window.clone()
    }

    fn wire(
        &self,
        open_button: &Button,
        add_folder_button: &Button,
        search: &Entry,
        browser_button: &Button,
        prefs_button: &Button,
    ) {
        let state = &self.state;

        // The reader docks: the host window drives the fullscreen
        // chrome and the Q exit.
        state.reader.set_host(&self.window);

        // The last reader tab closes → the browser view shows again
        // (the C# `Close` reveals the browser).
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .reader
                .set_on_last_tab_closed(move || {
                    if let Some(sh) = state.upgrade() {
                        sh.pages.clear_book();
                        sh.show_quick_open();
                        sh.sync_enabled();
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
                            "ShowBrowser" => sh.show_browser(),
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
                            sh.stack.set_visible_child_name("browser");
                        }
                        // Undock/re-dock changes the menubar rule.
                        sh.sync_enabled();
                    }
                });
        }

        // The Browser button: reveal the browser grid while comics
        // stay open in the reader (`BrowserVisible`).
        {
            let state = Rc::downgrade(state);
            browser_button.connect_clicked(move |_| {
                if let Some(sh) = state.upgrade() {
                    sh.show_browser();
                }
            });
        }

        // The Preferences dialog (the C# Tools → Preferences): the
        // shared handler (the action routes to the same method).
        {
            let state = Rc::downgrade(state);
            prefs_button.connect_clicked(move |_| {
                if let Some(sh) = state.upgrade() {
                    sh.show_preferences();
                }
            });
        }

        // The Pages panel: rebinds on every visible-book change (the
        // C# `Viewer_BookChanged` → `pagesView.Book`), follows the
        // bound book's page turns, and navigates on double-click.
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .reader
                .set_on_book_changed(move || {
                    if let Some(sh) = state.upgrade() {
                        if let Some(book) = sh.reader.current_comic_book() {
                            let page = book.current_page.max(0) as usize;
                            sh.pages.set_book(book);
                            sh.pages.set_current_page(page);
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
                .panel_stack
                .connect_visible_child_notify(move |_stack| {
                    if let Some(sh) = state.upgrade() {
                        sh.pages.reflow();
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
                        if let Some((_name, books)) = library::evaluate_books(id) {
                            let count = books.len();
                            sh.item_view.set_books(books);
                            sh.update_status(count, 0);
                        }
                        sh.sync_enabled();
                    }
                });
        }

        // The selection count on the status bar.
        {
            let state = Rc::downgrade(state);
            state
                .upgrade()
                .expect("state")
                .item_view
                .connect_selection_changed(move |selected| {
                    if let Some(sh) = state.upgrade() {
                        let count = sh.item_view.book_count();
                        sh.update_status(count, selected);
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

        // The quick search (`UpdateQuickFilter`): the AllProperties
        // matcher, or a full query for MATCH/NOT text. Debounced.
        {
            let state = Rc::downgrade(state);
            search.connect_changed(move |entry| {
                let text = entry.text().to_string();
                let state = state.clone();
                glib::timeout_add_local(
                    std::time::Duration::from_millis(SEARCH_DEBOUNCE_MS),
                    move || {
                        if let Some(sh) = state.upgrade() {
                            let matcher = search_matcher(&text);
                            sh.item_view.set_filter(matcher);
                        }
                        glib::ControlFlow::Break
                    },
                );
            });
        }

        // Open… → the file dialog into the docked reader.
        {
            let state = Rc::downgrade(state);
            let window = self.window.clone();
            open_button.connect_clicked(move |_| {
                let state = state.clone();
                open_file_dialog(&window, move |path| {
                    if let Some(sh) = state.upgrade() {
                        sh.open_comic(Path::new(&path));
                    }
                });
            });
        }

        // Add Folder to Library… → the scan; the navigator tree and
        // the current view refresh.
        {
            let state = Rc::downgrade(state);
            let window = self.window.clone();
            add_folder_button.connect_clicked(move |_| {
                crate::app::add_folder_dialog(&window);
                // The scan is async; the tree refreshes with it
                // (the scan result dialog confirms).
                {
                    let state2 = state.clone();
                    glib::timeout_add_local(std::time::Duration::from_millis(500), move || {
                        if let Some(sh) = state2.upgrade() {
                            sh.navigator.refill(&library::comic_lists_snapshot());
                        }
                        glib::ControlFlow::Break
                    });
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
                            sh.reader.focus_current();
                        } else {
                            sh.item_view.grab_focus();
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
        {
            let lists = library::quick_open_lists();
            let total: usize = lists.iter().map(|(_, b)| b.len()).sum();
            if total > 0 {
                let mut books: Vec<ComicBook> = Vec::new();
                for (_, group) in lists {
                    books.extend(group);
                }
                state.quick_view.set_books(books);
                state.stack.set_visible_child_name("quickopen");
            }
        }
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
            if let Some(sh) = state.upgrade() {
                f(&sh);
                // The C# re-syncs the command enable/check states on
                // every menu operation (`CommandMapper` idle update);
                // every action dispatch refreshes ours.
                sh.sync_enabled();
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
        let has_book = !self.reader.is_empty();
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
        ] {
            self.set_action_enabled(name, has_book);
        }
        self.set_action_enabled("prev-tab", slots > 1);
        self.set_action_enabled("next-tab", slots > 1);
        // Selection commands (`GetBookList(Selected)` non-empty).
        for name in [
            "info", "rating-0", "rating-1", "rating-2", "rating-3", "rating-4", "rating-5",
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
        // `() => BrowserVisible`, `() => Program.Settings.AutoScrolling`
        // (the view mirrors it), `() => ComicDisplay.TwoPageNavigation`,
        // MinimalGui / FullScreen / Autorotate.
        if let Some(a) = self.action("toggle-browser") {
            let visible = self.stack.visible_child_name().as_deref() == Some("browser");
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
        self.update_menubar();
        self.sync_menubar();
    }

    /// Applies the menubar visibility rule (`OnGuiVisibilities`
    /// Fill-mode parity — `menubar::menubar_visible`).
    fn update_menubar(&self) {
        let minimal = self.reader.is_minimal_gui();
        let undocked = self.reader.is_undocked();
        let is_comic_viewer = self.stack.visible_child_name().as_deref() == Some("reader");
        let has_book = !self.reader.is_empty();
        let (auto_hide, show_no_comic) = {
            let s = cr_ui_settings();
            let b = s.borrow();
            (b.auto_hide_main_menu, b.show_main_menu_no_comic_open)
        };
        let revealed = self.menubar_revealed.get();
        let visible = super::menubar::menubar_visible(
            minimal,
            undocked,
            is_comic_viewer,
            has_book,
            auto_hide,
            show_no_comic,
            revealed,
        );
        self.menubar.widget().set_visible(visible);
    }

    /// Pushes the current action states into the menubar rows
    /// (check/radio marks + disabled graying + the hide rules — the
    /// custom bar has no model-driven state rendering).
    fn sync_menubar(&self) {
        let actions = self.actions.borrow();
        // The active-panel emphasis (the C# highlights the
        // miViewLibrary/miViewPages row of the shown panel — no
        // checkbox on those items).
        let panel = self.panel_stack.visible_child_name().unwrap_or_default();
        let panel = panel.as_str();
        // `fileMenu_DropDownOpening`: "Update all Book Files" hides
        // while `AutoUpdateComicsFiles` is on.
        let update_files_visible = !cr_ui_settings().borrow().auto_update_comics_files;
        self.menubar.sync(&|base| {
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
            Some(super::menubar::ActionState {
                enabled: action.is_enabled(),
                state: action.state(),
                highlight,
                visible,
            })
        });
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
        let has_book = !self.reader.is_empty();
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
        crate::dialogs::book_editor::show(&self.window, books, commit);
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
        self.menubar
            .set_dyn_fill(Rc::new(move |id| match state.upgrade() {
                Some(sh) => sh.dyn_fill(id),
                None => Vec::new(),
            }));
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
            _ => Vec::new(),
        }
    }

    /// `RefreshDisplay` (F5): the tree re-fills and the current list
    /// re-evaluates.
    fn refresh_view(&self) {
        self.navigator.refill(&library::comic_lists_snapshot());
        self.refresh_view_from_list();
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

    /// `ToggleBrowser`: the browser and reader pages flip; without a
    /// book the browser/QuickOpen stays.
    fn toggle_browser(&self) {
        let visible = self
            .stack
            .visible_child_name()
            .map(|s| s.to_string())
            .unwrap_or_default();
        match visible.as_str() {
            "reader" => self.show_browser(),
            "browser" | "quickopen" if !self.reader.is_empty() => {
                self.stack.set_visible_child_name("reader");
            }
            _ => {}
        }
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
            mode_action.connect_activate(move |action, value| {
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
                action.set_state(&name.to_variant());
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

        // sort-column (string parameter = the property name).
        let sort_action = gio::SimpleAction::new("sort-column", Some(glib::VariantTy::STRING));
        {
            let state = state.clone();
            sort_action.connect_activate(move |_, value| {
                if let (Some(sh), Some(column)) =
                    (state.upgrade(), value.and_then(|v| v.get::<String>()))
                {
                    sh.item_view.set_sort_column(&column);
                }
            });
        }
        group.add_action(&sort_action);

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

        // group-by (string parameter; "" = none).
        let group_action = gio::SimpleAction::new("group-by", Some(glib::VariantTy::STRING));
        {
            let state = state.clone();
            group_action.connect_activate(move |_, value| {
                if let Some(sh) = state.upgrade() {
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
                }
            });
        }
        group.add_action(&group_action);

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
        // `OpenBooks.AddSlot`: the empty slot shows QuickOpen.
        self.add_simple(&group, "new-tab", |sh| sh.show_quick_open());
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
                sh.reader.switch_to_slot(slot);
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
        // tasks — T13 lands the Tasks dialog.
        self.add_disabled(&group, "tasks");
        // generate-thumbnails — the C# `CacheThumbnails` queue
        // command; the thumbnail-queue work owns it.
        self.add_disabled(&group, "generate-thumbnails");
        // new-book-entry — fileless books are unported.
        self.add_disabled(&group, "new-book-entry");
        self.add_simple(&group, "restart", |sh| {
            // `MenuRestart`: save, then re-launch the binary (the C#
            // `Program.Restart` + `Application.Restart`).
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
        // quick-rating — T13 lands the dialog.
        self.add_disabled(&group, "quick-rating");
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
        // copy-page / export-page — the clipboard path is unported.
        self.add_disabled(&group, "copy-page");
        self.add_disabled(&group, "export-page");
        self.add_simple(&group, "refresh", |sh| sh.refresh_view());
        self.add_simple(&group, "preferences", ShellState::show_preferences);

        // --- Browse ---
        // `ToggleBrowser` with the `() => BrowserVisible` check.
        self.add_check(&group, "toggle-browser", true, |sh| sh.toggle_browser());
        self.add_simple(&group, "view-library", |sh| {
            sh.show_browser();
            sh.panel_stack.set_visible_child_name("library");
        });
        self.add_simple(&group, "view-pages", |sh| {
            sh.show_browser();
            sh.panel_stack.set_visible_child_name("pages");
        });
        {
            let sidebar = gio::SimpleAction::new_stateful("sidebar", None, &true.to_variant());
            let state = state.clone();
            sidebar.connect_activate(move |action, _| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let visible = !sh.panel_box.is_visible();
                sh.panel_box.set_visible(visible);
                action.set_state(&visible.to_variant());
            });
            group.add_action(&sidebar);
            self.actions.borrow_mut().insert("sidebar", sidebar);
        }
        // small-preview — T11 lands the pane.
        self.add_disabled(&group, "small-preview");
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
            sh.reader.dispatch_current("PrevTab")
        });
        self.add_simple(&group, "next-tab", |sh| {
            sh.reader.dispatch_current("NextTab")
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
            let track = gio::SimpleAction::new_stateful(
                "track-current-page",
                None,
                &cr_ui_settings().borrow().track_current_page.to_variant(),
            );
            track.connect_activate(move |action, _| {
                let next = !cr_ui_settings().borrow().track_current_page;
                cr_ui_settings().borrow_mut().track_current_page = next;
                action.set_state(&next.to_variant());
            });
            group.add_action(&track);
        }

        // --- Display ---
        // display-settings — T12 lands the dialog.
        self.add_disabled(&group, "display-settings");
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
        // zoom-custom — T13 lands the dialog.
        self.add_disabled(&group, "zoom-custom");
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
        // about — T13 lands the About dialog.
        self.add_disabled(&group, "about");

        // --- The mainKeys shell commands ---
        self.add_simple(&group, "focus-search", |sh| {
            sh.search.grab_focus();
        });
        // toggle-navigator-search — T7 lands the navigator search box.
        self.add_disabled(&group, "toggle-navigator-search");

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
    /// (fullscreen enter/leave, MinimalGui) and the `AutoHideMainMenu`
    /// Alt reveal — Alt pressed and released ALONE toggles the
    /// reveal (`MainForm.OnKeyUp`; the 500 ms re-close debounce is
    /// not ported). GTK4 has no way to OPEN a PopoverMenuBar from
    /// code, so the C#'s "select the first item" step is not ported.
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
        // Alt alone reveals/hides the auto-hidden menubar.
        let alt_alone = Rc::new(Cell::new(false));
        let controller = gtk4::EventControllerKey::new();
        {
            let alt_alone = Rc::clone(&alt_alone);
            controller.connect_key_pressed(move |_c, key, _code, _mods| {
                alt_alone.set(matches!(key, gtk4::gdk::Key::Alt_L | gtk4::gdk::Key::Alt_R));
                glib::Propagation::Proceed
            });
        }
        {
            let state = Rc::downgrade(self);
            controller.connect_key_released(move |_c, key, _code, _mods| {
                if !matches!(key, gtk4::gdk::Key::Alt_L | gtk4::gdk::Key::Alt_R) {
                    return;
                }
                if !alt_alone.replace(false) {
                    return; // Another key sat between press and release.
                }
                let Some(sh) = state.upgrade() else {
                    return;
                };
                // `enableAutoHideMenu` parity: only while auto-hidden
                // and not minimal.
                let minimal = sh.reader.is_minimal_gui();
                let auto_hide = cr_ui_settings().borrow().auto_hide_main_menu;
                if !auto_hide || minimal {
                    return;
                }
                sh.menubar_revealed.set(!sh.menubar_revealed.get());
                sh.update_menubar();
            });
        }
        self.window.add_controller(controller);
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
    popover.popup();
}

/// `ComicBookAllPropertiesMatcher.Create` for the quick search: a
/// contains over the All field set; MATCH/NOT text parses as a full
/// query (`UpdateQuickFilter`).
fn search_matcher(text: &str) -> Option<Matcher> {
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let upper = text.to_ascii_uppercase();
    if upper.starts_with("MATCH") || upper.starts_with("NOT") {
        let mut t = cr_engine::tokenizer::Tokenizer::new(text);
        let group = cr_engine::matcher::query::parse_group_query(&mut t).ok()?;
        return Some(Matcher::Group(group));
    }
    let raw = cr_core::database::list_items::ComicBookMatcher::Value(
        cr_core::database::list_items::ValueMatcher {
            type_name: "ComicBookAllPropertiesMatcher".into(),
            match_operator: 1, // contains (STRING_OPS order)
            match_value: text.to_string(),
            option: Some("All".into()),
            ..Default::default()
        },
    );
    Matcher::from_raw(&raw)
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

fn view_menu_model() -> gio::Menu {
    let menu = gio::Menu::new();
    let mode = gio::Menu::new();
    mode.append(Some("Thumbnails"), Some("win.view-mode::thumbnail"));
    mode.append(Some("Tiles"), Some("win.view-mode::tile"));
    mode.append(Some("Details"), Some("win.view-mode::detail"));
    menu.append_section(None, &mode);
    let size = gio::Menu::new();
    size.append(Some("Bigger Covers"), Some("win.thumb-bigger"));
    size.append(Some("Smaller Covers"), Some("win.thumb-smaller"));
    menu.append_section(None, &size);
    menu
}

fn sort_menu_model() -> gio::Menu {
    let menu = gio::Menu::new();
    let columns = gio::Menu::new();
    for column in default_columns().iter().filter(|c| c.visible) {
        columns.append(
            Some(column.name),
            Some(&format!("win.sort-column::{}", column.property)),
        );
    }
    menu.append_section(None, &columns);
    menu.append(Some("Reverse Direction"), Some("win.sort-direction"));
    menu
}

fn group_menu_model() -> gio::Menu {
    let menu = gio::Menu::new();
    menu.append(Some("No Grouping"), Some("win.group-by::"));
    for (key, _) in cr_engine::group::groupers() {
        menu.append(Some(key), Some(&format!("win.group-by::{key}")));
    }
    menu
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
