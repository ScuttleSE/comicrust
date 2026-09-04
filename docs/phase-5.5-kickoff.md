# Phase 5.5 Kickoff — UI Chrome Parity

Goal: the app gets the chrome the original ComicRack has — the
menubar, the toolbars, the multi-panel status bar, the book tabs
with their context menu, the docked-browser mode, and the small
chrome dialogs. The target is *close* parity, not pixel equality:
some things stay out (see "Omissions"), a few things are our own
additions.

This phase sits between Phase 5 (dialogs, complete) and Phase 6
(scripting). Phase 6 keeps its scope from `docs/port-plan.md` and
starts only after this phase is done.

Read first: `AGENTS.md` (rules + status + all phase lessons),
`docs/decisions.md` (ADR-004 no libadwaita, ADR-018 GTK 4.0-era
surface, ADR-024 this phase's scope), `docs/phase-4-kickoff.md`
(browser shell hooks), `docs/phase-5-kickoff.md` (dialog
commits, settings wiring).

## How tasks work here (mandatory protocol)

1. Study the named C# source. Do not guess from names or memory.
2. Implement. Keep the geometry/menu content pure where you can
   (unit tests over widget code).
3. Gate: `cargo fmt --all`, `cargo clippy --workspace
   --all-targets -- -D warnings`, `cargo test --workspace`.
4. Commit and push. One task = one commit (small fixes may follow).
5. Write the user acceptance test (steps on the user's machine,
   `cargo run -p cr-app --release --`). STOP. The task is not
   complete until the user says PASS.
6. Update this file (task status + lessons) and the AGENTS.md
   "Current status" section. Then pick the next task.

Screenshots from the user live in `docs/screenshots/` (git-ignored).
NEVER commit them — they show the user's library. Commit only
cropped descriptions if needed.

The reference checkout root for every C# path below is
`/home/scuttle/Downloads/repo/ComicRackCE` (see AGENTS.md).

## Scope decisions (locked — ADR-024)

- **Browser dock modes: Fill + Bottom only.** CR also docks
  Left/Right. Nobody uses those in practice. The Bottom mode
  (reader above, collapsible browser strip below) is the real
  second mode.
- **Detail column chooser: IN** (right-click a column header in
  Details view → check-list of columns; C#
  `ItemView.cs:3760` auto-header menu).
- **Info panel: OUT.** CR's optional selected-book panel
  (Browse ▸ Info Panel, Shift+F9). The Properties editor and the
  sidebar preview cover it. Revisit later if missed.
- **Icons: bundled.** Copy the C# PNG icon set into
  `crates/cr-ui/assets/icons/` (same precedent as the paper
  textures, ADR-004 era). Source: `ComicRack/Resources/*.png`
  (183 files, referenced from `ComicRack/Properties/Resources.resx`
  as ResXFileRef entries). Use the C# names; add a small loader
  (`icon.rs`) that finds a PNG by name.
- **Omissions (all confirmed with the user):**
  - Undo/redo (no C# undo stack exists for the browser commands
    we port — the C# menu items sit unused for our flows)
  - Tray icon / minimize-to-tray
  - Remote library (Open Remote Library, remote tabs, remote
    server UI) — Phase 7 territory
  - Device sync (menu item, devices dialogs, sync gauge)
  - News dialog, Check For Update, help/homepage/forum links
  - Automation submenu (Phase 6 scripting hooks it)
  - Splash screen (cosmetic; startup is fast)
  - Crash dialog
  - Search-browser matcher panel (the browser's top matcher
    builder strip; the Quick search box + smart lists cover it)
  - Web comics menu item (WebComicProvider is an open Phase 1 gap)
  - Named workspaces (Save/Edit workspace presets) — we port only
    the automatic layout persistence (T14)
  - Undo/redo toolbar buttons, "Open in New Window" navigator item

## C# chrome inventory (the spec)

All paths relative to the reference root. Key files:

| File | LOC | Role |
|---|---|---|
| `ComicRack/MainForm.Designer.cs` | 3,876 | Every menu, toolbar, status panel, dialog wiring |
| `ComicRack/MainForm.cs` | 4,576 | The command handlers, dynamic menu fills |
| `ComicRack/CommandHandler.cs` + `CommandMapper.cs` | ~600 | The command/action layer (T1) |
| `ComicRack/Views/MainView.cs` + Designer | ~1,000 | The browser container: tab strip, dock modes |
| `ComicRack/Views/ComicBrowserControl.cs` + Designer | ~3,000 | The browser toolbar + grid host |
| `ComicRack/Views/ComicListLibraryBrowser.cs` | ~700 | The navigator + its toolbar |
| `ComicRack/Views/SmallComicPreview.cs` | ~400 | The sidebar preview pane |
| `ComicRack/Dialogs/ComicDisplaySettingsDialog.cs` | 405 | Book Display Settings (F9) |
| `ComicRack/ReaderForm.cs` | ~200 | The undocked reader window |
| `cYo.Common.Windows/Forms/TabBar.cs` | 1,926 | The tab strip widget (drag-reorder, middle-click close) |

### Window skeleton (MainForm.Designer.cs:3487-3490)

Docked children, in the order they claim edges:

1. `mainMenuStrip` — Top. The six menus.
2. `viewContainer` → `panelReader` → `readerContainer` — Fill.
   The reader area: `fileTabs` (the book tab strip, docked Top)
   and `quickOpenView` (the Quick Open panel).
3. `mainViewContainer` — Bottom, default 250 px. A collapsible
   `SizableContainer` holding `mainView` (the browser = MainView).
   Default workspace is Fill (browser fills the window, reader
   tabs live inside it) — see the dock-mode section below.
4. `statusStrip` — Bottom. The eight status panels.

The browser panel is movable between Fill and an edge
(`MainForm.cs:679-701`). In Fill mode `viewContainer` moves INSIDE
`mainView` and the browser fills the window. When docked to an
edge, the reader area sits above/inside and the browser becomes a
collapsible strip. Collapsed leaves a grip when
`AlwaysDisplayBrowserDockingGrip` (`MainForm.cs:3714`).
`PanelSize` default 400x250 (`DisplayWorkspace.cs:71-76`).

### The six menus (MainForm.Designer.cs:436-1712)

**File** (451-476): Open File... (Ctrl+O) | Close (Ctrl+X), Close
All (Ctrl+Shift+X) | — | New Tab (Ctrl+T) | — | Add Folder to
Library... (Ctrl+Shift+A), Scan Book Folders (Ctrl+Shift+S),
Update all Book Files (Ctrl+Shift+U), Update Web Comics
(Ctrl+Shift+W), Synchronize Devices, Generate Cover Thumbnails,
Tasks... (Ctrl+Shift+T), Automation (dynamic, scripts; hidden when
empty) | — | New fileless Book Entry... (Ctrl+Shift+N) | — | Open
Remote Library... (Ctrl+Shift+R) | — | Open Books (dynamic: one
entry per open tab, checked = current, Ctrl+Alt+F1..F12;
`MainForm.cs:3014-3024`) | Recent Books (dynamic: numbered
entries with cover thumbs, `MainForm.cs:2928-2958`) | — |
Restart (Ctrl+Shift+Q) | — | Exit (Ctrl+Q).
Dynamic: "Update all Book Files" hides when
`AutoUpdateComicsFiles` is on (`MainForm.cs:3570-3573`).

**Edit** (662-679): Info... (Ctrl+I) | — | Undo (Ctrl+Z), Redo
(Ctrl+Y) | — | My Rating (submenu: None / 1–5 stars / Quick
Rating...; Alt+Shift+0..5), Page Type (dynamic enum submenu,
`MainForm.cs:989-998`), Page Rotation, Bookmarks (submenu: Set
Bookmark... Ctrl+Shift+B, Remove Bookmark Ctrl+Shift+D | — |
Previous/Next Bookmark Ctrl+Shift+P/N | — | Last Page Read
Ctrl+Shift+L | sep + the dynamic bookmark list, `MainForm.cs:3757`),
Copy Page (Ctrl+C), Export Page... (Ctrl+Shift+C) | — | Refresh
(F5) | — | Devices..., Preferences... (Ctrl+F9).
Page Type / Rotation / rating items enable only with a book open
(`MainForm.cs:3575`).

**Browse** (954-970): Browser (F3 — toggle browser visibility) |
— | Library (F6), Folders (F7), Pages (F8) | — | Sidebar
(Shift+F6), Small Preview (Shift+F7), Search Browser (Shift+F8),
Info Panel (Shift+F9) | — | Previous List (Ctrl+J), Next List
(Ctrl+K) | — | Workspaces (Save Workspace... Ctrl+W, Edit
Workspaces..., the dynamic saved list — omitted) | List Layout
(Edit List Layout... Ctrl+L, Save List Layout..., Edit Layouts...
Ctrl+Alt+L, Set all Lists to current Layout, sep + dynamic
layouts — `MainForm.cs:2890-2927`).
Port notes: F6/F7 toggle the left-panel Library/Folders tabs (we
have Library|Pages; Folders view is Phase 7 — omit the F7 item or
hide it). List Layout menus wait for Detail column persistence
(T6); ship the menu when the backing data exists.

**Read** (1159-1176): First Page (Ctrl+B), Previous Page
(Ctrl+P), Next Page (Ctrl+N), Last Page (Ctrl+E) | — | Previous
Book (Ctrl+Alt+P), Next Book (Ctrl+Alt+N), Random Book
(Ctrl+Alt+O), Show in Browser (Ctrl+F3) | — | Previous Tab
(Ctrl+Shift+J), Next Tab (Ctrl+Shift+K) | — | Auto Scrolling
(Ctrl+S), Double Page Auto Scrolling (Alt+Shift+S) | — | Track
current Page (Alt+Shift+T).
The whole menu hides when no comic is visible
(`MainForm.cs:3939-3957`).

**Display** (1319-1330): Book Display Settings... (F9) | — |
Page Layout submenu (Original Size Ctrl+D1, Fit All Ctrl+D2, Fit
Width Ctrl+D3, Fit Width adaptive Ctrl+D4, Fit Height Ctrl+D5,
Fit Best Ctrl+D6 | Single Page Ctrl+D7, Two Pages Ctrl+D8, Two
Pages adaptive Ctrl+D9, Continuous, Right to Left Ctrl+D0 | Only
fit if oversized Ctrl+Shift+D0), Zoom submenu (Zoom In Ctrl+=,
Zoom Out Ctrl+-, Toggle Zoom Ctrl+Alt+Z | 100/125/150/200/400 % |
Custom... Ctrl+Shift+Z), Rotation submenu (Rotate Left
Ctrl+Shift+-, Rotate Right Ctrl+Shift++ | No Rotation
Ctrl+Shift+D7, 90/180/270° Ctrl+Shift+D8/D9/D0 | Autorotate Double
Pages) | — | Minimal User Interface (F10), Full Screen (F11),
Reader in own Window (F12) | — | Magnifier (Ctrl+M).

**Help** (1699-1712): Documentation (F1), Quick Introduction |
— | Homepage, User Forum | — | News..., Check For Update | — |
About... (Alt+F1). Omitted per scope: News/Update/links stay out;
About stays.

Keybinding note: the reader command table
(`cr-ui/src/reader/keys.rs`) already owns most Display/Read
commands — but only inside the reader widget. T1 moves command
*dispatch* up to the shell so the same commands fire from menus,
accelerators, and toolbars, and stay enabled state-aware.

### Toolbars

There is no classic menu-attached toolbar row. The chrome:

**a) The reader toolbar `mainToolStrip`** (Designer:2591-2614) —
Dock=Right INSIDE the tab row; in Fill mode it moves into the
browser's tab strip (`MainForm.cs:593-621`); hidden in
MinimalGui. Buttons in order (icon-only split buttons):

1. tbPrevPage — drop: First Page, Previous Bookmark | Previous
   Book from List
2. tbNextPage — drop: Last Page, Next Bookmark, Last Page Read,
   sep, Next/Random Book from List
3. tbPageLayout — drop: Single/Two Pages/Two adaptive/Continuous |
   Right to Left
4. tbFit — drop: Original/Fit All/Fit Width/Fit Width adaptive/
   Fit Height/Fit Best | Only fit if oversized
5. tbZoom — text shows the current %, drop: Zoom In/Out, presets,
   Custom...
6. tbRotate — text shows the current angle, drop: Rotate L/R,
   0/90/180/270, Autorotate
7. tbMagnify
8. tbFullScreen (toggle)
9. tbTools — a "Tools" drop ≈ a flattened menu: Open Book...,
   Open Remote Library..., Info... | Workspaces | Bookmarks | Auto
   Scrolling | Minimal GUI, Reader in own Window | Scan, Update
   all Book Files, Update Web Comics, Generate Cover Thumbnails,
   Synchronize Devices | Book Display Settings..., Preferences...,
   About... | Show Main Menu (Shift+F10 toggles
   `AutoHideMainMenu`, `MainForm.cs:1457`) | — | Exit

Reader-only buttons hide without a book
(`MainForm.cs:3939-3957`); zoom/rotate button text tracks state
(`MainForm.cs:3924-3928`).

**b) The browser toolbar** inside `ComicBrowserControl`
(Designer:976-991): tbSidebar (toggle navigator) |
btBrowsePrev/btBrowseNext (list browsing history) | tbbView
"Views" (Thumbnails/Tiles/Details | Collapse/Expand all Groups,
Show Group Headers | Show All / not Read / Reading / Read | Show
only Books / fileless | Show Duplicates) | tbbGroup "Group"
(dynamic grouper menu) | tbbStack "Stack" (dynamic; hidden in
Detail view) | tbbSort "Arrange" (dynamic sort menu) |
tsQuickSearch — the quick search box, right-aligned, with a scope
menu (All/Series/Writer/Artists/Descriptive/Catalog/Filename;
Designer:736-814) | tsListLayouts (Edit/Save List Layout, Reset
List Background, Edit Layouts) | tbbDuplicateList (visible for
library lists) | tbUndo, tbRedo (omitted).
Stacking UI waits for the stack feature — hide the Stack button
until stacking exists (it is not in this phase).

