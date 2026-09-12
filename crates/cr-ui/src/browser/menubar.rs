//! The main-window menubar (`MainForm.mainMenuStrip`, Phase 5.5
//! T3): a custom widget — a row of `MenuButton`s with hand-built
//! popover rows — rendered FROM the pure six-menu table.
//!
//! Why custom: the C# gives 78 menu items a 16 px icon
//! (`MainForm.Designer.cs` `mi*.Image = Resources.…`), and GTK4
//! removed menu-item icons (`PopoverMenuBar`/`PopoverMenu` ignore
//! the model's `icon` attribute — the `GtkImageMenuItem` removal).
//! The icons come from the bundled set (`icon.rs`); the mi→resx
//! mapping is extracted from the Designer verbatim and unit-gated
//! in both directions.
//!
//! Every present item cites its `MainForm.Designer.cs` source; the
//! absent items are the ADR-024 omissions, asserted absent in the
//! tests. Check/radio/disabled states come from the `win.` action
//! states (`sync`, driven by the shell after every dispatch).
//!
//! Recorded deviations of the custom shape (vs the C# ToolStrip):
//! no mnemonic-activation chain (labels still show the underline;
//! arrow-key focus movement and Enter activate), hover switching
//! and Left/Right top switching are hand-built, and the Escape /
//! click-away close is the popover native behavior.

use gtk4::glib;
use gtk4::prelude::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use MenuNode::{Dyn, Item, Sep, Sub};

