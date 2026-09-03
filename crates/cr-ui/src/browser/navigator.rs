//! The list navigator pane — the ComicLists tree
//! (`ComicListLibraryBrowser.tvQueries`): Library root, folders, and
//! smart/id lists with kind icons; selection evaluates the list
//! (debounced, the C# `updateTimer`); a context menu creates, renames,
//! and removes lists and folders.
//!
//! Ported behavior: node per `ComicListItem` in document order
//! (`FillListTree`), expansion and selection kept across refills by
//! item id, right-click selects the row under the cursor
//! (`tvQueries_MouseDown`), Library renames but never removes
//! (`RemoveListOrFolder` guard).
//!
//! Custom per-item thumbnails (`LibraryTreeSkin`) are Phase 5 polish;
//! the icons come from the GTK theme for now.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    Align, Button, GestureClick, Popover, ScrolledWindow, TreeIter, TreeSelection, TreeStore,
    TreeView, TreeViewColumn,
};

use cr_core::database::list_items::ComicListItem;
use cr_core::xml::scalar::CrGuid;

/// Selection-change debounce (`updateTimer`; large sets re-evaluate).
const SELECT_DEBOUNCE_MS: u64 = 200;

/// The tree columns.
const COL_NAME: u32 = 0;
const COL_ICON: u32 = 1;
const COL_ID: u32 = 2;
const COL_NAME_I: i32 = COL_NAME as i32;
const COL_ID_I: i32 = COL_ID as i32;

/// The context-menu commands the host implements
/// (`treeContextMenu`, the common subset).
#[derive(Clone, Copy, Debug)]
pub enum ListCommand {
    NewSmartList,
    NewFolder,
    Rename,
    Delete,
}

type SelectedFn = Box<dyn Fn(&CrGuid, &str)>;
type CommandFn = Box<dyn Fn(ListCommand, Option<CrGuid>)>;

pub struct Navigator {
    widget: ScrolledWindow,
    store: TreeStore,
    view: TreeView,
    selection: TreeSelection,
    expanded: RefCell<HashSet<CrGuid>>,
    on_selected: RefCell<Option<SelectedFn>>,
    on_command: RefCell<Option<CommandFn>>,
}

impl Navigator {
    pub fn new() -> Rc<Navigator> {
        let store = TreeStore::new(&[
            String::static_type(),
            String::static_type(),
            String::static_type(),
        ]);
        let view = TreeView::with_model(&store);
        view.set_headers_visible(false);
        view.append_column(&Self::icon_column());
        view.append_column(&Self::name_column());
        let selection = view.selection();
        selection.set_mode(gtk4::SelectionMode::Single);

        let widget = ScrolledWindow::builder()
            .child(&view)
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vexpand(true)
            .build();

        let nav = Rc::new(Navigator {
            widget,
            store,
            view,
            selection,
            expanded: RefCell::new(HashSet::new()),
            on_selected: RefCell::new(None),
            on_command: RefCell::new(None),
        });
        wire_signals(&nav);
        nav
    }

    pub fn widget(&self) -> &ScrolledWindow {
        &self.widget
    }

