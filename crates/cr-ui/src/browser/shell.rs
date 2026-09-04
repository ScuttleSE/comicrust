//! The browser shell — the app's main window (the C# `MainForm`):
//! the navigator pane + ItemView in a paned container with a status
//! bar, the quick search, the view-mode/sort/group/size/columns
//! commands, and the reader docked as a view (the C# reader replaces
//! the browser view; `D` undocks it into its own window).

use std::cell::RefCell;
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
    reader: ReaderShell,
    app: Application,
    /// The current navigator selection (refreshes after mutations).
    current_list: RefCell<Option<CrGuid>>,
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
                self.item_view.set_books(books);
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
        window.set_child(Some(&stack));

        let state = Rc::new(ShellState {
            window: window.clone(),
            stack: stack.clone(),
            status,
            navigator: Rc::clone(&navigator),
            item_view,
            quick_view,
            pages,
            panel_stack,
            reader,
            app: app.clone(),
            current_list: RefCell::new(None),
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

        // The Preferences dialog (the C# Tools → Preferences): a
        // modal settings clone committed on OK; the reader views and
        // the QuickOpen grid re-apply the changed values.
        {
            let state = Rc::downgrade(state);
            prefs_button.connect_clicked(move |_| {
                let Some(sh) = state.upgrade() else {
                    return;
                };
                let window = sh.window.clone();
                let state2 = state.clone();
                crate::settings::preferences::show_preferences(&window, move || {
                    if let Some(sh) = state2.upgrade() {
                        sh.reader.apply_settings_to_open_views();
                        let size = cr_ui_settings().borrow().quick_open_thumbnail_size as f64;
                        sh.quick_view.configure(|c| c.thumb_height = size);
                    }
                });
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
                        if let Some((_name, books)) = library::evaluate_books(id) {
                            let count = books.len();
                            sh.item_view.set_books(books);
                            sh.update_status(count, 0);
                        }
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
        let state = Rc::downgrade(&self.state);
        let actions = gio::SimpleActionGroup::new();

        // view-mode: thumbnail | tile | detail (radio).
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
        actions.add_action(&mode_action);

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
            actions.add_action(&action);
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
        actions.add_action(&sort_action);

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
        actions.add_action(&dir_action);

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
        actions.add_action(&group_action);

        self.window.insert_action_group("win", Some(&actions));
    }

    /// Opens a comic into the docked reader (the app's `open_reader`
    /// path).
    pub fn open_comic(&self, path: &Path) {
        self.state.open_comic(path);
    }

    pub fn present(&self) {
        self.window.present();
    }
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
                    // The grid shows the edited values on commit (the
                    // "remove" command pattern; the single editor
                    // path refreshes per save point too).
                    {
                        let commit_state = state.clone();
                        let commit_refresh = Rc::new(move || {
                            if let Some(sh) = commit_state.upgrade() {
                                sh.refresh_view_from_list();
                            }
                        }) as Rc<dyn Fn()>;
                        let commit: crate::dialogs::book_editor::CommitFn =
                            Rc::new(move |edited| {
                                library::apply_edited(edited);
                                commit_refresh();
                            });
                        let window2 = window.clone();
                        crate::dialogs::bulk_edit::show(&window2, books, commit);
                    }
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
                "remove" => {
                    if let Some(id) = target {
                        library::remove_book(&id);
                        sh.refresh_view_from_list();
                    }
                }
                "properties" => {
                    // The selection (plus the right-clicked row when
                    // it is outside it) — the C# opens the dialog
                    // over the selected books (prev/next when > 1).
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
                    // List order for the prev/next walk.
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
                    let commit: crate::dialogs::book_editor::CommitFn = Rc::new(|edited| {
                        let lib = library::session();
                        let mut l = lib.borrow_mut();
                        if let Some(book) = l
                            .database_mut()
                            .books
                            .iter_mut()
                            .find(|b| b.id == edited.id)
                        {
                            *book = edited.clone();
                            l.mark_dirty();
                        }
                    });
                    crate::dialogs::book_editor::show(&window, books, commit);
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
