# Current status

Update this file at the end of every work session. Replace stale content.
Do not append a history. History lives in `docs/archive/` and in git.

## Active phase

**Phase 15 — Comic Vine cache, rate budget, and missing-issue fill.**
File: `docs/phases/phase-15.md`. Status: T1 to T4 and T7 done, T5 part done, T6 next.
**Phase 16 — Comic Vine scraper quality of life** is planned and waits
for Phase 15 (`docs/phases/phase-16.md`).
Phase 13 is IMPLEMENTED, user test pending. Phases 0-8 and 10-12 are
COMPLETE (Phase 12 user-tested 2026-09-10); Phase 9 is DEFERRED to
`docs/backlog.md`.

## Current task

Phase 15 T6 — the warm task, and the scrape-window budget readout
that T5 still owes.

Phase 14 (right-click rescans, ADR-036) is IMPLEMENTED and waits for
its user test. The navigator tree-state and Detail column-toggle fixes
(2026-09-12) wait for their user tests too. The scan-robustness round
(ADR-034, ADR-035) and the Phase 13 user tests are still open.

## Verification record

- Commit: Phase 15 T7, "Fill Missing Issues" (2026-09-12). The book
  menu gains a row that compares the cached issue list of the volume
  against the issue numbers the library holds, then creates fileless
  books for the ticked gaps. Each new book carries the Comic Vine
  issue id, so a later scrape needs no search. A series that no book
  ties to a volume says so and stops; it does not guess from the
  series name. The number match ignores leading zeros and letter case.
- Commit: Phase 15 T5 data layer, the per-resource request budget
  (2026-09-12). Every API call passes one `CvClient` chokepoint that
  writes to `request_log`, so the budget survives a restart. The
  default ceiling is 200 per resource per hour, from the user's
  figure; the API reference page carries no rate-limit text, so the
  figure is NOT verified and the ceiling is a policy value. MEASURED
  by gate: a spent budget stops the sweep, the sweep offset of the
  pages that landed survives, and `/issues/` and `/volume/` count in
  separate buckets. The scrape-window readout is still open.
- Commit: Phase 15 T4, the freshness rule (2026-09-12). A volume is
  closed when its stored `count_of_issues` equals the cached issue
  count and its last cover date is past the horizon. MEASURED by
  mock-server gate, in requests: a closed volume 0, an unchanged open
  volume 1 (the probe only), a changed open volume 2, an unknown
  volume 1 + its pages. The probe uses `/volume/4050-<id>/`, the
  resource the scraper already queries, because a `/volumes` filter on
  `id` is not confirmed by the API reference page.
- Commit: Phase 15 T3, the incremental sweep (2026-09-12). One paged
  `/issues` query with `filter=date_last_updated:<start>|<end>` keeps
  the skeleton current. MEASURED by mock-server gate: a cancelled
  sweep resumes and pays for no repeated page, a complete sweep makes
  zero requests on a re-run, a new window restarts at offset zero, and
  the page cap stops the run with its offset kept.
- Commit: Phase 15 T2, the MCL reader and writer (2026-09-12). The
  reader accepts what the `Update Missing` writer really produces: the
  trailing comma on the number list, the `.&@1` and `.&@2` escapes that
  that writer never reverses, the quoted list its docstring promises
  but it never emits, and a comma that a space follows inside a number.
  `mcl::import` seeds the skeleton layer in batches with no API
  request.
- Commit: Phase 15 T1, the Comic Vine cache store (2026-09-12).
  `cr-scrape` gains `rusqlite` (bundled) and the `cache` module: a
  `CvCache` trait over a two-layer SQLite file (ADR-037), with
  `SqliteCache::in_memory` behind the tests. The merge rule is
  measured by gate: a cheap write (an MCL import, or the sweep's
  `id,issue_number,volume` field list) never erases what an expensive
  query found.
- `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D
  warnings` — green.
- `cargo test --workspace` — 649 pass (was 564; 85 new cache, MCL,
  sweep, freshness, budget, and missing-issue gates).
