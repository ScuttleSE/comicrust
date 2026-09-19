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

`MEASURED` (user, 2026-09-19): Cached Comic Vine series propagation linked six
books in approximately 6 ms. The derived refresh work then made the UI slow and
used one CPU core. Smart-list evaluations took up to 8.5 seconds. Sorting 972
books took up to 4.95 seconds. Incoming gap analysis took up to 14.53 seconds.
One GTK frame took 8.12 seconds.

`CODE-READ`: string matchers request proposed filename data even when the
selected field does not use it. The process-wide proposed-value cache uses one
mutex, holds the mutex during a cache-miss parse, and clears all entries at
100,000 entries. The reported pass processed 88,988 Incoming books and 16,837
library books. `UNKNOWN`: the trace did not record cache clears, mutex wait
time, or thread identities. New `CR_TRACE` instrumentation records those values
without changing cache behavior.

## Current task for the next context

Run the same cached series propagation with `CR_TRACE=1` on the real library.
Use the new proposed-cache and thread measurements to identify the cause. Do
not design the fix before this measurement.

After this regression is resolved, retest **Find in Incoming** for `2000 AD`
number `2498`. Then resume the deferred persistent-cache read task.

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

1. `MEASURED`: DirectoryMatcher gauge evaluation took approximately 2.6
   seconds over 53,618 books on the GTK thread. Measure its internal phases,
   then move the work to a worker with the ADR-019 pump pattern.
2. `CODE-READ`: `ShowOnlyDuplicates` writes to ComicDb.xml but does not restore
   per-list UI state.
3. `MEASURED`: `duplicates_probe` gate F reads settings before the Preferences
   worker commits. Fix the probe wait. Do not change the expected value.
4. `UNKNOWN`: Confirm Incoming discard speed, clean close, and scroll restore
   on the real CIFS library. See ADR-058.
5. The `.deb` package has no local content test because this machine has no
   `dpkg-deb`.
6. AppStream metadata has no hosted screenshot URLs.
7. Phase 16, Comic Vine scraper quality of life, remains planned.
8. Phase 9, the library SQLite backend, remains deferred.

## Open risk

Apache-2.0 is incompatible with GPL-2.0-only. The Apache-2.0 scraper port,
`ring`, and `webpki-roots` remain unresolved under ADR-041. The project does
not claim that the present combination is permissible. `cargo deny check
licenses` is not a CI gate.

## Latest verification

The CPU-regression trace instrumentation passed on 2026-09-19.

- `cargo fmt --all`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed.
- `MEASURED`: a traced smart-list performance test reported thread identity,
  cache hits, cache misses, clears, entry count, lock wait, lock hold, parse,
  property access, property extraction, and string comparison time.
- `UNKNOWN`: the real-library reproduction is pending.

## Environment notes

- Run UI probes in release mode under Xvfb with `GDK_BACKEND=x11`,
  `DISPLAY=:99`, and isolated XDG paths under `/tmp/opencode/`.
- Never point a probe at the real library.
