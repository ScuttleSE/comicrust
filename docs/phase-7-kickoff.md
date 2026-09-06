# Phase 7 Kickoff — Platform (re-scoped 2026-09-06)

Note (2026-09-06): the original Phase 7 scope (D-Bus single instance,
MTP/wireless sync, HTTP remote server, full i18n) was RE-SCOPED by
user decision (ADR-028): device sync, the HTTP remote library, the
tray icon, and the i18n (TR) port moved to `docs/backlog.md` — each
entry carries its C# research record, so no re-study is needed when
one is picked up. Phase 7 is now the single-instance + startup file
pipeline only; Phase 8 (polish/ship) follows.

Goal: the C# second-instance handoff and the startup file pipeline,
on GApplication unique mode.

Target crates: `cr-ui` (the app shell), `cr-app`, `cr-core` (a
settings helper).

## Scope decisions (locked — ADR-028)

- Device sync (all of it: disk + GVFS MTP + wireless) → backlog.
- HTTP remote library → backlog (ADR-005 stands).
- Tray icon (`-hidden`, minimize/close-to-tray) → backlog.
- i18n (the TR port + the 19 packs + the string sweep) → backlog; the
  Preferences language page stays deferred with it.
- Auto-update check, news feed, crash-watchdog dialog → backlog.

## C# spec (research record)

- `Program.cs:1149-1150` — `SingleInstance("ComicRackSingleInstance",
  StartNew, StartLast)`: a WCF named-pipe service; the SECOND launch
  sends its raw `string[] args` to the running instance and exits.
- `Program.cs:1063-1093` — `StartLast(args)`, marshalled to the UI
  thread: `RestoreToFront()` (un-minimize, bring to front), re-parse
  the received argv into a fresh `ExtendedSettings sw`,
  `sw.ImportList` → `ImportComicList`, `sw.InstallPlugin` →
  Preferences (N/A for the port — ADR-027), every positional file →
  `OpenSupportedFile(file, newSlot: true, sw.Page, fromShell: true)`.
- `MainForm.cs:2286-2314` — `OpenSupportedFile(file, newSlot, page,
  fromShell)`: `.crplugin` → Preferences; `.cbl` → import the list
  and open its newest-read book; anything else →
  `books.Open(file, newSlot, Math.Max(0, page - 1))`; then
  `fromShell && HideBrowserIfShellOpen` → `BrowserVisible = false`
  (the reader covers the browser).
- `MainForm.cs:1041-1061` — the FIRST-launch pipeline: existing
  command-line files → `OpenSupportedFile(file, newSlot: false, 0,
  fromShell: true)`; if nothing opened and `Settings.OpenLastFile` →
  open `Settings.LastOpenFiles` (new slots, no open-count bump);
  `-il` → `ImportComicList`.
- `Program.cs:1151-1155` + `Program.cs:1127-1136` + `160-165` —
  restart: re-spawn the exe with `-restart -waitpid <own pid>`; the
  new process waits up to 30 s for that pid to exit; `-restart`
  clears `Files`/`ImportList`/`InstallPlugin` so the restarted
  instance opens nothing.
- `MainForm.cs:1904-1908` — `MenuRestart` sets `Program.Restart` and
  closes the form.
- Switch parsing: `cYo.Common/Runtime/CommandLineParser.cs` — already
  ported (`cr-core/src/settings/extended.rs`: the full switch table,
  `parse_argv`, bool-toggle/next-arg rules, unknown switches
  swallowed, non-switch args → `files`).

## Task list

### T1. GApplication unique mode + the startup file pipeline

- `cr-core/src/settings/extended.rs`: add
  `ExtendedSettings::from_argv(argv) -> ExtendedSettings` (a fresh
  default + `parse_argv`) — the `StartLast` re-parse. The ini merge
  is irrelevant for the handoff: `Files`/`Page`/`ImportList` are
  command-line-only (`ini: false`), exactly like the C# fresh parse.
- `cr-ui/src/app.rs`:
  - Drop `NON_UNIQUE`; flags = `HANDLES_OPEN | HANDLES_COMMAND_LINE`.
    Register the app BEFORE any startup work: a remote (second)
    instance must not open the database — it forwards its argv and
    exits (`is_remote()` after `register()`).
  - `connect_command_line` = the one entry point for the primary's
    own argv (first launch) AND every handoff (the C# `StartNew`
    parse + `StartLast`). First call → the first-launch pipeline
    (`newSlot: false`, page 0, existing files only, then the
    `OpenLastFile` reopen when nothing opened); later calls →
    `StartLast` (present/restore-to-front, then files with
    `newSlot: true` + the `-p` page passthrough).
  - `connect_open` stays for explicit `g_application_open` senders;
    it routes through the same open helper (`fromShell` semantics).
  - The open helper ports `OpenSupportedFile` minus `.cbl` (T2):
    extension check, `newSlot`, `page - 1` (0-based, clamped ≥ 0),
    `HideBrowserIfShellOpen` (the reader workspace shows — the docked
    reader already covers the browser).
