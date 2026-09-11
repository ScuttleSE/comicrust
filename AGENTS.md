# AGENTS.md — Agent Onboarding

You are working on **comicrust**. This project is a from-scratch port of **ComicRack Community Edition** (a Windows C# WinForms comic library manager/reader). The target is a **Linux-native Rust + GTK4 application** with **full 1:1 feature parity**.

Read this file first. Then read `docs/port-plan.md` (architecture and roadmap), `docs/decisions.md` (locked decisions), and the active phase's kickoff doc (`docs/phase-<N>-kickoff.md` — the status section below names the active one; when NO phase is active, open work is picked from `docs/backlog.md` and re-homed into a kickoff first). Do not challenge a locked decision without explicit user approval.

---

## Agent working rules (hard limits)

These rules are absolute. Break none of them. If you break them, you waste the user's time and tokens.

### Rule 0: Limited speculation — then ask (THE MOST IMPORTANT RULE)
- This rule MUST NEVER BE BROKEN.
- If you do not know something, you get ONE speculation, tops TWO.
- After that, ask the user for clarification. Any random guessing is strictly forbidden.
- If you do not have enough data, ask for it or suggest adding logging.
- Do not "hold on, actually" yourself a dozen times and then come up with something anyway. That wastes time and tokens.
- This ban includes do-then-fantasize cycles: do something, fantasize a cause, do something else, fantasize again. Two speculations total, then stop and ask.

### Rule 1: No loops
- Do not guess in a loop. Do not repeat actions that give no new information.
- If the cause of a problem is not clear after two or three file reads, stop.
- Ask the user one question that targets the problem. Then wait for the answer.
- Do not chain guesses. Do not say "let me check one more thing" again and again.
- If you start to loop, or if you repeat searches without a clear answer, stop immediately.
- Do not wait until you notice the loop. Do not start a loop.
- You can continue after a loop only if you ask the user first. There is no other way.

### Rule 2: Short answers first
- Give the short answer first. Then stop. Do not write a long block of text.
- When you ask the user to do a task, write only the task. Then wait for the result.
- Do not add a plan, a hypothesis, or a "what I am looking for" section to a request for action.
- Do not restate the plan after each step. The user reads the plan one time.
- Keep each reply short. Add detail only if the user asks for it.

### Rule 3: Evidence before claims
- Find the true cause before you state a cause.
- Do not blame or clear a change without evidence. Get a measurement first.
- Do not assume the user's environment. The user runs the binary on a different machine.
- Local disk state, tools, and timing do not transfer to that machine.
- Trust the trace over the theory. If strace or gdb data conflicts with your reading of the code, the data wins.
- A "window did not appear" symptom means the main thread blocks. Find the main-thread stall (the futex or syscall gap). Do not look only at background workers.
- Confirm that the fix solves the measured problem. Do not stop at "it builds".

### Rule 4: Commit and push
- After you complete a change, commit all changes. Then push.
- Do not leave work uncommitted. A push starts CI.

### Rule 5: Language
- Write all communication and documentation in Simplified Technical English (ASD-STE100).
- Use the asd-ste100 skill for new text and for rewrites.

### Rule 6: Never block the GTK main thread
- Any new operation that can take real time (archive I/O, a
  `7z`/`rar`/`gio` subprocess, a big scan or DB pass, image decode)
  MUST run on a worker thread. The established shape: clone the
  needed data on the main thread, run the work on a `std::thread`,
  ship results over a std mpsc channel, drain with a
  `glib::timeout_add_local` pump (100-500 ms), and apply
  library/session changes back on the MAIN thread (the session is
  thread-local). Existing precedents: the Book Scanner
  (`scan_async`), the export dialog worker, the Info Writer
  (`update_book_file_async`), the cache-event pump, the folder scan.
  GTK widget handles (Rc) and `Box<dyn FnOnce>` callbacks are NOT
  Send — they stay on the main-thread side of the channel.

---

## Current status (KEEP UPDATED)

Update this section at the **end of every work session**. The next agent must know the exact state of the work.

### State summary

- **SMART-LIST EDITOR: RULE DELETE + CLIPBOARD OPS (2026-09-11,
  commit 19da428; user report "there doesn't seem to be a way to
  remove a rule"; user approved the scope incl. the clipboard ops):**
  ROOT CAUSE — the Delete op existed (`edit_ops::remove_node`, wired
  in the dialog) but its ONLY trigger was a right-click popover the
  row's own widgets swallowed (Entries claim button 3, an Entry shows
  GTK's text menu), so the menu never appeared. C# PARITY RESTORED:
  every MatcherEditor/MatcherGroupEditor row carries a visible
  `btEdit` dropdown (a 21 px button with `Resources.SmallArrowDown`
  at the row's right edge, MatcherEditor.Designer.cs:176-186) that
  opens the `cmEdit` menu; the port now mounts a MenuButton (the
  bundled `SmallArrowDown` texture) on value rows AND group frames
  (group: right edge of the mode row) opening the same row menu —
  the row right-click survives as a port addition. Menu = the
  Designer order (New Rule, New Group, Delete | Cut, Copy, Paste |
  Move Up, Move Down) with the `cmEdit_Opening` enable states
  (MatcherEditor.cs:76-85) re-run at every open (the button's
  `activate` + the right-click press): Delete/Cut iff the container
  holds >1 node, Move Up/Down by index, New Group at the
  `MAX_LEVEL` cap (`path.len() < 5` — consistent with the port's
  `add_group` gate, which is ONE level stricter than the C#
  `level <= 5`; pre-existing port cap, unit-pinned), Copy always,
  Paste iff the clipboard carries the matcher MIME. NEW: the
  Cut/Copy/Paste ops (the user picked "also port clipboard ops") —
  `ComicBookMatcher::to_clipboard_bytes`/`from_clipboard_bytes`
  (cr-core list_items: ONE `<ComicBookMatcher>` element wrapped in a
  namespace root; 2 unit tests) under the private MIME
  `application/x-comicrust-matcher` PLUS text/plain (a gdk
  ContentProvider UNION — the MIME gates Paste, read_text reads
  back); `edit_ops::paste_node` (insert AFTER the addressed node, a
  group payload rejected at the cap — the C# `level <= MaxLevel`
  shape; 2 unit tests); the UI `run_row_op` Copy/Cut arms write the
  clipboard synchronously (Cut = copy + remove), Paste reads
  `read_text_async` and applies through the shared `apply_paste`
  (insert + rebuild) on the main thread. DEVIATIONS: the C# ships a
  WinForms binary clipboard object (`ComicBookMatcher.ClipboardFormat`)
  — the payload is the XML element instead, so cross-instance paste
  needs the same comicrust build; the C# menu shortcut keys (Ctrl+R/
  G/X/C/V/U/D) are not bound. PROBE TRAP (measured): the Xvfb
  clipboard read hangs on every read AFTER the first set+read cycle
  (X11 selection transfer without a WM) while the identical sequence
  works in a minimal window — the probe injects the paste payload
  through `probe_paste_payload` (the REAL apply path; only the GDK
  read is skipped) and gates Cut/Copy via
  `probe_clipboard_has_matcher` (formats() is sync); the round-trip
  itself is user-test territory on a real desktop. GATES: the
  rewritten `smartlistmenu_probe` — A the rows carry the edit
  machinery (3 rows), C the enable states (row 0: Up off/Down on;
  row 2 the reverse; Delete on), B delete through the REAL action
  path (Series A gone, rows re-registered), F copy→paste inserts a
  clone after row 1 ([B,C,B]), G cut (remove + clipboard set), D
  delete-to-1 disables Delete/Cut, the post-cut paste restores 2,
  H OK commits the reduced set, then a depth-5 group-chain editor:
  the value row (index 5) has New Group DISABLED, a group payload
  paste is REJECTED at the cap while a value payload pastes. A
  row-destruction fix rode along: the row's right-click PopoverMenu
  now unparents on the row's destroy (`connect_destroy`) — GTK
  warned "Finalizing GtkFrame ... still has children left". 506
  tests; fmt/clippy green. USER TEST = rebuild, open a smart list's
  editor: every rule row and group carries a small ▾ button at the
  right edge — click it → New Rule / New Group / Delete / Cut /
  Copy / Paste / Move Up / Move Down with the honest enable states
  (the single remaining rule cannot Delete/Cut); Delete removes a
  rule; Copy on one row + Paste on another inserts a clone; a Query
  tab round trip stays clean.
- **"NO METADATA" TAG ON EMPTY BOOKS (2026-09-11; user request
  "a subtle tag on books where there was no metadata fetched during
  initial scan; it disappears if you edit in metadata manually, or
  the comicvine scraper adds it"):** PORT ADDITION (no C#
  counterpart), DERIVED — no persisted flag (a ComicDb.xml field
  would break the byte-parity invariant). `item::metadata_missing`
  (cr-ui/browser/item.rs): file-backed (`file_path` non-empty) +
  present (`!file_is_missing`) + ALL key descriptive fields empty
  (series/title/number/volume(-1)/writer/publisher/summary) — a
  ComicInfo that carried only pages/page-count still reads as "no
  info" (the user's mental model: "has no info"). The tag is a
  small translucent dark chip with a "?" at the top-left of the
  cover (`draw_metadata_tag`), drawn in Thumbnail AND Tile (the
  same condition; Detail has no cover — no tag). It clears with NO
  state to track: any edit that fills a key field hides it — the
  editor commit and the Comic Vine scrape both funnel through those
  fields (`apply_edited`/`set_info`). Probe seam: the draw counts
  tags per frame (`ItemViewState.badge_draws`, reset at every frame
  start — the arrow-zone pattern); accessors
  `ItemView::probe_metadata_badge_draws` +
  `state_grid_metadata_badge_draws`. GATES: the `item.rs` unit test
  `metadata_missing_tracks_the_key_fields` (each key field clears;
  fileless/missing-file books never tag) + `metadatatag_probe`
  (NEW example; isolated-XDG pair): A = exactly ONE tag in a
  2-book grid (a metadataless book + a series-carrying neighbor,
  both real-decodable zips — the badge draws only on a ready thumb,
  so the settle waits the decode), B = the `apply_edited` edit +
  `refresh_after_data_change` clears it (0 tags). 502 tests;
  fmt/clippy green; browserbar + statusbar probes re-run COMPLETE.
  USER TEST = rebuild; books in the grid whose scan found no
  metadata show a small dark "?" chip top-left on the cover
  (Thumbnail and Tile views); edit any key field (series/title/
  number/writer/publisher/summary/volume) in Properties or let the
  Comic Vine scrape fill one → the chip disappears on the next
  refresh. Placement/glyph is negotiable (the user asked for
  "subtle"; current: 12-20 px chip, alpha 0.62).
- **SCAN/OPEN METADATA IMPORT FIXED — THE INFO CHAIN NOW READS AT
  CREATE (2026-09-11; user report: "imported magazines have no info,
  yet they have a ComicInfo.xml"):** ROOT CAUSE — the port's scan
  NEVER read ComicInfo.xml: `create_book` (scanner.rs) called only
  `refresh_file_info` (size/times/page count), while the C# scan calls
  `AddedTime = now; RefreshInfoFromFile()` for every NEW book
  (ComicScanner.cs:219-225) — `RefreshInfoFromFile` (ComicBook
  .cs:2442-2500) loads the FULL info chain (stored xattrs → sidecar →
  in-archive ComicInfo.xml, MetronInfo.xml mapped, method `Complete`
  for a fresh book) merges it `SetInfo(ci, onlyUpdateEmpty: true)`,
  then the ComicBook.xml copy (`cb.SetInfo(ci, onlyUpdateEmpty: false)`
  + `SetBook(cb)`, skipped when `IgnoreEmbeddedComicBookXml`), then the
  page count. The only `load_info` caller in the app was the Folders
  view preview. FIXES: (1) cr-core `ComicBook::set_book` — the
  `SetBook`+`CopyFrom` port (ComicBook.cs:2669/2023): the
  file-determined fields (path/size/times) are protected, the library
  list id kept when the file carries none (`LastOpenedFromListId
  Specified` parity = empty Guid), `Id` NOT copied (the C# `Id`
  property setter is a decompiled no-op, ComicBook.cs:259 — the
  `CopyFrom` assignment never moves); (2) cr-engine
  `apply_info_chain(book, provider)` (+ a gated test seam) — the
  `LoadInfo`/`SetInfo`/`LoadBook`/`SetBook` slice, called by
  `create_book` with ONE provider open serving the chain and the page
  count (the provider count wins over the stored PageCount for a fresh
  book — the C# `needsPageCountRefresh` shape); (3) cr-ui
  `open_book`'s AddToLibraryOnOpen branch now routes through
  `create_book` (the C# `ComicBookFactory.Create(file, AddToStorage)`
  parity) and the reader's TEMPORARY-book branch calls
  `apply_info_chain` against the provider already open for the page
  merge (ComicBookFactory.cs:95) — Properties on a non-library comic
  shows its metadata now. Existing stored books are NOT re-read on
  rescan (C# parity — the scan skips them; the user declined a
  backfill: "don't do anything"); the `force_refresh_info` scan stub
  stays a recorded deviation (every caller passes false). The engine
  config gate `IgnoreEmbeddedComicBookXml` is honored
  (`apply_info_chain_gated`). GATES: cr-engine
  `apply_info_chain_gates_comic_book_xml` (the gate on/off), scanner_lib
  `scan_imports_comic_info_metadata` + `scan_comic_info_wins_over_
  comic_book_xml` + `scan_maps_metron_info_metadata`; scanrefresh_probe
  grew K (the "Scanned A" fixture carries an embedded ComicInfo.xml and
  the landed book carries its series — read in a SCOPED borrow: the
  Ref guard must be gone before gate_c dispatches the next scan, the
  session borrow_mut would panic otherwise). LESSONS: the port named
  the C# `InfoLoadingMethod.Complete` as `Slow` (same semantics —
  in-archive preferred); a ComicInfo with `<Pages>` but no
  `<PageCount>` drops its page list in the C# `SetInfo` too (the
  whole pages block gates on `ci.PageCount != 0`, ComicInfo.cs:1380) —
  fixtures must carry a PageCount. 501 tests; fmt/clippy green;
  scanrefresh A-K green. USER TEST = rebuild, rescan a folder
  containing magazines WITH ComicInfo.xml files — NEW files (and files
  added to the library via open) carry series/title/writer/page
  metadata; the ALREADY-imported empty books stay empty (re-import
  them if wanted); Properties on a non-library comic shows its
  metadata too.
- **SCAN-LIVENESS FIXED — THE SCANNER CLONES THE STORAGE, NOT A TAKE
  (2026-09-11, ADR-032; user report: "search hits blank mid-scan and
  stay blank — re-searching finds nothing, smart lists too, restart
  restores"; the user scanned via File ▸ Scan Book Folders over a
  watch folder, interrupting and resuming):** ROOT CAUSE —
  `start_scan_worker` did `std::mem::take` on the DB books, and a
  RE-scan fires `on_new` only for NEW files (scanner.rs
  process_file's already-stored branch), so ZERO batches flowed and
  the database held 0 books for the ENTIRE scan. Every evaluation
  read 0: the watch-poll landings' `refresh_after_data_change` →
  navigator refill → the 200 ms debounced select → `set_books(0)`
  (the blank — it fires AFTER the same-tick pop of the next queued
  leg re-took the storage), smart lists, F5, any list switch. The
  search itself is view-side only (rebuild_filter → ViewState), so
  re-searching filtered an already-emptied book set. File ▸ Scan
  Book Folders queues one scan PER watch root and each landing pops
  the next in the same pump tick, so the DB stayed empty through the
  whole sequence; the 1 s watch poll stacked another rescan per
  second on top. Restart fixed it because `save_if_dirty` skips
  mid-scan and the exit abort merged the worker's full storage.
  FIXES (user approved the ADR-019-shape change): (1) the worker
  scans a CLONE (`db.books.clone()`); the DB keeps the full library
  and grows by the batch appends; the landing runs
  `merge_scan_storage` (pure, 6 unit tests): the worker's storage is
  master (scanned file-info updates + new books), database-only
  books stay (mid-scan adds), TOUCHED ids keep the DB copy
  (`record_scan_touch` at every main-thread mutation site:
  apply_edited, open_book, record_page_change, the cache-events
  pump, the Info Writer pump, the export surgery), REMOVED ids drop
  everywhere (`record_scan_removal` at remove_book, the delete-files
  retain in shell.rs, the export remove_by_path); (2) the per-tick
  hook no longer falls back to a full refresh for non-Library views
  (a per-100 ms-tick re-evaluation was a refresh storm; the landing
  hook does the one refresh); (3) `take_watch_folder_rescans` holds
  the pending roots while a scan runs. GATES: scanrefresh_probe grew
  G (a mid-RE-scan grid + a Never-Read evaluation + the
  refresh_after_data_change path all see the FULL stored set — fails
  at 0 on the take), H (the rescan is HELD mid-scan, delivers after;
  the probe now registers watch folders through the REAL
  `add_watch_folder` — a direct DB push builds no watcher and the
  events never existed), I (a non-Library view holds its count
  mid-scan, stepping only at leg landings — intermediate batch-sized
  values = the churn), J (a mid-scan remove stays removed and a
  mid-scan edit keeps the edit after the merge); A-F re-run green
  (D: abort kept 2376 of 10002; F's trigger now rides the LIVE count
  base — the earlier gates grow the library past the old fixed
  threshold). Probe LESSON: `win.scan-folders` runs one scan leg per
  watch root and EVERY leg landing fires the hook's `&[]` full
  refresh of the current view — a mid-scan count assertion must
  allow the post-landing step, not only the pre-scan value. 497
  tests; fmt/clippy green. USER TEST = rebuild, search "Action
  Comics", scan the folder (and interrupt/resume it) — the hits stay
  visible the whole time; smart lists and re-searching keep working
  mid-scan; removing/editing a book during a scan survives it.
- **CHOOSER SUBMENUS + SMART-LIST RULE MENU + DETAIL GRID LINES
  (2026-09-10, follow-up round; user report: "column picker doesn't
  work — the sub-menus (All, A-B, C-F etc.) are all empty"; plus
  "add a thin vertical line per column going up through the header"
  and "the smart-list rule-type pick list should be built like the
  column picker"):**
  (1) EMPTY-SUBMENU ROOT CAUSE, proven at the GTK source
  (gtkmenusectionbox.c): the previous round built submenu placeholders
  as model items with BOTH a `custom` attribute AND a submenu link —
  the insert branch order is separator → SUBMENU → custom, so the
  submenu branch won: the page is built from the LINKED model (empty)
  named by the label, the custom attribute is ignored, and
  `gtk_menu_section_box_add_custom` returns false (the custom-slot
  hash only fills for items WITHOUT a submenu link — custom widgets
  are INLINE slots, never submenu pages). FIX: drop the custom pages
  entirely — the submenus are REAL model submenus carrying the same
  rows as check items bound to the `cols.col<id>` actions; the tall
  "All" page scrolls in the PopoverMenu's own outer scroller (the
  popover child IS a ScrolledWindow wrapping the page stack —
  gtkpopovermenu.c). NEW GATE: `state_column_chooser_page_rows(sub)`
  sets the popover's `visible-submenu` property (READWRITE — drives
  the page stack headlessly) and walks the named stack page counting
  GtkModelButton rows; browserbar_probe D now reads all=92 / a-b=16
  (title button + 91/15 rows) — the empty-page regression fails the
  gate at 1/1.
  (2) DETAIL GRID LINES (user addition; the C# Detail body has NO
  vertical lines — the C# header frame edge is the only line): one
  1 px vertical line per column boundary at the header edge x, from
  y=0 THROUGH the header down to the content bottom, fg @ alpha 0.2,
  painted under the row content (the translucent banding keeps it
  visible; opaque selection covers it). The column boundaries are
  collected in the header strip loop (`x + column.width - 0.5` —
  the same x the C# header frame uses), drawn before the item loop
  so the lines stay under the text.
  (3) SMART-LIST RULE MENU: the matcher row's type combo became the
  C# `btMatcher` shape (MatcherEditor.cs:150 — a left-aligned
  button) opening a PopoverMenu in the
  `Program.CreateComicBookMatchersMenu` shape
  (Program.cs:439-455): one parametered `sm.switch-type::<class>`
  action per row group; items = the `ContextMenuBuilder.Create(20)`
  fill via the SAME `columns::chooser_menu` helper (All =
  alphabetical, letter submenus with the run-merge rule over the 97
  spec descriptions). The C# "Recent" rung (the 5 used matcher
  types, LastTimeUsed) is NOT ported — no usage tracking, and the
  C# hides the submenu while empty. Gate: `smartlistmenu_probe`
  (new; the editor builds rule rows + commits — the menu mechanism
  itself is gated by the chooser D gate).
  491 tests; fmt/clippy green; browserbar (D + the new submenu-row
  gate + D2/D3), statusbar (A-J2), workspace, commands (73/73),
  menubar, navpages, detailresize probes green. USER TEST = the
  chooser submenus (All + A-B/C-F/G-O/P-R/S/T-Y) now list their
  columns and toggle from any row; the Detail view carries the thin
  vertical column lines through header and rows; the smart-list
  editor's rule rows pick the type from the All/letter menu (the
  button shows the current description; switching keeps the values).
- **DETAIL VIEW MATCHED TO CR (2026-09-10; user batch: "text slightly
  too small, rows almost twice CR's, alternate white/grey rows,
  column chooser like CR"):** four fixes, all C#-source derived.
  (1) FONT: the Detail cells + header strip + auto-size measure drew
  at Sans 12; the C# item font is `SystemFonts.IconTitleFont`
  (`ComicBrowserControl.cs:859`) ≈ Sans 13 on the Linux stack —
  `DETAIL_FONT_SIZE = 13.0` in one const, used by the cell draw, the
  header strip, and `AutoSizeHeader`. The cell text is now CENTERED
  per cell (`LineAlignment.Center`), aligned Near/Far/Center with
  widths MEASURED through `text_extents` (the old 6.6/char estimate
  drifted), 2 px inset (`Inflate(-2, 0)`).
  (2) ROW HEIGHT: `DEFAULT_ROW_HEIGHT` 16 → 21 = the C# boot formula
  `ItemRowHeight = Font.Height + ScaleDpiY(6)`
  (ComicBrowserControl.cs:861); `DEFAULT_HEADER_HEIGHT` → 21
  (`ColumnHeaderHeight = ItemRowHeight`, :862). The workspace
  restore now carries the C# apply guard (`value.ItemRowHeight >=
  8`, ItemView.cs:1596) and the cr-core workspace default is 0 =
  unset (keeps the boot default). USER-REPORT ROOT CAUSE, measured:
  the OLD T8 `sync_slider` set the slider range UNGUARDED — the GTK
  clamp fired `value_changed` and ANY mode switch slammed the row
  height to the 48 ceiling (fixed in d5f53b8, but every close after
  such a switch SAVED 48; this machine's Config.xml carries
  `<ItemRowHeight>48</ItemRowHeight>`). C# parity keeps restoring
  the stored value, so the user must drag the status-bar slider
  (Detail range 12..48) or Ctrl+wheel down ONCE; later saves
  persist the choice.
  (3) BANDING: `ItemRect.group_row` (the row index within its
  group — the C# `ItemDrawInformation.GroupItem` restarts per
  group) and the Detail draw paints every non-selected even row
  with the C# `ThemeColors.DetailView.RowHighlight` =
  `Color.LightGray` at alpha 96 over the window base (dark theme:
  RGB 72,72,72 per DarkThemeColorTable; base luminance picks the
  target). The band spans the full client width
  (`GetItemBounds` = (clientWidth, ItemRowHeight)), not just the
  column strip.
  (4) COLUMN CHOOSER = the CR `ContextMenuBuilder.Create(20)` shape
  (`ItemView.CreateHeaderMenu`): the 13 default-visible columns as
  top-level check rows (registration order), then "All" (every
  column, alphabetical — the user's explicit spec; the C# orders it
  by registration), then letter submenus with the C# run-merge rule
  (a run stays open while its size + the next bucket stays under
  20) → `A-B, C-F, G-O, P-R, S, T-Y` over the full table (gated by
  the `chooser_menu_matches_the_c_sharp_letter_groups` unit test).
  The chooser is now a `PopoverMenu` (one surface, submenus swap
  inside it — Wayland-safe) whose big pages are CUSTOM widgets via
  `add_child` (the GTK contract: a model item with a `custom`
  attribute + an empty `submenu` link names the page) because model
  pages cannot scroll; the pages are scrollers of CheckButton rows.
  Per-column checkmarks ride one stateful `cols.col<id>` action per
  column (states refreshed at every open so `win.toggle-column`
  toggles between opens stay honest). The C# "Recent" submenu is
  NOT ported (no LastTimeVisible tracking; the C# hides it while
  empty). The column TABLE is now the full C# registration list
  (91 entries, ComicBrowserControl.cs:755-843): Web, Linked, the
  Book Price/Age/Store/Owner/Condition/Collection
  Status/Location family, ISBN, Proposed Values, Gap Information,
  Icons, Scan Information, Main Character/Team, Review,
  Published (Regional), the 14 "Series:" stats columns, Actual
  File Format (slow) — plus the property fixes (B&W →
  BlackAndWhiteAsText, Manga → MangaAsText, Series complete
  (lowercase, as in the C#) → SeriesCompleteAsText, Read →
  HasBeenReadAsText). New cell-text arms in
  `display_text::column_text` (`GetYesNoAsText` shape: Unknown →
  empty; BookPriceAsText `{:.2}`/Unknown; ActualFileFormat through
  `cr_io::formats::source_format`) and the "Series:" columns draw
  LIVE against a lazily built per-series table
  (`cr_engine::matcher::series::create` over the view's books —
  the `ComicBooknistics.Create` shape; built before the draw loop,
  cleared in set_books + append_books; the
  `ComicBookSeriesStatistics.*AsText` formats incl. "N Page(s)",
  "{n}%", "None" for gaps). Gap Information/Icons stay textless
  (the C# draws images — recorded deviation; `is_text_column`
  excludes ids 0/60/62/101). 491 tests; fmt/clippy green;
  browserbar (D chooser-open+height=600+toggle, D2+D3 grouping),
  statusbar (A-J2), workspace, commands (73/73), menubar probes
  green. USER TEST = switch the browser to Detail: the text size
  and row rhythm match CR once the saved 48 is dragged off the
  slider (see (2)); rows alternate grey/white starting grey, the
  selected row keeps the highlight; right-click the column header:
  the 13 defaults, All (alphabetical), then A-B/C-F/G-O/P-R/S/T-Y;
  enable "Series: Books" — the per-series counts fill.
- **GROUP-HEADER ARROW = VECTOR TRIANGLE (2026-09-10, commit
  d8e83ed; user ask: "make the icon a triangle pointing right when
  collapsed; a click rotates it 90° to point down and the group
  expands"):** the arrow was a font glyph (U+25B8/U+25BE) — renders
  inconsistently across systems. Now a filled cairo triangle (12 px,
  vertically centered, the C# `groupCollapsedImage`/
  `groupExpandedImage` bitmaps as vector geometry): RIGHT = collapsed,
  DOWN = expanded; the single arrow click toggles (the existing
  behavior — the draw state was the only change). The hit zone is
  the triangle square + slack at full header height. 489 tests;
  fmt/clippy green; browserbar D2+D3 green (the probe reads the
  recorded zone — unaffected by the draw change).
- **GROUP COLLAPSE: COUNTS + DOUBLE-CLICK DIRECTION FIXED
  (2026-09-10, commit 6b4016d; user report: "when I collapse grouped
  by series, all series have 0 titles, and I can't expand just one
  of them"):** two bugs. (1) The header count read
  `group.items.len()`, and the rebuild EMPTIES a collapsed group's
  items — every collapsed header showed "(0)". The C# header keeps
  `Items` attached while collapsed
  (`GroupHeaderInformation.ItemCount`); the port now carries
  `Group.count` (the TRUE bucket size, set on EVERY rebuild) and the
  draw shows it. (2) The double-click direction was INVERTED: the C#
  fires the single-click toggle on BOTH MouseUps before the
  DoubleClick event, so the clicked header is back at its ORIGINAL
  state when the all-toggle reads it — net: every group takes the
  OPPOSITE of the clicked header's original state (double-click a
  collapsed arrow = expand ALL; the user's double-clicks collapsed
  everything). The port fires the toggle ONCE (press n=1), so the
  n=2 all-toggle must apply the POST-first-click state directly
  (`set_all_collapsed(collapsed)`), not its negation. GATE:
  browserbar_probe D3 is now a press-sequence MACHINE (one press per
  150 ms tick — the draw between presses re-records the arrow
  zones; a one-shot timer installs the repeating machine, a
  repeating timer with a long interval does NOT start it early):
  label select, single-click collapse AND expand of one group, both
  double-click directions, counts surviving a collapse-all;
  view_state unit tests pin the counts. 489 tests; fmt/clippy
  green; statusbar (J/J2), commands, scanrefresh (A-F) green.
- **GROUP-HEADER CRASH FIXED + GROUPER PERSISTENCE (2026-09-10,
  commit 9606815; user report: "when I grouped by series it
  segfaulted" — `RefCell already borrowed` at item_view.rs:1068 in
  the pressed trampoline → `panic in a function that cannot unwind`
  → SIGABRT):** two bugs. (1) The crash — the header hit read
  `state.borrow()` inside the `if let` SCRUTINEE ARGUMENT
  (`if let Some(group) = hit_group_header(&state.borrow().layout,
  ...)`), so the borrow lived to the END of the if/else (the
  edition-2021 temporaries lesson) and the branch's `borrow_mut`
  panicked; the recorded sweep regex (`if (self|view)\.state\.borrow`)
  does NOT match this shape — the borrow hides in a scrutinee call
  ARGUMENT. FIX: both press paths (single + double click) now run
  through `ItemView::handle_group_header_press` with every borrow
  hoisted into `let` blocks; the gesture routes through it. NEW
  PROBE SEAM: `probe_group_press(n, x, y)` + `probe_group_arrow_zone`
  drive the REAL press paths headlessly — browserbar_probe D3 gates
  the label select (expanded), the arrow toggle, the n=2
  expand-all-no-panic, and toggle-groups (the arrow zone is recorded
  by the DRAW — the gate settles one paint cycle before reading it;
  a zero zone = inconclusive). (2) FOUND THROUGH THE GATE:
  `ItemView::set_books` carried only the FILTER across the book-set
  swap and silently dropped the GROUPER — every list refresh (scan
  landing, duplicate, rating commit) ungrouped the view (the C#
  keeps `ItemGrouper` on the ItemView across `FillBookList`); the
  port now re-applies it like the filter. Recorded deviation: the
  C# selects a COLLAPSED group's items on a label click (its headers
  keep the items attached — `GroupHeaderInformation.Items`); the
  port's collapsed groups drop their items
  (`ViewState::rebuild` pushes `items: Vec::new()`), so that select
  is a no-op — a model-level difference left as-is (a restructure to
  C#-shape = collapsed items in display_order with layout-skipped
  placement). 489 tests; fmt/clippy green; browserbar (D2+D3),
  statusbar (J/J2), commands, scanrefresh (A-F) probes green.
- **GROUP HEADERS + SCAN LAMP + THUMB OPTION (2026-09-10, commit
  fc748a7; user batch: group headers/collapse in all views, the
  animated scan lamp with Cancel scan, the thumbnail question, an
  on-demand off switch), user test PENDING:** (1) GROUPING — the
  port NEVER rendered group headers (nothing set
  `LayoutConfig.groups_visible`; the Group menu bucketed items but
  showed no header strips). Fixed to the C# shape:
  `layout::groups_visible(config, view)` gates on the view's
  grouper (the `AreGroupsVisible` port; `IsTopLayout` covers Detail,
  so headers now show in EVERY mode while grouped), the header draw
  RECORDS the arrow zone (`GroupRect.arrow` — the
  `ArrowBounds` port, recorded at draw like the C#), the click
  SPLITS arrow (toggle that group) from label (select ALL the
  group's items — `ViewState::select_group_items`), and a
  DOUBLE-click on the arrow expands/collapses ALL groups (the
  direction = the clicked header's post-first-click state). The
  Views drop gains `miExpandAllGroups` verbatim ("Collapse/Expand
  all Groups", `win.toggle-groups`, enabled iff a grouper is set,
  `ItemView::toggle_all_groups` → `ViewState::toggle_groups` — the
  FIRST group's state decides the direction). Collapse flags stay
  in-memory per view (carried across rebuilds by caption) — the
  per-list DB `GroupsStatus` codec stays dead code (recorded
  deviation). Gate: browserbar_probe D2 (ungrouped (1,0) + disabled
  action, 3 series → toggle collapses/expands all) + view_state
  unit tests. (2) SCAN LAMP — the C# lamp is an animated resx GIF
  (`ScanAnimation.gif`); the port bundled it as 4 coalesced PNGs
  (`cr-ui/assets/scan/frame-N.png`, shipped by BOTH release
  workflows + the deb/arch `kind` loops) animated by a 120 ms timer
  that runs ONLY while the lamp is visible (WinForms animates
  status-label GIFs natively — recorded deviation). The lamp click
  opens a small popover with ONE row, "Cancel scan" →
  `library::abort_scan` (the user's ask; the C# lamp opens Tasks and
  the abort lives in its scan row — the other two lamps keep
  Tasks). Probe gates J/J2 in statusbar_probe: 4 frames, anim runs
  only while visible, the menu maps, the row fires the abort hook.
  PROBE TRAPS: the popover cannot map with a HIDDEN parent (the J
  gate must leave the lamp visible) and the 1 s activity poll
  re-hides an idle scan lamp on the second marks — keep the J2
  reads clear of the whole-second marks. (3) THUMBNAILS — ANSWER:
  they are ON-DEMAND ONLY (first grid draw → `queue_visible_thumbs`
  → the ImagePool thumb queues → the disk cache under the SAME
  `front_cover_thumbnail_key` the grid reads); a scan generates
  NONE, and the backfill command ALREADY EXISTED (File ▸ Generate
  Cover Thumbnails → the unlimited queue; the C#
  `GenerateFrontCoverCache`). NEW: `GenerateThumbnailsOnDemand`
  (PORT ADDITION, no C# counterpart — default true, Config.xml
  key "GenerateThumbnailsOnDemand", Preferences ▸ Advanced ▸
  Thumbnails row): off = `queue_visible_thumbs` loads only covers
  the pool already has (`ImagePool::thumbnail_cached` = memory or
  a header-only disk stat) and leaves placeholders for the rest —
  a later backfill lands on the next draw. 489 tests; fmt/clippy
  green; commands_probe + scanrefresh_probe (A-F) re-run green.
  USER TEST = (a) Group by Series (Group menu) → header strips with
  counts show in Thumbnail/Tile/Details; click the arrow collapses
  that group, click the label selects its books, double-click the
  arrow collapses/expands ALL; the Views menu row does the
  collapse/expand-all and grays out without grouping. (b) Start a
  scan → the status-bar lamp animates; click it → "Cancel scan"
  stops the scan (books found so far stay). (c) Preferences ▸
  Advanced, uncheck "Generate cover thumbnails on demand", OK,
  restart → the grid shows placeholders until File ▸ Generate
  Cover Thumbnails backfills; re-check restores the old behavior.
- **EXIT MID-SCAN FIXED (2026-09-10, user report: "can't quit the
  app while the scan goes on — have to kill it via console; after
  restarting, the library was empty, nothing had been saved"):** two
  bugs. (1) The close-request path ran `library::save()`, which
  WAITED for the running scan to finish (the pump loop) — a long
  scan = a hung window. The C# exit STOPS the scanner first
  (`QueueManager.Dispose` → `Scanner.Dispose` →
  `Stop(clearQueue: true)`, ComicScanner.cs:228-234). FIX:
  `library::save()` now calls `abort_scan()` when a scan is in
  flight before the pump wait — the abort makes the wait a one-tick
  partial merge, and the save writes the books found so far. (2) The
  console kill signals (Ctrl+C / `kill`) terminated the process with
  no save at all. FIX: SIGINT/SIGTERM route through the GRACEFUL
  close — a libc `signal()` handler raises a flag (one store,
  async-signal-safe; the glib crate has no unix-signal binding in
  0.22 — `libc` joined the workspace deps) and a 200 ms main-loop
  poll closes the shell window (the close-request path saves);
  SIGKILL stays uncatchable. (3) FOUND THROUGH THE GATE: the
  scan-folders done callback ran `refresh_view_from_list` AFTER the
  queue popped the next root's scan — that scan had already taken
  the storage, so the refresh evaluated "0 books" and WIPED the
  view between roots ("evaluate 10002 books" immediately followed by
  "evaluate 0 books" in the trace); the done callback now runs
  BEFORE the queue pop. Gate: scanrefresh_probe grew F — a fresh
  5000-book folder starts landing, the window closes mid-add, and
  main verifies the SAVED ComicDb.xml holds the partial (measured:
  close at 10322 books in the grid → the DB file holds 10399 =
  10002 + the 397-book partial; a RE-scan abort instead lands the
  full stored set — the gate's fixture must ADD books). 486 tests;
  fmt/clippy green. USER TEST = start a scan, close the window (or
  Ctrl+C) mid-scan → the app exits promptly and the restart shows
  the books found so far; `kill -9` remains unsaveable (uncatchable).
- **SCAN-GLITCH FIXED — INCREMENTAL APPEND (2026-09-10, user report
  "while it scans it periodically redraws the entire view — looks
  like the app is glitching"):** the progressive-fill hook ran the
  FULL refresh per tick (100 ms) — `set_books` clears EVERY per-book
  cache (thumbs/captions/detail/tile) and rebuilds the whole
  ViewState; at 10k books that is hundreds of ms of recompute per
  tick, 10×/s — the visible glitch. FIX: an incremental append path —
  `ViewState::append_books(batch, filter)` (extend + ONE rebuild) +
  `ItemView::append_books` (the set_books shape WITHOUT the cache
  clears — the C# ItemView inserts items incrementally) + the hook
  payload: `ScanViewHook = Fn(&[ComicBook])`, the pump collects the
  tick's batches and fires ONCE with them, `&[]` = the landing (the
  full refresh); `ShellState::append_scan_batch` takes the append
  path only when the view shows the Library root
  (`library::is_library_list` — smart lists keep the full refresh);
  the selection reselects after the rebuild. MEASURED (release,
  scanrefresh_probe, 10k books): the mid-scan tick is 0.7-4.7 ms
  (`append_books: +N books in T ms` trace) — was a full-rebuild
  storm. 486 tests; fmt/clippy green; the probe gates A-E green.
  USER TEST = scan a big folder on the Library view: books stream
  in WITHOUT the periodic full redraw.
- **PROGRESSIVE SCAN FILL + SCAN ABORT (2026-09-10, user decisions
  "we have to populate as we scan" + "what happens if the scan is
  aborted — will the whole scan need to be re-done?"):** the C# adds
  each scanned book to the LIVE storage (`ComicBookCollection.Add` →
  `OnBookAdded`, ComicBookCollection.cs:37) and its Tasks scan row is
  abortable ("Abort Scanning" → `Scanner.Stop(clearQueue: true)`,
  QueueManager.cs:702-708; the volatile `abortScanning` checks per
  walked file, ComicScanner.cs:130, and the abort SKIPS the
  AutoRemove pass). The port now: `scan_sync_with_progress` gained
  `stop: &dyn Fn() -> bool` (per-file, the walk STOPS — the bool
  return propagates up the recursion) and `on_new:
  &mut dyn FnMut(&ComicBook)` (per new book); the cr-ui worker ships
  20-book batches over the same mpsc channel (enum `ScanWorkerMsg`),
  the 100 ms pump appends them to the DB and fires the ONE view
  refresh per tick through `SCAN_VIEW_HOOK` (the shell installs it at
  create, a Weak capture; a PER-BATCH refresh is O(N²) per tick —
  114 refreshes starved the main loop at 10k books, measured, fixed
  to one-per-drain); `library::abort_scan()` drops the queued
  requests + flags the worker (the Tasks "Abort Scanning" row + the
  abort-all handler). ANSWER to the re-do question: NO — the books
  found so far are IN the library; a re-scan refreshes stored paths
  cheaply and only adds the rest. Gate: scanrefresh_probe grew
  D/E (D: the grid fills mid-scan at 10k fake books then Abort
  Scanning keeps 2300 of 10002; E: the re-scan completes at 10002,
  the trace shows "added 7702 updated 2298" — the stored books just
  refreshed); cr-engine `scan_stops_when_flagged_and_keeps_partial`
  (the AutoRemove pass is SKIPPED on abort); the T13 test
  `snapshot_lists_queues_in_csharp_order` flipped its scan-row
  expectation to `Some(ABORT_SCAN)` (the C# parity — it pinned the
  old not-abortable deviation). 486 tests; fmt/clippy green. USER
  TEST = rebuild, scan a folder → books appear WHILE it walks; Tasks
  ▸ Abort Scanning stops it with the found-so-far books kept; a
  re-scan completes the library.
- **FROZEN VERSION MARKER FIXED (2026-09-10, commit 7f4d0c0; user
  alarm "why does it say starting build 0.0.233"):** the binary the
  user ran WAS current (built 2026-09-10 18:54, HEAD 305/306) — only
  the version STRING was stale. ROOT CAUSE: `cr-ui/build.rs` emitted
  `cargo:rerun-if-env-changed=VERSION` and nothing else; ANY
  rerun-if-* directive disables cargo's default "rerun when any
  package file changes", so the build script's output (the stamped
  `COMICRUST_VERSION`) was cached since its last full rebuild at
  commit 233 (2026-09-04) and rode along through 70+ commits of
  incremental rebuilds. FIX: `watch_head()` declares the git-dir
  refs as build-script inputs (`git rev-parse --absolute-git-dir`
  → rerun-if-changed on HEAD + packed-refs + refs/heads/<branch>;
  missing files watch fine — a later creation counts as a change).
  Measured: the next build after the fix commit stamps 0.0.307, a
  no-op rebuild keeps it. LESSON: a build script that emits any
  rerun-if-* must declare EVERY input that can move its output —
  including the git state a dev build reads. The marker is now the
  honest "which binary" proof it was built to be.
- **SCAN PROGRESS + LOGGING (2026-09-10, follow-up to the scan-land
  fix; user report: "it says scanning but no progress, the library
  totally empty, as if the app is doing nothing — logging before you
  dream something up"):** three evidence facts. (1) The C# updates
  `Scanner.CurrentLocation` PER WALKED FILE (ComicScanner.cs:125,
  before the File.Exists check) — the Tasks "Scanning" line is
  supposed to move through the walk; the port set the location once
  at start (the root) and never again. (2) The C# walk is a LAZY
  generator (`FileUtility.GetFiles` yield-recursion,
  FileUtility.cs:63) — walk and process interleave; the port walked
  the WHOLE tree eagerly before processing any file (a huge tree =
  a fully silent window). (3) The port had zero logging on the scan
  path. FIX: `scan_sync_with_progress` in `cr-engine/src/scanner.rs`
  — the eager `collect_files` became the lazy `walk_files` (per
  folder: sorted files first riding `progress` + the processor, then
  the subfolders — the same order), the per-file decision extracted
  as `process_file`; the cr-ui Book Scanner worker ships walked paths
  over a second mpsc channel, the 100 ms pump drains it and moves
  `SCAN_LOCATION` (the Tasks line now tracks the walk) and CR_TRACEs
  "scan progress: N files, current '<path>'"; the worker traces
  "scan start '<location>'" and "scan done '<location>' in X ms:
  added/updated/moved/removed". Gate: `progress_fires_per_walked_file`
  (cr-engine) + scanrefresh_probe re-run green; 485 tests. USER TEST
  = run `CR_TRACE=1 comicrust 2>&1 | tee scan.log` from a terminal,
  scan the folder, then either the Tasks line visibly advances or
  the scan.log tail pins WHERE it sits (a stalled "current '<file>'"
  = the suspect file; no progress lines at all = the walk/mount is
  the slow part; "scan done" + a landing = a different bug).
- **SCAN-LAND VIEW REFRESH FIXED (2026-09-10, commit 2fe9b23; user
  report: fresh config + a watch folder + File ▸ Scan Book Folders →
  the Tasks dialog showed the "Scanning '<path>'" line but the
  Library stayed empty):** ROOT CAUSE — the scan itself ran and
  landed fine; the scan's done callback was EMPTY at both UI entry
  points (`win.scan-folders` in `browser/shell.rs` and the
  watch-folder rescan poll in `app.rs`), and the port's scan shape
  moves the whole book storage to the Book Scanner worker and merges
  it back ONCE at landing (the Phase 4 ADR-019 architecture; the C#
  adds books to the LIVE storage per file — `ComicBookCollection.Add`
  → `OnBookAdded` — so its view fills progressively), so nothing
  ever re-evaluated the view. The Tasks "Scanning" line is correct
  C#-parity behavior (`Scanner.CurrentLocation`) while the worker
  holds the storage. FIX: `win.scan-folders`'s callback now
  `refresh_view_from_list` + `sync_enabled` (the folders-view
  Add-Folder shape, a `Weak<ShellState>` capture), and the watch-poll
  callback calls `BrowserShell::refresh_after_data_change` (its
  trace labels generalized from "path-migration" to "data-change").
  Gate: `scanrefresh_probe` (new; isolated XDG pair; settles 600 ms
  past the boot fill's 200 ms debounced Library evaluation FIRST —
  an immediate dispatch passed against the UNFIXED code through that
  boot race, measured; then A the fresh grid is 0, B the scan lands
  and the grid shows the scanned books, C the re-scan keeps the
  count). Verified BOTH ways: the stashed fix FAILS gate B
  ("the grid shows 0 books — the scan-land refresh did not run"),
  the fix passes. fmt/clippy/484 tests green. USER TEST = fresh
  config, add a watch folder, File ▸ Scan Book Folders → the books
  appear in the Library when the scan finishes (big folders stay
  silent until the landing — the recorded mid-scan deviation; the
  status-bar scan lamp + the Tasks line are the progress signal).
  LESSON: a "did X happen" probe must settle past every boot-time
  deferred fill (the navigator's SELECT_DEBOUNCE_MS = 200 ms) or the
  boot race passes the gate against unfixed code.
- **SCRAPER UX FIX ROUND 3 + EDITOR/PREFS FIXES (2026-09-10, the
  user-reported batch) — USER-TESTED, ALL PASS (same day, "all ok";
  commit e2e9b65; the kickoff record is the FIX ROUND 3 section of
  `docs/phase-12-kickoff.md`):** nine items; fmt/clippy/484 tests
  green; gates `scrapeprefs_probe` (new) + the engine test
  `show_issues_forces_the_issue_dialog` (new). (1) The Comic Vine
  Scraper settings now live as a "Comic Vine Scraper" PAGE of the
  Preferences dialog — `dialogs/scrape_config.rs` gained
  `ScrapeConfigWidgets` (build/collect) shared by the standalone
  config dialog (`win.scrape-config`, still probed by
  scrapeconfig_probe) and the Preferences page (the page loads the
  plugin settings.json on open, collects + saves it on OK, Cancel
  discards); `show_preferences(parent, initial_page, on_ok)` gained
  the initial-page parameter and `open_scrape`'s no-API-key path
  opens Preferences on that page (Some("scraper")) instead of the
  standalone dialog. (2)+(3) The series pick dialog rows are
  left-aligned 4-column grids (Series | Year | Issues | Publisher,
  with a heading row; GtkLabel centers text by default — xalign 0
  everywhere). (4) Both pick dialogs set
  `activate_on_single_click(false)` — a single click only SELECTS;
  the commit is double-click/Enter (the single-click
  row-activated commits was the reported "click immediately
  matches"). (5) `SeriesResult::Show` no longer auto-picks: the
  engine's `choose_issue_ref` skips BOTH shortcut auto-picks when
  the dialog is forced (Show Issues / Confirm Issues) and passes
  the matched issue as the hint; the UI preselects the hint row
  (PickIssueRequest carries it). (6) The book editor's cover shows
  a fileless book's CUSTOM thumbnail now (`queue_cover` queues
  `front_cover_thumbnail_key` for fileless books — the same
  `custom:\\` resource the grid renders; the "Unknown" page label
  stays, C# parity for a book without page metadata). (7) The Plot
  tab Summary box is double height (90 → 180). (8) The context
  menus left-align: new `cr_ui::widgets::menu_item_button`
  (frameless + left-aligned label — GtkButton centers its label)
  used by the book, folder and navigator context menus, the editor
  Pages menu, and the favorites dropdown rows. (9) Properties ▸
  Custom is EDITABLE (the C# `customValuesData` grid): one row per
  library-wide key (the union of all books' custom keys,
  case-insensitive, sorted; dotted script keys hidden unless
  `ShowCustomScriptValues` but kept for the save), the value
  editable, "Add Value…" adds a new-key row; save rebuilds
  `custom_values_store` (non-empty key AND value → set, empty
  value → delete, case-insensitive last-wins — the
  `SetCustomValueInStore` shape); the grid refills per book on
  prev/next (it previously showed only the FIRST book's values —
  fixed); `library::try_session()` added (the editor tolerates the
  session-free probes).
  USER TEST (release build): (a) Scrape with no API key →
  Preferences opens on the Comic Vine Scraper page; set the key, OK,
  scrape works; reopen Preferences ▸ Comic Vine Scraper to confirm
  the settings persist. (b) The series list shows columns and a
  double-click commits. (c) "Show Issues" opens the issue list with
  the number preselected. (d) A scraped fileless book shows its
  cover in Properties (not a black box). (e) The Plot Summary box
  fits long summaries. (f) Right-click menus are left-aligned. (g)
  Properties ▸ Custom edits values (edit one on two books, check
  the value survives prev/next and OK, an emptied value removes the
  key). RESULT: ALL PASS ("all ok", 2026-09-10).
- **WRITE-BACK ON THE INFO WRITER WORKER (2026-09-10, follow-up to
  the export freeze):** `library::update_book_file` used to run
  `ComicProvider::open` + `store_info_scoped` (a full archive
  rewrite; a `7z`/`rar` subprocess for CB7/CBR) + `refresh_file_info`
  inline on the GTK main thread from THREE sites: the "Update Book
  File(s)" context action, the 100 ms debounced editor-commit timer,
  and `update_all_book_files` (one write per loop tick). Now:
  `run_book_file_write` (pure, Send, session-free — also the test
  seam) runs on a persistent "Info Writer" thread with a
  Mutex/Condvar VecDeque queue + dedup by book id (a re-request
  replaces the queued clone — the C# ProcessingQueue semantics); the
  enqueue side (`update_book_file_async(id, always_write, on_done)`)
  keeps the gates + the book clone on the main thread; a 100 ms
  pump applies results (file properties + the full info carry, the
  dirty-flag clear guarded by a PRISTINE info snapshot — the
  post-refresh times always differ, so the guard compares against
  the pre-refresh clone and a re-edited book keeps its dirty flag
  for the editor's re-schedule) and fires the per-book callbacks
  (they stay main-thread — Rc callbacks are not Send; the channel
  ships only the outcome). The menu action collects the batch and
  shows the error dialog + refresh when the LAST write lands (the
  report is asynchronous now — C# queue parity). writeback_probe
  green (edit lands in the archive, flag cleared); fmt/clippy/408
  tests green; user test = edit a property of a CBR/CB7 book and
  run Update Book File(s) (UI stays responsive; the write lands in
  the file; the "Files to update" list clears).
- **EXPORT FREEZE FIXED (2026-09-10, user report: exporting a
  CBR froze the app):** the export dialog's OK handler ran the
  WHOLE conversion inline on the GTK main thread
  (`crates/cr-ui/src/dialogs/export.rs` — decode + re-encode +
  zip write per page; the C# runs it through the background
  QueueManager). Now the `export_book`/`export_books_combined`
  loop runs on a worker thread, progress lands over a std mpsc
  channel drained by a 50 ms `glib::timeout_add_local` pump, and
  `export_post_process` stays on the MAIN thread (it touches the
  library session thread-locals). Per-book stop flag
  (`Arc<AtomicBool>`) preserves the C# break-on-first-error rule;
  the dialog ignores close/cancel while the worker runs (a closed
  dialog cannot cancel the worker; the C# modal progress has no
  cancel path either). fmt/clippy/408 tests green; user test =
  re-run the CBR→CBZ export (window stays responsive, progress
  label updates, result lands as before).
- **CI GROUP-PERF FLAKE FIXED (2026-09-09, commit 9a759fd):** the
  `group_pass_5000_books_stays_scalar_fast` gate failed in CI
  (18.16 s vs the 15 s budget; locally reproduced 13.76 s with a
  COLD cache). Two causes, both fixed. (1) The test was flaky by
  construction: its timed loop paid the FIRST parse of all 5000
  paths, and the recorded 78 µs numbers only happened when the sort
  test's pre-warm filled the shared process-wide `proposed_cached`
  cache first in the same binary — the group gate now pre-warms its
  PropTable like the sort gate ("the gate's subject is the pass,
  not the lazy first-parse"). (2) The real offender underneath:
  `ComicNameInfo` `rx_count` (the OfValues pattern) compiled a
  fancy-regex on EVERY parse — the one non-cached pattern; now
  cached per pattern (`HashMap<String, &'static Regex>`, 32-entry
  cap) in `comic_name_info.rs`. Every first parse in the app pays
  no compile anymore. Measured after: cold group gate 13.76 s →
  425 µs; suite sort 29.8 ms / group 425 µs / dup 987 ms (debug);
  fmt/clippy/400 tests green. LESSON: the view_perf "warm" numbers
  assumed test-order luck inside one binary; any new gate must
  pre-warm what its subject does not measure. Same-day round 2
  (commit after the 26.08 packaging fix): `queues::
  duplicate_add_does_not_requeue` flaked on CI (left 2, right 1) —
  a no-op queue callback can claim, run, and FINISH between two
  adds, making the re-add a legitimate second run; the queue
  semantics were CORRECT (C# parity, dedup covers pending AND
  running items, queue.rs:6-13). The test now pins item 7 pending
  behind a barrier blocker (the file's established shape) and also
  asserts the AddToTop move deterministically; 10 local runs green.
- **PHASE 10 ACTIVE (2026-09-09) — CBR/RAR write-back** (the kickoff
  is `docs/phase-10-kickoff.md`, decision ADR-030): T1-T4 ALL
  IMPLEMENTED + GATED (commits cbbd689, 9eb9df1; fmt/clippy/400
  tests green), BOTH USER TESTS PENDING — steps 1-4 (rar
  write-back) and steps 5-8 (the export "Replace source" surgery)
  at the end of the kickoff. T1-T3 = the rar writer: `cr-io` gained
  `rar.rs` (`find_rar` = `CR_RAR` env then PATH, never `unrar`;
  `add_files` = one `rar a -y` with cwd at the staging dir so entries
  land root-level bare-named, stdin null) and the
  `store_info_scoped` CBR/RAR5 branch (one `rar a` call for the
  ComicInfo.xml/ComicBook.xml pairs, `with_book_info` scoping as
  CB7;   success reports changed). The app path
  (`update_book_file_async` → the Info Writer worker) surfaces
  "rar executable not found" through
  the existing Update-Book-Files error dialog; the queue path swallows
  (book stays in Files-to-update). `supports_update` stays FALSE for
  RAR (C# parity; only consumers are the write gate +
  `ComicProvider::store_info`, no UI gates). MEASURED FACTS: rar 7.x
  has NO `-ma4` (cannot CREATE RAR4) but UPDATES existing RAR4
  archives fine (format preserved, pages intact — proven on a rar
  6.24-built fixture); `-p-` is NOT a rar switch (it ENCRYPTS with
  password "-"; the no-hang guarantee is stdin null — password
  targets fail fast, measured exit 12); the update staging dirs are
  now unique per call (pid alone raced between concurrent updates —
  fixed for BOTH the rar and 7z paths). Gates:
  `cr-io/tests/rar_gated.rs` (the missing-binary error path runs
  everywhere incl. CI; `CR_RAR_TESTS=1` + `rar` + `7z` gates the
  rar5 round-trip; `CBR_RAR4_FIXTURE=<path>` adds the RAR4
  round-trip on a git-ignored real file). The xattr
  (`NtfsInfoStorage`) parity write-back was OFFERED and DECLINED by
  the user — DB stays the master copy without `rar`. USER TEST =
  the 4 steps at the end of `docs/phase-10-kickoff.md`.
  PACKAGING FIX ROUND 1 (2026-09-09, after the first packaging run on
  v0.0.273): the arch job built fine (2m45s vendored offline) but
  package() died on `cp -r` into missing parents — `install -d`
  added before the assets loop (the flatpak manifest got the same
  fix + a single-line loop). The flatpak job died twice: rofiles-fuse
  (`/dev/fuse` absent in the container) and — the deeper measured
  fact — `bwrap: No permissions to create a new namespace` (the
  openh264 apply_extra warning): the runner's docker profile blocks
  namespaces. Fixes: `options: --privileged` on the flatpak job
  container (act_runner honors the GH-spec container.options; if a
  future runner ignores it, the fallback is privileged containers in
  the runner host config) + `--disable-rofiles-fuse` (drops the fuse
  dependency entirely). Both attach scripts gained
  REPLACE-same-name-asset (idempotent re-runs — the first run had
  already attached the source tarball; a strict append would die on
  the re-attach). The packaging files ride the TAG's checkout, so a
  re-run needs the tag moved to the fixed HEAD first. FIX ROUND 2
  (same day, after the v0.0.276 packaging run): arch GREEN (the
  .pkg.tar.zst attached); flatpak still died at the cargo build with
  `bwrap: No permissions to create a new namespace` — the
  workflow-level `options: --privileged` did NOT take effect. The
  act_runner example config states it plainly: "With privileged
  disabled, options that could escape the container (--security-opt,
  --device, --cap-add, --pid, ...) are ignored in a workflow's
  container.options, but keep working here." So the fix is HOST-side:
  in the runner's config.yaml set `container: privileged: true` and
  restart the runner daemon (the runner-level `options:` field also
  accepts security flags; the workflow-level one never will until
  the runner allows them).   The workflow keeps the
  `options: --privileged` line as documented intent. LESSON: any
  future job needing bwrap/kvm/devices hits the same wall — fix it
  in the runner config, not the workflow. FIX ROUND 3 (same day,
  after the runner got `privileged: true`): the sandbox ran but the
  cargo build died — the 24.08 `rust-stable` Sdk extension is
  end-of-life frozen at rustc 1.89 while the locked gtk-rs crates
  need 1.92+. Flathub carries the extension up to runtime 26.08 and
  updates it continuously (rust 1.98.0 on 2026-08-16) — the manifest
  `runtime-version` + the workflow's three install refs moved to
  26.08. The flatpak job builds from the TAG, so every such fix
  needs the tag re-pointed (v0.0.273 → v0.0.276 → v0.0.278; the
  user chose renumber-over-move to keep tag = commit count — the
  record commit itself consumed the 277 count, so the fix landed at
  278). FLATPAK DROPPED + .DEB PIVOT (2026-09-09, user decision
  "seems very cumbersome") — round 3 failed even on 26.08 at the
  cargo build: `Package 'gtk4' not found` — the freedesktop 26.08
  runtime has NO GTK4 (only gtk3; verified through the
  freedesktop-sdk components tree). So flatpak would have needed the
  GNOME runtime + new extension wiring; instead `packaging/flatpak/`
  is deleted, the workflow's flatpak job became the `deb` job (same
  comicrust-ci image, fetch+verify tarball → extract → offline build
  → `packaging/deb/build.sh` (hand-rolled dpkg-deb --build, zero new
  tooling; control has libc6 (>= 2.41) + libgtk-4-1 floors, postinst
  refreshes desktop-db/icon-cache) → dpkg-deb info/contents sanity →
  attach `comicrust_<v>-1_amd64.deb` + sha). The deb targets Debian
  13 / glibc-2.41 derivatives; older distros use the tarball.
  DEB JOB FIX ROUNDS (2026-09-09, both mine, both in the workflow's
  "Build the package" step): (1) `dpkg-deb --contents | grep -q` —
  grep exits at the first match, closes the pipe, dpkg-deb dies on
  the SIGPIPE ("tar subprocess was killed by signal"); capture the
  listing first. (2) The step built the deb into the extracted
  /tmp/comicrust tree but sha256sum'd a bare name after cd-ing back
  to the workspace — absolute paths everywhere; the step body was
  DRY-RUN locally with stub dpkg-deb/cargo before pushing (the
  lesson: prove CI step bodies with a stubbed end-to-end run, not by
  re-dispatching). THE TAG CONVENTION (user-enforced, cost two
  renumbers): tag = commit count = LATEST MAIN, all three at once;
  any commit after tagging (even a workflow-only fix) breaks
  "tag = latest main", so the user renumbers — v0.0.273 → 276 → 278
  → 281 → 282 → 283. GOING FORWARD: before re-running Packaging,
  always re-point the tag to current main (renumber if the count
  moved); dispatches read workflow files from MAIN, not the tag.
  PHASE 11 PIPELINE COMPLETE (2026-09-09): the v0.0.283 release
  carries the full 9-asset set on BOTH hosts (binary tarball + sha,
  source tarball + sha, arch .pkg.tar.zst + sha + sed'd PKGBUILD,
  .deb + sha). USER TEST PENDING = the kickoff's 5 install steps
  (Arch makepkg/pacman flow, deb apt flow, the launch + asset
  checks, the portable-tarball regression). Open gaps: the LICENSE
  file (placeholders in PKGBUILD/metainfo), placeholder icon art,
  AUR submission (manual), RPM.
  T4 IMPLEMENTED + TESTED (2026-09-09, user test pending) — the
  export post-processing ("convert rar→zip" request): the port of
  `QueueManager.ExportComic` lines 455-508. cr-core gained
  `ComicInfo::set_info` (ComicInfo.cs:1210 field-by-field port,
  per-type empty rules) + `ComicBook::set_info` (the
  ComicBook.cs:2662 page clamps); cr-io `export_book`/
  `export_books_combined` now RETURN the output path and
  `build_export_info` is pub; cr-ui `library::export_post_process`
  (+ `_with` trash-injectable) does replace-source re-point (write
  back BEFORE the by-path source removal, or the removal eats the
  key book — the C# order is load-bearing) + refresh_file_info_basic
  + info set-back + FromComic color-reset + the wasReplaced dirty
  rule, delete-original, add-to-library; a failed trash skips only
  that source's removal (data-safe, the ShellFile.DeleteFile throw
  shape). Dialog: surgery runs per group in the OK path,
  "Add to library" disables on Replace-source
  (ExportComicsDialog.cs:204). Gate: `cr-ui/tests/export_surgery.rs`
  (isolated XDG + FAKE trash — `gio trash` REFUSES tmpfs/system
  mounts, so desktop trash behavior stays user-test territory).
  Recorded deviations: no pre-export `RefreshInfoFromFile` pass, no
  `FileIsInDatabase` duplicate-target guard. User test = steps 5-8
  in the kickoff.
- **PHASE 11 ACTIVE (2026-09-09) — PACKAGING (Arch + Flatpak)** (the
  kickoff is `docs/phase-11-kickoff.md`; re-homed from the deferred
  Phase 8 T8; user shapes: in-repo PKGBUILD + CI artifact, self-hosted
  .flatpak bundle, manual dispatch on a tag; .deb/RPM/AUR-publish/
  Flathub stay in the backlog). ALL FOUR TASKS IMPLEMENTED
  (fmt/clippy/408 tests green), CI-first validation — no local
  makepkg/flatpak on this machine, the containers + the user test
  decide. T1: `cr-ui/src/assets.rs` — the shared asset-lookup roots
  (CWD pair first = old behavior, then $XDG_DATA_HOME, then
  exe-relative `../share/comicrust/assets` — covers /usr/bin AND
  flatpak /app/bin, then $XDG_DATA_DIRS); `icon.rs` +
  `page_view.rs` texture roots rewired onto it (+8 unit tests, the
  env mapping is a pure function — NO env mutation in tests).
  T1b: `packaging/` — the desktop file, metainfo, hicolor icons
  (PLACEHOLDER art from `packaging/gen_placeholder_icon.py`, replace
  when real art exists). T2: `packaging/arch/PKGBUILD` — builds from
  the release source tarball (vendored = offline; `VERSION=$pkgver`
  stamps the About version), installs /usr/bin/comicrust +
  /usr/share/comicrust/assets + desktop integration; license + the
  metainfo project_license are PLACEHOLDERS (no LICENSE file in the
  repo — open user decision). T3:
  `packaging/flatpak/io.github.ScuttleSE.comicrust.yml` —
  freedesktop 24.08 + the rust-stable Sdk extension; the source block
  carries url+sha placeholders that CI seds to a local `path:` (a
  release asset URL may need auth — flatpak-builder's fetch has
  none); `branch:` must match the build-bundle arg. T4: the attach
  scripts (`.gitea/attach_release_assets.sh` +
  `attach_github_release_assets.sh` — APPEND-only; the publish
  scripts DELETE+recreate the release and would drop assets) +
  `.gitea/workflows/packaging.yaml` (dispatch on tag; checks → source
  → arch in `archlinux:base-devel` ∥ flatpak in comicrust-ci). DESIGN
  NOTES: the source job attaches the vendored tarball to the release
  and arch/flatpak download it BACK through the release API (sha
  verified) — no upload/download-artifact JS actions needed (the arch
  container gets nodejs via pacman BEFORE actions/checkout — the
  comicrust-ci node lesson); makepkg runs as a non-root user
  (`useradd -m -s /bin/bash builder`, su); `pacman -Syu nodejs rust
  gtk4 jq` (Arch does not split dev packages — gtk4 carries the
  headers). USER TEST = the 5 steps at the end of the kickoff
  (needs a tagged release + a workflow run).
- **PHASE 12 COMPLETE, USER-TESTED (2026-09-10) — NATIVE
  MODULES I: COMIC VINE SCRAPER** (the kickoff is
  `docs/phase-12-kickoff.md`; the modularity pattern is ADR-031: one
  crate per plugin, engine on a worker thread behind a message
  protocol, plugin-local config, enumerated minimal base
  touchpoints). User decisions locked (2026-09-10): C#-parity wizard
  flow; plugin-local `settings.json`; no legacy profile import;
  context menu + toolbar button only; fileless-book covers DISPLAY
  (the T8 `type://` render path is the one base touch);
  `format=json` + ureq. ALL TASKS T1-T9 IMPLEMENTED (fmt/clippy/481
  tests green; commits 8aa5196, 6fd1d54, 0cedd54, 435edd5, 7664c26,
  2a4a2f4, 88d85c5, 4155208):
  - T1: `crates/cr-scrape` (deps cr-core, cr-image, fancy-regex,
    regex, serde, serde_json, ureq; Apache-2.0 attribution header);
    `fnameparser.rs` — the full `extract()`/`regex()` port gated by
    the plugin's own 218-case `test_fnameparser.data` (218/218;
    the real Python module was validated under CPython with
    clr/log/utils stubs first, `/tmp/opencode/cvspy/` — the data
    file IS the spec). `config.rs`: `Configuration` (~33 basic
    fields, C# defaults, serde JSON with the C# key names —
    `updateNotes`/`updateTags` are the persisted names of
    rescrape_notes/tags), the advanced-settings string parser (16
    KEY=VALUE forms incl. SCRAPE_DELAY clamp 2..3600 with default 1,
    PUBLISHER_ALIAS/IMPRINT arrow parsing, ALT_SEARCH_REGEX
    compile-check), load/save into ONE settings.json under
    `~/.config/comicrust/plugins/comic-vine-scraper`; `advanced` is
    private + `advanced()` accessor — serde skips it and both
    mutation paths reparse.
  - T2-T9: `cv/` (models with key-only Eq/Hash ref sets, the 1100 ms
    throttled + retried client, the four endpoints in `format=json`,
    the 4050-/4000- URL decode, the cvinfo magic file, search-term
    cleanup, the verbatim imprints table); `bookdata.rs` (the read
    side with Shadow values + the filename fallback, the full
    update() massage rules, key tags + CVDBSKIP + the Scraped-
    metadata Notes line, publisher/imprint conversions); `matching/`
    (8x8 average hash, MatchScore, filters, the automatcher with the
    trade-paperback bail-out); the engine loop (BookStatus, fast
    rescrape, skip tag, cvinfo file, delayed requeue, ScrapeUi
    protocol); the config dialog (`scrapeconfig_probe`); the wizard
    (`dialogs/scrape.rs`: the non-modal status window, worker
    thread, modal search/series/issue dialogs over channels,
    per-book `apply_edited` commit, prior_series.json persistence,
    `win.scrape-books`/`win.scrape-config`, the context-menu item,
    the toolbar button with the bundled plugin icon); fileless-book
    custom thumbnails (the pool's custom-thumb dir +
    AddCustomThumbnail parity + the Resource/'custom' render branch;
    `front_cover_thumbnail_key` keys through the C# GetThumbnailKey
    ordering; the item view keys through it now; the ThumbInstaller
    seam keeps the engine pool-free). Gates: 218/218 fnameparser
    vectors, 10 mock-server CV tests, 23 bookdata tests, 9 matching
    tests, 6 engine tests, 2 pool tests, scrapeconfig_probe,
    scrape_probe.
  FIX ROUND 1 (2026-09-10, commit e42eb40, user report "no results
  when I try to scrape, API key entered"): ROOT CAUSE measured
  against the live API — the port requests format=json and
  ComicVine's JSON carries the results list as a FLAT array (the
  invalid-key response itself shows {"status_code":100,
  "results":[]}); the C# fetched format=xml whose dom wraps the
  elements (results.volume / results.issue), and my mock fixtures
  encoded THAT wrapper shape — so the port parsed every real
  search/issues response as empty. The mocks passed because they
  reproduced the port's wrong assumption, not the API. FIX:
  result_items() in queries.rs accepts both shapes (the flat array
  is the real form); every fixture now carries the real shape; a
  new cv_mock test pins the XML-wrapper tolerance. LOGGING (the
  user's ask): crates/cr-scrape/src/log.rs — CR_SCRAPE_DEBUG=1
  prints every GET (the api_key redacted), HTTP + API status codes,
  byte counts, and the engine's decision points; ScrapeUi::error
  surfaces query failures in the wizard's progress line instead of
  dying into Unscraped/Delayed. 482 tests. USER TEST = rerun the
  scrape (CR_SCRAPE_DEBUG=1 shows every query); LESSON: when a port
  changes the wire format (xml -> json), re-derive the fixture
  shapes from the LIVE API, never from the C# dom shapes.
  FIX ROUND 2 (2026-09-10, commit 3cd8051, user report "clicked the
  correct series, then nothing" — the log ended at '100 series
  found' with no issue fetch): ROOT CAUSE — the pick dialogs only
  committed through their BUTTONS; a row click merely selected it,
  so the engine sat blocked in request_series. FIX:
  connect_row_activated commits the row (double-click/Enter) in
  BOTH dialogs, behind a one-shot finish shared with the button
  paths. SAME REPORT exposed the frozen-status gap: the issue-list
  fetch for a 1000-issue series runs ~11 throttled pages with the
  label stuck — the C# per-page callbacks (callback_function
  (matches, expected) / (ratio)) are now ported
  (SeriesProgressFn/IssueProgressFn) and the status line shows
  'Searching... N results' / 'Loading issues... N%'. CR_SCRAPE_DEBUG
  logs the dialog presentation + every user choice. 482 tests.
  **PHASE 12 USER-TESTED, ALL PASS (2026-09-10, "works" after fix
  round 2)**: the user scraped a book with the real API — the series
  pick dialog listed the matches, the double-click commit flowed
  through the issue fetch to the landing details; the API key entry
  and the settings dialog worked on the first run. The rescrape fast
  path and the fileless cover render are probe-gated (scrape_probe,
  the pool tests) and stay watch-items for daily use.
- **Phase:** NO PHASE IS ACTIVE — the last active phase (12, the
  Comic Vine Scraper) closed USER-TESTED 2026-09-10; the fix-round-3
  batch (the user-reported nine: the Preferences scraper page, the
  series columns + double-click, the forced issue dialog, the
  fileless cover, the Summary height, the left-aligned menus, the
  editable Custom tab) also passed its user test the same day
  ("all ok", commit e2e9b65). Phases 0-8 are COMPLETE
  (every delivered task user-tested; the trail below carries the
  records). Phase 9 (the SQLite backend) is DEFERRED to
  `docs/backlog.md` with its full design intact. Open work is
  picked from `docs/backlog.md` and re-homed into a kickoff FIRST.
  The 2026-09-10 SCAN SESSION (after Phase 12): five fix rounds on
  the scan path, all in the state blocks above — the scan-land view
  refresh (2fe9b23), the per-file progress + CR_TRACE logging + the
  lazy interleaved walk (19ce25b), the frozen version marker (7f4d0c0,
  build.rs watches the git ref), the progressive fill + Abort
  Scanning (778bd44), the no-glitch incremental append (8822981),
  and the graceful exit mid-scan + the signal handling + the
  done-before-pop fix (707950f). The 2026-09-10 GROUP/LAMP/THUMB
  SESSION (immediately after, same day, in the state blocks at the
  top): the group headers + the animated scan lamp + the on-demand
  thumbnail switch batch (fc748a7), the group-header double-click
  RefCell abort + the grouper persistence (9606815), the collapsed
  counts + the double-click direction (6b4016d), the vector
  disclosure triangle (d8e83ed). The user drove three fix rounds
  while testing grouping (the crash, the 0 counts, the inverted
  double-click) — grouping is effectively user-exercised; the lamp
  + thumbnail parts still need the explicit test below. The
  2026-09-10 DETAIL/CHOOSER SESSION (immediately after, same day, in
  the state blocks at the top): the Detail-view CR-matching batch
  (c21086a) and the chooser-submenu fix + grid lines + the
  smart-list rule menu (f20a690). The 2026-09-11 SCAN-LIVENESS session follows (the clone-not-take fix, ADR-032); see its block at the top for the current HEAD.
  Open gaps: WebComicProvider, PDF/DjVu writers, the LICENSE file
  (Phase 11 packaging gap), the T14 per-list sort deviation,
  HEIF/AVIF decode. The Phase 11 PIPELINE is COMPLETE (the v0.0.283
  release carries all 9 assets on both hosts); the install steps
  (the kickoff user test) remain.
  OPEN USER TESTS (2026-09-11, in test order): the SMART-LIST RULE
  DELETE + clipboard ops (rebuild, open a smart list's editor — every
  rule row and group carries a small ▾ button at the right edge with
  the New Rule / New Group / Delete / Cut / Copy / Paste / Move Up /
  Move Down menu and the honest enable states; Delete removes a rule;
  Copy + Paste inserts a clone; a Query round trip stays clean; the
  Cut/Copy/Paste clipboard round trip on a real desktop — Xvfb
  stalls those reads), the "NO METADATA" TAG
  (rebuild; books whose scan found no metadata carry a small dark "?"
  chip top-left on the cover in Thumbnail and Tile; Properties edits
  to a key field or a Comic Vine scrape make it disappear), the
  SCAN/OPEN METADATA
  import fix (rebuild, rescan a folder with ComicInfo.xml-bearing
  magazines — NEW files carry series/title/writer/page metadata; files
  added via open too; Properties on a non-library comic shows its
  metadata; already-imported empty books stay empty — the user
  declined a backfill), the DETAIL-VIEW
  round (c21086a + f20a690: switch the browser to Detail — after ONE
  slider drag the text size and row rhythm match CR (the saved
  ItemRowHeight 48 artifact must be dragged off the slider once, the
  status-bar slider re-ranges 12..48); rows alternate grey/white
  starting grey and the selection keeps the highlight; the thin
  vertical column lines run through the header and the rows;
  right-click the column header — the 13 defaults, All
  (alphabetical), then A-B/C-F/G-O/P-R/S/T-Y, every row toggles from
  every page; the smart-list editor's rule rows pick the type from
  the All/letter menu), the GROUP/LAMP/THUMB
  batch (grouping end-to-end after the fixes: Group by Series in
  Thumbnail/Tile/Details → header strips with TRUE counts,
  single-click the disclosure triangle collapses/expands ONE group,
  double-click collapses/expands ALL, the Views-menu row does the
  collapse/expand-all and grays out without grouping; the scan lamp
  animates while scanning and its click opens the "Cancel scan"
  menu; Preferences ▸ Advanced ▸ Thumbnails off → placeholders until
  File ▸ Generate Cover Thumbnails backfills), the scan round (the
  user confirmed the progressive fill + the no-glitch append "works
  now" on 2026-09-10; still open: Tasks ▸ Abort Scanning on a real
  scan, and the graceful exit mid-scan = close the window / Ctrl+C →
  exits promptly, the restart shows the books found so far), the
  Phase 10 steps 1-4 + 5-8 (the kickoff tail), the Phase 11 install
  steps (the kickoff tail), the export-freeze fix (re-run a CBR→CBZ
  export — responsive window, progress ticks), and the write-back
  fix (edit a CBR/CB7 property + Update Book File(s) — responsive
  UI, write lands, the Files-to-update list clears). The Phase 12
  scrape steps AND the fix-round-3 batch passed their user tests
  (see the Phase 12 block + the SCRAPER UX FIX ROUND 3 block).
- **(Phase 8 — CLOSED 2026-09-08 — the kickoff is
  `docs/phase-8-kickoff.md`; done so far: T3 (the
  CBL-import perf) + T10 first slice (the view-side proposed-parse
  storms), both records in the paragraphs below, both user tests
  PENDING; next in the task order: T1 → T2 → T4 → T5 → T6 → T8 →
  T9 → T11 → the T10 remainder. Phases 0-7 are COMPLETE — all their
  tasks user-tested (the trail below): 6 (native features +
  de-scripting) COMPLETE (2026-09-06); Phase 5.5 (UI chrome
  parity) also COMPLETE — all tasks T0-T14 user-tested (2026-09-06),
  the close-out record lives at the end of
  `docs/phase-5.5-kickoff.md`. The per-task records below stay for
  the fix-round facts.
  **PHASE 6 RE-SCOPED + IMPLEMENTED (2026-09-06, ADR-027): the
  scripting host is DROPPED** — no PyO3/CPython, no plugin hooks,
  no `.crplugin`; the used-script set ports natively instead (the
  decision + the feasibility evidence live in ADR-027 and the
  superseded record in `docs/phase-6-kickoff.md`). Landed: the
  native "New Comic…" fileless flow (`library::insert_new_book` +
  the editor; the C# `MainForm.AddNewBook` parity, idempotent per
  id because editor commits fire per save point), the "New fileless
  Book Series…" dialog (the NewComics.py port: series/volume/range,
  the OK-enable rule, the >100 silent abort, books created +
  selected), the FilelessMarker state icon on fileless covers, the
  reader-open gate for empty-path books (the C# `NavigatorManager
  .Open` `IsLinked` rule — fileless books never open a slot), the
  `Expression`/`User Scripts` matchers registered as
  `MatcherKind::Script` (parse + render byte-stably, evaluate to
  no-match; the `PluginKey` XML attribute round-trips — golden
  fixture), `cr-script` deleted from the workspace, Copy Page
  (`create_page_image` → clipboard PNG) and Export Page (the
  "Save Page as" chooser: the C# 5-format filter, the persisted
  `LastExportPageFilterIndex`, `AddExtension` parity). Probes:
  `newbook_probe` (insert idempotence, the series dialog flow, the
  abort, the editor cancel, the marker icon, the open gate),
  `exportpage_probe` (enable gates, the page image, the chooser
  name). 356 tests. **PHASE 6 COMPLETE — user-tested, all pass
  (2026-09-06; the 5 user-test steps at the end of
  `docs/phase-6-kickoff.md`). NEXT: Phase 7 (platform) — RE-SCOPED
  by ADR-028 (2026-09-06): device sync, the HTTP remote library,
  the tray icon, and the i18n (TR) port moved to `docs/backlog.md`
  WITH their research records; Phase 7 is now the D-Bus single
  instance + the startup file pipeline only.
  T1 IMPLEMENTED + PROBE-PROVEN (2026-09-06), user test pending:
  the app runs a UNIQUE GApplication (`NON_UNIQUE` dropped;
  `HANDLES_OPEN | HANDLES_COMMAND_LINE`) — a second launch
  registers remote, forwards its argv and exits ~0.1 s; the primary
  parses the handoff (`StartLast`: present-to-front, files with
  `newSlot: true`, the `-p` 1-based page, `.cbl`/`-il` parse but
  stay inert until T2, `.crplugin` inert per ADR-027) and boots
  through the same handler (first-launch files `newSlot: false`,
  the `OpenLastFile` session reopen, the exit-time
  `Settings.LastOpenFiles` capture in the close-request handler).
  Restart = spawn `<exe> -restart -waitpid <pid>` then quit (the
  new process polls /proc up to 30 s before GTK). PROBE LESSONS:
  `activate` NEVER fires with HANDLES_COMMAND_LINE (the command-line
  handler IS the boot); argv[0] rides BOTH deliveries (strip
  element 0 or the app opens its own binary as a comic); a forwarded
  client is a ZOMBIE until reaped (`/proc` exists — gate exit via
  `Child::try_wait`); the primary must stay ~500 ms after the
  handoff for the client's reply. Probe:
  `cr-ui/examples/singleinstance_probe.rs` (A/B/C gates; no XDG
  isolation needed). 357 tests.   T1 USER-TESTED, ALL PASS
  (2026-09-06). T2 COMPLETE — USER-TESTED (2026-09-06): the
  `.cbl` import (`ImportComicList` port) — the container model +
  parse/write in `cr-core/src/database/reading_list.rs` (the net48
  shape), the matching in `cr-engine/src/reading_list.rs`
  (Guid → file name → the series/number relaxation ladder;
  `SeriesEquals` with the rxVolume/rxSpecial ports; placeholders =
  fresh-Guid fileless books), the flow in
  `cr-ui/src/dialogs/import_list.rs` (the missing-books question),
  landing in `ComicDatabase::temporary_folder` or the selection's
  container, the navigator "Import Reading List…" item + the
  TempFolder icon, the `.cbl` branch in `OpenSupportedFile`, `-il`
  on both boot paths. The user imported the real ComicRack
  "Venomous.cbl" (193 items, all solved), used Add-missing (the
  fileless placeholders), confirmed the stored order.
  USER-REPORTED FIXES during the test: (1) the rxNumber RTL parser
  emulation — rightmost-START match (`rightmost_start_match`);
  "Watchmen 001" parsed series "Wat" before; (2) the reading-list
  DISPLAY order — the ItemView's SortChain fell back to guid order
  on the empty chain, shuffling every unsorted view; the empty
  chain now returns Equal (input order = display order).
  INCIDENT: `gio trash ""` from the fileless placeholders trashed
  the repo CWD — recovered from the trash; the delete-path audit +
  guards (the empty/is_file check + the reveal gate) recorded.
  BOOT-CRASH FIX: the Detail-workspace slider re-entrancy
  (set_range emits value_changed outside the sync guard;
  notify_and_redraw held the borrow across the hook) — the user
  verified the fix. Probes: importlist_probe, listorder_probe,
  bootreentry_probe, singleinstance_probe. 368 tests.
  PHASE 7 COMPLETE — ALL TASKS USER-TESTED (2026-09-06). NEXT:
  Phase 8 (polish/ship + the user-reported list) — the kickoff is
  `docs/phase-8-kickoff.md` (T1 the UI fixes batch: the & mnemonics,
  the popover arrows, the tree-menu position; T2 the default view =
  Library; T3/T4 the CBL-import and fileless-delete perf; T5 the
  Details column resize; T6 the Folders tab; T8 packaging; T9 docs +
  migration; T11 the Windows-path migration dialog (added 2026-09-07,
  user request — scope + the four user decisions live in the T11
  section); T10 the perf sweep). The database-backend item became
  PHASE 9 (2026-09-07, spun out of the Phase 8 T7 exploration):
  `docs/phase-9-kickoff.md` — SQLite canonical after an explicit
  Settings migration (Postgres rejected; XML becomes the ComicRack
  import/export codec; fresh installs keep the XML default until
  migrated; FULL design with hot columns + per-book dirty tracking +
  incremental saves, ~3-5 wk), starts after Phase 8 closes, ADR-029
  gate before any code. Open gaps: WebComicProvider, PDF/DjVu
  writers, HEIF/AVIF decode, the T14 per-list sort (the port resets
  the view sort on every list switch; the C# keeps it per list — a
   recorded deviation).
  PHASE 8 STATE (2026-09-08 late, the session-fresh pointer):
  DONE + USER-TESTED = T1, T2, T3 ("the cbl-lists imported
  reasonably fast now"), T4 ("works fine"), T5 ("works fine"), T6
  ("works fine" — three fix rounds recorded in the T6 section:
  the side-by-side split, the worker-thread folder scan, the
  last-close → ShowLast), T10 slice 2 ("works now"), T10 slice 1
  USER-TESTED, ALL PASS (2026-09-08 — sort/group column clicks +
  the Show Duplicates toggle feel instant). T11 IMPLEMENTED
  (2026-09-08, user test pending): the Windows-path migration —
  `cr-engine/src/path_migration.rs` (detection, the collapsed
  common-prefix roots, component-wise case-insensitive
  `map_relative`, found → re-home + refresh, not-found → fileless,
  watch/blacklist rewrite-on-exists; 11 unit tests) + the
  `Library` methods + the dialog (`dialogs/path_migration.rs`,
  the live "N of M found" rows) + the boot prompt
  (`maybe_prompt_windows_path_migration`, after the attention
  dialog, before the file pipeline) + `win.migrate-paths` (File
  menu, enabled only while Windows paths remain). Gate:
  `pathmigration_probe`. LESSON: the mapping STRIPS the Windows
  root (`C:\Comics\X` → `<target>/X`) — test mirrors put files
  directly in the target; and a shared XDG across probe runs
  POISONS them (the statusbar false alarm) — fresh XDG per probe.
  USER TEST (2026-09-08): the migration works; FIX ROUND 1 for the
  "OK froze the app until all the comics were loaded" report — the
  apply ran the FULL `refresh_file_info` per found book whose
  page-count branch OPENS EVERY ARCHIVE inline on the UI thread
  (the stored Windows mtime always differs after a copy, so it
  always fired; the scanner runs the same cost on its worker).
  `scanner::refresh_file_info_basic` (metadata-only) is now public
  and the apply uses it; the stored page count rides (the reader
  fills unknowns on open). Gate: `cr-engine/tests/path_migration_perf.rs`
  (apply 120 real zips ~1 ms, page counts asserted intact, the old
  path timed for the record — CB7/CBR libraries paid a 7z
  subprocess per book). USER-TESTED, ALL PASS (2026-09-08, "works
  just fine"). T8 DEFERRED to `docs/backlog.md` + HEIF/AVIF
  SKIPPED (user decisions, 2026-09-08 — the tarball tracks stand).
  PHASE 8 REMAINING (user-decided order): the T10 remainder (three
  measurement gates: startup_probe, scan_perf, list_eval_perf —
  fix only measured offenders) → T9 (docs + `cr-cli migrate`) →
  Phase 8 closes. THE T10 REMAINDER IS COMPLETE (2026-09-08): the
  three gates exist and the list-eval one condemned real offenders
  — fixed: the folder-union/intersect + id-list linear-scan dedups
  (O(N²) → HashSet; 28 s / 277 ms → 2.7 ms / 0.94 ms at 10k), the
  EAGER series-stats build in MatchContext (forced a proposed parse
  per parse-needy book per evaluation, 27 s at 10k → LAZY on first
  stats_for), and the parse itself now rides
  `book_view::proposed_cached` — the path-keyed process-wide
  Proposed cache (the parse's only input is the file path, so the
  key IS the invalidation; 100k-entry cap; the backlog plan-B entry
  is LANDED). Startup (init 3.2 ms + shell 22.5 ms at 255;
  25.6/33.5 ms at 10k, release) and the scan (33 ms/1000 fresh,
  5.4 ms re-scan) measured HEALTHY. No UI change → no user test.
  All parse consumers must use `proposed_cached`, never
  `proposed` directly (tests excepted). T9 COMPLETE (2026-09-08):
  `cr-cli migrate <source> [--out] [--force] [--dry-run]` (the CE
  profile verify + the byte-identical DB copy with the
  .premigrate.bak guard + the consumed-ini-key mapping; gate
  `migrate_copies_and_maps_a_ce_profile` on the real-world fixture)
  + the README migration rewrite (the helper + the manual way + the
  T11 dialog flow). **PHASE 8 CLOSED 2026-09-08** — T1-T6, T10,
  T11 done + user-tested; T7 → Phase 9; T8 + HEIF/AVIF deferred by
  user decision (docs/backlog.md). PHASE 9 DEFERRED (2026-09-08,
  user decision immediately after Phase 8 closed): the SQLite
  backend has NO active phase now — the full design stands in
  `docs/phase-9-kickoff.md` and the backlog entry
  ("From Phase 9") points there; whoever picks it up re-homes the
  kickoff into an active phase first and starts at T1 (the spike)
  → T2 (ADR-029 + user sign-off; NO product code before that
  gate). NO PHASE IS ACTIVE — open work is picked from
  `docs/backlog.md` (open gaps: WebComicProvider, PDF/DjVu
  writers, packaging T8, the T14 per-list sort deviation).
  BEHAVIOR CHANGE a
  fresh agent must know: the LAST-tab-close handler runs
  `select_last_browser()` (the C# RebuildBookTabs tail —
  MainForm.cs:3140 `ShowLast()`), NOT QuickOpen; the T2 record
  below is OUTDATED on that point — QuickOpen's home is now the
  `+` empty slot / any empty current slot (the C# empty-reader
  overlay; the blank reader when ShowQuickOpen is off or the DB
  has no books), and the tab-change hook follows the current slot
  while the reader area shows. bootview (C=the browser on the
  last close, D=`+` → quickopen) and tabstrip (G=quickopen,
  H=the close follows the neighbor slot, I=ShowLast → Pages)
  expectations moved to that shape. T4 record: the delete hang WAS the view
  rebuild — the eager `book_view::PropTable` parsed ~one
  ComicNameInfo per book per rebuild (~0.33 ms × 2627 books;
  `needs_prop` is true for nearly every book: `enable_proposed`
  defaults TRUE and Title is usually empty) even with no grouper
  and an empty sort chain. The PropTable is LAZY now (the C#
  `Proposed` semantics — parse on first read; the dead books
  resolve to the shared empty parse; the empty-chain guard sits in
  the sort closure so the resolve args don't parse first).
  Measured (release, the real chronology + 250 placeholder
  removals through the real menu → dialog): the remove+refresh
  closure 886 ms → 20 ms. Gates:
  `view_state::tests::rebuild_reading_list_scale_stays_fast` +
  `deleteperf_probe` (the real import + remove flow; falls back to
  the synthetic scenario without the fixtures). The C# per-book
  session cache stays plan B (backlog). T5 record: the C#
  separator behavior over the Detail header — the ±2 px hit zone
  at each visible column's right edge
  (`layout::column_separator_hit`, scanned last-to-first,
  unit-tested), left-drag with the C# clamp (0..10000; a 0-width
  column shows nothing), live reflow, double-click auto-size
  (the widest displayed cell text + 8 on a scratch cairo context;
  image columns keep their width — deviation), the col-resize
  cursor, the full-height ResizeMarker line, per-column clipped
  captions with a 1 px framed edge. Widths already rode the T14
  round-trip. NOT ported: header-click sort + drag-reorder. Gate:
  `detailresize_probe`. T6 record: the Files (Folders) view —
  `folder_tree.rs` (the provider `folder_book_list`: the plain
  FileUtility walk, the extension registry, the session books via
  the now-public `scanner::create_book`, the stored metadata read
  for the first 100 files only; the tree: one "/" root, LAZY
  dummy-child fill (a childless row shows no expander),
  `drill_to` through `expand_to_path` (per-row expand_row FAILS on
  fresh rows — measured), the favorites dropdown +
  Add To Favorites (the Settings `FavoriteFolders` port — the
  `<string>` items) + the Include Sub Folders toggle + Add Folder
  To Library + Refresh). The shell: the "folders" stack page with
  its OWN ItemView, `TabId::Folders` (the FileBrowser GIF is not
  bundled → text-only; `DisableFoldersView` hides the tab), the
  CaptionClick toggle + last_browser=2, the folder context menu
  (Open/Reveal/Move to Recycle Bin — the C# RemoveBooks ask + the
  `RemoveFilesfromDatabase` option + the failed-delete message +
  the is-file guard), the status panels + the slider route to the
  ACTIVE browser, LastExplorerFolder captured at close. Deviations:
  no per-view browser toolbar on the folders page (the menubar
  book commands stay library-bound), no RemoveFavorite/Open
  Window/Open Tab, no FileView workspace persistence, the scan is
  synchronous, dot-dirs skipped, names only (no shell icons), the
  View-menu Folders item not ported. Gate: `foldersview_probe`.
  T6 INCIDENT: a probe run wrote Config.xml into the REAL
  `~/.config/comicrust` (the probe guarded only XDG_DATA_HOME;
  `add_favorite` → `save_settings`) — the polluted fields
  (ExplorerIncludeSubFolders/FavoriteFolders/LastExplorerFolder)
  were repaired to defaults, the user's pre-probe workspace/
  settings snapshot is NOT recoverable (the app told in the UAT);
  foldersview/deleteperf/detailresize now REFUSE without BOTH XDG
  vars. Boot-restore lesson: `set_include_sub` fired the toggled
  handler through the boot's settings borrow — the edition-2021
  temporaries lesson AGAIN (hoist the read). T1 record: `&`
  mnemonics strip at render (`menubar::strip_amp`; tables stay
  verbatim; the submenu-parent key strips `&`+`_` so
  `set_sub_enabled("Recent Books"/"Page Type")` finally matches),
  `set_has_arrow(false)` on the navigator/book/page-menu/column-
  chooser popovers (the menubar/dropdown popovers were already
  arrowless), and the navigator context menu parents to the
  TREEVIEW with the raw gesture coords (was: a Box parent + cell
  coords → GTK fell back to the top edge). Probes: menubar
  (labels-with-amp 0 static+filled, top popovers arrowless),
  browserbar (Views drop), navpages step F (arrow=false + the
  pointing rect at the click point). T2 record: boot calls
  `show_browser()` (the C# MainForm.cs:3140 shape; the navigator
  boot fill selects the Library root — no LastLibraryItem
  persistence was added). [OUTDATED since 2026-09-08: the
  LAST-tab-close now runs `select_last_browser()` and QuickOpen
  lives at the `+` empty slot — see the PHASE 8 STATE block.]
  Work rules that paid off in T3/T10/T4: MEASURE the before with a
  committed timing gate, keep the C# algorithm shapes intact (kill
  only the redundant parses), and reuse
  `book_view::needs_prop`/`PropTable::build`+`get` for any new
  per-book proposed-parse need (the table is LAZY now — read
  through it, never parse directly).
  PHASE 8 T10 SLICE 2 COMPLETE — USER-TESTED, ALL PASS (2026-09-07,
  "works now"): the ItemView SCROLL storm behind the "the
  2875-book reading list is unusable to scroll" report. CAUSE
  (measured, not read off):
  the draw func built its culling window as
  `max(viewport_page_size, draw_size)` — but the draw size IS the
  canvas's FULL virtual allocation (`set_content_height`), so
  `visible_items` culled only items ABOVE the scroll position and
  every frame drew every item BELOW it (probe evidence at scroll
  y=4000: 2589 items drawn per frame, 960 ms/frame; ~35 were
  actually visible — GTK clipped the rest), and the first frame
  queued ~2875 thumb loads at once. FIX: cull against the
  adjustment page size (the true viewport; draw-size fallback only
  pre-allocation) in `item_view.rs` set_draw_func. Side fixes that
  fell out: `config.view_height` now gets the real viewport height,
  so PageUp/PageDown step ONE page (was: the whole list —
  page_step_display divides by it); `error_surface()` is a
  thread-local cache (it decoded a PNG per failed-thumb item per
  frame — the Failed placeholder path). Gate:
  `cr-ui/examples/scrollperf_probe.rs` (2875 synthetic books;
  CR_TRACE=1 prints per-frame win/items/pending/ms lines).
  MEASURED (release): 2589→78 items/frame, 960→1.5 ms steady
  frames. RESIDUAL (accepted): a ~110 ms hitch on the first paint
  of a fresh viewport = the one-time per-book caption compute
  (`caption_value` resolves up to 9 placeholders, each can trigger
  an uncached `book_view::proposed()` full filename parse; the
  result is cached per book in the `captions` map) — the C# pays
  the same one-time class; the user test passed without a hitch
  report, so Plan B (the parked per-book NameInfo cache,
  docs/backlog.md) stays unpicked. 373 tests;
  fmt/clippy green; listorder/contextmenu/commands/browserbar/
  statusbar/tabstrip/navpages probes green (the contextmenu Gtk-
  CRITICAL + commands GLib-GIO-CRITICAL verified pre-existing on
  the base HEAD).
  PHASE 8 T3 COMPLETE — USER-TESTED, ALL PASS (2026-09-07, "the
  cbl-lists imported reasonably fast now"): the CBL-import perf —
  `reading_list.rs` builds a `LibraryIndex` per
  `create_from_reading_list` call (Guid + file-name HashMaps,
  first-wins `find` parity) with a `BookShadow` row per book (the
  five shadow values from ONE lazy `proposed()` parse — only when
  `EnableProposed` and a field falls through — plus the
  series_iv/series_sd forms matching the C# `SeriesEquals` option
  chain); the ladder/narrowings/placeholders unchanged. Timing gate
  `cr-engine/tests/reading_list_perf.rs` (synthetic 2500×2500 + the
  real chronology `.cbl` × the real-world library, skipped without
  the git-ignored fixture). MEASURED (release): the user scenario
  (2886 items × 255 books) 413.5 s → 0.069 s; the 2500×2500 storm
  > 15 min (unfinished) → 0.47 s; debug 7.3 s / 0.9 s, the 30 s
  budget holds in both profiles; the placeholder count is asserted
  (matching semantics guarded).   No progress dialog (no real wait
  remains). Probes importlist_probe + listorder_probe green.
  370 tests.
  PHASE 8 T10 FIRST SLICE IMPLEMENTED (2026-09-07), user test
  pending: the view-side proposed-parse storms (the T3 audit's
  remaining sites) — `book_view::needs_prop` (the shared
  parse-dead gate) + `prop_table` (one parse per book per operation)
  + `empty_prop` (the never-read dead value) in
  `cr-engine/src/matcher/book_view.rs`. Sites: (1) the sort
  comparers take precomputed props — `sort.rs` signatures,
  `group.rs::compare_by_column` (+2 Option args), the
  `view_state.rs::SortChain::compare` chain; `rebuild` builds the
  table once per rebuild and sorts bucket indexes; (2) `Grouper`
  gained the prop arg (`group.rs`; the `groupers()` table shape
  unchanged) + the `bucket_of` HashMap rider; (3) `match_duplicates`
  precomputes the five comparer values per book before the O(N²)
  pair loop (the loop's shape/short-circuits/ternary quirk
  unchanged; `compress_series` extracted) and takes the ctx now;
  (4) the smart-list SortedBySeries sort reads `ctx.prop`; (5)
  MatchContext props are LAZY (RefCell parse-on-first-use; the
  series stats build reuses the cache) — a metadata-complete
  library parses nothing per evaluation. Timing gate
  `cr-engine/tests/view_perf.rs`: sort 5000 by Series 25.3 s →
  4.4 ms, group pass 5000 0.84 s → 78 µs, duplicates 1000 ~268 s →
  5.8 ms (release; debug 27 ms / 0.44 ms / 125 ms; budgets
  15/15/30 s). Plan B (the C#-parity per-book session cache) is in
  `docs/backlog.md` with the invalidation surface. 373 tests;
  fmt/clippy green; listorder/browserbar/commands probes green.
  T9 FIRST SLICE (2026-09-06): README.md rewritten for END USERS
  (status, features, format table incl. the verified no-CBR/RAR
  write-back, install from the release tarball or source, data
  paths from cr-core::paths, Windows migration, differences,
  credits). Facts verified against the workflows + write.rs + the
  paths module; no new code.
  INIT-GLOBAL BOOT BUG FIXED (2026-09-06, the cache-folder user
  report "the setting reverts after restart"): `init_global` used
  `OnceLock::set`, which SILENTLY FAILS when an early `global()`
  reader already froze the defaults — and `library::initialize()`
  opens the database (→ `Paths::new_default()` → the
  ExtendedSettings global) BEFORE `initialize_settings` loads the
  ini. Every boot ini/argv value (CachePath, Theme, quick-open
  size…) was dropped at startup; only runtime writes (the dark-mode
  toggle) ever landed, and the ADR-025 "the app starts light on an
  existing config" observation was THIS bug, not C# parity — with
  the write-through fix an existing `Theme=Dark` boots dark (the
  C# behavior). `EngineConfiguration::init_global` had the same
  pattern (the DB load reads it before init) — now a write-through
  RwLock guard (`EngineConfigurationGuard`). Evidence: the CR_DEBUG_SL
  boot trace showed `ini-cache-path=Some(...)` while the app built
  its pool with the default paths; after the fix the override dirs
  appear at boot. 350 tests + all probes green.
  CACHE WIRING (the C# `CacheManager`) COMPLETE — IMPLEMENTED +
  PROBE-PROVEN (2026-09-06), user test pending. The whole cache
  machinery existed but was inert: `ImagePool::new(None)` built no
  disk caches, the thumb memory pool was write-only, and covers
  re-decoded from the comic file on every startup/list swap. Now:
  (1) `ImagePool::with_config(&ImagePoolConfig)` (the CacheManager
  ctor parity) — disk caches from `Paths::{thumbnail,image}_cache_path`
  with budgets + enable flags from Settings (the Preferences
  Advanced page spins now consume — resolves the Phase 5 T1
  deferral), thumb memory = `MEMORY_THUMBNAIL_CACHE_SIZE` (8192
  items) × `MemoryThumbCacheSizeMB`, page memory =
  `MemoryPageCacheCount`; (2) `render_thumbnail` is memory-first
  (`get_thumb_memory`) so the grid/QuickOpen/tabstrip/editor share
  one decode; (3) `DiskCache` gained `CacheSizeMB` pruning (mtime
  LRU, stride-throttled) + `Enabled` + a header-only
  `is_available` (no JPEG body read); (4) T4a:
  `front_cover_thumbnail_key` (the `GetFrontCoverThumbnailKey`
  port) is THE cover key for ItemView, the tab strip (TabInfo
  gained `cover_key`), and the warm-up — `generate-front-cover-thumbnail`
  now renders through the chain and File ▸ "Generate Cover
  Thumbnails" (`win.generate-thumbnails`, was a disabled stub)
  queues one unlimited-queue job per book; (5) T4b:
  `ComicInfo::update_page_size` (+ `get_page_mut_or_add` — the
  `GetPage(page, add:true)` port with sequential-index growth and
  short truncation) + `CacheEventTx` (`PageCached`/`ThumbnailCached`
  on the memory-cache inserts) + `library::install_cache_events`
  (a 500 ms drain writes the decoded pixel sizes into the DB books,
  `TranslateImageIndexToPage` parity; temp books skip). FIXED on
  the way: `render_page` inserted into the page memory pool under
  the BASE key hash but read it under the TIERED hash — the page
  memory pool could never hit its own entries. Gate:
  `cache_probe` (isolated XDG; the 5 gates: 2 cache files while
  browsing, sized-books=2, warm-up idempotent, second-pool-reuses,
  ImageWidth persisted) + 349 tests + fmt/clippy + all other
  probes green. Deviations: the C# default 500 MB budgets kept
  (user raises them in Preferences for large libraries —
  user decision, no auto-scaling); the writer emits sizes only
  after a decode (the C# fills them the same lazy way).
  CACHE-FOLDER OVERRIDE (2026-09-06, commit 2751357): the C# boots
  the cache root from `ExtendedSettings.CachePath` (the `-cp`
  switch / the ini key, SystemPaths.cs:47-50) — the port carried
  the setting but never consumed it. Now `Paths::new_default()`
  reads it (a non-empty override replaces the whole cache root;
  database + config trees unaffected) and Preferences ▸ Advanced
  gained a Cache Folder row (a recorded ADDITION — the C# has no
  UI for it): Change…/Reset write the ini key + the global (the
  theme-persistence pattern); takes effect on the next start.
  `cache_probe` gates F/G cover the override + reset; 350 tests.
  (Phase 6 note: the scripting host was DROPPED with ADR-027 on the
  same day — see the Phase bullet above; `cr-script` is deleted.)
  T1 (the command/action layer +
  accelerators) COMPLETE — USER-TESTED, ALL PASS (2026-09-04, one
  fix round). `cr-ui/src/commands.rs` holds the pure table (69
  shell actions + the C# menu accelerators; the two C# accel
  collisions resolve by menu order — recorded);
  `browser/shell.rs::install_commands` wires every `win.` action
  and syncs enable-state from book/selection/history. Reader
  commands route through `PageView::run_command`; the Library
  group (Next/Prev/Random Book + ShowBrowser) forwards to the
  shell. Enable-state gates the accels (a disabled action swallows
  its accelerator). Probe: `cr-ui/examples/commands_probe.rs`.
  T2 (the bundled icon set) COMPLETE (2026-09-04; no user test —
  the acceptance is the resx gate + headless probe, user directed
  continue). 212 PNGs under `cr-ui/assets/icons/` (identity resx
  mapping except the 29 `Dark*` names → `Dark/<base>.png`; the 17
  GIF animations + ICO not bundled — deviation recorded),
  `cr-ui/src/icon.rs` (`path_for_name` + cached `gdk::Texture`,
  `#variant` fallback), the navigator renders the C# `treeImages`
  icons through a texture column, `cr-ui/tests/icons.rs` gates all
  212 resx names, `icons_probe` proves the loads + the visible
  gallery. Release tarballs ship `assets/icons` now.
  T3 (the menubar) COMPLETE — USER-TESTED, ALL PASS (2026-09-05;
  three fix rounds + two polish rounds). A CUSTOM menubar widget
  (GTK4 model menus cannot show the C#'s 78 menu-item icons):
  `cr-ui/src/browser/menubar.rs` — the pure six-menu table (the
  `OnGuiVisibilities` Fill-mode visibility rule ported and
  unit-tested; the T4 dynamic parents stay out; omissions asserted)
  + the Designer mi→resx icon mapping (unit-gated both directions);
  the widget: flat Button row → hand-built popovers (check slot +
  16 px icon + label + gray accel | submenu arrow), ONE active
  popover at a time (the Wayland one-grab rule — popdown-all-then-
  popup; `GtkPopoverMenuBar.set_active_item` port), Up/Down focus,
  Left/Right + hover top switching, nested submenus, no arrow +
  left-edge alignment via a POP_WIDTH pointing rect, no
  MenuButton frame on submenu rows; `sync` writes check/radio/
  disabled/highlight from the `win.` action states after every
  dispatch (Browse ▸ Library/Pages highlight the active panel —
  C# has no checkbox there); Alt-alone reveal unchanged.
  `gtk4` carries `v4_6`+`v4_10` features; cr-ui mirrors the
  workspace lints with `deprecated = allow` (the GTK3-era family
  the port uses deprecates under those features). Row clicks pass
  the FULL `win.` action name (stripped names fail silently —
  accels kept working while clicks died; round-2 lesson).
  `menubar_probe` gates: the six menus, the visibility rule, the
  row-click + highlight proof (real widget path via
  `MenubarWidget::click_row`), the switching sequence;
  `commands_probe` 69/69. OMISSIONS/BACKLOG TRACKING:
  per-task "Omitted / postponed per task" section in
  `docs/phase-5.5-kickoff.md` (keep it current; a task closes only
  when its entries are resolved or re-homed); cross-phase ideas in
  `docs/backlog.md`.
  T4 (the dynamic menus) COMPLETE — USER-TESTED, ALL PASS
  (2026-09-05; one fix round: the dynamic submenus re-fill on
  their OWN popover open — the top-menu funnel missed revisits
  inside an open menu; the Preferences OK path re-runs the sync so
  the update-book-files hide rule is immediate). `MenuNode::Dyn(id)`
  slots in the pure table (File ▸ Open Books / Recent Books, Edit ▸
  Page Type / Page Rotation, and the Bookmarks dynamic list)
  rebuilt by a shell-owned fill provider at every menu open
  (`MenubarWidget::set_dyn_fill` + `refresh_top` in the open
  funnel — the C# `DropDownOpening` shape; check/disabled state is
  BAKED at fill). New actions: `open-tab`/`recent-book`/
  `open-bookmark`/`page-type`/`page-rotation` (string parameters)
  + Set/Remove Bookmark real (the C# `UpdateBookmark` parity; the
  name prompt is `dialogs::name_prompt.rs` — the
  `SelectItemDialog.GetName` shape). `cr-core::ComicInfo::
  seek_bookmark` (the collection SeekBookmark port; the callers
  pass `current + dir`) + the reader-key bookmark commands forward
  from the view to `ReaderShell::bookmark_nav`. Page edits
  (`edit_open_book`) go through the session book → `apply_edited`
  (the gates) → the Pages panel rebind; the Y page-rotations write
  through now and the stored rotations seed the view at open. My
  Rating became STATEFUL checks (`selection_common_rating` = the
  `RatingEditor.GetRating` port) — the rating actions had silently
  skipped the actions registry (a T1 gap); `refresh_view_from_list`
  now RESTORES the selection (`ItemView::reselect` — the C#
  refresh keeps it; without it every rating commit cleared the
  selection). FIXED (T3 regression): radio-row clicks passed a
  detailed name + an explicit parameter — `activate_action` parses
  a detailed name only WITHOUT args, so the radio rows never fired
  from clicks; the handlers pass the BARE name + the parameter
  now. The menubar-hides-in-browser-with-a-book-open observation
  is DEFERRED to T9 (the port matches the C# formula per the
  menubarvis_probe evidence; side-by-side then).
  Deviations in the kickoff tracker: Recent Books text-only (no
  16 px cover thumbs), a Deleted-page bookmark unreachable, the
  hide rule in the sync (not menu-open), slot accels live from the
  first fill.
  T5 (the reader toolbar) COMPLETE — USER-TESTED, ALL PASS
  (2026-09-05; one fix round: the unparented dropdown-popover
  segfault — `Dropdown::open` now parents the popover to its
  stored ANCHOR BUTTON before presenting; parenting to the window
  would break the undock; the probe gained the OPEN gate — a probe
  that only clicks rows never exercises the present path). The
  user verified: the strip mounts with the C# icons, the page-turn
  main clicks, all seven dropdowns open and fire (radios, bookmark
  rows), the state text (zoom %/rotation °) tracks, the undock
  carries the strip, no crash. `browser/toolbar.rs` — the
  nine-button strip (prev/next split buttons with page-turn main
  clicks, layout/fit drop-only, zoom% + rotate° state text,
  magnifier/fullscreen toggles, the Tools flattened menu) above
  the reader, right-aligned; the drops reuse the menubar row
  machinery (`menubar::build_dropdown` — one shared resolve
  closure pushes the action states into both bars); the RTL/fit/
  layout icons track the reader; the bar rides the undock
  (`ReaderShell::set_undock_chrome`); `win.show-main-menu` (check
  = !AutoHideMainMenu). FIXED: `do_zoom` dropped a preset with no
  composed page (the C# ImageZoom setter stores unconditionally).
  Fill-mode placement into the browser tab strip is T9.
  Deviations in the kickoff tracker (row-above-reader mount,
  drop-only zoom/rotate, the ADR-024 Tools omissions).
  T6 (the browser toolbar reorg + the Detail column chooser)
  COMPLETE — USER-TESTED, ALL PASS (2026-09-05; four fix rounds).
  `browser/browser_toolbar.rs`: the strip in the RIGHT (item) pane
  above the grid — Sidebar, Browse Previous/Next (the list
  history), Views (the view radios + the read-state radios + the
  comic-type checks + Show Duplicates), Group + Arrange (the
  dynamic Not Grouped/Not-Sorted-first tables, stateful check
  rows), the right-aligned Quick Search with the C# scope menu
  (All/Series/Writer/Artists/Descriptive/Catalog/Filename — the
  AllProperties option) on the entry's secondary chevron, a
  DISABLED List Layouts button (T14), the Duplicate List drop (the
  folder walk). The old header folds into the menubar + the strip;
  the header carries the reader page display only. The reader
  toolbar (T5) moved UNDER the menubar (a `toolbar_box` above the
  view stack) so Tools/Fullscreen show in the library view too
  (the C# `OnGuiVisibilities`/`OnUpdateGui` parity); it still rides
  the undock. New stateful actions:
  `view-filter`/`comic-type`/`duplicates-only`/`search-scope`/
  `toggle-column`/`duplicate-list`; sort-column and group-by
  became STATEFUL (check marks; "" = Not Sorted/Not Grouped via
  `ViewState::clear_sort`). EVERY stateful handler calls
  `sync_enabled` (the check marks re-render only on the sync — the
  T6 round-1 bug); `view-mode` state derives from
  `item_view.mode()`. The composed filter (`compose_quick_filter`,
  the C# `ComicBookAllPropertiesMatcher.Create` parity — read-state
  ReadPercentageMatcher, comic-type FileMatcher Not, AllProperties
  op-3 ContainsAll with the enum option name, a MATCH/NOT query
  parses ONLY for the All scope and then the view filters do not
  apply; the duplicate matcher rides on top) — unit-tested in
  `shell.rs::tests`. The Detail column chooser: the ItemView
  right-click routes a Detail-header hit to a PLAIN popover of
  CheckButton rows (`popup_column_chooser` — NOT `build_dropdown`;
  the arrow-less/child-popover dropdown does not MAP window-
  parented on Wayland; the scroller needs BOTH natural-size
  propagations). The Duplicate List engine:
  `library::duplicate_smart_list` (the matcher-values name + the
  `NumberedString` numbering ported in `cr-engine/src/text.rs`) +
  `library::list_folders`. Probe `browserbar_probe` gates the
  OPEN paths + the filter narrowing (3/1/1/1/3) + the scoped
  search + the chooser open/height/toggle + the toggle-browser
  page flip + the view-mode check sync + the duplicate landing;
  the T3/T4/T5 probes stay green. Deviations + the Wayland lesson
  in the kickoff tracker.
  T7 IMPLEMENTED (2026-09-05), user test pending. The two panel
  toolbars: `browser/navigator.rs` mounts [toolbar][search box
  (hidden)][tree] — New Folder/New List/New Smart List (the SAME
  ListCommand path as the context menu, target = the selection),
  Expand/Collapse All (any-expanded → collapse else expand; the
  signals keep the id set), Refresh (`connect_refresh` → refill +
  re-evaluate; the C# RefreshLists wire is dead for the local
  library), the right-aligned Quick Search toggle (the T1 stub is
  now the real stateful `toggle-navigator-search`, check = box
  visibility) with the name-contains filter (Library always shows,
  folders match via ANY child — the `ComicListItemFolder.Filter`
  override). `browser/pages_view.rs` mounts [toolbar][scroller] —
  the Views split button (main click cycles, chevron opens) over
  `win.pages-view-mode` radios Thumbnail/Tile; Tile = fixed cells,
  thumb left + the `ComicTextBuilder` `DefaultPage` text lines
  right ("Page #N"/type/Size/Resolution/Rotation/Bookmark, the
  tab-stop shape). Deviations in the kickoff tracker (Open
  Window/Tab, Favorites, Pages Details/groups/filter/sort).
  `navpages_probe` gates the dispatch, the 9→1→9 filter, the
  expand flip, the Views OPEN gate + the radio click + the
  main-click cycle; 314 tests. T7 fix round 1 (user report:
  Ctrl+wheel dead): the resize handlers were never wired — both
  grids now carry a DISCRETE scroll controller (Ctrl = ±16 resize
  + Stop, the `ItemViewMouseWheel`/`itemView_MouseWheel` parity;
  Detail mode keeps scrolling), pages Tile scales with the same
  height.
  T7 COMPLETE — USER-TESTED, ALL PASS (2026-09-05; one fix round).
  T9 (the workspace tab strip) COMPLETE — USER-TESTED, ALL PASS
  (2026-09-05; one fix round of six items). The user report that
  drove it: the old CR tab bar sits DIRECTLY under the menubar with
  Library, Folders, Pages (if a comic is open) and every open comic
  as separate FULL-WINDOW tabs. `browser/tabstrip.rs` — the strip
  row under the menubar: fixed Library/Pages items (resx `Library`/
  `ComicPage`), one comic tab per open slot (async 16 px cover
  through the thumb pool + `gdk::MemoryTexture` from the
  ThumbnailImage blob, display-name caption, close button, bold =
  current slot; every tab is one CSS `.tab` BOX — caption click AND
  the close button inside it), the `+` (AddSlot → an EMPTY slot,
  silent per user decision), and the right-aligned HOST box that
  parents the T5 reader toolbar (the C# Fill
  `MainToolStripVisible=false` shape; the undock chrome keeps
  riding it — docked home = the strip host). The reader Notebook
  carries NO tabs (`set_show_tabs` false). The stack pages:
  quickopen (startup) ⇄ browser ⇄ pages (FULL-WINDOW, was the left
  mini-tab) ⇄ reader; the left StackSwitcher is gone (Sidebar hides
  the plain navigator pane); the status label sits BELOW the stack
  and rides the C# `flag4` with the strip
  (`tabstrip::tabstrip_visible`, unit-tested: visible unless
  MinimalGui+reader, always on the browser, the
  ShowMainMenuNoComicOpen escape, undocked always). Reader shell:
  `tab_infos()` (cached captions), `has_current_book()/
  open_book_count()` (the empty slot gates the reader commands +
  the Pages tab on the CURRENT book; flag2 counts book slots),
  `add_empty_slot`/`close_slot`/`cycle_slot`, the `on_tabs_changed`
  hook, refresh_chrome fires book_changed ALWAYS (an empty slot
  clears the Pages panel). Behavior: strip clicks swap the
  workspace, a RE-click on the selected item toggles
  browser/reader (the C# `tab_CaptionClick`), Browse ▸ Library/
  Pages select tabs, prev/next-tab + Open Books rows reveal the
  reader (the C# `ShowView(i)`), the sync derives the strip
  selection from the visible workspace. The MENUBAR is ALWAYS
  visible now (user decision): `AutoHideMainMenu` + the Alt-alone
  reveal are REMOVED — `menubar::menubar_visible` + its tests keep
  the C# formula for the record; the setting no longer drives
  visibility (this resolves the deferred T3 hide-rule observation).
  Fix-round facts a fresh agent needs: dropping a Rust widget
  handle does NOT unparent a GTK widget (the X-close bug — the
  strip's retain removes the root explicitly, and the probe gates
  slots == WIDGETS); the workspace stack + paned + scrollers need
  vexpand (the Pages page collapsed without it; the probe gates
  stack-h == pages-h); the strip compacts via valign Center +
  `min-height: 0` + a scoped `.tabstrip button` rule (row 36 px,
  probe gates < 40). Folders tab HIDDEN (no engine;
  `DisableFoldersView` parity — its own follow-up task), dock modes
  deferred (T10). Probe `tabstrip_probe` gates the whole flow (it
  loads the theme CSS itself); `navpages_probe` gained the
  pages-workspace step (the Views drop needs a MAPPED anchor); all
  other probes stay green; 315 tests. Deviations in the kickoff
  tracker (no tab context menu, no drag-reorder, silent empty
  slot, one undocked reader).
  Dark/Light toggle COMPLETE — USER-TESTED, ALL PASS (2026-09-05;
  two fix rounds, ADR-025).
  Browse ▸ _Dark Mode (`win.dark-mode`, iconless, no accel): a
  recorded ADDITION — the C# theme is the boot-only `-dark`/
  `Theme` ini switch with no menu command and no ini write-back.
  `ExtendedSettings::effective_theme` ports the `Theme` getter;
  the global moved `OnceLock` → `RwLock` (`global_mut`). The
  handler flips the global, clears `use_dark_mode`, applies
  `theme::set_dark` (instant re-style), and persists `Theme` +
  `UseDarkMode=False` through `library::save_ini_keys` (a new ini
  merge-writer into the LAST chain file). `Themes::Default` =
  LIGHT (C# parity) — `-dark`/`-theme Dark` still work.
  (CORRECTED above: "starts light on an existing Dark config" was
  the init-global boot bug, not C# parity.) `.placeholder-label`
  mid-gray (both-theme readable).
  FIX ROUND 1 (user report: the ItemView grid + the Pages panel
  stayed dark in light mode): the two views drew hardcoded dark
  palettes. `theme::Palette` resolves the GTK named colors
  (`theme_base_color`/`theme_bg_color`/`theme_fg_color`/
  `theme_selected_*`) per DRAW CALL through the widget style
  context (the C# `SystemColors` parity — `ThemeColors.ItemView.
  DefaultBack` = `SystemColors.Window`; no cache to invalidate);
  `theme::redraw_on_theme_change` queue-draws the two canvases on
  the prefer-dark notify (GTK does not invalidate custom cairo
  draws on a theme flip). The reader PAGE SURFACE stays dark in
  both themes BY PARITY (the C# `ImageDisplayControl.Initialize
  Component` sets `BackColor = Color.Black` unconditionally; the
  reader background is the Auto/Color/Texture setting, not the
  theme) — the dead `window.reader-window`/`.reader-page-area`
  CSS rules (no widget carried the classes) are removed.
  FIX ROUND 2 (user report: the reader surround stayed dark in
  light mode): `page_view::background_color` resolves the
  surround per frame — Auto keeps the page-corner sampling, every
  other mode uses `theme::palette(area).base` — a RECORDED
  DEVIATION (the C# paints `BackColor = Color.Black`
  unconditionally, the reader never follows the Windows theme;
  the user chose theme-following so the whole app flips). The
  magnifier lens threads the same color; the reader canvas rides
  `redraw_on_theme_change`. The user verified: instant flips
  both directions, persistence across restart, light reader
  surround with page turns/transitions/magnifier/continuous.
  `menubar_probe` gained the
  dark-click gate; `commands_probe` 70/70; 317 tests. Deviations
  recorded in the kickoff tracker + ADR-025.
  T8 (the status bar) COMPLETE — USER-TESTED, ALL PASS (2026-09-05;
  two fix rounds + one clarification; the full fix-round record
  lives in `docs/phase-5.5-kickoff.md`, the T8 entries).
  `browser/status_bar.rs` — the panel row under the workspace stack
  (the C# `statusStrip`): the selection-info spring panel (the
  `SelectionInfo` port — "ListName: N Books (M filtered) / size -
  K selected / size", the single selection shows the file path,
  sizes via the `FileLengthFormat` port; EMPTY without an active
  browser workspace — the C# `FindActiveService` shape; "Ready" is
  only the Designer default), three image lamps (export/write/scan,
  static PNGs per the T2 record, click → the `win.tasks` T13 stub,
  the ported 1 s `updateActivityTimer` poll; `library::is_scanning`/
  `writes_pending`/`export_in_flight` flag the activities), the
  data-source light (always connected), the book caption ("None",
  60-char ellipsis), the page panel (1-based display page, "NA"
  empty, the Locked.png icon while TrackCurrentPage is OFF, click →
  `win.track-current-page`), the page count ("N Page(s)"/"Unknown"),
  and the thumb-size slider (a 120 px GtkScale, browser-workspace
  only, `layout::item_size_range`/`clamp_item_size` — the
  `GetItemSize`/`SetItemSize` ports; Thumbnail 96..512 thumb height,
  Tile 64..512 tile HEIGHT with the width doubled, Detail 12..48
  row height; the drag → `ItemView::set_item_size`). The panel
  updates fold into `sync_enabled` + `rebuild_filter`; the page
  panel also follows the `page_change` hook (wheel/click turns
  dispatch no action — inside the reader borrow, so only the
  passed page value). `track-current-page` is a proper `add_check`
  action (the check derives from the SETTING in the sync). The
  Ctrl+wheel routes through `set_item_size` (FIXES a T7 gap — the
  browser Tile mode never resized from the wheel). Fix-round facts
  a fresh agent needs: the reader page CLICK is `ToggleBrowserFromReader`
  Fill parity = MinimalGui (MainForm.cs:1585 + 2133-2144; the shell
  forwards to `dispatch_current("ToggleMenu")`, the workspace never
  switches; double-click → Full Screen matches the C# 1664);
  GTK's built-in `GtkWindow:handle-menubar-accel` (capture-phase
  F10, default on) consumes F10 before any app accel — the main
  window sets it false (the C# F10 is MinimalGui); comic file tabs
  never toggle on re-click (the C# wires CaptionClick only on the
  workspace items, MainView.cs:161-163); `.tabstrip .tab button`
  is transparent so the active-tab highlight covers the X; probes
  that seed books REFUSE without an isolated `XDG_DATA_HOME=/tmp/
  opencode/...` (an unisolated run polluted the real library once;
  cleaned through the cr-core byte-stable writer). Deviations: the
  export lamp shows only between synchronous export runs (in-dialog
  UI-thread export; the C# uses a background queue), the read-info/
  page/backup/device-sync lamps + the server panel omitted, the
  Win7 overlay icon not portable. Gate: 321 tests, `statusbar_probe`
  (defaults, the info line, the slider resize/sync, the page click,
  the lamp flags, the MinimalGui action), all other probes green.
  **Next: T12 (the Book Display Settings dialog, F9).** T10 (dock
  modes) and T11 (the sidebar preview) moved to the BACKLOG
  (2026-09-05, user decision, ADR-026 — now `docs/backlog.md`);
  the port stays Fill-only and the Small Preview stub stays
  disabled. After T12: T13 (small dialogs), T14 (persistence,
  now carrying the display-options persistence). Phase 6
  (scripting) starts only after 5.5.
  T12 IMPLEMENTED (2026-09-05), user test pending. The C#
  `ComicDisplaySettingsDialog` port — the STUDY-THE-SOURCE
  correction: the dialog is WORKSPACE-scoped, not per-comic
  (`EditWorkspaceDisplaySettings` snapshots the live display into a
  `DisplayWorkspace`; OK/Apply push back via
  `SetWorkspaceDisplayOptions`). `page_view.rs` gained `ImageLayout`,
  the `DisplayOptions` snapshot + the session copy (new views seed
  from it), `parse_texture_file_name` (the `[C]`/`[S]`/`[Z]` layout
  codes + `PascalToSpaced`), `PageView::display_options/
  apply_display_options`, the real render for every dialog field:
  background TEXTURE per layout (None/Tile/Center/Stretch/Zoom) over
  the solid surround, Solid Color via the picker (only when picked —
  the ADR-025 theme base otherwise), Realistic Pages ornaments (the
  1 px frame + edge bows at 7 %/alpha 92 + a stepped-band shadow —
  the cairo no-blur deviation) in composition AND continuous paths,
  paper strength (the `CreateWorkingPaperTexture` white composite,
  <0.05 disables) + paper layout, and the margin zoom factor
  (`ImageZoom * (1 - percent)`, `ComicDisplayControl.cs:1368`).
  DEFAULT CHANGE (C# parity): `DrawRealisticPages` = TRUE — the
  reader draws the ornaments out of the box; Shift+D
  (`ToggleRealisticPages`) flips the real flag now (the old
  paper-fold hack is gone). The dialog
  (`dialogs/display_settings.rs`): General/Effects/Background
  groups + OK/Apply/Cancel, the C# visibility rules, `SelectTexture`
  File parity (case-insensitive match, last-custom replaced on
  browse), bundled layouts parse from file names and apply silently;
  the 14 background textures bundled (`assets/backgrounds/` + both
  release workflows). `win.display-settings` un-stubbed (always
  enabled — no C# gate); apply records the session copy first, then
  pushes onto every open view (`apply_display_options_all`).
  Deviations: persistence lands with T14, combo swatches/the named
  color list/tooltips reduced, the Effects group always shows (the
  C# gates it on the hardware renderer), Page Turn degrades to Fade
  (Phase 3 record). Gate: 325 tests (+4),
  `displaysettings_probe` (5 gates), all probes green.
  T12 COMPLETE — USER-TESTED, ALL PASS (2026-09-05, "all OK"; the
  full 10-item acceptance record lives in the kickoff T12 entry).
  T13 COMPLETE — USER-TESTED, ALL PASS (2026-09-06, "all OK"; no
  fix round; the acceptance record lives in the kickoff T13
  entry).
  - Zoom (`Dialogs/ZoomDialog.cs` port,
    `cr-ui/src/dialogs/zoom.rs`): "Custom Zoom", a 100..800 step-10
    SpinButton, OK applies through `ReaderShell::zoom_current`; the
    clamp mirrors the C# setter (`clamp_percent`, unit-tested).
    Always enabled (the C# carries no enable lambda).
  - Tasks (`Dialogs/TasksDialog.cs` port,
    `cr-ui/src/dialogs/tasks.rs`): the PURE snapshot (`pending_tasks`
    — the ported queues in the C# GetQueues order, the C# message
    texts verbatim, the 10-row cap + the gray "N more..." row, the
    Running-first rule, the abort texts; 4 unit tests) + a
    NON-modal single-instance window (`ShowPendingTasks`
    re-present; the lamps and the menu share it) with the Task|State
    TreeView (bold group rows), the 1 s refresh, and Abort-all
    (clears the unlimited-thumbnail queue + the export queue + the
    pending write timers). New `library` accessors:
    `pending_write_files`/`clear_pending_writes` (the debounced
    write timers) + `scan_location` (the `Scanner.CurrentLocation`
    parity). No Server Statistics tab (ADR-024); the scan row is not
    abortable (no scan-stop port).
  - Quick Rating (`Dialogs/QuickRatingDialog.cs` port,
    `cr-ui/src/dialogs/quick_rating.rs`): title
    "Quick Rating - {CaptionWithoutTitle}", async cover through the
    thumb queue, review TextView, rating Scale 0..5 half steps (the
    star control → a Scale), the "Show when Book read" checkbox
    (AutoShowQuickReview). Edits the FIRST selected book (the
    `books.FirstOrDefault()` quirk) through `apply_edited`. NEW: the
    `OnBookClosing` auto-show (`should_auto_show`: the setting &&
    HasBeenRead && Rating == 0) rides a new
    `ReaderShell::set_on_book_closing` hook fired in `close_tab`
    after the state borrow drops.
  - About: the C# About IS the Splash form — a small modal
    AboutDialog with the bundled Splash.png (`include_bytes!`,
    `cr-ui/assets/splash.png`) + the ADR-020 version: a new
    `cr-ui/build.rs` reads `VERSION` (the release env) and falls
    back to the local git commit count (`0.0.<commits>`; `0.0.dev`
    without git).
  Probe `smalldialogs_probe` gates all five flows (the
  programmatic-dialog shape: `find_toplevel` by title prefix +
  `Dialog::response`); commands_probe 70/70 with the four live
  actions; all other probes green; 328 tests. Copy Page / Export
  Page re-homed to the BACKLOG (`docs/port-plan.md` §6). The
  tracker + the 6-step user test live in the kickoff T13 entry.
  T14 COMPLETE — USER-TESTED, ALL PASS (2026-09-06, "seems to work
  fine"; no fix round; the acceptance record lives in the kickoff
  T14 entry). The layout
  persistence (`Settings.CurrentWorkspace` — the
  `<CurrentWorkspace>` element written into Config.xml after
  `AutoShowQuickReview`). `cr-core/src/settings/workspace.rs`: the
  `WorkspaceState` port of the `DisplayWorkspace` T14 slice with
  the C# element/attribute names verbatim (DatabaseView =
  ComicExplorerViewSettings attributes + the ItemViewConfig child
  carrying Columns/ThumbnailSize/TileSize/ItemRowHeight; the reader
  layout family = the `LandscapeLayout` BookPageLayout element; the
  display family = the T12 fields; 4 unit tests) + `cr-ui/src/
  workspace.rs` (the pure conversions: the C# member-name strings
  for the cr-ui enums, `DisplayOptions` ↔ `DisplayState` with the
  picked color as `#rrggbb`, the browser-readouts mapping; 4
  tests). The shell: `collect_workspace` (the exit snapshot:
  sidebar visibility + split, view mode, sort key + direction,
  grouper, the three mode sizes, the Detail column set, the window
  size + maximized, the reader fit/layout/rotation/zoom/RTL with
  the PREVIOUS save as fallback when no view is open, the display
  family from the session copy) + `apply_workspace` (the startup
  restore; the display options seed the session copy; the reader
  layout seeds every NEW view through `ReaderShell::set_reader_seed`
  — the `ReaderSeed` applies in both PageView creation paths).
  Save points: the close-request handler (`MainFormFormClosed` →
  `CleanUp` parity) and the restart action; restore in
  `BrowserShell::create` after the wire (the `MainForm.Load`
  parity). New ItemView accessors: tile/row-height reads,
  `set_sort_direction`, `set_detail_columns_state`. Probe
  `workspace_probe` gates: the mutate → collect → Config.xml shape
  → a SECOND shell restores → the close-path save survives the
  re-read. Deviations recorded in the kickoff tracker (ONE
  implicit workspace; ONE reader layout family — the C# resolves
  Landscape/Portrait per screen orientation; FormBounds X/Y never
  restored, Wayland; `PagesViewConfig`/`FileView`/
  `ComicBookDialogPagesConfig`/the dock + undock keys wait on
  their tasks). The T12 deviation "persistence lands with T14" is
  RESOLVED. T13 TEST CORRECTIONS during the gate: the two Tasks
  tests failed at the T13 HEAD baseline (verified with git stash) —
  they contradicted the C#-parity queues (the five pools default
  AddToTop → page rows read DESCENDING; the page queues Trim at
  `pageCount*2` = 10) — fixed against the UNLIMITED cover queue
  for the cap/more gate.
  **Phase 5.5 is COMPLETE — every task user-tested. Phase 6
  (scripting) starts from `docs/phase-6-kickoff.md`.**
  Phases 0-5 are complete (their gates stay green). Open Phase 1
  gaps: WebComicProvider and the PDF/DjVu writers (tracked in
  `docs/phase-1-kickoff.md`).
- **State:** `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` are green. 506 tests — 36 suites plus the cr-scrape suites (the Phase 8 perf gates: `reading_list_perf`, `view_perf`, `path_migration_perf`, `list_eval_perf`, `scan_perf`; the cr-ui probes are examples, not tests — the newest are `browserbar_probe` gate D (the column chooser: the open + the submenu-row gate `state_column_chooser_page_rows` reading all=92/a-b=16 through the popover's `visible-submenu` page + the toggle) and D2+D3 (the grouping: ungrouped (1,0) + the disabled action + toggle-groups; the D3 press-sequence machine drives the REAL group-header press paths — label select, single-click collapse/expand of one group, both double-click directions, the true counts on collapsed headers), `statusbar_probe` gates J/J2 (the scan lamp: frames, the visible-only animation, the Cancel-scan menu map + the abort hook), and `smartlistmenu_probe` (the smart-list editor: the rule rows carry the btEdit dropdown + the row menu; the enable states follow `cmEdit_Opening`; delete/cut run through the REAL action path; copy→paste inserts a clone after the row; a group payload is rejected at the cap while a value payload pastes; OK commits — the paste payload rides `probe_paste_payload` because the Xvfb clipboard read stalls, see the state block); `scanrefresh_probe` (gates A-J: the scan-land refresh, the re-scan idempotence, the mid-scan fill, the abort partial landing, the re-scan completion, the mid-add exit save, the mid-RE-scan library liveness (G), the watch-rescan dedupe (H), the non-Library per-tick churn (I), the mid-scan remove/edit survival (J) — it REFUSES a non-isolated XDG pair and wipes it at start, the exit save pollutes it); the real-fixture parts skip in CI without the git-ignored `tests/testfiles/` files; the RAR round-trips skip without `CR_RAR_TESTS` + `rar`). CI runs on the `docker-runner-amd64` container runner (ADR-020) and is LIVE (it caught the 2026-09-09 group-gate flake — the runner + `comicrust-ci:latest` image work). The release tracks are `release.yaml` (rolling prerelease per push) and `tagged-release.yaml` (manual dispatch, stable release for an existing tag — ADR-021, 2026-09-03); `packaging.yaml` (Phase 11) attaches the source tarball, the Arch package, and the .deb to a tagged release. First real tagged-release run (v0.0.273, 2026-09-09) exposed a latent env bug: the "Publish to GitHub mirror" step lacked `TAG` (the Gitea publish succeeded; the mirror step died on `set -u`) — fixed in commit 05483da.
- **GitHub mirror (2026-09-06):** remote `github` = `git@github.com:ScuttleSE/comicrust.git` — a TRUE mirror (identical SHAs; `.gitea/` rides along but is inert there, GitHub Actions only reads `.github/workflows/`). After every origin push also `git push github main`; stable tags get pushed manually once; the `rolling` tag is CI-managed on BOTH sides (each release run deletes/recreates it) — never push it by hand. Both release workflows also publish the built tarball + sha256 to GitHub Releases through `.gitea/publish_github_release.sh` (build once on Gitea, assets on both); it needs the Gitea secret `MIRROR_RELEASE_TOKEN` (GitHub PAT with Contents read/write on ScuttleSE/comicrust; Gitea forbids a `GITHUB_` prefix) — unset secret = the step skips with a notice.
- **Phase 0 gate status:** byte-stable ComicDb.xml round-trip proven on all three synthetic fixtures AND the real-world database `tests/realworld/ComicDb.xml` (255 books, 584 KB, 2026-09-02, user-approved commit).
- **Phase 2 gate status:** every saved smart list in the real-world DB (a) binds to the matcher registry, (b) renders to a `Match` query string that re-parses and re-renders byte-identically, and (c) evaluates to the SAME book sets the C# cached in `CacheStorage` (Never Read = all 255, Files to update = the 3 dirty books, Reading/Read = empty). Evidence: `crates/cr-engine/tests/realworld_query.rs`.
- **Phase 3 gate status (COMPLETE):** a real comic (`tests/testfiles/`, git-ignored, user-supplied) opens in a GTK4 window and reads comfortably: single/double/adaptive/continuous layouts, spread composition with cover-right + binding-edge rules, fit modes with anamorphic tolerance, zoom/pan/rotation, RTL, continuous scroll with anchor-stable layout rebuilds, fade/slide transitions, paper texture, Auto/Color/Texture backgrounds, the real `MainForm` input map, session tabs with undock, fullscreen chrome with cursor auto-hide, reading-state tracking, the magnifier, error pages, and pool-queue page loads. User-verified after each task; UI smoke tests on this machine run headless under Xvfb + screenshots (see the probe lessons below — the key-injection tools are unreliable; only user tests decide input behavior).

- **Phase 4 gate status (COMPLETE):** a user browses the migrated 255-book library in daily-driver comfort: list evaluation in the navigator, real covers in Thumbnail/Tile/Detail with sort/group/search, the status bar, the context menu, double-click → the docked reader (tabs + undock), reading-state round-trips through byte-stable saves, the Pages panel bound to the open comic, and QuickOpen at startup. User-verified per task over the sessions of 2026-09-03/04; 236 tests, fmt + clippy clean.

### Phase 5 progress (session of 2026-09-04)

T1 (the settings port + the options builder + the Preferences
shell) COMPLETE — USER-TESTED, ALL PASS (2026-09-04). The user
verified: the dialog opens from the header button; the Behavior
page matches the C# auto-panel; a toggle + OK persists across
restart (Config.xml in `~/.config/comicrust`); Cancel discards;
the page-wall setting visibly changes the reader; the
add-to-library-on-open flow adds non-library comics; the
fullscreen cursor hide (5000 ms) + AutoMinimalGui work; the
watch-folder page persists add/Watch toggles. Implementation
record (the layer, the C# corrections, the stand-up
reconciliation, the probe recipe) lives in
`docs/phase-5-kickoff.md`. Deferred within Phase 5: the
disk-cache settings do not consume into the ImagePool disk
caches yet (memory-only pools), the language page waits on the
TR loader, the Scripts page on Phase 6.

T2 (the book editor) CHECKPOINT 1 IMPLEMENTED — user test pending
(2026-09-04). `cr-ui/src/dialogs/book_editor.rs`: the Details/Plot/
Catalog/Catalog-form rows through the cr-core registry (the
`SaveBook` parse semantics unit-tested), the proposed-value
placeholders on the EnableProposed combo, the Pages tab (list +
preview + first/prev/next/last + the context menu: page type /
rotation / position / mark-deleted / move top+bottom / reset
order), the Colors tab (five sliders → `book.color_adjustment`),
and Apply/OK/Cancel with the C# commit-point semantics (prev/next
save first; Cancel never reverts). The cr-core page ops
(`update_page_type/rotation/position`, `move_pages` with the C#
IndexOf identity + cursor arithmetic — unit-tested,
`reset_page_sequence`, `sort_pages_by_key` with an injected
comparer, `translate_image_index_to_page`, `front_cover_page_index`)
landed in `comic_info.rs`. The browser context menu "Properties…"
opens the editor over the selection (prev/next with >1);
`library::apply_edited` replaces the library book by id + marks
dirty (the DB save persists). Probe proof: the `editor_probe`
example under Xvfb renders the Details grid and the Pages tab; the
probe caught the first-draft page-type values being shifted by one
(FrontCover = 1, Deleted = 1024 — fixed). Deferred to checkpoint 2:
the file write-back queue (`UpdateComicFiles` + never-write-
defaults + "Files to update" flips), the bulk-edit dialog, the
custom-thumbnail buttons (the `type://` pool loader), the
white-point color pick, the library-wide custom-value key editor.
T2 CHECKPOINT 1 COMPLETE — USER-TESTED, ALL PASS (2026-09-04,
six fix rounds: cover blob parsing, page-list selection, the menu
mechanism swap, the reader color-adjustment rendering, the
completion-payload display-position fix (the sequence scrambling),
the provider-count + stored-overlay merge in
`cr-ui::pages::merged_page_entries` used by both the reader and
the editor, and the editor's missing half of that merge — the
scripted-edit lesson is recorded).
T2 CHECKPOINT 2 IMPLEMENTED (2026-09-04), user test pending: the
file write-back + the bulk editor. `apply_edited` marks
`comic_info_is_dirty` (the `WatchedBookHasChanged` parity) and
schedules the 100 ms debounced `library::update_book_file` (the
`AddBookToFileUpdate` gates: UpdateComicFiles, then
AutoUpdateComicsFiles || alwaysWrite, then the dirty flag;
`WriteInfoToFileWithCacheUpdate` parity: the scoped write via
`cr_io::write::store_info_scoped` — ComicBook.xml only with
`UpdateComicBookFiles` —, the file-properties refresh, the flag
clear). The "Files to update" list flips (the matcher reads the
flag). The context menu gains "Edit…" (the bulk editor,
`bulk_edit.rs`: a Set check per field, the common-value gray cue,
only checked fields apply) and "Update Book File(s)" (the manual
alwaysWrite path). Probe-proven: the writeback_probe seeds an
isolated library, edits through apply_edited, and the archive's
ComicInfo.xml carries the edit with the flag cleared. Deferred:
the exit SaveDirtyBooks ask-dialog for temporary books, the
ComicBookIsDirty half (never set), the C# tri-state list-merge
checkbox mode. T2 CHECKPOINT 2 COMPLETE — USER-TESTED, ALL PASS
(2026-09-04; one fix round: the editor commits now refresh the
browser grid). **T2 is COMPLETE. Next: T3 (the smart-list editor
`SmartListDialog` + the matcher editors).**

### Phase 5 progress (2026-09-04, T3)

T3 checkpoint 1 IMPLEMENTED — user test pending. The model layer:
`cr-engine/src/matcher/edit_ops.rs` (add_rule duplicates after the
node, add_group wraps a clone in a new And-group, remove blocked
at one node, move up/down, switch_type keeping values + clamping
the operator into the new spec — the C# `newMatcher.Set(current)`
parity; MAX_LEVEL 5; nested index paths; 5 unit tests). The
dialog: `cr-ui/src/dialogs/smart_list.rs` — one Designer | Query
notebook (the C# Ctrl-swaps two dialogs): the head fields
(name/notes/base combo with the recursion-filtered options via
`library::smart_list_base_options` — the chain-walk
RecursionTest parity; Library = the empty Guid / ALL-ANY / Not-
in-base / limit type+value / QuickOpen) + the matcher rows (the
type combo over all 97 spec descriptions, the per-spec operator
combo, 0-2 value fields by argument count, the Not check, the
right-click menu New Rule / New Group / Delete / Move Up /
Down — `cr_ui::library::{find_smart_list, update_smart_list}`
with the `SetList` id/count/cache parity). The Query tab renders
the item (`Matcher::from_raw` → `render_smart_list_query`) and OK
parses it back (`to_raw`); a parse failure blocks the close with
the error line (the C# keeps the old item). Navigator: "New Smart
List…" inserts an empty list and opens the editor (Cancel removes
the fresh empty insert — the C# flow; `new_smart_list` now
returns the id); "Edit Smart List…" opens pre-filled. Headless
probe: the editor opens from the navigator menu with all head
fields. Deferred to the next step: the reading-list editor
(`ListEditorDialog`, the `IdListItem` model) and the folder
`EditListDialog` (still the bare name prompt).
T3 CHECKPOINT 1 COMPLETE — USER-TESTED, ALL PASS (2026-09-04; one
fix round: the re-entrant close response + the query-text
clearing — the record lives in `docs/phase-5-kickoff.md`).
T3 TAIL COMPLETE (2026-09-04), user test pending: the
`EditListDialog` port (`cr-ui/src/dialogs/list_editor.rs`) — the
C# routes FOLDERS (name/notes + combine mode) and READING LISTS
(name/notes + QuickOpen) through it from the one Edit… item (the
C# `ListEditorDialog` is an unrelated workspaces editor). The
navigator gains "New List…" (dialog-first, then insert; a
cancelled fresh insert pops) and Rename routes through Edit.
`library::{new_id_list, update_list_fields}` + `new_folder` now
returns the id. Headless probe: the full New Folder flow lands
the typed name in the tree. The reading list's book management
(drag-in ordering) stays with the browser drag-drop work.
T3 TAIL COMPLETE — USER-TESTED, ALL PASS (2026-09-04). **T3 is
COMPLETE.**
### Phase 5 progress (2026-09-04, T4)
T4 IMPLEMENTED — user test pending. The export engine:
`cr-io/src/export.rs` grew the `ExportSetting` model (the C#
enums verbatim, `[DefaultValue]` defaults), the
GetTargetFilePath/GetTargetFileName/GetTargetPath port
(filename/caption/custom+start naming, MakeValidFilename), and
the sequential export engine (`export_book` +
`export_books_combined`): CBZ/CBT native packing (Original
pass-through with the original names, JPEG conversion via
cr-image — PNG/WebP re-encode to JPEG, a documented deviation;
CB7 reports not-ported), Store/Medium/Strong, overwrite,
keep-original-names, tags-to-append, ComicInfo embed. The
parallel/spill machinery stays single-threaded on purpose
(documented). The dialog: `cr-ui/src/dialogs/export.rs`
(target/folder/format/compression/naming/page-format/quality/
flags + progress line + errors); the browser context menu gains
"Export…"; the last-used settings persist for the session
(`CurrentExportSetting` parity — session-only until the settings
schema grows the export lists). Tests: the naming/target-path
unit tests + the end-to-end engine test. The small dialogs: the
remove flow gained the delete-confirm (list vs library vs also
delete the files — `gio trash`, the ADR-006 parity); the export
progress feeds inline in the export dialog. Deferred: the
quick-rating dialog (batched with the reader close-flow polish),
the splash (cosmetic), export presets (the settings schema).
Headless probe: the export dialog renders all fields over a
seeded book; the engine test proves the archive output.

### Phase 4 progress (sessions of 2026-09-03)

T1 (the library session) COMPLETE — USER-TESTED, all pass (re-link
"23 re-linked" persisted, reading-state resume, temporary-book
reset, lists Read 1 / Never Read 254). The session
lives in `cr-engine/src/library.rs` (`Library`: open/save/dirty/
scan/watch + the QueueManager), wired in `cr-ui/src/library.rs` (the
`Program.DatabaseManager` equivalent). The database opens at startup
from `~/.local/share/comicrust/ComicDb/ComicDb.xml` (ADR-022 — a
minimal `cr-core::paths` slice; full settings port stays open), saves
on the reader window's close-request and every 600 s when dirty
(`DatabaseBackgroundSaving` parity). The reader reuses library books
(`ComicBookFactory.Create` parity: file-info refresh + open stamps),
mirrors page turns into the DB book, and the exit save persists the
reading state; non-library comics keep session-only state (C#
`AddToTemporary` parity). Same-path opens focus the existing tab.
The launcher grew "Add Folder to Library…" (a recursive scan into
the DB, `AddFolderToLibrary` parity); watch-folder events rescan the
affected roots. Fixed on the way: the `open_with_fallback` fresh-DB
path now uses `create_new()` (default list tree), a MISSING file is
a silent `OpenStatus::FreshEmpty` (only a CORRUPT file shows the
"problem" message), and the OpenMessage dialog deferred to the shell
(a pre-startup dialog warns in GTK4). Acceptance:
`crates/cr-engine/tests/library.rs` — fresh-DB default tree, the
real-world session lifecycle (unmutated re-save byte-identical,
reading-state round-trip, Never Read 255→254 + Read 0→1 flip,
exactly the mutated books change, stable re-save), scan
add/missing-flag, watch→rescan.

T2 (the list navigator) COMPLETE — USER-TESTED, all pass (tree,
evaluation with the live Never Read flip, create/rename/delete
persisted across restart; the smart-list query needs the exact C#
form `Match [Series] contains "Batman"` — the dialog example was
fixed).

T3 (the ItemView core) COMPLETE — USER-TESTED, all pass (grid with
real covers, selection, keyboard nav, type-ahead, list-driven
sets, double-click → reader repeatedly). `view_state.rs` (MRU-3
sort chain, group buckets, collapse, selection model),
`layout.rs` (Thumbnail/Tile/Detail flow, group headers, culling,
hit tests, keyboard movement), `columns.rs` (the C# default
columns), the engine's `display_text.rs`
(`GetPropertyValue(proposed:true)` parity), and `item_view.rs` (the
DrawingArea widget: covers through the thumb queues + pump,
click/ctrl/shift/rubber band, full keyboard nav, type-ahead,
group-header collapse, double-click/Enter → the reader). The
launcher window is now the browser: navigator left, ItemView right.
236 tests green. T4 added `item.rs` (covers, read-marker ribbons,
rating tags, missing marker, the exact Caption format through the
ported `ExtendedStringFormater`, tile text lines with the shared
tab stop) — USER-TESTED, all pass. Deferred: the unported state
PNGs, the dog-ear curl, and the custom-thumbnail display (needs
the settings port for the `type://` loader + the CustomThumbnails
path). User-test lessons: the thumb pump must live while
loads are in flight (a first-idle break strands completions); the
pool's cached thumb blob is the C# `ThumbnailImage` serialization
(parse before decoding); the canvas grabs focus on click AND on
window activation (GTK4 has no click-to-focus); the app's reader
slot must clear when the reader window closes (a closed window in
the slot swallows every later open). Known cosmetic: a startup
`gtk_css_node_insert_after` GTK critical. T5 COMPLETE — user-tested over five fix rounds. T5 lessons:
GApplication routes the `open` signal BEFORE `activate` — the shell
must be created on first need (a mapped window holds the app;
without one it exits cleanly pre-activate); an "unreachable
pattern" warning exposed a duplicated `Ok` match arm that silently
skipped the reader stack switch; an empty page list must not
`clamp(0, -1)` inside non-unwindable GTK closures (the C# tolerates
empty comics); a widget packed twice keeps its FIRST parent (the
reader subtitle never showed — remove the dead pack); callbacks
registered on a widget must never fire while that widget holds its
own borrow (the status bar re-entered the ItemView — drop the
borrow first); a FIRED one-shot glib source must not be removed
(the timeout clears its own slot); `starts_with` checks need
case-insensitivity where the C# uses OrdinalIgnoreCase (the search
query example); PER-CACHE RULE — every per-frame text path
(captions, Detail cells, Tile segments) caches per book id and
clears on a book-set swap (the proposed-name fallback runs filename
regexes; the tile truncation walked one char per text_extents —
binary search + cached segments); the draw path only reflows on a
viewport-width change. T6 added `browser/pages_view.rs` (the Pages
panel: binds the OPEN comic, page-number badge, bookmark pennant,
current-page marker + ensure-visible, double-click →
`PageView::navigate`), the browser-panel Library|Pages tab switch,
the QuickOpen stack page (the three built-in lists through
`library::quick_open_lists`, captionless covers via
`LayoutConfig.hide_captions`), and `ReaderShell` hooks
(`current_comic_book`, `navigate_current` with the view cloned out
before the callback fires, `set_on_page_change`,
`set_on_book_changed`). T6 COMPLETE — user-tested over five fix
rounds; the round lessons: the Pages panel binds the OPEN comic
and rebinds on reader tab switches (slot-guard the turn hook or
the marker follows the wrong comic), thumb completions carry the
queued source path (stale loads after a rebind otherwise land in
the new comic's map), a hidden tab has width 0 (reflow on tab
visibility + the draw-path height self-correction), and the
provider index fills `info.pages` on open (the C#
`ProviderIndexRetrievalCompleted` parity) — `refresh_file_info`
sets only the count.
`cr-engine/src/lists.rs` evaluates the ComicLists tree (Library =
all, folder Or = union / And = intersect / Empty, id lists, smart
lists with recursive base-list resolution + a cycle guard; the
`OnGetBooks` family). `cr-ui/src/browser/navigator.rs` is the tree
widget (TreeView/TreeStore, kind icons, expansion + selection kept
across refills, right-click context menu with New Smart List / New
Folder / Rename / Delete through bare entry dialogs — the editor
dialogs are Phase 5). The launcher window became the browser
skeleton: navigator left, placeholder right showing the evaluated
list + count. Headless probes verified the debounced selection
evaluation (Library 255, Never Read 255) and rendering for both a
fresh DB and the fixture. Probe lessons: the widget's Rc must
outlive the window (the host holds it); GtkTreeSelection
`select_iter` silently no-ops on rows inside collapsed folders —
`expand_to_path` first (the WinForms `SelectedNode` parity).

### Phase 3 progress (sessions of 2026-09-03)

COMPLETE — T1-T6 all done and user-tested. Architecture per ADR-017:
one widget (`cr-ui/src/reader/page_view.rs`) renders one virtual image
through the part machinery (`reader/display.rs`, the
`ImageDisplayControl` `DisplayOutput` port, unit-tested); the comic
layer composes pages into that virtual image — single page, spread
(`compose_spread`), or continuous strip (`reader/continuous.rs`, the
`ContinuousPageLayout` port). T3 shipped: layout modes Single/Double/
DoubleAdaptive/Continuous, spread rules (cover right, RTL FlipPages
swap, `DoublePageOverlap` trim, forced-double slot), Fade/LeftRight/
TopDown transitions (Paging degrades to Fade until GL), paper texture
(bundled `cr-ui/assets/papers`), Auto/Color/Texture backgrounds, and
`MemoryPool::get` + `ImagePool::render_page` cache-first ordering.

T4-T6 (the closing slice): T4 shipped `reader/keys.rs` — the exact
`MainForm.InitializeKeyboard` table (41 reader commands, registration
order = dispatch priority, exact key+modifier match), wired to wheel,
tilt, click, double-click, left-drag pan (5 px threshold), middle-drag
zoom, zoom anchoring, page walls (`PAGE_WALL` 300 ms,
`IsPageChangeWalled`), the scroll family (`ScrollingDoesBrowse`,
`MouseWheelSpeed`), view-side page rotation, and Q exit. T5 shipped
the reader shell (`reader_window.rs`): session tabs (closable, Tab/
Shift+Tab), undock/re-dock (`D`, one chrome-less `ReaderForm`-style
window), fullscreen chrome hide + reveal strip, MinimalGui (`K`),
fullscreen cursor auto-hide, and reading-state write-back
(`OpenedTime`/`OpenedCount` stamped on open, `CurrentPage`/
`LastPageRead` per turn via `ComicBook::set_current_page`; session
only — the ComicDb wiring is Phase 4's job). T6 shipped the pool-queue
page loads (the private decode worker is gone; `queue_for` →
`ImagePool::add_page_to_queue` fast/slow with AddToTop/bottom, results
via queue callbacks + a `timeout_add_local` pump — ADR-019), the
magnifier (`M`, 200 px lens, zoom 2, cairo rim instead of the C#
glass bitmap), the error page (bundled `ErrorPage.jpg` + cairo text,
`CreateErrorPage` parity), and `cr-image::error_assets`
(`CreateErrorThumbnail` port with the bundled RedCross, unit-tested).
A window-activation focus re-grab fixes the dead-first-keypress race
(also visible under sway).

The user test protocol for UI tasks stays mandatory for Phase 4:
implement, gate (fmt/clippy/test), commit+push, then PAUSE with a
written user test the user runs on their machine with
`cargo run -p cr-app --release --` (the release build matters — debug
decodes ~50x slower). Iterate on failures with evidence before fixes.

### Phase 2 progress (session of 2026-09-03)

T1 done. `cr-engine/src/tokenizer.rs` — scan-based port of the
`ComicSmartListItem.rxTokenizer` regex (fancy-regex rejects the
variable-length lookbehind; the scanner reproduces .NET semantics:
unclosed quotes end at line end, `(?<=\]\s+)` multi-word operator
tokens, backslash-escape quirks). `text.rs` — Escape/Unescape/Intent
(sequential-replace order is load-bearing; NL is "\r\n", the C#
Windows reference). `matcher/spec.rs` — the registry of all 97
concrete matchers (class name + English description + kind); kinds
decide operator lists and argument counts. `matcher/query.rs` —
`Match` string parse (`ConvertQueryToParamerters` +
`CreateMatcherFromQuery`) and render (`ConvertParametersToQuery` +
`ComicSmartListItem.ToString` Name/In prelude).

T2 done. `matcher/book_view.rs` — the ComicBook computed properties
(Shadow* with proposed-name fallbacks, Published clamping, Week
(FirstDay+Monday), ReadPercentage, LanguageAsText ISO table, custom
values). `matcher/text_number.rs` — TextNumberFloat/
ComicTextNumberFloat ("1/2" → 0.5, first-float prefix). `matcher/
eval.rs` — MatcherSet pipeline (And filters, Or appends, Not via
Except), per-item families (string/numeric/date/yesno/manga/custom/
all-properties), series-statistics matchers, the duplicate matcher
with the C# ternary quirk preserved (ADR-013 #1). `matcher/series.rs`
— ComicBookSeriesStatistics (count/page/read/gaps/averages/complete/
last-times). cr-core: ValueMatcher gained `ignore_case` +
`option` (`<Option>` of the AllProperties matcher) and ComicBook
gained `file_is_missing` (`<Missing>`).

T3 done. `smart_list.rs` — evaluation of saved lists (limits
Count/MB/GB, selection Position/SortedBySeries/Random, FilteredIds,
NotInBaseList). cr-core `create_new()` now seeds the C# default list
tree (Library + Smart Lists folder with My Favorites/Recently Added/
Recently Read/Never Read/Reading/Read/Files to update using the
engine-configuration defaults 14/95/10); `CrGuid::new_random()`
(/dev/urandom v4). `sort.rs` — .NET Framework Random port
(`DotNetRandom`, vectors verified against an independent
transliteration of `CompatPrng`), `guid_compare` (LE u32/u16 field
order), series comparer family. Acceptance: the real-world evaluation
test (see gate status above).

T4 done. `queue.rs` — ProcessingQueue port (dedup with callback keys,
AddToTop/Bottom moves, Trim-from-back, claim-inside-lock — ADR-014,
Stop(abort)/graceful drain, no thread aborts). `image_pool.rs` — the
five ImagePool queues with the exact C# names/priorities/sizes/
AddToTop, render chain wired to cr-image (decode → adjust → rotate →
memory+disk caches); `Image::rotate` added to cr-image. `queue_manager.rs` — the ComicBook queues (dynamic update/export/read-info/
write-info with update-threads count).

T5 done. `scanner.rs` — ComicScanner parity: recursive walk honoring
`comicrackscanner.ini` (IgnoreFolder/IgnoreSubFolders), per-file
decisions (existing book refresh; same-name+size recovery for moved
files; new book with defaults + AddedTime), AutoRemove for vanished
files, file-info refresh (size/times/page count).
`watch.rs` — `notify`-based watch folders with debounce (the
`WatchFolder.Watch` flag drives it). `cr-engine` gained `zip` (dev)
and `notify` deps.

T6 done. `backup.rs` — `backup_to` (zip: comment "ComicRack Backup",
`ComicDb.xml` entry + `Thumbnails/*`), `restore_backup` (extracts to
the `.restore` slot), and the full create → destroy → restore flow
test. Fixed a Phase 0 deviation: `.restore` is now `ComicDb.restore`
(C# `DatabaseFile + ".restore"`), not `ComicDb.xml.restore` (ADR-013
#4).

T7 done. `group.rs` — grouper ladders with exact C# captions and sort
keys (date ladder, count buckets, rating groups, alphabet groups,
name groups incl. compressed form), plus `groupers()`/
`compare_by_column()` registry tables. Full per-column groupers are
completed in Phase 4 as the browser consumes them.

### Phase 1 progress (sessions of 2026-09-02)

T1 done except web comics. Provider framework in `cr-io` ported
from `ComicRack.Engine/IO/Provider/`: format registry (`formats.rs`,
deterministic registration order; `.cbr`/`.rar` map to CBR first, both
route to the same accessor), `ExtendedStringComparer` IgnoreCase
natural-sort port (`extended_compare.rs` — this defines page order),
`ComicProvider` (filter + sort page list, `CreateHashFromImageList`
SHA-1/Base32 hash in `hash.rs`), accessors for CBZ/CBT (pure Rust
`zip`/`tar`), CB7/CBR/RAR5 (`sevenzip.rs`, `7z` subprocess, list via
`l -slt` blocks, read via `e -so`), PDF (`pdf.rs`, pdfium-render,
`CalculateSize` port verified: 612x792pt page renders 1920x2484),
DjVu (`djvu.rs`, `djvm`/`ddjvu` subprocess, PPM instead of TIFF
intermediate), and folder comics (FOLDER id 100, recursive). PDF/DjVu
use the whole-file SHA-1 hash and the raw page list (no filter/sort),
per their C# provider classes.

T2 done. `cr-core`: MetronInfo schema + serializer + parser +
`to_comic_info` mapping (`model/metron_info.rs`, byte-stable
round-trip tested; `MetronInfoProvider.ToXml` port including the
RoleValues substring quirks and LocalizeEnum English defaults);
`ComicInfo::serialize_bytes`, `ComicBook::serialize_bytes` (stripped
sidecar form) + `serialize_full_bytes`; `ComicBook::parse_root` for
the `<ComicBook>` root; `is_same_content` chains. `cr-io`
(`info.rs`): load chain xattrs → sidecar (`<file>.xml`, then
extension-swapped) → in-archive (ComicInfo.xml order 0, MetronInfo.xml
order 1 mapped; ComicBook.xml for books), `InfoLoadingMethod`
Fast/Slow; `NtfsInfoStorage` port to xattrs `user.comicrack.ComicRackInfo`
/ `user.comicrack.ComicRackBook` (ADR-006) with skip-on-same-content.

T3 done. `cr-image`: `Image` RGBA8 currency, decode chain
(`decode.rs`: zune-jpeg with the `JpegFile.RemoveExif` APPn-strip
retry (the 32-bit EXIF quirk preserved), png/gif/tiff/bmp/webp via
`image`, jxl via jxl-oxide; HEIF/AVIF/J2K report UnsupportedFormat —
they need system libs, documented gap), `normalize_to_jpeg` (the
`RetrieveSourceByteImage` conversion chain, wired into
`ComicProvider::read_page`), JPEG encode q75. `adjust.rs`: port of
`ApplyAdjustment` — histogram black/white point scan, color matrix
(ROW-vector convention: out_r = r·m00 + g·m10 + b·m20 + m30; the
custom 5x5 matrices transposed relative to GDI+ ColorMatrix),
gamma LUT, sharpen convolution with border preservation. `resize.rs`:
fit-to-box scale (GetScale semantics, scales UP too), filter mapping
(Triangle ≈ bilinear, CatmullRom ≈ bicubic). `thumbnail.rs`:
`ThumbnailImage` port (MaxHeight 512, JPEG q60, FastBilinear,
size+data serialization).

T4 done. `keys.rs` (ImageKey/PageKey/ThumbnailKey with `IsSameFile`,
resource locator `type:\\...` parsing), `memory.rs` (LRU pool with
item + byte budgets, C# defaults 5 pages / 20 thumbs + 5 MB),
`disk.rs` (fresh format: one file per entry, FNV-1a name, header with
key text for verification, atomic writes, index rebuilt by scan).
The ProcessingQueue machinery stays for Phase 2 (QueueManager).

T5/T6 done. `write.rs`: write-back — CBZ/CBT native full rewrite
(same entry order, content identical, temp file + atomic rename),
CB7 via `7z u` subprocess (C# UpdateComicInfos parity), folder direct
files; failure errors surface, not silent. `export.rs`: skeleton
(ExportImageContainer, compression levels, page-order CBZ packing;
parallel/spill/progress open until Phase 5 dialogs). `cr-cli` has
`pages`, `extract` (`--decode`), `thumb`, `rewrite` (verifies only
metadata entries change; never writes when no metadata found —
writing defaults would destroy file metadata), `metron`.

Known gaps / decisions:
- WebComicProvider (`.cbw`, dynamic) is the ONE remaining Phase 1
  item. Measured rationale: it needs the 853-LOC `WebComic.cs`
  (URL template + regex PagePart engine over fetched HTML),
  compositing, HTTP fetch (`HttpAccess.ReadBinary`) and `FileCache`
  interplay. Headless verification needs a small local HTTP fixture
  server; port it as a standalone task (start with the .cbw XML
  config schema and `GetParsedImages`, test with a std TcpListener
  server).
- HEIF/AVIF/J2K page decode returns UnsupportedFormat (needs
  libheif/openjpeg; decide at packaging time). WebP/JXL decode works.
- Subprocess-format tests are gated: `CR_FORMAT_TESTS=1` for 7z,
  `CR_PDFIUM=<libpdfium.so>` for PDF, djvulibre tools on `PATH` for
  DjVu. CI runs them only if the tools exist.
- 7z/DjVu/pdfium binaries are discovered on `PATH` with env overrides
  (`CR_SEVENZIP`, `CR_PDFIUM`, `CR_DJVULIBRE`).
- Missing external tools degrade to an empty page list (C# parse
  try/catch parity), not an error.

### Real-world validation record (2026-09-02)

`tests/realworld/ComicDb.xml` round-trips byte-identically. The first
run found five writer defects that all synthetic fixtures missed. All
fixed: declaration without `encoding` attribute, `xmlns:xsd` before
`xmlns:xsi`, element names `FileModifiedTime`/`FileCreationTime`, no
`<Size>` wrapper in `ThumbnailSize`/`TileSize`, and empty-text elements
serialize self-closing. Full record in `tests/realworld/README.md`.
Do not edit or reformat that fixture; byte identity is the test.

### What exists (cr-core module map)

| Path | Contents |
|---|---|
| `crates/cr-core/src/xml/mod.rs` | `Emitter` — hand-rolled writer that reproduces net48 `XmlSerializer.Serialize(Stream)` byte for byte. Rules in `tests/golden/README.md`. |
| `crates/cr-core/src/xml/reader.rs` | `XmlReader` — token reader over quick-xml. Order-tolerant. Captures unknown elements raw. |
| `crates/cr-core/src/xml/scalar.rs` | `CrGuid` (lowercase "d" form), `CrDateTime` (.NET kind suffixes), `net_f32` (.NET float text). |
| `crates/cr-core/src/model/` | `comic_info.rs`, `comic_book.rs` (+ `values_store` codec), `comic_page_info.rs`, `enums.rs` (macro-generated, exact member names), `bitmap_adjustment.rs`, `comic_name_info.rs`. |
| `crates/cr-core/src/database/` | `comic_database.rs` (load, save with `.bak` rotation, `open_with_fallback` with `.restore` → main → `.bak` → quarantine chain), `list_items.rs` (ComicLists tree, matchers with `xsi:type` passthrough), `display_config.rs` (the `<Display>` subtree). |
| `crates/cr-core/src/registry.rs` | Property registry: C# property name → typed getter/setter on `ComicBook`. Entry point for matchers, columns, remote updates. |
| `crates/cr-io/src/formats.rs` | Format registry (`KnownFileFormats` + `FileFormat`), extension lookup, signatures. |
| `crates/cr-io/src/extended_compare.rs` | `ExtendedStringComparer` IgnoreCase port — defines page order. |
| `crates/cr-io/src/provider.rs` | `ComicAccessor` trait, `ProviderImageInfo`, `ComicProvider` (filter/sort/read/hash), folder accessor. |
| `crates/cr-io/src/accessors.rs` | Zip (`ZipSharpZipEngine`) and tar (`TarSharpZipEngine`) accessors, signature check. |
| `crates/cr-io/src/sevenzip.rs` | CB7/CBR/RAR5 via `7z` subprocess (ADR-007). |
| `crates/cr-io/src/pdf.rs` | PDF via pdfium-render; `CalculateSize` port; JPEG out (q75). |
| `crates/cr-io/src/djvu.rs` | DjVu via `djvm`/`ddjvu` subprocess; PPM→JPEG (q75). |
| `crates/cr-io/src/hash.rs` | `CreateHashFromImageList` (BinaryWriter layout, SHA-1, cYo Base32) + file hash for PDF/DjVu. |
| `crates/cr-io/src/info.rs` | Metadata load chain (xattrs → sidecar → in-archive), xattr store, `InfoLoadingMethod`. |
| `crates/cr-io/src/write.rs` | Write-back: CBZ/CBT native rewrite, CB7 `7z u`, folder files. |
| `crates/cr-io/src/export.rs` | Export skeleton (ExportImageContainer, CBZ packing). |
| `crates/cr-image/src/decode.rs` | Decode chain + `normalize_to_jpeg` + JPEG encode + EXIF-strip retry. |
| `crates/cr-image/src/adjust.rs` | `ApplyAdjustment` port (histogram, color matrix, gamma, sharpen). |
| `crates/cr-image/src/resize.rs` | Fit-to-box scale, filter mapping. |
| `crates/cr-image/src/thumbnail.rs` | `ThumbnailImage` port (512px, JPEG q60, serialization). |
| `crates/cr-image/src/keys.rs` | ImageKey/PageKey/ThumbnailKey. |
| `crates/cr-image/src/memory.rs`, `disk.rs` | LRU pools + fresh-format disk cache. |
| `crates/cr-engine/src/tokenizer.rs`, `text.rs` | `Tokenizer` + `rxTokenizer` scan port; Escape/Unescape/Intent. |
| `crates/cr-engine/src/matcher/spec.rs` | All 97 concrete matchers: class name ↔ description ↔ kind (operators, argument count). |
| `crates/cr-engine/src/matcher/query.rs` | `Match` string parse/render (byte-stable round trip). |
| `crates/cr-engine/src/matcher/eval.rs`, `book_view.rs`, `series.rs`, `text_number.rs` | Matcher evaluation over `ComicBook` sets; computed properties; series statistics. |
| `crates/cr-engine/src/smart_list.rs` | Smart-list evaluation (limits, filtered ids, base lists). |
| `crates/cr-engine/src/sort.rs`, `group.rs` | .NET Random/Guid order, series comparers; grouper ladders + registry tables. |
| `crates/cr-engine/src/queue.rs`, `queue_manager.rs`, `image_pool.rs` | ProcessingQueue port, ComicBook queues, the five ImagePool queues + render chain. |
| `crates/cr-engine/src/scanner.rs`, `watch.rs` | Library scanner (add/move/remove parity) + notify watch folders. |
| `crates/cr-engine/src/backup.rs` | Backup zip create/restore + the `.restore` flow. |
| `crates/cr-core/src/settings/` | The settings layer: `ini.rs` (IniFile), `registry.rs` (typed field tables + `settings_fields!`), `engine_config.rs` (EngineConfiguration), `extended.rs` (ExtendedSettings, the argv switches), `enums.rs` (the C# settings enums), `settings.rs` (the ~120-field Settings + the Config.xml Emitter/reader), `workspace.rs` (the T14 `WorkspaceState` — the `<CurrentWorkspace>` element). |
| `crates/cr-cli/src/main.rs` | `info`, `db-dump`, `db-roundtrip`, `pages`, `extract`, `thumb`, `rewrite`, `metron`, `lists`. |

The UI crate (Phase 3):

| Path | Contents |
|---|---|
| `crates/cr-ui/src/app.rs` | GtkApplication shell: `open` signal file handling, launcher window, error dialog. |
| `crates/cr-ui/src/theme.rs` | CSS provider + the dark/light toggle (`set_dark`), the cairo-view `Palette` (the GTK named colors per draw call), the theme-flip redraw hook (no libadwaita, ADR-004; ADR-025). |
| `crates/cr-ui/src/reader/display.rs` | Pure `ImageDisplayControl` geometry: fit modes (anamorphic tolerance), part grid, binding edges, RTL, rotation, clamped offsets, interpolate, matrix inverse, magnifier zoom premultiply. Unit-tested. |
| `crates/cr-ui/src/reader/continuous.rs` | `ContinuousPageLayout` port: strip geometry, visible-window binary search, anchors. Unit-tested. |
| `crates/cr-ui/src/reader/keys.rs` | The `MainForm.InitializeKeyboard` command table (41 commands, exact key+modifier match, registration order = priority) + dispatch resolution. Unit-tested. |
| `crates/cr-ui/src/reader/page_view.rs` | The reader widget: part-transform cairo drawing, spread composition (`compose_spread`), layout modes, continuous offset-model scrolling, transitions (Fade/LeftRight/TopDown), paper texture, pool-queue page loads (fast/slow queues, completion callbacks + pump), magnifier, error page, navigation/zoom/pan, the full input map. |
| `crates/cr-ui/src/reader_window.rs` | Reader shell: session tabs (closable, Tab cycling), undock/re-dock, fullscreen chrome hide + reveal strip, MinimalGui, cursor auto-hide, reading-state write-back. |
| `crates/cr-ui/assets/papers/` | Paper textures copied from the C# `Resources/Textures/Papers`. |
| `crates/cr-image/src/error_assets.rs` | `CreateErrorPage`/`CreateErrorThumbnail` port with the bundled `ErrorPage.jpg` + `RedCross.png`. Unit-tested. |
| `crates/cr-ui/src/library.rs` | The app session (`Program` statics): the Library open/save/scan wiring, the Settings + engine-config load/save, `apply_edited` (the editor commit + the dirty mark + the debounced file write), the file write-back on the Info Writer worker (`update_book_file_async` enqueue + `run_book_file_write` pure worker part + the 100 ms result pump), list CRUD (new smart list/folder/id list, update, evaluate), QuickOpen lists, the last-export setting, `save_ini_keys` (the ini merge-writer — the theme persistence). |
| `crates/cr-ui/src/browser/shell.rs` | The browser window: navigator + ItemView + reader dock, the header commands, the context menu (open/reveal/edit/update-file/export/remove/properties), the quick search + the composed view filter (`compose_quick_filter`), view/sort/group/filter/scope actions, the Detail column chooser (`popup_column_chooser` — a model-driven PopoverMenu whose submenus ride the `cols.col<id>` actions), the dynamic menu fills (`dyn_fill`), the probe accessors (`state_*`/`toolbar_*`/`browserbar_*`). |
| `crates/cr-ui/src/browser/menubar.rs` | The T3 custom menubar: the pure six-menu table (MenuNode Item/Sub/Sep/Dyn) + the popover widget (one-active-popover state machine, the Designer icon mapping) + the standalone `Dropdown` (`build_dropdown`) + the dynamic fill machinery (`set_dyn_fill`, `refresh_top`, per-slot map hooks) + the `menubar_visible` rule. |
| `crates/cr-ui/src/browser/toolbar.rs` | The T5 reader toolbar: the nine-button strip (prev/next splits, layout/fit/zoom/rotate drops with state text, magnifier/fullscreen, Tools) + the `Dropdown` tables (PREV/NEXT/FIT/ZOOM/ROTATE/TOOLS); the bar rides the undock (docked home since T9: the tab strip's right host). |
| `crates/cr-ui/src/browser/tabstrip.rs` | The T9 workspace tab strip (`MainView.tabStrip`): Library/Pages/comic-tabs/`+` under the menubar, the comic tabs with async 16 px covers + close + the bold current-slot marker, the right HOST box for the reader toolbar, `tabstrip_visible` (the Fill `flag4` rule, unit-tested). |
| `crates/cr-ui/src/browser/browser_toolbar.rs` | The T6 browser toolbar: the strip in the item pane (Sidebar, Browse prev/next, Views/Group/Arrange drops, right-aligned Quick Search with the scope menu, List Layouts stub, Duplicate List drop) + the `VIEWS`/`SEARCH_SCOPE`/`DUPLICATE` tables and the dynamic `sort_defs`/`group_defs`; the `sync`/`sync_labels` push the action states + the Group/Arrange labels. |
| `crates/cr-ui/src/browser/status_bar.rs` | The T8 status bar (`statusStrip`): the selection-info spring panel (`selection_info` — the C# `SelectionInfo` port, unit-tested), export/write/scan lamps + the data-source light, the book/page/page-count panels, the thumb-size slider (`item_size_range`/`clamp_item_size` in `layout.rs`, unit-tested); the 1 s activity poll (`start_activity_timer`). |
| `crates/cr-ui/src/browser/navigator.rs` | The list tree (Library/Smart Lists/folders/reading lists) with the context menu + the command dispatch. |
| `crates/cr-ui/src/browser/item_view.rs` | The book grid: view modes, sort/group, selection (select_book/reselect), type-ahead, thumbs via the pool queues. |
| `crates/cr-ui/src/browser/pages_view.rs` | The Pages panel: the open comic's page grid, the current-page marker, double-click navigation. |
| `crates/cr-ui/src/reader_shell.rs` | The reader shell: session tabs, undock/re-dock (the T5 toolbar rides via `set_undock_chrome`), bookmark navigation (`bookmark_nav`), page-rotation write-through, fullscreen chrome, reading-state write-back. |
| `crates/cr-ui/src/dialogs/name_prompt.rs` | The name prompt (`SelectItemDialog.GetName` shape): caption + prefilled entry, used by Set Bookmark. |
| `crates/cr-ui/src/dialogs/book_editor.rs` | The book editor (Properties…): Details/Plot/Catalog/Pages/Colors/Custom tabs (the Custom tab is the editable library-key grid — the C# `customValuesData` port), the proposed-value placeholders, the per-page edit menu, the Colors sliders, Apply/OK/Cancel commit points. |
| `crates/cr-ui/src/dialogs/scrape.rs` | The Comic Vine Scraper wizard: the non-modal status window, the engine worker + the request pump, the modal search/series (4-column)/issue pick dialogs over channels. |
| `crates/cr-ui/src/dialogs/scrape_config.rs` | The Comic Vine Scraper config widgets (`ScrapeConfigWidgets`) shared by the standalone dialog and the Preferences "Comic Vine Scraper" page. |
| `crates/cr-ui/src/widgets.rs` | Small shared widget helpers (`menu_item_button` — the left-aligned context-menu row). |
| `crates/cr-ui/src/dialogs/bulk_edit.rs` | The bulk editor (Edit…): a Set check per field, the common-value cue, only checked fields apply. |
| `crates/cr-ui/src/dialogs/smart_list.rs` | The smart-list editor: Designer (matcher rows/groups — the rule type rides a `btMatcher`-style menu button whose PopoverMenu carries the `CreateComicBookMatchersMenu` All/letter shape over the 97 spec descriptions; operator/value/not combos + the structure menu) | Query (the rendered query text round-trip). |
| `crates/cr-ui/src/dialogs/list_editor.rs` | The list editor for folders (name/notes/combine) and reading lists (name/notes/quick-open). |
| `crates/cr-ui/src/dialogs/export.rs` | The export dialog: target/folder/format/compression/naming/page-format/quality + the flags, the inline progress, the session-persisted last settings. |
| `crates/cr-ui/examples/` | The headless probes: `commands_probe` (69 actions + accels), `menubar_probe` (the T3 bar), `dynmenus_probe` (the T4 fills), `toolbar_probe` (the T5 strip + the dropdown OPEN gate), `browserbar_probe` (the T6 browser toolbar: OPEN gates, the read/scope filters, the column chooser open/height/toggle, the duplicate landing), `navpages_probe` (the T7 navigator/Pages toolbars: the dispatch, the search filter, the expand flip, the Views OPEN + radio), `tabstrip_probe` (the T9 workspace strip: open/close/+/select flows, the Pages visibility, the bold slot, the comic-tab re-click, the reader-click MinimalGui gate), `statusbar_probe` (the T8 bar: defaults, the info line, the slider resize/sync, the page click, the lamp flags, the MinimalGui action; REFUSES a non-isolated XDG), `workspace_probe` (the T14 persistence: the mutate → collect → Config.xml shape → the second-shell restore → the close-path save; REFUSES a non-isolated XDG pair), `menubarvis_probe` (the visibility evidence), `displaysettings_probe`, `smalldialogs_probe`, `icons_probe`, `editor_probe`, `writeback_probe`, `scrape_probe` (the Phase 12 wizard), `scrapeconfig_probe` (the standalone scraper config dialog), `scrapeprefs_probe` (the Preferences scraper page: the default page, the initial-page open, the OK commit). |
| `crates/cr-ui/src/settings/` | The Preferences dialog (`preferences.rs`) + the options builder (`options.rs`, the `FillPanelWithOptions` parity). |
| `crates/cr-ui/src/workspace.rs` | The T14 persistence conversions: the C# member-name strings for the cr-ui display enums, `DisplayOptions` ↔ `DisplayState` (the picked color as `#rrggbb`), the `browser_view_state` readouts mapping. Unit-tested. |
| `crates/cr-ui/src/pages.rs` | The page-entry merge (`merged_page_entries`): the provider count + the stored overlay — the reader and the editor both use it. |
| `crates/cr-ui/src/bitmap.rs` | The cairo surface helpers (RGBA→premultiplied ARGB, the thumbnail-blob split). |

Tests: `crates/cr-core/tests/golden_roundtrip.rs`, `crates/cr-engine/tests/realworld_query.rs` (the Phase 2 gate), `crates/cr-engine/tests/eval.rs`, `crates/cr-engine/tests/queues.rs`, `crates/cr-engine/tests/image_pool.rs`, `crates/cr-engine/tests/scanner_lib.rs`, `crates/cr-cli/tests/cli.rs`, plus the in-crate unit tests (`cr-ui` geometry/continuous/spread suites). Fixtures: `tests/golden/` (read `tests/golden/README.md` before you touch the XML layer).

### How to verify

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo run -p cr-cli -- db-roundtrip <ComicDb.xml>
cargo run -p cr-cli -- db-dump <ComicDb.xml>
cargo run -p cr-cli -- lists <ComicDb.xml>
cargo run -p cr-cli -- info <comic-file>
cargo run -p cr-cli -- pages <comic-file>
cargo run -p cr-cli -- extract <comic-file> <page> -o <out>
cargo run -p cr-app --release -- <comic-file>   # the reader (release build — see Phase 3 lessons)
```

Headless UI smoke tests (this machine): `Xvfb :99` + `GDK_BACKEND=x11
DISPLAY=:99`, screenshot diffs via ImageMagick `import -window root`,
keys via `xdotool key`. Read the Phase 3 probe lessons first — the
key-injection path is unreliable without an X input focus; set
`xdotool windowfocus <wid>` before keys and expect first-key drops.
Committed UI tests are the pure-geometry suites only; screenshots
decide rendering, user tests decide input behavior.

Subprocess-format tests: `CR_FORMAT_TESTS=1 cargo test -p cr-io` runs
the 7z suite when `7z` is installed; PDF needs `CR_PDFIUM=<path to
libpdfium.so>`; DjVu needs the djvulibre tools (`c44`, `djvm`,
`ddjvu`) on `PATH`.

Re-bless the `db-large.xml` snapshot after a deliberate model change: `CR_BLESS=1 cargo test -p cr-core --test golden_roundtrip`. Re-blessing changes fixture bytes. Review the diff before you commit it.

### Remaining Phase 0 work (in order)

1. ~~**Settings port (T2 tail).**~~ DONE in Phase 5 T1 (2026-09-04): `cr-core/src/settings/` (`IniFile`, `EngineConfiguration`, `ExtendedSettings`, `Settings` as Config.xml) with tests; `ComicNameInfo` now reads `OfValues`/the legacy flag from `EngineConfiguration::global()`.
2. ~~**Fresh-DB default lists.**~~ DONE in Phase 2 T3 (`create_new()` seeds the default tree).
3. ~~**MetronInfo mapping (T1 remainder).**~~ DONE in Phase 1 (`cr-core/model/metron_info.rs`).
4. **Phase 0 exit review.** Confirm all acceptance criteria in `docs/phase-0-kickoff.md` (criteria #2 and #3 are already met — see the real-world validation record above). Record anything learned in `docs/decisions.md`. The settings-port wait is over — close this review in the T1 wrap-up.

### Lessons from Phase 0 (do not re-learn these)

- `chrono::NaiveDateTime::MIN` is not .NET `DateTime.MinValue`. Build the min value from year 1 (see `CrDateTime::min_value`).
- fancy-regex rejects variable-length lookbehind (`LookBehindNotConst`). `ComicNameInfo` emulates those with prefix guards (see `last_match_guard` in `model/comic_name_info.rs`).
- .NET `RegexOptions.RightToLeft` means "take the last match". `ComicNameInfo` emulates this with `last_match`.
- The reader treats whitespace-only text as indentation. A whitespace-only element value does not survive a round-trip. This is a documented tolerance.
- The captured .NET reference output had two errors against the C# source: no `<Display />` in list items, and an `ExtraSyncInformation` with 2 of 6 members. The C# source wins. The fixture was corrected; details in `tests/golden/README.md`.
- The real-world database corrected five more writer assumptions. See the "Real-world validation record" above and `tests/realworld/README.md`. When a .NET replica run and the C# source disagree, a real ComicRack file decides.
- Git normalizes CRLF to LF in the upstream repo (`* text=auto`). Never trust checked-out line endings as format evidence. Read the blob or reason from the writer.
- `cr-cli` panics on `Broken pipe` when output goes through `head`. Cosmetic. Fix when you touch the CLI.

### Lessons from Phase 1 (do not re-learn these)

- The C# `IsImageThumbnailFolder` literals include backslashes (".DS_Store\\", "__MACOSX\\") — a plain ".DS_Store" entry passes that filter and is then rejected by the extension check (the .NET `Path.GetExtension` leading-dot rule: ".DS_Store" is all extension).
- .NET `Path.GetExtension` treats a leading-dot filename as all extension (".gitignore" → ".gitignore"). The cr-io port encodes this; extension comparisons carry the dot.
- The cYo `ExtendedStringComparer` sorts "007" before "07" before "7" (equal values: more leading zeros first — total-length tiebreak). It is the page-order spec; see `cr-io/src/extended_compare.rs`.
- The cYo custom 5x5 adjust matrices are ROW-vector (`out = c · M`, additive row 3) — transposed relative to the GDI+ `ColorMatrix` they get stuffed into. The cr-image port applies them directly; do not "fix" the order.
- The C# scale path (`Size.ToRectangle`, default mode) also scales UP — thumbnails of an 8px page are 512px. Only explicit `OnlyShrink` shrinks-only.
- ComicRack CE writes metadata into zip/tar through `7z u` subprocesses; our port preserves the behavior (only metadata entries change) with native zip/tar rewrites instead of the mechanism.
- The ComicBook.xml sidecar is written in the STRIPPED form (`ComicBook.Serialize`) but read with the FULL deserializer (`DeserializeFull`). The xattr streams behave the same way.
- The C# `LoadInfo`/`LoadBook` chain: stored (NTFS ADS → our xattrs) → sidecar (`<file>.xml`, then extension swapped) → in-archive (Fast returns the first hit; Slow prefers in-archive). `DisableNTFS`/`DisableSidecar` are engine options not yet wired (settings port).
- `7z l -slt` blocks and `djvm -l` lines are the two subprocess listing formats; both parse to `ProviderImageInfo` with index 0 (name is the read key). Missing subprocess binaries degrade to an empty page list (C# parse try/catch parity).
- The real `ComicRack` files show `xsd` before `xsi` on roots even though net48 `XmlSerializer` defaults to xsi-first — the Emitter root() is xsd-first by evidence, keep it.
- Clippy pedantry that will bite every new file: `as_chunks::<N>()` over `chunks_exact(N)`, no `format!` without args, no redundant field names, no identity ops in tests. Run `cargo clippy --workspace --all-targets -- -D warnings` before every commit.

### Lessons from Phase 2 (do not re-learn these)

- The query tokenizer's C# regex (`ComicSmartListItem.rxTokenizer`)
  uses a variable-length lookbehind — no Rust regex crate accepts it.
  The scanner in `tokenizer.rs` reproduces .NET semantics: multi-word
  operators (`equals yes`, `is in the range`) arrive as ONE token via
  the `(?<=\]\s+)[\w\s]+` run; `Match` after a `]` is alternation 3;
  unclosed quotes/brackets end at the line end.
- The C# renders the NEUTRAL (English) operator words in queries, not
  the localized list ("equals yes", not "is Yes"). The `Intent` and
  `ToString` newlines: group queries use `\r\n` (Windows), the
  `Name`/`In` prelude uses literal `\n`.
- ComicBook inherits ComicInfo in C#: unknown XML elements land in the
  ComicInfo `UnparsedElements` capture wherever they appear. Matcher
  leaves carry `IgnoreCase="false"` (only when false) and the
  AllProperties matcher carries an `<Option>` element.
- The duplicate matcher's equality has a C# ternary-precedence quirk:
  with a year on either side only the year compares (ADR-013 #1).
  Port the compiled behavior, not the intent.
- `ReadPercentage = ((LastPageRead+1)*100/PageCount).Clamp(1,100)` with
  0 when PageCount/LastPageRead <= 0; `Week` = CalendarWeekRule.FirstDay
  with Monday (week 1 starts Jan 1 at its own weekday).
- The .NET Framework `Random` is fully deterministic and portable —
  ported as `DotNetRandom`; verify vectors against the dotnet/runtime
  `CompatPrng` source, not from memory.
- The real-world DB's `CacheStorage` fields hold the C#'s cached
  evaluation results (comma-separated book ids; `Custom` = a mode that
  persists nothing). They are free ground truth for engine tests — but
  a naive regex over the XML crosses item boundaries; parse the
  fixture with the cr-core model instead.
- `notify`-based watch folders work in CI (inotify present); debounce
  into scan runs, and only watch folders with `Watch="true"`.
- Backup zip: comment "ComicRack Backup", entry `ComicDb.xml`,
  `Thumbnails/*` for custom thumbnails; restore writes the
  `ComicDb.restore` slot that `open_with_fallback` consumes.
- Clippy: `field_reassign_with_default` fires on any
  `let mut x = T::default(); x.field = ...` in tests — build struct
  literals instead. Run the clippy line before every commit.

### Lessons from Phase 3 (do not re-learn these)

- The C# part transform is PART-LOCAL: `DisplayOutput.Create`
  builds the matrix for the part window from its origin, and the
  renderer maps source rectangles (page placements, strip pages)
  through `source − partBounds.origin`. Drawing them in full-image
  coordinates renders every part as the part-0 slice. This bug
  appeared twice (composition placements and the continuous strip)
  before it was understood.
- Continuous mode keeps the WHOLE scroll in part 0's offset:
  `GetClampedPartOffset` clamps against the full image height, not
  the grid row. Never step the part index in continuous mode, and
  restore rebuilds as `part 0 + offset`. Mixing the two models makes
  every layout rebuild snap the scroll back to the top.
- Rebuild the continuous layout only when its inputs changed
  (`ContinuousPageLayout::matches` over sources + content width +
  preserve flag); the content width caches like
  `GetContinuousContentWidth`. The anchor restores from the drawn
  viewport top (part position + offset), which lives in
  `ViewState::continuous_viewport_top`.
- A composed spread is never "landscape" (`IsDoubleImage` parity):
  no auto-rotate, no FlipParts, no paired part grid. Miss this and
  every spread renders rotated 90°.
- `ImageAutoRotate` defaults to FALSE (workspace
  `[DefaultValue(false)]`; `MainForm` toggles it). Do not copy the
  widget field's uninitialized default incorrectly.
- Double-page navigation steps 2 pages per turn while a spread shows
  (`PagingMode.Double`), 1 from a single-page view. Backward uses the
  same step from a spread.
- Forced-double (Double layout, portrait page, no neighbor): the
  page renders at natural aspect in ONE slot — cover left, other
  pages right — the other slot stays background. Never stretch.
- Page types default to Story (ComicBook defaults) until ComicInfo
  page metadata reaches the reader, so `IsSinglePageType`/
  `IsSingleRightPageType` are false; the cover-right behavior comes
  from the C# `flag3` page-0 rule.
- The logical page advances per press while images trail (the C#
  book/display split): the header and navigation counters move at
  once, decodes stream in latest-wins, the display goes blank
  between pages, and a same-page guard must not block the initial
  page-0 load (`open()` calls `request_and_go` directly).
- Transition frames paint their OWN background inside the part rect
  (`RenderImageSafe` parity) — blank slots otherwise bleed the
  previous frame through. In fades the OLD frame fades OUT
  (alpha 1−p); in slides the old frame stays put and the new one
  slides in from the edge (`PageForward`/`PageBackward`).
- gtk4-rs 0.11: stay on the GTK 4.0-era API surface (ADR-018) until
  the CI runner's GTK version is known. `gtk::init()` must run
  before any object construction. GApplication intercepts positional
  file args — register `HANDLES_OPEN` and connect the `open` signal
  (the C# exe-association path) instead of parsing argv.
- glib 0.22 has NO `MainContext::channel`/`glib::Sender`: bridge
  worker results with std mpsc + a `timeout_add_local` poll. Bind
  the `try_recv()` result BEFORE matching — a `while let` scrutinee
  borrow lives through the loop body and collides with the
  handler's `borrow_mut` (RefCell panic).
- Release build matters: debug decodes ~50x slower (1.9 s vs 34 ms
  per page). All user tests run `cargo run -p cr-app --release --`.
- Edition 2021 holds `if`-condition temporaries until the END of the
  whole if/else statement. `if self.state.borrow().x == y { body }`
  panics ("RefCell already borrowed") the moment the body or the
  else-arm borrows again — this crashed the T4 wheel path. Hoist
  every condition borrow into a `let` statement before branching
  (`let v = self.state.borrow().x;` then branch on `v`). Same for
  bodies that call further borrowing methods (pan threshold,
  click dispatch). Sweep: `rg "if (self|view)\.state\.borrow"`.
  The sweep MISSES the scrutinee-ARGUMENT shape —
  `if let Some(g) = hit_group_header(&state.borrow().layout, ...)`
  keeps the borrow through the branch too, and a `borrow_mut` inside
  panicked (the 2026-09-10 group-double-click crash). A borrow hides
  anywhere in the condition/scrutinee EXPRESSION, not just as the
  direct condition; hoist those the same way (the fixed shape:
  `let group_hit = { let s = state.borrow(); hit_group_header(&s.layout, ...) };`).
- Headless smoke tests: Xvfb + `import -window root` screenshot
  diffs, `xdotool key`. `xdotool click 4/5` does NOT produce scroll
  events under GTK/X11 (the wheel path is only user-testable). GTK
  apps crash with "GTK has not been initialized" if `theme::init`
  runs before `gtk::init()`.
- xdotool key delivery is the fragile part of every probe. XTEST
  (`xdotool key <key>` with NO `--window`) + `xdotool windowfocus
  <wid>` is the working combination; `key --window` (XSendEvent) and
  key presses before the toplevel is active silently reach nothing —
  the widget logs prove it. A restarted WM-less Xvfb may report
  `XGetInputFocus = 1`; keys stay dead for EVERY binary (T5 and T6
  binaries both failed until `windowfocus` + the is-active fix) —
  suspect the probe first, bisect with `git stash` before blaming
  code.
- GTK4 has NO click-to-focus and grab_focus is ignored while the
  toplevel is inactive. The reader re-grabs focus on the window's
  `is-active` notify (both the main window and the undocked one).
  Any future widget that must take keys on window activation needs
  the same hook (this was the sway "first keypress dead" bug).
- The zoom commands anchor at the part-bounds center (`ImageZoom`
  setter parity); the magnifier composes as `mat.append(zoom_about(
  cursor))` (`Mat::premultiply_zoom`) — the zoom applies OVER the
  part matrix, then the whole scene re-renders under a circular
  clip. The lens reuses `draw_composition`/`draw_continuous` with
  the premultiplied display; `DisplayOutput` is `Clone` for exactly
  this.
- Page loads ride the ImagePool queues (ADR-019): the queue callback
  renders and ships a `PageDone` over std mpsc; the UI pump drains
  it. The completion payload carries the comic source string so
  stale results from a previous open are dropped. Failure =
  `image: None` → the error page surface (a `thread_local` cache —
  cairo surfaces are not Send/Sync, no `OnceLock`).
- `ProcessingQueue` callbacks must be `Fn + Send + Sync`; std
  `mpsc::Sender` is not Sync — wrap it (`PageTx(Arc<Mutex<Sender>>)`).
  The slow queue runs several workers.
- cairo ImageSurface from RGBA needs ARGB pre-multiplication and the
  surface's stride, not `width*4` (`image_surface_from_rgba`).
- The cr-cli `pages` JSON is the ground truth for page indexes when
  a test needs "which entry is page N" (natural sort of full entry
  names; archives without a `00000` cover shift by one).
- `MemoryPool::get` (cache-hit without produce) was added for the
  C# `GetPage(onlyMemory)` ordering; `ImagePool::render_page` checks
  the pages pool before any provider work.

### Lessons from Phase 5 (do not re-learn these)

- GtkDialog dialogs that call `dlg.close()` inside a response arm
  get a RE-ENTRANT delete-event response (the C# WinForms
  `DialogResult` shape does not do this). Every dialog that ends
  in close() needs a one-shot `done` Cell guard, or the Cancel
  path runs twice (the fresh-insert removal bug).
- The provider-count + stored-overlay merge
  (`cr-ui::pages::merged_page_entries`) is THE open semantics:
  PageCount always comes from the PROVIDER and the stored page
  entries overlay it. A partial metadata list must never shrink
  the display. The reader sequence + the editor both consume it.
- The page-queue completion payload must carry the DISPLAY
  position, never the key's provider index — under a display
  sequence they differ, and reporting the key's index scrambles
  every landing slot (blank pages).
- The pool's thumbnail blob carries the `ThumbnailImage`
  serialization header — parse (`Thumbnail::from_bytes`) before
  decoding. Shared helper: `cr_ui::bitmap::surface_from_thumb_blob`.
- Env-gated `eprintln!` probes (`CR_DEBUG_SL`) + an isolated-XDG
  app run is the fastest way to get commit-path evidence. ALWAYS
  rebuild `cr-app --release` before probing — a stale release
  binary cost a full round once.
- Verify scripted multi-line edits by grepping the NEW symbol in
  the changed file, not by the build result — a drifted target
  text matches nothing and the build still passes (the editor's
  missing merge half).
- GTK widgets are the single source of truth for dialog fields:
  write picker results INTO the widget and let the widget's sync
  write the model — a direct model write gets overwritten by the
  next widget sync (the export folder chooser).
- `FileChooserNative`/`MessageDialog` internals: a MessageDialog's
  message_area is reachable via
  `child().and_downcast::<Box>().and_then(|v| first_child()...)`.
- The C# `EditListDialog` routes FOLDERS and READING LISTS from
  the single Edit menu item; the C# `ListEditorDialog` is an
  UNRELATED workspaces editor — do not port it for lists.

### Lessons from Phase 7 (do not re-learn these)

- INCIDENT (2026-09-06, user-reported): deleting the imported
  fileless placeholder books with "Also delete the files" ran
  `gio trash ""` — gio resolves an EMPTY argument to the process's
  CURRENT WORKING DIRECTORY and trashed the whole repo checkout.
  Everything survived in `~/.local/share/Trash/files` (one trashed
  copy held the git-ignored test comics + the user's real `.cbl`
  exports — restored with `cp -n` from the trash copy). The fix: the
  remove flow only trashes a path that is non-empty AND an existing
  FILE (`p.is_file()`); fileless books never touch the trash. RULE:
  never hand a book-derived path to `gio trash` without the
  is-file check — fileless books carry `file_path = ""` everywhere.
  AUDIT (the user's follow-up): the ONLY user-data deletion in the
  app is the remove flow's `gio trash` (+ the reveal `xdg-open`,
  now empty-guarded too). Folder comics (a directory `file_path`)
  are SKIPPED by the port's is_file check — a recorded deviation
  (the C# trashes the folder via ShellFile.DeleteFile). The C#
  itself guards with `IsLinked` (ComicBook.cs:1321) — the empty
  check is parity, is_file is defense. Every other destructive
  operation touches only app-controlled paths (the DB's .bak/.rest-
  ore, the `.tmp` write sibling, `*.cache` pruning, test temp dirs)
  — the full table lives in the Phase 7 kickoff incident record.
- The C# `OnGetBooks` for `ComicIdListItem` walks `BookIds` in LIST
  order — a reading list displays in its stored order, not the
  library order. The port's IdList evaluation must walk `book_ids`
  first-seen (HashSet dedupe parity).
- The ComicNameInfo rxNumber RightToLeft emulation: the C# RTL scan
  takes the match with the rightmost START; a left-to-right
  find_iter + last-item is wrong when a leftmost candidate overlaps
  the real one ("Watchmen 001" → "chmen 001"). The year/get-number
  stages have no overlapping candidates and keep the cheap
  emulation.

### Lessons from Phase 5.5 (do not re-learn these)

- When a UI symptom appears at "some later time", find the GTK
  mechanism that SCHEDULES work later — before writing any
  watcher. The right-click scroll jump (2026-09-06) took two hack
  rounds (a synchronous compare-and-restore, then a 10-idle-turn
  poll) before the header read: the `ScrolledWindow`'s
  `GtkViewport` has scroll-to-focus ON by default (GTK 4.6+,
  `gtk_viewport_set_scroll_to_focus`), so every `grab_focus` on a
  full-content canvas scrolls to y=0. The proper fix was ONE
  boolean in `ItemView::create` (`set_scroll_to_focus(false)`;
  the grid's own scrolling moves the adjustment directly and never
  relies on focus). Both watcher hacks are ripped out; the full
  mistake record lives at the end of `docs/phase-4-kickoff.md`.
- A window-parented CONTEXT popover on Wayland must be a PLAIN
  `gtk4::Popover` (the book-context-menu shape), NOT a
  `build_dropdown` one. The `build_dropdown` popover (has_arrow
  off + submenu child popovers) is ANCHOR-parented by design; it
  maps only from a widget anchor, and fails to MAP when parented
  to the top-level window on Wayland (X11 tolerates it, so Xvfb
  probes cannot catch this — the `CR_DEBUG_CHOOSER` trace on the
  user's machine showed `header_hit=true` + the popup call firing
  with nothing appearing; the T6 column chooser). A scroller
  inside a popover needs BOTH `propagate_natural_width` AND
  `propagate_natural_height`, else it collapses to ~2 rows.
- A stateful `SimpleAction` whose handler only `set_state`s does
  NOT re-render the custom menubar/toolbar rows (they render from
  the `sync` resolve closure, driven by `sync_enabled`). Every
  stateful handler must call `sync_enabled` after `set_state`, and
  radio state should derive from the SOURCE OF TRUTH in the sync
  (e.g. `view-mode` from `item_view.mode()`), not be trusted from
  the click parameter (the T6 "the check never moved" bug).
- The reader toolbar (`mainToolStrip`) is visible in BOTH the
  browser and reader views (the C# `OnGuiVisibilities` keeps
  MainToolStripVisible in Fill mode); only MinimalGui hides the
  whole bar, and `OnUpdateGui` gates only prev/next/layout/fit/
  zoom/rotate/magnifier on an open book — Fullscreen/Tools always
  show. Mount it above the view stack (not inside the reader page)
  so the library view keeps Tools/Fullscreen.

- GTK accelerators match the PRODUCED keyval. Shift rewrites the
  symbol on most layouts (Shift+4 → '¤'/'$'), so accels like
  `<Alt><Shift>4` or `<Control><Shift>7` never fire — while the
  C# matched WinForms VIRTUAL keys (layout-independent). Fix: the
  window key controller in `browser/shell.rs::
  install_shifted_key_fallback` + the pure
  `commands::shifted_symbol_command` — resolve the keycode's
  UNSHIFTED keyval via `gdk_display_map_keycode` (level 0) and
  fire only when the raw keyval DIFFERS from the unshifted one
  (layouts where Shift keeps the symbol stay on the real accel —
  no double-fire). Shifted LETTERS need no fallback (GTK matches
  letter-case variants). `<Control>equal` is zoom-in's primary
  spelling; `<Control>plus` is the numpad.
- Register radio accels through DETAILED action names
  (`win.page-fit::<value>`), not the bare stateful action.
- `gtk_application` accels for `win.*` resolve only in windows
  that insert the `win.` group — the undocked reader window gets
  NO shell accels (ReaderForm parity, no menubar there).
- A disabled `SimpleAction` swallows its accelerator — the
  enable-state sync is what keeps stale accels inert.
- The reader's own HeaderBar is UNPARENTED in the docked shape —
  toggling it hides nothing. Docked chrome changes go through
  `reader_shell.rs::apply_chrome_visibility`, which reaches the
  HOST window's titlebar. The fullscreen state of a reader view
  must come from the view's ROOT window (an undocked reader
  fullscreens its own window, not the host).
- In a headless probe, NEVER blanket-activate every shell action:
  the `restart` action spawns the binary and self-perpetuates.
  Skip side-effecting commands (`restart`, `quit`) and the
  parametered radio actions.
- Xvfb screenshots come back all-black for GTK4 windows on this
  machine now (GL/DRI3; `GSK_RENDERER=cairo` did not help). The
  probe log lines are the evidence; the user test decides
  rendering.
- A probe dwell window MUST be an `ApplicationWindow` built with
  `.application(app)` (the shell's builder pattern). A plain
  `gtk4::Window` holds nothing — the GApplication loop exits the
  moment activate returns, before any timeout fires; `app.hold()`
  did not rescue it (the icon probe needed the ApplicationWindow
  to reach its 1.5 s screenshot dwell).
- On Wayland NEVER present a second popover while one is open —
  the autohide popup only maps when no other grabbing popup is up
  (`can_map_grabbing_popup`, `gdkpopup-wayland.c`); a failed map
  leaves the seat grab LIVE and the whole window goes unclickable
  (the "non-top most parent" warning spam). Port the
  `GtkPopoverMenuBar.set_active_item` shape: one active slot,
  popdown-ALL-others-then-popup, close clears the slot. X11
  tolerates parallel popovers, so Xvfb probes CANNOT catch this —
  the user test is the only evidence.   Grab-focus-on-map belongs
  in an idle, not the map callback (it runs inside the grab
  setup).
- `gtk_widget_activate_action` (and gtk4-rs's `activate_action`)
  resolves actions through action GROUPS — pass the FULL detailed
  name (`"win.next-page"`, radio `"win.page-fit::original"` +
  the value as the explicit parameter); a stripped bare name
  matches no group and fails SILENTLY. Accels keep working while
  every widget-path click dies (the T3 round-2 bug: Ctrl+N turned
  pages, the menu item did nothing) — gate the row-click path in
  a probe (`MenubarWidget::click_row` walks the real handler).
- Probe handles that clone a widget struct must SHARE the
  widget-holding fields (`Rc<Vec<ItemRow>>`), never copy-with-
  empty — a `clone_handle` that drops the rows silently loses
  click AND sync (the menubar probe needed it).
- A probe that builds a shell MUST keep the shell object alive
  for the whole run (`std::mem::forget(shell.clone())` or the
  app's thread-local pattern): every shell action handler holds
  `Weak<ShellState>`, and a dropped shell turns each dispatch
  into a SILENT NO-OP — no panic, no log, actions "registered"
  but dead (the T3 round-3 probe spent hours on fake evidence
  because the probe dropped its BrowserShell after activate).
- stdout/stderr interleave UNRELIABLY when piped (stdout is
  block-buffered, stderr is not) — debug prints that must be
  order-compared go through println! on ONE stream.
- A popover needs a PARENT (a widget inside a toplevel) BEFORE
  popup() — an unparented popover realizes nothing and the present
  segfaults (`gtk_widget_realize() on a widget that isn't inside a
  toplevel` → `gdk_surface_new_popup: no parent surface` → SIGSEGV;
  the T5 toolbar dropdowns). Parent to the ANCHOR BUTTON, not the
  window, when the widget can re-parent across toplevels (the
  undock). A probe that only CLICKS rows never exercises the
  present path — gate the OPEN (`popover.is_mapped() == true`
  after `open()`), not just the click.
- `activate_action` with a DETAILED name AND an explicit parameter
  errors silently (`Gtk-CRITICAL ... detailed action name ... in
  conjunction` — the T4 probe caught the radio rows dead on
  click): the detailed form parses only WITHOUT args; pass the
  BARE name + the variant parameter.
- Revisit-within-an-open-menu: a dropdown submenu re-fills on its
  OWN popover map (the child-popover `connect_map` hook) — the
  top-menu open funnel never fires for a nested revisit (the T4
  stale-check finding).
- A probe counter that walks only the FIRST top-level subtree
  silently undercounts (the T7 expand-all gate read 0 for a
  folder sitting on row 2): every model walk needs the outer
  sibling loop + recursive children — never a single-root stack.
- Wheel/input paths have NO probe gate (xdotool produces no GTK
  scroll events) — wire them, then the user test decides (the T7
  Ctrl+wheel round: the handler existed but nothing called it;
  grep the CALLER, not the method).
- Dropping a Rust widget handle does NOT unparent a GTK widget —
  the parent holds its own ref, so a "removed" item stays visible
  until an explicit `parent.remove(child)` (the T9 X-close bug:
  the strip's retain looked right while the tab widgets piled up;
  gate slots == WIDGETS, not just the model vec).
- The workspace stack (and every Box page under it) needs vexpand —
  without it a stack page collapses to its toolbar's height (the
  T9 Pages report). The probe gates stack-h == pages-h, measured
  one frame AFTER the switch (an allocation read in the SAME tick
  reads 0/stale).
- Probes run UNSTYLED unless they call `cr_ui::theme::init()`
  themselves — the app loads the CSS in `app::run` only, so every
  CSS-dependent gate (tab boxes, compact buttons, heights)
  measured theme defaults until the probe loads it (the T9 height
  round: 48 px was the unstyled number, 36 px the real one).
- A cairo-drawn view does NOT restyle on a theme flip: GTK never
  invalidates a custom draw, and hardcoded palettes ignore the
  mode entirely (the dark/light toggle needed two rounds). The
  shape: resolve `theme::palette(widget)` PER DRAW CALL through
  the widget style context (the GTK named colors — the
  `SystemColors` parity; no cache to invalidate) and hook
  `theme::redraw_on_theme_change` on every drawn canvas. Any new
  DrawingArea that paints colors needs both, or it keeps the old
  theme's look until the next unrelated redraw.
- A headless probe drives MODAL dialogs programmatically: find the
  toplevel by a TITLE PREFIX (`find_toplevel` — the Quick Rating
  title carries the caption, so exact titles never match), walk the
  widget tree (`first_child`/`next_sibling` — no container-type
  assumptions) to reach the SpinButton/Scale, then `Dialog::
  response(Ok)` walks the real response path. The dialog classes
  are gate-free (pure helpers carry the unit tests): the Tasks
  snapshot (`pending_tasks`) and the zoom clamp (`clamp_percent`)
  test without GTK; the queue workers drain a no-op callback
  instantly in tests — `queue.stop(true)` first, then add items.
- `ProcessingQueue::stop(true)` also works as a TEST tool (the
  workers exit; items stay queued for a snapshot) — after a stop,
  every state reads Waiting (the Running rule needs an active
  queue; test it as a pure function instead).

### Lessons from the scan session (2026-09-10, do not re-learn these)

- A build script that emits ANY `rerun-if-*` must declare EVERY
  input that can move its output — including the git state a dev
  build reads. `cr-ui/build.rs` declared only
  `rerun-if-env-changed=VERSION` and the stamped commit count froze
  at the last full rebuild (0.0.233 rode along through 70+ commits);
  the fix watches `--absolute-git-dir` refs (HEAD + packed-refs +
  refs/heads/<branch>).
- A "did X happen" probe must settle past every boot-time deferred
  fill before it dispatches — the navigator's boot selection
  evaluates 200 ms after create (SELECT_DEBOUNCE_MS), and an
  immediate dispatch passed the scan-land gate against the UNFIXED
  code through that race. Settle (600 ms) first, then gate.
- A per-batch view refresh is O(N²) per tick: 114 refreshes of a
  10k-book view in one pump tick starved the main loop. The shape:
  collect the tick's batches, extend the DB once, fire the view
  hook ONCE per drain. The incremental append
  (`ItemView::append_books` — extend + ONE rebuild, per-book caches
  KEPT) measures 0.7-4.7 ms/tick at 10k books; a full `set_books`
  per tick (caches cleared) is the visible glitch.
- A scan's done callback must run BEFORE the queue pop: the next
  scan takes the storage, and a refresh after the take evaluates
  "0 books" and wipes the view between roots (the F-gate trace:
  "evaluate 10002 books" immediately followed by "evaluate 0
  books").
- Exit paths STOP the scan first (`library::save()` aborts an
  in-flight scan before the pump wait — the C# Scanner.Stop
  order); waiting for a long scan hangs the window, and saving
  mid-scan without the stop would write the taken, empty book list.
- The C# `FileUtility.GetFiles` walk is a lazy generator — walk and
  process interleave. An eager collect-then-process walk is a
  silent window on huge trees; the lazy `walk_files` shape (per
  folder: sorted files first riding progress + the processor, then
  the subfolders) is the parity shape and gives live Tasks-line
  movement.
- A gated mid-scan scenario needs a fixture that is SLOW enough to
  observe: zero-byte comic files scan between two poll ticks
  (~0.05 ms/file in release); real one-page zips stretch the walk
  (~0.3 ms/file). And an abort gate must have the fixture ADD
  books — an abort of a RE-scan returns the full taken set, which
  passes the assertion without proving anything.

### Lessons from the group/lamp session (2026-09-10, do not re-learn these)

- `ItemView::set_books` replaces the whole `ViewState` — EVERY piece
  of live view config that must survive a refresh must be cloned out
  and re-applied (the filter was; the GROUPER was not, and every
  list refresh silently ungrouped the view). When adding state to
  `ViewState`, audit `set_books` in the same change.
- The C# `GroupHeaderInformation` keeps `Items` attached to a
  COLLAPSED header (`ItemCount` shows the true number); the port
  drops collapsed items, so the header needs its own `count` field —
  never derive the header count from the placement list.
- The C# fires the group single-click toggle on BOTH MouseUps of a
  double-click before `DoubleClick` runs, so the all-toggle reads the
  clicked header's ORIGINAL state (net: every group takes the
  opposite of the original). A port that fires the toggle once per
  press must apply the POST-first-click state in the n=2 handler —
  its negation is the inverted direction.
- A popover cannot MAP from a hidden parent: the scan-lamp gate must
  leave the lamp visible before `popup()`, and the 1 s activity poll
  re-hides an idle lamp on the whole-second marks — keep the probe
  reads clear of the second marks.
- A probe press-sequence gate needs one press per tick (a
  one-shot timer installs the repeating machine) — the draw between
  presses re-records the arrow zones; a repeating timer with a LONG
  interval does not start the machine early, and presses in ONE tick
  see zeroed zones.
- The app never set `groups_visible` — a config flag with a correct
  default that nothing writes is a dead feature; when a C# property
  gates a render (`AreGroupsVisible`), wire the port's setter path
  in the same task that adds the draw.

### Lessons from the detail-view session (2026-09-10, do not re-learn these)

- GTK `GtkPopoverMenu` custom-page contract (proven at
  gtkmenusectionbox.c): a model item with BOTH a `custom` attribute
  and a submenu link takes the SUBMENU branch — the page is built
  from the LINKED model (empty = an empty page) and named by the
  LABEL, and `add_child` returns false: the custom-slot hash fills
  ONLY for items WITHOUT a submenu link (custom widgets are INLINE
  slots, never submenu pages). Nested check menus = REAL model
  submenus with the rows bound to stateful actions; the tall pages
  scroll in the popover's OWN outer scroller (its child is a
  ScrolledWindow wrapping the page stack — gtkpopovermenu.c).
- `GtkPopoverMenu:visible-submenu` is READWRITE and drives the page
  stack headlessly — set it to the submenu LABEL and count the
  stack page's GtkModelButton rows (the probe seam behind
  `state_column_chooser_page_rows`; the title button rides each
  page, so the counts are rows + 1).
- A GTK range write (`set_range`) that CLAMPS the current value
  fires `value_changed` — every programmatic range/value write must
  sit inside the sync guard, or the handler re-enters the widget
  with the clamped value (the T8 slider slam that left
  `ItemRowHeight=48` in saved workspaces).
- The C# `ContextMenuBuilder.Create(20)` is ONE builder for three
  menus: the column header menu (`CreateHeaderMenu` — visible
  columns at top level), the smart-list matcher menu
  (`CreateComicBookMatchersMenu` — all entries `topLevel: false`,
  so no top-level defaults; the "Recent" rung is usage-tracking
  data the port does not keep). The pure structure lives in
  `columns::chooser_menu` (unit-gated: A-B/C-F/G-O/P-R/S/T-Y over
  the full column table) — reuse it for any new Create(20) menu.

### Blockers / open questions

None. The real-world database is committed under `tests/realworld/` with user permission (see `tests/realworld/README.md`; remove it first if the repo ever goes public).

Repo hygiene: `tests/testfiles/` is the designated home for
user-supplied test comics. It is git-ignored — NEVER commit its
contents (invariant: never commit user library data). One 42 MB
comic was briefly committed by accident in Phase 3; the history was
rewritten the same day (`filter-branch` index-filter, force-push,
local gc) and the blob is gone from the remote. The file stays
local-only for user tests. Caveat: the Gitea server may retain the
old pack objects until its own GC runs; if that ever matters, run
the server-side GC.

---

## Reference codebase (THE SPEC)

- **Local checkout:** `/home/scuttle/Downloads/repo/ComicRackCE` (if this path is stale, locate the checkout and update this file)
- **Upstream:** https://github.com/maforget/ComicRackCE (branch `master`)
- **Golden rule:** the C# source is the specification. Before you implement any behavior, **find and read the corresponding C# code**. Never guess from names, screenshots, or memory of "how ComicRack works".
- **Decompiled caveat:** decompilation produced the reference. Expect dead `using`s, odd names, swallowed exceptions, and dead code. Example: `UseWPF=true` is vestigial. There is no WPF. The target is *behavior*, not style.

### Source project map

| C# project | LOC (.cs) | Role | Key files to know |
|---|---|---|---|
| `ComicRack` | ~59,900 | Main WinForms app: shell, ~50 dialogs, views | `MainForm.cs` (4,576 — the orchestrator), `ScriptUtility.cs`, `Config/DisplayWorkspace.cs`, `Dialogs/ComicBookDialog.cs`, `Dialogs/PreferencesDialog.cs`, `PackageManager.cs` |
| `ComicRack.Engine` | ~46,900 | Core engine (≈85-90% UI-free): providers, DB, matchers, caches, sync, remote | `ComicBook.cs` (3,076), `ComicInfo.cs` (1,594), `Database/ComicDatabase.cs`, `Database/ComicLibrary.cs`, `DatabaseManager.cs`, `IO/Provider/*` (readers/storage), `Metadata/ComicBook/Matcher/*` (76 matchers), `ComicNameInfo.cs` (filename parsing), `QueueManager.cs`, `IO/Cache/ImagePool.cs`, `IO/DiskCache.cs` lives in cYo.Common, `IO/Network/ComicLibraryServer.cs` |
| `cYo.Common` | ~37,400 | Base lib: imaging (GDI+), threading, IO, text, XML, Win32 interop | `Drawing/ImageProcessing.cs` (1,557), `Drawing/BitmapExtensions.cs`, `Threading/ProcessingQueue.cs` (460), `IO/DiskCache.cs` (623), `Xml/XmlUtility.cs`, `Compression/SevenZip/*` (COM interop), `Text/Tokenizer.cs` |
| `cYo.Common.Windows` | ~30,200 | The custom WinForms control toolkit (~50 controls) | `Forms/ItemView.cs` (4,770 — the browser list), `Forms/TabBar.cs` (1,926), `Forms/ScrollControl.cs`, `Forms/SizableContainer.cs`, `FormUtility.cs` (reflection-driven options UI) |
| `cYo.Common.Presentation` | ~8,600 | Renderer abstraction (GDI/OpenGL), Ceco XHTML text engine, panels/overlays | `Tao/ControlOpenGlRenderer.cs`, `Tao/TextureManager.cs`, `Ceco/*` |
| `ComicRack.Engine.Display.Forms` | ~7,700 | The book reader controls | `ImageDisplayControl.cs` (2,632), `ComicDisplayControl.cs` (3,514), Engine `Display/ComicDisplay.cs` (2,018) |
| `ComicRack.Plugins` | ~1,900 | IronPython 2.7 scripting host | `PluginEngine.cs` (244), `PythonCommand.cs`, `PythonPluginInitializer.cs`, `PluginEnvironment.cs` (the `ComicRack` object scripts see) |

### Key formats/locations in the reference

- Library database: single XML at `%APPDATA%\cYo\ComicRack Community Edition\ComicDb\ComicDb.xml`. See `Engine/SystemPaths.cs` and `DatabaseManager.cs` (`.bak`/`.restore` rotation, corruption fallback).
- Localization: `ComicRack/Output/Languages/<lang>/*.xml`. **19 languages, reused as-is.** Lookup pattern: `TR.Load("FormName")["Key", "Default"]`.
- Sample scripts: `ComicRack/Output/Scripts/*.py`
- Reader paper textures: `ComicRack/Output/Resources/Textures/Papers`

---

## This repo

Crate layout (seven crates; `cr-script` was removed with ADR-027 — the scripting host is dropped; `cr-engine`, `cr-ui`, and `cr-app` are active):

| Crate | Contents |
|---|---|
| `crates/cr-core` | Data model (ComicBook/ComicInfo/MetronInfo/PageInfo), ComicDb.xml serde, settings, filename parsing |
| `crates/cr-io` | Comic providers (zip/tar/7z/rar/pdf/folder/web), ComicInfo.xml read/write-back, archives |
| `crates/cr-image` | Image currency type, decode/encode pipeline, resize/adjust filters, page/thumbnail caches |
| `crates/cr-engine` | Smart-list parser + matchers, queue manager, scanner, watch folders, backup, sync, remote server |
| `crates/cr-ui` | GTK4: reader (GtkGLArea), ItemView browser, shell, dialogs, theming, i18n |
| `crates/cr-cli` | Headless verification tooling (`info`, `db-dump`, round-trip) |
| `crates/cr-app` | Main binary: D-Bus single instance, app wiring, packaging |

---

## Compatibility invariants (DO NOT BREAK)

1. **ComicDb.xml read/write.** Element and attribute names, casing, and structure must match the C# `XmlSerializer` output exactly (the ComicLists tree, the custom values store). Golden-file round-trip tests verify this. The database is the one artifact users cannot lose.
2. **Metadata schema compat:** `ComicInfo.xml` (Anansi standard), ComicRack's `ComicBook.xml`, and `MetronInfo.xml` — read AND write in-archive.
3. **Smart-list query language** must parse and match identically (saved lists contain these queries — see `ComicSmartListItem.cs` and `ComicBookGroupMatcher.cs`). The `Expression`/plugin-list matchers parse and render byte-stably; they evaluate to an explicit not-supported result (ADR-027 — no scripting host).
4. **Caches are disposable. The database is not.** Thumbnail/image caches (`DiskCache` `cache.idx`, BinaryFormatter-serialized) have NO compat requirement. Design fresh formats freely.

---

## Critical gotchas

- **unrar license is GPL-incompatible.** Use subprocess/7z or libarchive for RAR. Never static-link unrar.
- **No scripting host (ADR-027).** The IronPython plugin ecosystem is not ported; scripts hit the WinForms/clr wall on CPython, and the used-script set ports natively instead. Saved queries carrying `Expression`/plugin matchers parse and render byte-stably but evaluate to an explicit not-supported result.
- **The WCF net.tcp remote protocol is NOT preserved** (see `docs/decisions.md`). Android app protocol compat was explicitly dropped.
- **NTFS ADS metadata → Linux xattrs** (`user.comicrack.*`) with sidecar fallback.
- **Reflection-based property access by string name** is load-bearing in the C# (matchers, columns, remote `UpdateComic`, `FormUtility` options panels). Rust needs an explicit property registry for this. Plan for it early in `cr-core`.
- **Windows paths are baked into user data** (workspace paper textures point at install paths). Be lenient when you load.
- **Tao.OpenGL is legacy GL.** The reader targets GL 3.2 core via `glow`, with a cairo fallback first (the C# app itself falls back to GDI+).
- **32-bit JPEG EXIF quirk** in the decode path (`BitmapExtensions.BitmapFromBytes`). Preserve the fix.
- Localization is data-driven per widget name. Port the `TR` lookup. Do not gettext-ify.

---

## Verification workflow

- CI: Gitea Actions. All three workflows run on the `docker-runner-amd64` runner inside the `comicrust-ci` container image (`.gitea/container/Dockerfile`; build it on the runner host — apt-get needs `--no-install-recommends`, rustup's `--component` takes a comma-separated list, and the image ships nodejs because the runner execs JS actions like `actions/checkout` with the job container's own node). `ci.yaml` runs fmt, clippy, and tests on every push to main (`CR_FORMAT_TESTS=1` — the image ships 7z and djvulibre). `release.yaml` builds `cr-app` in release mode and republishes the single `rolling` prerelease: version `0.0.<total commits on main>`, assets `comicrust-<version>-linux-amd64.tar.gz` + `.sha256` (binary renamed `comicrust`, plus `assets/papers/`). `tagged-release.yaml` runs on manual dispatch with a tag input (create the tag first, e.g. `v0.1.0`): a checks job gates a stable release for that tag, version taken from the tag. Both release workflows publish through `.gitea/publish_release.sh` (rolling deletes the tag so it follows main; the tagged release never touches the tag). See ADR-020 and ADR-021.
- `cargo fmt --check` and `cargo clippy -- -D warnings` must pass before every commit.
- `cargo test` — golden-file round-trip tests for ComicDb.xml are the phase gate for Phases 0-2 (see `docs/phase-0-kickoff.md`).
- `cr-cli` subcommands (`info <file>`, `db-dump <ComicDb.xml>`) are the manual verification tools against real data.
- **Never commit user library data.** Golden test files must be synthesized or anonymized fixtures under `tests/golden/`.

## Conventions

- Commits: imperative mood, concise subject (`Add ComicDb.xml round-trip test`).
- Add decisions to `docs/decisions.md` as new ADRs. Append only. Language-only rewrites (ASD-STE100) are allowed.
- Commit and push all changes after each completed task (see Agent working rules).
- Update the **Current status** section at the top of this file every session.
