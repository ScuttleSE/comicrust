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

`MEASURED`: `CR_TRACE` found 2,441 books with stored Series and Number values.
It found 43 books with both values empty and proposed metadata enabled. Both
groups use stored Volume 1977 and Comic Vine volume 19752. The old gap pass
made an empty-Series group for the 43 books and reported all 2,500 cached
issues in that group. Of these cached issues, 2,483 already had a linked book
in the scope.

ADR-067 makes Missing Issues use enabled Proposed Series and Number values when
the stored values are empty. `MEASURED`: the regression test combines stored
and proposed metadata into one group and reports only the absent issue.
`UNKNOWN`: the corrected count needs a real-library user test.

## Current task for the next context

Retest Missing Issues with the reported 2000 AD scope. Confirm that the
`Unspecified` group is absent and that the report contains only real gaps.

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

The Missing Issues proposed-value fix passed on 2026-09-19.

- `cargo fmt --all`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed.
- `MEASURED`: the proposed-Series and proposed-Number regression test passes.
- `UNKNOWN`: the corrected real 2000 AD result needs a user test.

## Environment notes

- Run UI probes in release mode under Xvfb with `GDK_BACKEND=x11`,
  `DISPLAY=:99`, and isolated XDG paths under `/tmp/opencode/`.
- Never point a probe at the real library.
