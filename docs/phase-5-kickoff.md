# Phase 5 Kickoff — Dialogs

Goal: comicrust's remaining interactions become real — the book
editor (edit a comic's metadata and write it back to the file), the
smart-list editor, the preferences dialog, and the export dialog.
The browser shell's Phase 4 stubs (the Properties placeholder, the
navigator's bare entry dialogs, the "Files to update" list) become
the real commands.

Read first: `AGENTS.md` (rules + status), `docs/phase-4-kickoff.md`
(the browser — its shell hooks are this phase's integration
points), `docs/decisions.md` (ADR-004 no libadwaita, ADR-018 GTK
4.0-era surface, ADR-022 the library session). The reader
(`docs/phase-3-kickoff.md`) and the browser already carry the
user-test protocol — it stays mandatory.

## Why this phase is long

The C# dialog layer is ~50 dialogs backed by two reflection
engines: `FormUtility` (883 LOC — builds options panels from
`[Category]/[Description]` attributes, the PreferencesDialog is
DATA-DRIVEN from the Settings object) and the matcher editor tree
(`MatcherEditor`/`MatcherGroupEditor`, 334+ LOC — the smart-list
query builder UI over the matcher registry). The port must rebuild
both engines in GTK4 and then the dialogs on top.

| C# file | LOC | Role |
|---|---|---|
| `ComicRack/Dialogs/ComicBookDialog.cs` | 1,183 | The book editor: all metadata fields, per-page edits, thumbnails |
| `ComicRack/Config/Settings.cs` + `EngineConfiguration.cs` | ~3,000 | The settings object the Preferences dialog renders (the settings port is ALSO a Phase 0 tail — see below) |
| `Dialogs/PreferencesDialog.cs` | 1,512 | The preferences tree (data-driven panels) |
| `cYo.Common.Windows/Forms/FormUtility.cs` | 883 | The reflection-driven options builder |
| `Dialogs/SmartListDialog.cs` + `SmartListQueryDialog.cs` | ~800 | The smart-list editor (visual + query forms) |
| `Dialogs/MatcherEditor.cs` + `MatcherGroupEditor.cs` | ~700 | The matcher property editors |
| `Dialogs/ExportComicsDialog.cs` | 360 | Export (the cr-io export skeleton exists) |
| `Dialogs/MultipleComicBooksDialog.cs` | ~400 | Bulk edit |

Supporting: `ValueEditorDialog`, `QuickRatingDialog`,
`DeleteItemDialog`, `ListEditorDialog` (reading lists),
`ListLayoutDialog` (columns), `ComicDisplaySettingsDialog`,
`SaveWorkspaceDialog`, `ProgressDialog`, `Splash`.

## The settings port (Phase 0 tail — REQUIRED EARLY)

The Preferences dialog renders the Settings object; the reader
hard-codes `TRACK_CURRENT_PAGE`, the caches are memory-only, and
the QuickOpen thumbnail size is fixed — all wait on this. Port
`IniFile` (`cYo.Common/Runtime/IniFile.cs`), `EngineConfiguration`,
and the Settings object into `cr-core` (or a new `cr-settings`
module) with:
- The ini persistence (`config.ini` beside the DB — the C# stores
  it in `ApplicationDataPath`).
- Field-level defaults matching the C# (`[DefaultValue]` parity —
  Phase 1/2 hard-coded the important ones with comments; reconcile).
- A typed registry the GTK options builder can walk (name, type,
  category, description — the `FormUtility` data).
This unblocks: the Preferences dialog, the reader's settings-gated
behaviors (`TrackCurrentPage`), cache locations (the `type://`
custom-thumbnail loader + the CustomThumbnails path — deferred
from Phase 4 T4), and the QuickOpen thumbnail size.

## Task breakdown

Order: T1 unblocks everything; T2 is the critical path (the
deepest dialog); keep the editors pure-model first, GTK last.

### T1. The settings port + the options builder — COMPLETE (2026-09-04), user-tested