- `cr-ui/src/reader_shell.rs`: `open_comic` gains
  `new_slot`/`start_page` — `new_slot` skips the same-path slot focus
  and always opens a fresh tab; `start_page > 0` replaces the resume
  position (`page - 1` clamped to the display count).
- `cr-ui/src/library.rs` (`initialize_settings`): after the boot
  parse, a `restart` flag clears `files`/`import_list`/
  `install_plugin` (the C# `Program.cs:160-165` ExtendedSettings
  getter).
- The restart action (`shell.rs`): spawn
  `<exe> -restart -waitpid <pid>` instead of a bare spawn (with
  unique mode a bare spawn would forward to the dying instance and
  restart would break), then quit. The wait: in `cr_ui::run` before
  GTK init — `-waitpid N` polls `/proc/<N>` every 100 ms up to 30 s
  (the C# `WaitForExit(30000)`), Linux-only is fine.
- Recorded deviations: `-il`/`.cbl` opens parse but are inert until
  T2; `.crplugin` is inert (ADR-027); `-hidden` parses but is inert
  (no tray — backlog); the QuickManual startup step is not ported
  (help UI, ADR-024).

Gate: `cargo test`, an Xvfb probe (a second launch forwards argv to
the primary, focuses the window, opens the file in a new slot; a
first launch still boots the library), then the user test below.

### T2. `.cbl` reading-list import (the `ImportComicList` port)

- C# spec: `MainForm.cs:1893` → `ComicListLibraryBrowser.cs:1483`
  `ImportList(file)`: `ComicReadingListContainer.Deserialize` (the
  `.cbl` XML — `ComicReadingListContainer.cs`, 103 LOC: Name,
  MatcherMode, Matchers, Items); with matchers → a smart list; else
  `ComicIdListItem.CreateFromReadingList` (`ComicIdListItem.cs`, 257
  LOC: match the list items against the library books, collect the
  unsolved ones); the missing-books question (import solved only /
  add missing books / cancel); the list lands in
  `Library.TemporaryFolder.Items` when no target collection is given,
  the tree refills and the list selects.
- Port: the `.cbl` container model + reader (cr-core), the matching
  (cr-engine or `cr-ui::library`), the import flow + dialog
  (cr-ui). The navigator gains "Import Reading List…"
  (`ComicListLibraryBrowser.cs:341`).
- Un-stubs: the `.cbl` branch in T1's open helper, `-il` on both the
  first launch and the handoff.

Gate: unit tests on a real `.cbl` fixture (solved/missing match),
user test.

## User test (T2)

1. Right-click the navigator tree → "Import Reading List…" → pick a
   `.cbl` whose books exist in the library → the tree gains the list
   (inside "Temporary Lists" when nothing was selected, or inside the
   selected folder) and shows its books.
2. Import a `.cbl` with books NOT in the library → the question
   dialog lists the missing captions; "Add missing Books to Library"
   imports the list AND creates the fileless books (the Fileless
   marker shows on their covers, the open gate blocks opening them);
   "Import" imports the list with only the solved books; Cancel
   imports nothing.
3. `comicrust some-list.cbl` (no instance running) → the app starts
   and imports the list, then opens the most recently read book of
   the list.
4. With an instance running: `comicrust -il some-list.cbl` → the list
   imports into Temporary Lists (no book opens, the window comes to
   front).
5. A `.cbl` exported by real ComicRack (matchers or id list) imports
   and evaluates like it does in ComicRack.

## User test (T1)

1. Start comicrust with a comic; from a terminal run
   `comicrust <another-comic>` → the SAME window comes to front and
   the second comic opens as a NEW reader tab.
2. `comicrust -p 5 <comic>` (with an instance running) → opens at
   page 5 (1-based).
3. With no instance running, `comicrust <comic>` → starts, opens the
   comic; a repeat of the same command focuses the existing tab (no
   duplicate).
4. File ▸ Restart → the app exits and comes back fresh (no comics
   reopened), layout kept.
5. Close the app normally (the window X) with a comic open, then
   start with no arguments → the session comic reopens
   (`OpenLastFile`); with no comic it shows the QuickOpen as before.

## Probe (T1)

`cr-ui/examples/singleinstance_probe.rs` — the probe binary spawns
itself as the second instance against its own unique-mode primary
and gates: (A) the primary's boot arrives as `command-line` and
carries ONLY argv[0]; (B) the handoff delivers the client's argv and
`ExtendedSettings::from_argv` parses the files + `-p 7`; (C) the
client process exits (reaped via the kept Child handle). Run:
`GDK_BACKEND=x11 DISPLAY=:99 cargo run -p cr-ui --example
singleinstance_probe` (no library touched — no XDG isolation
needed). The REAL app was verified headless too: a second
`cr-app <comic>` exits 0 in ~0.1 s while the primary's window
switches to the new comic; `-p 5` flips the current tab.

Probe lessons (do not re-learn):

- With `HANDLES_COMMAND_LINE`, the gio primary emits
  `command-line` for its own argv and NEVER `activate` — the boot
  work lives in the command-line handler (the old
  `connect_activate(show_shell)` path is dead code under this flag).
- argv[0] rides BOTH deliveries (the primary's own boot AND the
  remote handoff) — the handler strips element 0 or the binary path
  lands in `files` and the app tries to open ITSELF as a comic.
- A second process that exits after forwarding stays a ZOMBIE until
  the parent reaps — `/proc/<pid>` exists for zombies; gate "the
  client exited" through `Child::try_wait` on the kept handle.
- The primary must stay alive ~500 ms after the handoff before it
  quits: the client's forward call needs the reply.

## Incident record (2026-09-06, T2 user test)

The user imported a real `.cbl` ("Add missing Books to Library"),
then removed the fileless placeholder books with "Also delete the
files". `book_path` returned `Some("")` for every fileless book and
the remove flow ran `gio trash ""` once per book — gio resolves an
empty argument to the process's CURRENT WORKING DIRECTORY (proven
with a /tmp experiment) and trashed the repo checkout the app was
launched from (six trash entries, 18:19). Nothing was lost: the
trash held a full copy including the git-ignored test comics and
the user's two real ComicRack `.cbl` exports; all six files were
restored with `cp -n` from the trash copy (which stays in the trash
as a second net until the user confirms). Fix (shell.rs remove
flow): only trash a path that is non-empty AND `is_file()` —
fileless books never touch the trash. The real `.cbl` exports are
back in `tests/testfiles/` for the T2 user test.

