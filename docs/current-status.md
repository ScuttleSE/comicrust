# Current status

Update this file at the end of every work session. Replace stale content.
Do not append history. Git and `docs/archive/` hold history.

## Active phase

**Phase 20: Comic Vine cache manager.**

Implemented. User test pending. See `docs/phases/phase-20.md`, ADR-064, and
open user test 26.

The File-menu dialog searches by Comic Vine volume ID. **Update from API**
stores the complete volume response and replaces the issue-number map.
**Complete Update from API** also stores each issue detail and resumes after an
interruption. Both operations use the shared request budget and worker-job UI.

`MEASURED`: Mock-server tests cover summary replacement, complete detail,
cancel, and resume. A version 1 cache migrates without losing its volume row.
The release GTK probe passed cache search, issue display, and manual metadata
save. `UNKNOWN`: neither API update mode has run against the live API.

## Latest user finding

`MEASURED` (user, 2026-09-19): The new trace identified the CPU and UI stall.
The Incoming gap worker parsed 88,129 paths while it held the global proposed-
value cache mutex for 7.25 seconds. GTK operations waited on that mutex for up
to 6.62 seconds. The 100,000-entry cache cleared repeatedly while the active
Library and Incoming catalogs contained up to 105,825 paths. Cached Comic Vine
series propagation itself changed three books in approximately 7 ms.

`CODE-READ`: the fix removes the fixed cache limit, reserves for all active
paths at startup, and parses through per-path `OnceLock` values outside the
global map lock. String matchers request proposed values only for Series,
Title, and Format. Gauge evaluation runs on a worker. A new generation cancels
obsolete Incoming gap work.

`MEASURED` (user, 2026-09-19): The same real-library workflow completed in one
to two seconds after the fix. The user reports that it is much better. The
scrape matched the selected book and three other books in the same series.
`UNKNOWN`: peak memory use was not measured.

## Current task for the next context

Retest **Find in Incoming** for `2000 AD` number `2498`. Confirm that the dialog
shows both reported candidates. Then complete the remaining Missing Issues
scope test and confirm that the report contains only real gaps.

After this defect is resolved, resume the deferred task: connect normal
**Scrape from Comic Vine** to persistent-cache reads. Define cache-use and
forced-refresh rules before that implementation.

## Open user tests

The procedures are in `docs/open-user-tests.md`.

- Test 1: Library-tree gauge badges failed. No orange Unread badge appeared,
  and red New equaled green Total.
- Test 4: Select Worst Duplicates is passable. `UNKNOWN`: the requested
  improvement is not specified.
- Test 9: Library Organizer simulation showed no report.
- Test 17: Incoming folder setup and review views have a partial pass.
- Test 22: Missing Issues gap report is not complete.
- Test 23: Link Series from Cache is not complete.
- Test 24: Find Missing Issues in Incoming has a bounded-match fix. The real
  `2000 AD` number `2498` retest is pending.
- Test 25: Cached series metadata propagation is not complete. The Proposed
  Number regression now passes on the real library.
- Test 26: Comic Vine cache manager is not complete. Test both API modes
  against a real volume.

## Other open work

1. `CODE-READ`: `ShowOnlyDuplicates` writes to ComicDb.xml but does not restore
   per-list UI state.
2. `MEASURED`: `duplicates_probe` gate F reads settings before the Preferences
   worker commits. Fix the probe wait. Do not change the expected value.
3. `UNKNOWN`: Confirm Incoming discard speed, clean close, and scroll restore
   on the real CIFS library. See ADR-058.
4. The `.deb` package has no local content test because this machine has no
   `dpkg-deb`.
5. AppStream metadata has no hosted screenshot URLs.
6. Phase 16, Comic Vine scraper quality of life, remains planned.
7. Phase 9, the library SQLite backend, remains deferred.

## Open risk

Apache-2.0 is incompatible with GPL-2.0-only. The Apache-2.0 scraper port,
`ring`, and `webpki-roots` remain unresolved under ADR-041. The project does
not claim that the present combination is permissible. `cargo deny check
licenses` is not a CI gate.

## Latest verification

The proposed-cache contention fix passed on 2026-09-19.

- `cargo fmt --all`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed.
- `MEASURED`: the 150,001-entry cache regression test passed.
- `MEASURED`: the gauge worker ordering test passed.
- `MEASURED`: stale gauge and gap worker cancellation tests passed.
- `MEASURED`: the release gauge probe passed all gates with fresh isolated
  data. A prior rerun reused mutated probe data and failed its value checks.
- `MEASURED`: all direct users of the process-wide Incoming mutation guard now
  use one test-local mutex. The transaction test binary and workspace pass.
- `MEASURED` (user): the real-library propagation workflow completed in one to
  two seconds and matched three other books in the series.
- `UNKNOWN`: peak memory use on the real library was not measured.

## Environment notes

- Run UI probes in release mode under Xvfb with `GDK_BACKEND=x11`,
  `DISPLAY=:99`, and isolated XDG paths under `/tmp/opencode/`.
- Never point a probe at the real library.
