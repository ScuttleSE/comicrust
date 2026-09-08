# Phase 8 Kickoff — Polish/Ship + the user-reported list

Goal: ship quality — packaging, docs, migration tooling — plus the
user-reported UX/perf items collected on 2026-09-06 after the Phase 7
close-out.

Target crates: `cr-ui` (T1-T6, T11), `cr-core`/`cr-engine` (T3, T4,
T11), new packaging files (T8).

## User-reported items (2026-09-06, verbatim scope)

1. Default view when opening up the app should be Library view.
2. Slow operation when adding a large CBL file — several minutes for
   ~2500 titles (example in
   `tests/testfiles/[Spider-Man] 00 - Complete 616 Chronology.cbl`).
3. Slow operation when deleting empty books — ~200 tagged empty
   books, delete, the app hung for close to a minute.
4. Explore moving from the XML file for the database to a
   sqlite/postgres backend instead. (→ Phase 9,
   `docs/phase-9-kickoff.md`)
5. Implement the Folders tab (filesystem view) next to the Library
   tab.
6. Manually resizing columns in the Details view.
7. Many menu items carry a `&` in the name ("&Recent Books").
8. Right-click menus have a small "arrow" originating from the
   right-click box — get rid of it.
9. Right-clicking an item in the library tree: the right-click menu
   always originates from the top of the tree.

## C# spec pointers (researched so far)

- Default view: `MainForm.cs:3140` — at startup, with no open books
  and `!Settings.ShowQuickOpen`, `BrowserVisible = true; mainView
  .ShowLast()` — the browser shows the last view tab. The C#
  `ShowQuickOpen` default true shows QuickOpen instead. The user
  wants the Library view as the default: boot to the browser
  workspace with the Library list selected; QuickOpen stays
  reachable (the C# keeps it as a mainView tab).
- Folders tab: `MainForm.cs:744` gates it on
  `ExtendedSettings.DisableFoldersView` (the port's T9 record hid it
  with that parity). The engine is
  `ComicRack/Views/FolderComicListProvider.cs` (a filesystem-tree
  `IComicBookListProvider` with its own `RemoveBooks` — note its
  `ShellFile.DeleteFile` on a FOLDER path, see the Phase 7 delete
  audit). The view shows the disk directory tree; selecting a
  folder lists its comics.
- Menu `&`: the C# Designer texts carry WinForms mnemonics
  (`miOpenRecent.Text = "&Recent Books"`,
  `fileMenu.Text = "&File"`). The port's `menubar.rs` table carries
  them verbatim and renders the `&` literally. The custom menubar
  has no Alt-mnemonic machinery — strip the `&` at render (the C#
  VISIBLE text has no `&`; it underlines the next letter).
- Popover arrow: the C# `ContextMenuStrip`/`ToolStripDropDown` have
  no arrow. The port's context menus, dropdowns and the column
  chooser are `gtk4::Popover`s — default `autoarrow` draws the
  pointing arrow. Fix: `set_autoarrow(false)` on the context/dropdown
  popovers (the menubar top-menu popovers too — the C# MenuStrip
  drops have no arrow).
- Tree menu position: the C# `ContextMenuStrip.Show(cursor)` — the
  menu opens AT THE CURSOR. The port's `navigator.rs::open_menu`
  points the popover at the gesture's (x, y) but parents it to the
  navigator Box (toolbar + search + tree) — the coordinates do not
  match the parent, and GTK falls back to an edge position (the
  observed "top of the tree"). Fix: translate the tree-relative
  coords into the parent widget's space (or parent the popover to
  the TreeView).
- Details columns: the C# ItemView Detail columns are drag-resizable
  and the widths persist in the workspace (`ItemViewConfig.Columns`).
  The port's T14 workspace already stores the column list — check
  whether widths ride along; add the drag handle + the persistence.
- Database backend: RE-HOMED to Phase 9
  (`docs/phase-9-kickoff.md`) on 2026-09-07 — it touches
  compatibility invariant 1 (byte-stable ComicDb.xml) and grew into
  a full phase after the exploration + user decisions. ADR-029
  remains REQUIRED before any code.

## Task list

### T1. UI fixes batch (the user items 7, 8, 9)

- `menubar.rs` (+ the toolbar/dropdown row builders): strip the
  leading `&` from the C# text when rendering (keep the table
  verbatim — the table is the Designer record). The accelerator
  column already exists; the `&` carries no mnemonic in the port.
- `Popover::set_autoarrow(false)` on: the navigator context menu,
  the book context menu, the column chooser, `build_dropdown`
  popovers, the menubar top-menu popovers. The C# drops have no
  arrow.
- `navigator.rs::open_menu`: point the menu at the actual cursor
  (parent to the TreeView or translate the coords; verify against a
  right-click on a row near the BOTTOM of a long tree).
