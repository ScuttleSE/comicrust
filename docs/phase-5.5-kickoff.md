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
- Port notes (the T4/T5 learnings apply): build the browser
  toolbar's menus with `menubar::build_dropdown` (the same
  machinery the reader toolbar uses — parent to the anchor button
  before popup, one popover per instance); the Views/Group/Sort
  state pushes through the SAME `sync_menubar` resolve closure
  (add the view-mode/read-filter radio states to the actions
  registry — the C# reads them from `IComicBrowser`); the View
  menu tables live in `toolbar.rs`-style consts; new stateful
  actions (`view-filter`/`duplicate-only`/the scope menu) follow
  the `page-fit` radio pattern (BARE name + variant parameter on
  clicks — the T4 lesson).
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
- T3 IMPLEMENTED (2026-09-04), user test pending. The menubar:
  `cr-ui/src/browser/menubar.rs` — a PURE table (`MENUS`: the six
  C# menus with GTK `_` mnemonics, every item citing its Designer
  source) + a Gio model builder (sections = separators, `accel`
  attributes drive the shortcut display) + the `menubar_visible`
  rule — the `OnGuiVisibilities` FILL-mode port
  (`MainForm.cs:3673-3688`): `flag4 && (!AutoHideMainMenu ||
  (ShowMainMenuNoComicOpen && !bookOpen))`, the undocked shape
  forces ON, MinimalGui kills it, the Alt-reveal overrides. The
  bar mounts in a wrapper Box above the shell stack (the window
  keeps its HeaderBar). Present/absent decisions (all unit-asserted):
  the ADR-024 omissions are absent; the T4 dynamic PARENTS (Open
  Books, Recent Books, Page Type, Page Rotation) stay out until
  their fills exist; Help carries only About (the docs/forum/news
  links stay out); the C# in-menu star-slider control under My
  Rating is not ported (the Quick Rating dialog covers it, T13);
  Bookmarks lists the five static items (the dynamic list is T4).
  New actions: `toggle-zoom` (the `MainForm.ToggleZoom` port —
  `PageView::toggle_zoom` with the `lastZoom` field), `zoom-preset`
  (100..400 % → `ImageZoom = v` via the new `ReaderShell::
  zoom_current`), `generate-thumbnails` (disabled stub — no task
  owns the thumbnail-queue command yet), and `display-settings`
  gained its F9 accel (the stub existed unbound). Five toggles
  became STATEFUL check actions (toggle-browser, auto-scroll,
  double-auto-scroll, minimal-gui, full-screen) and `sync_enabled`
  writes every check/radio state from the reader getters (new:
  PageView `auto_scrolling`/`two_page_navigation`/`auto_rotate`,
  ReaderShell `current_auto_scrolling`/`current_two_page_navigation`/
  `current_auto_rotate`/`is_minimal_gui`/`is_undocked`/
  `is_fullscreen`); the sync runs after EVERY action dispatch (the
  `CommandMapper` idle-update parity) and from the new
  `ReaderShell::set_on_chrome_change` hook (fired by
  `apply_chrome_visibility`; `toggle_minimal_gui` now routes
  through it; the callback fires AFTER the state borrow drops).
  AutoHideMainMenu (default TRUE): Alt pressed-and-released ALONE
  toggles the reveal (`MainForm.OnKeyUp` port; enableAutoHideMenu
  parity — only while auto-hidden and not minimal). Deviations
  recorded: the 500 ms re-close debounce is not ported; GTK4
  cannot open a PopoverMenuBar from code (the C# "select the first
  item" step is out) and has no popdown signal (the C# re-hides on
  menu deactivate; ours re-hides on action activation + Alt, an
  Esc/click-away leaves the bar until the next action); the C#
  miAutoScroll persists `Program.Settings.AutoScrolling`, the port
  flips the view field (session-only). Auto-scroll/double-auto-
  scroll/minimal-gui/full-screen stay gated to an open book (the
  check cannot fire without a view). Gate: 4 menubar unit tests
  (action existence, display-accel consistency with the commands
  table, the omission list, the visibility truth table); 293
  workspace tests green; `menubar_probe` (six menus, the startup
  visibility equals the rule, clean exit) + `commands_probe` still
  69/69; the Xvfb screenshot shows the bar under the header.
  **USER TEST (the T3 acceptance):**
  1. `cargo run -p cr-app --release --` — the menubar shows
     (File Edit Browse Read Display Help) under the header; the
     browser view has no book open.
  2. Open a comic — the menubar hides (AutoHideMainMenu default);
     Alt alone reveals it; Alt again hides it.
  3. With the bar revealed: fire Read ▸ Next Page — the page turns
     and the bar hides again (the action-activation re-hide).
  4. Compare the six menus against ComicRack — every item present
     or absent for a recorded reason (the list above).
  5. Check/radio state: Display ▸ Page Layout carries the radio dot
     on the current fit/layout; Ctrl+0 flips Right to Left (spread
     or RTL comic to see it); Ctrl+Shift+0 flips Only fit if
     oversized; Ctrl+S and Alt+Shift+S flip the two Auto Scrolling
     checks; F10/F11 (MinimalGui/Full Screen) check AND hide the
     bar — leaving restores it; Browse ▸ Browser/Sidebar follow
     their toggles; Edit ▸ Track current Page flips.
  6. Disabled state: no book open greys the Read menu and the
     bookmarks; a selection greys/un-greys My Rating; Zoom In/Out
     need a book.
  7. Zoom: Ctrl+= / Ctrl+- / Toggle Zoom (Ctrl+Alt+Z) and the
     100–400 % presets work in the reader; the menu shows the C#
     shortcuts.
- T3 REWORK — CUSTOM MENUBAR WITH ICONS (2026-09-04), user test
  pending (the retest below REPLACES the earlier list — same steps
  plus the icon check). Cause: the user compared against CR and the
  menu-item icons were missing — 78 of the C# main-menu items carry
  a 16 px image (`MainForm.Designer.cs` `mi*.Image`), and GTK4
  removed menu-item icons: `PopoverMenuBar`/`PopoverMenu` IGNORE the
  model's `icon` attribute (`GtkImageMenuItem` was removed in GTK4;
  gtk4-rs has no icon path for model menus). The user chose the
  full rework (option 1) over the deviation record.
  Implementation: `menubar.rs` now renders the pure table through a
  CUSTOM widget — a flat row of flat `MenuButton`s (mnemonic
  labels), each opening a hand-built popover: rows carry
  [check slot 16 px][icon 16 px][label][right-aligned gray accel |
  submenu arrow], separators, and nested `MenuButton` submenus
  (Right-positioned child popovers). The icon per item comes from
  the bundled set (`icon.rs`) via the mi→resx mapping extracted
  from the Designer (`CSHARP_ITEM_ICONS`, ~60 entries, unit-gated
  in BOTH directions: no drift, no invented icons, every named
  icon resolves; submenu parents checked too). Click → the popover
  pops down + the detailed action fires (radio values as
  parameters). Up/Down moves focus through the rows; Left/Right
  switches top menus while one is open (per-popover key
  controller — a popover is its own native surface); hover-enter
  switches top menus while one is open (the WinForms strip
  behavior; an open-count cell gates it). `sync` pushes action
  states into the rows (object-select check mark, radio match,
  disabled graying) — `sync_enabled` calls it after every action
  dispatch; the shell keeps `menubar_visible` for the visibility
  (unchanged rule) and `menubar_revealed` for the Alt reveal. The
  row click closes via the widget's Popover ancestor (an Item is
  always inside its popover's content Box). About icon: the C#
  resx ships About.gif; the still frame ships as
  `assets/icons/About.png` (16 px, 213 PNGs now; the icons test
  counts updated). DEVIATIONS recorded (vs the C# ToolStrip):
  no mnemonic-activation chain (labels show the underline; arrow +
  Enter navigate/activate), hover switching is hand-built
  (open-count race is theoretically possible), the C# 500 ms
  re-close debounce stays out, dynamic submenu fills (T4) must
  rebuild popover content (the widget rebuilds from node lists).
  BINDING COST: `gtk4` features `v4_6` + `v4_10` (MenuButton
  set_child/active) — which deprecates the GTK3-era widget family
  the port uses (TreeView, Dialog, FileChooserNative, ...), so
  cr-ui now mirrors the workspace lints with `deprecated = allow`
  (commented; CI is GTK 4.14, compile-time only). Gate: 5 menubar
  unit tests (the 4 carried + the icon/Designer gate + the accel
  display format), 294 workspace tests, fmt/clippy clean,
  `commands_probe` 69/69, `menubar_probe` opens the File popover
  headlessly (open_top — impossible with the model bar), the
  Xvfb screenshot shows icons + accels + disabled graying.
  **RETEST (the T3 acceptance — replaces the earlier list):**
  1. `cargo run -p cr-app --release --` — the menubar shows under
     the header; every menu ITEM that ComicRack gives an icon
     shows the same icon here (spot-check File, Read, Display).
  2. Open a comic — the menubar hides; Alt alone reveals; Alt
     again hides. Click a top menu — the popover opens; hovering
     the next top menu switches to it.
  3. Arrow keys inside an open menu: Up/Down moves, Enter
     activates, Left/Right walks the top row; Esc/click-away
     closes.
  4. Fire Read ▸ Next Page from the menu — the page turns and the
     menu closes; the bar re-hides (AutoHideMainMenu).
  5. Compare the six menus against ComicRack — items present or
     absent for a recorded reason (the lists above).
  6. Check/radio state: Display ▸ Page Layout carries the check on
     the current fit/layout; Ctrl+0 flips Right to Left; Ctrl+Shift+0
     flips Only fit if oversized; Ctrl+S and Alt+Shift+S flip the
     Auto Scrolling checks; F10/F11 check AND hide the bar —
     leaving restores it; Browse ▸ Browser/Sidebar follow their
     toggles; Edit ▸ Track current Page flips. Reopen the menu —
     the marks moved.
  7. Disabled state: no book open greys Read and the bookmarks (and
     Close/Close All); a selection greys/un-greys My Rating; Zoom
     In/Out need a book.
  8. Zoom: Ctrl+= / Ctrl+- / Toggle Zoom (Ctrl+Alt+Z) and the
     100–400 % presets work in the reader; the menu shows the C#
     shortcuts.
  9. Accelerator DISPLAY text reads "Ctrl+Shift+X"-style like CR.
- T3 FIX ROUND 1 (2026-09-05), user-tested FAIL → fix → retest
  pending. Symptom: clicking one top menu left the whole window
  unclickable, console full of "Tried to map a grabbing popup with
  a non-top most parent". Root cause (evidence-first): the warning
  is in the WAYLAND backend only
  (`gdk/wayland/gdkpopup-wayland.c:981`, `can_map_grabbing_popup`
  904-918 — an autohide popup may map only when its parent is the
  current TOP-MOST grabbing popup; X11 has no such rule, which is
  why Xvfb never reproduced it). The rework's top row used
  `MenuButton`s: the hover controller set the next button active
  WHILE the previous popover still held the Wayland grab → the new
  popover failed to map but `gdk_seat_grab` had already succeeded →
  a live grab with no visible popup → every click swallowed. Two
  warnings in one instant = the pointer crossed two top buttons in
  one motion. Fix: the `GtkPopoverMenuBar.set_active_item` state
  machine ported (`gtkpopovermenubar.c:124-175` is the spec): ONE
  active slot (Rc<Cell<Option<usize>>>); `set_active_item` pops
  down EVERY other mapped popover first, then presents the target;
  the click handler toggle-closes the open menu; hover and
  Left/Right route through the same funnel; every popover close
  clears the slot (guarded against a stale old-popover close
  clearing a new one). The row focus grab moved off the map
  callback into an idle (it ran inside the Wayland grab setup).
  MenuButton is gone from the top row (plain flat Buttons +
  explicit `popover.popup()/popdown()`; popovers parent
  explicitly). Probe covers the switching sequence (File → Edit →
  Help); 294 tests, clippy clean. LESSON: on Wayland NEVER present
  a second popover while one is open — popdown first, every time;
  and the grab-focus-on-map pattern belongs in an idle.
- T3 POLISH (2026-09-05, user feedback): the submenu rows rendered
  as outlined buttons — `MenuButton` draws its own frame and the
  `flat` class does not reach the inner toggle button. Fix:
  `set_has_frame(false)` on the three submenu rows. Probe dwell
  moved to the Display menu (it carries the three submenus, so the
  screenshot proves the row shape). 294 tests, clippy clean.
- T3 FIX ROUND 2 (2026-09-05), user test FAIL → fix → retest
  pending. Symptom: EVERY menu-item click was a silent no-op while
  the accelerators worked (Ctrl+N turned pages; Read ▸ Next Page
  did nothing; Restart/Exit dead). Root cause: the row click
  handler STRIPPED the "win." prefix before `activate_action` —
  GTK resolves actions through action GROUPS, so the bare
  "next-page" found no group and failed silently. Fix: pass the
  FULL detailed name ("win.next-page"; radio targets
  "win.page-fit::original" + the value as the explicit parameter —
  the detailed form GTK parses). Probe proof: `MenubarWidget::
  click_row` walks the REAL widget path (`emit_clicked` → handler →
  popdown + activate); the menubar probe reads the
  `track_current_page` setting around a direct activation AND a
  programmatic row click — DIRECT true→false, CLICK false→true
  (the round-2 bug class is now gated end to end). Probe lesson:
  `clone_handle` originally DROPPED the sync rows (empty Vec) —
  the handle could not click or sync; rows moved into
  `Rc<Vec<ItemRow>>` shared by every clone. User-reported File-menu
  report disposition: Generate Cover Thumbnails / Tasks / New
  fileless Book Entry = intentional disabled stubs (owning tasks
  recorded); Automation, Open Remote Library, Open Books, Recent
  Books = recorded omissions (ADR-024 + T4 dynamic fills); "New
  fileless Book Series..." is NOT a built-in menu item — it is the
  bundled IronPython script `Output/Scripts/NewComics.py`
  (`#@Hook NewBooks`) that surfaces under the C# Automation
  submenu, so it is covered by the Automation omission; a
  BACKLOG item (docs/port-plan.md §6) notes porting selected
  bundled scripts (NewComics.py first) natively instead of the
  Phase 6 Python host. C# parity note for T4: "Update all Book
  Files" hides when AutoUpdateComicsFiles is on
  (fileMenu_DropDownOpening) — not yet ported.
- T3 ROUND 3 (2026-09-05), user-reported Browse-menu findings →
  fix + tracker. FIXED: Browse ▸ Library/Pages now carry the
  ACTIVE emphasis — the C# has no checkbox on those items; the
  shown panel highlights the row (`ActionState.highlight`, the
  `menu-row-active` CSS class, sync_menubar reads panel_stack).
  The probe proves the move (start on Library → click Pages →
  only Pages highlighted). PROBE LESSON (big one): the probe
  DROPPED the BrowserShell after `connect_activate` — the app
  keeps it in the BROWSER thread-local, the probe didn't — and
  every shell action handler holds `Weak<ShellState>`, so each
  dispatch became a SILENT NO-OP (no panic, no log). All earlier
  "view-library does not activate" evidence was the probe killing
  its own shell; the track-current-page "success" was its inline
  handler that never touches shell state, and the "highlight" was
  the install-time default. `std::mem::forget(shell.clone())` in
  the probe fixes it; the real-app clicks were already fixed in
  round 2. DEBUG-LESSON: stdout/stderr interleave UNRELIABLY when
  piped (stdout is block-buffered) — debug prints that must be
  order-compared go through println! on one stream.
- T3 COMPLETE — USER-TESTED, ALL PASS (2026-09-05; three fix rounds
  + two polish rounds). The user verified across the rounds: the
  six menus with the C# icons, the Wayland grab fix (menus stay
  clickable, hover/arrow switching), the row clicks firing (Next
  Page, Exit, Restart, radios), the active-panel row highlight on
  Browse ▸ Library/Pages, no arrow + left-edge alignment, submenu
  rows without the button frame, accel display text, disabled
  stubs grey. **Next: T4 (the dynamic menus).**