**c) The navigator toolbar** inside `ComicListLibraryBrowser`
(Designer:320-331): tbNewFolder, tbNewList, tbNewSmartList |
tbOpenWindow, tbOpenTab | tbExpandCollapseAll, tbRefresh |
right-aligned: tbFavorites (toggles the Favorites pane — omitted),
tsQuickSearch button (toggles the navigator's own search box,
Ctrl+Alt+F).

**d) The browser tab-strip alignment button** (`MainView.cs:
199-221`): "Docking Mode" drop — Dock Bottom (Ctrl+Shift+D1), Dock
Left (Ctrl+Shift+D2), Dock Right (Ctrl+Shift+D3), Fill
(Ctrl+Shift+D4). Port only Bottom + Fill.

**e) The preview toolbar** (`SmallComicPreview.Designer.cs:
81-92`): Open | First, Prev, Next, Last | Two Pages toggle |
Refresh, Close.

**f) The Pages-panel toolbar** (ComicPagesView.Designer.cs:
66-141): Views / Group / Arrange + a right-aligned Page Filter
button. Port a reduced set: view mode + sort only (the Pages grid
is simpler than the book browser).

### Status bar (statusStrip, Designer:1799-1812)

Eight panels left to right (`MainForm.cs:3888-4042`):

1. Spring panel — browser selection info: "ListName: N Books,
   M selected / size / filepath" (`MainForm.cs:3923`,
   `ComicBrowserControl.cs:635`). Default "Ready".
