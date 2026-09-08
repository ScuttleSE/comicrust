//! The Files (Folders) view — the `ComicListFolderFilesBrowser` port:
//! the filesystem tree (`tvFolders`, lazy-expanded), the panel
//! toolbar (favorites dropdown, Add To Favorites, Refresh, Add Folder
//! To Library, Include Sub Folders), and the folder book list
//! (`FolderComicListProvider.GetFolderBookList` — the files of the
//! selected folder as `AddToTemporary` session books).
//!
//! Deviations: the C# tvFolders shows shell icons (the port: names
//! only — the shell image list is not portable) and hidden folders
//! (the port skips dot-dirs, the Linux convention); RemoveFavorite
//! and Open Window/Tab are not ported; the folder books read stored
//! metadata only for the first 100 files (the C#
//! `DontReadInformation` rule).

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::Path;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{
    Button, MenuButton, Popover, ScrolledWindow, ToggleButton, TreeIter, TreeSelection, TreeStore,
    TreeView, TreeViewColumn,
};

use cr_core::model::comic_book::ComicBook;
use cr_io::info::InfoLoadingMethod;

use crate::library;

/// The tree columns.
const COL_NAME: u32 = 0;
const COL_PATH: u32 = 1;
const COL_NAME_I: i32 = COL_NAME as i32;
const COL_PATH_I: i32 = COL_PATH as i32;

type SelectedFn = Box<dyn Fn(&str)>;
type IncludeFn = Box<dyn Fn(bool)>;

/// `FolderComicListProvider.GetFolderBookList`: the folder's comic
/// files (the provider extension registry — `Providers.Readers.
/// GetFileExtensions` + the EndsWith check) as fresh session books
/// (`CreateBookOption.AddToTemporary`). `FileUtility.GetFiles`
/// order (files first, then recursion — plain walk, no
/// `comicrackscanner.ini` honoring). Stored metadata loads for the
/// first 100 files only (`RefreshInfoOptions.DontReadInformation`
/// beyond), the provider page count always wins (the Phase 5 merge
/// rule).
pub fn folder_book_list(folder: &Path, include_sub: bool) -> Vec<ComicBook> {
    let mut files = Vec::new();
    collect_comic_files(folder, include_sub, &mut files);
    let now = cr_core::xml::scalar::CrDateTime::now();
    let mut list = Vec::new();
    for (i, file) in files.iter().enumerate() {
        let mut book = cr_engine::scanner::create_book(&file.to_string_lossy(), &now);
        if i < 100 {
            if let Ok(provider) = cr_io::ComicProvider::open(file) {
                if let Some(info) = provider.load_info(InfoLoadingMethod::Fast) {
                    let page_count = book.info.page_count;
                    book.info = info;
                    book.info.page_count = page_count;
                }
            }
        }
        list.push(book);
    }
    list
}

/// `FileUtility.GetFiles` shape: the folder's comic files, then the
/// subfolders when recursing. Names sort byte-wise (the Windows
/// directory order).
fn collect_comic_files(folder: &Path, include_sub: bool, out: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = std::fs::read_dir(folder) else {
        return;
    };
    let mut entries: Vec<std::path::PathBuf> =
        rd.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    entries.sort();
    let mut dirs = Vec::new();
    for path in entries {
        if path.is_dir() {
            dirs.push(path);
        } else if cr_io::formats::source_format(&path).is_some() {
            out.push(path);
        }
    }
    if include_sub {
        for d in dirs {
            collect_comic_files(&d, true, out);
        }
    }
}

pub struct FolderTreeWidgets {
    /// The mounted panel: [toolbar][tree].
    pub widget: gtk4::Box,
}

