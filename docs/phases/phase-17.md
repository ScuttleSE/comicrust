# Phase 17: Native modules II — the Library Organizer

## Status

IMPLEMENTED, USER TEST PENDING. Engine, UI, wiring, and probe completed on
2026-09-13. Phase 16 stays PLANNED.

## Goal

Port the Library Organizer plugin
(https://github.com/Stonepaw/comicrack-library-organizer, v2.1,
IronPython, Apache-2.0, Stonepaw) as the second native module under
ADR-031. It is a rule- and token-based file organizer: per-profile
templates rename and move comic files, exclude rules filter which books
qualify, Move/Copy/Simulate modes, duplicate-destination handling,
fileless-book cover export, and an undo log.

## The source

Read in full (2026-09-13): `losettings.py` (Profile schema + XML
persistence), `locommon.py` (Mode, ExcludeRule/ExcludeGroup,
UndoCollection, earliest/last book of series), `lobookmover.py` (mover +
PathMaker template engine), `libraryorganizer.py` (entry flow).
Read at implementation time: `configureform.py`,
`configformcontrols.py`, `loforms.py`, `loworkerform.py`,
`loduplicate.py` (config dialog, dialog results, worker form).

## Locked decisions (2026-09-13, user)

- **ADR-031 shape** — new crate `cr-organize` (pure Rust, no GTK;
  depends on cr-core, cr-io, cr-image), engine on a worker thread,
  request/response over mpsc, results applied main-thread through
  `library::apply_edited` / `insert_new_book` / `remove_book`.
- **ADR-033 config** — profiles in `[plugins.library-organizer]` of
  `comicrust.toml`; the undo log is state under
  `~/.config/comicrust/plugins/library-organizer/undo.dat` (same
  `profile|current|undo` line format).
- **Full-parity config dialog** — profile list, template editors,
  per-field Prefix/Postfix/Separator/Empty-value tables, exclude-rule
  tree with nested Any/All groups, months editor, illegal-character
  editor, folder lists.
- **XML import + export** — the addon's profile XML
  (`Profiles/Profile`, single-`Profile`, legacy `Settings/Setting`
  roots, Version upgrade incl. the 1.6→2.0 key renames) is read and
  written; imported Windows paths need hand-fixing (lenient load).
- **Both mid-run dialogs** — Duplicate (Cancel/Rename/Overwrite +
  "always do") and Multi-Value Selection, as main-thread
  request/response events.
- **Surfaces** — book context menu ("Library Organizer…", "Library
  Organizer (Quick)"), Tools drop ("Configure Library Organizer…",
  "Undo Last Move").

## Port decisions