/// One menubar node. Actions are detailed `win.` names (radio
/// targets spell the value: `win.page-fit::original`); check/radio
/// display comes from the action state, never from the node. The
/// 4th field is the resx icon ("" = the C# item has no image).
pub enum MenuNode {
    /// Label (GTK `_` mnemonic; a C# `&` renders stripped —
    /// [`strip_amp`]), detailed action name, display
    /// accelerator ("" = the C# item has no shortcut), resx icon.
    Item(&'static str, &'static str, &'static str, &'static str),
    /// Submenu label + children.
    Sub(&'static str, &'static [MenuNode]),
    /// A separator.
    Sep,
    /// A dynamic fill slot (`MainForm` `DropDownOpening` rebuilds):
    /// the provider set with [`MenubarWidget::set_dyn_fill`] supplies
    /// the rows every time the owning menu opens.
    Dyn(&'static str),
}

/// The File menu (`fileMenu.DropDownItems`, Designer:451-476).
/// Absent per ADR-024, now permanent: Update Web Comics (provider
/// gap), Synchronize Devices, Open Remote Library. The Automation
/// submenu is absent permanently — no scripting host (ADR-027); the
/// "New fileless Book Series..." item that C# sourced from the
/// NewComics.py script is a native row below. Open Books/Recent
/// Books carry the dynamic fills (T4).
pub const FILE: &[MenuNode] = &[
    Item("_Open File...", "win.open-file", "<Control>o", "Open"),
    Item("_Close", "win.close", "<Control>x", ""),
    Item("Close A_ll", "win.close-all", "<Control><Shift>x", ""),
    Sep,
    Item("New _Tab", "win.new-tab", "<Control>t", "NewTab"),
    Sep,
    Item(
        "_Add Folder to Library...",
        "win.add-folder",
        "<Control><Shift>a",
        "AddFolder",
    ),
    // NO C# item — the Windows-path migration helper (Phase 8 T11,
    // the user request): a port addition next to the other
    // re-point-the-library command.
    Item("Migrate _Windows Paths...", "win.migrate-paths", "", ""),
    Item(
        "Scan Book _Folders",
        "win.scan-folders",
        "<Control><Shift>s",
        "Scan",
    ),
    Item(
        "Update all Book Files",
        "win.update-book-files",
        "<Control><Shift>u",
        "UpdateSmall",
    ),
    Item(
        "Generate Cover Thumbnails",
        "win.generate-thumbnails",
        "",
        "Screenshot",
    ),
    Item(
        "_Tasks...",
        "win.tasks",
        "<Control><Shift>t",
        "BackgroundJob",
    ),
    // NO C# item — the Comic Vine disk cache (ADR-037, Phase 15).
    // The C# plugin had no cache, so it needed no command.
    Item("Import Comic Vine MCL File...", "win.cv-import-mcl", "", ""),
    Item("Update Comic Vine Cache", "win.cv-update", "", ""),
    Item("Warm Comic Vine Cache", "win.cv-warm", "", ""),
    Sep,
    Item(
        "_New fileless Book Entry...",
        "win.new-book-entry",
        "<Control><Shift>n",
        "",
    ),
    // The NewComics.py port (ADR-027) — the C# inserted the script's
    // NewBooks item directly after `miNewComic` (MainForm.cs:787).
    Item(
        "New fileless Book _Series...",
        "win.new-book-series",
        "",
        "",
    ),
    Sep,
    // The dynamic Open Books / Recent Books submenus (`miOpenNow`/
    // `miOpenRecent`; the parents carry no C# images).
    Sub("_Open Books", &[Dyn("open-books")]),
    Sub("&Recent Books", &[Dyn("recent-books")]),
    Sep,
    Item("Rest_art", "win.restart", "<Control><Shift>q", "Restart"),
    Sep,
    Item("_Exit", "win.quit", "<Control>q", ""),
];

/// The My Rating submenu (`contextRating`, Designer:729-741). The
/// C# also embeds a star-slider control before the last separator
/// (MainForm.cs:1043-1046) — a WinForms in-menu editor; the port
/// keeps the items (the Quick Rating dialog covers it, T13). The
/// C# rating items carry no images.
pub const RATING: &[MenuNode] = &[
    Item("_None", "win.rating-0", "<Alt><Shift>0", ""),
    Sep,
    Item("* (1 Star)", "win.rating-1", "<Alt><Shift>1", ""),
    Item("** (2 Stars)", "win.rating-2", "<Alt><Shift>2", ""),
    Item("*** (3 Stars)", "win.rating-3", "<Alt><Shift>3", ""),
    Item("**** (4 Stars)", "win.rating-4", "<Alt><Shift>4", ""),
    Item("***** (5 Stars)", "win.rating-5", "<Alt><Shift>5", ""),
    Sep,
    Item(
        "Quick Rating and Review...",
        "win.quick-rating",
        "<Alt><Shift>q",
        "",
    ),
];

/// The Bookmarks submenu (`miBookmarks.DropDownItems`,
/// Designer:823-830): the five static items, then the dynamic
/// per-page list after the C# "bms" separator (T4).
pub const BOOKMARKS: &[MenuNode] = &[
    Item(
        "Set Bookmark...",
        "win.set-bookmark",
        "<Control><Shift>b",
        "NewBookmark",
    ),
    Item(
        "Remove Bookmark",
        "win.remove-bookmark",
        "<Control><Shift>d",
        "RemoveBookmark",
    ),
    Item(
        "Previous Bookmark",
        "win.prev-bookmark",
        "<Control><Shift>p",
        "PreviousBookmark",
    ),
    // The C# gives Next Bookmark Ctrl+Shift+N too; File ▸ New
    // fileless Book Entry wins the collision (menu order — see
    // commands.rs).
    Item("Next Bookmark", "win.next-bookmark", "", "NextBookmark"),
    Item(
        "L_ast Page Read",
        "win.last-page-read",
        "<Control><Shift>l",
        "",
    ),
    Sep,
    Dyn("bookmarks"),
];

/// The Edit menu (`editMenu.DropDownItems`, Designer:662-679).
/// Absent: Undo/Redo (ADR-024), Devices... The Page Type / Page
/// Rotation parents sit between My Rating and Bookmarks (Designer
/// order) and fill dynamically (T4).
pub const EDIT: &[MenuNode] = &[
    Item("Info...", "win.info", "<Control>i", "GetInfo"),
    Sep,
    Sub("My R_ating", RATING),
    Sub("&Page Type", &[Dyn("page-type")]),
    Sub("Page Rotation", &[Dyn("page-rotation")]),
    Sub("_Bookmarks", BOOKMARKS),
    Sep,
    Item("_Copy Page", "win.copy-page", "<Control>c", "Copy"),
    Item(
        "_Export Page...",
        "win.export-page",
        "<Control><Shift>c",
        "",
    ),
    Sep,
    Item("_Refresh", "win.refresh", "F5", "Refresh"),
    Sep,
    Item(
        "_Preferences...",
        "win.preferences",
        "<Control>F9",
        "Preferences",
    ),
];

/// The Browse menu (`browseMenu.DropDownItems`, Designer:954-970).
/// Absent: Folders F7 (Phase 7), Search Browser, Info Panel,
/// Workspaces, List Layout (lands with the T14 data).
pub const BROWSE: &[MenuNode] = &[
    Item("_Browser", "win.toggle-browser", "F3", "Browser"),
    Sep,
    Item("Li_brary", "win.view-library", "F6", "Database"),
    Item("_Pages", "win.view-pages", "F8", "ComicPage"),
    Sep,
    Item("_Sidebar", "win.sidebar", "<Shift>F6", "Sidebar"),
    Item(
        "S_mall Preview",
        "win.small-preview",
        "<Shift>F7",
        "SmallPreview",
    ),
    Sep,
    // The dark-mode toggle — no C# item (recorded deviation; the C#
    // theme is the `-dark` boot switch). Iconless by the Designer
    // rule: the C# gives no image, so the icon gate asserts none.
    Item("_Dark Mode", "win.dark-mode", "", ""),
    Sep,
    Item(
        "Previous List",
        "win.prev-list",
        "<Control>j",
        "BrowsePrevious",
    ),
    Item("Next List", "win.next-list", "<Control>k", "BrowseNext"),
];

/// The Page Layout submenu (`miPageLayout.DropDownItems`,
/// Designer:1368-1382): the fit radios, the layout radios, the RTL
/// and Only-fit checks.
pub const PAGE_LAYOUT: &[MenuNode] = &[
    Item(
        "Original Size",
        "win.page-fit::original",
        "<Control>1",
        "Original",
    ),
    Item("Fit _All", "win.page-fit::fit-all", "<Control>2", "FitAll"),
    Item(
        "Fit _Width",
        "win.page-fit::fit-width",
        "<Control>3",
        "FitWidth",
    ),
    Item(
        "Fit Width (adaptive)",
        "win.page-fit::fit-width-adaptive",
        "<Control>4",
        "FitWidthAdaptive",
    ),
    Item(
        "Fit _Height",
        "win.page-fit::fit-height",
        "<Control>5",
        "FitHeight",
    ),
    Item(
        "Fit _Best",
        "win.page-fit::fit-best",
        "<Control>6",
        "FitBest",
    ),
    Sep,
    Item(
        "Single Page",
        "win.page-layout::single",
        "<Control>7",
        "SinglePage",
    ),
    Item(
        "Two Pages",
        "win.page-layout::double",
        "<Control>8",
        "TwoPageForced",
    ),
    Item(
        "Two Pages (adaptive)",
        "win.page-layout::double-adaptive",
        "<Control>9",
        "TwoPage",
    ),
    Item(
        "Continuous",
        "win.page-layout::continuous",
        "",
        "SinglePage",
    ),
    Item(
        "Right to Left",
        "win.right-to-left",
        "<Control>0",
        "RightToLeft",
    ),
    Sep,
    Item(
        "_Only fit if oversized",
        "win.only-fit-oversized",
        "<Control><Shift>0",
        "Oversized",
    ),
];

/// The Zoom submenu (`miZoom.DropDownItems`, Designer:1493-1509):
/// in/out/toggle, the five presets, Custom...
pub const ZOOM: &[MenuNode] = &[
    Item("Zoom _In", "win.zoom-in", "<Control>equal", "ZoomIn"),
    Item("Zoom _Out", "win.zoom-out", "<Control>minus", "ZoomOut"),
    Item("Toggle Zoom", "win.toggle-zoom", "<Control><Alt>z", ""),
    Sep,
    Item("100%", "win.zoom-preset::100", "", ""),
    Item("125%", "win.zoom-preset::125", "", ""),
    Item("150%", "win.zoom-preset::150", "", ""),
    Item("200%", "win.zoom-preset::200", "", ""),
    Item("400%", "win.zoom-preset::400", "", ""),
    Sep,
    Item("_Custom...", "win.zoom-custom", "<Control><Shift>z", ""),
];

/// The Rotation submenu (`miRotation.DropDownItems`,
/// Designer:1581-1595).
pub const ROTATION: &[MenuNode] = &[
    Item(
        "Rotate Left",
        "win.rotate-left",
        "<Control><Shift>minus",
        "RotateLeft",
    ),
    Item(
        "Rotate Right",
        "win.rotate-right",
        "<Control><Shift>plus",
        "RotateRight",
    ),
    Sep,
    Item(
        "_No Rotation",
        "win.rotate-0",
        "<Control><Shift>7",
        "Rotate0",
    ),
    Item("90°", "win.rotate-90", "<Control><Shift>8", "Rotate90"),
    Item("180°", "win.rotate-180", "<Control><Shift>9", "Rotate180"),
    // The C# also gives Rotate 270 Ctrl+Shift+D0; Only fit if
    // oversized wins the collision (menu order — see commands.rs).
    Item("270°", "win.rotate-270", "", "Rotate270"),
    Sep,
    Item(
        "Autorotate Double Pages",
        "win.auto-rotate",
        "",
        "AutoRotate",
    ),
];

/// The Display menu (`displayMenu.DropDownItems`, Designer:1319-1330).
pub const DISPLAY: &[MenuNode] = &[
    Item(
        "Book Display Settings...",
        "win.display-settings",
        "F9",
        "DisplaySettings",
    ),
    Sep,
    Sub("_Page Layout", PAGE_LAYOUT),
    Sub("Zoom", ZOOM),
    Sub("_Rotation", ROTATION),
    Sep,
    Item(
        "Minimal User Interface",
        "win.minimal-gui",
        "F10",
        "MenuToggle",
    ),
    Item("_Full Screen", "win.full-screen", "F11", "FullScreen"),
    Item(
        "Reader in _own Window",
        "win.undock-reader",
        "F12",
        "UndockReader",
    ),
    Sep,
    Item("_Magnifier", "win.magnifier", "<Control>m", "Zoom"),
];

/// The Help menu (`helpMenu.DropDownItems`, Designer:1699-1712).
/// Absent per ADR-024: the documentation/homepage/forum/news/update
/// links and the Plugins help (Phase 6).
pub const HELP: &[MenuNode] = &[Item("_About...", "win.about", "<Alt>F1", "About")];

/// The Read menu (`readMenu.DropDownItems`, Designer:1159-1176).
pub const READ: &[MenuNode] = &[
    Item("_First Page", "win.first-page", "<Control>b", "GoFirst"),
    Item(
        "_Previous Page",
        "win.prev-page",
        "<Control>p",
        "GoPrevious",
    ),
    Item("_Next Page", "win.next-page", "<Control>n", "GoNext"),
    Item("_Last Page", "win.last-page", "<Control>e", "GoLast"),
    Sep,
    Item(
        "Pre_vious Book",
        "win.prev-book",
        "<Control><Alt>p",
        "PrevFromList",
    ),
    Item(
        "Ne_xt Book",
        "win.next-book",
        "<Control><Alt>n",
        "NextFromList",
    ),
    Item(
        "Random Book",
        "win.random-book",
        "<Control><Alt>o",
        "RandomComic",
    ),
    Item(
        "Show in _Browser",
        "win.show-in-browser",
        "<Control>F3",
        "SyncBrowser",
    ),
    Sep,
    Item(
        "_Previous Tab",
        "win.prev-tab",
        "<Control><Shift>j",
        "Previous",
    ),
    Item("Next _Tab", "win.next-tab", "<Control><Shift>k", "Next"),
    Sep,
    Item(
        "_Auto Scrolling",
        "win.auto-scroll",
        "<Control>s",
        "CursorScroll",
    ),
    Item(
        "Double Page Auto Scrolling",
        "win.double-auto-scroll",
        "<Alt><Shift>s",
        "TwoPageAutoscroll",
    ),
    Sep,
    Item(
        "Track current Page",
        "win.track-current-page",
        "<Alt><Shift>t",
        "",
    ),
];

/// The six menus in order (`mainMenuStrip.Items`).
pub const MENUS: &[(&str, &[MenuNode])] = &[
    ("_File", FILE),
    ("_Edit", EDIT),
    ("_Browse", BROWSE),
    ("_Read", READ),
    ("_Display", DISPLAY),
    ("_Help", HELP),
];

/// The mi→resx icon assignments the C# Designer makes for the items
/// this port carries (`MainForm.Designer.cs` `mi*.Image` lines; all
/// resx names are in the bundled set). `_sub:bookmarks` is the
/// Bookmarks submenu parent.
#[cfg(test)]
const CSHARP_ITEM_ICONS: &[(&str, &str)] = &[
    ("open-file", "Open"),
    ("new-tab", "NewTab"),
    ("add-folder", "AddFolder"),
    ("scan-folders", "Scan"),
    ("update-book-files", "UpdateSmall"),
    ("generate-thumbnails", "Screenshot"),
    ("tasks", "BackgroundJob"),
    ("restart", "Restart"),
    ("info", "GetInfo"),
    ("set-bookmark", "NewBookmark"),
    ("remove-bookmark", "RemoveBookmark"),
    ("prev-bookmark", "PreviousBookmark"),
    ("next-bookmark", "NextBookmark"),
    ("copy-page", "Copy"),
    ("refresh", "Refresh"),
    ("preferences", "Preferences"),
    ("toggle-browser", "Browser"),
    ("view-library", "Database"),
    ("view-pages", "ComicPage"),
    ("sidebar", "Sidebar"),
    ("small-preview", "SmallPreview"),
    ("prev-list", "BrowsePrevious"),
    ("next-list", "BrowseNext"),
    ("first-page", "GoFirst"),
    ("prev-page", "GoPrevious"),
    ("next-page", "GoNext"),
    ("last-page", "GoLast"),
    ("prev-book", "PrevFromList"),
    ("next-book", "NextFromList"),
    ("random-book", "RandomComic"),
    ("show-in-browser", "SyncBrowser"),
    ("prev-tab", "Previous"),
    ("next-tab", "Next"),
    ("auto-scroll", "CursorScroll"),
    ("double-auto-scroll", "TwoPageAutoscroll"),
    ("display-settings", "DisplaySettings"),
    ("page-fit::original", "Original"),
    ("page-fit::fit-all", "FitAll"),
    ("page-fit::fit-width", "FitWidth"),
    ("page-fit::fit-width-adaptive", "FitWidthAdaptive"),
    ("page-fit::fit-height", "FitHeight"),
    ("page-fit::fit-best", "FitBest"),
    ("page-layout::single", "SinglePage"),
    ("page-layout::double", "TwoPageForced"),
    ("page-layout::double-adaptive", "TwoPage"),
    ("page-layout::continuous", "SinglePage"),
    ("right-to-left", "RightToLeft"),
    ("only-fit-oversized", "Oversized"),
    ("zoom-in", "ZoomIn"),
    ("zoom-out", "ZoomOut"),
    ("rotate-left", "RotateLeft"),
    ("rotate-right", "RotateRight"),
    ("rotate-0", "Rotate0"),
    ("rotate-90", "Rotate90"),
    ("rotate-180", "Rotate180"),
    ("rotate-270", "Rotate270"),
    ("auto-rotate", "AutoRotate"),
    ("minimal-gui", "MenuToggle"),
    ("full-screen", "FullScreen"),
    ("undock-reader", "UndockReader"),
    ("magnifier", "Zoom"),
    ("about", "About"),
    ("_sub:bookmarks", "Bookmark"),
];

/// The WinForms mnemonic `&` of the Designer texts, stripped for
/// display (the C# VISIBLE text has no `&`; the port has no
/// mnemonic-activation chain — the table keeps the texts verbatim).
pub fn strip_amp(label: &str) -> String {
    label.replace('&', "")
}

/// The displayed accelerator text (`ShortcutKeyDisplayString`
/// parity): `<Control><Shift>x` → "Ctrl+Shift+X", `F5` → "F5".
pub fn accel_display(accel: &str) -> String {
    let mut out = String::new();
    let mut rest = accel;
    while let Some(open) = rest.find('<') {
        let Some(close) = rest[open..].find('>') else {
            break;
        };
        match &rest[open + 1..open + close] {
            "Control" => out.push_str("Ctrl+"),
            "Shift" => out.push_str("Shift+"),
            "Alt" => out.push_str("Alt+"),
            _ => {}
        }
        rest = &rest[open + close + 1..];
    }
    let key: &str = match rest {
        "plus" => "+",
        "minus" => "\u{2212}",
        "equal" => "=",
        other => other,
    };
    if key.chars().count() == 1 {
        out.push_str(&key.to_ascii_uppercase());
    } else {
        out.push_str(key);
    }
    out
}

/// One syncable item row (plain items AND the leaf rows inside
/// submenus; the submenu buttons themselves are not activatable).
pub struct ItemRow {
    /// The action registry key, e.g. "page-fit". Empty for dynamic
    /// rows (they bake their own check/disabled state at fill time —
    /// the C# refreshes them at DropDownOpening, not through the
    /// command states).
    base: &'static str,
    /// The radio value for parametered targets.
    value: Option<&'static str>,
    /// The full detailed action ("win.next-page") — the probe's
    /// row-click path keys on it. Dynamic rows carry dynamic targets
    /// (slot ids, paths).
    action: String,
    button: gtk4::Button,
    indicator: gtk4::Image,
}

/// One dynamic fill item (the `DropDownOpening` ToolStripMenuItem).
pub struct DynItem {
    pub label: String,
    /// The full detailed action ("win.open-tab::3").
    pub action: String,
    /// The displayed accelerator ("" = none).
    pub accel: String,
    /// The resx icon ("" = none).
    pub icon: &'static str,
    pub checked: bool,
    pub enabled: bool,
}

/// A dynamic fill node.
pub enum DynNode {
    Item(DynItem),
    Sep,
}

/// The fill provider: id → the rows for that slot (the shell owns
/// the book/tabs context).
pub type DynFillFn = Rc<dyn Fn(&str) -> Vec<DynNode>>;

/// One dynamic slot's rows (rebuilt on every menu open; the rows
/// share the click path with the static ones).
struct DynSlot {
    id: &'static str,
    /// The owning top menu (refresh on its open).
    top: usize,
    container: gtk4::Box,
    rows: RefCell<Vec<ItemRow>>,
}

/// The dynamic-fill context (shared with every widget clone).
struct MenubarDyn {
    window: gtk4::ApplicationWindow,
    slots: RefCell<Vec<DynSlot>>,
    fill: RefCell<Option<DynFillFn>>,
}

impl MenubarDyn {
    /// Rebuilds every slot of one top menu (`DropDownOpening`
    /// parity — the fill runs BEFORE the popover maps, so the
    /// checked/disabled state is fresh).
    fn refresh_top(&self, top: usize) {
        let fill = self.fill.borrow().clone();
        let Some(fill) = fill else {
            return;
        };
        let slots = self.slots.borrow();
        for slot in slots.iter().filter(|s| s.top == top) {
            self.rebuild_slot(slot, &fill);
        }
    }

