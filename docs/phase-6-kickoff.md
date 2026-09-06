# Phase 6 Kickoff — Native features + de-scripting (ADR-027)

Note (2026-09-06): the original Phase 6 (the PyO3/CPython scripting
host) was DROPPED — ADR-027 supersedes ADR-003. The feasibility
evidence (the WinForms wall in the flagship plugin and the bundled
scripts, the tiny used-script set, the zero-use `Expression` matchers
in the real-world DB) is recorded in ADR-027. The hook-signature
reference from the old kickoff survives at the end of this file. The
full scripting task breakdown is in git history.

Goal: the C#'s native features that the scripting layer obscured —
fileless book creation — plus the de-scripting cleanup and the two
deferred Edit-menu page commands. Exit gate: no scripting surface
remains; the fileless-book flow is user-tested; the matcher
round-trip stays byte-stable.

Target crates: `cr-ui` (dialogs, shell), `cr-engine` (matchers), the
workspace root (crate removal).

## Status

COMPLETE — ALL TASKS USER-TESTED, ALL PASS (2026-09-06). The user ran
the five steps in the "User test" section below on their machine.

- T1 DONE (the ADR-027 commit).
- T2 DONE — `newbook_probe` A/F; user-tested.
- T3 DONE — `newbook_probe` B/C/D; user-tested.
- T4 DONE — headless: query round-trip + eval tests, the golden
  byte-identity fixture for the PluginKey attribute; 356 tests.
