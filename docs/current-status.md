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
then derive the year and the month for the issue picker.

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

## Open user tests

None. `docs/open-user-tests.md` explains how to add one.

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
- The T14 per-list sort deviation stands.
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