/// The filesystem tree panel (the C# `tvFolders` + the panel
/// `toolStrip`). One "/" root, lazily filled on expand; the
/// selection fires the host callback with the folder path.
pub struct FolderTree {
    widget: gtk4::Box,
    store: TreeStore,
    view: TreeView,
    selection: TreeSelection,
    /// The paths whose children are already in the store.
    filled: RefCell<HashSet<String>>,
    on_selected: RefCell<Option<SelectedFn>>,
    on_include_sub: RefCell<Option<IncludeFn>>,
    /// The favorites popover (rebuilt on every open — the dynamic
    /// fill lesson); the MenuButton is dropped after the toolbar
    /// mount (the popover keeps it alive).
    favorites_pop: Popover,
    /// The Include Sub Folders toggle (the C# `tbIncludeSubFolders`).
    include_btn: ToggleButton,
    /// (name, button) — the probe's real-click path.
    buttons: Vec<(&'static str, Button)>,
}

impl FolderTree {
    pub fn create() -> Rc<FolderTree> {
        let store = TreeStore::new(&[String::static_type(), String::static_type()]);
        let view = TreeView::with_model(&store);
        view.set_headers_visible(false);
        let name_cell = gtk4::CellRendererText::new();
        let col = TreeViewColumn::new();
        col.pack_start(&name_cell, true);
        col.add_attribute(&name_cell, "text", COL_NAME_I);
        view.append_column(&col);
        let selection = view.selection();
        selection.set_mode(gtk4::SelectionMode::Single);

        let scroller = ScrolledWindow::builder()
            .child(&view)
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vexpand(true)
            .build();

        // The panel toolbar (the C# Files browser's toolStrip
        // commands): the favorites dropdown, Add To Favorites,
        // Refresh, Include Sub Folders (check), Add Folder To
        // Library. Open Window/Tab skipped (no browser-window
        // surface); RemoveFavorite not ported.
        let toolbar = gtk4::Box::new(gtk4::Orientation::Horizontal, 2);
        toolbar.add_css_class("toolbar");
        let mut buttons: Vec<(&'static str, Button)> = Vec::new();
        let favorites_pop = Popover::new();
        favorites_pop.set_has_arrow(false);
        let favorites_menu = MenuButton::new();
        favorites_menu.add_css_class("flat");
        favorites_menu.set_tooltip_text(Some("Favorite Folders"));
        favorites_menu.set_popover(Some(&favorites_pop));
        if let Some(texture) = crate::icon::icon("Favorites") {
            let img = gtk4::Image::from_paintable(Some(&texture));
            img.set_pixel_size(16);
            favorites_menu.set_child(Some(&img));
        }
        toolbar.append(&favorites_menu);
        let mk = |bar: &gtk4::Box,
                  buttons: &mut Vec<(&'static str, Button)>,
                  name: &'static str,
                  icon: &'static str,
                  tooltip: &str| {
            let button = Button::new();
            button.add_css_class("flat");
            button.set_tooltip_text(Some(tooltip));
            if let Some(texture) = crate::icon::icon(icon) {
                let img = gtk4::Image::from_paintable(Some(&texture));
                img.set_pixel_size(16);
                button.set_child(Some(&img));
            }
            bar.append(&button);
            buttons.push((name, button));
        };
        mk(
            &toolbar,
            &mut buttons,
            "add-favorite",
            "AddFavorites",
            "Add the current folder to the favorites",
        );
        let include_btn = ToggleButton::new();
        include_btn.add_css_class("flat");
        include_btn.set_tooltip_text(Some("Include Sub Folders"));
        if let Some(texture) = crate::icon::icon("IncludeSubFolders") {
            let img = gtk4::Image::from_paintable(Some(&texture));
            img.set_pixel_size(16);
            include_btn.set_child(Some(&img));
        }
        toolbar.append(&include_btn);
        mk(&toolbar, &mut buttons, "refresh", "Refresh", "Refresh");
        mk(
            &toolbar,
            &mut buttons,
            "add-library",
            "AddFolder",
            "Add the current folder to the Library",
        );

        let widget = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        widget.append(&toolbar);
        widget.append(&scroller);

        let tree = Rc::new(FolderTree {
            widget,
            store,
            view,
            selection,
            filled: RefCell::new(HashSet::new()),
            on_selected: RefCell::new(None),
            on_include_sub: RefCell::new(None),
            favorites_pop,
            include_btn,
            buttons,
        });

        // The root row (the C# `tvFolders.Init` — the desktop roots;
        // Linux shows the filesystem root).
        tree.insert_root();
        wire(&tree);
        tree
    }

    pub fn widget(&self) -> &gtk4::Box {
        &self.widget
    }

    /// The selection-change callback (the folder path). Fires after
    /// the C# `tvFolders_AfterSelect`.
    pub fn connect_selected<F: Fn(&str) + 'static>(&self, f: F) {
        *self.on_selected.borrow_mut() = Some(Box::new(f));
    }

    /// The toolbar buttons by name (the probe's real-click path).
    pub fn button(&self, name: &str) -> Option<Button> {
        self.buttons
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, b)| b.clone())
    }

    /// The currently selected folder path.
    pub fn current_folder(&self) -> Option<String> {
        let (_model, iter) = self.selection.selected()?;
        self.row_path(&iter)
    }