    /// Rebuilds ONE slot (the submenu-map hook: revisiting the
    /// submenu inside an already-open menu must re-fill too — the
    /// C# rebuilds at every `DropDownOpening`, and a nested
    /// ToolStripDropDownItem opening is one).
    fn refresh_slot(&self, id: &str) {
        let fill = self.fill.borrow().clone();
        let Some(fill) = fill else {
            return;
        };
        let slots = self.slots.borrow();
        for slot in slots.iter().filter(|s| s.id == id) {
            self.rebuild_slot(slot, &fill);
        }
    }

    fn rebuild_slot(&self, slot: &DynSlot, fill: &DynFillFn) {
        while let Some(child) = slot.container.first_child() {
            slot.container.remove(&child);
        }
        let mut rows = slot.rows.borrow_mut();
        rows.clear();
        for node in fill(slot.id) {
            match node {
                DynNode::Sep => {
                    let sep = gtk4::Separator::new(gtk4::Orientation::Horizontal);
                    sep.set_margin_top(3);
                    sep.set_margin_bottom(3);
                    slot.container.append(&sep);
                }
                DynNode::Item(item) => {
                    let row = build_dyn_row(&item, &self.window);
                    slot.container.append(&row.button);
                    rows.push(ItemRow {
                        base: "",
                        value: None,
                        action: row.action,
                        button: row.button,
                        indicator: row.indicator,
                    });
                }
            }
        }
    }
}

struct BuiltRow {
    action: String,
    button: gtk4::Button,
    indicator: gtk4::Image,
}

/// Builds one dynamic row: baked check/disabled state (the fill is
/// the state source — the command states never drive these), the
/// full detailed action on click.
fn build_dyn_row(item: &DynItem, window: &gtk4::ApplicationWindow) -> BuiltRow {
    let hbox = dyn_row_content(&item.label, &item.accel, item.icon);
    let button = gtk4::Button::builder()
        .css_classes(["flat", "menu-row"])
        .child(&hbox)
        .build();
    button.set_sensitive(item.enabled);
    button.set_halign(gtk4::Align::Fill);
    // Baked check mark (the fill ran at open — the C# DropDownOpening
    // shape; a reopened menu refreshes).
    let indicator = hbox_indicator(&hbox);
    if item.checked {
        indicator.set_icon_name(Some("object-select-symbolic"));
    }
    // Click → close the popover, fire the full detailed action
    // (the T3 round-2 lesson: the FULL "win." form). The value rides
    // as the explicit parameter (detailed + args errors silently —
    // the T4 probe lesson).
    {
        let window = window.clone();
        let (bare, value) = match item.action.split_once("::") {
            Some((b, v)) => (b.to_string(), Some(v.to_string())),
            None => (item.action.clone(), None),
        };
        button.connect_clicked(move |btn| {
            if let Some(popover) = btn
                .ancestor(gtk4::Popover::static_type())
                .and_downcast::<gtk4::Popover>()
            {
                popover.popdown();
            }
            let variant = value
                .as_ref()
                .map(|v| gtk4::glib::Variant::from(v.as_str()));
            let _ =
                gtk4::prelude::WidgetExt::activate_action(&window, bare.as_str(), variant.as_ref());
        });
    }
    BuiltRow {
        action: item.action.clone(),
        button,
        indicator,
    }
}

/// The dynamic row content ([check slot][icon][label][accel]) — the
/// owned-string shape of `row_content`. The indicator lives in the
/// hbox's first slot; `hbox_indicator` reaches it.
fn dyn_row_content(label: &str, accel: &str, icon: &'static str) -> gtk4::Box {
    let hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    hbox.set_width_request(250);
    let indicator_slot = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    indicator_slot.set_width_request(16);
    let indicator = gtk4::Image::new();
    indicator.set_pixel_size(16);
    indicator_slot.append(&indicator);
    hbox.append(&indicator_slot);
    let icon_widget = gtk4::Image::new();
    icon_widget.set_pixel_size(16);
    if !icon.is_empty() {
        if let Some(texture) = crate::icon::icon(icon) {
            icon_widget.set_paintable(Some(&texture));
        }
    }
    hbox.append(&icon_widget);
    let label_widget = gtk4::Label::builder()
        .label(strip_amp(label))
        .halign(gtk4::Align::Start)
        .build();
    hbox.append(&label_widget);
    let accel_widget = gtk4::Label::builder()
        .label(accel_display(accel))
        .halign(gtk4::Align::End)
        .hexpand(true)
        .css_classes(["dim-label"])
        .build();
    hbox.append(&accel_widget);
    hbox
}

/// The check indicator of a built dynamic row (the first 16 px Image
/// inside the row's hbox).
fn hbox_indicator(hbox: &gtk4::Box) -> gtk4::Image {
    let slot = hbox
        .first_child()
        .and_then(|w| w.downcast::<gtk4::Box>().ok())
        .expect("row hbox check slot");
    slot.first_child()
        .and_downcast::<gtk4::Image>()
        .expect("indicator image")
}

/// The label text of a row button (the probe snapshot). The row
/// hbox children: [check slot][icon][label][accel].
fn row_label(button: &gtk4::Button) -> String {
    button
        .child()
        .and_then(|c| widget_label(&c))
        .unwrap_or_default()
}

/// The action view the host resolves per base name.
pub struct ActionState {
    pub enabled: bool,
    pub state: Option<glib::Variant>,
    /// The ACTIVE emphasis (the C# `miViewLibrary`/`miViewPages`
    /// shape: the selected panel highlights the row's icon instead
    /// of a check mark — the C# has no checkbox on these).
    pub highlight: bool,
    /// Visibility (the `fileMenu_DropDownOpening` hide rule — e.g.
    /// "Update all Book Files" hides while `AutoUpdateComicsFiles`
    /// is on).
    pub visible: bool,
}

/// One top-level menu (the bar's flat row).
struct TopMenu {
    button: gtk4::Button,
    popover: gtk4::Popover,
}

/// The bar's open-menu state — the `GtkPopoverMenuBar.active_item`
/// equivalent. ONE slot: on Wayland a grabbing popup may only map
/// when no other grabbing popup is up (`can_map_grabbing_popup`,
/// `gdkpopup-wayland.c:904`) — a second present while one popover
/// holds the grab fails to map BUT the seat grab stays, freezing
/// all input. So the bar NEVER presents a second popover without
/// popping the open one down first.
type ActiveSlot = Rc<Cell<Option<usize>>>;

/// The menubar widget: the flat top row + every popover's rows
/// (kept for the state sync). Shared through `Rc` by the shell and
/// the probes.
pub struct MenubarWidget {
    bar: gtk4::Box,
    tops: Vec<TopMenu>,
    /// Shared with every clone/handle (the sync and the row-click
    /// path both walk it).
    rows: Rc<Vec<ItemRow>>,
    /// The submenu parent rows by label (without the mnemonic) —
    /// the parent enable-state sync.
    subs: Rc<RefCell<Vec<(String, gtk4::MenuButton)>>>,
    /// The dynamic fill slots + provider (shared with every clone —
    /// the probe clicks the rebuilt rows through the handle).
    dyn_ctx: Rc<MenubarDyn>,
    active: ActiveSlot,
}

impl Clone for MenubarWidget {
    fn clone(&self) -> Self {
        Self {
            bar: self.bar.clone(),
            tops: self
                .tops
                .iter()
                .map(|t| TopMenu {
                    button: t.button.clone(),
                    popover: t.popover.clone(),
                })
                .collect(),
            rows: Rc::clone(&self.rows),
            subs: Rc::clone(&self.subs),
            dyn_ctx: Rc::clone(&self.dyn_ctx),
            active: Rc::clone(&self.active),
        }
    }
}

impl MenubarWidget {
    /// A handle for the probes (the rows are shared, so the handle
    /// clicks and syncs like the original).
    pub fn clone_handle(&self) -> MenubarWidget {
        self.clone()
    }

