# Current status

Update this file at the end of every work session. Replace stale content.
Do not append a history. History lives in `docs/archive/` and in git.

## Active phase

**Phase 16 — Comic Vine scraper quality of life.**
File: `docs/phases/phase-16.md`. Status: PLANNED. No task started.

Phases 0-8 and 10-15 are COMPLETE. Phases 13, 14, and 15 were
user-tested on 2026-09-12 and are archived. Phase 9 is DEFERRED to
`docs/backlog.md`.

## Current task

Phase 16 T1 — add `cover_date` to `IssueRef` and to the issue queries,
then derive the year and the month for the issue picker. NOT STARTED.

Side task done on 2026-09-12: three user requests, all landed and
gated (ADR-039, ADR-040).

1. The page and thumbnail activity lamp (ADR-040) — a parity gap. The
   C# `tsPageActivity` was never ported. `ImagePool::is_working()`
   drives it; `ReadPagesAnimation.gif` is bundled as
   `assets/pages/frame-N.png` (16 frames); a click opens Tasks.
   MEASURED by `statusbar_probe` gate L.
2. Detail-view cells no longer spill into the next column. The header
   draw and the tile draw both clipped; the detail cell draw did
   neither (`item_view.rs:2449`). It now ellipsizes to the column
   width and clips as a backstop, through one shared
   `ellipsize_to_width` the tile path uses too. Four unit tests.
3. Per-list view settings (ADR-039). This CLOSES the T14 per-list
   sort deviation. The `<Item>/<Display>/<View>` subtree was already
   serde-complete and dead; it is now applied on list entry and
   stored on list leave, gated on a dirty flag. An absent `<View>`
   means inherit: the browser keeps the view it shows, which is the
   C# null-config behavior and the user's explicit choice over
   inheriting from the Library. "Reset View Settings" on the
   navigator context menu clears it. MEASURED by `browserbar_probe`
   gates G1-G4.

`browserbar_probe` gate E2 asserted that a list switch CLEARED the
sort — the deviation ADR-039 removes. Its expectation changed with
the behavior, not to make a failing gate pass.

Side task done on 2026-09-12, commit `a9873ac`: the smart-list guide now
holds a "books that have an author" recipe and a "Finding empty books"
section. No code change. The matcher still has no "is empty" operator,
and none was added: a new operator index would read as "match nothing"
in ComicRack (`ComicBookStringMatcher.cs:125`). `regex "."` is the
supported form.

Read `docs/phases/phase-16.md` first. It names the source of every
feature: the `Fableton/comic-vine-scraper-ce` fork of the Comic Vine
Scraper plugin. The port took its scraper from the UPSTREAM v1.0.102
release, so none of that fork's work is present.

## Verification record

Phases 13, 14, and 15 all passed their user tests on 2026-09-12. Their
records are in `docs/archive/phases/`.

Last commits, `5aea7c3` to `7cb852b` (Phase 15 and the 15a visibility
round):

- The Comic Vine cache is a two-layer SQLite file (ADR-037). MEASURED
  by mock-server gate, in API requests: a closed volume 0, an unchanged
  open volume 1 (the probe only), a changed open volume 2, an unknown
  volume 1 plus its pages.
- The sweep uses `filter=date_last_updated:<start>|<end>`. The API
  reference page renders its per-field filter marks as images, so that
  page does NOT state the filter is allowed; the production
  `update_missing.py` is the evidence.
- The budget default is 200 requests per resource per hour, from the
  user's figure. The API reference page carries NO rate-limit text at
  all, so the figure is NOT verified and the ceiling is the
  `CACHE_RATE_LIMIT` key.
- `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D
  warnings` — green.
- `cargo test --workspace` — 671 pass, 0 fail.
- Probes (release, Xvfb): `scrape_probe` GATE V and A/B/C,
  `statusbar_probe` A-K2, `scanmarker` A-E, `metadatatag` A/B.

The 2026-09-12 side task (ADR-039, ADR-040):

- `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D
  warnings` — green.