2. Activity lamps (icon-only, hidden unless active, click opens
   the Tasks dialog): scan, export, write-info, read-info,
   thumbnails, backup. Simplify to three lamps in the port: scan,
   export, file-write (the others have no ported activity yet).
3. Data source state (gray/green light).
4. Book — the open book caption; "None" otherwise.
5. Current page number (clickable — toggles TrackCurrentPage).
6. Page count ("24 Pages"; "retrieving" while the provider index
   is incomplete).
7. **Thumbnail size slider** — a trackbar; only when the browser
   is visible; drives `SetItemSize` (`MainForm.cs:3718-3721`).
8. Server activity light (omitted — no remote server).

### Tabs (the book tabs)

`RebuildBookTabs` (`MainForm.cs:2991-3146`) fills the tab strip:
one item per open book, caption = the book caption, 16 px cover
icon, bold for the current tab, a trailing "+" item (New Tab), and
per-tab close buttons. TabBar behaviors (`TabBar.cs`): drag to
reorder (1488), middle-click closes (1600), right-click shows the
tab context menu (3374 in the Designer): Close (Ctrl+X), Close All
But This, Close All to the Right | Show in Browser (Ctrl+F3) |
Reveal in Explorer (Ctrl+G), Copy Full Path to Clipboard. In Fill
mode the same tabs also appear on the browser's tab strip
(`MainView.cs:465-469`). Menu mirrors: File ▸ Open Books
(Ctrl+Alt+F1..F12). Workspace "tabs" are NOT tabs — they are saved
layout presets (omitted).