    pub fn widget(&self) -> &gtk4::Box {
        &self.bar
    }

    /// The number of top menus.
    pub fn top_count(&self) -> usize {
        self.tops.len()
    }

    /// Opens a top menu programmatically (the probe; GTK4 cannot
    /// open a model menubar from code — the custom shape can). The
    /// dynamic slots of that menu refresh first.
    pub fn open_top(&self, index: usize) {
        self.dyn_ctx.refresh_top(index);
        set_active_item(&self.tops, &self.active, index);
    }

    /// Emulates a row CLICK through the real widget path
    /// (`emit_clicked` → the row's handler → popdown +
    /// `activate_action`) — the probe proof that menu rows fire
    /// actions (the round-2 bug class: accels fired, clicks did
    /// not). No-op for an unknown action.
    pub fn click_row(&self, action: &str) {
        for row in self.rows.iter() {
            if row.action == action {
                row.button.emit_clicked();
                return;
            }
        }
        for slot in self.dyn_ctx.slots.borrow().iter() {
            for row in slot.rows.borrow().iter() {
                if row.action == action {
                    row.button.emit_clicked();
                    return;
                }
            }
        }
    }

    /// Installs the dynamic fill provider (`DropDownOpening`
    /// rebuilds).
    pub fn set_dyn_fill(&self, fill: DynFillFn) {
        *self.dyn_ctx.fill.borrow_mut() = Some(fill);
    }

