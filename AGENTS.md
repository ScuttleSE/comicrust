# AGENTS.md — Agent Onboarding

You are working on **comicrust**. This project is a from-scratch port of **ComicRack Community Edition** (a Windows C# WinForms comic library manager/reader). The target is a **Linux-native Rust + GTK4 application** with **full 1:1 feature parity**.

Read this file first. Then read `docs/port-plan.md` (architecture and roadmap), `docs/decisions.md` (locked decisions), and the current phase's kickoff doc (`docs/phase-<N>-kickoff.md` — the status section below names the active one). Do not challenge a locked decision without explicit user approval.

---

## Agent working rules (hard limits)

These rules are absolute. Break none of them. If you break them, you waste the user's time and tokens.

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

---

## Current status (KEEP UPDATED)

Update this section at the **end of every work session**. The next agent must know the exact state of the work.

### State summary

- **Phase:** 5 (the dialogs) — T1, T2, T3 COMPLETE (all
  user-tested, 2026-09-04; the records live in
  `docs/phase-5-kickoff.md`). T4 (the export dialog + engine + the
  remove confirm) IMPLEMENTED (2026-09-04), user test pending.
  **Next: run the T4 user test; then the phase-gate review.**
  Phases 0-4 are complete (their gates stay green).
  Phase 1 gaps that remain open: WebComicProvider and the PDF/DjVu
  writers (tracked in `docs/phase-1-kickoff.md`). The Phase 0 exit
  review can close with the T1 wrap-up (the settings port it
  waited on is in).
- **State:** `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, and `cargo test --workspace` are green. 261 tests pass across 29 suites. CI runs on the `docker-runner-amd64` container runner (ADR-020). The release tracks are `release.yaml` (rolling prerelease per push) and `tagged-release.yaml` (manual dispatch, stable release for an existing tag — ADR-021, 2026-09-03). Until the runner is registered and `comicrust-ci:latest` is built on the runner host, pushed and dispatched workflows sit queued on that label.
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
| `crates/cr-cli/src/main.rs` | `info`, `db-dump`, `db-roundtrip`, `pages`, `extract`, `thumb`, `rewrite`, `metron`, `lists`. |

The UI crate (Phase 3):

| Path | Contents |
|---|---|
| `crates/cr-ui/src/app.rs` | GtkApplication shell: `open` signal file handling, launcher window, error dialog. |
| `crates/cr-ui/src/theme.rs` | CSS provider skeleton + dark preference (no libadwaita, ADR-004). |
| `crates/cr-ui/src/reader/display.rs` | Pure `ImageDisplayControl` geometry: fit modes (anamorphic tolerance), part grid, binding edges, RTL, rotation, clamped offsets, interpolate, matrix inverse, magnifier zoom premultiply. Unit-tested. |
| `crates/cr-ui/src/reader/continuous.rs` | `ContinuousPageLayout` port: strip geometry, visible-window binary search, anchors. Unit-tested. |
| `crates/cr-ui/src/reader/keys.rs` | The `MainForm.InitializeKeyboard` command table (41 commands, exact key+modifier match, registration order = priority) + dispatch resolution. Unit-tested. |
| `crates/cr-ui/src/reader/page_view.rs` | The reader widget: part-transform cairo drawing, spread composition (`compose_spread`), layout modes, continuous offset-model scrolling, transitions (Fade/LeftRight/TopDown), paper texture, pool-queue page loads (fast/slow queues, completion callbacks + pump), magnifier, error page, navigation/zoom/pan, the full input map. |
| `crates/cr-ui/src/reader_window.rs` | Reader shell: session tabs (closable, Tab cycling), undock/re-dock, fullscreen chrome hide + reveal strip, MinimalGui, cursor auto-hide, reading-state write-back. |
| `crates/cr-ui/assets/papers/` | Paper textures copied from the C# `Resources/Textures/Papers`. |
| `crates/cr-image/src/error_assets.rs` | `CreateErrorPage`/`CreateErrorThumbnail` port with the bundled `ErrorPage.jpg` + `RedCross.png`. Unit-tested. |

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

Crate layout (all eight crates exist. `cr-core`, `cr-io`, `cr-image`, and `cr-cli` are active; `cr-engine` is the Phase 2 target; `cr-script`, `cr-ui`, `cr-app` are still empty stubs — see `docs/port-plan.md`):

| Crate | Contents |
|---|---|
| `crates/cr-core` | Data model (ComicBook/ComicInfo/MetronInfo/PageInfo), ComicDb.xml serde, settings, filename parsing |
| `crates/cr-io` | Comic providers (zip/tar/7z/rar/pdf/folder/web), ComicInfo.xml read/write-back, archives |
| `crates/cr-image` | Image currency type, decode/encode pipeline, resize/adjust filters, page/thumbnail caches |
| `crates/cr-engine` | Smart-list parser + matchers, queue manager, scanner, watch folders, backup, sync, remote server |
| `crates/cr-script` | PyO3 plugin host, `#@Directive` loader, `.crplugin` packages |
| `crates/cr-ui` | GTK4: reader (GtkGLArea), ItemView browser, shell, dialogs, theming, i18n |
| `crates/cr-cli` | Headless verification tooling (`info`, `db-dump`, round-trip) |
| `crates/cr-app` | Main binary: D-Bus single instance, app wiring, packaging |

---

## Compatibility invariants (DO NOT BREAK)

1. **ComicDb.xml read/write.** Element and attribute names, casing, and structure must match the C# `XmlSerializer` output exactly (the ComicLists tree, the custom values store). Golden-file round-trip tests verify this. The database is the one artifact users cannot lose.
2. **Metadata schema compat:** `ComicInfo.xml` (Anansi standard), ComicRack's `ComicBook.xml`, and `MetronInfo.xml` — read AND write in-archive.
3. **Plugin file formats:** `.py` scripts with `#@Name/#@Hook/#@Key/#@Description/#@PCount/#@Enabled/#@Image` comment directives + one command per `def` (see `PythonPluginInitializer.cs`), XML manifests, `.crplugin` = zip with `package.ini`.
4. **Smart-list query language** must parse and match identically (saved lists contain these queries — see `ComicSmartListItem.cs` and `ComicBookGroupMatcher.cs`).
5. **Caches are disposable. The database is not.** Thumbnail/image caches (`DiskCache` `cache.idx`, BinaryFormatter-serialized) have NO compat requirement. Design fresh formats freely.

---

## Critical gotchas

- **IronPython 2.7 = Python 2 semantics** (scripts use `print '...'` statements). We target PyO3/CPython 3, so existing ecosystem scripts need a 2to3 pass. The host API (`IPluginEnvironment`, ~40 methods) is the shim surface.
- **unrar license is GPL-incompatible.** Use subprocess/7z or libarchive for RAR. Never static-link unrar.
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