### Other chrome

- Quick search: the browser toolbar box (debounce 500 ms in C#;
  ours is 300 ms — keep ours), plus the navigator's own box
  (Ctrl+Alt+F).
- The auto-hide menu bar: `AutoHideMainMenu` hides the menubar;
  Alt reveals it (`MainForm.cs:3863-3885`). Port as a setting +
  Alt accelerator.
- MinimalGui (F10/K): hides menubar, tab bars, status bar
  (`MainForm.cs:3658-3716`).
- Window title: "ComicRack" or "ComicRack — <caption>"
  (`MainForm.cs:3912-3919`). Ours: "ComicRust — <caption>".
- Sidebar: 252 px, collapsible; preview pane 207 px, OFF by
  default; the SmallComicPreview shows the first selected book
  ("Nothing Selected" placeholder), 500 ms refresh delay after
  selection change (`ComicExplorerView.cs:294-307`).
- QuickOpen: shows when `ShowQuickOpen` && no book is open
  (`MainForm.cs:4440-4514`) — we have this already.

### ReaderForm (the undocked reader)

`ReaderForm.Designer.cs` is empty (43 lines). The undocked reader
keeps the same tab strip + reader toolbar; it gets NO menubar and
NO status bar. Title = book caption. Closing asks
"This will only close the reader Window and not the open Book(s)!"
unless suppressed (`ReaderForm.cs:59-69`). Our undock already
matches the chrome-less shape; add the toolbar when T5 exists.

## Current Rust state (what this phase builds on)

Inventory as of 2026-09-04 (full list in AGENTS.md):

- Browser window: HeaderBar only (Open / Add Folder / Quick
  search / Browser toggle / Preferences / View / Sort / Group +
  the reader subtitle). No menubar, no toolbar, no accelerators on
  the `win.*` actions. Left panel = StackSwitcher (Library |
  Pages) + Paned. Status = one Label. QuickOpen page exists.
- Item view context menu (Open / Reveal / Edit / Update File /
  Export / Remove / Properties) and navigator context menu (New
  Smart List / New List / Edit / New Folder / Rename / Delete)
  exist.
- Reader: session tabs with close buttons, Tab/Shift+Tab, undock
  (D), fullscreen, MinimalGui (K), the full C# key table in
  `reader/keys.rs` (reader-internal dispatch only), chrome
  auto-hide.
- No layout persistence (panel sizes, dock mode), no bundled
  icons (navigator icons come from the GTK icon theme), theme is a
  40-line CSS + dark preference.

Gap summary: menubar (all), toolbars (all), status bar (rich),
book tabs in the main window, dock modes, preview pane, Book
Display Settings, About/Zoom/Quick Rating/Tasks dialogs, icons,
accelerators, layout persistence.

## Task breakdown

Order: T1 unblocks everything (actions + accelerators). T2 (icons)
lands early — every later bar needs them. The bars follow: menus,
toolbars, status, tabs. T10 (dock mode) is the deepest layout
change; it lands after the bars so they exist in both modes.

### T1. Command/action layer + accelerators

- C# spec: `ComicRack/CommandHandler.cs`, `CommandMapper.cs`,
  `MainForm.InitializeKeyboard` (the reader table is already
  ported in `reader/keys.rs`).
