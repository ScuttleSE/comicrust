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
//! Drag and drop moves one list or folder inside the tree
//! (`tvQueries_ItemDrag` + `tvQueries_DragDrop`): a drop ON a folder
//! appends the item to it, a drop on any other row — or in the top
//! 4 px of a folder row — puts the item BEFORE that row, and a drop on
//! empty space moves it to the root end. The Library root never drags,
//! and a folder refuses a drop into its own subtree. Absent against
//! the C#: Ctrl-drag copy, the `.cbl` file and book drop branches, the
//! `treeSkin` drop highlight, and drag auto-scroll.
//!
//! The "Sort" command (`SortList`, `miNodeSort`) sorts ONE folder's
//! items: folders first, then the rest by name.
//!
//! The T7 toolbar (the control's own `toolStrip`, Designer:320-331):
//! New Folder / New List / New Smart List (the SAME commands as the
//! context menu, acting on the selection), Expand/Collapse All
//! (any-expanded → collapse, else expand — `ExpandCollapseAllNodes`),
//! Refresh, and the right-aligned Quick Search toggle that shows the
//! navigator's own search box (`tsQuickSearch` + `ToggleQuickSearch`,
//! Ctrl+Alt+F). Absent per scope: Open in New Window (ADR-024),
//! Open in New Tab (no list-tab surface), the Favorites pane.
//!
//! Custom per-item thumbnails (`LibraryTreeSkin`) are Phase 5 polish;
//! the kind icons come from the bundled ComicRack set (`icon.rs`) —
//! the C# `treeImages` table (`ComicListLibraryBrowser.cs:313-317`)
//! with the `ComicListItem.ImageKey` keys.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use gtk4::gdk;
use gtk4::glib;
use gtk4::prelude::*;
use gtk4::{
    Button, Entry, GestureClick, Popover, ScrolledWindow, TreeIter, TreeSelection, TreeStore,
    TreeView, TreeViewColumn,
};

use cr_core::database::list_items::ComicListItem;
use cr_core::xml::scalar::CrGuid;

use crate::icon;

/// Selection-change debounce (`updateTimer`; large sets re-evaluate).
const SELECT_DEBOUNCE_MS: u64 = 200;

/// The height of a row's top strip that makes a drop insert BEFORE the
/// row instead of into it (`SetDropEffects`: `point2.Y < 4`).
const SEPARATOR_EDGE_PX: i32 = 4;

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
    NewList,
    Edit,
    NewFolder,
    Rename,
    Delete,
    Import,
    /// `SortList` (`miNodeSort`, "&Sort"): sorts the selected FOLDER's
    /// items — folders first, then the rest by name. Folder rows only,
    /// which is the C# enable rule.
    Sort,
    /// "Scan List Contents" (ADR-036, a port addition): scan the
    /// distinct linked file paths of the selected list's books. Smart
    /// lists and reading lists only.
    ScanList,
    /// "Reset View Settings" (ADR-039, a port addition): clears the
    /// list's own `<Display><View>` so it inherits again. Shown only
    /// for a list that HAS settings of its own.
    ResetViewSettings,
}

type SelectedFn = Box<dyn Fn(&CrGuid, &str)>;
type CommandFn = Box<dyn Fn(ListCommand, Option<CrGuid>)>;
type RefreshFn = Box<dyn Fn()>;

