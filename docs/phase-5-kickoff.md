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

### T1. The settings port + the options builder

- [ ] `IniFile` + `EngineConfiguration` + Settings in cr-core (or
      `cr-settings`): field defaults = the C# `[DefaultValue]`
      attributes; ini round-trip tests.
- [ ] Reconcile the hard-coded stand-ups: `TRACK_CURRENT_PAGE`
      (reader), `AddToLibraryOnOpen` (currently hard false — the
      reader flow), the engine defaults 14/95/10 (list trees),
      `ComicNameInfo`'s `OfValues`/legacy-parser flag (Phase 0
      note).
- [ ] The GTK options builder (`cr-ui/src/settings/`): walk the
      typed registry → labeled widgets per type (bool check,
      numeric spin, enum combo, string entry), grouped by
      category — the `FormUtility` parity. Unit-test the
      model↔widget mapping headlessly.
- [ ] Wire the Preferences dialog shell (tree of categories +
      panels), OK/Cancel semantics (edit a clone, commit on OK).

### T2. The book editor (`ComicBookDialog`)

- [ ] The metadata form: every ComicInfo field the registry
      exposes, the proposed-value flow (`EnableProposed` — the
      gray "proposed" text from the filename parse, accept
      per-field), the `Checked` semantics.
- [ ] The thumbnail page: front-cover choice, custom thumbnail
      set/clear (needs the settings port's CustomThumbnails path —
      see above), the page-type/rotation/position edits per page
      (the Pages panel's deferred edit commands).
- [ ] Write-back: `ComicBook.IsDirty` → the write-info queue
      (`write.rs` — the CBZ/CBT native rewrite, CB7 `7z u`) with
      the Phase 1 rules (never write defaults over file metadata).
      The "Files to update" smart list flips as the user saves.
- [ ] Bulk edit (`MultipleComicBooksDialog`): the union/intersection
      field model over a selection (the browser context menu gains
      "Edit" on multi-select).

### T3. The smart-list editor (`SmartListDialog` + matchers)

- [ ] The visual matcher builder: property combo (the registry),
      operator combo (the spec's operators), 1–2 argument fields,
      and/or mode, not flags, nested groups (`MatcherGroupEditor`).
      The model layer (a matcher-tree → editor-state mapping) is
      pure and unit-tested.
- [ ] The query tab: the text form with the live parse (the Phase 2
      query language; the T2 lesson — `Match [Series] contains
      "Batman"`).
- [ ] The navigator's "New Smart List" opens the editor; Edit
      opens it pre-filled. `EditListDialog` for list properties.
- [ ] The reading-list editor (`ListEditorDialog`) — orderable book
      lists (the `IdListItem` model).

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

- [ ] T1 settings port + options builder + Preferences shell
- [ ] T2 book editor + bulk edit + write-back wiring
- [ ] T3 smart-list/reading-list editors
- [ ] T4 export + small dialogs

Check the boxes here and in `AGENTS.md`'s status as tasks close.
The phase gate: a user edits a comic's metadata (single + bulk),
saves it back to the file (verified outside the app), edits a
smart list both visually and as a query, exports a comic, and
changes preferences that visibly move the reader and browser.
