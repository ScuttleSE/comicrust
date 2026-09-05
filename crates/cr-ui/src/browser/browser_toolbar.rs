//! The browser toolbar (`ComicBrowserControl.toolStrip`, Phase 5.5
//! T6): the strip above the navigator + ItemView panes. Buttons in
//! Designer order (:976-991): Sidebar toggle, Browse Previous/Next
//! (the list history), Views, Group, Arrange, the right-aligned
//! Quick Search, List Layouts, Duplicate List. Absent per scope:
//! Stack (the port has no stacker), Undo/Redo (ADR-024), and the
//! read-state display-option panel (the port grays nothing — the
//! filters narrow the grid directly).
//!
//! The drop tables reuse the menubar row machinery
//! (`menubar::build_dropdown` — parent to the anchor before popup,
//! one popover per instance); the radio/check states ride the same
//! resolve closure the menubar gets. Group/Arrange are dynamic
//! tables (the C# `CreateGroupMenu`/`CreateArrangeMenu` — Not
//! Grouped/Not Sorted first, then the columns) built once.

use gtk4::prelude::*;
use gtk4::Button;

use super::menubar::{self, Dropdown, MenuNode};
use MenuNode::{Dyn, Item, Sep};

/// The Views drop (`tbbView.DropDownItems`): the three view radios,
/// the read-state filter radios, the comic-type checks, duplicates.
pub const VIEWS: &[MenuNode] = &[
    Item("T&humbnails", "win.view-mode::thumbnail", "", "ThumbView"),
    Item("&Tiles", "win.view-mode::tile", "", "TileView"),
    Item("&Details", "win.view-mode::detail", "", "DetailView"),
    Sep,
    Item("Show All", "win.view-filter::all", "", ""),
    Item("Show not Read", "win.view-filter::unread", "", ""),
    Item("Show Reading", "win.view-filter::reading", "", ""),
    Item("Show Read", "win.view-filter::read", "", ""),
    Sep,
    Item("Show only Books", "win.comic-type::books", "", ""),
    Item(
        "Show only fileless Entries",
        "win.comic-type::fileless",
        "",
        "",
    ),
    Sep,
    Item("Show Duplicates", "win.duplicates-only", "", ""),
];

/// The Quick Search scope menu (`contextQuickSearch`, the search
/// box's drop): All, then the field groups. "Filename" maps to the
/// C# `MatcherOption.File`.
pub const SEARCH_SCOPE: &[MenuNode] = &[
    Item("All", "win.search-scope::all", "", ""),
    Sep,
    Item("Series", "win.search-scope::series", "", ""),
    Item("Writer", "win.search-scope::writer", "", ""),
    Item("Artists", "win.search-scope::artists", "", ""),
    Item("Descriptive", "win.search-scope::descriptive", "", ""),
    Item("Catalog", "win.search-scope::catalog", "", ""),
    Item("Filename", "win.search-scope::file", "", ""),
];

/// The Duplicate List drop: the folder list fills dynamically (the
/// C# `tbbDuplicateList_DropDownOpening` walks
/// `ComicLists.GetItems<ComicListItemFolder>()` with an indent per
/// child level).
pub const DUPLICATE: &[MenuNode] = &[Dyn("duplicate-list")];

/// The Group (tbbGroup) and Arrange (tbbSort) tables are dynamic:
/// `CreateGroupMenu`/`CreateArrangeMenu` open with the Not
/// Grouped/Not Sorted entry, then one row per column.
pub fn sort_defs() -> Vec<MenuNode> {
    let mut defs = vec![Item("Not Sorted", "win.sort-column::", "", "SortUp"), Sep];
    for column in super::columns::default_columns()
        .iter()
        .filter(|c| c.visible && c.is_text_column())
    {
        defs.push(Item(column.name, sort_action(column.property), "", ""));
    }
    defs
}