DELETE-PATH AUDIT (the user's follow-up: verify the fix, folder
paths, other deleters). All destructive operations in the
workspace, each verified:

- The remove flow's `gio trash` (shell.rs) — FIXED (above). The C#
  main flow guards the same way (`book2.IsLinked` →
  ComicListLibraryBrowser.cs:147; `IsLinked =>
  !string.IsNullOrEmpty(filePath)`, ComicBook.cs:1321) — the port's
  empty-check is exact parity, `is_file()` is extra defense.
- Folder comics (a comic whose `file_path` IS a directory): the C#
  WOULD trash the folder (`ShellFile.DeleteFile` on the folder
  path, ComicListLibraryBrowser.cs:155 + FolderComicListProvider
  .cs:151). The port's `is_file()` skips them — a RECORDED
  DEVIATION (safe over parity; the port has no Folders view, and a
  mis-pathed book must never trash a whole user directory — that
  is exactly the incident mechanism).
- "Reveal" (`xdg-open` the parent folder) — FIXED: the empty-path
  guard (the C# `IsLinked` shape; an empty path resolved the file
  manager to `/`).
- Every open path (`open_comic` consumers: the grid, Quick view,
  the context menu, list browsing) — already gated: `open_comic_at`
  returns early on an empty path (the Phase 6 `NavigatorManager
  .Open` IsLinked gate).
- The write-back temp cleanup (`cr-io/src/write.rs`) — removes only
  the `.tmp` sibling the write itself created, from a linked book's
  file; the write queue never runs for fileless books.
- The DB `.bak`/`.restore` rotation (`cr-core`) — the database's
  own app-controlled path.
- The disk-cache pruning (`cr-image/src/disk.rs`) — only `*.cache`
  files under the app's cache root.
- The scanner "AutoRemove" — flags books missing, never deletes
  files (the Phase 2 record).
- The backup round-trip's `remove_file` (`cr-engine/src/backup.rs`)
  — the create → destroy → restore test helper only ("future UI"
  comment: the real backup UI must restore over the DB, not delete
  first).
- `remove_dir_all` in cr-io/export.rs and cr-io/write.rs —
  `#[cfg(test)]` temp dirs only.

No other user-data deletion exists in the app.

## Status

- T1 COMPLETE — USER-TESTED, ALL PASS (2026-09-06; the 5 steps in
  the User test section above). 357 tests; fmt + clippy green.