- Commit: navigator tree state + Detail column toggle (2026-09-12).
  The navigator expansion now persists in
  `ComicListItemFolder.Collapsed` (the C# `FillListTree` /
  `tvQueries_AfterExpand` / `AfterCollapse`), so the tree no longer
  comes up fully collapsed. The Detail column chooser rows now
  activate: the popover no longer unparents itself on close.
  MEASURED with `GTK_DEBUG=actions` on GTK 4.22.4 — a row click runs
  the model button's default handler first, which pops the menu down;
  an unparent inside `closed` tore the action muxer down, every
  tracker item logged "action cols.col<id> was removed", `can_activate`
  went off, and the row handler that ran next activated nothing.
- Probes (release, Xvfb): `browserbar` gate D4 (the new
  rendered-row click round) green, `navpages` gates C and C2 (the new
  expand/collapse persistence round) green.
- `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D
  warnings` — green.
- `cargo test --workspace` — 564 pass (was 563; the new
  `folder_collapsed_round_trips_at_depth` golden gate).
- Commit: Phase 14 (2026-09-12). The book menu gains "Rescan Book
  File(s)" (one explicit scan request over the selected files, forced
  one-shot retry); the navigator menu gains "Scan List Contents" on
  smart and reading lists; the right-click selection follows the C#
  rule and every menu command reads the selection (the C#
  `UpdateSelectionFromMouse`, ItemView.cs:3855).
- Probes (release, Xvfb): `scanmarker` A-E green (E is the new
  book-menu rescan round), `contextmenu` ALL PASS (S1/S2 the new
  selection gates), `navpages` ALL PASS (F/G/H/H2 the new navigator
  menu gates), `scanrefresh` A-K green (release).
- `docs/guides/smart-list-queries.md` stays gated by its doc tests.
- The ADR-034 archive-reader measurements live in `docs/decisions.md`.

## Open user tests

Run in this order. Each one needs a rebuild first.

1. **Navigator tree state** (2026-09-12) — expand some navigator
   folders, close the app, and start it again: the same folders come
   back expanded. Collapse them, restart: they come back collapsed. A
   brand-new folder starts expanded.
2. **Detail column add and remove** (2026-09-12) — switch to Detail,
   right-click the column header, and uncheck "Opened": the column
   goes away at once and the row is unchecked at the next open. Check
   it again from the "All" page and from its letter page: it comes
   back. Restart: the choice holds.
3. **Right-click rescans** (Phase 14, ADR-036) — rebuild, then select
   one timed-out book and run the book menu's "Rescan Book File(s)": the
   book re-reads on the Book Scanner worker, a known-bad unchanged file
   re-reads too, and a still-bad file re-marks with a fresh verdict plus
   the summary window. Right-click an UNSELECTED book: only that book is
   selected. Right-click a book inside a multi-selection: the selection
   keeps, and Edit, Update Book File(s), Export, Remove, and Properties
   act on it. Then right-click a smart list (the `comicrust.scan.status`
   query list works): "Scan List Contents" scans the list's distinct
   file paths and reports once. The row must NOT appear on the Library
   root or on a folder. The Files view right-click follows the same
   selection rule.
4. **Scan robustness and problem markers** (ADR-034, ADR-035) — rescan
   the real library. It must run to the end with no stall: the files
   that used to take minutes each now take under a second. Books that
   could not be read carry a red "!" chip at the top left of the cover;
   books whose content does not match their file name carry an amber
   "≠" chip and still show their pages. Hover a chip: the tooltip gives
   the verdict, the format disagreement, and the reason. One summary
   window appears at the end. Make a smart list, paste the query
   `Match [Custom Value] regex "comicrust.scan.status" "."`, and confirm
   it lists exactly those books. Repair or replace one bad file, rescan,
   and its chip disappears without any other action. While a scan runs,
   click the scan lamp and use "Skip current file" (the same row is in
   Tasks): the scan moves on and the skipped book is marked "Skipped".
5. **Double-click open crash fix** (commit `140ba4c`) — double-click a book
   in the grid. The reader opens with no abort. Read some pages, then close
   the tab. The green read-ribbon moves in the grid without a second click.
6. **Phase 13 config unification** — the full steps are in
   `docs/phases/phase-13.md`.
7. **Config seed + reference doc** (commit `d5c52b3`) — the first start
   writes every `[extended]` and `[engine]` key at its default. Set
   `DatabaseBackgroundSaving = 60`, restart, and the database saves every
   minute mid-scan. Delete a key line, restart, and the key returns at its
   default. Every key you look up is in `docs/config-reference.md`.
