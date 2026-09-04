# Phase 4 Kickoff — The Browser (library view)

Goal: comicrust loads a real library, shows the ComicRack browser —
list navigator, book list with thumbnail/tile/detail modes, sorting,
grouping, columns, search — and hands books to the Phase 3 reader.
Exit gate: **a user browses their migrated library (the 255-book
real-world ComicDb.xml) in daily-driver comfort: lists evaluate,
books display with covers, sort/group/column changes apply, search
filters, double-click opens the reader, and reading state round-trips
through a byte-stable ComicDb.xml save.**

Read first: `AGENTS.md` (rules + status), `docs/phase-3-kickoff.md`
(the reader, and the user-test protocol — it stays mandatory),
`docs/decisions.md` (ADR-008 cairo, ADR-017 reader architecture,
ADR-018 GTK surface, ADR-019 pool-queue page loads).

## Why this phase is the long pole

The C# browser is the single largest custom build in the codebase:

| C# file | LOC | Role |
|---|---|---|
| `cYo.Common.Windows/Forms/ItemView.cs` | 4,770 | The list control: modes, layout, groups, columns, selection, drag |
| `ComicRack/Views/ComicBrowserControl.cs` | 3,536 | The comic browser: search, view switcher, wiring |
| `ComicRack/Controls/CoverViewItem.cs` | 1,648 | The comic item: cover, badges, text lines, comparers |
| `ComicRack/Views/ComicListLibraryBrowser.cs` | 1,647 | The library browser pane (list navigator + browser host) |
| `ComicRack/Controls/PagesView.cs` | 833 | The pages panel (page thumbnails of one book) |
| `ComicRack/NavigatorManager.cs` | 461 | The list navigator model (ComicLists tree ↔ views) |
| `ComicRack/Views/QuickOpenView.cs` | 210 | QuickOpen (recent lists as a cover grid) |
| `ComicRack/Views/ComicPagesView.cs` | 241 | The pages view host |

Supporting specs: `ItemViewColumn.cs`/`ItemViewConfig.cs`/
`ItemViewMode.cs` (Thumbnail/Tile/Detail), `ItemViewGroupsStatus.cs`,
`CoverViewItem*Comparer/Grouper.cs` (the Phase 2 T7 registry tables
feed these), `cYo.Common.Windows/Forms/SearchTextBox.cs` +
`ToolStripSearchTextBox.cs` + `SearchContextMenuBuilder.cs` (search),
`ComicRack.Engine/Controls/SearchBrowserControl.cs`, and the browser
wiring regions of `ComicRack/MainForm.cs` (4,576 — the orchestrator).

## What already exists (do not rebuild)