- Gate: extend `menubar_probe`/`navpages_probe` (arrow property
  false, the menu's pointing rect at the clicked row), then the user
  test.

IMPLEMENTED 2026-09-07 (both fix sets of the batch — the `&`
mnemonics, the popover arrows, the tree-menu position). The render
path: `menubar::strip_amp` strips every `&` from the label at
render in `row_content` + `dyn_row_content` (the tables stay
verbatim — the Designer record; `_` keeps its GTK underline, the
C# `&` renders plain). Side fix the work surfaced: the submenu
parent registry keyed on `label.replace('_', '')`, so
`set_sub_enabled("Recent Books")`/`("Page Type")` never matched the
`&Recent Books`/`&Page Type` rows — the key now strips both
mnemonic characters and the parent-enable works (C# parity). The
arrows: `set_has_arrow(false)` (the definitive kill — `autoarrow`
is ignored once `has_arrow` is false; the menubar/dropdown
popovers already used it) on the navigator context menu, the book
context menu, the column chooser, and the book editor's page menu;
`build_dropdown`/menubar popovers were already arrowless. The
position: `navigator.rs::open_menu` parents the popover to the
TREEVIEW (the click coords are view-relative; the old Box parent
misread them and GTK fell back to the top edge) and keeps the
gesture's (x, y) — the `path_at_pos` cell coords are gone. The
no-row-under-cursor gate stays (the prior behavior). Probes:
`menubar_probe` gates labels-with-`&` = 0 over static + submenu +
filled rows (re-checked after the dynamic fills) and all six top
popovers arrowless; `browserbar_probe` gates the Views drop
(amps=0, arrow=false); `navpages_probe` step F fires the real
context-menu path and gates arrow=false + pointing rect exactly at
the click point (40, 28 = y 20 + the 8 px offset). All green.
USER-TESTED, ALL PASS (2026-09-07, "works now").

### T2. Default view = Library (item 1)

- Boot to the browser workspace with the Library list selected
  (the C# `ShowQuickOpen=false` startup shape, MainForm.cs:3140).
  QuickOpen stays reachable (the C# mainView tab — the port's
  Books tab / the `win.quick-open` path if it exists; research the
  port's current QuickOpen entry points and keep one).
- The last-opened-list restore (`LastLibraryItem`) keeps working —
  the C# `ShowLast()` parity: the boot selects the last view.
- Gate: a probe (boot → the browser workspace visible → the Library
  list is the current list), then the user test.

IMPLEMENTED 2026-09-07. The boot (`shell.rs` initial fill) calls
`show_browser()` instead of `show_quick_open()` — the C#
MainForm.cs:3140 shape; the app opens on the Library workspace with
the Library list selected (the navigator's boot fill selects the
first row = Library root; no `LastLibraryItem` persistence existed
in the port, none added — the boot is Library-root per the user's
"Library view" request). QuickOpen stays reachable through the
C# `UpdateQuickList` path: the LAST-tab-close handler now calls
`show_quick_open()` (covers when `ShowQuickOpen` and the database
has books, the browser otherwise) instead of forcing the Library
workspace — this is the C# behavior after closing the last book and
it replaces the port's only other QuickOpen entry point. Recorded
behavior changes a user test will see: (1) boot lands on the
Library view, not the QuickOpen covers; (2) closing the last comic
tab now shows the QuickOpen covers (was: the Library view). The
`+` empty slot still shows the blank reader (the recorded
deviation). Gate: `bootview_probe` (boot → browser + Library
selected; open → reader; last close → quickopen; Browse ▸ Browser →
browser). The `tabstrip_probe` expectations A (boot page) and I
(close-all page) moved to the new shape. All probes green.
USER-TESTED, ALL PASS (2026-09-07, "works now").

### T3. Perf: the large-CBL import (item 2)

- Primary suspect (code-level): `cr-engine/src/reading_list.rs`
  computes `book_view::proposed(book)` — a full ComicNameInfo
  REGEX parse — for every book × every list item in the series
  ladder (`series_match`). 2500 items × 2500 books ≈ 6M regex
  storms. Fix: precompute the shadow table ONCE per import
  (series/number/year/volume/format + the file-name map), then the
  per-item matching is hash/scalar work.
- Measure first: a timing gate with the real chronology `.cbl`
  (2500 titles) against a synthetic 2500-book library — record the
  before, target "seconds" after. The automatic-progress-dialog
  deviation from T2 can be revisited ONLY if a real wait remains.
- Gate: the timing test + `importlist_probe`/`listorder_probe`
  stay green.

IMPLEMENTED 2026-09-07 (no fix round). `reading_list.rs` builds a
`LibraryIndex` per `create_from_reading_list` call: a Guid map and a
file-name map (first-book-wins = the `find` parity; OrdinalIgnoreCase
via `to_ascii_lowercase`) plus a `BookShadow` row per book — the five
shadow values with ONE lazy `proposed()` parse per book (only when
`EnableProposed` and a field actually falls through) and the two
transformed series forms (`series_iv` = rxVolume strip + trim,
`series_sd` = the IV→rxSpecial chain — byte-exact with the C#
`SeriesEquals` option ladder; the item side computes its two forms
once per item). The ladder, the year/volume/format narrowings and the
placeholder flow are unchanged; `series_equals` stays public for
parity/tests. Timing gate `cr-engine/tests/reading_list_perf.rs`
(synthetic 2000 metadata + 500 proposed books × 1600 + 400 + 500
items; the real chronology `.cbl` × the real-world 255-book library
is the second test, skipped when the git-ignored `.cbl` is absent).
MEASURED (release): the user scenario (2886-item chronology × 255
books) was **413.5 s**, now **0.069 s** (~6000×); the 2500×2500
synthetic storm (> 15 min, unfinished pre-fix) is 0.47 s; debug
7.3 s / 0.9 s — the 30 s budget holds in both profiles. Placeholders
count asserted unchanged (matching semantics guarded). The
automatic-progress-dialog stays out (no real wait remains). Gates:
fmt/clippy/`cargo test --workspace` green, `importlist_probe` +
`listorder_probe` green. User test pending.

### T4. Perf: the fileless-book delete hang (item 3)

- Symptom: ~200 tagged empty books, delete → the app hangs ~1 min.
- Profile the remove flow first (shell.rs "remove" →
  `library::remove_book` per id → one `refresh_view_from_list`).
  Suspects: per-remove DB churn inside `remove_book`, the write-back
  timers, the thumbnail/pool queues, the status/selection re-syncs,
  the `books_by_ids`/selection snapshot costs, or the view relayout
  per notify.
- Fix + a timing gate (delete 200 fileless books headlessly —
  seconds). The user test decides.

IMPLEMENTED 2026-09-08 (measured, no fix round for the remove flow
itself — the storm was the rebuild). The profile
(`deleteperf_probe` — the REAL flow: the chronology `.cbl` imported
through the missing-books dialog, ~250 fileless placeholders
selected, the context menu → the confirm dialog): the per-id loop +
evaluate ≈ 2 ms, the DB save 4 ms, `set_books` ≈ 875 ms — the WHOLE
hang sat in the view REBUILD. CAUSE: the eager `book_view::PropTable`
parsed ~one ComicNameInfo per book per rebuild (~0.33 ms each —
`needs_prop` is true for nearly every book: `enable_proposed`
defaults TRUE and Title is usually empty) even when no grouper/sort
read a single parse. The pre-T10-slice code (the user's report) paid
the same parse per COMPARISON in the sort + the error-surface decode
storm on top — the minute. FIX: the PropTable went LAZY (the C#
`ComicBook.Proposed` semantics — parse on first read): build()
only allocates, get() parses slot i on first read, dead books
resolve to the shared empty parse, and the unsorted/ungrouped view
(no getter ever reads) parses NOTHING; the empty-sort-chain guard
moved into the sort closure (the resolve arguments must not parse
before compare early-outs). The C# per-book SESSION cache stays
plan B in `docs/backlog.md`. MEASURED (release, the user scenario):
the remove+refresh closure **886 ms → 20 ms**. Gates:
`view_state::tests::rebuild_reading_list_scale_stays_fast` (2886
books, 1 s budget — a parse-per-book regression blows it in debug)
+ `deleteperf_probe` (the real import + real remove path; falls
back to the synthetic 200-fileless scenario without the fixtures).
CR_TRACE stage lines live in refresh/set_books. 375 tests; probes
green. USER-TESTED, ALL PASS (2026-09-08, "works fine").

### T5. Details-view column resize (item 6)

- The C# Detail columns drag-resize; widths persist in
  `ItemViewConfig.Columns` (the T14 workspace). Check what the
  port's `workspace.rs`/`columns.rs` store (the T14 record carries
  `Columns` — widths may be missing).
- Implement: a drag handle in the Detail header (hit zone + a grab
  cursor), min widths, live reflow; persist the widths in the
  workspace (the T14 write path).
- Gate: `workspace_probe` + a probe (drag → the column width + the
  persisted round-trip), then the user test.

IMPLEMENTED 2026-09-08 (no fix round). The C# separator behavior
over the port's Detail header: `ColumnHeaderSeparatorHitTest` (the
±2 px zone at each visible column's RIGHT edge, scanned last-to-
first — pure in layout.rs, unit-tested), left-drag resize with the
C# clamp math (`width = start + dx`, 0..10000 — the kickoff's "min
widths" IS the C# clamp; a 0-width column shows nothing — the
captions clip per column), live reflow per move, the double-click
auto-size (`GetAutoHeaderSize` — the widest displayed cell text +
8 padding measured on a scratch cairo context; image-only columns
keep their width — recorded deviation), the col-resize cursor on
the hit zone (`Cursors.VSplit`), the full-height ResizeMarker line
while dragging, per-column clipped header captions with a 1 px
framed edge (the C# `DrawStyledRectangle` separator). The widths
ALREADY rode the T14 workspace round-trip (`detail_columns_state`
kept them) — the probe now gates the resize through it. NOT ported:
header-click sort + drag-reorder (the port's header has no
sort-click parity yet — recorded). Gate: the layout unit tests +
`detailresize_probe` (the real drag path: Series 200→280 through
the move math, the zero clamp, the auto-size 92, and the collect →
second-shell width restore). 375 tests; fmt/clippy green; the
workspace/browserbar/listorder/bootview/statusbar probes green.
USER-TESTED, ALL PASS (2026-09-08, "works fine").

### T6. Folders tab (item 5)

- Port `FolderComicListProvider` (the filesystem tree + the books of
  the selected folder) and mount the Folders tab next to Library in
  the tab strip (the T9 strip; the C# mainView tabs).
- The remove flow in the Folders view recycles the file — the Phase
  7 delete audit applies (the C# trashes FOLDER paths there; the
  port's is_file guard skips folder comics — keep the guard, record
  the deviation).
- Research first: the C# `FolderComicListProvider.cs` +
  `MainForm.cs:744` (the DisableFoldersView gate) + the favorites/
  path dropdown shape. This is the largest task — estimate it like a
  Phase 4-sized view task.
- Gate: a probe (the folder tree renders, selecting a folder lists
  its comics), then the user test.

IMPLEMENTED 2026-09-08. The provider
(`folder_tree::folder_book_list`): the `FileUtility.GetFiles` walk
(files first, then recursion — plain, no `comicrackscanner.ini`
honoring), the provider extension registry filter
(`Providers.Readers.GetFileExtensions`), the `AddToTemporary`
session books (`scanner::create_book` made public), the stored
metadata read for the first 100 files only
(`RefreshInfoOptions.DontReadInformation` beyond), the provider
page count always wins. The panel (`FolderTree`): one "/" root
("File System"), LAZY fill with the dummy-child pattern (a childless
row shows no expander — every fresh row carries one dummy the fill
swaps out), names sorted with `extended_compare_ignore_case`,
`drill_to` through the navigator-proven `expand_to_path` chain
(per-row `expand_row` fails on fresh rows — measured, then fixed),
refresh re-roots + re-drills. The toolbar: the favorites dropdown
(rebuilt on open — the dynamic fill lesson), Add To Favorites, the
Include Sub Folders toggle (the Settings
`ExplorerIncludeSubFolders` write + the rescan), Refresh, Add
Folder To Library (the scan flow + the view refresh). Settings:
`FavoriteFolders` ported (the `<FavoriteFolders><string>…` shape,
reader/writer arms + the round-trip fixture). The shell: the
"folders" stack page with its OWN ItemView (the C#
`AddExplorerView(null, filesBrowser, tsbFolders)` shape), the paned
tree left/grid right, `TabId::Folders` + the tab (the FileBrowser
GIF is not bundled → text-only; `DisableFoldersView` hides the tab
— the C# removes it), the tab click rules (the CaptionClick toggle
+ `last_browser=2` in the ShowLast chain), the selection → the
synced scan, the folder context menu (Open / Reveal / Move to
Recycle Bin — the C# `RemoveBooks` ask with the
"Additionally remove… from the Library" option =
`RemoveFilesfromDatabase` + the failed-delete message; the is-file
guard from the Phase 7 audit), the status panels + the thumb slider
route to the ACTIVE browser (the C# `FindActiveService`), and
`LastExplorerFolder` captured at close (the boot drills to it).
FIX ROUNDS during the gate: (1) the drill expanded per-row
(`expand_row` false on fresh rows) → `expand_to_path`; (2) the
insert_root pre-marked "/" filled (no children ever) → the lazy
dummy; (3) the boot `set_include_sub` RefCell abort — the
edition-2021 temporaries lesson (the settings borrow held through
the toggled handler's borrow_mut); (4) INCIDENT: a probe run
(foldersview) wrote Config.xml into the REAL `~/.config/comicrust`
(the probe guarded only XDG_DATA_HOME; `add_favorite` calls
`save_settings` directly) — `ExplorerIncludeSubFolders=true`, the
/tmp favorite, `LastExplorerFolder` repaired to defaults; the
user's pre-probe Config.xml (workspace/settings) is NOT recoverable
— the probe and deleteperf/detailresize now REFUSE without BOTH
XDG vars isolated. USER TEST (2026-09-08) FIX ROUND: (a) the grid
sat BELOW the paned (a horizontal split) — it is the paned's END
child now (the Library shape: tree left, grid right; the probe
gates the grid inside a HORIZONTAL paned); (b) the folder scan
froze the UI — the scan runs on a WORKER THREAD now (`scan_folder_async`,
the ADR-019 pattern; the C# wraps the same work in
`AutomaticProgressDialog`, the port keeps the UI free instead) with
a generation guard (a stale scan of an older folder drops on
arrival); the selection, the include-sub toggle, and the
remove-rescan share the helper. The per-file scan COST is C# parity
(the fast page-count open + the first-100 metadata read) — the
wall time to fill the grid is unchanged, the freeze is gone.
SECOND USER REPORT (2026-09-08): a comic opened from the Folders
view, once closed, landed on QuickOpen — the C# `RebuildBookTabs`
tail (MainForm.cs:3140) runs `ShowLast()` on the last close: the
LAST browser tab returns (Library/Folders/Pages). The port's
last-tab-close handler calls `select_last_browser()` now, and
QuickOpen moved to its C# empty-reader-overlay home: the `+` empty
slot, an empty-slot tab click, and a close that lands on an empty
current slot show the QuickOpen covers (when `ShowQuickOpen` and
the DB has books — the blank reader otherwise; the recorded "the +
shows a blank reader" deviation resolves); the tab-change hook
follows the current slot while the reader area shows (the C#
`OpenBooks_CurrentSlotChanged` rebind). Probes: bootview C gates
the browser on the last close + a new `+` → QuickOpen step;
tabstrip G/H/I moved to the new shape. The T6 + T4 + T5 user
tests: ALL PASS (2026-09-08, "works fine"). Deviations (recorded): no per-view browser
toolbar on the folders page (the C# ComicBrowserControl toolStrip
— Views/Group/Arrange/search stay library-bound; the folders view
is double-click/context-menu driven; the menubar book commands stay
library-bound), no RemoveFavorite / Open Window / Open Tab, no
FileView workspace persistence (the T14 deferral stands), the scan
is synchronous (no progress dialog), the tree skips dot-dirs and
shows names only (the shell icon list is not portable), the View
menu's Folders item not ported (the tab is the entry point). Gate:
`foldersview_probe` (A the tab+strip selection, B the drill → 2
comics with the stored-series caption, C the include-sub rescan →
3, D the favorite persists, E back to Library). 375 tests;
fmt/clippy green; tabstrip/bootview/statusbar/browserbar/listorder/
menubar/commands/navpages/foldersview/detailresize/deleteperf
probes green. USER-TESTED, ALL PASS (2026-09-08, "works fine" —
two fix rounds: the side-by-side split + the worker-thread scan;
the close-a-comic report became the ShowLast change below).

### T7. Database backend — MOVED TO PHASE 9 (2026-09-07)

- The exploration session of 2026-09-07 scoped the work (SQLite
  canonical after migration; Postgres rejected; XML becomes the
  import/export codec; fresh installs keep the XML default). The
  user decisions, the coupling audit, the work breakdown, and the
  task list now live in `docs/phase-9-kickoff.md`.
- Nothing changed for Phase 8: no code, no ADR yet. Phase 9 T1
  (the spike) starts after Phase 8 closes and produces ADR-029.

### T8. Packaging — DEFERRED (2026-09-08, user decision)

- The Flatpak/.deb/AUR + release-artifact work moved to
  `docs/backlog.md` ("From Phase 8") — the existing tarball release
  tracks stand; add nothing else for now.
- The HEIF/AVIF decode decision that rode this task is made: SKIP
  (no libheif dependency; the gap stays recorded — the backlog
  entry updated).

### T8 original scope (for whoever picks the backlog item up)

- Flatpak (the manifest + the metainfo + the desktop entry + the
  icons), .deb, AUR (PKGBUILD). The release workflows build/attach
  the artifacts (ADR-021's tracks).
- The HEIF/AVIF decode decision lands here (system libheif or skip —
  the Phase 1 gap).
- Gate: the artifacts install and launch on this machine (and the
  Flatpak on a fresh sandbox), `cargo test` unaffected.

### T9. Docs + migration tooling

- README: install paths, first run, migrating from ComicRack CE
  (point at the `%APPDATA%\cYo\ComicRack CE` folder → copy
  ComicDb.xml; the port reads it as-is), the ADR-027 scripting note
  (the used-script set ports natively), the known gaps.
- Migration helper: a first-run dialog or `cr-cli migrate` that
  takes a ComicRack CE profile dir and copies/verifies ComicDb.xml
  (+ the settings mapping: the ini keys the port consumes).
- Gate: the migration tool runs against the real-world fixture.

### T10. General perf passes

- Startup time, the large-library scan, the list evaluation on
  10k+ synthetic books. T3/T4 cover the two measured user pains;
  this task sweeps the rest only with measurements (no speculative
  tuning).

FIRST SLICE IMPLEMENTED 2026-09-07 (the view-side proposed-parse
storms — the same pattern the T3 audit found in the sort, group,
duplicate, and matcher-context paths; no fix round). The shared
primitive: `book_view::needs_prop` (the parse is dead unless
`EnableProposed` and a shadow field falls through) +
`book_view::prop_table` (one parse per book per operation, `None` =
dead) + `book_view::empty_prop` (the never-read dead value). Sites:

1. **Sort** (`sort.rs` + `group.rs::compare_by_column` +
   `view_state.rs::SortChain::compare`): every comparer takes the
   precomputed props; `rebuild` builds the table once per rebuild
   and sorts bucket indexes through it. Was: a full ComicNameInfo
   regex parse per side per comparison, on every view rebuild.
2. **Groupers** (`group.rs`): `Grouper = fn(&ComicBook,
   &ComicNameInfo) -> GroupInfo`; the table shape is unchanged; the
   five prop-consuming groupers read the passed parse. Rider: the
   `bucket_of` linear scan became a `(sort_key, caption)` HashMap.
3. **Duplicate matcher** (`eval.rs::match_duplicates`): the five
   duplicate-comparer values precomputed per book before the O(N²)
   pair loop (which keeps its exact C# shape and short-circuit
   chain, including the ternary quirk); `compress_series` extracted
   from the old per-pair `compressed_name_eq`.
4. **Smart-list SortedBySeries** (`smart_list.rs`): sorts through
   `ctx.prop` (the context cache) instead of fresh parses.
5. **MatchContext** (`eval.rs`) — found during the work, same storm
   family: the props map is now LAZY (parse on first use per book
   via RefCell; books with full metadata never parse). The series
   statistics build reuses the same cache, so a filter rebuild over
   a metadata-complete library parses nothing.

Timing gate `cr-engine/tests/view_perf.rs` (sort 5000 by Series /
group pass 5000 / duplicate matcher 1000; budgets 15/15/30 s).
MEASURED (release): sort 5000 books **25.3 s → 4.4 ms**; group pass
5000 **0.84 s → 78 µs**; duplicates 1000 books **~268 s → 5.8 ms**
(the pre-fix number at the user's 255-book scale ≈ 17 s per Show
Duplicates toggle). Debug: 27 ms / 0.44 ms / 125 ms — all budgets
hold. Gates: 373 tests green (sort/group/matcher semantics
untouched), fmt + clippy clean, `listorder_probe` +
`browserbar_probe` + `commands_probe` green. User test pending
(sort/group clicks + Show Duplicates feel instant). Deferred to the
backlog: plan B (the C#-parity per-book session cache) with the
full rationale + invalidation surface (`docs/backlog.md`,
"C#-parity per-book proposed cache").

SECOND SLICE — USER-TESTED, ALL PASS 2026-09-07 ("works now"): the
ItemView scroll storm behind the "the 2875-book reading list is
unusable to scroll" report. Cause (measured): the draw func built
its culling window as `max(viewport_page_size, draw_size)`, but the
draw size IS the canvas's full virtual allocation
(`set_content_height`), so `visible_items` culled only items ABOVE
the scroll position and every frame drew every item BELOW it
(probe evidence at scroll y=4000: 2589 items drawn per frame,
960 ms/frame; ~35 actually visible — GTK clipped the rest), and the
first frame queued ~2875 thumb loads at once. Fix: cull against the
adjustment page size (the true viewport; draw-size fallback only
pre-allocation) in `item_view.rs` set_draw_func. Side fixes:
`config.view_height` now gets the real viewport height, so
PageUp/PageDown step ONE page (was the whole list —
`page_step_display` divides by it); `error_surface()` is a
thread-local cache (it decoded a PNG per failed-thumb item per
frame). Gate: `cr-ui/examples/scrollperf_probe.rs` (2875 synthetic
books; `CR_TRACE=1` prints per-frame win/items/pending/ms lines).
Measured (release): 2589 → 78 items/frame, 960 → 1.5 ms steady
frames. Residual (accepted): a ~110 ms hitch on the first paint of
a fresh viewport = the one-time per-book caption compute
(`caption_value` resolves up to 9 placeholders, each can trigger an
uncached `book_view::proposed()` filename parse; cached per book in
the `captions` map) — the C# pays the same one-time class; the user
test passed without a hitch report, so plan B stays unpicked.

### T11. Windows-path migration (first run from a ComicRack CE database)

Added 2026-09-07 (user request). A migrated ComicRack CE
`ComicDb.xml` carries Windows paths (`C:\…`, `\\server\…`) in
`Book@File`, `WatchFolder@Folder`, and `BlackList`/`File`. Today the
user re-adds the folders by hand and a scan re-homes the files.
This task replaces that manual step with a migration dialog.
T9's profile-copy helper (`cr-cli migrate`) stays separate — T11 is
the path rewrite inside the app.

- Detection: a path is Windows-style when it has a drive-letter root
  or is UNC. At boot (first launch, after the attention dialog,
  BEFORE the session-reopen pipeline) the app checks the three path
  families. Any hit → the dialog; no hit → nothing shows. No skip
  flag: the dialog re-prompts each boot while Windows paths remain
  (user decision).
- Dialog (`cr-ui/src/dialogs/path_migration.rs`): one row per
  COLLAPSED common-prefix root — path components merge below the
  drive root (`C:\Comics\Batman` + `C:\Comics\Daredevil` collapse to
  `C:\Comics`; `C:\a` + `C:\b` never collapse to `C:\`). Each row:
  the Windows root, book/watch/blacklist counts, a
  browse-to-Linux-folder chooser, and a live "N of M found" preview.
- OK applies (`cr-engine/src/path_migration.rs`, scanner-parity):
  case-insensitive prefix strip, `\` → `/`, join under the chosen
  target. File exists → set `file_path` +
  `refresh_file_info` (size/times/missing flag — the
  scanner.rs:57-61 parity). File NOT found → CLEAR `file_path`:
  the book becomes a fileless book, metadata kept (user decision;
  the Phase 6 FilelessMarker/open-gate/delete-guard machinery
  already covers it). The dialog states the not-found count before
  applying. Watch folders + blacklist: rewrite when the target
  exists, else leave unchanged + report (removable in Preferences).
  Direct DB mutation like the scanner move-recovery — NO
  ComicInfo write-back queue (a path fix must not write the files),
  DB marked dirty, watcher rebuilt, ItemView + navigator refresh,
  saved by the normal dirty-save path.
- Manual re-run: File ▸ "Migrate Windows Paths…"
  (`win.migrate-paths`), enabled only when detection finds Windows
  paths (user decision).
- No model/XML-schema change — only string values change, so
  ComicDb.xml byte-stability and the golden tests are untouched.
- Gate: unit tests (collapse, case/UNC/separator mixes, the fileless
  fallback), `pathmigration_probe` (isolated XDG + a Windows-path
  fixture DB + a mirrored Linux tree: the popup appears, the mapping
  rewrites, not-found → fileless, DB dirty, the menu re-run), then
  the user test.

IMPLEMENTED 2026-09-08. The engine
(`cr-engine/src/path_migration.rs`, 11 unit tests): `is_windows_path`
(drive-letter root or UNC), `collect_roots` (the three families →
groups per drive/share root → the longest common component prefix;
the bare drive never collapses — `C:\a` + `C:\b` stay two roots,
drive-root files get a `C:\` row; counting is case-insensitive),
`map_relative` (component-wise case-insensitive prefix, `\`→`/`,
join under the target — the boundary `C:\Comics2` does not match
`C:\Comics`), `preview_books` (the live found/not-found counts),
`apply` (exists → set path + `refresh_file_info` when a file, a dir
target keeps the path with just the missing flag cleared; NOT found
→ `file_path.clear()` — the Phase 6 fileless machinery takes over;
watch folders rewrite on `is_dir`, blacklist on `exists`, else left +
counted; direct DB mutation, no ComicInfo write-back).
`Library::{windows_path_roots, has_windows_paths, apply_path_migration}`
(mutate + dirty + watcher rebuild). The dialog
(`cr-ui/src/dialogs/path_migration.rs`): one row per root (the root +
counts, a target entry + Choose… FileChooserNative, the live "N of M
found; K not found will become fileless books." label), OK applies
the non-empty rows through `library::apply_path_migration`, unmapped
rows stay for the next prompt. The boot hook: `app.rs` first branch
runs `maybe_prompt_windows_path_migration` (pub — the probe shares
it) right after the attention dialog, before the file pipeline; the
apply refreshes via the new `BrowserShell::refresh_after_data_change`
(ItemView re-evaluate + navigator refill + sync). `win.migrate-paths`
(`cmd` in commands.rs, the File-menu item next to Add Folder to
Library, no icon — a port addition): enabled only while
`has_windows_paths()` (the sync checks the database; it flips off
after the apply). Gate: 386 tests + `pathmigration_probe` (A the
collapse + counts, B the boot prompt opens the dialog, C the live
previews "2 of 3 found; 1 not found", D the apply through the real
response path — books re-home with a refreshed size, Gone → fileless,
both watch folders + the blacklist rewrite, dirty=true, E
`has_windows_paths` false → the action disabled). PROBE LESSON (twice
in one day): the mapping STRIPS the Windows root — `C:\Comics\X`
lands at `<target>/X`, so a test mirror puts the files directly in
the target, not in `<target>/Comics`. Probe battery green (the
statusbar_probe failure mid-round was a shared/polluted XDG — fresh
XDG per probe run is the rule). fmt/clippy green. User test pending.

USER TEST (2026-09-08, "It worked, but after clicking OK the whole
app froze until all the comics were loaded"): FIX ROUND 1 — the OK
path called the FULL file-info refresh per found book, and its
page-count branch (scanner.rs: "page count == 0 || date_modified")
opened EVERY archive inline on the UI thread; the stored
Windows-era mtime always differs from the copied file's, so the open
always fired (the scanner pays the same cost on its worker thread —
the apply ran it inline). Fix: `scanner::refresh_file_info_basic`
(the metadata-only slice: size/times/missing) is public and the
apply uses it — the file content is the one the DB describes, only
the path changed, the stored page count rides (the reader fills
unknown counts from the provider index on open), and
`refresh_file_info` keeps the full `GetFastPageCount` semantics for
the scanner. Gate: `cr-engine/tests/path_migration_perf.rs` — the
apply over 120 real zip archives stays at ~1 ms with the stored page
counts asserted intact, and TIMES the old full-refresh path for the
record (5.7 ms for 120 small zips — for CB7/CBR libraries the old
path is a 7z SUBPROCESS per book, i.e. minutes; the light refresh
removes the class entirely). CR_TRACE stage lines added around the
apply + the refresh stages. All gates + the probe re-run green.
USER-TESTED, ALL PASS (2026-09-08, "works just fine" — the re-test
after fix round 1).

## Order

T1 (quick wins) → T2 → T3 → T4 (the user pains first) → T5 → T6 →
T8 → T9 → T11 → T10. The database-backend item is Phase 9 now (see T7).

## Status

- Kickoff written 2026-09-06. T1 is next.
- 2026-09-07: T7 (the database-backend exploration) re-homed to the
  new Phase 9 (`docs/phase-9-kickoff.md`) with the user decisions
  recorded there.
- 2026-09-07: T11 added (the Windows-path migration dialog, user
  request). Scope + the four user decisions (collapsed roots,
  not-found → fileless, re-prompt each boot, the File menu re-run)
  are recorded in the T11 section. Not started.
- 2026-09-07: T3 implemented (the CBL-import perf — 413.5 s →
  0.069 s on the user scenario; record in the T3 section). User
  test pending. T1/T2/T4+ not started.
- 2026-09-07: T10 first slice implemented (the view-side
  proposed-parse storms: sort 25.3 s → 4.4 ms, duplicates ~268 s →
  5.8 ms, group 0.84 s → 78 µs at gate scale; record in the T10
  section; plan B deferred to the backlog). User test pending.
- 2026-09-07: T10 second slice implemented + USER-TESTED ("works
  now"): the ItemView scroll storm — the draw culled against the
  full virtual canvas, so every frame drew every item below the
  scroll (2589 items / 960 ms at y=4000 on the 2875-book list);
  the draw now culls against the viewport (78 items / 1.5 ms
  steady; gate `scrollperf_probe`, record in the T10 section).
  Side fixes: PageUp/PageDown step one page (was the whole list);
  the failed-cover error surface is decoded once, not per item per
  frame. The residual one-time caption parse per fresh book was
  accepted (the C# pays the same class; plan B stays in the
  backlog).
- 2026-09-07: T3 USER-TESTED ("the cbl-lists imported reasonably
  fast now"). T10 slice 1 user test still pending.
- 2026-09-07: T1 + T2 IMPLEMENTED (records in their task sections;
  probes extended: menubar/browserbar/navpages gates + the new
  bootview_probe + the tabstrip A/I expectations moved to the T2
  shape). T1 + T2 USER-TESTED, ALL PASS ("works now").
- 2026-09-08: T4 + T5 + T6 IMPLEMENTED + USER-TESTED, ALL PASS
  ("works fine"; records in their task sections; new gates:
  `rebuild_reading_list_scale_stays_fast` + `deleteperf_probe` +
  `detailresize_probe` + `foldersview_probe`; 375 tests). T4
  measured 886 ms → 20 ms on the user's delete scenario. T6 fix
  rounds from the tests: the side-by-side split, the worker-thread
  scan, and the last-tab-close → `ShowLast()` change (the T2
  last-close → QuickOpen shape is REPLACED — QuickOpen lives at
  the `+` empty slot now; bootview/tabstrip probe expectations
  moved).
- 2026-09-08: T10 slice 1 USER-TESTED, ALL PASS (sort/group column
  clicks + Show Duplicates feel instant). Remaining order
  (user-decided): T11 (the app work) → the T10 remainder (measured
  only) → T8 (packaging) → T9 (docs).
- 2026-09-08: T11 IMPLEMENTED (the engine + dialog + boot hook +
  the File-menu re-run; record in the T11 section). User test
  pending.
- 2026-09-08: T11 USER-TESTED, ALL PASS ("works just fine"; fix
  round 1 = the metadata-only file-info refresh, record in the T11
  section). T8 DEFERRED to the backlog + HEIF/AVIF skipped (user
  decisions). Remaining order: the T10 remainder (the three
  measurement gates; fix only measured offenders) → T9 (docs +
  `cr-cli migrate`) → Phase 8 closes.
