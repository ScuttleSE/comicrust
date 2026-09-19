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

`MEASURED` (user, 2026-09-19): **Link Series from Cache** required one command
for each series when a selection contained books from many series.

ADR-066 changes the command to process all selected series groups in display
order. Each group contains selected books with the same case-insensitive Series
and exact Volume. Books outside the selection do not change. A canceled volume
selection stops the remaining batch. One summary reports the completed batch.
`MEASURED`: the grouping test covers first-seen order, case-insensitive names,
and separate Volume values. `UNKNOWN`: the GTK batch flow needs a user test.

## Current task for the next context

Connect normal **Scrape from Comic Vine** to the persistent cache.

`CODE-READ`: `Cv::query_issue_refs`, `Cv::query_issue`, and the volume-details
path write reusable data to SQLite but do not read it before network requests.
A fully cached and linked book can therefore still request issue and volume
details.

The next context must first define the cache-use and forced-refresh rules. Then
it can make normal scraping read:

- cached issue-number maps before `/issues`;
- cached issue-detail JSON before `/issue/4000-<id>/`;
- cached volume JSON before `/volume/4050-<id>/`.

Add mock-server request-count tests. Keep explicit API refresh commands as the
way to bypass cached data. Do not infer freshness behavior without recording a
decision.

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

The Link Series from Cache batch change passed on 2026-09-19.

- `cargo fmt --all`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed.
- `MEASURED`: the batch grouping test passes.
- `UNKNOWN`: the GTK batch sequence has not had a user test.

## Environment notes

- Run UI probes in release mode under Xvfb with `GDK_BACKEND=x11`,
  `DISPLAY=:99`, and isolated XDG paths under `/tmp/opencode/`.
- Never point a probe at the real library.