pub struct Navigator {
    /// The mounted widget: [toolbar][search box (hidden)][tree].
    widget: gtk4::Box,
    store: TreeStore,
    view: TreeView,
    selection: TreeSelection,
    expanded: RefCell<HashSet<CrGuid>>,
    on_selected: RefCell<Option<SelectedFn>>,
    on_command: RefCell<Option<CommandFn>>,
    on_refresh: RefCell<Option<RefreshFn>>,
    /// The navigator's own search box (`quickSearchPanel`; hidden
    /// until `tsQuickSearch` toggles it).
    search_box: gtk4::Box,
    search_entry: Entry,
    search_visible: std::cell::Cell<bool>,
    /// The last context menu (the probe's arrow/position gate).
    last_menu: RefCell<Option<Popover>>,
    /// (name, button) — the probe's real-click path.
    buttons: Vec<(&'static str, Button)>,
}

impl Navigator {
    pub fn new() -> Rc<Navigator> {
        let store = TreeStore::new(&[
            String::static_type(),
            gdk::Texture::static_type(),
            String::static_type(),
        ]);
        let view = TreeView::with_model(&store);
        view.set_headers_visible(false);
        view.append_column(&Self::icon_column());
        view.append_column(&Self::name_column());
        let selection = view.selection();
        selection.set_mode(gtk4::SelectionMode::Single);

        let scroller = ScrolledWindow::builder()
            .child(&view)
            .hscrollbar_policy(gtk4::PolicyType::Never)
            .vexpand(true)
            .build();

        // The control's own toolbar (`ComicListLibraryBrowser.
        // toolStrip`): the three New buttons, Expand/Collapse All,
        // Refresh; the Quick Search toggle right-aligned. Click
        // handlers wire in `wire()` (the Weak needs the built Rc).
        let toolbar = gtk4::Box::new(gtk4::Orientation::Horizontal, 2);
        toolbar.add_css_class("toolbar");
        let mut buttons: Vec<(&'static str, Button)> = Vec::new();
        let mk = |bar: &gtk4::Box,
                  buttons: &mut Vec<(&'static str, Button)>,
                  name: &'static str,
                  icon: &'static str,
                  tooltip: &str| {
            let (button, _) = Self::tool_button(icon, tooltip);
            bar.append(&button);
            buttons.push((name, button));
        };
        mk(
            &toolbar,
            &mut buttons,
            "new-folder",
            "NewSearchFolder",
            "Create a new folder to organize your lists",
        );
        mk(
            &toolbar,
            &mut buttons,
            "new-list",
            "NewList",
            "Create a new custom List",
        );
        mk(
            &toolbar,
            &mut buttons,
            "new-smart-list",
            "NewSearchDocument",
            "Create a new Smart List",
        );
        let sep1 = gtk4::Separator::new(gtk4::Orientation::Horizontal);
        sep1.set_margin_top(4);
        sep1.set_margin_bottom(4);
        toolbar.append(&sep1);
        mk(
            &toolbar,
            &mut buttons,
            "expand-collapse-all",
            "ExpandCollapseAll",
            "Expand/Collapse all",
        );
        mk(&toolbar, &mut buttons, "refresh", "Refresh", "Refresh");
        let spacer = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        spacer.set_hexpand(true);
        toolbar.append(&spacer);
        mk(
            &toolbar,
            &mut buttons,
            "quick-search",
            "Search",
            "Quick Search (Ctrl+Alt+F)",
        );

        // The search box (`quickSearchPanel`): hidden until the
        // Quick Search button toggles it.
        let search_box = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
        let search_entry = Entry::new();
        // The C# cue text IS the toggle button's text
        // (`quickSearch.SetCueText(tsQuickSearch.Text)`).
        search_entry.set_placeholder_text(Some("Quick Search (Ctrl+Alt+F)"));
        search_entry.set_margin_start(4);
        search_entry.set_margin_end(4);
        search_entry.set_margin_top(2);
        search_entry.set_margin_bottom(2);
        search_box.append(&search_entry);
        search_box.set_visible(false);

        let widget = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        widget.append(&toolbar);
        widget.append(&search_box);
        widget.append(&scroller);

        let nav = Rc::new(Navigator {
            widget,
            store,
            view,
            selection,
            expanded: RefCell::new(HashSet::new()),
            on_selected: RefCell::new(None),
            on_command: RefCell::new(None),
            on_refresh: RefCell::new(None),
            search_box,
            search_entry,
            search_visible: std::cell::Cell::new(false),
            last_menu: RefCell::new(None),
            buttons,
        });
        wire_signals(&nav);
        nav
    }

    pub fn widget(&self) -> &gtk4::Box {
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

    /// The Refresh button callback (the shell refills the tree and
    /// re-evaluates the current list).
    pub fn connect_refresh<F: Fn() + 'static>(&self, f: F) {
        *self.on_refresh.borrow_mut() = Some(Box::new(f));
    }

    /// Fires one context-menu command with the current selection as
    /// the target (the toolbar buttons and the context menu share
    /// the host callback).
    fn fire_command(&self, command: ListCommand) {
        if let Some(f) = self.on_command.borrow().as_ref() {
            let target = self.current_selection().map(|(id, _)| id);
            f(command, target);
        }
    }

    fn wire(self: &Rc<Self>) {
        // The toolbar clicks (the buttons built in `new`; the
        // commands resolve here where the Weak works).
        {
            let nav = Rc::downgrade(self);
            for (name, button) in &self.buttons {
                let name = *name;
                let nav = nav.clone();
                button.connect_clicked(move |_| {
                    let Some(n) = nav.upgrade() else {
                        return;
                    };
                    match name {
                        "new-folder" => n.fire_command(ListCommand::NewFolder),
                        "new-list" => n.fire_command(ListCommand::NewList),
                        "new-smart-list" => n.fire_command(ListCommand::NewSmartList),
                        "expand-collapse-all" => n.expand_collapse_all(),
                        "refresh" => {
                            if let Some(f) = n.on_refresh.borrow().as_ref() {
                                f();
                            }
                        }
                        "quick-search" => n.toggle_search(),
                        _ => {}
                    }
                });
            }
        }
        // The search text filters the tree (`quickSearch_TextChanged`
        // → `FillListTree`).
        {
            let nav = Rc::downgrade(self);
            self.search_entry.connect_changed(move |_| {
                let Some(n) = nav.upgrade() else {
                    return;
                };
                if n.search_visible.get() {
                    n.refill(&crate::library::comic_lists_snapshot());
                }
            });
        }
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
                nav.context_menu_at(x, y);
            });
            self.view.add_controller(gesture);
        }
        // Drag and drop inside the tree (`tvQueries_ItemDrag` +
        // `tvQueries_DragDrop`): a list or a folder moves into a
        // folder, lands before a row, or goes to the root end. The
        // payload is the dragged item id.
        //
        // The source runs in the CAPTURE phase: the TreeView claims
        // the pointer sequence for its own selection handling in the
        // bubble phase, and a source behind it never reaches the drag
        // threshold. A click that does not pass the threshold is not
        // claimed, so row selection still works.
        {
            let nav = Rc::downgrade(self);
            let source = gtk4::DragSource::new();
            source.set_actions(gdk::DragAction::MOVE);
            source.set_propagation_phase(gtk4::PropagationPhase::Capture);
            source.connect_prepare(move |_, x, y| {
                let nav = nav.upgrade()?;
                let id = nav.row_id_at(x, y)?;
                // The C# clears the drag node for the Library root.
                if matches!(
                    crate::library::find_list_item_any(&id),
                    Some(ComicListItem::Library(_))
                ) {
                    return None;
                }
                Some(gdk::ContentProvider::for_value(
                    &id.to_d_string().to_value(),
                ))
            });
            self.view.add_controller(source);
        }
        {
            let nav = Rc::downgrade(self);
            let target = gtk4::DropTarget::new(String::static_type(), gdk::DragAction::MOVE);
            target.connect_drop(move |_, value, x, y| {
                let Some(nav) = nav.upgrade() else {
                    return false;
                };
                let Ok(text) = value.get::<String>() else {
                    return false;
                };
                let Ok(id) = CrGuid::parse(&text) else {
                    return false;
                };
                nav.apply_drop(&id, x, y)
            });
            self.view.add_controller(target);
        }
    }

