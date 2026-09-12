# Current status

Update this file at the end of every work session. Replace stale content.
Do not append a history. History lives in `docs/archive/` and in git.

## Active phase

**Phase 15 — Comic Vine cache, rate budget, and missing-issue fill.**
File: `docs/phases/phase-15.md`. Status: IMPLEMENTED (T1 to T7),
user test pending.
**Phase 16 — Comic Vine scraper quality of life** is planned and waits
for Phase 15 (`docs/phases/phase-16.md`).
Phase 13 is IMPLEMENTED, user test pending. Phases 0-8 and 10-12 are
COMPLETE (Phase 12 user-tested 2026-09-10); Phase 9 is DEFERRED to
`docs/backlog.md`.

## Current task

Phase 15 waits for its user test. Phase 16 (the scraper dialog quality
of life, `docs/phases/phase-16.md`) starts after it.

Phase 14 (right-click rescans, ADR-036) is IMPLEMENTED and waits for
its user test. The navigator tree-state and Detail column-toggle fixes
(2026-09-12) wait for their user tests too. The scan-robustness round
(ADR-034, ADR-035) and the Phase 13 user tests are still open.

## Verification record

Phase 15 (2026-09-12), commits `5aea7c3` to `f930cb8`. The older
records are in git.

- The cache is a two-layer SQLite file (ADR-037). MEASURED by gate, in
  API requests: a closed volume 0, an unchanged open volume 1 (the
  probe only), a changed open volume 2, an unknown volume 1 plus its
  pages. A second call on a volume just paged asks for nothing.
- The merge rule is gated: a cheap write (an MCL import, or the sweep's
  `id,issue_number,volume` field list) never erases what an expensive
  query found. `fetched_at` merges with MAX, so a probe cannot move the
  check time backwards.
- The MCL reader accepts what the `Update Missing` writer really
  produces, not what its docstring promises: the trailing comma on the
  number list, the `.&@1` and `.&@2` escapes that the writer never
  reverses, the quoted list it never emits, and a comma that a space
  follows inside a number. Fixtures pin all four, plus volume 77901.
- The sweep uses `filter=date_last_updated:<start>|<end>`. The API
  reference page renders its per-field filter marks as images, so that
  page does NOT state the filter is allowed; the production
  `update_missing.py` is the evidence. Gated: a cancelled sweep
  resumes and pays for no repeated page, a complete sweep makes zero
  requests on a re-run, a new window restarts at zero, and the page cap
  keeps its offset.
- The budget default is 200 requests per resource per hour, from the
  user's figure. The API reference page carries NO rate-limit text at
  all (no "200", no "per resource", no 420, and a `status_code` table
  that stops at 105), so the figure is NOT verified here and the
  ceiling is the `CACHE_RATE_LIMIT` key.
- DEVIATION: the warm task and the sweep report in a completion dialog,
  not in the Tasks window. A Tasks row is on `docs/backlog.md`.
- Image downloads do not pass the budget. They come from the image
  host, not from an API resource.
- `cargo fmt --all` and `cargo clippy --workspace --all-targets -- -D
  warnings` — green.
- `cargo test --workspace` — 660 pass (was 564; 96 new cache, MCL,
  sweep, freshness, budget, warm, and missing-issue gates).
- No probe drives the new dialogs or commands; the user test covers
  them, as the navigator list command does in Phase 14.

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