- T2 IMPLEMENTED (2026-09-06), user test pending. Landed:
  - `cr-core/src/database/reading_list.rs`: the `ComicReadingListContainer`
    port — `<ReadingList MatcherMode>` root, `<Name>`, `<Books><Book>`
    items (Series/Number/Volume/Year/Format attrs with the C# defaults,
    `<Id>` always, `<FileName>` when set) and `<Matchers>` (the same
    matcher serialization the ComicLists tree uses —
    `ComicBookMatcher::from_start` reused). Order-tolerant parse +
    a byte-stable writer (round-trip tested against a hand-written
    net48-shaped fixture: declaration without encoding, xsd/xsi
    namespaces, omitted default attrs, `<Books />`/`<Matchers />`
    empties).
  - `cr-engine/src/reading_list.rs`: the `ComicIdListItem
    .CreateFromReadingList` port — resolve by Guid, by file name
    (name-without-extension, OrdinalIgnoreCase), then the
    series/number relaxation ladder (`SeriesEquals` None →
    IgnoreVolumeInName → +StripDown; the `rxVolume`/`rxSpecial`
    regexes ported, the trailing empty alternative dropped as a
    no-op) with the year ±1 / volume / format narrowings that fall
    back to the previous candidate set. `SetFileNameInfo` parity:
    the file-name parse OVERWRITES the stored fields for unsolved
    items only. Unsolved items become placeholder `ComicBook`s
    (fresh Guid, `AddedTime` now, the parsed series data) collected
    for the missing-books question.
  - `cr-ui/src/dialogs/import_list.rs`: the `ImportList` flow —
    parse → smart list (matchers) or the library match → the
    missing-books question (`Import` / `Add missing Books to
    Library` / `Cancel`; the C# message shape with the 25-caption
    cap + `...`) → insert → tree refill + selection.
  - Landing: `library::import_temporary_item` (the
    `ComicLibrary.TemporaryFolder` find-or-create — a
    `ComicDatabase::temporary_folder` helper appending a
    "Temporary Lists" folder at the tree end) and
    `library::import_list_item(target)` (the
    `GetNodeComicListCollection` shape: folder → last child, item →
    its parent container, none/unknown → top level).
  - Wiring: the `.cbl` branch in `OpenSupportedFile` (import, then
    open the newest-read book of the list — ties to the later
    entry; fileless placeholders filtered out), `-il` on both
    boot paths (first launch: files → OpenLastFile → import,
    `MainForm.cs:1058`; handoff: import BEFORE files, `StartLast`
    order), and the navigator "Import Reading List…" context item
    (`ListCommand::Import` → the multi-select `.cbl`/xml chooser
    importing into the current selection's container). The
    navigator gained the `TempFolder` icon for temporary folders
    (`ComicListItemFolder.ImageKey` parity).
- T2 PROBE: `cr-ui/examples/importlist_probe.rs` (isolated XDG
  required). Gates: (A) the question dialog + "Add missing" adds
  the placeholder (Watchmen/1/1986 parsed from the file name),
  (B) the list lands in the Temporary Lists folder selected in the
  tree, (C) solved-by-id + solved-by-file-name (3 evaluated), (D)
  the "Import" answer keeps the library at 3 and drops the unsolved
  id (empty list), (E) a matchers-only `.cbl` lands as a smart list
  evaluating to the right book. All gates green under Xvfb.
- PARSER FIX found by the T2 probe (cr-core `comic_name_info.rs`):
  the rxNumber RightToLeft emulation used the last match of a
  left-to-right scan; for "Watchmen 001" the `c\w*\s*` alternative
  match "chmen 001" swallows the real number and the series came
  out "Wat". The C# RTL scan takes the match with the rightmost
  START ("001") — `rightmost_start_match` now serves the rxNumber
  stage (guarded variant for the `part\s+` lookbehind); the
  year/get-number stages keep the last-of-scan emulation (no
  overlapping candidates there). Regression tests: the number
  removal no longer swallows the series ("Super Comics vol 2 014…"
  → series "Super Comics") + the Watchmen parse.
- ORDER FIX (the user asked, rightly): the C# `OnGetBooks` walks
  `BookIds` in LIST order — an unsorted reading list shows the books
  in the .cbl item order. The port's IdList evaluation filtered the
  library slice (DB order) instead; `evaluate_inner` now walks
  `book_ids` first-seen (the HashSet dedupe parity) over an id index,
  with a shuffled-order regression test (`id_list_evaluates_in_book_
  ids_order`). The browser sort applies on top when set, exactly
  like the C#.
- Recorded deviations: the `AutomaticProgressDialog` (matching
  progress + cancel) is not ported — the in-memory match runs
  synchronously; the question dialog is a GTK MessageDialog; the
  newest-book open filters to linked books (the C# `books.Open`
  would fail on a fileless book anyway — the Phase 6 open gate);
  the list lands as the C# default names ("Temporary Lists", the
  English TR defaults).
- Gate: 367 tests (+10), fmt + clippy green, the T1/T2 probes and
  the command/menubar/single-instance probes green. USER TEST
  PENDING.
