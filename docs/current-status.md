# Current status

Update this file at the end of every work session. Replace stale content.
Do not append a history. History lives in `docs/archive/` and in git.

## Active phase

**Phase 13 — one unified config file + data tables out of code.**
File: `docs/phases/phase-13.md`. Status: IMPLEMENTED, user test pending.

The last closed phase is Phase 12 (the Comic Vine Scraper), user-tested
2026-09-10. Phases 0-8 and 10-12 are COMPLETE. Phase 9 (the SQLite
backend) is DEFERRED to `docs/backlog.md` with its design intact.

## Current task

None in flight. The scan-robustness round (ADR-034, ADR-035) is
implemented and waits for its user test. The Phase 13 user test is
still open.

## Verification record

- Commit: the GitHub mirror publish corrected (2026-09-11). The
  422 failures were real: the pushed commits were missing on GitHub
  because a push went to Gitea only. The workflow never pushes git
  refs; every push goes to both forges (GitHub first), and the
  workflow only copies release artifacts. The tolerated tag delete no
  longer prints its 422.
- Commit: the README rewrite (2026-09-11). Two lists: "Added in
  comicrust" and "Not ported from ComicRack CE". The Windows migration
  section is now one instruction.
- Commit: the scan-robustness round (2026-09-11).
- `cargo fmt --all` — green.
- `cargo clippy --workspace --all-targets -- -D warnings` — green.
- `cargo test --workspace` — 561 pass (was 522).
- Probes: `scanrefresh` A-K green (release), `scanmarker` A-D green
  (release).
- `docs/guides/smart-list-queries.md` is gated by
  `cr-engine/tests/query_doc.rs` (every example parses; the field
  tables match the registry both ways) and
  `query_doc_semantics.rs` (the behaviour claims).
- Measured on the live CIFS library after the fix, with no contention:
  the 2.8 GB ZIP64 omnibus 0.664 s / 850 pages, the 33 MB
  RAR-named-`.cbz` 0.513 s / 57 pages, the 18 MB directory-less file
  0.014 s / rejected, a 67 MB CBZ 0.053 s / 37 pages. Before the fix
  the same files cost minutes to hours each.

## Open user tests

Run these in this order. Each one needs a rebuild first.

1. **Scan robustness and problem markers** (ADR-034, ADR-035) — rescan
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
2. **Double-click open crash fix** (commit `140ba4c`) — double-click a book
   in the grid. The reader opens with no abort. Read some pages, then close
   the tab. The green read-ribbon moves in the grid without a second click.
3. **Phase 13 config unification** — the full steps are in
   `docs/phases/phase-13.md`.
4. **Config seed + reference doc** (commit `d5c52b3`) — the first start
   writes every `[extended]` and `[engine]` key at its default. Set
   `DatabaseBackgroundSaving = 60`, restart, and the database saves every
   minute mid-scan. Delete a key line, restart, and the key returns at its
   default. Every key you look up is in `docs/config-reference.md`.
5. **Mid-scan background save** (commit `d9262a4`) — start a scan of a large
   folder. Within about 10 minutes `~/.local/share/comicrust/ComicDb/
   ComicDb.xml` appears on disk and holds the books found so far.
6. **Smart-list rule delete and clipboard operations** — open a smart list
   editor. Every rule row and group carries a small ▾ button at the right
   edge with New Rule, New Group, Delete, Cut, Copy, Paste, Move Up, and
   Move Down, with honest enable states. Delete removes a rule. Copy and
   Paste inserts a clone. A Query round trip stays clean. Test the
   Cut/Copy/Paste clipboard round trip on a real desktop, because Xvfb
   stalls those reads.
7. **"No metadata" tag** — books whose scan found no metadata carry a small
   dark "?" chip at the top left of the cover in Thumbnail and Tile view. A
   Properties edit to a key field, or a Comic Vine scrape, removes the chip.
8. **Scan and open metadata import** — rescan a folder that holds magazines
   with `ComicInfo.xml`. New files carry series, title, writer, and page
   metadata. Files added through Open carry it too. Properties on a
   non-library comic shows its metadata. Books already imported as empty
   stay empty (the user declined a backfill).
9. **Detail view round** (commits `c21086a`, `f20a690`) — switch the browser
   to Detail. After ONE slider drag the text size and row rhythm match
   ComicRack. The saved `ItemRowHeight` 48 artifact must be dragged off the
   slider once; the status-bar slider re-ranges 12..48. Rows alternate grey
   and white, starting grey, and the selection keeps the highlight. The thin
   vertical column lines run through the header and the rows. Right-click
   the column header: the 13 defaults, All (alphabetical), then A-B, C-F,
   G-O, P-R, S, T-Y. Every row toggles from every page. The smart-list
   editor rule rows pick the type from the All and letter menus.
10. **Group, lamp, and thumbnail batch** — Group by Series in Thumbnail,
    Tile, and Details view gives header strips with true counts. A
    single-click on the disclosure triangle collapses or expands ONE group. A
    double-click collapses or expands ALL groups. The Views menu row does the
    collapse and expand of all groups, and it grays out without grouping. The
    scan lamp animates while a scan runs, and a click on it opens the "Cancel
    scan" menu. Preferences ▸ Advanced ▸ Thumbnails off shows placeholders
    until File ▸ Generate Cover Thumbnails backfills them.
11. **Scan control** — Tasks ▸ Abort Scanning on a real scan. Then a
    graceful exit mid-scan: close the window or press Ctrl+C. The app exits
    promptly, and a restart shows the books found so far. (The progressive
    fill and the no-glitch append passed on 2026-09-10.)
12. **Export freeze fix** — re-run a CBR to CBZ export. The window stays
    responsive and the progress ticks.
13. **Write-back fix** — edit a property of a CBR or CB7 book, then run
    Update Book File(s). The UI stays responsive, the write lands, and the
    Files-to-update list clears.
14. **Phase 10 install and duplicate steps** — the steps at the tail of
    `docs/archive/phases/phase-10.md`.
15. **Phase 11 install steps** — the steps at the tail of
    `docs/archive/phases/phase-11.md`.

## Blockers

None.

## Known gaps

These are tracked in `docs/backlog.md`. They do not block the active phase.

- `WebComicProvider` is not ported.
- PDF and DjVu writers are missing.
- The `LICENSE` file is missing (a Phase 11 packaging gap).
- The T14 per-list sort deviation stands.
- HEIF and AVIF decode is missing.

## Environment notes

- `scanrefresh` gate E stalls mid-scan in a DEBUG build on this machine. The
  unmodified base HEAD fails the same gate in the same way, so this is a
  debug-timing environment flake, not a regression. Use the RELEASE run as
  the reference.
- `newbook`, `exportpage`, and `contextmenu` probes reach "PROBE DONE", then
  their internal watchdog fires with `rc=2`. The unmodified base HEAD shows
  the same shape. This is a pre-existing probe quirk.
- `editor_probe` runs a main loop forever by design. A kill under timeout is
  its normal completion.
