# Current status

Update this file at the end of every work session. Replace stale content.
Do not append a history. History lives in `docs/archive/` and in git.

## Active phase

**Phase 15 — Comic Vine cache, rate budget, and missing-issue fill.**
File: `docs/phases/phase-15.md`. Status: IMPLEMENTED (T1 to T7), plus
the Phase 15a visibility round. User test pending.
**Phase 16 — Comic Vine scraper quality of life** is planned and waits
for Phase 15 (`docs/phases/phase-16.md`).
Phase 13 is IMPLEMENTED, user test pending. Phases 0-8 and 10-12 are
COMPLETE (Phase 12 user-tested 2026-09-10); Phase 9 is DEFERRED to
`docs/backlog.md`.

## Current task

Phase 15 waits for its user test, which now covers the status-bar
lamp, the Tasks row, and the cancel. Phase 16 (the scraper dialog
quality of life, `docs/phases/phase-16.md`) starts after it.

Phase 14 (right-click rescans, ADR-036) is IMPLEMENTED and waits for
its user test. The navigator tree-state and Detail column-toggle fixes
(2026-09-12) wait for their user tests too. The scan-robustness round
(ADR-034, ADR-035) and the Phase 13 user tests are still open.

## Verification record

Phase 15a (2026-09-12), commits `8eba46d` onward: make background work
visible. Three defects the Phase 15 user test surfaced.

- **The scrape progress window was never presented.** It was built,
  filled, and closed, but `git log -S "window.present()"` shows that
  call has never existed in `dialogs/scrape.rs`. Every scrape since
  Phase 12 ran with no status list, no progress line, no budget
  readout, and NO REACHABLE CANCEL BUTTON. `scrape_probe` GATE V now
  checks visibility SYNCHRONOUSLY after the call: a timed check races,
  because the mock has zero delays and the run closes the window in
  under 200 ms. MEASURED both ways — the gate fails with `present()`
  removed and passes with it restored.
- **The report dialogs had no transient parent.** `show_info_dialog`
  and `show_error_dialog` used `.application(...)` with no
  `transient_for` and no `modal`, so the window manager put the cache
  report behind the main window. They were the only two such dialogs
  in `cr-ui`; the other 30 set a parent. They now take the window and
  are renamed `show_report_dialog` / `show_failure_dialog`. The forced
  `"Cannot open {title}"` heading is gone; the page-export path said
  "Cannot open" for a WRITE failure and now says "Cannot save".
- **The cache jobs reported nothing and could not be stopped.** The
  progress callback was `|_| {}` and the cancel flag was held by
  nothing. Every job now publishes over an mpsc channel that the MAIN
  thread drains (a worker that writes a thread-local writes its own
  copy), and it appears as a status-bar lamp with a live tooltip, a
  Tasks window row, and a working cancel. `Budget::with_wait_report`
  is wired at last, so a spent budget reads "the issues budget is
  spent, resuming at 14:32" instead of looking frozen for an hour.
  One job runs at a time: two sweeps would race on the one
  `sweep_state` row.
- Probes (release, Xvfb): `scrape_probe` GATE V plus A/B/C green;
  `statusbar_probe` A-J2 and I green, with the new K and K2 (the lamp
  follows the job slot, a second job is refused, the tooltip carries
  the live line, and the menu row reaches the worker's atomic flag).
- `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D
  warnings` — green.
- `cargo test --workspace` — 666 pass (was 660; 6 new cache-job gates).

## Open user tests

The steps are in `docs/open-user-tests.md`. Run them in this order.

1. Comic Vine cache (Phase 15, ADR-037, ADR-038)
2. Navigator tree state (2026-09-12)
3. Detail column add and remove (2026-09-12)
4. Right-click rescans (Phase 14, ADR-036)
5. Scan robustness and problem markers (ADR-034, ADR-035)
6. Double-click open crash fix (commit `140ba4c`)
7. Phase 13 config unification (ADR-033)
8. Config seed and reference document (commit `d5c52b3`)
9. Mid-scan background save (commit `d9262a4`)
10. Smart-list rule delete and clipboard
11. "No metadata" tag
12. Scan and open metadata import
13. Detail view round (commits `c21086a`, `f20a690`)
14. Group, lamp, and thumbnail batch
15. Scan control
16. Export freeze fix
17. Write-back fix
18. Phase 10 export and write-back steps
19. Phase 11 install steps

Delete a line here AND its section there when a test passes.

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