    /// Re-fills ONE dynamic slot (the probe gate for the
    /// submenu-map hook; the widget path runs it on the child
    /// popover's map).
    pub fn refresh_dyn_slot(&self, id: &str) {
        self.dyn_ctx.refresh_slot(id);
    }

    /// Enables/disables a submenu PARENT row (the C# enables the
    /// parent with the fill: `miOpenNow.Enabled`, the Page Type/
    /// Rotation `EnumMenuUtility.Enabled`). `label` is the menu
    /// label without the `_` mnemonic.
    pub fn set_sub_enabled(&self, label: &str, enabled: bool) {
        let hit = self
            .subs
            .borrow()
            .iter()
            .find(|(key, _)| *key == label)
            .map(|(_, b)| b.clone());
        if let Some(btn) = hit {
            btn.set_sensitive(enabled);
        }
    }

    /// Applies the action states: check marks (checks and radio
    /// targets), disabled graying, and the visibility rule.
    /// `resolve` maps an action base name to (enabled, state).
    pub fn sync(&self, resolve: &dyn Fn(&str) -> Option<ActionState>) {
        for row in self.rows.iter() {
            sync_row(row, resolve);
        }
        for slot in self.dyn_ctx.slots.borrow().iter() {
            for row in slot.rows.borrow().iter() {
                sync_row(row, resolve);
            }
        }
    }

    /// Whether a row currently carries the active emphasis (the
    /// probe assertion).
    pub fn is_row_highlighted(&self, action: &str) -> bool {
        self.rows
            .iter()
            .find(|row| row.action == action)
            .is_some_and(|row| row.button.has_css_class("menu-row-active"))
    }

    /// The dynamic slot rows for one id: (label, checked, enabled) —
    /// the probe evidence for the fills (a probe calls open_top
    /// first, which rebuilds).
    pub fn dyn_rows_snapshot(&self, id: &str) -> Vec<(String, bool, bool)> {
        let slots = self.dyn_ctx.slots.borrow();
        let mut out = Vec::new();
        for slot in slots.iter().filter(|s| s.id == id) {
            for row in slot.rows.borrow().iter() {
                let checked = row
                    .indicator
                    .icon_name()
                    .is_some_and(|n| n == "object-select-symbolic");
                out.push((row_label(&row.button), checked, row.button.is_sensitive()));
            }
        }
        out
    }

    /// Every RENDERED label: static item rows, submenu parents, and
    /// the dynamic slots (the probe's no-`&` gate).
    pub fn all_row_labels(&self) -> Vec<String> {
        let mut out: Vec<String> = self.rows.iter().map(|r| row_label(&r.button)).collect();
        let subs = self.subs.borrow();
        for (_, mb) in subs.iter() {
            if let Some(l) = mb.child().and_then(|c| widget_label(&c)) {
                out.push(l);
            }
        }
        for slot in self.dyn_ctx.slots.borrow().iter() {
            for row in slot.rows.borrow().iter() {
                out.push(row_label(&row.button));
            }
        }
        out
    }

    /// The six top-menu popovers (the probe's arrow gate).
    pub fn top_popovers(&self) -> Vec<gtk4::Popover> {
        self.tops.iter().map(|t| t.popover.clone()).collect()
    }
}

/// The label text of a row hbox ([check slot][icon][label][...]) —
/// shared by the Button and MenuButton row shapes.
fn widget_label(w: &gtk4::Widget) -> Option<String> {
    let hbox = w.downcast_ref::<gtk4::Box>()?;
    hbox.first_child()?
        .next_sibling()?
        .next_sibling()?
        .downcast::<gtk4::Label>()
        .ok()
        .map(|l| l.text().to_string())
}

/// A standalone dropdown menu (the T5 toolbar's split-button drops):
/// the same row builder + state sync as the menubar popovers, one
/// popover per instance.
pub struct Dropdown {
    popover: gtk4::Popover,
    rows: Rc<Vec<ItemRow>>,
    dyn_ctx: Rc<MenubarDyn>,
}

impl Clone for Dropdown {
    fn clone(&self) -> Self {
        Self {
            popover: self.popover.clone(),
            rows: Rc::clone(&self.rows),
            dyn_ctx: Rc::clone(&self.dyn_ctx),
        }
    }
}

impl Dropdown {
    pub fn popover(&self) -> &gtk4::Popover {
        &self.popover
    }

    /// Opens the popover below `button` (the fill refresh runs
    /// first — the top-level slots of a standalone dropdown). The
    /// popover parents to the anchor button on first open: a
    /// popover without a toplevel parent realizes nothing and the
    /// popup segfaults (`gdk_surface_new_popup: no parent surface`).
    /// Parenting to the BUTTON (not the window) keeps it right when
    /// the toolbar re-parents across windows (the undock).
    pub fn open(&self, button: &gtk4::Button) {
        self.open_at(button);
    }

    /// Opens the popover anchored at ANY widget (the T6 browser
    /// toolbar's search scope icon, the Detail header column
    /// chooser): parents to the anchor on first open, points below
    /// it, refreshes the top-level dynamic slots.
    pub fn open_at(&self, anchor: &impl IsA<gtk4::Widget>) {
        if self.popover.parent().is_none() {
            self.popover.set_parent(anchor);
        }
        self.dyn_ctx.refresh_top(0);
        align_below_widget(anchor.as_ref(), &self.popover);
        self.popover.popup();
    }

    /// Applies the action states to the rows (checks/disabled/
    /// highlight — the shell resolves the same states as the
    /// menubar's).
    pub fn sync(&self, resolve: &dyn Fn(&str) -> Option<ActionState>) {
        for row in self.rows.iter() {
            sync_row(row, resolve);
        }
        let slots = self.dyn_ctx.slots.borrow();
        for slot in slots.iter() {
            for row in slot.rows.borrow().iter() {
                sync_row(row, resolve);
            }
        }
    }

    /// Installs the dynamic fill provider (the shell shares its
    /// provider with the menubar).
    pub fn set_dyn_fill(&self, fill: DynFillFn) {
        *self.dyn_ctx.fill.borrow_mut() = Some(fill);
    }

    /// Re-fills ONE dynamic slot (the probe gate for the map hook).
    pub fn refresh_slot(&self, id: &str) {
        self.dyn_ctx.refresh_slot(id);
    }

    /// Probe: clicks a row through the real widget path. Returns
    /// whether the action exists in this dropdown.
    pub fn click_row(&self, action: &str) -> bool {
        for row in self.rows.iter() {
            if row.action == action {
                row.button.emit_clicked();
                return true;
            }
        }
        let slots = self.dyn_ctx.slots.borrow();
        for slot in slots.iter() {
            for row in slot.rows.borrow().iter() {
                if row.action == action {
                    row.button.emit_clicked();
                    return true;
                }
            }
        }
        false
    }

    /// Probe: the rows of one dynamic slot (label, checked, enabled).
    pub fn dyn_rows_snapshot(&self, id: &str) -> Vec<(String, bool, bool)> {
        let slots = self.dyn_ctx.slots.borrow();
        let mut out = Vec::new();
        for slot in slots.iter().filter(|s| s.id == id) {
            for row in slot.rows.borrow().iter() {
                let checked = row
                    .indicator
                    .icon_name()
                    .is_some_and(|n| n == "object-select-symbolic");
                out.push((row_label(&row.button), checked, row.button.is_sensitive()));
            }
        }
        out
    }