- Scope: a shell-level command registry (`cr-ui/src/commands.rs`):
  every browser/shell command becomes a `gio::SimpleAction` in the
  `win.` group (most exist as header-menu actions already); add
  enable/disable state (no book open → Read/Edit commands
  disabled); register ALL accelerators from the menu table above
  (`set_accels_for_action`). Reader-only commands route into the
  docked reader's existing dispatch (the shell forwards the
  action name to the reader command table). Menu-bar accelerators
  in GTK need real GioMenu actions — this task makes every command
  a real action so later tasks only build widgets.
- Acceptance: keyboard-only user run — Ctrl+O opens a file, F5
  refreshes, Ctrl+I opens Properties, F3 toggles the browser page
  over the reader, Ctrl+1..9 select fit/layout modes in the
  reader, F11 fullscreen, Ctrl+D2 docks (after T10). No visible
  chrome change yet.

### T2. Bundled icon set + theme upgrade

- C# spec: `ComicRack/Resources/*.png` (183 files; the resx name
  is the C# property, e.g. `Resources.Open` → `Open.png`).
- Scope: copy the PNGs into `crates/cr-ui/assets/icons/` (git
  add — they are CC-licensed app assets of the same origin as the
  papers, ADR-004 precedent). Add `cr-ui/src/icon.rs`: name →
  `gdk::Texture` cache (parse the resx name → file name
  identically, `#` variants fall back to the base name). Use
  `set_tooltip_text` where the C# uses tooltips. Give the
  navigator tree the CR item icons (folder/smart list/list kinds)
  from this set (`Special/*.png` holds the special ones).
  ItemView/page chrome keeps cairo painting (only chrome icons
  switch to PNGs in this task).
- Acceptance: a probe (Xvfb) renders an icon gallery from the
  loader; the navigator shows the CR folder/list icons; no missed
  files (a test iterates the resx name list vs the asset dir).

### T3. Menubar — static skeleton

- C# spec: `MainForm.Designer.cs:436-1712` (the menu tree above).
- Scope: a `PopoverMenuBar` from a `Gio::Menu` model at the top of
  the browser window (GTK4 has no in-window MenuBar; the
  PopoverMenuBar is the WinForms visual equivalent). All six
  menus, items, separators, checkable items (Track current Page,
  Autorotate, RTL...), radio items (fit modes, layout modes),
  shortcuts shown via the accelerators from T1. Wire every item
  to an existing or stubbed action; disabled-state logic from
  `editMenu_DropDownOpening` etc. Omitted items (see Omissions)
  are simply absent. `AutoHideMainMenu` support (Alt reveals).
- Acceptance: user compares the open menus against CR (screenshots
  provided); every item is present or absent for a recorded
  reason; check/radio state follows the reader state.

### T4. Dynamic menus

- C# spec: `MainForm.cs` — Open Books (3014), Recent Books
  (2928), My Rating submenu, Page Type (989), Bookmarks (3757),
  Workspaces/List Layout lists (2890).
- Scope: the dynamic submenu fills: Open Books (one entry per
  open tab, check on current), Recent Books (with cover thumbs if
  cheap, else text-only — record the deviation), Page Type (the
  enum list, radio), Page Rotation, Bookmarks (Set/Remove/Prev/
  Next/Last Read + the per-page bookmark list), My Rating.
  List Layout menus land with T6/T14 data.
- Acceptance: open three comics — the Open Books menu lists all
  three and checks the current; set a bookmark — it appears in
  Bookmarks; rate 4 stars — the menu shows it.

### T5. Reader toolbar

- C# spec: `MainForm.Designer.cs:2591-2614`, the drop lists above,
  `MainForm.cs:3924-3975` (visibility + state text).
- Scope: the nine-button strip (prev/next + dropdowns, page
  layout, fit, zoom, rotate, magnifier, fullscreen, tools) built
  from MenuButtons + Popovers; right-aligned in the tab row; state
  text on zoom/rotate buttons; hide reader-only buttons with no
  book; MinimalGui hides it. Undocked reader gets the same bar.
- Acceptance: every button fires the matching command in the
  reader; zoom % and rotation angle track state; MinimalGui hides
  the bar; the Tools menu opens Preferences.

### T6. Browser toolbar reorg + Detail column chooser

- C# spec: `ComicBrowserControl.Designer.cs:736-991`,
  `ComicBrowserControl.cs` (grouper/sort menu fills 2872+),
  `ItemView.cs:3760` (the auto header menu).
- Scope: reorganize the current header: the browser toolbar row
  gets Sidebar toggle, list history back/forward, Views (radio:
  Thumbnails/Tiles/Details + show filters: all/not read/reading/
  read, books only, duplicates), Group (the existing grouper
  menu), Sort (the existing sort menu), List Layout (disabled
  until T14 data), Duplicate List. Quick search stays right-aligned
  (add the C# scope menu: All/Series/Writer/Artists/Descriptive/
  Catalog/Filename — map to the AllProperties fields). The
  header's Open/Preferences/View/Sort/Group buttons move into the
  new structure. Column chooser: right-click a Detail column
  header → check-list of all registered columns (persist in T14).
- Acceptance: toggles work (Sidebar hides the left panel); the
  read-state filter changes the grid; column chooser shows/hides
  Detail columns live; sort/group still work.

### T7. Navigator toolbar + Pages toolbar