/// The C# arrange menu rows carry the column; the port rides the
/// property through the detailed action name. The strings leak —
/// the menu tables live for the process lifetime (like the consts).
fn sort_action(property: &'static str) -> &'static str {
    Box::leak(format!("win.sort-column::{property}").into_boxed_str())
}

pub fn group_defs() -> Vec<MenuNode> {
    let mut defs = vec![Item("Not Grouped", "win.group-by::", "", ""), Sep];
    for (key, _) in cr_engine::group::groupers() {
        defs.push(Item(key, group_action(key), "", ""));
    }
    defs
}

fn group_action(key: &'static str) -> &'static str {
    Box::leak(format!("win.group-by::{key}").into_boxed_str())
}

/// The scope value → cue label (`quickSearchCueTexts`: "Search" +
/// the menu text; the C# cue array omits Catalog — an index bug that
/// would throw on the Catalog scope — the port gives it a cue).
pub const SEARCH_SCOPE_LABELS: &[(&str, &str)] = &[
    ("all", "Search All"),
    ("series", "Search Series"),
    ("writer", "Search Writer"),
    ("artists", "Search Artists"),
    ("descriptive", "Search Descriptive"),
    ("catalog", "Search Catalog"),
    ("file", "Search File"),
];

/// The browser toolbar widget.
pub struct BrowserToolbar {
    bar: gtk4::Box,
    /// (action base, button) — the enable sync (Sidebar, Browse
    /// Previous/Next).
    toggles: Vec<(&'static str, Button)>,
    group_label: gtk4::Label,
    sort_label: gtk4::Label,
    sort_icon: gtk4::Image,
    /// (name, dropdown, anchor) — the probe paths.
    dropdowns: Vec<(&'static str, Dropdown, gtk4::Button)>,
}

impl Clone for BrowserToolbar {
    fn clone(&self) -> Self {
        Self {
            bar: self.bar.clone(),
            toggles: self.toggles.clone(),
            group_label: self.group_label.clone(),
            sort_label: self.sort_label.clone(),
            sort_icon: self.sort_icon.clone(),
            dropdowns: self.dropdowns.clone(),
        }
    }
}

/// A flat button: [icon 16 px][label] (the C# toolbar buttons show
/// Image + Text).
fn tool_button(icon: &'static str, label: &str, tooltip: &str) -> (Button, gtk4::Image) {
    let button = Button::new();
    let hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 4);
    let image = gtk4::Image::new();
    image.set_pixel_size(16);
    if let Some(texture) = crate::icon::icon(icon) {
        image.set_paintable(Some(&texture));
    }
    hbox.append(&image);
    if !label.is_empty() {
        let lbl = gtk4::Label::new(Some(label));
        hbox.append(&lbl);
    }
    button.set_child(Some(&hbox));
    button.set_tooltip_text(Some(tooltip));
    button.add_css_class("flat");
    (button, image)
}

/// A dropdown button (drop only — the C# split buttons without a
/// main click).
fn drop_tool_button(
    icon: &'static str,
    label: &str,
    tooltip: &str,
    drop: Dropdown,
) -> (Button, gtk4::Image, Option<gtk4::Label>) {
    let (button, image) = tool_button(icon, label, tooltip);
    let label_widget = button.child().and_downcast::<gtk4::Box>().and_then(|hbox| {
        hbox.first_child()
            .and_then(|image| image.next_sibling())
            .and_downcast::<gtk4::Label>()
    });
    {
        let drop = drop.clone();
        let btn = button.clone();
        button.connect_clicked(move |_| drop.open(&btn));
    }
    (button, image, label_widget)
}

impl BrowserToolbar {
    /// Builds the strip above the browser panes. `search` is the
    /// Quick Search entry — mounted right-aligned with the scope
    /// drop (the C# `tsQuickSearch.SearchMenu`).
    pub fn create(window: &gtk4::ApplicationWindow, search: &gtk4::Entry) -> BrowserToolbar {
        let bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 2);
        bar.add_css_class("toolbar");
        let mut toggles: Vec<(&'static str, Button)> = Vec::new();
        let mut dropdowns: Vec<(&'static str, Dropdown, gtk4::Button)> = Vec::new();
        let mk = |defs: &[MenuNode]| menubar::build_dropdown(defs, window);

        // tbSidebar: the left-panel toggle (win.sidebar).
        let (sidebar_btn, _) = tool_button("Sidebar", "", "Sidebar");
        {
            let window = window.clone();
            sidebar_btn.connect_clicked(move |_| {
                let _ = gtk4::prelude::WidgetExt::activate_action(&window, "win.sidebar", None);
            });
        }
        toggles.push(("sidebar", sidebar_btn.clone()));
        bar.append(&sidebar_btn);

        // btBrowsePrev/btBrowseNext: the list history.
        let (prev_btn, _) = tool_button("BrowsePrevious", "", "Previous List");
        {
            let window = window.clone();
            prev_btn.connect_clicked(move |_| {
                let _ = gtk4::prelude::WidgetExt::activate_action(&window, "win.prev-list", None);
            });
        }
        toggles.push(("prev-list", prev_btn.clone()));
        bar.append(&prev_btn);

        let (next_btn, _) = tool_button("BrowseNext", "", "Next List");
        {
            let window = window.clone();
            next_btn.connect_clicked(move |_| {
                let _ = gtk4::prelude::WidgetExt::activate_action(&window, "win.next-list", None);
            });
        }
        toggles.push(("next-list", next_btn.clone()));
        bar.append(&next_btn);

        let sep1 = gtk4::Separator::new(gtk4::Orientation::Horizontal);
        sep1.set_margin_top(4);
        sep1.set_margin_bottom(4);
        bar.append(&sep1);

        // tbbView: the Views drop.
        let views_drop = mk(VIEWS);
        let (views_btn, _, _) = drop_tool_button(
            "View",
            "Views",
            "Change how and what Books are displayed",
            views_drop.clone(),
        );
        dropdowns.push(("views", views_drop, views_btn.clone()));
        bar.append(&views_btn);

        // tbbGroup: the dynamic group drop (text = the current
        // grouper — the C# OnIdle label update).
        let group_drop = mk(&group_defs());
        let (group_btn, _, group_label) = drop_tool_button(
            "Group",
            "Group",
            "Group Books by different criteria",
            group_drop.clone(),
        );
        dropdowns.push(("group", group_drop, group_btn.clone()));
        bar.append(&group_btn);

        // tbbSort ("Arrange"): the sort drop.
        let sort_drop = mk(&sort_defs());
        let (sort_btn, sort_icon, sort_label) = drop_tool_button(
            "SortUp",
            "Arrange",
            "Change the sort order of the Books",
            sort_drop.clone(),
        );
        dropdowns.push(("sort", sort_drop, sort_btn.clone()));
        bar.append(&sort_btn);

        // The Quick Search rides right-aligned (the C#
        // `tsQuickSearch` Alignment=Right): it expands, everything
        // after it packs to the end.
        search.set_hexpand(true);
        search.set_placeholder_text(Some("Search All"));
        // The scope chevron inside the box (the C#
        // `tsQuickSearch.TextBox.SearchMenu`).
        search.set_secondary_icon_name(Some("pan-down-symbolic"));
        {
            let scope_drop = mk(SEARCH_SCOPE);
            let entry = search.clone();
            search.connect_icon_press(move |_, _pos| {
                scope_drop.open_at(&entry);
            });
        }
        bar.append(search);

        // tsListLayouts: the C# List Layouts drop (Edit List Layout,
        // Save, Reset Background, Edit Layouts) — disabled until the
        // T14 workspace data lands.
        let (layouts_btn, _) = tool_button("ListLayout", "", "Manage List Layouts");
        layouts_btn.set_sensitive(false);
        bar.append(&layouts_btn);

        let sep2 = gtk4::Separator::new(gtk4::Orientation::Horizontal);
        sep2.set_margin_top(4);
        sep2.set_margin_bottom(4);
        bar.append(&sep2);

        // tbbDuplicateList: the folder drop (dynamic fill).
        let dup_drop = mk(DUPLICATE);
        let (dup_btn, _, _) =
            drop_tool_button("AddList", "", "Duplicate current List", dup_drop.clone());
        dropdowns.push(("duplicate", dup_drop, dup_btn.clone()));
        bar.append(&dup_btn);

        BrowserToolbar {
            bar,
            toggles,
            // The Group/Arrange buttons carry labels (the label
            // text sync — the C# OnIdle updates).
            group_label: group_label.expect("group button label"),
            sort_label: sort_label.expect("sort button label"),
            sort_icon,
            dropdowns,
        }
    }

    pub fn widget(&self) -> &gtk4::Box {
        &self.bar
    }

    /// Installs the shared dynamic fill provider (the Duplicate List
    /// folder walk).
    pub fn set_dyn_fill(&self, fill: menubar::DynFillFn) {
        for (_, drop, _) in &self.dropdowns {
            drop.set_dyn_fill(fill.clone());
        }
    }

    /// Applies the action states (the enable sync — Sidebar needs no
    /// book, the Browse buttons need a walkable history).
    pub fn sync(&self, resolve: &dyn Fn(&str) -> Option<menubar::ActionState>) {
        for (_, drop, _) in &self.dropdowns {
            drop.sync(resolve);
        }
        for (base, button) in &self.toggles {
            let enabled = resolve(base).map(|s| s.enabled).unwrap_or(true);
            button.set_sensitive(enabled);
        }
    }

    /// The Group/Arrange button texts + the sort direction icon (the
    /// C# `OnIdle`: `tbbSort.Text = sortColumn.Text`,
    /// `tbbGroup.Text = groupColumn.Text`, the image flips with the
    /// sort order).
    pub fn sync_labels(
        &self,
        sort_column: Option<String>,
        sort_descending: bool,
        grouper: Option<&str>,
    ) {
        self.sort_label
            .set_text(sort_column.as_deref().unwrap_or("Arrange"));
        self.group_label.set_text(grouper.unwrap_or("Group"));
        let icon = if sort_descending {
            "SortDown"
        } else {
            "SortUp"
        };
        if let Some(texture) = crate::icon::icon(icon) {
            self.sort_icon.set_paintable(Some(&texture));
        }
    }

    /// The Group/Arrange label texts (the probe).
    pub fn label_texts(&self) -> (String, String) {
        (
            self.group_label.text().to_string(),
            self.sort_label.text().to_string(),
        )
    }

    /// The dropdowns by name (the probe).
    pub fn dropdown(&self, name: &str) -> Option<Dropdown> {
        self.dropdowns
            .iter()
            .find(|(n, _, _)| *n == name)
            .map(|(_, d, _)| d.clone())
    }

    /// Opens one dropdown through its stored anchor (the probe).
    pub fn open_dropdown(&self, name: &str) -> bool {
        let hit = self
            .dropdowns
            .iter()
            .find(|(n, _, _)| *n == name)
            .map(|(_, d, b)| (d.clone(), b.clone()));
        match hit {
            Some((drop, anchor)) => {
                drop.open(&anchor);
                true
            }
            None => false,
        }
    }

    /// Closes one dropdown (the probe cleanup).
    pub fn close_dropdown(&self, name: &str) {
        if let Some((_, d, _)) = self.dropdowns.iter().find(|(n, _, _)| *n == name) {
            d.popover().popdown();
        }
    }

    /// Whether a dropdown's popover is mapped (the probe).
    pub fn drop_mapped(&self, name: &str) -> bool {
        self.dropdowns
            .iter()
            .find(|(n, _, _)| *n == name)
            .is_some_and(|(_, d, _)| d.popover().is_mapped())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every browser-toolbar action exists in the registry (the
    /// COMMANDS table or the shell-only actions).
    #[test]
    fn browser_toolbar_actions_exist() {
        let known: std::collections::HashSet<&str> = crate::commands::COMMANDS
            .iter()
            .map(|c| c.action)
            .chain([
                "view-mode",
                "view-filter",
                "comic-type",
                "duplicates-only",
                "sort-column",
                "group-by",
                "search-scope",
                "sidebar",
                "prev-list",
                "next-list",
            ])
            .collect();
        for defs in [VIEWS, SEARCH_SCOPE] {
            for node in defs {
                if let Item(_, action, _, _) = node {
                    let base = action
                        .split("::")
                        .next()
                        .unwrap()
                        .strip_prefix("win.")
                        .unwrap();
                    assert!(
                        known.contains(base),
                        "browser toolbar action {action} has no command"
                    );
                }
            }
        }
        for node in sort_defs().iter().chain(group_defs().iter()) {
            if let Item(_, action, _, _) = node {
                let base = action
                    .split("::")
                    .next()
                    .unwrap()
                    .strip_prefix("win.")
                    .unwrap();
                assert!(
                    known.contains(base),
                    "browser toolbar action {action} has no command"
                );
            }
        }
    }

    /// The toolbar icon names resolve in the bundled set.
    #[test]
    fn browser_toolbar_icons_resolve() {
        for name in [
            "Sidebar",
            "BrowsePrevious",
            "BrowseNext",
            "View",
            "Group",
            "SortUp",
            "SortDown",
            "ListLayout",
            "AddList",
        ] {
            assert!(crate::icon::path_for_name(name).is_some(), "{name} missing");
        }
        for node in VIEWS.iter() {
            if let Item(_, _, _, icon) = node {
                if icon.is_empty() {
                    continue;
                }
                assert!(crate::icon::path_for_name(icon).is_some(), "{icon} missing");
            }
        }
    }
}