    /// Every RENDERED label: static rows + the dynamic slots (the
    /// probe's no-`&` gate).
    pub fn row_labels(&self) -> Vec<String> {
        let mut out: Vec<String> = self.rows.iter().map(|r| row_label(&r.button)).collect();
        for slot in self.dyn_ctx.slots.borrow().iter() {
            for row in slot.rows.borrow().iter() {
                out.push(row_label(&row.button));
            }
        }
        out
    }
}

/// Applies the action states to ONE row (checks, radio marks,
/// disabled graying, visibility, the active emphasis) — shared by
/// the menubar and the standalone dropdowns.
fn sync_row(row: &ItemRow, resolve: &dyn Fn(&str) -> Option<ActionState>) {
    if row.base.is_empty() {
        return;
    }
    let Some(view) = resolve(row.base) else {
        return;
    };
    row.button.set_visible(view.visible);
    row.button.set_sensitive(view.enabled);
    let checked = match row.value {
        Some(value) => view
            .state
            .as_ref()
            .and_then(|s| s.get::<String>())
            .is_some_and(|s| s == value),
        None => view
            .state
            .as_ref()
            .and_then(|s| s.get::<bool>())
            .unwrap_or(false),
    };
    if checked {
        row.indicator.set_icon_name(Some("object-select-symbolic"));
    } else {
        row.indicator.set_icon_name(None);
    }
    // The active-panel emphasis (view-library/view-pages):
    // a row highlight, never a check mark (CR parity).
    if view.highlight {
        row.button.add_css_class("menu-row-active");
    } else {
        row.button.remove_css_class("menu-row-active");
    }
}

/// Builds a standalone dropdown from a node table (the toolbar's
/// split-button menus).
pub fn build_dropdown(defs: &[MenuNode], window: &gtk4::ApplicationWindow) -> Dropdown {
    let dyn_ctx = Rc::new(MenubarDyn {
        window: window.clone(),
        slots: RefCell::new(Vec::new()),
        fill: RefCell::new(None),
    });
    let mut rows = Vec::new();
    let mut subs = Vec::new();
    let mut child_popovers = Vec::new();
    let (content, _first, _top_dyn) = build_menu_content(
        defs,
        window,
        &mut rows,
        &mut child_popovers,
        &mut subs,
        &dyn_ctx,
        0,
    );
    let popover = gtk4::Popover::new();
    popover.set_child(Some(&content));
    popover.set_has_arrow(false);
    popover.set_position(gtk4::PositionType::Bottom);
    popover.set_size_request(POP_WIDTH, -1);
    for child in child_popovers.iter() {
        let child = child.clone();
        popover.connect_closed(move |_| child.popdown());
    }
    Dropdown {
        popover,
        rows: Rc::new(rows),
        dyn_ctx,
    }
}

/// The row widget: [check slot 16px] [icon 16px] [label] [accel |
/// submenu arrow].
fn row_content(
    label: &'static str,
    accel: &'static str,
    icon: &'static str,
    with_arrow: bool,
) -> (gtk4::Box, gtk4::Image) {
    let hbox = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    hbox.set_width_request(250);
    let indicator_slot = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    indicator_slot.set_width_request(16);
    let indicator = gtk4::Image::new();
    indicator.set_pixel_size(16);
    indicator_slot.append(&indicator);
    hbox.append(&indicator_slot);
    let icon_widget = gtk4::Image::new();
    icon_widget.set_pixel_size(16);
    if !icon.is_empty() {
        if let Some(texture) = crate::icon::icon(icon) {
            icon_widget.set_paintable(Some(&texture));
        }
    }
    hbox.append(&icon_widget);
    let label_widget = gtk4::Label::builder()
        .label(strip_amp(label))
        .use_underline(true)
        .halign(gtk4::Align::Start)
        .build();
    hbox.append(&label_widget);
    if with_arrow {
        let arrow = gtk4::Image::from_icon_name("pan-end-symbolic");
        arrow.set_halign(gtk4::Align::End);
        arrow.set_hexpand(true);
        hbox.append(&arrow);
    } else {
        let accel_widget = gtk4::Label::builder()
            .label(accel_display(accel))
            .halign(gtk4::Align::End)
            .hexpand(true)
            .css_classes(["dim-label"])
            .build();
        hbox.append(&accel_widget);
    }
    (hbox, indicator)
}

/// Up/Down moves focus through this level's rows (the model menus'
/// arrow navigation; Enter/Space activate the focused button).
fn install_row_nav(content: &gtk4::Box, buttons: Vec<gtk4::Widget>) {
    if buttons.is_empty() {
        return;
    }
    let controller = gtk4::EventControllerKey::new();
    controller.connect_key_pressed(move |_c, key, _code, _mods| {
        let step: isize = match key {
            gtk4::gdk::Key::Up => -1,
            gtk4::gdk::Key::Down => 1,
            _ => return glib::Propagation::Proceed,
        };
        let n = buttons.len() as isize;
        let target = match buttons.iter().position(|b| b.has_focus()) {
            Some(i) => (i as isize + step).rem_euclid(n) as usize,
            None => 0,
        };
        let _ = buttons[target].grab_focus();
        glib::Propagation::Stop
    });
    content.add_controller(controller);
}

/// Builds one menu level: rows + separators + submenu buttons.
/// Returns (content, first focusable row, the dyn ids registered at
/// THIS level — the caller hooks them to the popover that hosts
/// this content).
fn build_menu_content(
    defs: &[MenuNode],
    window: &gtk4::ApplicationWindow,
    rows: &mut Vec<ItemRow>,
    child_popovers: &mut Vec<gtk4::Popover>,
    subs: &mut Vec<(String, gtk4::MenuButton)>,
    dyn_ctx: &Rc<MenubarDyn>,
    top: usize,
) -> (gtk4::Box, Option<gtk4::Widget>, Vec<&'static str>) {
    let content = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    let mut nav: Vec<gtk4::Widget> = Vec::new();
    let mut my_dyn_ids: Vec<&'static str> = Vec::new();
    for node in defs {
        match node {
            MenuNode::Sep => {
                let sep = gtk4::Separator::new(gtk4::Orientation::Horizontal);
                sep.set_margin_top(3);
                sep.set_margin_bottom(3);
                content.append(&sep);
            }
            MenuNode::Item(label, action, accel, icon) => {
                let (hbox, indicator) = row_content(label, accel, icon, false);
                let button = gtk4::Button::builder()
                    .css_classes(["flat", "menu-row"])
                    .child(&hbox)
                    .build();
                button.set_halign(gtk4::Align::Fill);
                // Click → close the popover, fire the action (a
                // custom row has no model auto-close). The action
                // name keeps its FULL "win." group form — GTK
                // resolves actions through action GROUPS; a stripped
                // "next-page" finds no group and silently fails
                // (the T3 round-2 bug: accels worked, clicks did
                // not). Radio targets split at "::": the BARE name
                // plus the value as the explicit parameter —
                // `activate_action` parses a detailed name ONLY when
                // no args ride along; detailed + args errors and
                // the click dies silently (the T4 probe caught the
                // T3 radio rows dead on click).
                {
                    let window = window.clone();
                    let (bare, value) = match action.split_once("::") {
                        Some((b, v)) => ((*b).to_string(), Some(v.to_string())),
                        None => ((*action).to_string(), None),
                    };
                    button.connect_clicked(move |btn| {
                        if let Some(popover) = btn
                            .ancestor(gtk4::Popover::static_type())
                            .and_downcast::<gtk4::Popover>()
                        {
                            popover.popdown();
                        }
                        let variant = value
                            .as_ref()
                            .map(|v| gtk4::glib::Variant::from(v.as_str()));
                        let _ = gtk4::prelude::WidgetExt::activate_action(
                            &window,
                            bare.as_str(),
                            variant.as_ref(),
                        );
                    });
                }
                content.append(&button);
                nav.push(button.clone().upcast());
                let (base, value) = match action.split_once("::") {
                    Some((b, v)) => (b.strip_prefix("win.").unwrap_or(b), Some(v)),
                    None => (action.strip_prefix("win.").unwrap_or(action), None),
                };
                rows.push(ItemRow {
                    base,
                    value,
                    action: (*action).to_string(),
                    button,
                    indicator,
                });
            }
            MenuNode::Sub(label, children) => {
                // A nested MenuButton row: GTK places the child
                // popover and holds the grab; the arrow marks the
                // submenu (the C# submenu arrow). `has_frame(false)`
                // removes the MenuButton outline (the flat class
                // does not reach the inner toggle button).
                let (hbox, _indicator) = row_content(label, "", "", true);
                let sub = gtk4::MenuButton::builder()
                    .css_classes(["flat", "menu-row"])
                    .build();
                sub.set_has_frame(false);
                sub.set_child(Some(&hbox));
                sub.set_halign(gtk4::Align::Fill);
                let child_popover = gtk4::Popover::new();
                child_popover.set_position(gtk4::PositionType::Right);
                child_popover.set_has_arrow(false);
                let (child_content, _child_first, child_dyn) =
                    build_menu_content(children, window, rows, child_popovers, subs, dyn_ctx, top);
                child_popover.set_child(Some(&child_content));
                sub.set_popover(Some(&child_popover));
                // The dynamic slots the CHILD content registered:
                // re-fill on the child popover's own map (the user
                // revisits the submenu inside an already-open menu —
                // the top-menu funnel never fires for it).
                for id in child_dyn {
                    let dyn_ctx = Rc::clone(dyn_ctx);
                    child_popover.connect_map(move |_| {
                        dyn_ctx.refresh_slot(id);
                    });
                }
                child_popovers.push(child_popover);
                content.append(&sub);
                nav.push(sub.clone().upcast());
                // The parent registry (set_sub_enabled keys on the
                // label without the mnemonics — `_` AND the C# `&`).
                subs.push((label.replace(['&', '_'], ""), sub));
            }
            MenuNode::Dyn(id) => {
                // The fill container: the provider rebuilds it at
                // every menu open (`DropDownOpening`).
                let container = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
                content.append(&container);
                dyn_ctx.slots.borrow_mut().push(DynSlot {
                    id,
                    top,
                    container,
                    rows: RefCell::new(Vec::new()),
                });
                my_dyn_ids.push(id);
            }
        }
    }
    install_row_nav(&content, nav);
    let first = content.first_child();
    (content, first, my_dyn_ids)
}

fn popover_with(
    defs: &[MenuNode],
    window: &gtk4::ApplicationWindow,
    top: usize,
    subs: &mut Vec<(String, gtk4::MenuButton)>,
    dyn_ctx: &Rc<MenubarDyn>,
) -> (gtk4::Popover, Vec<ItemRow>) {
    let mut rows = Vec::new();
    let mut child_popovers = Vec::new();
    let (content, first, _top_dyn) = build_menu_content(
        defs,
        window,
        &mut rows,
        &mut child_popovers,
        subs,
        dyn_ctx,
        top,
    );
    let popover = gtk4::Popover::new();
    popover.set_child(Some(&content));
    // No pointing arrow, fixed width (the C# ToolStrip drop-down
    // shape): the window width is deterministic, which the
    // left-edge alignment below relies on.
    popover.set_has_arrow(false);
    popover.set_position(gtk4::PositionType::Bottom);
    popover.set_size_request(POP_WIDTH, -1);
    // Closing the parent hides the child popovers (they are native
    // windows — no automatic chain).
    for child in child_popovers.iter() {
        let child = child.clone();
        popover.connect_closed(move |_| child.popdown());
    }
    // Reveal focuses the first row (the C# `Items[0].Select()`
    // reveal step) — via an idle AFTER the map sequence completes:
    // an explicit grab_focus inside the map callback would run in
    // the middle of the Wayland grab setup.
    if let Some(first) = first {
        popover.connect_map(move |_| {
            let first = first.clone();
            glib::idle_add_local_once(move || {
                let _ = first.grab_focus();
            });
        });
    }
    (popover, rows)
}

/// The menu drop-down width (the C# ToolStrip drop-downs size to
/// their widest item; 274 = the 250 px row + default 12 px popover
/// padding each side).
const POP_WIDTH: i32 = 274;

/// Points the popover at a POP_WIDTH-wide rect starting at the
/// button's LEFT edge: GTK centers the popover on the rect's
/// center, so the popover's left edge lands exactly on the
/// button's left edge (the WinForms drop-down alignment). The
/// rect's y sits at the button's bottom edge.
fn align_below_widget(button: &gtk4::Widget, popover: &gtk4::Popover) {
    let alloc = button.allocation();
    let rect = gtk4::gdk::Rectangle::new(0, alloc.height(), POP_WIDTH, 1);
    popover.set_pointing_to(Some(&rect));
}

/// The `GtkPopoverMenuBar.set_active_item` state machine: pop every
/// OTHER open popover down FIRST, then present the target (the only
/// Wayland-safe order — a grabbing popup may only map when no other
/// grabbing popup is up; a failed map leaves the seat grab live and
/// freezes input). Hover/keys/open_top route through here; the
/// click handler adds the toggle-close.
fn set_active_item(tops: &[TopMenu], active: &ActiveSlot, index: usize) {
    for (i, top) in tops.iter().enumerate() {
        if i != index && top.popover.is_mapped() {
            top.popover.popdown();
        }
    }
    let Some(top) = tops.get(index) else {
        return;
    };
    align_below_widget(top.button.upcast_ref(), &top.popover);
    active.set(Some(index));
    top.popover.popup();
}

/// Builds the menubar (call once per browser window).
pub fn create_menubar(window: &gtk4::ApplicationWindow) -> MenubarWidget {
    let bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 0);
    bar.add_css_class("menubar");
    bar.set_halign(gtk4::Align::Start);
    let active: ActiveSlot = Rc::new(Cell::new(None));
    let mut rows = Vec::new();
    let mut subs: Vec<(String, gtk4::MenuButton)> = Vec::new();
    let dyn_ctx = Rc::new(MenubarDyn {
        window: window.clone(),
        slots: RefCell::new(Vec::new()),
        fill: RefCell::new(None),
    });
    let mut tops: Vec<TopMenu> = Vec::new();
    for (top, (label, defs)) in MENUS.iter().enumerate() {
        let button = gtk4::Button::new();
        let lbl = gtk4::Label::builder()
            .label(*label)
            .use_underline(true)
            .build();
        button.set_child(Some(&lbl));
        button.set_css_classes(&["flat"]);
        let (popover, menu_rows) = popover_with(defs, window, top, &mut subs, &dyn_ctx);
        // Explicit parenting (the MenuButton toggle semantics are
        // what made parallel presents possible).
        popover.set_parent(&button);
        rows.extend(menu_rows);
        // Any close clears the slot (guarded — a late close of the
        // OLD popover must not clear a NEW one's slot).
        {
            let active = Rc::clone(&active);
            popover.connect_closed(move |_| {
                let _ = active;
            });
        }
        bar.append(&button);
        tops.push(TopMenu { button, popover });
    }
    // Wire the state machine now that every TopMenu exists.
    let shared_tops: Rc<Vec<TopMenu>> = Rc::new(
        tops.iter()
            .map(|t| TopMenu {
                button: t.button.clone(),
                popover: t.popover.clone(),
            })
            .collect(),
    );
    for (index, (label, defs)) in MENUS.iter().enumerate() {
        let _ = (label, defs);
        // Click: open or toggle-close (the WinForms MenuStrip).
        {
            let tops = Rc::clone(&shared_tops);
            let active = Rc::clone(&active);
            let dyn_ctx = Rc::clone(&dyn_ctx);
            shared_tops[index].button.connect_clicked(move |_| {
                let current = active.get();
                if current == Some(index) {
                    if let Some(top) = tops.get(index) {
                        top.popover.popdown();
                    }
                } else {
                    dyn_ctx.refresh_top(index);
                    set_active_item(&tops, &active, index);
                }
            });
        }
        // Hover switching: while a menu is open, entering another
        // top button switches (the WinForms strip behavior). When
        // NO menu is open, hover does nothing (click opens).
        {
            let tops = Rc::clone(&shared_tops);
            let active = Rc::clone(&active);
            let dyn_ctx = Rc::clone(&dyn_ctx);
            let motion = gtk4::EventControllerMotion::new();
            motion.connect_enter(move |_, _, _| {
                let current = active.get();
                if current.is_some() && current != Some(index) {
                    dyn_ctx.refresh_top(index);
                    set_active_item(&tops, &active, index);
                }
            });
            shared_tops[index].button.add_controller(motion);
        }
        // Left/Right switches top menus while one is open (the key
        // controller lives on each popover — a popover is its own
        // native surface, the bar never sees its keys).
        {
            let tops = Rc::clone(&shared_tops);
            let active = Rc::clone(&active);
            let dyn_ctx = Rc::clone(&dyn_ctx);
            let controller = gtk4::EventControllerKey::new();
            controller.connect_key_pressed(move |_c, key, _code, _mods| {
                if !matches!(key, gtk4::gdk::Key::Left | gtk4::gdk::Key::Right) {
                    return glib::Propagation::Proceed;
                }
                let Some(idx) = active.get() else {
                    return glib::Propagation::Proceed;
                };
                let step = if key == gtk4::gdk::Key::Left { -1 } else { 1 };
                let next = (idx as isize + step).rem_euclid(tops.len() as isize) as usize;
                dyn_ctx.refresh_top(next);
                set_active_item(&tops, &active, next);
                glib::Propagation::Stop
            });
            shared_tops[index].popover.add_controller(controller);
        }
    }
    // Any popover close clears the slot (guarded against a stale
    // OLD popover's late close clearing a NEW one).
    for (index, top) in shared_tops.iter().enumerate() {
        let active = Rc::clone(&active);
        top.popover.connect_closed(move |_| {
            if active.get() == Some(index) {
                active.set(None);
            }
        });
    }
    MenubarWidget {
        bar,
        tops: shared_tops
            .iter()
            .map(|t| TopMenu {
                button: t.button.clone(),
                popover: t.popover.clone(),
            })
            .collect(),
        rows: Rc::new(rows),
        subs: Rc::new(RefCell::new(subs)),
        dyn_ctx,
        active,
    }
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