- [x] `IniFile` + `EngineConfiguration` + Settings in cr-core (or
      `cr-settings`): field defaults = the C# `[DefaultValue]`
      attributes; ini round-trip tests. Done:
      `cr-core/src/settings/` — `ini.rs` (the `IniFile` port:
      sections, `;`/`#` comments, first-`=` split, key trim +
      value trim-start, case-insensitive binding, the `|` file
      chain, the unanchored `-switch=value` regex), `registry.rs`
      (the typed `FieldDesc` tables + the `settings_fields!` macro +
      the `EnumValue` currency — the reflection replacement),
      `engine_config.rs` (all fields with the C# CONSTRUCTOR
      defaults — the stale `[DefaultValue]` attributes noted:
      BlendDuration 400 not 250, ParallelConversions 32 not 4 —,
      the Size/Color converter texts, the setter clamps as a
      post-load `normalize`), `extended.rs` (the command-line
      switch table with short names, bool TOGGLE semantics, the
      `[IniFile(false)]` command-line-only fields, `files` from the
      plain args), `settings.rs` (the ~120 scalar Settings fields +
      the Config.xml read/write via the Emitter — declaration-order
      elements, null strings omitted, empty strings self-closing,
      Size as Width/Height children, StringPair as Key/Value
      attributes; unknown elements skip so a Windows Config.xml
      loads). Tests: ini round-trip + binding, converter tests,
      argv semantics (toggle, swallow, files), Settings byte-stable
      round-trip + lenient read + corrupt→default.
      IMPORTANT correction to this doc's original text: the C#
      does NOT persist Settings as an ini — `Program` stores
      `Config.xml` (`Settings.Load/Save` via `XmlUtility` =
      XmlSerializer) and only `EngineConfiguration`/`ExtendedSettings`
      ride the ini (`ComicRack.ini` → `comicrust.ini`). Ported as
      the source says.
- [x] Reconcile the hard-coded stand-ups: `TRACK_CURRENT_PAGE`
      (reader — now `Settings.track_current_page`, read per open),
      `AddToLibraryOnOpen` (the reader flow now mirrors
      `ComicBookFactory.Create(file, AddToStorage)`: a new book with
      `AddedTime = now` joins the library when the setting is on),
      the engine defaults 14/95/10 (the default list tree in
      `create_new()` + the QuickOpen built-in lists read
      `EngineConfiguration::global()`), `ComicNameInfo`'s
      `OfValues`/legacy-parser flag (the global engine config;
      `from_file_path` uses `OfValues ?? "of,von,de"`). Also wired
      live: `MouseWheelSpeed`, `ScrollingDoesBrowse`,
      `PageChangeDelay` (the 300 ms page wall — `PageWallTicks`
      parity), `HideCursorFullScreen` +
      `ExtendedSettings.AutoHideCursorDuration` (5000 ms, was a
      hard 1000), `AutoMinimalGui` (fullscreen toggles minimal GUI),
      `ShowQuickOpen`, `QuickOpenThumbnailSize` (applied at startup,
      stored on exit — the C# `UpdateSettings`/close flow). The
      settings load at startup (`cr-ui/src/library.rs` — Config.xml
      + the ini chain + argv into the `EngineConfiguration`/
      `ExtendedSettings` globals) and save on the main-window close
      (`library::save_settings`).
- [x] The GTK options builder (`cr-ui/src/settings/options.rs`):
      walks the typed registry → labeled check boxes per browsable
      bool with a description, collapsible groups by category
      (first-encounter order, later re-encounters merge — the C#
      finds the existing `CollapsibleGroupBox`), rows sorted by
      description — the `FormUtility` parity (which is bool-only:
      the numeric/enum widgets are hand-built on the C# pages, as
      here). Unit-tested row set + consolidation semantics.
- [x] Wire the Preferences dialog shell (tree of categories +
      panels), OK/Cancel semantics (edit a clone, commit on OK).
      Done: `cr-ui/src/settings/preferences.rs` — the sidebar shell
      with the C# five-tab shape: Reader (wheel speed + RTL combo —
      GTK 4.0-era widgets per ADR-018), Behavior (the auto panel),
      Libraries (watch folders: list + Watch toggles persisted into
      the DB + add/remove), Advanced (the cache spins with the C#
      ranges 20..100 / 5..500, disk MB fields, the
      `chkUpdateComicFiles` enable/uncheck chain). OK commits the
      clone into the session, saves Config.xml, re-applies the
      display settings to open reader views + the QuickOpen size.
      Deviations recorded: the Scripts page hidden until Phase 6
      (no plugin host), no language list until the TR loader port,
      the backup/association groups are Windows-shell features
      (Phase 8). Opened from the header "Preferences" button.

### T2. The book editor (`ComicBookDialog`) — COMPLETE (2026-09-04), user-tested (both checkpoints)