    /// The item id of the row under a widget-relative point.
    fn row_id_at(&self, x: f64, y: f64) -> Option<CrGuid> {
        let (bx, by) = self
            .view
            .convert_widget_to_bin_window_coords(x as i32, y as i32);
        let (Some(path), ..) = self.view.path_at_pos(bx, by)? else {
            return None;
        };
        let iter = self.store.iter(&path)?;
        self.row_id(&iter)
    }

    /// Where a drop at this point lands (`SetDropEffects`,
    /// `ComicListLibraryBrowser.cs:979`): no row under the pointer is
    /// the root end; the top 4 px of a row, and every row that is not
    /// a folder, insert BEFORE that row; a folder row takes the item
    /// as its last child.
    pub fn drop_target_at(&self, x: f64, y: f64) -> crate::library::ListDrop {
        use crate::library::ListDrop;
        let (bx, by) = self
            .view
            .convert_widget_to_bin_window_coords(x as i32, y as i32);
        let Some((Some(path), ..)) = self.view.path_at_pos(bx, by) else {
            return ListDrop::RootEnd;
        };
        let Some(iter) = self.store.iter(&path) else {
            return ListDrop::RootEnd;
        };
        let Some(id) = self.row_id(&iter) else {
            return ListDrop::RootEnd;
        };
        let is_folder = matches!(
            crate::library::find_list_item_any(&id),
            Some(ComicListItem::Folder(_))
        );
        let area = self.view.cell_area(Some(&path), None::<&TreeViewColumn>);
        let offset = by - area.y();
        let on_top_edge = (0..SEPARATOR_EDGE_PX).contains(&offset);
        if on_top_edge || !is_folder {
            ListDrop::BeforeItem(id)
        } else {
            ListDrop::IntoFolder(id)
        }
    }