- C# spec: `ComicListLibraryBrowser.Designer.cs:320-331`,
  `ComicPagesView.Designer.cs:66-141`.
- Scope: a small icon toolbar above the navigator tree (New
  Folder, New List, New Smart List | Expand/Collapse All,
  Refresh) and one above the Pages grid (view mode radio
  Thumbnails/Tiles, the page filter button as a stub or hidden).
  Wire to the existing navigator commands.
- Acceptance: the buttons create/edit the same things as the
  context menu; Expand/Collapse works on the tree.

### T8. Status bar (multi-panel)

- C# spec: `MainForm.Designer.cs:1799-1812`, `MainForm.cs:3888-4042`.
- Scope: a bottom bar (GtkBox) with: selection-info label (list
  name + count + selection + total size; build from the same data
  the browser has), activity lamps (start: scan / export /
  file-write; visible only while active; click → the Tasks dialog
  from T13), book caption, current page (click toggles
  TrackCurrentPage), page count, the thumb-size slider (a
  GtkScale; drag resizes the item view — `SetItemSize` parity).
  The current "N book(s), M selected" label folds into the
  selection-info panel.
- Acceptance: select books — the info line updates; open a book —
  book + page + count appear; drag the slider — the grid thumbs
  resize; lamps appear during a scan/export.

### T9. Book tabs in the main window

- C# spec: `MainForm.cs:2991-3146` (RebuildBookTabs),
  `TabBar.cs` behaviors, the tab context menu
  (`MainForm.Designer.cs:3374-3417`).
- Scope: the reader tabs (already closable) become full book tabs:
  a "+" (New Tab → QuickOpen), middle-click close, right-click
  context menu (Close / Close All But This / Close All to the
  Right / Show in Browser / Reveal in Explorer / Copy Full Path),
  the tab caption = the book caption (the browser captions we
  already format), and tab switching already refreshes the
  Pages panel + status bar. Drag-reorder: only if the GTK
  notebook/menu path is cheap — otherwise record the deviation
  (the C# drags inside TabBar; GTK4 Notebooks reorder by drag
  natively — prefer a Notebook if it keeps the look).
- Acceptance: open three books; middle-click closes; the context
  menu items work; Show in Browser flips to the browser page with
  the right book selected.

### T10. Browser dock modes (Fill + Bottom)

- C# spec: `MainForm.cs:679-701` (BrowserDock), `3629-3716`
  (dock changed + grip), `MainView.cs:199-221` (the alignment
  button), `DisplayWorkspace.cs` (PanelSize).
- Scope: the browser window gains the second layout mode: the
  reader area (tab strip + reader) fills the window and the
  browser (tab strip label + navigator + grid) docks to the
  Bottom inside a resizable, collapsible Paned/SizableContainer
  equivalent. A grip/collapsed state shows a thin bar
  (`BrowserVisible`, F3 toggles). Switching via the Browse menu +
  the docking-mode button (T6). Persisted in T14. Fill stays the
  default. GTK mapping: an GtkOverlay or a vertical Paned with the
  reader on top; the "Fill" mode keeps today's stack. Do NOT
  attempt Left/Right.
- Acceptance: switch to Bottom with a book open — the reader sits
  above, the browser below; F3 collapses to a grip; drag the
  divider; the reader keeps working in both modes; restart keeps
  the mode (after T14) — for this task, in-session only.

### T11. Sidebar preview pane