- [ ] The metadata form: every ComicInfo field the registry
      exposes, the proposed-value flow (`EnableProposed` — the
      gray "proposed" text from the filename parse, accept
      per-field), the `Checked` semantics. DONE (checkpoint 1):
      `cr-ui/src/dialogs/book_editor.rs` — the left column (cover
      thumb via the thumb queue + the mpsc pump, Page/Type/Size/
      Path labels, prev/next for multi-book edits) beside Details
      (all registry text/number rows + the four combos), Plot
      (Summary/Notes/Review TextViews + Characters/Teams/Main/
      Locations), Catalog (the book* fields + ISBN + Added/Released
      date entries). Load = `SetComicToEditor`/`SetDataToEditor`
      parity through the cr-core registry; save = `SaveBook` parity
      (trimmed text assigns unconditionally; `GetNumber`/`GetReal`
      parse-or--1; YesNo/Manga combos; rating/date lenient parses).
      The proposed placeholders (the seven fields) follow the
      EnableProposed combo. Commit points = Apply / OK / prev/next
      (the C# live-object semantics: Cancel does not revert already
      committed edits). The commit applies via
      `library::apply_edited` (replace by id + mark dirty).
- [ ] The thumbnail page: front-cover choice (a page typed
      FrontCover becomes the cover — `front_cover_page_index`
      ported: PreferredFrontCover clamp, first non-Other fallback),
      custom thumbnail set/clear (needs the pool `type://` loader —
      DEFERRED to checkpoint 2, the path exists per ADR-023), the
      page-type/rotation/position edits per page (the Pages panel's
      deferred edit commands). DONE (checkpoint 1): the Pages tab —
      the page list (number/type/rotation/position captions), the
      preview + first/prev/next/last, the context menu with Set
      Page Type (the 11 single values with the C# enum values),
      Rotate, Position, Mark as Deleted (toggle), Move to Top /
      Bottom (the `MovePages` cursor algorithm with the C# IndexOf
      identity — unit-tested), Reset Original Order (sort by
      ImageIndex). The Colors tab: the five sliders writing
      `book.color_adjustment` on save (saturation/brightness/
      contrast/gamma ±100%, sharpening 0..3) with the loaded values
      on every load. Custom values: read-only list in checkpoint 1
      (the library-wide key editor joins with the write-back).
- [x] Write-back: `ComicBook.IsDirty` → the write-info queue
      (`write.rs` — the CBZ/CBT native rewrite, CB7 `7z u`) with
      the Phase 1 rules. DONE (checkpoint 2): `apply_edited` marks
      the book `comic_info_is_dirty` (the C#
      `WatchedBookHasChanged` parity) and schedules the debounced
      100 ms write; `library::update_book_file` ports
      `AddBookToFileUpdate` + `WriteInfoToFileWithCacheUpdate` —
      the gates (`UpdateComicFiles`, then
      `AutoUpdateComicsFiles || alwaysWrite`, then the dirty flag),
      the scoped write (ComicBook.xml only when
      `UpdateComicBookFiles` — `store_info_scoped`, the C#
      `GetInfo()` scope parity), the file-properties refresh, and
      the flag clear. The "Files to update" smart list flips (the
      matcher reads the flag). The browser context menu gains
      "Update Book File(s)" (the `alwaysWrite: true` manual path).
      Probe-proven end-to-end (`writeback_probe`): an isolated
      library + settings, an edit through `apply_edited`, and the
      archive's ComicInfo.xml carries the edit with the flag
      cleared. Deferred: the exit-time `SaveDirtyBooks` ask-dialog
      for TEMPORARY books (session books stay session-only), and
      the ComicBookIsDirty half (the port never sets it).
- [x] Bulk edit (`MultipleComicBooksDialog`): DONE (checkpoint 2):
      `cr-ui/src/dialogs/bulk_edit.rs` — the same row set as the
      single editor, a "Set" check per field (unchecked = leave;
      the C# tri-state list-merge mode deferred, recorded), the
      gray cue = the common value (`GetSameValue`), OK applies
      only the checked fields through the registry to every book
      and commits each changed one. The context menu gains
      "Edit…" over the selection.
      First user-test finding (the grid did not show the edits):
      the context-menu commands never refreshed the ItemView —
      both the bulk "Edit…" and the single "Properties…" commit
      closures now call `refresh_view_from_list` per commit (the
      "remove" command pattern).
      **CHECKPOINT 2 COMPLETE — USER-TESTED, ALL PASS (2026-09-04).**

**CHECKPOINT 1 COMPLETE — USER-TESTED, ALL PASS (2026-09-04).**
Six fix rounds total (the record below). The user confirmed: the
editor opens with the cover, the full metadata round-trips through
OK + restart, the Pages tab lists every page for all three stored-
list shapes (full / partial / none) with the stored overlays, page
type/rotation/position edits land, mark-deleted hides the page
from the reader flips, move/reset reorder correctly, the cover
follows the FrontCover rule, the colors apply in the reader, the
nav buttons move the preview + highlight, and the proposed-value
placeholders toggle with EnableProposed.

Headless proof: the probe binary (`cr-ui/examples/editor_probe.rs`)
opens the editor over synthetic books under Xvfb — the Details grid
renders all rows/combos with the loaded values, the Pages tab lists
the pages with the C# type names, the page-type enum values match
the model (FrontCover = 1; the first draft had the values shifted —
caught by the probe). The context menus (browser + page rows) are
user-test scope (the synthetic button-3 injection does not reach
GTK gestures — the standing probe lesson).

First user-test round (2026-09-04) — five defects, all fixed:
1. No cover thumbnail: the pool blob carries the `ThumbnailImage`
   serialization header — the editor decoded it raw. Fixed with
   `surface_from_thumb_blob` (the pages_view lesson re-learned —
   it is now in bitmap.rs for shared use).
5. Page clicks did nothing: the list had SelectionMode::None and NO
   selection handler. Fixed: SelectionMode::Single +
   `connect_row_selected` → page/preview (`PagesViewSelectedIndex
   Changed` parity); rebuilds re-select the current page's row.
6/7/8. The page menu items did nothing: the PopoverMenu +
   action-group route did not activate. Replaced with the
   user-tested manual popover + buttons (the browser context menu
   mechanism), sections for type/rotation/position + the commands.
9. Colors "no change": the READER never passed the book's color
   adjustment into the page keys (`page_key` used an empty
   adjustment). Fixed: `PageView::set_base_adjustment` from the
   book on open (the C# `ComicDisplay` renders with
   `book.ColorAdjustment`); the dialog preview now uses the
   WORKING adjustment (the slider state) — live preview parity.
Verified after the fixes (probe, real comic from
`tests/testfiles/`): the cover renders, the preview shows pages
with the nav buttons ("Page 2" renders page 2 — the debug build
needs ~5 s per page decode; the release build is fast).

Second user-test round (2026-09-04) — the reader-side page model
was the gap (the C# navigates a FILTERED page list and reads by
`ImageIndex`; the port was positional 1:1):
- Reader: `PageView::open_with_sequence` — the shell builds the
  display sequence from the book's page entries (Deleted drops,
  the default `PageFilter` = All parity) and every page read
  resolves through it (`page_key` → `Pages[page].ImageIndex`).
  The provider-index fill now stamps `Image` (`set_image_index(i)`)
  like the C# handler. Books without page entries keep 1:1; an
  all-deleted list falls back to 1:1 (safety).
- Editor: the cover refreshes on a page-type change (the `after`
  path re-queues the cover); the preview/cover read the ENTRY's
  `ImageIndex` (a reorder moves entries, not archive slots — the
  "move to top changed the captions but not the pages" report);
  the nav buttons move the list highlight (`select_row`, the
  selection hook guards the same page).

Third user-test round (2026-09-04) — the completion-payload bug:
the page-queue callback reported `k.key.index` (the PROVIDER
index) as the display page, so under a sequence every completed
image landed in the wrong slot (blank page 1, scrambled flips).
Fixed: the callback carries the requesting DISPLAY position.
Proven end-to-end with the `reorder_probe` recipe (seed an
isolated DB with a moved page, launch the app): display 0 renders
archive index 1 (`dispatch: display=0 key_index=1`). The probe
also exposed that GApplication hands the app ABSOLUTE paths — the
seed must store absolute paths like the real scanner.
Cover note: setting page N to Front Cover does NOT move the cover
while an EARLIER FrontCover page exists — `FrontCoverPageIndex`
takes the `PreferredFrontCover`-th (default 0) FrontCover page,
C# parity. Moving the cover = change the old cover's type too
(Story), or set the new cover when no earlier FrontCover exists.

Fourth round (2026-09-04): the editor's Pages tab was empty for
titles without a STORED page list — the C# editor opens the comic
through a navigator (the provider index fills `Pages`); the port
never did. Fixed: the editor load fills `info.pages` from the
provider when empty (the reader_shell fill parity, `Image` = i),
and the filled list persists on save (`comic.SetPages` parity).
Probe-proven with a page-list-cleared book: the list, preview, and
cover render.

Fifth round (2026-09-04) — the real C# open semantics: PageCount
always comes from the PROVIDER and the stored entries OVERLAY it
(`ProviderIndexRetrievalCompleted` → PageCount = provider count +
`TrimExcessPageInfo`; `GetPage(i)` returns the entry or a default).
A PARTIAL stored list (1-2 entries — most migrated books) must not
shrink the display. New `cr-ui::pages::merged_page_entries` builds
the full provider-count list with the stored entries overlaid
(usable-Image fallback to the position); the reader shell and the
editor both use it (unit-tested with a folder provider). The
merged list persists on save like the C# navigator's book.
PROCESS lesson from the round: the editor half of the fix was
landed by a scripted edit that silently matched NOTHING (the
target text had drifted) — the compile stayed green because
nothing changed, and the user retest caught it ("just showed
FrontCover"). Rule: a scripted multi-line replacement must be
verified by grepping for the NEW symbol in the changed file, not
by the build result.

### T3. The smart-list editor (`SmartListDialog` + matchers) — CHECKPOINT 1 COMPLETE (2026-09-04), user-tested

- [x] The visual matcher builder: property combo (the registry),
      operator combo (the spec's operators), 1–2 argument fields,
      and/or mode, not flags, nested groups (`MatcherGroupEditor`).
      DONE: `cr-engine/src/matcher/edit_ops.rs` (the pure command
      model — add_rule duplicates after the node, add_group wraps a
      clone in a new And-group, delete (blocked at one node — the
      C# disables Delete), move up/down, and the type switch
      keeping values + clamping the operator into the new spec's
      list (`newMatcher.Set(current)` parity); MAX_LEVEL 5; the
      nested paths address containers — 5 unit tests) and
      `cr-ui/src/dialogs/smart_list.rs` (the dialog: the head
      fields — name/notes/base-list combo with the
      `RecursionTest`-style recursion filter/ALL-ANY mode/Not-in-
      base/limit type+value/QuickOpen — plus the matcher rows: the
      type combo over all 97 spec descriptions, the operator combo
      per spec, 0-2 value fields per the argument count, the Not
      check, and the right-click edit menu New Rule / New Group /
      Delete / Move Up / Down; structural changes rebuild the row
      area wholesale). Group rows carry their own ALL/ANY combo and
      nested rows.
- [x] The query tab: the text form with the live parse (the Phase 2
      query language). DONE: one dialog with a Designer | Query
      notebook (the C# Ctrl-swaps two dialogs). Entering Query
      renders the item (`item_to_query` → `render_smart_list_query`,
      raw→engine via `Matcher::from_raw`); OK parses the text into
      the item (engine tree → `Matcher::to_raw`); a parse failure
      blocks the close with the error line (the C# keeps the old
      item).
- [x] The navigator's "New Smart List" opens the editor; Edit
      opens it pre-filled. DONE: `library::new_smart_list` now
      returns the new id; New Smart List inserts an empty list and
      opens the editor (Cancel removes the fresh empty insert —
      the C# flow); "Edit Smart List…" (the new nav command) opens
      pre-filled and commits via `library::update_smart_list`
      (`SetList` parity: id/position/book counts/cache stay).
      `EditListDialog` for folders stays the bare name prompt
      (deferred with the reading lists).
- [x] The reading-list editor + the folder `EditListDialog`.
      DONE: `cr-ui/src/dialogs/list_editor.rs` (the `EditListDialog`
      port — the C# routes FOLDERS and READING LISTS through it from
      the one `miEditSmartList` menu item; `ListEditorDialog` in the
      C# is an unrelated workspaces editor). Folders: name/notes +
      the combine mode ("All Books from every list" / "Only Books
      existing in every list" / "Empty list"); reading lists:
      name/notes + QuickOpen. The navigator gains "New List…"
      (dialog-first, then the insert — the C# `NewList`; a cancelled
      fresh insert pops) and New Folder/Edit route through the
      dialog (`library::{new_id_list, new_folder -> id,
      update_list_fields}` with the `SetList`-style base-field
      preservation); Rename routes through Edit (the C# has no
      separate rename). Headless probe: the full New Folder flow —
      dialog renders (Name/Notes/Combine), the typed name lands in
      the tree after OK. The done-Cell re-entrancy guard applies
      here too. The reading list's BOOK management (the orderable
      drag-in list) is the browser's "add to reading list" flow —
      Phase 6/7 polish with the drag-drop work.
Headless proof: the app probe seeds a fresh DB, right-clicks the
navigator, New Smart List… opens the editor with the head fields
(the Base List combo showing Library, the limit row disabled).

First user-test finding (the named list VANISHED after OK): two
bugs, both probe-confirmed with `CR_DEBUG_SL` instrumentation —
(1) the OK handler read the Query tab's text, which is EMPTY
unless the user visited that tab, and an empty text CLEARED the
matchers (the designer's rule was discarded); (2) `dlg.close()`
inside the OK arm makes GtkDialog emit the delete-event Cancel
response RE-ENTRANTLY, which ran the None arm and REMOVED the
fresh uncommitted insert (`update changed=false` in the log —
the item was already gone). Fixed: a `query_dirty` flag (set on
query-buffer edits, cleared when the tab renders the item) makes
OK parse the text only when edited — a designer-only session
commits the state the row widgets wrote; a `done` Cell swallows
the re-entrant response. The probe log after the fix: `matchers=1
... changed=true`, and the tree shows the list.
**CHECKPOINT 1 COMPLETE — USER-TESTED, ALL PASS (2026-09-04).**

### T4. Export + the remaining dialogs

- [ ] Export dialog (`ExportComicsDialog`) over the `cr-io` export
      skeleton: format, compression level, page range, target
      naming; the parallel/spill/progress plumbing the skeleton
      deferred.
- [ ] Quick rating (the reader's close flow), delete-confirm,
      progress dialog, splash — the small dialogs the flows above
      need.
- [ ] Devices/sync and the remote server stay OUT (Phases 6-7 per
      the port plan).

## Non-goals for Phase 5

- The remote server, device sync, scripting — Phases 6-7.
- Workspace persistence (save/switch layouts) — Phase 7, except
  the SaveWorkspaceDialog if the display-config work lands early.
- Packaging — Phase 8.

## Test strategy

- The model layers (settings round-trip, matcher-editor state,
  export options) are pure and unit-tested like every phase.
- Dialogs get headless Xvfb screenshots (the Phase 3/4 probe
  lessons: `windowfocus` before keys, screenshots decide
  rendering) and the mandatory user-test protocol per task.
- The write-back tests reuse the Phase 1 golden rules: a metadata
  rewrite touches only the metadata entries.

## Risks / lessons that apply

- The settings port is the phase's long pole by implication: EVERY
  dialog reads or writes it. Do T1 first and reconcile the
  hard-coded stand-ups immediately (they drift otherwise).
- GTK has no WinForms property-grid — build the options builder
  from the typed registry, not reflection. Keep the registry the
  single source (the matchers, columns, and options all read it).
- The dialog code quality bar from the C# is low (decompiled) —
  port the OBSERVED behavior; where the C# swallows errors or
  double-applies defaults, match the compiled behavior and record
  it (ADR per deviation).
- The Phase 4 per-cache rule stands: any new per-frame or
  per-dialog text rendering caches per book/entity.

## Update at the end of every task

- [x] T1 settings port + options builder + Preferences shell
- [x] T2 book editor + bulk edit + write-back wiring (both
      checkpoints user-tested)
- [ ] T3 smart-list/reading-list editors
- [ ] T4 export + small dialogs

Check the boxes here and in `AGENTS.md`'s status as tasks close.
The phase gate: a user edits a comic's metadata (single + bulk),
saves it back to the file (verified outside the app), edits a
smart list both visually and as a query, exports a comic, and
changes preferences that visibly move the reader and browser.

## Progress (2026-09-04)

- **T1 COMPLETE — USER-TESTED, ALL PASS (2026-09-04).** The user
  verified: the Preferences button opens the dialog; the Behavior
  page matches the C# auto-panel (groups, defaults, sort, wheel
  scrolling); a toggle + OK persists across a restart (Config.xml
  in `~/.config/comicrust` carries the change); Cancel discards;
  the page-wall setting visibly changes the reader's margin
  behavior; "Opened Files are added to the Library" adds non-library
  comics on open; the fullscreen cursor hide (5000 ms) and
  AutoMinimalGui work; the Libraries page lists watch folders with
  add/Watch-toggle persistence. The phase-5 doc above carries the
  full implementation record.