    /// The shared drop body: move the item in the ComicLists model,
    /// then refill and re-select it (the C# `FindItemNode` re-select,
    /// :1088). Returns whether the tree changed. An impossible move —
    /// the item onto itself or into its own subtree — returns false
    /// and leaves the tree alone.
    fn apply_drop(&self, src: &CrGuid, x: f64, y: f64) -> bool {
        let drop = self.drop_target_at(x, y);
        if !crate::library::move_list_item(src, &drop) {
            return false;
        }
        self.refill(&crate::library::comic_lists_snapshot());
        self.select_list(src);
        true
    }

    /// The probe's drop path (the same body the DropTarget fires).
    pub fn probe_drop(&self, src: &CrGuid, x: f64, y: f64) -> bool {
        self.apply_drop(src, x, y)
    }

    /// The probe's drop geometry: the widget-relative centre of a
    /// row, and a point 1 px inside its top edge (the separator
    /// strip). `None` when the row is absent or has no allocation.
    pub fn probe_row_points(&self, id: &CrGuid) -> Option<((f64, f64), (f64, f64))> {
        let text = id.to_d_string();
        let first = self.store.iter_first()?;
        let iter = self.find_iter(Some(&first), &text)?;
        let path = self.store.path(&iter);
        let area = self.view.cell_area(Some(&path), None::<&TreeViewColumn>);
        if area.height() == 0 {
            return None;
        }
        let centre = self
            .view
            .convert_bin_window_to_widget_coords(20, area.y() + area.height() / 2);
        let top = self
            .view
            .convert_bin_window_to_widget_coords(20, area.y() + 1);
        Some((
            (centre.0 as f64, centre.1 as f64),
            (top.0 as f64, top.1 as f64),
        ))
    }

    /// The right-click path: select the row under the cursor and
    /// open the menu AT THE CURSOR (the probe drives the same body;
    /// no row under the cursor keeps the prior no-menu gate).
    fn context_menu_at(self: &Rc<Self>, x: f64, y: f64) {
        if let Some((Some(path), ..)) = self.view.path_at_pos(x as i32, y as i32) {
            self.selection.select_path(&path);
            self.open_menu(x, y);
        }
    }