- `cargo test --workspace` — 680 pass, 0 fail (671 + 9 new: four for
  `ellipsize_to_width`, two for the `<View>` bridge, three existing
  counts unchanged).
- Probes (release, Xvfb): `statusbar_probe` A-L (L is the new page
  lamp: 16 frames, hidden at rest, the timer runs only while
  visible, the click reaches Tasks); `browserbar_probe` A-G4 (G1-G4
  are the new per-list cycle); `detailresize_probe` A-D;
  `workspace_probe`, `bootview_probe`, `bootreentry_probe`,
  `listorder_probe` — all unchanged and green.
- MEASURED constraint found while gating: the navigator selection is
  debounced 200 ms (`navigator.rs:45`). A view change made inside that
  window is attributed to the OUTGOING list. The first G-gate run
  failed on exactly this; the probe timing was corrected, not the
  expectation.

## Open user tests

Three, all from the 2026-09-12 side task. A passing probe is not a
passing user test.

1. **The thumbnail lamp.** Run Generate Thumbnails on a large list.
   A small animated icon must appear in the status bar while the work
   runs and disappear when it ends. A click on it must open Tasks.
2. **Detail-column overflow.** Switch to Details and narrow a column
   that holds long text (Series, or Title). The text must end in an
   ellipsis at the column edge and must NOT paint over the next
   column.
3. **Per-list view settings.** Set list A to Details with a small row
   height, and list B to Thumbnails with a large thumbnail. Switch
   between them: each must come back the way you left it. Then make a
   NEW list and select it — it must show the view you came from, and
   must not change until you change it. Right-click list B, choose
   "Reset View Settings", leave it and come back: it must no longer
   force its own view. Restart the app and confirm all of it
   survived.

## Blockers

None.

## Lessons from the Phase 15 user test

Four defects reached the user because no gate watched them. Read these
before starting Phase 16; the same traps are in the dialogs it touches.

1. **A GTK4 window is invisible until `present()` is called.** The
   scrape progress window was built, filled, and closed but never
   presented, from Phase 12 until 2026-09-12. `scrape_probe` GATE V now
   checks it. Add the same check for any new window.
2. **A dialog with `.application(...)` and no `.transient_for(...)`
   lands behind the main window.** Every dialog needs the window as its
   parent. Use `show_report_dialog` or `show_failure_dialog` in
   `browser/shell.rs`, which do it correctly.
3. **A worker thread must never write the UI thread-locals.** It writes
   its own copy. Send over an `mpsc` channel and let the main-thread
   pump write the state. The scan and the Comic Vine cache jobs both
   use this shape.
4. **Long work must show progress and offer a cancel.** A job with no
   indicator reads as a job that did nothing. The status-bar lamp and
   the Tasks window row are the two surfaces; see
   `library::start_cv_job` and the `Comic Vine cache` block in
   `dialogs/tasks.rs`.

One process lesson: when a user reports "nothing happened", read the
presentation path before theorising about the engine.

## Known gaps

Tracked in `docs/backlog.md`; they do not block Phase 16.

- `WebComicProvider` is not ported.
- PDF and DjVu writers are missing.
- The `LICENSE` file is missing (a Phase 11 packaging gap).
- HEIF and AVIF decode is missing.
- "Fill Missing Issues" has no volume picker for an unscraped series.
- The Comic Vine cache jobs have no per-row abort in the Tasks window;
  the lamp menu carries the targeted cancel.

## Environment notes

- Run UI probes in RELEASE with `Xvfb :99`, `GDK_BACKEND=x11`,
  `DISPLAY=:99`, and isolated `XDG_DATA_HOME` and `XDG_CONFIG_HOME`
  under `/tmp/opencode/`. The probes refuse to run against the real
  library.
- `scanrefresh` gate E stalls mid-scan in a DEBUG build on this
  machine; the unmodified base HEAD fails the same way. Use RELEASE.
- `newbook` and `exportpage` probes reach "PROBE DONE", then their
  internal watchdog fires with `rc=2`. The unmodified base HEAD shows
  the same shape — a pre-existing probe quirk.
- `editor_probe` runs a main loop forever by design; a kill under
  timeout is its normal completion.