    /// `DrillToFolder` + `SelectedNode.EnsureVisible`: expand the
    /// chain for `path`, select the row. Missing components clamp to
    /// the deepest existing ancestor.
    pub fn drill_to(&self, path: &str) {
        let Some(root_path) = self.root_path() else {
            return;
        };
        let Some(mut iter) = self.store.iter(&root_path) else {
            return;
        };
        let target = std::path::PathBuf::from(path);
        let mut acc = std::path::PathBuf::from("/");
        for comp in target.components().skip(1) {
            let name = comp.as_os_str().to_string_lossy().to_string();
            self.fill_children(&iter, &acc.to_string_lossy());
            acc.push(&name);
            let Some(child) = self.child_iter(&iter, &name) else {
                if std::env::var_os("CR_TRACE").is_some() {
                    eprintln!(
                        "[trace] folders: drill MISS {name} under {}",
                        acc.parent().and_then(|p| p.to_str()).unwrap_or("?")
                    );
                }
                break;
            };
            iter = child;
        }
        // The whole chain expands in one call (the C#
        // `DrillToFolder` shape; per-row expand_row fails on rows
        // whose parents just expanded — the navigator's
        // expand_to_path lesson).
        let final_path = self.store.path(&iter);
        self.view.expand_to_path(&final_path);
        if std::env::var_os("CR_TRACE").is_some() {
            eprintln!(
                "[trace] folders: drill to select {}",
                self.row_path(&iter).unwrap_or_default()
            );
        }
        self.selection.select_iter(&iter);
        self.view
            .scroll_to_cell(Some(&final_path), None, false, 0.0, 0.0);
    }

    /// `OnRefreshDisplay`: re-root the tree and re-drill to the
    /// current folder.
    pub fn refresh(&self) {
        let current = self.current_folder();
        self.store.clear();
        self.filled.borrow_mut().clear();
        self.insert_root();
        if let Some(p) = current {
            self.drill_to(&p);
        }
    }

    /// The include-sub-folders toggle callback (the C#
    /// `SwitchIncludeSubFolders` — the shell rescans).
    pub fn connect_include_sub<F: Fn(bool) + 'static>(&self, f: F) {
        *self.on_include_sub.borrow_mut() = Some(Box::new(f));
    }

    /// Marks the include button pressed/unpressed (the Settings
    /// restore at boot).
    pub fn set_include_sub(&self, active: bool) {
        self.include_btn.set_active(active);
    }

    fn insert_root(&self) {
        let iter = self.store.append(None);
        let name = "File System";
        let path = "/";
        let values: Vec<(u32, &dyn gtk4::glib::prelude::ToValue)> =
            vec![(COL_NAME, &name), (COL_PATH, &path)];
        self.store.set(&iter, &values);
        // The lazy-tree dummy: a childless row shows no expander, so
        // every fresh row carries one dummy child that the fill
        // swaps out (the C# ShellItemTree pre-populates the same
        // way).
        let dummy = self.store.append(Some(&iter));
        self.store.set(&dummy, &[(COL_NAME, &""), (COL_PATH, &"")]);
        // The root's children fill lazily (the first expand/drill) —
        // do NOT pre-mark it filled.
    }

    fn root_path(&self) -> Option<gtk4::TreePath> {
        self.store.iter_first().map(|i| self.store.path(&i))
    }

    fn row_path(&self, iter: &TreeIter) -> Option<String> {
        self.store.get_value(iter, COL_PATH_I).get::<String>().ok()
    }

    fn fill_children(&self, iter: &TreeIter, path: &str) {
        if self.filled.borrow().contains(path) {
            return;
        }
        self.filled.borrow_mut().insert(path.to_string());
        if std::env::var_os("CR_TRACE").is_some() {
            eprintln!("[trace] folders: fill {path}");
        }
        // Swap the lazy-tree dummies out.
        while let Some(dummy) = self.store.iter_children(Some(iter)) {
            let is_dummy = self
                .store
                .get_value(&dummy, COL_NAME_I)
                .get::<String>()
                .is_ok_and(|n| n.is_empty());
            if !is_dummy {
                break;
            }
            self.store.remove(&dummy);
        }
        let dir = Path::new(path);
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        let mut names: Vec<String> = rd
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            // Dot-dirs stay out (the Linux convention; the C# shows
            // the shell's hidden folders — recorded deviation).
            .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort_by(|a, b| cr_io::extended_compare::extended_compare_ignore_case(a, b));
        for name in names {
            let full = Path::new(path).join(&name);
            let child = self.store.append(Some(iter));
            let full_str = full.to_string_lossy().to_string();
            let values: Vec<(u32, &dyn gtk4::glib::prelude::ToValue)> =
                vec![(COL_NAME, &name), (COL_PATH, &full_str)];
            self.store.set(&child, &values);
            let dummy = self.store.append(Some(&child));
            self.store.set(&dummy, &[(COL_NAME, &""), (COL_PATH, &"")]);
        }
    }

    fn child_iter(&self, parent: &TreeIter, name: &str) -> Option<TreeIter> {
        let mut child = self.store.iter_children(Some(parent));
        while let Some(c) = child {
            let n = self.store.get_value(&c, COL_NAME_I).get::<String>().ok()?;
            if n == name {
                return Some(c);
            }
            let mut next = c;
            child = self.store.iter_next(&mut next).then_some(next);
        }
        None
    }
}