    fn track_expanded(&self, iter: &TreeIter, expanded: bool) {
        let Some(id) = self.row_id(iter) else {
            return;
        };
        crate::library::set_folder_collapsed(&id, !expanded);
        if expanded {
            self.expanded.borrow_mut().insert(id);
        } else {
            self.expanded.borrow_mut().remove(&id);
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
    /// preserving expansion and selection by item id. The active
    /// quick-search text filters the items (`FillListTree(filter)`).
    pub fn refill(&self, items: &[ComicListItem]) {
        let previous = self.current_selection().map(|(id, _)| id);
        // The persisted folder state drives the expansion (`FillListTree`:
        // a folder expands while NOT `Collapsed`, the Library root always
        // expands). The user expand/collapse writes the field back
        // (`tvQueries_AfterExpand`/`AfterCollapse`).
        let mut expanded = HashSet::new();
        collect_expanded(items, &mut expanded);
        let filter = self.search_entry.text().to_string();
        let items = filter_items(items, filter.trim());
        self.store.clear();
        self.fill_items(None, &items);
        self.apply_expansion(None, &expanded);
        let target = previous.or_else(|| items.first().map(|i| i.base().id));
        if let Some(id) = target {
            self.select_by_id(&id);
        }
        let _ = &previous;
    }

    /// `ExpandCollapseAllNodes`: any row expanded → collapse all,
    /// else expand all. The row-expanded/collapsed signals keep the
    /// id set in step.
    fn expand_collapse_all(&self) {
        if !self.expanded.borrow().is_empty() {
            self.view.collapse_all();
            self.expanded.borrow_mut().clear();
        } else {
            self.view.expand_all();
        }
    }

    /// `ToggleQuickSearch`: show + focus the box, or clear + hide.
    pub fn toggle_search(&self) {
        if !self.search_visible.get() {
            self.search_visible.set(true);
            self.search_box.set_visible(true);
            self.search_entry.grab_focus();
        } else {
            self.search_entry.set_text("");
            self.search_visible.set(false);
            self.search_box.set_visible(false);
        }
    }

    /// Whether the search box shows (the `tsQuickSearch` check
    /// state — the shell sync reads it).
    pub fn search_visible(&self) -> bool {
        self.search_visible.get()
    }

    /// The probe's real button click (walks the same handler the
    /// user's click fires).
    pub fn click_button(&self, name: &str) -> bool {
        let hit = self
            .buttons
            .iter()
            .find(|(n, _)| *n == name)
            .map(|(_, b)| b.clone());
        match hit {
            Some(b) => {
                b.emit_clicked();
                true
            }
            None => false,
        }
    }

    /// The probe's search-text path (`set_text` fires the same
    /// changed signal typing does).
    pub fn set_search_text(&self, text: &str) {
        self.search_entry.set_text(text);
    }

    /// The tree row count (the probe's filter evidence).
    pub fn row_count(&self) -> usize {
        let mut count = 0;
        if let Some(first) = self.store.iter_first() {
            count += Self::count_rows(self, Some(&first));
        }
        count
    }

    fn count_rows(&self, iter: Option<&TreeIter>) -> usize {
        let mut iter = match iter {
            Some(i) => *i,
            None => return 0,
        };
        let mut count = 0;
        loop {
            count += 1;
            if let Some(child) = self.store.iter_children(Some(&iter)) {
                count += self.count_rows(Some(&child));
            }
            if !self.store.iter_next(&mut iter) {
                break;
            }
        }
        count
    }

    /// The expanded row count (the probe's expand/collapse evidence).
    pub fn expanded_count(&self) -> usize {
        let mut count = 0;
        if let Some(first) = self.store.iter_first() {
            count += self.count_expanded(Some(&first));
        }
        count
    }

    /// (name, expanded) per row in depth-first order (the probe's
    /// expand/collapse debugging seam).
    pub fn expanded_dump(&self) -> Vec<(String, bool)> {
        let mut out = Vec::new();
        if let Some(first) = self.store.iter_first() {
            self.dump_expanded(Some(&first), &mut out);
        }
        out
    }

    fn dump_expanded(&self, iter: Option<&TreeIter>, out: &mut Vec<(String, bool)>) {
        let mut iter = match iter {
            Some(i) => *i,
            None => return,
        };
        loop {
            let name = self
                .store
                .get_value(&iter, COL_NAME_I)
                .get::<String>()
                .unwrap_or_default();
            let path = self.store.path(&iter);
            out.push((name, self.view.row_expanded(&path)));
            if let Some(child) = self.store.iter_children(Some(&iter)) {
                self.dump_expanded(Some(&child), out);
            }
            if !self.store.iter_next(&mut iter) {
                break;
            }
        }
    }

    fn count_expanded(&self, iter: Option<&TreeIter>) -> usize {
        let mut iter = match iter {
            Some(i) => *i,
            None => return 0,
        };
        let mut count = 0;
        loop {
            let path = self.store.path(&iter);
            if self.view.row_expanded(&path) {
                count += 1;
            }
            if let Some(child) = self.store.iter_children(Some(&iter)) {
                count += self.count_expanded(Some(&child));
            }
            if !self.store.iter_next(&mut iter) {
                break;
            }
        }
        count
    }

    fn fill_items(&self, parent: Option<&TreeIter>, items: &[ComicListItem]) {
        for item in items {
            let iter = self.store.append(parent);
            let name = item.base().name.clone().unwrap_or_default();
            let id = item.base().id.to_d_string();
            let texture = icon::icon(Self::icon_for(item));
            let mut values: Vec<(u32, &dyn gtk4::glib::prelude::ToValue)> =
                vec![(COL_NAME, &name), (COL_ID, &id)];
            if let Some(tex) = texture.as_ref() {
                values.push((COL_ICON, tex));
            }
            self.store.set(&iter, &values);
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

    /// Selects a list row by item id (the shell's Previous/Next
    /// List history walk — the WinForms `SelectedNode` path:
    /// ancestors expand first).
    pub fn select_list(&self, id: &CrGuid) {
        self.select_by_id(id);
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
    /// It parents to the TREEVIEW and points at the click point: the
    /// coords are view-relative, so a Box parent (toolbar + search +
    /// tree) misread them and GTK fell back to the top edge (the
    /// Phase 8 T1 report). `ContextMenuStrip.Show(cursor)` parity.
    fn open_menu(self: &Rc<Self>, x: f64, y: f64) {
        let target = self.current_selection().map(|(id, _)| id);
        // "Scan List Contents" exists only for the list kinds (the
        // user's scope: smart lists and reading lists — ADR-036). The
        // Library root, folders, and a missing node never show it.
        let scanable = target
            .as_ref()
            .and_then(crate::library::find_list_item_any)
            .is_some_and(|item| matches!(item, ComicListItem::Smart(_) | ComicListItem::IdList(_)));
        // "Reset View Settings" shows only for a list that HAS its
        // own settings; an inheriting list has nothing to reset
        // (ADR-039).
        let has_own_view = target
            .as_ref()
            .is_some_and(|id| crate::library::list_view_config(id).is_some());
        // "Sort" is a folder-row command in the C#
        // (`commands.Add(SortList, () => ... is ComicListItemFolder)`,
        // ComicListLibraryBrowser.cs:338).
        let sortable = target
            .as_ref()
            .and_then(crate::library::find_list_item_any)
            .is_some_and(|item| matches!(item, ComicListItem::Folder(_)));
        crate::trace::trace(format!("nav menu target={target:?} scan-row={scanable}"));
        let popover = Popover::new();
        popover.set_has_arrow(false);
        let box_ = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        box_.set_margin_top(4);
        box_.set_margin_bottom(4);
        box_.set_margin_start(4);
        box_.set_margin_end(4);

        let nav = Rc::downgrade(self);
        let add_item = |box_: &gtk4::Box, label: &str, command: ListCommand| {
            let nav = nav.clone();
            let popover = popover.clone();
            let button = crate::widgets::menu_item_button(label);
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
        add_item(&box_, "New List…", ListCommand::NewList);
        // `miEditSmartList`: the C# routes smart lists, folders and
        // reading lists through their editors from this one item.
        add_item(&box_, "Edit…", ListCommand::Edit);
        if scanable {
            add_item(&box_, "Scan List Contents", ListCommand::ScanList);
        }
        add_item(&box_, "New Folder…", ListCommand::NewFolder);
        add_item(&box_, "Rename…", ListCommand::Rename);
        // The C# context menu order is Edit, Rename, Sort
        // (`treeContextMenu.Items`, Designer:142-144).
        if sortable {
            add_item(&box_, "Sort", ListCommand::Sort);
        }
        add_item(&box_, "Delete", ListCommand::Delete);
        // `miImportReadingList` (the C# menu sits between the
        // Export/Import pair and the Open commands).
        add_item(&box_, "Import Reading List…", ListCommand::Import);
        if has_own_view {
            add_item(&box_, "Reset View Settings", ListCommand::ResetViewSettings);
        }
        popover.set_child(Some(&box_));
        popover.set_parent(&self.view);
        popover.connect_closed(|p| p.unparent());
        let rect = gtk4::gdk::Rectangle::new(x as i32, y as i32 + 8, 1, 1);
        popover.set_pointing_to(Some(&rect));
        *self.last_menu.borrow_mut() = Some(popover.clone());
        popover.popup();
    }

    /// The probe's right-click path (the same body the gesture
    /// fires).
    pub fn probe_context_menu(self: &Rc<Self>, x: f64, y: f64) {
        self.context_menu_at(x, y);
    }

    /// The probe's menu-open path for the CURRENT selection (the
    /// keyboard-menu shape: no pointer row overrides the selection).
    /// The menu opens at the selected row's cell area.
    pub fn probe_context_menu_for_selection(self: &Rc<Self>) {
        let Some((id, _)) = self.current_selection() else {
            return;
        };
        let text = id.to_d_string();
        let first = self.store.iter_first();
        let Some(iter) = first.and_then(|f| self.find_iter(Some(&f), &text)) else {
            return;
        };
        let path = self.store.path(&iter);
        let rect = self.view.cell_area(Some(&path), None::<&TreeViewColumn>);
        self.open_menu(rect.x() as f64, rect.y() as f64);
    }

    /// The last context menu (the probe's arrow/position gate).
    pub fn last_menu_popover(&self) -> Option<Popover> {
        self.last_menu.borrow().clone()
    }

    /// The probe's row/grid evidence: the store count, whether the
    /// view is mapped/realized, and hits along a column.
    pub fn probe_grid(&self) -> String {
        let rows = self.row_count();
        let mapped = self.view.is_mapped();
        let realized = self.view.is_realized();
        let mut hits = String::new();
        for y in [4i32, 14, 24, 34, 44, 54, 64, 74, 100, 140] {
            let h = self.view.path_at_pos(20, y).is_some();
            hits.push_str(&format!("{y}:{h} "));
        }
        format!("rows={rows} mapped={mapped} realized={realized} hits[{hits}]")
    }

    fn icon_column() -> TreeViewColumn {
        let cell = gtk4::CellRendererPixbuf::new();
        let col = TreeViewColumn::new();
        col.pack_start(&cell, true);
        col.add_attribute(&cell, "texture", COL_NAME_I + 1);
        col
    }

    fn name_column() -> TreeViewColumn {
        let cell = gtk4::CellRendererText::new();
        let col = TreeViewColumn::new();
        col.pack_start(&cell, true);
        col.add_attribute(&cell, "text", COL_NAME_I);
        col
    }

    /// The resx icon name for an item — the C# `treeImages` table
    /// (`ComicListLibraryBrowser.cs:313-317`): the ImageKey "Folder"
    /// shows `Resources.SearchFolder`, "Search" shows
    /// `Resources.SearchDocument`; the rest are identity.
    fn icon_for(item: &ComicListItem) -> &'static str {
        match item {
            ComicListItem::Library(_) => "Library",
            // `ComicListItemFolder.ImageKey`: Temporary → "TempFolder".
            ComicListItem::Folder(f) if f.temporary => "TempFolder",
            ComicListItem::Folder(_) => "SearchFolder",
            ComicListItem::Smart(_) => "SearchDocument",
            ComicListItem::IdList(_) => "List",
        }
    }

    /// A 16 px flat icon button with the Designer tooltip.
    fn tool_button(icon: &'static str, tooltip: &str) -> (Button, gtk4::Image) {
        let button = Button::new();
        let image = gtk4::Image::new();
        image.set_pixel_size(16);
        if let Some(texture) = icon::icon(icon) {
            image.set_paintable(Some(&texture));
        }
        button.set_child(Some(&image));
        button.set_tooltip_text(Some(tooltip));
        button.add_css_class("flat");
        (button, image)
    }
}

fn wire_signals(nav: &Rc<Navigator>) {
    nav.wire();
}

/// The quick-search filter (`FillListTree` + `ComicListItem.Filter`):
/// Library rows always show; folders pass when ANY child passes (the
/// `ComicListItemFolder.Filter` override — the folder's own name is
/// NOT searched); every other item passes when its name contains the
/// filter (case-insensitive).
fn filter_items(items: &[ComicListItem], filter: &str) -> Vec<ComicListItem> {
    if filter.is_empty() {
        return items.to_vec();
    }
    let needle = filter.to_lowercase();
    items
        .iter()
        .filter(|item| item_matches(item, &needle))
        .cloned()
        .collect()
}

/// The ids whose rows must expand after a fill (`FillListTree`'s
/// `flag`): a folder expands while NOT `Collapsed`, the Library root
/// always expands.
fn collect_expanded(items: &[ComicListItem], out: &mut HashSet<CrGuid>) {
    for item in items {
        match item {
            ComicListItem::Library(_) => out.insert(item.base().id),
            ComicListItem::Folder(folder) => {
                if !folder.collapsed {
                    out.insert(folder.base.id);
                }
                collect_expanded(&folder.items, out);
                false
            }
            _ => false,
        };
    }
}

fn item_matches(item: &ComicListItem, needle: &str) -> bool {
    match item {
        ComicListItem::Library(_) => true,
        ComicListItem::Folder(folder) => {
            folder.items.iter().any(|child| item_matches(child, needle))
        }
        _ => item
            .base()
            .name
            .as_deref()
            .unwrap_or("")
            .to_lowercase()
            .contains(needle),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cr_core::database::list_items::{FolderItem, ListItemBase, SmartListItem};

    fn smart(name: &str) -> ComicListItem {
        ComicListItem::Smart(SmartListItem {
            base: ListItemBase {
                name: Some(name.to_string()),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    fn folder(name: &str, items: Vec<ComicListItem>) -> ComicListItem {
        ComicListItem::Folder(FolderItem {
            base: ListItemBase {
                name: Some(name.to_string()),
                ..Default::default()
            },
            items,
            ..Default::default()
        })
    }

    #[test]
    fn filter_keeps_library_hides_non_matching() {
        let items = vec![smart("Batman"), smart("Superman")];
        let out = filter_items(&items, "bat");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].base().name.as_deref(), Some("Batman"));
        // Empty filter = everything.
        assert_eq!(filter_items(&items, "").len(), 2);
    }

    #[test]
    fn filter_is_case_insensitive_and_folder_recursive() {
        let items = vec![folder("My Folder", vec![smart("Watchmen")])];
        // The folder name does NOT match, but a child does (the C#
        // folder override).
        let out = filter_items(&items, "watch");
        assert_eq!(out.len(), 1);
        // A non-matching folder vanishes entirely.
        assert!(filter_items(&items, "zzz").is_empty());
    }
}