    /// Walks every Item node of the whole table.
    fn all_items() -> Vec<(&'static str, &'static str, &'static str)> {
        let mut out = Vec::new();
        for (_, defs) in MENUS {
            for node in defs.iter() {
                if let Item(label, action, accel, _) = node {
                    out.push((*label, *action, *accel));
                }
            }
        }
        for defs in [RATING, BOOKMARKS, PAGE_LAYOUT, ZOOM, ROTATION] {
            for node in defs {
                if let Item(label, action, accel, _) = node {
                    out.push((*label, *action, *accel));
                }
            }
        }
        out
    }

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
                // The T4 dynamic fill targets.
                "open-tab",
                "recent-book",
                "open-bookmark",
                "page-type",
                "page-rotation",
            ])
            .collect();
        for (_, action, _) in all_items() {
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
        for (_, action, accel) in all_items() {
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
        }
    }

    /// The ADR-024 omissions stay out of the skeleton. The T4
    /// dynamic parents (Open Books, Recent Books, Page Type, Page
    /// Rotation) are PRESENT (the fill slots assert below). The
    /// Automation submenu is absent permanently (ADR-027 — no
    /// scripting host).
    #[test]
    fn omitted_items_are_absent() {
        let all_labels: Vec<String> = MENUS
            .iter()
            .flat_map(|(_, defs)| defs.iter())
            .map(|node| match node {
                Item(label, _, _, _) => (*label).to_string(),
                Sub(label, _) => (*label).to_string(),
                Sep => String::new(),
                Dyn(_) => String::new(),
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
            "Devices...",
            "Search Browser",
            "Info Panel",
            "Workspaces",
            "List Layout",
            "Update Web Comics",
            "Synchronize Devices",
            "Automation",
            "Open Remote Library",
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

    /// The dynamic fill slots sit exactly where the C# dynamic
    /// submenus live (the ids the shell's fill provider serves).
    #[test]
    fn dyn_slots_match_the_csharp_dynamic_fills() {
        let mut found: Vec<&str> = Vec::new();
        for (_, defs) in MENUS {
            for node in defs.iter() {
                match node {
                    Sub(_, children) => {
                        for child in children.iter() {
                            if let Dyn(id) = child {
                                found.push(id);
                            }
                        }
                    }
                    Dyn(id) => found.push(id),
                    _ => {}
                }
            }
        }
        found.sort_unstable();
        assert_eq!(
            found,
            [
                "bookmarks",
                "open-books",
                "page-rotation",
                "page-type",
                "recent-books"
            ]
        );
    }

    /// The icon table matches the Designer assignment-for-assignment
    /// (both directions): every C#-ported item that carries an image
    /// has it, no item invents one, and every named icon resolves in
    /// the bundled set.
    #[test]
    fn icons_match_the_designer() {
        let expected: std::collections::HashMap<&str, &str> =
            CSHARP_ITEM_ICONS.iter().copied().collect();
        let mut pairs: Vec<(&str, &str)> = Vec::new();
        for (_, defs) in MENUS {
            for node in defs.iter() {
                if let Item(_, action, _, icon) = node {
                    pairs.push((*action, *icon));
                }
            }
        }
        for defs in [RATING, BOOKMARKS, PAGE_LAYOUT, ZOOM, ROTATION] {
            for node in defs {
                if let Item(_, action, _, icon) = node {
                    pairs.push((*action, *icon));
                }
            }
        }
        for (action, icon) in &pairs {
            let key = action.strip_prefix("win.").unwrap();
            match expected.get(key) {
                Some(want) => assert_eq!(
                    *icon, *want,
                    "icon drift for {action}: table says '{icon}', Designer says '{want}'"
                ),
                None => assert!(
                    icon.is_empty(),
                    "item {action} invents an icon the C# does not give"
                ),
            }
            if !icon.is_empty() {
                assert!(
                    crate::icon::path_for_name(icon).is_some(),
                    "icon {icon} for {action} is not in the bundled set"
                );
            }
        }
        // Every expected item is present (a dropped C# item would
        // lose its icon silently).
        for key in expected.keys() {
            if let Some(_base) = key.strip_prefix("_sub:") {
                // Submenu parents live in Sub nodes, not Items.
                assert!(
                    MENUS.iter().any(|(_, defs)| defs.iter().any(|n| matches!(
                        n,
                        Sub(label, _) if label.eq_ignore_ascii_case(&format!("_{}", &key[5..]))
                    ))),
                    "submenu parent {key} missing"
                );
                continue;
            }
            assert!(
                pairs.iter().any(|(a, _)| *a == format!("win.{key}")),
                "C# icon for {key} is missing from the table"
            );
        }
    }

    /// The accelerator display text (`ShortcutKeyDisplayString`
    /// parity).
    #[test]
    fn accel_display_format() {
        assert_eq!(accel_display("<Control>o"), "Ctrl+O");
        assert_eq!(accel_display("<Control><Shift>x"), "Ctrl+Shift+X");
        assert_eq!(accel_display("<Alt><Shift>4"), "Alt+Shift+4");
        assert_eq!(accel_display("F5"), "F5");
        assert_eq!(accel_display("<Control>F9"), "Ctrl+F9");
        assert_eq!(accel_display("<Control>equal"), "Ctrl+=");
        assert_eq!(accel_display("<Control>minus"), "Ctrl+\u{2212}");
        assert_eq!(
            accel_display("<Control><Shift>minus"),
            "Ctrl+Shift+\u{2212}"
        );
        assert_eq!(accel_display("<Control><Shift>plus"), "Ctrl+Shift++");
        assert_eq!(accel_display(""), "");
    }
}