8. **Mid-scan background save** (commit `d9262a4`) — start a scan of a large
   folder. Within about 10 minutes `~/.local/share/comicrust/ComicDb/
   ComicDb.xml` appears on disk and holds the books found so far.
9. **Smart-list rule delete and clipboard operations** — open a smart list
   editor. Every rule row and group carries a small ▾ button at the right
   edge with New Rule, New Group, Delete, Cut, Copy, Paste, Move Up, and
   Move Down, with honest enable states. Delete removes a rule. Copy and
   Paste inserts a clone. A Query round trip stays clean. Test the
   Cut/Copy/Paste clipboard round trip on a real desktop, because Xvfb
   stalls those reads.
10. **"No metadata" tag** — books whose scan found no metadata carry a small
   dark "?" chip at the top left of the cover in Thumbnail and Tile view. A
   Properties edit to a key field, or a Comic Vine scrape, removes the chip.
11. **Scan and open metadata import** — rescan a folder that holds magazines
   with `ComicInfo.xml`. New files carry series, title, writer, and page
   metadata. Files added through Open carry it too. Properties on a
   non-library comic shows its metadata. Books already imported as empty
   stay empty (the user declined a backfill).
12. **Detail view round** (commits `c21086a`, `f20a690`) — switch the browser
   to Detail. After ONE slider drag the text size and row rhythm match
   ComicRack. The saved `ItemRowHeight` 48 artifact must be dragged off the
   slider once; the status-bar slider re-ranges 12..48. Rows alternate grey
   and white, starting grey, and the selection keeps the highlight. The thin
   vertical column lines run through the header and the rows. Right-click
   the column header: the 13 defaults, All (alphabetical), then A-B, C-F,
   G-O, P-R, S, T-Y. Every row toggles from every page. The smart-list
   editor rule rows pick the type from the All and letter menus.
13. **Group, lamp, and thumbnail batch** — Group by Series in Thumbnail,
    Tile, and Details view gives header strips with true counts. A
    single-click on the disclosure triangle collapses or expands ONE group. A
    double-click collapses or expands ALL groups. The Views menu row does the
    collapse and expand of all groups, and it grays out without grouping. The
    scan lamp animates while a scan runs, and a click on it opens the "Cancel
    scan" menu. Preferences ▸ Advanced ▸ Thumbnails off shows placeholders
    until File ▸ Generate Cover Thumbnails backfills them.
14. **Scan control** — Tasks ▸ Abort Scanning on a real scan. Then a
    graceful exit mid-scan: close the window or press Ctrl+C. The app exits
    promptly, and a restart shows the books found so far. (The progressive
    fill and the no-glitch append passed on 2026-09-10.)
15. **Export freeze fix** — re-run a CBR to CBZ export. The window stays
    responsive and the progress ticks.
16. **Write-back fix** — edit a property of a CBR or CB7 book, then run
    Update Book File(s). The UI stays responsive, the write lands, and the
    Files-to-update list clears.
17. **Phase 10 install and duplicate steps** — the steps at the tail of
    `docs/archive/phases/phase-10.md`.
18. **Phase 11 install steps** — the steps at the tail of
    `docs/archive/phases/phase-11.md`.

## Blockers

None.

## Known gaps

Tracked in `docs/backlog.md`; they do not block the active phase.

- `WebComicProvider` is not ported.
- PDF and DjVu writers are missing.
- The `LICENSE` file is missing (a Phase 11 packaging gap).
- The T14 per-list sort deviation stands.
- HEIF and AVIF decode is missing.

## Environment notes

- `scanrefresh` gate E stalls mid-scan in a DEBUG build on this machine;
  the unmodified base HEAD fails the same gate the same way (a
  debug-timing environment flake, not a regression). Use the RELEASE run.
- `newbook` and `exportpage` probes reach "PROBE DONE", then their
  internal watchdog fires with `rc=2` (the unmodified base HEAD shows
  the same shape — a pre-existing probe quirk). `contextmenu`
  completed with `rc=0` on 2026-09-12; the quirk no longer shows there.
- `editor_probe` runs a main loop forever by design; a kill under timeout is its normal completion.