- T5 DONE — `cr-script` removed; 356 tests green.
- T6 DONE — `exportpage_probe`; user-tested (the clipboard paste on
  the user's Wayland/X11 session).

## Task list

### T1. The decision record (this commit)
- `docs/decisions.md`: ADR-027 appended; ADR-003 status superseded.
- `docs/port-plan.md`: the crate diagram, the IronPython/MSHTML
  mapping rows, the Phase 6 roadmap row, sequencing rationale item 3,
  the backlog moves (the NewComics entry resolved; WikiSearch added).
- `docs/risk-register.md`: risk #3 closed.
- `AGENTS.md`: the plugin-format invariant retired; the IronPython
  gotcha removed; the crate tables updated.
- Status: DONE with this kickoff.

### T2. Native "New Comic…" (the fileless-book flow)
- C# spec: `MainForm.cs:1879` `AddNewBook(showDialog=true)` — a new
  `ComicBook { AddedTime = now }` → `ComicBookDialog.Show` →
  `Program.Database.Add` on OK. Cancel adds nothing. The menu item is
  `miNewComic` (native C#, never a script).
- Port: un-stub `new-book-entry` (`cr-ui/src/browser/shell.rs`, the
  "fileless books are unported" comment) — build the default book,
  open the existing book editor, insert through the library on OK,
  refresh the view.
- Verify the fileless book end-to-end: browses (no file path), the
  `fileless` comic-type filter shows it (the filter side exists —
  `compose_quick_filter`), the FilelessMarker icon renders, and a
  reader open with 0 pages is safe (the Phase 3 empty-page-list
  lesson).
- Gate: a headless probe (create through the action, book count,
  fileless filter) + the user test.

### T3. "New fileless Book Series…" (the NewComics.py port)
- C# spec: the script's dialog — series, volume, from-number,
  to-number; OK enabled when the series is non-empty and
  `start >= 0` and `end >= start`; a range over 100 aborts; on OK it
  creates N = end-start+1 fileless books (`Number = str(n)`) and
  selects them (`ComicRack.Browser.SelectComics`).
- Port: a small dialog over the same insert path as T2, then select
  the created books in the ItemView.
- Menu position: File menu directly after "New Comic…" (the C#
  inserts the NewBooks items after `miNewComic`, MainForm.cs:787).
- Gate: probe + the user test.

### T4. Matcher parse-compat (`Expression` + plugin-list)
- `cr-engine`: the tokenizer already accepts both matcher types in
  saved queries (compat invariant: the query language parses
  identically). Verify the render round-trip stays byte-stable, and
  the evaluation returns an explicit not-supported result (an empty
  set, never a panic, no silent wrong results).
- Unit tests: a fixture list carrying each type parses, renders
  identically, evaluates empty. The real-world gate stays green (the
  fixture carries none).

### T5. De-scripting cleanup
- Remove `crates/cr-script` and every workspace reference
  (`workspace.dependencies` in the root Cargo.toml; check the crate
  dependency graph for dependents).
- Settings: the parser keeps accepting the `Scripting` and
  `PluginsStates` keys from existing Config.xml (read, ignore — no
  write-through changes).
- Preferences: the Scripts-page deferral note becomes "removed
  permanently (ADR-027)".
- Menubar: the Automation-submenu omission wording (the T4 record)
  becomes permanent with the ADR reference.

### T6. Copy Page / Export Page (Edit menu)
- C# spec: `MainForm.cs:1338-1339` wire `ComicDisplay.CopyPageToClipboard`
  and `ExportCurrentImage` (`MainForm.cs:2326` —
  `ComicDisplay.CreatePageImage` → `ExportImage(caption, image)`);
  enable-state = a book is open.
- Copy Page: the current page surface → the GTK clipboard (a
  `gdk::ContentProvider` image; the render path yields cairo
  surfaces — convert to a `gdk::MemoryTexture`).
- Export Page: the small export-image dialog (format + file name from
  the caption) writing the current page surface as PNG/JPEG.
- Un-stub the two Edit-menu rows (the T13 record).
- Gate: the probe gates the dialog + the enable-state; the user test
  decides the clipboard (headless clipboard checks are unreliable).

## Risks / known traps

- A fileless book has no file: every path that assumes a file
  (provider, thumbnails, file-info refresh, write-back, the scanner)
  must tolerate it. Watch the thumb queue — a fileless book must not
  enqueue provider loads.
- The reader with 0 pages: the Phase 3 `clamp(0, -1)` lesson applies;
  test the open path explicitly.
- The clipboard image path differs between X11 and Wayland; the user
  test decides (the standing input-path rule).
- The not-supported matcher evaluation must not break the smart-list
  editor's query round-trip (render byte-stability).

## Verification

```sh
cargo fmt --all && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace
cargo run -p cr-ui --example newbook_probe        # T2/T3
cargo run -p cr-ui --example exportpage_probe     # T6 (dialog gates)
```

Headless probes: Xvfb + an isolated XDG pair (the standing
lesson). The user-test protocol stays mandatory per task
(`cargo run -p cr-app --release --`, then PAUSE with the written
test).

## User test (write at the phase close — T2/T3/T6)

Build: `cargo run -p cr-app --release --` (release matters).

1. **New fileless Book Entry:** File ▸ "New fileless Book Entry..."
   (Ctrl+Shift+N) opens the book editor over an untitled book. Type a
   series/number, OK. The book appears in the Library list, carries
   the fileless marker on its cover, and survives a restart.
   Cancel adds nothing.
2. **New fileless Book Series:** File ▸ "New fileless Book
   Series..." — OK stays disabled until the series is non-empty and
   the range is valid. A 5..7 range creates 3 books, selected in the
   grid. A range over 100 (e.g. 1..150) silently adds nothing.
3. **Fileless browsing:** the comic-type filter "Show only fileless
   Entries" shows exactly the created books. Double-click on a
   fileless book opens nothing (no error dialog) — the C#
   `IsLinked` gate.
4. **Copy Page:** open a real comic, Edit ▸ Copy Page, paste into
   another app — the composed page image lands (check both layouts:
   single and double — double copies the whole spread).
5. **Export Page:** Edit ▸ Export Page — "Save Page as" dialog, the
   name is "{Caption} - Page {N}", the format dropdown carries
   JPEG/BMP/PNG/GIF/TIFF and the chosen format persists across
   invocations; the written file opens.

## Deviations recorded

- The book editor's window title falls back to "Book" when the
  caption is empty (the C# shows the bare empty caption).
- The `lblPages` label follows the C# `PagesAsText` rule
  ("Unknown" for 0 pages, the "N/" prefix only when LastPageRead >
  0) — this replaces the port's earlier "Page N/M Page(s)." shape.
- The fileless marker reuses the port's bottom-left state-strip
  shape (the C# centers a state-image row — the missing-marker
  deviation, now shared).
- Copy Page writes a PNG to the clipboard (the C# writes a DIB
  bitmap); Export Page uses `FileChooserNative` with the C#'s
  5-format filter table.
- A double-page export copies the whole composed spread (the C#
  `CreatePageImage` renders the whole virtual image — same
  behavior).

---

## Superseded record: the scripting kickoff (2026-09-06)

The C# hook-signature table (`PythonCommand.hookTypes` — the shape
every hook calls with). Kept as the reference for any future
scripting ADR:

| Hook | Signature |
|---|---|
| Library / Books / NewBooks | `Action<ComicBook[]>` |
| ParseComicPath | `Action<string, ComicNameInfo>` |
| BookOpened | `Action<ComicBook>` |
| CreateBookList | `Func<ComicBook[], string, string, IEnumerable<ComicBook>>` |
| ReaderResized | `Action<int, int>` |
| NetSearch | `Func<string, string, int, Dictionary<string, string>>` |
| ConfigScript / Startup | `Action` |
| Shutdown | `Func<bool, bool>` |
| ComicInfoHtml / QuickOpenHtml | `Func<ComicBook[], string>` |
| ComicInfoUI / QuickOpenUI | `Func<Control>` |
| DrawThumbnailOverlay | `Action<ComicBook, Graphics, Rectangle, int>` |

The host surface was `IPluginEnvironment` (~40 methods across
IApplication/IBrowser/IOpenBooksManager — see
`ComicRack.Plugins/Automation/*.cs`), injected as the global
`ComicRack`, plus `ScriptPath`. The `#@Directive` scan and the
`ConfigScript` pairing lived in `PythonPluginInitializer.cs` /
`PluginEngine.cs`; `.crplugin` packages in `PackageManager.cs`.
Findings that killed the plan: every UI-bearing script needs WinForms
(unportable), the flagship Comic Vine Scraper imports `clr`
(unportable to CPython), and the used-script set is small enough to
port natively (ADR-027).