| Need | Where |
|---|---|
| The ComicLists tree model (raw matchers) | `cr-core/database/list_items.rs`, `display_config.rs` |
| ComicDatabase load/save, `.bak`/`.restore` chain | `cr-core/database/comic_database.rs` |
| Book defaults, reading-state fields, `set_current_page` | `cr-core/model/comic_book.rs` |
| Property registry (C# property name → typed accessors) | `cr-core/registry.rs` — drives columns AND search |
| List evaluation (limits, filtered ids, base lists) | `cr-engine/smart_list.rs` |
| Sort comparers + grouper ladders + `compare_by_column` | `cr-engine/sort.rs`, `group.rs` |
| Thumbnails (512 px q60, `ThumbnailKey`, disk+memory) | `cr-image/thumbnail.rs`, `cr-engine/image_pool.rs` (`add_thumb_to_queue`, fast/slow thumb queues) |
| Error thumbnail (broken covers) | `cr-image/error_assets.rs` |
| Scanner + watch folders (library refresh) | `cr-engine/scanner.rs`, `watch.rs` |
| The reader (tabs, undock, input map) | `cr-ui/reader*` — Phase 4 re-hosts it |
| Queue→UI callback pattern (`PageTx` + pump) | `cr-ui/reader/page_view.rs` — reuse for thumbnail loads |

## The one structural decision (this doc's ADR-020 proposal)

The C# main window hosts panes: the list navigator (left), the
browser (center), the reader (replaces the browser view when a comic
opens, or tabs beside it). Phase 3 shipped a reader-only window with
its own session tabs. Phase 4 makes the browser window the app's main
window and the reader a VIEW inside it (the C# `ComicDisplay` panel),
keeping the undock path (`D`) that already exists. The Phase 3
session tabs become the browser's open-comic tabs, matching the C#
`MainForm` layout. Do not port `TabBar.cs`/`SizableContainer.cs`
control-by-control — build the GTK4 layout that produces the same
user-visible behavior (paned positions, tab captions, undock).

## Task breakdown

Order: T1 (session) and T3 (ItemView core) are the critical path;
T3 is the largest single build — start it early and keep it pure
(geometry unit tests like the reader's).

### T1. The library session (`cr-ui` app wiring + `cr-engine`) — COMPLETE (2026-09-03)

- [x] `ComicDatabase::open_with_fallback` at startup (the
      `.restore` → main → `.bak` chain exists); the in-memory
      library state: books + list tree + display config.
      Done: `cr_engine::library::Library` (ADR-022); the app opens
      it at the ADR-022 default path. Fixed on the way: the
      `NewEmpty` fallback now uses `create_new()` (default list
      tree, C# parity), and a MISSING file is a new silent
      `OpenStatus::FreshEmpty` (only a CORRUPT file shows the
      "problem" message).
- [x] Save on exit through the C# `DatabaseManager` semantics
      (`.bak` rotation, temp-file atomicity — the writer is proven,
      wire the lifecycle). The byte-stable round-trip gate MUST stay
      green with a saved real-world DB. Done: the reader window's
      close-request saves (`DatabaseManager.Dispose` → `Save`
      parity); the 600 s background save (`save_if_dirty`) matches
      `DatabaseBackgroundSaving`. Unmutated real-world re-save is
      byte-identical (acceptance test).
- [x] Scanner integration: the stored watch folders + a manual
      "Add folder" flow → `scanner::scan_database` → new/removed
      books appear. File-info refresh through the ComicBook queues
      (`queue_manager.rs`). Done: the launcher's "Add Folder to
      Library…" → `scan_file_or_folder` (the C# `AddFolderToLibrary`
      is exactly this scan); watch events debounce into rescans of
      the affected roots (`remove_missing: false`). Deviations: the
      scan runs synchronously (the C# uses a low-priority worker;
      ADR-022), and the per-book file-info refresh on open calls the
      sync `scanner::refresh_file_info` directly (the queue
      callback's `&BookRef` cannot mutate — the C# refresh also runs
      inside the scan flow, not on the ComicBook queues).
- [x] Reading-state persistence: the Phase 3 session-only
      write-back (`OpenedTime`/`OpenedCount`/`CurrentPage`/
      `LastPageRead`) now lands in the saved DB. The Phase 2
      ground truth (Never Read = all 255) must flip correctly as
      the user reads. Done: comics found in the library reuse the
      stored book (`ComicBookFactory.Create` parity — refresh, open
      stamps, dirty); page turns mirror into it by path; the save
      persists it. The acceptance test flips Never Read 255 → 254
      and Read 0 → 1. Non-library comics keep session-only state
      (C# `AddToTemporary` parity). Also ported: a same-path open
      focuses the existing tab (`NavigatorManager.Open` slot
      lookup).
- [x] Verify headless first: a `cr-cli`-style integration test
      loads the real-world DB, scans a synthetic folder, saves,
      round-trips byte-stable on the XML. Done:
      `crates/cr-engine/tests/library.rs` (4 tests).

### T2. The list navigator pane (`cr-ui`) — COMPLETE (2026-09-03)

C# spec: `NavigatorManager.cs`, `ComicListNavigator` usage in
`MainForm.cs`, the tree skin in `ComicRack/Controls/LibraryTreeSkin.cs`.

- [x] The ComicLists tree as a GTK4 tree view: Library root,
      smart lists, folders, the default-list icons, nested lists.
      Custom thumbnails on list items are Phase 5 polish.
      Done: `cr-ui/src/browser/navigator.rs` (TreeView + TreeStore,
      kind icons from the GTK theme; expansion + selection kept
      across refills by item id; select reveals ancestors like the
      WinForms `SelectedNode` setter).
- [x] Selection → evaluation: the selected list's books via
      `cr-engine/smart_list::evaluate_smart_list` (the matcher
      binding exists). Evaluation runs on selection change; large
      sets debounce. Done: the full tree evaluator
      `cr-engine/src/lists.rs` (`evaluate_list`: Library = all,
      folder Or = union / And = intersect / Empty, id lists,
      recursive base-list resolution with a cycle guard — the C#
      `OnGetBooks` family); the widget debounces 200 ms (the C#
      `updateTimer`).
- [x] "New smart list" creates a list with a `Match` string (the
      editor UI itself is Phase 5 — a bare list with a hand-written
      query is enough here); folders create/rename/delete.
      Done: the context menu (right-click selects the row under the
      cursor, `tvQueries_MouseDown` parity) → bare entry dialogs;
      insertion after the selection into the selection's container
      (`GetCurrentNodeComicListCollection` parity); Library renames
      but never removes (`RemoveListOrFolder` guard); the query
      parses through the Phase 2 matcher language
      (`parse_group_query` → `Matcher::to_raw`).
- [x] The list's evaluation result feeds T3/T5's book set.
      Done: `library::evaluate_list(id) -> (name, ids, count)` —
      T3 consumes the id set.

### T3. The ItemView core (`cr-ui/src/browser/`) — the long pole — COMPLETE (2026-09-04)

C# spec: `ItemView.cs` (4,770). Port the BEHAVIOR, not the WinForms
machinery. Structure it like the reader: pure geometry + state
modules with unit tests, one GTK4 drawing-area widget on top.

- [x] `view_state.rs` — the item set: book list, current sort
      (`compare_by_column`), grouping (the `group.rs` ladders →
      group ranges), stacking (by the stack column), filtered
      selection. Done: the MRU-3 sort chain (`Descending` =
      `comparer.Reverse()` parity), group buckets ordered by the
      `GroupInfo.Compare` rule (bucket index →
      ExtendedStringComparer IgnoreArticles|IgnoreCase → a
      deterministic tie-break; the C# sort is unstable there),
      collapse by caption across rebuilds, and the full selection
      model (click/ctrl/shift/rubber-band-from-snapshot, focus,
      anchor — the anchor moves only on plain clicks). Stacking
      plumbing deferred with the browser default (no stacker) —
      T5 wires the stack menu.
- [x] `layout.rs` — the pure layout engine: Thumbnail (cover grid,
      per-thumb size), Tile (cover + text lines), Detail (the
      columned report view) modes; group headers (ItemViewLayout
      Top/Left semantics); item rects, hit testing, visible-window
      culling for virtualization (the continuous-mode lesson
      applies: only visible items draw). Done: the greedy flow with
      the `>=` wrap rule and 2 px gaps, full-width group headers,
      collapsed groups drop their items, Detail rows over the
      column strip (x+8 first-column offset), column-aware
      keyboard movement (`GetRelativeItem`), page steps, hit tests.
      Left layout is NOT ported (nothing in ComicRack ever sets
      it — documented deviation).
- [x] `columns.rs` — column set from the C# browser defaults
      (the `MainForm` default columns), widths, visibility,
      order; cell text via the property registry
      (`cr-core/registry.rs`) — the same source the matchers use.
      Column drag-reorder and resize are GTK-overlay polish; ship
      fixed order + configurable widths first. Done: the full
      default column table (13 visible + the hidden rest, ids and
      widths from `ComicBrowserControl`); cell text through the
      engine's `display_text.rs` (`GetPropertyValue(proposed:
      true)` parity — Shadow*/AsText/FormatVolume/FormatYear/date
      forms, registry fallback). Headers draw in Detail; drag-
      reorder and resize stay T5.
- [x] The widget: scrolling (mouse wheel = scroll lines, the
      reader's scroll machinery is the model), selection (click,
      ctrl/shift-click, rubber band — the C# `ItemView` selection
      semantics), keyboard navigation (arrows, Home/End, type-ahead
      find), focus rectangle. NO drag-drop reorder yet (T5). Done:
      one DrawingArea in a ScrolledWindow sized to the virtual
      size; native wheel scrolling is a DOCUMENTED DEVIATION (the
      C# steps 16 px per line — the ScrolledWindow wheel scrolls
      comfortably; revisit only with user evidence); selection,
      keyboard, type-ahead (2500 ms buffer), focus visuals, the
      focus grab on click and window activation (the Phase 3
      lesson).
- [x] Thumbnails load through `ImagePool::add_thumb_to_queue`
      (fast/slow thumb queues, ADR-019 pattern: callbacks + the
      pump). Failed covers render the error thumbnail
      (`cr-image::error_assets`). Done — plus two defects the user
      test caught: the pump broke after its first idle poll (now
      lives while `pending_thumbs > 0`, started by the draw path),
      and the completion blob is the C# `ThumbnailImage`
      serialization (20-byte header + JPEG) — parse it before
      decoding.
- [x] Unit tests: layout math (rects, groups, culling), sort/group
      composition over synthetic books, selection model. Done: 16
      new tests (232 total).

### T4. The comic item (`cr-ui/src/browser/item.rs`) — IMPLEMENTED (2026-09-04), user test pending

C# spec: `CoverViewItem.cs` + the `CoverViewItem*Comparer/Grouper`
family.

- [x] Cover drawing (fit-to-box scaling incl. the UP-scaling
      parity lesson from Phase 1), the overlay badges: read
      markers, page-count/rating text — port the C# `DrawItem`
      visuals in cairo, one badge at a time, user-tested. Done in
      `item.rs`: `draw_cover` (border 4, shadow reserve, the
      right-keeping crop of a landscape source, 1 px frame,
      selection tint), `draw_bookmarks` (the `DrawBookmarkV`
      swallowtail ribbons at the right edge — Orange = CurrentPage,
      Green = LastPageRead, over the PageCount denominator),
      `draw_rating_tags` (the default numeric mode: the personal
      gold / community blue tags at the bottom-right with the
      scaled number), and the file-missing marker (the bundled
      RedCross, bottom-left strip). The caption is the exact
      `Comic.Caption` — `display_text` ports the
      `ExtendedStringFormater` group semantics over
      `DefaultCaptionFormat` (a group emits iff every direct
      placeholder resolved; nested failures do not fail the
      parent), unit-tested. Deferred (assets/settings): the
      dirty/open/last/new-pages state PNGs, the dog-ear page curl,
      and the bow shadow.
- [x] Tile/Detail text lines from the registry (the C# format
      strings; start with the English defaults). Done: Detail used
      the T3 column texts; Tile renders the
      `ComicTextElements.DefaultFileComic` line list
      (`CaptionWithoutTitle` bold, `ShadowTitle` bold,
      `ArtistInfo` (the unique-name "/" join), the wrapped
      Summary, and the two-column Size/Opened/Added/Format/File
      block with the shared tab stop) via
      `item::tile_text_lines` (unit-tested).
- [ ] Custom book thumbnail: stored `ThumbnailKey` data wins over
      the generated cover (the C# `SetCustomThumbnail` path; the
      backup format already carries `Thumbnails/*`). DEFERRED to
      the settings port: the resource locator
      (`ThumbnailKey::with_locator`) parses but the pool has no
      `type://` loader and the CustomThumbnails folder path needs
      the settings port. The model field (`custom_thumbnail_key`)
      round-trips already.

### T5. The browser shell (`cr-ui/src/browser/` + app window)

C# spec: `ComicBrowserControl.cs` (3,536) — port the user-visible
subset; `MainForm` browser regions.

- [ ] The main window layout: navigator pane (T2) + ItemView (T3)
      in a GTK paned container; the reader opens as a view/tab in
      the same window (the Phase 3 shell moves under it — keep
      undock/re-dock and the session tabs working).
- [ ] The search box (`ToolStripSearchTextBox` + 
      `SearchContextMenuBuilder`): text → matcher query over the
      registry properties (`ComicBookMatcher` search mapping),
      filters the current list's book set. The C# search field
      builds `ComicBookMatcher` queries — reuse the Phase 2
      matcher plumbing, not a new filter language.
- [ ] View-mode switcher (Thumbnail/Tile/Detail), thumbnail-size
      control, sort menu (column + direction), group menu (the
      grouper registry), column visibility.
- [ ] The status bar: book count/selection count (the C#
      `ComicBrowserControl` status strip).
- [ ] Double-click / Enter → open the book in the reader (in-tab,
      per the C#); the reading-state write-back loop closes.
- [ ] Context menu (right-click): the common commands only —
      open, reveal in file manager (xdg-open), remove from
      library, properties stub (the editor dialog is Phase 5).
- [ ] Rubber-band drag of books onto folders/desktop = OUT for
      this phase (GTK drag sources are Phase 5/7 polish); the C#
      `DragDropContainer` behavior is recorded here so it is not
      forgotten.

### T6. PagesView + QuickOpen

C# spec: `PagesView.cs` (833), `ComicPagesView.cs` (241),
`QuickOpenView.cs` (210).

- [ ] PagesView: the selected book's pages as a thumbnail grid
      (thumbnail keys per page index), double-click → the reader
      at that page (`open_with_state` already takes a page).
      Bookmarks show when the book has them (Phase 5 adds the
      bookmark editor; display-only here).
- [ ] QuickOpen: when the browser pane is hidden
      (`ShowQuickOpen` setting), the recent/favorite lists show as
      a cover grid (the C# default quick-open lists); click →
      open. Minimal and honest.

## Non-goals for Phase 4

- All remaining dialogs (book editor, bulk edit, preferences,
  smart-list editor, export, devices) — Phase 5.
- Scripting hooks, remote server, sync — Phases 6-7.
- The GL renderer swap (ADR-008) — cairo carries the browser;
  revisit only if scrolling proves too slow with measured evidence.
- Workspace persistence (pane layout, per-list column config saved
  into the DB `<Display>` subtree display-config port) — the
  display_config model exists; wire what the browser needs, defer
  the full workspace system to Phase 7.

## Test strategy

- Pure modules (layout, selection, sort/group composition, session
  state) get unit tests like the reader's geometry suites.
- The real-world DB (`tests/realworld/ComicDb.xml`, read its README)
  stays the evaluation ground truth: list → book id sets must keep
  matching the Phase 2 evidence; add a T1 acceptance test that
  loads → mutates reading state → saves → verifies the diff AND a
  byte-stable re-save of unmutated state.
- Thumbnail queue tests reuse `cr-engine/tests/image_pool.rs`
  patterns (real threads, short timeouts).
- Headless Xvfb probes for rendering (the Phase 3 probe lessons:
  `windowfocus` before keys, screenshots decide rendering).
- The user-test protocol stays mandatory per task: gate, commit,
  push, pause with a written test, iterate on evidence.

## Risks / lessons that apply

- The ItemView port is WinForms machinery at its worst — port the
  observable behavior (what the user sees and does), never the
  control hierarchy. When the C# reaches for owner-draw, draw in
  cairo directly.
- Virtualize early: a 255-book list is small, but the C# design
  point is 50k books. Cull offscreen items in the layout engine
  from day one (the continuous-strip culling code is the model).
- Thumbnail loads arrive late and out of order — the Phase 3
  `PageDone`/pump pattern with source-keyed staleness checks is
  the proven answer; per-item staleness keys on the book id.
- GTK4 list widgets (GtkListView/Gio.ListStore) exist but fight
  the C# drawing model (owner-draw cells, per-item pixel layout).
  The reader precedent — one DrawingArea + pure layout modules —
  is the house style; do not mix GtkListView into ItemView.
- The DB save path is sacred: every task that touches persisted
  state re-runs the golden round-trip (`CR_BLESS` is for deliberate
  model changes only — review the diff before committing).
- Update the **Current status** section of `AGENTS.md` at the end
  of every session, and commit+push per task (working rules).

## Progress (2026-09-03)

- **T1 COMPLETE.** The library session: `cr-engine/src/library.rs`
  (`Library`: open/save/dirty/scan/watch + QueueManager) behind
  `cr-ui/src/library.rs` (the `Program.DatabaseManager` session).
  The DB lives at `~/.local/share/comicrust/ComicDb/ComicDb.xml`
  (ADR-022; a minimal `cr-core::paths` `SystemPaths` slice — the
  settings port stays open). The reader reuses library books
  (`ComicBookFactory.Create` parity), stamps
  `OpenedTime`/`OpenedCount`, mirrors page turns, and the exit save
  persists the reading state; same-path opens focus the existing
  tab. Fresh DBs carry the default list tree (`create_new` on the
  fallback path — fixed); a missing DB file starts silently
  (`OpenStatus::FreshEmpty`). Watch events rescan the affected
  roots. Acceptance: `crates/cr-engine/tests/library.rs` (fresh-tree,
  real-world session lifecycle incl. the Never Read 255→254 flip +
  byte-stable saves, scan add/missing, watch→rescan). Headless Xvfb
  smoke test: launcher renders with Open + Add Folder, fresh DB
  silent, no criticals.

  Fix during the T1 user test: the first "Add Folder" run froze the
  UI — the scan ran synchronously on the GTK thread. The scan now
  runs on the "Book Scanner" worker thread (books move to the worker
  and back over mpsc + the main-loop pump; ADR-019 pattern), queued
  one at a time, with the exit/background saves guarding an
  in-flight scan (a mid-scan save would write the taken, empty book
  list). A headless probe measured a 705 µs max main-loop gap during
  a scan. The watch-event poll timer (1 s) is wired in `app.rs`.
  Second user-test finding (the "no books found" report): the scan
  had actually re-linked all 23 books (same-name+size recovery from
  the `Z:\` Windows paths to the Linux mount) — the result dialog
  only counted added/updated and misreported it, and the launcher
  window had no save-on-close, so the re-link was discarded on exit.
  Both fixed: the dialog reports added/updated/re-linked/removed,
  and the launcher saves on close (`MainFormFormClosed` → `CleanUp`
  parity). Probes: the app-shaped scan (real 255-book storage + the
  user's folder) yields moved=23 with reading state kept.
  **T1 USER-TESTED, ALL PASS (2026-09-03).** The re-link flow
  ("23 re-linked", persisted after launcher close), the reading-state
  resume on a library comic, the temporary-book reset, and the
  lists ground truth (Read 1 / Never Read 254) all verified on the
  user's machine. Note: closing the reader window currently closes
  the whole app (the browser pane is T5) — expected, the exit save
  runs there.
- **T2 COMPLETE (2026-09-03).** The navigator pane:
  `cr-engine/src/lists.rs` (the tree evaluator, unit-tested incl.
  combine modes, id lists, recursion guard, and the real-world
  default tree) + `cr-ui/src/browser/navigator.rs` (the tree widget,
  debounced selection evaluation, context-menu CRUD through bare
  entry dialogs). The launcher became the browser-skeleton window
  (navigator left, placeholder right showing the evaluated list and
  book count; T5 replaces the placeholder with the ItemView).
  Headless probes: the widget fires debounced selection events with
  correct evaluations (Library 255, Never Read 255 on the fixture);
  screenshots show the tree with icons for both a fresh DB and the
  255-book fixture. `select_next`/`select_by_name` exposed for the
  keyboard/restore paths. A probe lesson: the widget's Rc must
  outlive the window (the host holds it).
  **T2 USER-TESTED, ALL PASS (2026-09-03).** The tree, the debounced
  evaluation with the live reading-state flip (T2.2), and the
  create/rename/delete flows (T2.3, persisted across restart) all
  verified on the user's machine with their 255-book library.
  Lesson: the New Smart List query needs the exact C# form —
  `Match [Series] contains "Batman"` (`Match` keyword, matcher name
  in brackets, operator word, quoted value; the grammar is in
  `cr-engine/src/matcher/query.rs`). The dialog example was
  corrected; the full editor UI is Phase 5.
- **T3 (ItemView core) IMPLEMENTED (2026-09-03), user test
  pending.** The pure modules: `view_state.rs` (the MRU-3 sort
  chain with `Reverse()` semantics, the group buckets ordered by
  the `GroupInfo.Compare` rule — bucket index, then
  ExtendedStringComparer IgnoreArticles|IgnoreCase, deterministic
  tie-break —, collapse by caption, and the full selection model:
  click/ctrl/shift/rubber-band-from-snapshot/focus/anchor),
  `layout.rs` (Thumbnail greedy flow with the `>=` wrap and 2 px
  gaps, Tile 192×96 cells, Detail rows over the column strip with
  the x+8 offset, full-width group headers, collapsed groups drop
  their items, culling, hit tests, column-aware keyboard movement,
  page steps), `columns.rs` (the C# default column set — 13
  visible + the hidden rest), and the engine's
  `display_text.rs` (`GetPropertyValue(proposed: true)` parity:
  Shadow*/AsText/Published/date/registry fallback).
  `item_view.rs` — the DrawingArea in a ScrolledWindow (content
  sized to the virtual size; native wheel scrolling is a documented
  deviation from the C# 16 px line step), draw = culled items with
  selection/focus visuals, group headers, Detail header strip;
  covers ride `add_thumb_to_queue` + the mpsc pump (the ADR-019
  pattern), failures fall back to the error thumbnail; click /
  ctrl / shift / rubber band, arrows / Home / End / PageUp /
  PageDown / Enter, type-ahead (2500 ms). Double-click / Enter →
  `open_reader` (the browser stays — the C# main-form shape).
  Fixed on the way: the rebuild carried collapse flags only after
  the groups reset (read them first), `relative_item`'s `?` skipped
  the row-edge fallback, and a RefCell double-borrow in `set_books`.
  Probe: the browser renders the 255-book fixture grid with
  captions; Detail/Tile geometry unit-tested. T4 adds the real
  cover drawing/badges and the exact text lines.
  T3 fix from the first user test ("no thumbnails, only black
  rectangles"): the thumb pump broke permanently after the first
  idle poll (the worker takes > 10 ms), stranding every completion
  in the channel — the pump now starts whenever loads are in flight
  (`pending_thumbs` counter, started by the draw path after
  queueing). Second defect behind it: the pump decoded the pool's
  cached blob raw — the pool caches the C# `ThumbnailImage`
  serialization (20-byte header + JPEG); parse it with
  `Thumbnail::from_bytes` first. Headless proof: seeded DB with real
  Linux-path comics → the grid draws the actual pages.
  T3 fixes from the user test round 2: (1) arrows/type-ahead never
  reached the grid — the canvas takes focus on click (`grab_focus`
  in the press handler; GTK4 has no click-to-focus) and on window
  activation (the reader's is-active re-grab on the shell window);
  (2) double-click opened the reader once — the closed reader window
  stayed in the app's session slot and silently swallowed every
  later open; the main window's close-request now clears the slot
  (`app::reader_closed`). Known cosmetic critical at startup:
  `gtk_css_node_insert_after` assertion (GTK-internal CSS ordering,
  no user-visible effect — investigate when the shell lands in T5).
- **T3 COMPLETE — USER-TESTED, ALL PASS (2026-09-04).** The grid
  with real covers for the re-linked comics, selection
  (click/ctrl/shift/rubber band), full keyboard navigation with the
  focus split between tree and grid following the last click,
  type-ahead, scroll, list-driven sets, and double-click → reader
  (repeatedly; the reader window re-opens fresh after close) all
  verified on the user's machine. Known cosmetic startup critical:
  `gtk_css_node_insert_after` (GTK-internal CSS ordering; revisit
  with the T5 shell).
- **T4 IMPLEMENTED (2026-09-04), user test pending.** The item
  drawing lives in `cr-ui/src/browser/item.rs` (+ the caption
  engine in `cr-engine/src/display_text.rs`). Headless probe: real
  covers draw with border/shadow/frame, the read-marker ribbons
  ride the right edge, the numeric tags render (gold personal,
  blue community, absent at 0), captions wrap centered with the
  C# group degradation. 236 tests green.
- **T4 COMPLETE — USER-TESTED, ALL PASS (2026-09-04).** Covers
  (frame/shadow), captions (the C# format with the group
  degradation), the read-marker ribbons at the reading position
  (moved after reading), the rating tags (gold personal / blue
  community, none at 0), and the missing-file red cross after a
  rescan all verified on the user's machine. Open question the
  user raised: opening a comic shows the reader in a NEW window —
  correct answer: docked (the C# main-form shape); that is T5's
  deliverable (the reader as a view in the browser window, the
  Phase 3 window becomes the undock path).
