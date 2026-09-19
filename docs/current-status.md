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

`MEASURED` (user, 2026-09-19): **Find in Incoming** did not find two candidates
for missing `2000 AD` number `2498`, volume `1977`. The Incoming view showed
number `2498` for both candidates. Their Series values were `2000AD` and
`2000AD prog`. Both Volume values were blank.

`CODE-READ`: the old matcher required exact normalized Series, Volume, and
Number values. The blank Incoming Volume rejected both candidates. The `prog`
suffix also rejected the first candidate.

ADR-068 keeps exact matching first and adds a bounded fallback. The Number must
match. A blank Incoming Volume can match, and one trailing Series word of at
most four characters can be ignored. A different nonblank Volume still does
not match. `MEASURED`: regression tests match both reported filenames and
reject wrong Number, Volume, Series, and long-suffix candidates. `UNKNOWN`: the
corrected result needs a real-library user test.

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

The Find in Incoming bounded-match fix passed on 2026-09-19.

- `cargo fmt --all`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed.
- `MEASURED`: both reported number `2498` filenames match in the regression
  test.
- `MEASURED`: conflicting Number, Volume, Series, and long-suffix candidates
  do not match in the regression test.
- `MEASURED`: a parallel transaction test exposed interference between two
  tests that shared the process-wide database epoch. A test-local mutex now
  serializes those two tests without changing their assertions. The complete
  transaction test binary and the final workspace run pass.
- `UNKNOWN`: the corrected real-library dialog result needs a user test.

## Environment notes

- Run UI probes in release mode under Xvfb with `GDK_BACKEND=x11`,
  `DISPLAY=:99`, and isolated XDG paths under `/tmp/opencode/`.
- Never point a probe at the real library.