fn wire(tree: &Rc<FolderTree>) {
    // Lazy fill on expand (the C# `tvFolders_BeforeExpand`).
    {
        let t = Rc::downgrade(tree);
        tree.view.connect_row_expanded(move |_view, iter, _path| {
            let Some(t) = t.upgrade() else {
                return;
            };
            if let Some(path) = t.row_path(iter) {
                if std::env::var_os("CR_TRACE").is_some() {
                    eprintln!("[trace] folders: expand {path}");
                }
                t.fill_children(iter, &path);
            }
        });
    }
    // Selection → the host callback (`tvFolders_AfterSelect` →
    // `FillBooks(CurrentFolder)`).
    {
        let t = Rc::downgrade(tree);
        tree.selection.connect_changed(move |_| {
            let Some(t) = t.upgrade() else {
                return;
            };
            if let Some(path) = t.current_folder() {
                if std::env::var_os("CR_TRACE").is_some() {
                    eprintln!("[trace] folders: selected {path}");
                }
                if let Some(f) = t.on_selected.borrow().as_ref() {
                    f(&path);
                }
            }
        });
    }
    // The toolbar buttons.
    {
        let t = Rc::downgrade(tree);
        for (name, button) in &tree.buttons {
            let name = *name;
            let t = t.clone();
            button.connect_clicked(move |_| {
                let Some(tree) = t.upgrade() else {
                    return;
                };
                if name == "refresh" {
                    tree.refresh();
                }
            });
        }
    }
    // The Include Sub Folders toggle (`SwitchIncludeSubFolders`).
    {
        let t = Rc::downgrade(tree);
        let btn = tree.include_btn.clone();
        btn.connect_toggled(move |btn| {
            let Some(tree) = t.upgrade() else {
                return;
            };
            let f = tree.on_include_sub.borrow_mut().take();
            if let Some(f) = f {
                f(btn.is_active());
                *tree.on_include_sub.borrow_mut() = Some(f);
            }
        });
    }
    // The favorites dropdown rebuilds on open (the dynamic fill
    // lesson: revisit-within-an-open-menu re-fills on its own map).
    {
        let t = Rc::downgrade(tree);
        tree.favorites_pop.connect_map({
            let pop = tree.favorites_pop.clone();
            move |_| {
                let Some(tree) = t.upgrade() else {
                    return;
                };
                rebuild_favorites(&tree, &pop);
            }
        });
    }
}

/// The favorites rows (`FillFavorites`): one row per saved folder,
/// click drills to it.
fn rebuild_favorites(tree: &Rc<FolderTree>, pop: &Popover) {
    let list = library::settings().borrow().favorite_folders.clone();
    let box_ = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    box_.set_margin_top(4);
    box_.set_margin_bottom(4);
    box_.set_margin_start(4);
    box_.set_margin_end(4);
    let t = Rc::downgrade(tree);
    for path in &list {
        let label = Path::new(path)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone());
        let b = gtk4::Button::new();
        b.add_css_class("flat");
        b.set_label(&label);
        b.set_tooltip_text(Some(path));
        let t = t.clone();
        let p = path.clone();
        let pop = pop.clone();
        b.connect_clicked(move |_| {
            let _ = &pop;
            if let Some(tree) = t.upgrade() {
                tree.drill_to(&p);
            }
            pop.popdown();
        });
        box_.append(&b);
    }
    if list.is_empty() {
        let l = gtk4::Label::new(Some("No favorite folders yet"));
        l.add_css_class("dim-label");
        l.set_margin_start(8);
        l.set_margin_end(8);
        box_.append(&l);
    }
    pop.set_child(Some(&box_));
}

/// `AddToFavorites`: the current folder joins the saved list
/// (persisted through the settings file).
pub fn add_favorite(tree: &FolderTree) -> bool {
    let Some(current) = tree.current_folder() else {
        return false;
    };
    let settings = library::settings();
    {
        let mut s = settings.borrow_mut();
        if s.favorite_folders.contains(&current) {
            return false;
        }
        s.favorite_folders.push(current);
    }
    library::save_settings();
    true
}

/// The include-sub-folders write-back helper (the shell owns the
/// Settings value + the rescan).
pub fn set_include_sub_setting(active: bool) {
    library::settings()
        .borrow_mut()
        .explorer_include_sub_folders = active;
}

/// The include-sub-folders probe read.
pub fn include_sub_active(tree: &FolderTree) -> bool {
    tree.include_btn.is_active()
}

/// The include-sub-folders click-through (the probe).
pub fn click_include_sub(tree: &FolderTree) {
    tree.include_btn.emit_clicked();
}
