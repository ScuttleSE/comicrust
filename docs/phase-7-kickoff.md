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

## Status

- T1 IMPLEMENTED + PROBE-PROVEN (2026-09-06), user test pending.
  357 tests; fmt + clippy green. The real-app handoff (second
  launch → focus + new-tab open, `-p` passthrough) verified under
  Xvfb with an isolated XDG.
- T2 pending.