- T4 IMPLEMENTED (2026-09-05), user test pending. The dynamic
  fills (the C# `DropDownOpening` rebuilds) for Open Books, Recent
  Books, Bookmarks, Page Type, Page Rotation + the bookmark
  commands themselves:
  - `menubar.rs`: a `MenuNode::Dyn(id)` slot — the shell installs
    a fill provider (`set_dyn_fill`) and every menu open rebuilds
    the slot's rows (`refresh_top` runs at the click/hover/arrow/
    open_top funnel, BEFORE the popover maps — the C#
    `DropDownOpening` shape). The fill BAKES checked/disabled (the
    C# also refreshes at open, not through command states); dyn
    rows skip the state sync (base = ""). New `set_sub_enabled`
    (the parent enables: Open Books/Recent Books/Page Type/Page
    Rotation) and `ActionState.visible` (the
    `fileMenu_DropDownOpening` hide rule — "Update all Book Files"
    hides while `AutoUpdateComicsFiles` is on; the T1-postponed
    entry). Probe accessors: `dyn_rows_snapshot`.
  - Fills: Open Books = one row per open tab (caption through
    `display_text::caption` — `GetSlotCaption` → `Comic.Caption`
    parity), checked on the current, Ctrl+Alt+F1..F12 on the first
    12 (registered per slot value at fill time). Recent Books =
    `library::recent_books(20)` (`GetRecentFiles` parity:
    OpenedTime desc, `RecentFileCount` = 20, existing files only,
    numbered "N - filename"). Bookmarks = the per-page list after
    the C# "bms" separator ("name (Page N)", disabled on the
    current page, `win.open-bookmark::<provider>`). Page Type =
    the 11-value enum radio over the CURRENT page (the editor's
    shared `PAGE_TYPE_ITEMS` table; all rows disabled without a
    book — `pageEditor.IsValid`). Page Rotation = the
    None/90/180/270 radio with the C# Permanent icons.
  - Bookmark commands (`SetBookmark`/`RemoveBookmark`/
    `DisplayPreviousBookmarkedPage`/`DisplayNextBookmarkedPage`):
    `cr-core::ComicInfo::seek_bookmark` (the C#
    `ComicPageInfoCollection.SeekBookmark` — start page counts,
    the callers pass `current + dir`), `ReaderShell::bookmark_nav`
    (provider-space seek → display-sequence navigation), the Set
    flow opens the new `dialogs::name_prompt` (`SelectItemDialog
    .GetName` shape; the proposal = the existing bookmark or "Page
    N") and writes through `ComicInfo.UpdateBookmark` semantics
    (empty clears) + `apply_edited` (the dirty mark + the gated
    file write). The reader KEYS (Ctrl+PageUp/PageDown →
    MoveToPrevBookmark/MoveToNextBookmark) now forward from the
    view through the shell (the dispatch no-op is gone).
  - Page Type/Rotation SET: `win.page-type::<value>` /
    `win.page-rotation::<value>` → `edit_open_book` (the session
    book mutates, `apply_edited` mirrors, the Pages panel rebinds)
    + the view rotation map for rotation
    (`PageView::set_page_rotation_for`). The Y/Shift+Y
    page-rotate commands now write through too
    (`PageRotateC`/`CC` forward to the shell → the view applies +
    the book copies mirror). The stored rotations now seed the
    view at OPEN (`ComicPageInfo.Rotation` seeds the map through
    the display sequence — the C# render pipeline reads them per
    page).
  - My Rating check states: the rating actions became STATEFUL
    (and joined the actions registry — they were silently absent
    from the enable sync, a T1 gap the probe caught);
    `selection_common_rating` = the `RatingEditor.GetRating` port
    (the common value, -1 mixed), check = `round == n`.
  - `refresh_view_from_list` now RESTORES the selection after the
    book-set swap (`ItemView::reselect` — the C# refresh updates
    items in place; without it every rating commit cleared the
    selection and the check never showed).
  - FIXED (the T3 regression the probe caught): the row-click
    handler passed the DETAILED action name PLUS an explicit
    parameter — `activate_action` parses a detailed name only when
    no args ride along; the combo errors silently, so the T3
    RADIO rows (page-fit/page-layout/zoom presets) never fired
    from clicks (accels kept working). Both click handlers (the
    static + the dynamic builder) now pass the BARE name + the
    value as the parameter. The `dynmenus_probe` gates the row
    click on a parametered target.
  - DEVIATIONS recorded (the tracker section): Recent Books is
    text-only (the C# fetches 16 px cover thumbs at menu-open);
    a bookmark on a Deleted page has no display position and is
    unreachable (the C# navigates provider space); the hide rule
    for Update all Book Files runs in the continuous sync (the C#
    refreshes at menu-open); the recent-books label uses the raw
    file name (no `GetSafeFileName` ellipsis — it IS the file
    name); slot accels live from the first fill (no per-open
    teardown).
  - **USER TEST (the T4 acceptance):** 1. Open three
    comics — File ▸ Open Books lists all three, checks the
    current, Ctrl+Alt+F1..F3 switch tabs; the grey parent turns on
    with the first open. 2. Set Bookmark (Ctrl+Shift+B) — the
    prompt proposes "Page N"/the old name; OK → Edit ▸ Bookmarks
    lists "name (Page N)" (grey on the current page); clicking
    another comic's tab makes the row clickable → it jumps back.
    Remove Bookmark clears it (the row vanishes). 3. Prev/Next
    Bookmark (Ctrl+Shift+P / the unbound next) walk the bookmarks;
    grey when no bookmark lies before/after. 4. Rate 4 stars
    (Alt+Shift+4 or the menu) — Edit ▸ My Rating checks the 4-star
    row; a mixed selection unchecks all. 5. Page Type/Page
    Rotation (Edit menu) — the radio marks the current page; set
    another type/rotation — the reader re-decodes (rotation), the
    Pages panel follows; Y/Shift+Y keep working. 6. File ▸ Recent
    Books lists the opened books; clicking opens. 7. Preferences →
    turn Auto Update Comics Files ON — "Update all Book Files"
    hides from the File menu (reveal again with it OFF).
- T4 FIX ROUND 1 (2026-09-05), user test: findings 1 + 7 → fixed,
  retest pending. (1) The dynamic SUBMENU content only refreshed
  when the TOP menu opened — the fill funnel sat in the top-menu
  open path (click/hover/arrow), so revisiting the submenu inside
  an already-open menu showed the stale check. Fix: the fill
  rebuild hooks each dynamic slot's host (child) popover MAP —
  every submenu open re-fills (the C# `DropDownOpening` fires for
  nested drop-downs too). Probe: the REMAP gate (switch slot →
  refresh the slot the way the map does → the check moves without
  a top reopen). (7) The Preferences OK path never re-ran the
  sync, so the "Update all Book Files" hide rule waited for the
  next unrelated dispatch (an app restart made it look
  settings-driven). Fix: `show_preferences`'s callback runs
  `sync_enabled` after applying. All gates re-run: 296 tests,
  `dynmenus_probe` (now 6 gates incl. REMAP), `menubar_probe`,
  `commands_probe` 69/69. **RETEST:** re-run the two failed items
  (1: switch tabs with the File menu open — revisiting Open Books
  shows the check on the new current; 7: toggle Auto Update Comics
  Files in Preferences — the File-menu item flips immediately).
- T4 COMPLETE — USER-TESTED, ALL PASS (2026-09-05; one fix round).
  The user verified the retest (stale-check REMAP + the immediate
  hide rule) and the rest of the acceptance ("Rest is OK"). One
  extra observation DEFERRED to T9 (see the T4 tracker entry): the
  menubar hides in the browser view while a book stays open in a
  reader tab. **Next: T5 (the reader toolbar).**
- T5 IMPLEMENTED (2026-09-05), user test pending. The nine-button
  reader strip (`mainToolStrip`):
  - `menubar.rs` grew the reusable `Dropdown` (`build_dropdown`):
    the same row builder/state sync as the menubar popovers, one
    popover per instance (`open` refreshes the top-level fills
    first, `sync`/`click_row`/`dyn_rows_snapshot` shared) — the
    menubar and the toolbar resolve the SAME action states through
    one closure in `sync_menubar`.
  - `toolbar.rs`: the strip in Designer order — prev/next split
    buttons (main click = page turn via `CommandMapper` parity),
    page layout / fit (drop-only — the C# split buttons have no
    main-click handler), zoom (icon + "NNN%" text), rotate (icon +
    "NN°" text), magnifier (toggle; Zoom/ZoomClear icon), full
    screen (toggle), tools (the flattened menu: open/info/
    Bookmarks+list/AutoScroll/Minimal/Undock/Scan/Update/
    Thumbnails/DisplaySettings/Preferences/About/Show Main Menu/
    Exit; ADR-024 omissions absent). The dropdowns reuse the
    menubar tables (PAGE_LAYOUT) + the fit/zoom/rotate tables;
    layout/fit icons track the reader state (the C#
    `GetFitModeImage`/`GetLayoutImage` — the RTL variants bundle).
    The whole bar mounts right-aligned above the reader content
    (the C# Dock=Right inside the tab row — Fill-mode placement
    into the browser tab strip is T9). `sync_visibility(has_book,
    !minimal)` gates the reader-only buttons (`OnUpdateGui`) and
    the whole bar (MinimalGui).
  - The toolbar rides the UNDOCK (`ReaderForm` keeps the strip):
    `ReaderShell::set_undock_chrome(widget, docked_parent)` moves
    it above the undocked view and back on re-dock.
  - `win.show-main-menu` (stateful check = !AutoHideMainMenu — the
    `tbShowMainMenu` command port; flips the setting + re-applies
    the menubar rule immediately).
  - FIXED on the way: `PageView::do_zoom` returned early with no
    composed page — the C# `ImageZoom` setter stores
    unconditionally (a preset before the first decode silently
    dropped); now the zoom stores and the next compose renders at
    it. Plus `PageView::zoom()/rotation()/magnifier_visible()` and
    `ReaderShell::current_zoom/current_rotation/current_magnifier`
    for the state text.
  - Dyn fills: `bookmarks-prev`/`bookmarks-next` (the C#
    `UpdateBookmarkMenu(direction)`: the bookmarks before/after the
    current page, nearest first for the backward drop, all
    clickable).
  - Gate: 299 tests (+3 toolbar gates: actions exist, the
    layout-table reuse, every icon resolves), fmt/clippy clean;
    `toolbar_probe` (the bar mounts, the zoom text 200%, the
    rotate text 90°, the fit dropdown row click fires, the
    next-page drop lists the bookmark), the other probes
    unchanged.
- T5 FIX ROUND 1 (2026-09-05), user crash report → fixed, retest
  pending. SYMPTOM: clicking toolbar buttons after opening a
  comic segfaulted — `gtk_widget_realize() on a widget that isn't
  inside a toplevel`, then `gdk_surface_new_popup: no parent
  surface` → SIGSEGV. ROOT CAUSE (evidence-first): the standalone
  `Dropdown` popovers were NEVER parented — the menubar popovers
  get `set_parent(&button)` in `create_menubar`, but
  `build_dropdown` skipped that step, and the probe never called
  `open()` (it only clicked rows — `click_row` pops down, never
  presents), so headless gates could not see it. FIX: `Dropdown::
  open` parents the popover to its ANCHOR BUTTON on first open
  (parenting to the window would break the undock — the popover
  must follow the toolbar across toplevels); every dropdown now
  stores its anchor button (the split-button helper returns the
  main part). The probe grew the OPEN gate: `open_dropdown("next")`
  through the real anchor → `is_mapped() == true`, alive, clean
  exit. LESSON: every popover needs a parent BEFORE popup(); a
  probe that only clicks rows never exercises the present path —
  gate the OPEN, not just the click. 299 tests, all probes green.
- T5 COMPLETE — USER-TESTED, ALL PASS (2026-09-05; one fix round:
  the unparented dropdown-popover segfault). The user verified: the
  strip mounts with the C# icons, the page-turn main clicks, all
  seven dropdowns open and fire (radios, bookmark rows), the
  state text (zoom %/rotation °) tracks, the undock carries the
  strip, no crash. **Next: T6 (the browser toolbar reorg + the
  Detail column chooser).** **USER TEST (the T5 acceptance):**
    1. Open a comic — the strip sits at the top right of the
       reader: [prev][next] | layout fit zoom% rotate° | magnifier
       fullscreen | tools, with the C# icons.
    2. Click the next-page main part — the page turns (the prev
       part turns back); the chevrons open the drops: prev = First
       Page/Previous Bookmark/bookmarks-before/Previous Book from
       List; next = Last Page/Next Bookmark/Last Page Read/
       bookmarks/Next+Random Book.
    3. Zoom presets set the % text; Rotate 90 sets the angle text;
       the fit/layout icons track the mode; the magnifier icon
       flips with M.
    4. The layout/fit/zoom/rotate drops carry the radio dots (fit
       original after clicking it).
    5. MinimalGui (F10/K) hides the whole strip; leaving restores.
    6. Close the comic — the reader-only buttons (prev/next/
       layout/fit/zoom/rotate/magnifier) hide with the reader
       page.
    7. Tools: About/Preferences/Display Settings open their
       dialogs; Auto Scrolling/Minimal/Full Screen check-flip; Show
       Main Menu flips the menubar auto-hide (the bar stays visible
       with a book open when checked OFF... actually the Alt-reveal
       stops hiding the bar).
    8. Undock (D) — the strip rides into the undocked window and
       works; D returns it.
- T6 IMPLEMENTED (2026-09-05), user test pending. The browser
  toolbar (`browser/browser_toolbar.rs`): the strip ABOVE the
  browser panes — Sidebar toggle, Browse Previous/Next (the list
  history), Views (drop: the three view radios + the read-state
  radios + the comic-type checks + Show Duplicates), Group +
  Arrange (the dynamic CreateGroupMenu/CreateArrangeMenu tables —
  Not Grouped/Not Sorted first, the columns as stateful rows), the
  right-aligned Quick Search (with the C# scope menu on the
  entry's secondary chevron), a disabled List Layouts button, the
  Duplicate List drop (the folder walk, indent per level). The old
  header (Open/Add Folder/Preferences/View/Sort/Group) folds into
  the menubar + this strip; the header carries the reader page
  display only. New stateful actions: `view-filter` (all/unread/
  reading/read), `comic-type` (books/fileless, the C# toggle-back
  shape), `duplicates-only`, `search-scope` (all/series/writer/
  artists/descriptive/catalog/file — the AllProperties option),
  `toggle-column`, `duplicate-list` (folder parameter); sort-column
  and group-by became STATEFUL (check marks on the active row; ""
  = Not Sorted / Not Grouped — `ViewState::clear_sort` added).
  The composed filter (`compose_quick_filter`): read-state
  (ComicBookReadPercentageMatcher), comic-type (ComicBookFileMatcher
  Not), the AllProperties text (op 3 ContainsAll, the C# enum
  option name), the duplicate matcher on top — unit-tested in
  `shell.rs::tests` (5 tests: scopes, read states, types, dups,
  the MATCH-query-only-for-All gate). The Detail column chooser:
  the ItemView right-click routes a header hit to the shell's
  chooser popover (`Dropdown` + the `detail-columns` dynamic
  fill; every registered column with its visibility check, live
  toggling through `win.toggle-column`). The Duplicate List engine:
  `library::duplicate_smart_list` (the matcher-values name + the
  NumberedString numbering, ported in `cr-engine/src/text.rs`,
  base_list_id parity) + `list_folders`. Probe `browserbar_probe`:
  the Views/Duplicate OPEN gates, the filter narrows 3/1/1/1/3, the
  scoped search hits, the chooser opens + toggles Series, the
  duplicate lands (+1 tree node). commands_probe/menubar_probe/
  toolbar_probe/dynmenus_probe all green (regression).
  **USER TEST (the T6 acceptance):**
    1. The browser shows its own toolbar row: Sidebar | prev next |
       Views Group Arrange ......... search field | (grey) list
       layouts | duplicates icon. The menubar stays on top; the
       old header buttons are gone.
    2. Sidebar toggles the left panel away and back; prev/next
       walk the visited lists (disabled until you switch lists).
    3. Views ▸ Tiles/Details/Thumbnails switch the grid with the
       check on the active one.
    4. Views ▸ Show Read/Reading/not Read narrow the grid; Show
       Duplicates keeps only books that share series+number.
    5. The search box: the chevron opens All/Series/Writer/
       Artists/Descriptive/Catalog/Filename; picking one changes
       the placeholder and the next search matches only that
       field; `MATCH [Series] contains "X"` still parses.
    6. Details view: right-click a column header — the chooser
       lists every column with checks; toggle Series/Writer live.
    7. Arrange ▸ Not Sorted clears the sort; a column sorts and
       shows its check; the button label becomes the column name.
       Group ▸ Series groups the grid; the Group button label
       becomes "Series".
    8. Duplicate List: open the drop (shows your folders), pick
       one — a new smart list appears under it (named from the
       active filter text, or the list name + "(2)").
- T6 COMPLETE — USER-TESTED, ALL PASS (2026-09-05; four fix
  rounds). Round-1 report → fixed: (a) Browse ▸ Browser did
  nothing from the QuickOpen page (`toggle_browser` now shows the
  browser from `quickopen`, not only from the reader); (b) the
  Views/filter/scope check marks never moved (the stateful action
  handlers set state but never ran the sync that re-renders the
  rows — every handler now calls `sync_enabled`, and `view-mode`
  state derives from `item_view.mode()` as the source of truth);
  (c) the browser toolbar covered the navigator (moved into the
  RIGHT pane above the item view — `item_box` = toolbar +
  scroller, the paned end child); (d) Tools/Fullscreen were hidden
  in the library view (the reader toolbar moved OUT of the reader
  stack page to a `toolbar_box` UNDER the menubar, above the
  stack — the C# `OnGuiVisibilities`/`OnUpdateGui` keeps
  MainToolStripVisible in both views, and only prev/next/layout/
  fit/zoom/rotate/magnifier gate on a book; the undock re-docks
  into `toolbar_box`). Round-2/3/4 (the column chooser): the
  right-click hit test + hook fired fine (the `CR_DEBUG_CHOOSER`
  trace confirmed `header_hit=true` + `CHOOSER popup`), but the
  popover never MAPPED on the user's Wayland — the T5/T6
  `build_dropdown` popover (has_arrow off + submenu child
  popovers) does not map when parented to the top-level window on
  Wayland. Fix: the chooser is a PLAIN `gtk4::Popover` of
  `CheckButton` rows (the exact Wayland-proven shape of the book
  context menu), parented to the window, unparented on close;
  then the popover was two rows tall (the `ScrolledWindow`
  propagated natural WIDTH but not HEIGHT) — added
  `propagate_natural_height(true)` + `max_content_height(480)`.
  **T6 is COMPLETE. Next: T7 (the navigator + Pages toolbars).**
  LESSON (recorded in the tracker): a `build_dropdown` popover is
  anchor-parented by design; a window-parented context popover on
  Wayland must be a PLAIN popover (the book-menu shape), and a
  scroller in a popover needs BOTH natural-size propagations.

### T5 — Reader toolbar (COMPLETE — see the closure entry in the progress log)
- DEVIATIONS (vs the C# ToolStrip):
  - The strip mounts as a right-aligned row ABOVE the reader
    content, not Dock=Right inside the tab row (the C# moves the
    strip INTO the browser's tab strip in Fill mode — that
    placement is T9 dock-mode work).
  - The prev/next drops open DOWNWARD from the chevron (the C#
    ToolStripSplitButton opens its drop below the whole button);
    the chevron is a separate click target (a GTK split look via
    the `linked` CSS class, not one composite widget).
  - The bookmark rows in the prev/next drops navigate by provider
    page (the same Deleted-page caveat as the Edit ▸ Bookmarks
    fill — a bookmark on a Deleted page lists but cannot jump).
  - The zoom/rotate buttons are DROP-ONLY (no main click); the C#
    split buttons have no main-click handler either — parity.
  - The Tools menu omits the ADR-024 omissions (Open Remote
    Library, Workspaces, Update Web Comics, Synchronize Devices);
    the Bookmarks submenu inside it carries the dynamic list.
  - `Generate Cover Thumbnails` stays a disabled stub (the
    thumbnail-queue work owns it).
- LESSON (the crash round): every popover needs a parent BEFORE
  popup(); a probe that only clicks rows never exercises the
  present path — gate the OPEN, not just the click.

### T6 — Browser toolbar reorg + Detail column chooser (COMPLETE — USER-TESTED)
- FIX ROUNDS (user test, 2026-09-05):
  - Browse ▸ Browser did nothing from the QuickOpen page —
    `toggle_browser` now shows the browser from `quickopen`.
  - The Views/filter/scope CHECK MARKS never moved — the stateful
    handlers set state but never ran the sync that re-renders the
    dropdown rows; every stateful handler now calls `sync_enabled`,
    and `view-mode` state derives from `item_view.mode()`.
  - The browser toolbar covered the navigator — moved into the
    RIGHT pane above the item view (`item_box` = toolbar +
    scroller, the paned end child).
  - Tools/Fullscreen were hidden in the library view — the reader
    toolbar moved OUT of the reader stack page into a `toolbar_box`
    UNDER the menubar (above the view stack); the C#
    `OnGuiVisibilities` keeps MainToolStripVisible in both views,
    and `OnUpdateGui` gates only prev/next/layout/fit/zoom/rotate/
    magnifier on a book (Fullscreen/Tools always show). The undock
    re-docks into `toolbar_box`.
  - The column chooser popover fired (`header_hit=true` +
    `CHOOSER popup`, per the `CR_DEBUG_CHOOSER` trace) but never
    MAPPED on Wayland: a `build_dropdown` popover (has_arrow off +
    submenu child popovers) does not map parented to the top-level
    window. Fix: a PLAIN `gtk4::Popover` of `CheckButton` rows
    (the book-context-menu shape). Then it was two rows tall — the
    `ScrolledWindow` propagated natural width only; added
    `propagate_natural_height(true)` + `max_content_height(480)`.
- LESSON (Wayland): a window-parented CONTEXT popover must be a
  PLAIN popover (the book-menu shape), NOT a `build_dropdown` one
  (those are anchor-parented — arrow-less + child popovers only map
  from a widget anchor, not the top-level window). A scroller
  inside a popover needs BOTH `propagate_natural_width` AND
  `propagate_natural_height`, else it collapses to ~2 rows.
- DEVIATIONS (vs `ComicBrowserControl.toolStrip`):
  - Stack omitted (no ItemStacker port; `tbbStack` sits between
    Group and Sort in the C# order) — covered by the ADR-024
    stack-family cut.
  - Undo/Redo absent (ADR-024; the C# toolbar carries tbUndo/
    tbRedo).
  - List Layouts is a DISABLED icon button (no drop); the C#
    drop items (Edit List Layout Ctrl+L, Save List Layout, Reset
    List Background, Edit Layouts Ctrl+Alt+L) land with the T14
    workspace data.
  - The Group/Arrange buttons keep static icons + labels; the C#
    flips the SortUp/SortDown icon and shows the active COLUMN
    NAME as the button text via OnIdle — the port syncs the label
    text (sync_labels) but not the direction icon... it does flip
    the icon (SortUp/SortDown with the first sort key).
  - The Arrange/Group menus list the DEFAULT-VISIBLE columns (the
    C# lists every column with a comparer and nests the
    not-recently-visible ones via ContextMenuBuilder — the port
    has no nested-column-submenu machinery; the check mark + the
    MRU chain keep the behavior).
  - The C# Quick Search cue array OMITS Catalog (an index that
    would throw); the port gives Catalog a cue ("Search Catalog").
  - The Views drop omits Collapse/Expand all Groups + Show Group
    Headers (the C# `itemView.ToggleGroups` +
    `ShowGroupHeaders` — the group-header side list is unported;
    the collapsed-group state stays per-group from the header
    clicks).
  - No Stack button; no Undo/Redo.
  - The Quick Search scope menu rides the entry's secondary icon
    (a chevron in the box) — the C# `TextBox.SearchMenu` shape is
    the closest GTK4 equivalent; the C# entry also doubles its
    width while focused (skipped).
- The column chooser omits the C# header-menu extras (Auto Size
  Column / Auto Size All Columns / Auto Fit All Columns and the
  Layout submenu — the port's column widths are static; the
  chooser carries the check-list only). Column VISIBILITY state
  persists in T14 (session-only now).
- The composed filter (search + view-filter + comic-type +
  duplicates) rebuilds on every part's change; the C#
  `UpdateQuickFilter` shape is preserved: a MATCH/NOT query parses
  only for the All scope and then the view filters do NOT apply;
  the Create path passes operator 3 (ContainsAll) with the RAW
  text (the pre-T6 port used operator 1 + the trimmed text).
- `NumberedString` (cYo.Common.Text) ported in `cr-engine/src/
  text.rs` for the Duplicate List naming (the MaxNumber/Format
  bracket numbering, first-match GetNumber quirk included).
- Duplicate List: the smart list carries the COMPOSED filter (the
  C# GetCurrentMatcher merges quickFilter + the selector panel;
  the selector panel is unported) and lands in the picked folder;
  the C# None-target (TemporaryFolder) is unreachable (the None
  row is only a disabled placeholder).
- T7 IMPLEMENTED (2026-09-05; COMPLETE — USER-TESTED, see the
  closure entry at the T7 fix round). The two panel
  toolbars:
  - Navigator (`ComicListLibraryBrowser.toolStrip`, the control's
    own chrome — `browser/navigator.rs` now mounts
    [toolbar][search box (hidden)][tree] instead of the bare
    scroller): New Folder / New List / New Smart List (the SAME
    ListCommand path as the context menu, target = the current
    selection — the C# commands.Add(miX, tbX) pairing), a
    separator, Expand/Collapse All (`ExpandCollapseAllNodes`: any
    row expanded → collapse_all else expand_all; the row-expanded/
    collapsed signals keep the id set in step), Refresh (a new
    `connect_refresh` — the shell refills the tree + re-evaluates
    the current list; the C# RefreshLists event has NO subscriber
    for the local library — a decompiled dead wire, the port makes
    it the FillListTree refresh), and the right-aligned Quick
    Search toggle (`tsQuickSearch` → `ToggleQuickSearch`: show +
    focus, or clear + hide). The T1 `toggle-navigator-search` stub
    became a REAL stateful check action (check = the box
    visibility, `() => quickSearchPanel.Visible`); the Ctrl+Alt+F
    accel was already registered. The search box (cue text = the
    toggle's tooltip, the C# `SetCueText(tsQuickSearch.Text)`
    parity) filters the tree on every keystroke
    (`quickSearch_TextChanged` → FillListTree): Library rows always
    show, folders pass when ANY child passes (the
    `ComicListItemFolder.Filter` override — the folder's own name
    is NOT searched), every other item by a case-insensitive name
    contains (`ComicListItem.Filter`); the filter runs inside
    `refill`, so shell-driven refills keep it.
  - Pages (`ComicPagesView.toolStrip` — `browser/pages_view.rs`
    now mounts [toolbar][scroller]): the Views split button (main
    click cycles the mode — `tbbView_ButtonClick`; chevron opens
    the drop) over the `win.pages-view-mode` radio action with the
    Thumbnail/Tile rows. The mode: Thumbnail (unchanged) + Tile —
    fixed cells (192×96 at the default thumb height, scaled with
    the size slider), thumb left / text right
    (`ThumbTileRenderer`), the text lines = the
    `ComicTextBuilder.GetTextBlocks` `DefaultPage` port ("Page #N"
    bold, the page type, "Size: …"/"Unknown Size", "Resolution:
    W x H", optional "Rotation: N°"/"Bookmark: name") with the
    shared tab-stop shape. PageCell now carries the tile fields
    (type name, file size, dims, rotation, bookmark name).
    The panel exposes `sync(&resolve)` — the shell pushes the same
    action states the menubar and the other toolbars get.
  - Gate: 314 tests (+7: the filter unit tests ×2, the tile-lines
    tests ×2, the actions-exist gate, the icon gate, the mode-name
    round trip); `navpages_probe` gates the command dispatch (the
    recorder), the search toggle + the 9→1→9 filter, the expand/
    collapse flip (1→0), the Views OPEN through the real anchor
    (the T5 lesson), the Tile radio row click + the main-click
    cycle, and the action state following the PANEL (the T6
    source-of-truth rule); all other probes unchanged (the
    GLib-GIO-CRITICAL line in commands_probe/toolbar_probe output
    is pre-existing HEAD noise — verified against a clean
    worktree).
  - **USER TEST (the T7 acceptance):**
    1. `cargo run -p cr-app --release --` — the Library panel
       shows its own toolbar: [New Folder][New List][New Smart
       List] | [Expand/Collapse All][Refresh] ... [search icon] —
       the C# icons.
    2. The three New buttons create/edit the same things as the
       context menu (a click with the Smart Lists folder selected
       inserts under it, the editor opens).
    3. Expand/Collapse All: one click expands every folder, a
       second collapses all.
    4. Refresh: nothing visibly changes on a quiet library (the
       tree refills — same shape as the context-menu mutations
       leave behind).
    5. The search icon shows the search box under the toolbar
       (focused); typing "bat" narrows the tree to the matches
       (folder names do NOT match — only list names and their
       children); clearing + the icon again hides the box;
       Ctrl+Alt+F toggles it too (and the menubar-less state still
       works — the action carries the accel).
    6. Open a comic → the Pages panel; its toolbar shows the
       Views split button. The chevron opens the drop (Thumbnail/
       Tile with the radio mark); Tiles switches the grid to the
       tile cells (thumb left, "Page #N"/type/size/resolution
       text right); the main part cycles back to Thumbnails.
    7. Ctrl+wheel (or the later T8 slider) resizes both modes.
    8. Book Display: the tile text shows the stored rotation and
       bookmark lines only when present (set a bookmark, rotate a
       page — reopen the Pages panel).

## Omitted / postponed per task (the tracker)

Live tracking of everything cut, deferred, or stubbed, per task.
New entries append here at the END of the task that owns them.
A task may only close when its entries here are either resolved or
re-homed (backlog / a later task / ADR-024). Cross-phase cuts and
native-port candidates live in the BACKLOG (`docs/port-plan.md`
§6); locked scope decisions live in `docs/decisions.md` (ADR-024
owns the Phase 5.5 omissions).

### T1 — Command/action layer + accelerators (COMPLETE)
- Accelerator collisions resolved by menu order (recorded): Rotate
  270 loses Ctrl+Shift+D0 to Only fit if oversized; Next Bookmark
  loses Ctrl+Shift+N to New fileless Book Entry.
- Undocked reader window gets NO shell accels (ReaderForm parity).
- Update all Book Files covers library books only (temporary books
  unported).
- Stub actions stay DISABLED until their owning task: Tasks (T13),
  Zoom Custom (T13), Display Settings (T12), About (T13), Quick
  Rating (T13), Copy/Export Page (T13), Small Preview (T11), New
  Book Entry (unported fileless books), navigator search (T7).
  RESOLVED in T4: Set/Remove Bookmark (the commands + the fills
  landed). RESOLVED in T7: toggle-navigator-search (the real
  stateful action + the navigator search box).
- Automation submenu omitted (Phase 6 scripting hooks it).
- RESOLVED in T4: "Update all Book Files" hides when
  AutoUpdateComicsFiles is on (`ActionState.visible` + the sync).

### T2 — Bundled icon set (COMPLETE)
- The 17 resx GIFs (task animations: scan/export/device-sync/
  read-info/update-info/big-small-ball) and ComicRackAppSmall.ico
  are NOT bundled — no ported consumer; T8's lamps use static
  PNGs.
- About resx is a GIF; the still frame ships as About.png.
- The `Special*.zip`/`Publishers*.zip`/`AgeRatings*.zip`/
  `Formats*.zip` icon packs (Program.cs:839-843) are not ported —
  they feed the custom-thumbnail/publisher display (deferred with
  the `type://` loader work).
- Dark* variants bundle only what the resx references (29 names).

### T3 — Menubar (COMPLETE — see the closure entry in the progress log)
- Present-but-disabled stubs (grey): Generate Cover Thumbnails
  (thumbnail-queue work), Tasks (T13), New fileless Book Entry
  (fileless books unported), Quick Rating (T13), Copy Page /
  Export Page (T13), Small Preview (T11), Zoom Custom (T13), Book
  Display Settings (T12), About (T13). RESOLVED in T4: Set/Remove
  Bookmark.
- Absent per ADR-024: Update Web Comics (WebComicProvider gap),
  Synchronize Devices, Automation (Phase 6), Open Remote Library
  (Phase 7), Undo/Redo, Devices..., Folders (F7, Phase 7), Search
  Browser, Info Panel, Workspaces, List Layout (T6/T14 data), the
  Help docs/homepage/forum/news/update links.
- Deferred to T4 (dynamic parents): RESOLVED — Open Books, Recent
  Books, Page Type, Page Rotation and the bookmark list fill
  dynamically (T4).
- NOT A MENU ITEM: "New fileless Book Series..." is the bundled
  script `Output/Scripts/NewComics.py` under the C# Automation
  submenu — covered by the Automation omission; native-port
  candidates live in the BACKLOG (docs/port-plan.md §6).
- Deviations of the custom widget (recorded): no mnemonic-
  activation chain; hover switching hand-built; the C# 500 ms
  re-close debounce out; the bar re-hides on action activation +
  Alt (Esc/click-away leaves it until the next action); the C#
  in-menu star-slider under My Rating not ported (T13 dialog
  covers it); Auto Scrolling persists session-only (the C# writes
  Config.xml).
- Fixed in-round: the Wayland grab (one active popover), the
  stripped action name (full "win." form), the MenuButton frame,
  the active-panel highlight.
- FIXED in T4 (a silent regression of this task): the radio-row
  clicks passed a DETAILED action name plus an explicit parameter;
  `activate_action` parses a detailed name only WITHOUT args — the
  combo errors silently, so the radio rows (page-fit/page-layout/
  zoom presets) fired only from their accelerators, never from
  clicks. Both click handlers pass the BARE name + the parameter
  now; the `dynmenus_probe` gates a parametered row click.
- T3 POLISH 2 (2026-09-05, user feedback): the menus popped with a
  pointing arrow centered on the button. Fix: `has_arrow(false)`
  (top popovers AND the nested submenu popovers) and the
  `GtkPopoverMenuBar`-style left-edge alignment — a
  `POP_WIDTH`-wide pointing rect at the button's left edge (GTK
  centers the popover on the rect, so the popover's left edge
  lands on the button's left edge; the fixed 274 px width replaces
  the auto-size the arrow shape used to force). Screenshot-proved
  on the Edit menu (open state, arrow gone, edges flush). 294
  tests, clippy clean.

### T4 — Dynamic menus (COMPLETE — see the closure entry in the progress log)
- DEFERRED to T9: the menubar hides in the browser view while a
  book stays open in a reader tab (the user report; the headless
  evidence probe `examples/menubarvis_probe.rs` shows the port
  matches the C# `OnGuiVisibilities` formula in all four states —
  browser/reader/browser-with-book/browser-empty. The C# default
  `ShowMainMenuNoComicOpen` only keeps the menu up with NO book
  open; `AutoHideMainMenu` (default on) hides it everywhere else,
  Alt reveals). Revisit with the T9 dock-mode work and a CR
  side-by-side; the probe stays for the re-check.
- DEVIATIONS (vs the C# fills):
  - Recent Books is TEXT-ONLY: the C# fetches a 16 px front-cover
    thumb per row at every menu-open (a synchronous thumb render
    per entry — stall-prone on real libraries); the rows carry the
    numbered file name only.
  - A bookmark on a Deleted page has no display position — the
    row lists it but the click no-ops (the C# navigates provider
    space; the port's display model skips Deleted pages).
  - The Update-all-Book-Files hide rule runs in the continuous
    sync (every action dispatch / selection change), not at
    menu-open.
  - The Open Books slot accels (Ctrl+Alt+F1..F12) register at the
    first fill and stay (no teardown for closed slots; the stale
    accel targets a dead slot id and stays inert).
  - The bookmark proposal uses the provider page number ("Page N")
    — the C# proposal is `CurrentPageAsText` in provider space
    too; with Deleted pages present the DISPLAY number differs
    (the same deviation family as the fill caption).
- KNOWN-GAP notes carried (not T4 scope): page-type changes do
  not recompose spreads (the composition model has no page-type
  input — the Phase 3 record); the Properties editor's session
  book staleness after external editor commits predates T4 (the
  clone round-trip shape).

### T7 — Navigator toolbar + Pages toolbar (COMPLETE — USER-TESTED)
- ABSENT from the navigator toolbar (all recorded):
  - tbOpenWindow "Open in New Window" (ADR-024) — joined by
    tbOpenTab "Open in New Tab": the port has no list-tab surface
    (the reader tab strip holds BOOK tabs only).
  - tbFavorites (the Favorites pane toggle — the C# inventory note
    omits the pane).
- The C# `RefreshLists` event behind tbRefresh has NO subscriber
  for the local library (only RemoteConnectionView wires it — a
  decompiled dead wire). The port gives Refresh real behavior:
  refill the tree + re-evaluate the current list.
- The navigator search box has no autocomplete list (the C#
  persists `LibraryQuickSearchList` to settings — the box is the
  scope: a filter, not a history).
- ABSENT from the Pages toolbar:
  - miViewDetails (Details mode) + miExpandAllGroups
    (Collapse/Expand all Groups) — the panel has no detail list
    and no groups.
  - tbbGroup/tbbSort (Group/Arrange) — cut per the scope line (the
    pages grid has no sort/group).
  - tbFilter "Page Filter" — omitted entirely (the kickoff allowed
    "a stub or hidden"; the panel consumes no page-type filter).
- The Tile mode is a reduced `ThumbTileRenderer`: the thumb left /
  the text right, no 3D backdrop, no state images; the rotation
  line shows "90°" (the C# shows the localized enum text
  "Rotate90").
- The mode radios ride `win.pages-view-mode` (a shell-only
  action — no C# command equivalent; the C# wires the view modes
  directly in `ComicPagesView.OnLoad`).
- PROBE LESSON (the self-caught gate): a probe counter that walks
  only the FIRST top-level subtree silently undercounts (the
  expand-all gate read 0 for the folder on row 2 — a stack walk
  missing the sibling advance); the fixed walk mirrors the
  count_rows shape (outer sibling loop + recursive children).
- T7 FIX ROUND 1 (2026-09-05), user report: Ctrl+wheel did
  nothing. Root cause: the resize handlers were NEVER wired —
  `PagesPanel::resize` existed but nothing called it, and the
  browser grid had no handler at all (the C# has BOTH:
  `PagesView.ItemViewMouseWheel` and `ComicBrowserControl.
  itemView_MouseWheel`, steps 16 within [96, 512], skipped only
  in Detail mode). Fix: a DISCRETE vertical scroll controller on
  each canvas — Ctrl resizes (+16 up / −16 down, the C# Delta
  sign) and STOPS the event, no modifier proceeds (the panel
  scrolls). No probe gate: the wheel path is only user-testable
  (the Phase 3 lesson — xdotool produces no GTK scroll events).
- T7 COMPLETE — USER-TESTED, ALL PASS (2026-09-05; one fix round:
  the never-wired Ctrl+wheel). The user verified: the navigator
  toolbar buttons match the context menu, Expand/Collapse All
  flips the tree, the search box shows/filters/hides, Ctrl+Alt+F
  works, the Pages Views drop + radio + main-click cycle work,
  and Ctrl+wheel resizes both grids. **Next: T8 (the status
  bar).**

### T9 — Workspace tab strip (COMPLETE — USER-TESTED)

The user report that drove the re-layout: the C# tab bar sits
DIRECTLY under the menubar and holds Library, Folders, Pages (if a
comic is open) and every open comic as separate FULL-WINDOW tabs —
the port's docked-reader Notebook tabs (visible only while the
reader page shows) and the left-panel Library|Pages mini-switcher
did not follow that model.

- C# spec (read before implementing): `Views/MainView.cs` (the
  `tabStrip` TabBar: tsbLibrary/tsbFolders/tsbPages + the file tabs
  + the `+` item Tag=-1 → `OpenBooks.AddSlot`; `ShowView` swaps the
  whole workspace; `OnGuiVisibility`: tsbPages.Visible =
  CurrentBook != null with the auto-switch to items[0] when the
  Pages tab is selected and the last book closes; fileTab.FontBold
  = slot == CurrentSlot; fileTab.Visible = Fill && !ReaderUndocked;
  the `tab_CaptionClick` ToggleBrowser on a re-click of the
  SELECTED item), `Views/MainView.Designer.cs` (the tab row hosts
  the docking-mode ToolStrip at the right), `MainForm.cs:593-621`
  (`MainToolStripVisible`: in Fill mode the READER TOOLBAR lives
  INSIDE the tab row — `mainView.TabBar.Controls.Add`),
  `MainForm.cs:3658-3716` (`OnGuiVisibilities` Fill branch: the
  TabBar AND the status strip ride `flag4 = !MinimalGui ||
  !IsComicViewer || (ShowMainMenuNoComicOpen && OpenCount == 0)`),
  `MainForm.cs:3098-3132` (the `+` tab → AddSlot + CurrentSlot =
  last; the 16 px cover thumbs land async — `Create tab
  thumbnails`).
- Implementation: `browser/tabstrip.rs` — the strip widget
  (`TabId` Library/Pages/Comic(slot)/Plus; reconciling `set_tabs`
  with per-slot 16 px cover thumbs through the pool thumb queue +
  a MemoryTexture conversion of the ThumbnailImage blob; the
  close buttons; the bold current-slot marker; the right-aligned
  HOST box that parents the T5 reader toolbar — the undock chrome
  keeps riding it, docked home = the strip host). `shell.rs`:
  the left panel_stack/StackSwitcher is GONE (the navigator pane
  is a plain Paned child; the Sidebar toggle hides it); the Pages
  panel is a full-window stack page ("pages"); the status label
  moved BELOW the workspace stack (the C# status strip is
  form-wide) and rides `flag4` with the strip
  (`tabstrip::tabstrip_visible`, unit-tested); the reader
  Notebook carries NO tabs (`set_show_tabs(false)` — the strip is
  the only tab UI). `reader_shell.rs`: `TabInfo`/`tab_infos()`
  (captions cached per slot — the proposed-name regex lesson),
  `has_current_book()`/`open_book_count()` (the AddSlot slot has
  no book: the reader commands and the Pages tab gate on the
  CURRENT slot's book, the menubar flag2 counts the book slots),
  `add_empty_slot()` (the `+`), `close_slot(slot)`,
  `cycle_slot(dir)` (prev/next-tab reveal the reader — the C#
  `ShowView(i)`), `on_tabs_changed` host hook, and refresh_chrome
  now fires book_changed ALWAYS (an empty slot clears the Pages
  panel). Behavior: selecting a strip item swaps the workspace;
  RE-clicking the selected item toggles browser/reader (the C#
  caption-click); Browse ▸ Library/Pages select tabs
  (`view-library`/`view-pages`); Open Books rows + prev/next-tab
  reveal the reader; `toggle-browser`'s check covers BOTH browser
  workspaces (the C# BrowserVisible holds the Library and Pages
  views); the sync derives the strip selection from the visible
  workspace (the T6 source-of-truth lesson — the first probe run
  caught the selection never moving without it).
- Probe: `tabstrip_probe` — the startup state (Library | + only,
  QuickOpen page), two opens → two tabs + Pages + bold last tab,
  the Library click, the comic-tab click to slot 0, the RE-click
  toggle, the Pages workspace, the `+` empty slot (reader shows
  blank, Pages hides, empty caption), the close button, and
  close-all → Library with Pages hidden. `navpages_probe` gained
  the pages-workspace step (the Views drop needs a MAPPED anchor —
  the full-window Pages tab must be selected first; the T5
  lesson). commands/browserbar/menubar/dynmenus/toolbar/menubarvis
  probes stay green; 315 tests.
- PROBE LESSON: a strip callback that runs INSIDE the strip's
  `on_select` borrow may not re-enter `connect_select` — the
  shell's select handler re-syncs the strip (set_selected)
  through a DIFFERENT RefCell, which is safe; the first draft
  cloned the callback out of the borrow, which does not compile
  (Box<dyn Fn> is not Clone) — call through the borrow.
- PROBE LESSON: `gtk_box_append` asserts on an ALREADY-parented
  child even into the SAME parent — the reconciling `set_tabs`
  reorder loop must skip in-place items (compare
  observe_children()[i] by pointer) and remove+append out-of-order
  ones; blanket re-append spams Gtk-CRITICALs and silently keeps
  the old order (a widget packed twice keeps its first parent).

### T9 — Workspace tab strip (COMPLETE — USER-TESTED)
- The FOLDERS tab is ABSENT (recorded decision, user-approved
  2026-09-05): the C# `tsbFolders` opens the `ComicListFolderFilesBrowser`
  (a file-system navigator over `Settings.FavoriteFolders` + the
  comics found in the browsed folder). The port has no folder
  browser engine; hiding the tab matches the C# `DisableFoldersView`
  extended setting. The Folders view is its own follow-up task (the
  strip renders it as a fixed item once the engine lands).
- The DOCK-MODE button (the tab row's right-end split: Dock
  Bottom/Left/Right/Fill + Info Panel Right, Ctrl+Shift+1..5) is
  ABSENT — the port stays Fill-only (the C# default). The second
  `fileTabs` TabBar (the comic-tab home when the browser docks
  beside the reader) does not exist. Tracked with T10.
- The `+` empty slot shows NO QuickOpen overlay (user decision,
  2026-09-05): the C# `ComicDisplayControl` hosts QuickOpen in an
  empty slot; the port shows a blank reader view. The QuickOpen
  covers remain the STARTUP page only (Phase 4 behavior).
- The tab context menu (Close / Close All But This / Close All to
  the Right / Show in Browser / Reveal in Explorer / Copy Full
  Path — `MainForm.Designer.cs:3374-3417`) is ABSENT; the close
  button and Ctrl+X/Ctrl+Shift+X cover closing. Middle-click close
  not ported (GTK Button has no middle-click default; a controller
  could add it with the context menu).
- Drag-reorder of comic tabs not ported (the C# TabBar
  DragDropReorder; the strip rebuilds from slot order — the
  C# order IS the slot order, so a drag needs a slot reorder
  command, not just a widget move).
- The tab tooltip shows the caption only (the C# appends the
  Ctrl+Alt+Fn shortcut hint); the Open Books menu rows carry no
  shortcuts either (the T4 note). The Ctrl+Alt+F1..F12 slot
  accels themselves register through the Open Books fill
  (unchanged).
- Remote-library tabs and list tabs (`AddListTab` — "Open in New
  Tab" on a list) are out (ADR-024: remote dropped; the list-tab
  surface is the T7 note's re-homed item).
- The undocked shape: the C# keeps the main window's strip fully
  visible while the ReaderForm floats (MainForm.cs:3664-3676); the
  port HIDES the comic tabs while undocked (`fileTab.Visible`
  parity) and shows the last browser workspace — a deliberate
  simplification carried from Phase 4 (one undocked reader).
- T9 FIX ROUND 1 (2026-09-05), user report, six items:
  1. Tabs too tall (dead space around the text) — the strip row
     stretched its children (valign fill) and the embedded reader
     toolbar's theme-default button min-heights made the row 48 px.
     Fix: valign Center + `min-height: 0` on the tab boxes and a
     scoped `.tabstrip button/separator` compaction; the row
     measures 36 px (the probe gates strip < 40).
  2. Tabs blended together — the comic tabs were a BARE button
     pair; now every tab is one bordered box (`.tab`: background +
     border + radius; `.tab-active` raises).
  3. The menubar must ALWAYS be visible — the `AutoHideMainMenu`
     auto-hide AND the Alt-alone reveal are REMOVED (the
     `menubar::menubar_visible` C# formula + its tests stay for the
     record; the shell shows the menubar unless MinimalGui). This
     also RESOLVES the deferred T3 observation (the menubar hiding
     in the browser with a book open — no longer ported behavior).
     The `AutoHideMainMenu`/Show-Main-Menu settings remain in the
     schema + Preferences but no longer drive visibility
     (re-homed here from T3's omissions).
  4. The X closed the comic but the TAB stayed — `set_tabs`' retain
     dropped only the Rust handle; a GTK widget stays in its parent
     until removed EXPLICITLY. Fix: retain removes the root from
     the row. The probe now gates the WIDGET count too (slots ==
     widgets) — the first probe missed it because it read only the
     slot vec.
  5. The X sat OUTSIDE the tab box — the comic tab is now one box
     (`.tab`) with the caption click AND the close button inside it
     (the C# TabBar shape).
  6. The Pages workspace collapsed to one toolbar's height — the
     workspace stack had no vexpand (and the Pages scroller
     neither). Fix: the stack + paned + the Pages/QuickOpen
     scrollers expand; the probe gates stack-h == pages-h > 300.
  - PROBE LESSON: the probes never loaded the theme CSS
    (`theme::init` runs in `app::run` only) — every CSS-dependent
    gate measured UNSTYLED defaults until the probe calls
    `cr_ui::theme::init()` itself (the height gates "worked" against
    the wrong widget tree). `tabstrip_probe` now loads it.
  - PROBE LESSON: an allocation read in the SAME timeout tick as
    the widget switch reads 0/STALE — measure one frame later (the
    F step split into F + F2 at +300 ms).
- T9 COMPLETE — USER-TESTED, ALL PASS (2026-09-05; one fix round
  of six items: compact tab boxes, the close bug, the always-on
  menubar, the X inside the box, the pages fill, the row height).
  The user verified: the strip look (boxes, height), the
  always-visible menubar, the tab close, and the full-window
  Pages view. **Next: T8 (the status bar).**

### Addition — Dark/Light mode toggle (2026-09-05, no C# item)

The C# theme is boot-only: `ExtendedSettings.UseDarkMode` (the
`-dark` switch) forces `Themes.Dark` over the stored `Theme` value
(the `Theme` getter), both read from the ini chain; the C# exposes
NO menu command and never writes the ini back. The port adds a
recorded ADDITION so the mode is switchable at runtime:

- `win.dark-mode` (Browse ▸ _Dark Mode, iconless, no accelerator —
  the C# table has none to port). A stateful bool action; the
  handler flips `ExtendedSettings::global_mut().theme`
  (Dark/Default) and clears `use_dark_mode` (the explicit toggle
  must not let a stale `-dark` re-darken the next boot), applies
  `theme::set_dark` (the GTK `prefer-dark` flag re-styles
  instantly), and persists `Theme` + `UseDarkMode=False` into the
  LAST ini-chain file (`library::save_ini_keys` — the C#-shaped
  storage; the C# itself never writes the ini, that write is the
  second half of the deviation).
- `ExtendedSettings::effective_theme` ports the C# `Theme` getter;
  the global moved from `OnceLock` to `RwLock` (the C# static is
  mutable). `Themes::Default` renders LIGHT — C# parity (WinForms
  Default does not follow the Windows dark setting). The app
  therefore starts LIGHT on a fresh/old config; `-dark` and
  `-theme Dark` keep working.
- Reader-window CSS stays dark in both themes (the C# reader paints
  its own background regardless of theme); `.placeholder-label`
  moved to a mid-gray that reads on both. Gate: `menubar_probe`
  (the row click flips the global AND the GTK flag),
  `commands_probe` 70/70, 317 tests.