    /// The selection-change callback: (item id, item name). Debounced.
    pub fn connect_selected<F: Fn(&CrGuid, &str) + 'static>(&self, f: F) {
        *self.on_selected.borrow_mut() = Some(Box::new(f));
    }

    /// The context-menu callback: the command and the row it targets.
    pub fn connect_command<F: Fn(ListCommand, Option<CrGuid>) + 'static>(&self, f: F) {
        *self.on_command.borrow_mut() = Some(Box::new(f));
    }

    fn wire(self: &Rc<Self>) {
        // Expansion tracking (kept across refills by item id).
        {
            let nav = Rc::downgrade(self);
            self.view.connect_row_expanded(move |_, iter, _| {
                if let Some(n) = nav.clone().upgrade() {
                    n.track_expanded(iter, true);
                }
            });
            let nav = Rc::downgrade(self);
            self.view.connect_row_collapsed(move |_, iter, _| {
                if let Some(n) = nav.upgrade() {
                    n.track_expanded(iter, false);
                }
            });
        }
        // Selection → debounced evaluation (the C# `updateTimer`).
        {
            let nav = Rc::downgrade(self);
            self.selection.connect_changed(move |_| {
                let Some(nav) = nav.upgrade() else {
                    return;
                };
                nav.schedule_select();
            });
        }
        // Right-click: select the row under the cursor and open the
        // context menu (`tvQueries_MouseDown`).
        {
            let nav = Rc::downgrade(self);
            let gesture = GestureClick::new();
            gesture.set_button(3);
            gesture.connect_pressed(move |gesture, _n, x, y| {
                let Some(nav) = nav.upgrade() else {
                    return;
                };
                gesture.set_state(gtk4::EventSequenceState::Claimed);
                if let Some((Some(path), _col, cx, cy)) = nav.view.path_at_pos(x as i32, y as i32) {
                    nav.selection.select_path(&path);
                    nav.open_menu(cx as f64, cy as f64);
                }
            });
            self.view.add_controller(gesture);
        }
    }

    fn track_expanded(&self, iter: &TreeIter, expanded: bool) {
        if let Some(id) = self.row_id(iter) {
            if expanded {
                self.expanded.borrow_mut().insert(id);
            } else {
                self.expanded.borrow_mut().remove(&id);
            }
        }
    }

    fn row_id(&self, iter: &TreeIter) -> Option<CrGuid> {
        let text = self.store.get_value(iter, COL_ID_I).get::<String>().ok()?;
        CrGuid::parse(&text).ok()
    }

    fn schedule_select(self: &Rc<Self>) {
        let nav = Rc::downgrade(self);
        glib::timeout_add_local(
            std::time::Duration::from_millis(SELECT_DEBOUNCE_MS),
            move || {
                let Some(nav) = nav.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                nav.fire_selected();
                glib::ControlFlow::Break
            },
        );
    }

    fn fire_selected(&self) {
        let callbacks = self.on_selected.borrow();
        let Some(f) = callbacks.as_ref() else {
            return;
        };
        if let Some((id, name)) = self.current_selection() {
            f(&id, &name);
        }
    }

    /// The currently selected (id, name) — read from the view, so it
    /// is always current.
    pub fn current_selection(&self) -> Option<(CrGuid, String)> {
        let (_model, iter) = self.selection.selected()?;
        let id = self.row_id(&iter)?;
        let name = self
            .store
            .get_value(&iter, COL_NAME_I)
            .get::<String>()
            .ok()?;
        Some((id, name))
    }

    /// `FillListTree`: rebuilds the tree from the ComicLists model,
    /// preserving expansion and selection by item id.
    pub fn refill(&self, items: &[ComicListItem]) {
        let previous = self.current_selection().map(|(id, _)| id);
        let expanded = self.expanded.borrow().clone();
        self.store.clear();
        self.fill_items(None, items);
        self.apply_expansion(None, &expanded);
        let target = previous.or_else(|| items.first().map(|i| i.base().id));
        if let Some(id) = target {
            self.select_by_id(&id);
        }
        let _ = &previous;
    }

    fn fill_items(&self, parent: Option<&TreeIter>, items: &[ComicListItem]) {
        for item in items {
            let iter = self.store.append(parent);
            let name = item.base().name.clone().unwrap_or_default();
            let id = item.base().id.to_d_string();
            let icon = Self::icon_for(item);
            self.store.set(
                &iter,
                &[(COL_NAME, &name), (COL_ICON, &icon), (COL_ID, &id)],
            );
            if let ComicListItem::Folder(folder) = item {
                self.fill_items(Some(&iter), &folder.items);
            }
        }
    }

    fn apply_expansion(&self, parent: Option<&TreeIter>, expanded: &HashSet<CrGuid>) {
        let next = match parent {
            Some(p) => self.store.iter_children(Some(p)),
            None => self.store.iter_first(),
        };
        let Some(mut iter) = next else {
            return;
        };
        loop {
            if let Some(id) = self.row_id(&iter) {
                if expanded.contains(&id) {
                    let path = self.store.path(&iter);
                    self.view.expand_row(&path, false);
                }
            }
            self.apply_expansion(Some(&iter), expanded);
            if !self.store.iter_next(&mut iter) {
                break;
            }
        }
    }

    fn select_by_id(&self, id: &CrGuid) {
        let text = id.to_d_string();
        let first = self.store.iter_first();
        if let Some(iter) = first.and_then(|f| self.find_iter(Some(&f), &text)) {
            let path = self.store.path(&iter);
            self.view.expand_to_path(&path);
            self.selection.select_path(&path);
        }
    }

    /// Depth-first row search by the id column text.
    fn find_iter(&self, start: Option<&TreeIter>, id_text: &str) -> Option<TreeIter> {
        let mut iter = *start?;
        loop {
            let row_id = self.store.get_value(&iter, COL_ID_I).get::<String>().ok();
            if row_id.as_deref() == Some(id_text) {
                return Some(iter);
            }
            if let Some(child) = self.store.iter_children(Some(&iter)) {
                if let Some(found) = self.find_iter(Some(&child), id_text) {
                    return Some(found);
                }
            }
            if !self.store.iter_next(&mut iter) {
                return None;
            }
        }
    }

    /// Moves the selection one row down (the Down-arrow behavior;
    /// the view handles keys itself when focused).
    pub fn select_next(&self) {
        let Some((_model, iter)) = self.selection.selected() else {
            return;
        };
        let mut path = self.store.path(&iter);
        path.next();
        if let Some(next) = self.store.iter(&path) {
            self.selection.select_iter(&next);
        }
    }

    /// Selects the first row named `name` (the C#
    /// `FindItemNode(LastLibraryItem)` restore path; ancestors expand
    /// like the WinForms `SelectedNode` setter). Returns whether a row
    /// matched.
    pub fn select_by_name(&self, name: &str) -> bool {
        let Some(first) = self.store.iter_first() else {
            return false;
        };
        let Some(iter) = self.find_by_name(Some(&first), name) else {
            return false;
        };
        let path = self.store.path(&iter);
        self.view.expand_to_path(&path);
        self.selection.select_path(&path);
        true
    }

    fn find_by_name(&self, start: Option<&TreeIter>, name: &str) -> Option<TreeIter> {
        let mut iter = *start?;
        loop {
            let row_name = self.store.get_value(&iter, COL_NAME_I).get::<String>().ok();
            if row_name.as_deref() == Some(name) {
                return Some(iter);
            }
            if let Some(child) = self.store.iter_children(Some(&iter)) {
                if let Some(found) = self.find_by_name(Some(&child), name) {
                    return Some(found);
                }
            }
            if !self.store.iter_next(&mut iter) {
                return None;
            }
        }
    }

    /// The context menu (`treeContextMenu`, the common commands only).
    fn open_menu(self: &Rc<Self>, x: f64, y: f64) {
        let target = self.current_selection().map(|(id, _)| id);
        let popover = Popover::new();
        let box_ = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        box_.set_margin_top(4);
        box_.set_margin_bottom(4);
        box_.set_margin_start(4);
        box_.set_margin_end(4);

        let nav = Rc::downgrade(self);
        let add_item = |box_: &gtk4::Box, label: &str, command: ListCommand| {
            let nav = nav.clone();
            let popover = popover.clone();
            let button = Button::with_label(label);
            button.set_has_frame(false);
            button.set_halign(Align::Fill);
            button.connect_clicked(move |_| {
                popover.popdown();
                if let Some(n) = nav.upgrade() {
                    if let Some(f) = n.on_command.borrow().as_ref() {
                        f(command, target);
                    }
                }
            });
            box_.append(&button);
        };
        add_item(&box_, "New Smart List…", ListCommand::NewSmartList);
        add_item(&box_, "New Folder…", ListCommand::NewFolder);
        add_item(&box_, "Rename…", ListCommand::Rename);
        add_item(&box_, "Delete", ListCommand::Delete);
        popover.set_child(Some(&box_));
        popover.set_parent(self.widget());
        popover.connect_closed(|p| p.unparent());
        let rect = gtk4::gdk::Rectangle::new(x as i32, y as i32 + 8, 1, 1);
        popover.set_pointing_to(Some(&rect));
        popover.popup();
    }

    fn icon_column() -> TreeViewColumn {
        let cell = gtk4::CellRendererPixbuf::new();
        let col = TreeViewColumn::new();
        col.pack_start(&cell, true);
        col.add_attribute(&cell, "icon-name", COL_NAME_I + 1);
        col
    }

    fn name_column() -> TreeViewColumn {
        let cell = gtk4::CellRendererText::new();
        let col = TreeViewColumn::new();
        col.pack_start(&cell, true);
        col.add_attribute(&cell, "text", COL_NAME_I);
        col
    }

    fn icon_for(item: &ComicListItem) -> &'static str {
        match item {
            ComicListItem::Library(_) => "view-list-symbolic",
            ComicListItem::Folder(_) => "folder-symbolic",
            ComicListItem::Smart(_) => "edit-find-symbolic",
            ComicListItem::IdList(_) => "text-x-generic-symbolic",
        }
    }
}

fn wire_signals(nav: &Rc<Navigator>) {
    nav.wire();
}