- C# spec: `SmallComicPreview.cs` + Designer, `ComicExplorerView.cs:
  294-307`.
- Scope: below the navigator tree, a collapsible pane showing the
  first selected book's cover (page 0 render through the thumb
  pool) + caption, with the mini toolbar (Open / First/Prev/Next/
  Last / Two Pages toggle / Refresh / Close). "Nothing Selected"
  placeholder. Toggle: Browse ▸ Small Preview (Shift+F7), the pane
  Close button, persisted in T14. 500 ms selection debounce.
- Acceptance: select books — the preview follows after a moment;
  toolbar buttons flip pages in the preview; Two Pages shows a
  spread; Close hides the pane; Shift+F7 reopens.

### T12. Book Display Settings dialog (F9)

- C# spec: `ComicRack/Dialogs/ComicDisplaySettingsDialog.cs` (405
  LOC) + Designer + `ComicRack/Config/BookPageLayout.cs`.
  STUDY THE SOURCE FIRST — this dialog was never ported; it edits
  the per-comic display settings (the ComicBook display fields:
  page layout, fit mode, rotation, background, paper, and the
  "realistic pages" family).
- Scope: the dialog bound to the current book's display settings
  with OK/Cancel commit semantics (match the C# commit point).
  Reuse the options builder from Phase 5 T1 where the layout is
  plain.
- Acceptance: change the page layout/fit for one book — the
  reader shows it; OK persists across restart (the DB book is
  dirty + saved); Cancel discards; another book is unaffected.

### T13. Small chrome dialogs

- C# spec: `Dialogs/ZoomDialog.cs`, `Dialogs/QuickRatingDialog.cs`,
  `Dialogs/TasksDialog.cs` (+ the activity model), the About box
  (the C# About is part of Help — rebuild as a small GTK about
  dialog with the app icon + version `0.0.<commits>` per ADR-020).
- Scope: Zoom... (Ctrl+Shift+Z — a spinbox; applies to the
  reader), Quick Rating (the C# mini rating popup — also the
  deferred Phase 5 item), Tasks (the queue/activity list — feed it
  the same state the status lamps use), About. The C# opens Tasks
  from the status lamps and the File menu.
- Acceptance: each dialog opens from its menu/status entry and
  does its job (zoom applies, rating applies to the selection,
  Tasks shows the running scan/export).

### T14. Layout persistence (workspace-lite)

- C# spec: `ComicRack/Config/DisplayWorkspace.cs` (the saved
  workspace fields) — we persist ONE implicit workspace, no named
  presets, no workspace UI.
- Scope: on exit, save to the settings (Config.xml, the existing
  `cr-core` settings layer): browser dock mode, browser panel
  size, sidebar width + visibility, preview pane visibility +
  height, status-bar visibility, menubar auto-hide state, last
  view mode/sort/group per list, Detail column set + widths (from
  T6), thumb size (from T8), window bounds. Restore at startup.
  The C# keys are the `DisplayWorkspace` fields; keep the names in
  comments for traceability.
- Acceptance: set a custom layout (Bottom dock, small sidebar,
  Details view, hidden menubar) — restart restores all of it.

## Risks / known traps

- ADR-018: GTK 4.0-era API only. PopoverMenuBar exists in 4.0.
  GtkNotebook tabs are fine. No GtkFileDialog, no
  `load_from_string`.
- Every bar lives in the browser header/rows — watch the
  reader-docked state (the C# moves the toolbar between the tab
  row and the browser; we mirror with visibility flips, not
  re-parenting).
- The T9 dock-mode reshape touches the shell stack created in
  `BrowserShell::create` — the Pages panel, QuickOpen, and the
  reader hooks all hang off it. Do it in one task, user-test
  immediately, and keep the Fill path bit-identical first
  (screenshots before/after).
- GTK4 has no click-to-focus and menu-bar accelerators need real
  Gio actions — T1 must land before any menu task.
- The status-bar slider and column widths feed the browser layout
  — keep the per-book-id text caches (Phase 4 lesson) intact.

## Progress log

- T0: kickoff written (2026-09-04). Scope locked with the user:
  Fill+Bottom only, column chooser IN, Info panel OUT, bundled CR
  icons, omissions as listed. User provides screenshots on demand.
- T1 IMPLEMENTED (2026-09-04), user test pending. `cr-ui/src/
  commands.rs`: the pure command table (69 shell actions with the
  C# menu accelerators; FIT_MODES/LAYOUT_MODES carry the radio
  values) + 4 unit tests (unique actions, unique accels, accel
  syntax, no plain-key shell accels). The wiring lives in
  `browser/shell.rs` `install_commands`: every command a
  `gio::SimpleAction` in `win.`; accels on the application
  (detailed names for the radio targets). Enable-state syncs from
  book-open/selection/history (`sync_enabled`; hooks on
  selection_changed, book_changed, last_tab_closed, list select).
  Fit/layout/rtl radios take state from the reader (new getters
  `current_fit_mode/page_layout/rtl`). Reader commands route
  through `PageView::run_command` (public) — the Library group
  (Next/Prev/Random Comic, ShowBrowser) forwards to the shell via
  the new `set_on_library_command` hook; Next/Prev/Random Book
  opens from the current list's view order (`OpenNextComic` port,
  random without repeats, `DotNetRandom` time-seeded). Show in
  Browser selects the open book (new `ItemView::select_book`/
  `selection_ids`; `Navigator::select_list`). Scan Book Folders
  scans the watch-folder roots. Update all Book Files drains the
  DIRTY books one per main-loop tick (`library::
  update_all_book_files` — the dirty flag gates even with
  alwaysWrite, verified against `AddBookToFileUpdate`). Previous/
  Next List walks a new list history (recorded on navigator
  selection; a history jump does not append). Restart re-launches
  the binary after the save. Fixed-angle Rotate 0/90/180/270
  dispatch (new `PageView::set_rotation`). The editor context-menu
  paths share the shell commits (`ShellState::open_editor/
  open_bulk_editor` — `apply_edited` semantics everywhere now;
  the old Properties commit was a bare replace). Stub actions
  (Tasks, Zoom Custom, Display Settings, About, Set/Remove
  Bookmark, Quick Rating, Copy/Export Page, Small Preview, New
  Book Entry, navigator search) stay DISABLED until their tasks
  land. Headless probe `examples/commands_probe.rs`: 65/65
  parameterless actions resolve, accels registered (incl. per-value
  radio), page flips dispatch, clean exit. Probe lesson: never
  blanket-activate every action in a probe — `restart` spawns the
  binary and self-perpetuates; skip side-effecting commands.
- T1 DEVIATIONS (recorded): the C# accelerator table has TWO
  collisions, resolved by menu order (File before Edit, Page Layout
  before Rotation): Only fit if oversized keeps Ctrl+Shift+D0 (the
  C# also gives it to Rotate 270 — unbound here) and New fileless
  Book Entry keeps Ctrl+Shift+N (Next Bookmark unbound). Shell
  accels resolve per window — the undocked reader window has NO
  shell accels (no `win.` group there), matching the C# ReaderForm
  (no menubar). Next/Prev/Random Book resolves against the ACTIVE
  view's books (the C# resolves the book's own container browser);
  the current book must be in the view. Update all Book Files
  covers library books only (temporary books are unported).
- T1 FIX ROUND 1 (2026-09-04), user test pending. Three findings:
  1. Alt+Shift+4 (My Rating) never fired: GTK accelerators match
     the PRODUCED keyval — Shift+4 yields '¤'/'$' on US/Swedish
     layouts — while the C# matched the WinForms VIRTUAL key
     (Keys.D4, layout-independent). The same family broke
     Ctrl+Shift+0/7/8/9 (Only fit, Rotate 0/90/180) and
     Ctrl+Shift+OemMinus (Rotate Left). Fix: a window key
     controller resolves the hardware keycode to its UNSHIFTED
     keyval (`gdk_display_map_keycode`, level 0) and fires the
     command (`commands::shifted_symbol_command`, unit-tested);
     it fires ONLY when the raw keyval differs from the unshifted
     one, so layouts where Shift keeps the symbol never
     double-fire with the real accelerator. Zoom In additionally
     registers `<Control>equal` (the '+' key's unshifted symbol —
     Ctrl+Oemplus parity; `<Control>plus` covers the numpad).
     Shifted-LETTER accels need no fallback (GTK matches letter
     case variants — the user's Ctrl+Shift+J/K passed).
  2. F10/K (MinimalGui) toggled the reader's own header — an
     UNPARENTED widget in the docked shape, so nothing visible.
     Fix: the chrome visibility applies to the HOST window's
     header bar when docked (`apply_chrome_visibility`; the T3/T8
     bars join when they exist). The fullscreen reveal strip and
     the AutoMinimalGui path drive the same helper; the
     fullscreen state now reads the view's own ROOT window (the
     undocked reader fullscreens its own window, not the host).
  3. Working-as-designed, explained to the user: Ctrl+0 toggles
     Right-to-Left reading (manga page order — visible with
     spreads), Ctrl+S toggles Auto Scrolling (the wheel turns
     pages instead of scrolling). Both lack a check indicator
     until the menus land (T3).
- T1 COMPLETE — USER-TESTED, ALL PASS (2026-09-04; one fix round).
  The retest covered: Alt+Shift+1..5 rating updates the grid,
  Ctrl+Shift+0/7/8/9/minus rotate and only-fit, F10 and K toggle
  the header chrome, plus the whole original keyboard sweep
  (open/close/tabs/history/fit/layout/zoom/fullscreen/undock).
  **Next: T2 (the bundled icon set + `icon.rs`).**
- T2 COMPLETE (2026-09-04) — NO USER TEST (the kickoff acceptance
  is machine-checkable: probe + resx test; the user directed
  commit-and-continue). Assets: the 212 resx PNGs copied verbatim —
  183 top-level + 29 `Dark/*` (`crates/cr-ui/assets/icons/`,
  1.4 MB, same origin as the papers). The resx name → file mapping
  is IDENTITY for every non-Dark name (a first positional parse
  suggested swaps — an extraction artifact; a proper per-element
  parse of `Resources.resx` settles it: only the 29 `Dark*` names
  map to `Dark\<base>.png`). DEVIATION: the 17 resx GIFs (scan/
  export/device-sync task animations) and `ComicRackAppSmall.ico`
  are NOT bundled — no ported consumer; the T8 lamps will use the
  static PNGs instead. `cr-ui/src/icon.rs`: `path_for_name` (pure
  rule: identity, `Dark` prefix → `Dark/`, `#variant` suffix falls
  back to the base — no C# fetcher produces `#` names for these
  resources, the rule is loader-side per the kickoff) +
  `icon(name) → Option<gdk::Texture>` cached per name (negatives
  too), loaded via `Texture::from_file` (GDK 4.0-era, ADR-018).
  Navigator: the tree now renders the bundled icons through a
  texture column — the C# `treeImages` table parity
  (`ComicListLibraryBrowser.cs:313-317`): Library → Library.png,
  the "Folder" key → SearchFolder.png, the "Search" key →
  SearchDocument.png, reading lists → List.png (the C# ImageKeys
  are NOT all resx names — "Folder"/"Search" re-map). Gate:
  `cr-ui/tests/icons.rs` — all 212 resx PNG names resolve to their
  exact asset files, the 17 GIF + 1 ICO names stay unresolved, the
  `#` fallback works. Probe `examples/icons_probe.rs`: LOADED
  212/212 headless, the navigator fills from the library snapshot,
  and the gallery window SCREENSHOTS with the icons visible (the
  all-black-screenshot streak broke on the first non-GL run).
  Probe lesson: a probe dwell window MUST be an `ApplicationWindow`
  built with `.application(app)` — a plain `gtk4::Window` holds
  nothing and the loop exits before any timeout fires (even with
  the window leaked). Release workflows now copy `assets/icons`
  next to `assets/papers` in the tarball (both release.yaml and
  tagged-release.yaml). **Next: T3 (the menubar skeleton).**