- `\` stays the folder separator in templates; both `/` and `\` stay in
  the default illegal-character map, so imported profiles work
  unchanged on Linux.
- The 259-char `PathTooLongForm` is dropped (Windows-only); an unusable
  path is a logged failure.
- The `@Hook Startup` auto-run is deferred to `docs/backlog.md` (the
  addon ships it disabled).
- Multi-value "always use" memory stays per-run only (addon behavior).
- Overwrite deletes to trash via the `gio trash` callback seam
  (precedent `export_post_process_with`), never permanent.
- Template/rule field names stay C#-style (`<series>`, `<number4>`,
  "Read Percentage") for profile compatibility; `fields.rs` maps them
  to the typed model. Shadow semantics (`ShadowYear` -1 = empty) come
  from the model's own -1 defaults.

## Tasks

- [x] **T1** — `profile.rs` (37 unit tests: defaults, TOML round
      trip, XML export/import incl. the legacy `Settings` root, the
      `Text` attribute, and the 1.6→2.0 renames).
- [x] **T2** — `template.rs` + `fields.rs` (16 golden tests:
      padding, conditionals, inversions, month names, EmptyData,
      illegal chars, first letter, counter, read%, yes/no, the
      default-template date group, .NET date formats, path combine).
- [x] **T3** — `rules.rs` + `series.rs` (7 + 4 tests: Any/All,
      Only/Do-not, nested groups, the year-quirk and string-ordered
      numbers, publisher+series+volume scoping).
- [x] **T4** — `mover.rs` + `engine.rs` (13 tempdir integration
      tests: move/copy/simulate, rules, duplicate
      cancel/rename/overwrite with the read-percentage carry, the
      empty-folder prune with exceptions, fileless export, the
      blank-filename failure, the multi-claim precedence, the undo
      round trip and its multi-move collapse).
- [x] **T5** — cr-ui run/duplicate/multi-value/undo dialogs + the
      mpsc pump (`dialogs/organize.rs`).
- [x] **T6** — cr-ui config dialog, full parity
      (`dialogs/organize_config.rs`): profile list (new/duplicate/
      rename/delete/import/export), Overview, Files + Folders
      templates with a token picker and a live preview against a
      sample book, the exclude-rule tree with nested groups and
      Yes/No value combos, months/illegal-char/empty-value/
      excluded-folders/failed-fields editors. Every edit commits into
      the working copy immediately.
- [x] **T7** — wiring: `organize-books` / `organize-quick` /
      `organize-configure` / `organize-undo` actions, two book
      context-menu rows, two File-menu rows, the ProfileSelector
      dialog, the `[plugins.library-organizer]` accessors and the
      `undo.dat` path in `library.rs`.
- [x] **T8** — `organize_probe` (release, Xvfb, isolated XDG): gates
      A-F all green (config dialog + store round trip, the move run
      with the undo log, the duplicate rename, the multi-value
      series selection, the undo restore). `commands_probe`
      RESOLVED 77/77 unchanged.

## Port deviations found while implementing

- **The LAST non-copy profile claims the book** (measured from
  `create_book_paths`: `path = result` runs unconditionally; the
  EARLIER claim is marked skipped with the addon's "The book is moved
  by a later profile" message). The phase plan's "earlier claims"
  wording was wrong; the tests pin the real order.
- **Dead rule fields skip instead of crashing.** The addon's
  `name_to_field` carries `AddedDate`, `Counter`, the spaced
  `"Released Date"`, and `FirstLetter` — none is a C# property, so a
  rule on them raised out of the run. The port returns no value and
  the rule contributes nothing (the `Verdict::Empty` path).
- **`rename_path` strips only single-digit ` (N)` suffixes** — the
  addon regex is ` \([0-9]$` — so a name ending ` (2012)` renames to
  ` (2012) (1)`. Kept verbatim.
- **Dates without a format arg** render as the invariant
  `MM/dd/yyyy HH:mm:ss` (the addon emitted the OS culture's form);
  the config dialog's .NET format strings translate to chrono through
  `net_date_format` (custom tokens + the `D`/`M`/`Y`/… standards).
- **The `PathTooLongForm` is dropped** (the plan's decision); an
  unusable path is a logged failure.
- **Truthiness parity**: text-field reads collapse Yes/No `No` and
  `Unknown`, the 0/-1 ints, and the 0.0 floats to empty (Python
  `not text or text == -1`); RULE text keeps `-1` as `"-1"`.
- **`CommunityRating` gets its own insert-control name** — the
  addon's control collides its `Name` with `Rating`
  (`configureform.py` sets both to "Rating"), so the two controls
  overwrote each other's Prefix/Postfix entries. The port keys them
  separately; the addon's stored files still import (the keys just
  stop colliding).
- **The overwrite read-percentage carry rides the moved book** (a
  pending-clone map) instead of a separate apply — the final
  `Update` must win in the session.
- **`months` keys are strings** — the unified config's
  `set_plugin` round trips through `toml::Value`, whose tables have
  string keys; integer keys made the whole plugin table silently
  fail to store (measured by the probe's gate B before the fix).
- The multi-value "always use" memory stays per-run (the addon's
  PathMaker state dies with the run).

## Gates

`cargo fmt --all`; `cargo clippy --workspace --all-targets -- -D
warnings`; `CR_FORMAT_TESTS=1 cargo test --workspace --locked`
(774 passed, 0 failed; 50 new in cr-organize, 1 menubar gate
updated); `cargo build --release --locked -p cr-app`; the probe and
`commands_probe` under Xvfb (release, isolated XDG) — all green.

## Verification

Standard gates + the probe + user tests: Simulate over real books, a
real Move + Undo round-trip, and an XML profile import from the user's
existing `losettingsx.dat`.

## Completion record

The implementation completed on 2026-09-13. The Library Organizer simulation,
move, conflict, undo, and profile-exchange tests in
`docs/open-user-tests.md` must pass before this phase closes.
