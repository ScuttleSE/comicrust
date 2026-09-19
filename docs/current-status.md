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

`MEASURED` (user, 2026-09-19): Missing Issues scoped to a 2,484-book 2000 AD
smart list reports 2,559 missing rows. It groups 59 rows under `2000 AD` and
2,500 rows under `Unspecified`. At least one reported row has the same visible
Series, Number, and Comic Vine issue ID as an owned book.

`CODE-READ`: The gap pass compares normalized Number inside a stored
case-insensitive `(Series, Volume)` key. It does not use the Comic Vine issue ID
for ownership. The view hides Volume and Comic Vine volume ID. It can show
proposed values in place of empty stored values.

`UNKNOWN`: The stored field or hidden key that separates the owned book from
the gap row. `CR_TRACE` now reports each gap key, scope counts, blank Number
counts, volume votes, cache counts, and linked-issue conflicts without paths or
book IDs.

## Current task for the next context

Collect the Missing Issues trace from the reported 2000 AD scope. Use the
measured key mismatch to design the fix and its regression test. Do not design
the fix before this measurement.

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
- Test 24: Find Missing Issues in Incoming is not complete.
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

The Missing Issues diagnostic trace change passed on 2026-09-19.

- `cargo fmt --all`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed.
- `MEASURED`: a `CR_TRACE=1` unit-test run printed the expected aggregate group
  fields.
- `UNKNOWN`: the real 2000 AD trace has not run.

## Environment notes

- Run UI probes in release mode under Xvfb with `GDK_BACKEND=x11`,
  `DISPLAY=:99`, and isolated XDG paths under `/tmp/opencode/`.
- Never point a probe at the real library.
