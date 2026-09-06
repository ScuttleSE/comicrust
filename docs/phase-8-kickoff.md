# Phase 8 Kickoff — Polish/Ship + the user-reported list

Goal: ship quality — packaging, docs, migration tooling — plus the
user-reported UX/perf items collected on 2026-09-06 after the Phase 7
close-out.

Target crates: `cr-ui` (T1-T6), `cr-core`/`cr-engine` (T3, T4, T7),
new packaging files (T8).

## User-reported items (2026-09-06, verbatim scope)

1. Default view when opening up the app should be Library view.
2. Slow operation when adding a large CBL file — several minutes for
   ~2500 titles (example in
   `tests/testfiles/[Spider-Man] 00 - Complete 616 Chronology.cbl`).
3. Slow operation when deleting empty books — ~200 tagged empty
   books, delete, the app hung for close to a minute.
4. Explore moving from the XML file for the database to a
   sqlite/postgres backend instead.
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
- Database backend: see T7 — this touches compatibility invariant 1
  (byte-stable ComicDb.xml); an ADR is REQUIRED before any change.

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

### T5. Details-view column resize (item 6)

- The C# Detail columns drag-resize; widths persist in
  `ItemViewConfig.Columns` (the T14 workspace). Check what the
  port's `workspace.rs`/`columns.rs` store (the T14 record carries
  `Columns` — widths may be missing).
- Implement: a drag handle in the Detail header (hit zone + a
  grab cursor), min widths, live reflow; persist the widths in the
  workspace (the T14 write path).
- Gate: `workspace_probe` + a probe (drag → the column width + the
  persisted round-trip), then the user test.

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

### T7. Database backend exploration (item 4) — RESEARCH SPIKE → ADR

- CONSTRAINT (compatibility invariant 1): byte-stable ComicDb.xml
  read/write is the ComicRack interop — the database is the one
  artifact users cannot lose. A different canonical store breaks
  interop with real ComicRack and the migration path.
- Postgres: rejected for a desktop app (no server, no daemon) —
  record the reasoning in the ADR regardless.
- Spike deliverable (ADR-029): measure the actual save/open cost of
  the XML on a large library (10k+ books synthetic), compare with an
  SQLite index of the same data, and decide between:
  (a) SQLite as a DERIVED index/cache — ComicDb.xml stays canonical
      (safe, keeps interop; the C# itself keeps XML),
  (b) SQLite as the canonical store — a compat BREAK requiring
      explicit user sign-off + a lossless XML export path.
- NO code lands from this task without the ADR + user approval.

### T8. Packaging

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

## Order

T1 (quick wins) → T2 → T3 → T4 (the user pains first) → T5 → T6 →
T7 (spike) → T8 → T9 → T10. T7's spike may run parallel to T5/T6
(read-only work).

## Status

- Kickoff written 2026-09-06. T1 is next.
