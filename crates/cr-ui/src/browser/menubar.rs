//! The main-window menubar (`MainForm.mainMenuStrip`, Phase 5.5
//! T3): a `PopoverMenuBar` from a pure `Gio::Menu` model — the six
//! C# menus with their items, separators, check items and radio
//! items, wired to the T1 `win.` actions. The C# dynamic submenu
//! fills (Open Books, Recent Books, Page Type, Page Rotation, the
//! bookmark list, the Workspaces/List Layout lists) land in T4/T6 —
//! their PARENTS stay out until they can hold content.
//!
//! Every present item cites its `MainForm.Designer.cs` source; the
//! absent items are the ADR-024 omissions, asserted absent in the
//! tests. The menu check/radio state follows the `win.` action
//! states (GTK renders stateful-action state on menu items).
//!
//! Menu-bar visibility is `OnGuiVisibilities` Fill-mode parity
//! (`MainForm.cs:3673-3688`): the C# hides the menubar through the
//! `AutoHideMainMenu` setting (default TRUE — the app starts with
//! the menubar hidden unless no book is open), Alt reveals it.

use gtk4::gio;
use gtk4::prelude::*;

use MenuNode::{Item, Sep, Sub};

/// One menubar node. Actions are detailed `win.` names (radio
/// targets spell the value: `win.page-fit::original`); a check/radio
/// display comes from the action state, never from the node.
pub enum MenuNode {
    /// Label (GTK `_` mnemonic), detailed action name, display
    /// accelerator ("" = the C# item has no shortcut).
    Item(&'static str, &'static str, &'static str),
    /// Submenu label + children.
    Sub(&'static str, &'static [MenuNode]),
    /// A separator (a section break in the Gio model).
    Sep,
}

/// The File menu (`fileMenu.DropDownItems`, Designer:451-476).
/// Absent per ADR-024: Update Web Comics (provider gap),
/// Synchronize Devices, Automation (Phase 6), Open Remote Library.
/// Open Books/Recent Books are dynamic (T4).
pub const FILE: &[MenuNode] = &[
    Item("_Open File...", "win.open-file", "<Control>o"),
    Item("_Close", "win.close", "<Control>x"),
    Item("Close A_ll", "win.close-all", "<Control><Shift>x"),
    Sep,
    Item("New _Tab", "win.new-tab", "<Control>t"),
    Sep,
    Item(
        "_Add Folder to Library...",
        "win.add-folder",
        "<Control><Shift>a",
    ),
    Item(
        "Scan Book _Folders",
        "win.scan-folders",
        "<Control><Shift>s",
    ),
    Item(
        "Update all Book Files",
        "win.update-book-files",
        "<Control><Shift>u",
    ),
    Item("Generate Cover Thumbnails", "win.generate-thumbnails", ""),
    Item("_Tasks...", "win.tasks", "<Control><Shift>t"),
    Sep,
    Item(
        "_New fileless Book Entry...",
        "win.new-book-entry",
        "<Control><Shift>n",
    ),
    Sep,
    Item("Rest_art", "win.restart", "<Control><Shift>q"),
    Sep,
    Item("_Exit", "win.quit", "<Control>q"),
];

/// The My Rating submenu (`contextRating`, Designer:729-741). The
/// C# also embeds a star-slider control before the last separator
/// (MainForm.cs:1043-1046) — a WinForms in-menu editor; the port
/// keeps the items (the Quick Rating dialog covers it, T13).
pub const RATING: &[MenuNode] = &[
    Item("_None", "win.rating-0", "<Alt><Shift>0"),
    Sep,
    Item("* (1 Star)", "win.rating-1", "<Alt><Shift>1"),
    Item("** (2 Stars)", "win.rating-2", "<Alt><Shift>2"),
    Item("*** (3 Stars)", "win.rating-3", "<Alt><Shift>3"),
    Item("**** (4 Stars)", "win.rating-4", "<Alt><Shift>4"),
    Item("***** (5 Stars)", "win.rating-5", "<Alt><Shift>5"),
    Sep,
    Item(
        "Quick Rating and Review...",
        "win.quick-rating",
        "<Alt><Shift>q",
    ),
];

/// The Bookmarks submenu (`miBookmarks.DropDownItems`,
/// Designer:823-830). The dynamic per-page bookmark list lands T4.
pub const BOOKMARKS: &[MenuNode] = &[
    Item("Set Bookmark...", "win.set-bookmark", "<Control><Shift>b"),
    Item(
        "Remove Bookmark",
        "win.remove-bookmark",
        "<Control><Shift>d",
    ),
    Item(
        "Previous Bookmark",
        "win.prev-bookmark",
        "<Control><Shift>p",
    ),
    // The C# gives Next Bookmark Ctrl+Shift+N too; File ▸ New
    // fileless Book Entry wins the collision (menu order — see
    // commands.rs).
    Item("Next Bookmark", "win.next-bookmark", ""),
    Item("L_ast Page Read", "win.last-page-read", "<Control><Shift>l"),
];

/// The Edit menu (`editMenu.DropDownItems`, Designer:662-679).
/// Absent: Undo/Redo (ADR-024), Devices..., the Page Type / Page
/// Rotation parents (dynamic fills — T4).
pub const EDIT: &[MenuNode] = &[
    Item("Info...", "win.info", "<Control>i"),
    Sep,
    Sub("My R_ating", RATING),
    Sub("_Bookmarks", BOOKMARKS),
    Sep,
    Item("_Copy Page", "win.copy-page", "<Control>c"),
    Item("_Export Page...", "win.export-page", "<Control><Shift>c"),
    Sep,
    Item("_Refresh", "win.refresh", "F5"),
    Sep,
    Item("_Preferences...", "win.preferences", "<Control>F9"),
];

/// The Browse menu (`browseMenu.DropDownItems`, Designer:954-970).
/// Absent: Folders F7 (Phase 7), Search Browser, Info Panel,
/// Workspaces, List Layout (lands with the T14 data).
pub const BROWSE: &[MenuNode] = &[
    Item("_Browser", "win.toggle-browser", "F3"),
    Sep,
    Item("Li_brary", "win.view-library", "F6"),
    Item("_Pages", "win.view-pages", "F8"),
    Sep,
    Item("_Sidebar", "win.sidebar", "<Shift>F6"),
    Item("S_mall Preview", "win.small-preview", "<Shift>F7"),
    Sep,
    Item("Previous List", "win.prev-list", "<Control>j"),
    Item("Next List", "win.next-list", "<Control>k"),
];

/// The Page Layout submenu (`miPageLayout.DropDownItems`,
/// Designer:1368-1382): the fit radios, the layout radios, the RTL
/// and Only-fit checks.
pub const PAGE_LAYOUT: &[MenuNode] = &[
    Item("Original Size", "win.page-fit::original", "<Control>1"),
    Item("Fit _All", "win.page-fit::fit-all", "<Control>2"),
    Item("Fit _Width", "win.page-fit::fit-width", "<Control>3"),
    Item(
        "Fit Width (adaptive)",
        "win.page-fit::fit-width-adaptive",
        "<Control>4",
    ),
    Item("Fit _Height", "win.page-fit::fit-height", "<Control>5"),
    Item("Fit _Best", "win.page-fit::fit-best", "<Control>6"),
    Sep,
    Item("Single Page", "win.page-layout::single", "<Control>7"),
    Item("Two Pages", "win.page-layout::double", "<Control>8"),
    Item(
        "Two Pages (adaptive)",
        "win.page-layout::double-adaptive",
        "<Control>9",
    ),
    Item("Continuous", "win.page-layout::continuous", ""),
    Item("Right to Left", "win.right-to-left", "<Control>0"),
    Sep,
    Item(
        "_Only fit if oversized",
        "win.only-fit-oversized",
        "<Control><Shift>0",
    ),
];

/// The Zoom submenu (`miZoom.DropDownItems`, Designer:1493-1509):
/// in/out/toggle, the five presets, Custom...
pub const ZOOM: &[MenuNode] = &[
    Item("Zoom _In", "win.zoom-in", "<Control>equal"),
    Item("Zoom _Out", "win.zoom-out", "<Control>minus"),
    Item("Toggle Zoom", "win.toggle-zoom", "<Control><Alt>z"),
    Sep,
    Item("100%", "win.zoom-preset::100", ""),
    Item("125%", "win.zoom-preset::125", ""),
    Item("150%", "win.zoom-preset::150", ""),
    Item("200%", "win.zoom-preset::200", ""),
    Item("400%", "win.zoom-preset::400", ""),
    Sep,
    Item("_Custom...", "win.zoom-custom", "<Control><Shift>z"),
];

/// The Rotation submenu (`miRotation.DropDownItems`,
/// Designer:1581-1595).
pub const ROTATION: &[MenuNode] = &[
    Item("Rotate Left", "win.rotate-left", "<Control><Shift>minus"),
    Item("Rotate Right", "win.rotate-right", "<Control><Shift>plus"),
    Sep,
    Item("_No Rotation", "win.rotate-0", "<Control><Shift>7"),
    Item("90°", "win.rotate-90", "<Control><Shift>8"),
    Item("180°", "win.rotate-180", "<Control><Shift>9"),
    // The C# also gives Rotate 270 Ctrl+Shift+D0; Only fit if
    // oversized wins the collision (menu order — see commands.rs).
    Item("270°", "win.rotate-270", ""),
    Sep,
    Item("Autorotate Double Pages", "win.auto-rotate", ""),
];

/// The Display menu (`displayMenu.DropDownItems`, Designer:1319-1330).
pub const DISPLAY: &[MenuNode] = &[
    Item("Book Display Settings...", "win.display-settings", "F9"),
    Sep,
    Sub("_Page Layout", PAGE_LAYOUT),
    Sub("Zoom", ZOOM),
    Sub("_Rotation", ROTATION),
    Sep,
    Item("Minimal User Interface", "win.minimal-gui", "F10"),
    Item("_Full Screen", "win.full-screen", "F11"),
    Item("Reader in _own Window", "win.undock-reader", "F12"),
    Sep,
    Item("_Magnifier", "win.magnifier", "<Control>m"),
];

/// The Help menu (`helpMenu.DropDownItems`, Designer:1699-1712).
/// Absent per ADR-024: the documentation/homepage/forum/news/update
/// links and the Plugins help (Phase 6).
pub const HELP: &[MenuNode] = &[Item("_About...", "win.about", "<Alt>F1")];

/// The six menus in order (`mainMenuStrip.Items`).
pub const MENUS: &[(&str, &[MenuNode])] = &[
    ("_File", FILE),
    ("_Edit", EDIT),
    ("_Browse", BROWSE),
    ("_Read", READ),
    ("_Display", DISPLAY),
    ("_Help", HELP),
];

/// The Read menu (`readMenu.DropDownItems`, Designer:1159-1176) —
/// declared after MENUS to keep the table order readable.
pub const READ: &[MenuNode] = &[
    Item("_First Page", "win.first-page", "<Control>b"),
    Item("_Previous Page", "win.prev-page", "<Control>p"),
    Item("_Next Page", "win.next-page", "<Control>n"),
    Item("_Last Page", "win.last-page", "<Control>e"),
    Sep,
    Item("Pre_vious Book", "win.prev-book", "<Control><Alt>p"),
    Item("Ne_xt Book", "win.next-book", "<Control><Alt>n"),
    Item("Random Book", "win.random-book", "<Control><Alt>o"),
    Item("Show in _Browser", "win.show-in-browser", "<Control>F3"),
    Sep,
    Item("_Previous Tab", "win.prev-tab", "<Control><Shift>j"),
    Item("Next _Tab", "win.next-tab", "<Control><Shift>k"),
    Sep,
    Item("_Auto Scrolling", "win.auto-scroll", "<Control>s"),
    Item(
        "Double Page Auto Scrolling",
        "win.double-auto-scroll",
        "<Alt><Shift>s",
    ),
    Sep,
    Item(
        "Track current Page",
        "win.track-current-page",
        "<Alt><Shift>t",
    ),
];

/// Builds the Gio menu model from the table. The `accel` attribute
/// drives the PopoverMenuBar's shortcut display; the T1
/// `set_accels_for_action` table stays the single ACTIVATION
/// source.
pub fn build_model(defs: &[MenuNode]) -> gio::Menu {
    let menu = gio::Menu::new();
    for node in defs {
        match node {
            MenuNode::Sep => menu.append_section(None, &gio::Menu::new()),
            MenuNode::Item(label, action, accel) => {
                let item = gio::MenuItem::new(Some(label), None);
                item.set_detailed_action(action);
                if !accel.is_empty() {
                    item.set_attribute_value("accel", Some(&accel.to_variant()));
                }
                menu.append_item(&item);
            }
            MenuNode::Sub(label, children) => {
                menu.append_submenu(Some(label), &build_model(children));
            }
        }
    }
    menu
}

/// The PopoverMenuBar for the browser window (call once).
pub fn create_menubar() -> gtk4::PopoverMenuBar {
    let model = gio::Menu::new();
    for (label, defs) in MENUS {
        model.append_submenu(Some(label), &build_model(defs));
    }
    gtk4::PopoverMenuBar::from_model(Some(&model))
}

/// `OnGuiVisibilities` Fill-mode parity (`MainForm.cs:3676-3688`):
/// `flag4 = flag || !IsComicViewer || (ShowMainMenuNoComicOpen &&
/// !bookOpen)`; `visible = flag4 && (!AutoHideMainMenu ||
/// (ShowMainMenuNoComicOpen && !bookOpen))`. The undocked shape
/// forces the menubar ON (`ReaderUndocked` branch, MainForm.cs:3668)
/// and MinimalGui turns `flag` off. `revealed` is the Alt-reveal
/// override (`OnKeyUp`, MainForm.cs:3863).
pub fn menubar_visible(
    minimal: bool,
    undocked: bool,
    is_comic_viewer: bool,
    has_book: bool,
    auto_hide: bool,
    show_no_comic: bool,
    revealed: bool,
) -> bool {
    if revealed {
        return !minimal;
    }
    if undocked {
        return true;
    }
    let flag = !minimal;
    let flag4 = flag || !is_comic_viewer || (show_no_comic && !has_book);
    flag4 && (!auto_hide || (show_no_comic && !has_book))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every action named in the table exists in the T1 registry —
    /// the COMMANDS table, the radio values, or the shell-only
    /// actions the command table does not carry.
    #[test]
    fn every_menu_action_exists() {
        let known: std::collections::HashSet<&str> = crate::commands::COMMANDS
            .iter()
            .map(|c| c.action)
            .chain([
                // The shell-only actions (no CommandSpec entry).
                "view-mode",
                "sort-column",
                "sort-direction",
                "group-by",
                "thumb-bigger",
                "thumb-smaller",
            ])
            .collect();
        let resolve = |action: &str| {
            let base = action
                .split("::")
                .next()
                .unwrap()
                .strip_prefix("win.")
                .unwrap();
            assert!(known.contains(base), "menu action {action} has no command");
            // Radio targets must be real values.
            if let Some((_, value)) = action.split_once("::") {
                let valid = match base {
                    "page-fit" => crate::commands::FIT_MODES.iter().any(|(v, _)| *v == value),
                    "page-layout" => crate::commands::LAYOUT_MODES
                        .iter()
                        .any(|(v, _)| *v == value),
                    "zoom-preset" => matches!(value, "100" | "125" | "150" | "200" | "400"),
                    _ => panic!("unexpected parametered action {action}"),
                };
                assert!(valid, "menu radio target {action} is not a value");
            }
        };
        let walk = |defs: &[MenuNode]| {
            for node in defs {
                if let MenuNode::Item(_, action, _) = node {
                    resolve(action);
                }
            }
        };
        for (label, defs) in MENUS {
            assert!(label.starts_with('_'), "top menu {label} has no mnemonic");
            walk(defs);
        }
        // Submenus live one level deeper; walk them explicitly.
        for defs in [RATING, BOOKMARKS, PAGE_LAYOUT, ZOOM, ROTATION] {
            for node in defs {
                if let MenuNode::Item(_, action, _) = node {
                    resolve(action);
                }
            }
        }
    }

    /// The displayed accelerator must equal the REGISTERED one (the
    /// commands table stays the single activation source; a drift
    /// would show a shortcut that does not fire).
    #[test]
    fn displayed_accels_match_the_registered_ones() {
        let registered: std::collections::HashMap<&str, &str> = crate::commands::COMMANDS
            .iter()
            .map(|c| (c.action, c.accels.first().copied().unwrap_or("")))
            .collect();
        let radio: std::collections::HashMap<&str, &str> = crate::commands::FIT_MODES
            .iter()
            .chain(crate::commands::LAYOUT_MODES.iter())
            .map(|(v, a)| (*v, *a))
            .collect();
        let check = |action: &str, accel: &str| {
            let (base, value) = action.split_once("::").unwrap_or((action, ""));
            let base = base.strip_prefix("win.").unwrap();
            let expected = match (base, value) {
                ("page-fit" | "page-layout", v) => radio.get(v).copied().unwrap_or(""),
                _ => registered.get(base).copied().unwrap_or(""),
            };
            assert_eq!(
                accel, expected,
                "menu display accel for {action} drifted from the commands table"
            );
        };
        for (_, defs) in MENUS {
            for node in defs.iter() {
                if let MenuNode::Item(_, action, accel) = node {
                    check(action, accel);
                }
            }
        }
        for defs in [RATING, BOOKMARKS, PAGE_LAYOUT, ZOOM, ROTATION] {
            for node in defs {
                if let MenuNode::Item(_, action, accel) = node {
                    check(action, accel);
                }
            }
        }
    }

    /// The ADR-024 omissions and the T4 dynamic parents stay out of
    /// the static skeleton.
    #[test]
    fn omitted_items_are_absent() {
        let all_labels: Vec<String> = MENUS
            .iter()
            .flat_map(|(_, defs)| defs.iter())
            .map(|node| match node {
                MenuNode::Item(label, _, _) => (*label).to_string(),
                MenuNode::Sub(label, _) => (*label).to_string(),
                MenuNode::Sep => String::new(),
            })
            .collect();
        let top: Vec<&str> = MENUS.iter().map(|(l, _)| *l).collect();
        assert_eq!(
            top,
            ["_File", "_Edit", "_Browse", "_Read", "_Display", "_Help"]
        );
        for absent in [
            "Undo",
            "Redo",
            "Page Type",
            "Page Rotation",
            "Devices...",
            "Search Browser",
            "Info Panel",
            "Workspaces",
            "List Layout",
            "Update Web Comics",
            "Synchronize Devices",
            "Automation",
            "Open Remote Library",
            "Open Books",
            "Recent Books",
            "News",
        ] {
            assert!(
                !all_labels.iter().any(|l| l.contains(absent)),
                "{absent} must stay out of the static skeleton"
            );
        }
        // The Browse "Folders" item (F7, Phase 7) is its own label —
        // distinct from "Scan Book _Folders".
        assert!(!all_labels.iter().any(|l| l == "Folders"));
    }

    /// `OnGuiVisibilities` truth table (the C# Fill-mode rule).
    #[test]
    fn menubar_visibility_rule() {
        // Browser view, no book: visible despite auto-hide (the
        // ShowMainMenuNoComicOpen default).
        assert!(menubar_visible(
            false, false, false, false, true, true, false
        ));
        // Reader view with a book: hidden (auto-hide default) — Alt
        // reveals.
        assert!(!menubar_visible(
            false, false, true, true, true, true, false
        ));
        assert!(menubar_visible(false, false, true, true, true, true, true));
        // Auto-hide OFF: always visible outside MinimalGui.
        assert!(menubar_visible(
            false, false, true, true, false, true, false
        ));
        // MinimalGui hides everything, reveal included.
        assert!(!menubar_visible(true, false, true, true, true, true, true));
        // Undocked: forced on.
        assert!(menubar_visible(false, true, false, true, true, true, false));
        // show_no_comic OFF + browser + no book: hidden with
        // auto-hide on.
        assert!(!menubar_visible(
            false, false, false, false, true, false, false
        ));
    }
}
