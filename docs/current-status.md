# Current status

Update this file at the end of every work session. Replace stale content.
Do not append history. Git and `docs/archive/` hold history.

## Active phase

**Phase 20: Comic Vine cache manager.**

Implemented. The approved scope is in `docs/phases/phase-20.md` and ADR-064.
The File-menu dialog searches by Comic Vine volume ID. **Update from API**
stores all volume metadata and replaces the issue-number map. **Complete Update
from API** also stores every issue detail and resumes after interruption.

`MEASURED`: Mock-server tests pass for summary replacement, complete detail,
cancel, and resume. A version 1 cache migrates without losing its volume row.
The release GTK probe passed search, issue display, and manual metadata save.
The required workspace verification passes. `UNKNOWN`: user test 26 has not
run against the live Comic Vine API.

**Phase 19: the Missing Issues gap view.**

Implemented: the pure gap engine (`cr_scrape::cache::missing::missing_issues_of_library`),
the worker pass (`library::refresh_missing_issues_async`, `CvJobKind::MissingIssuesGap`),
the navigator entry and forced Detail-mode column set, and the scope
selector/Refresh bar. `cargo fmt --all`, `cargo clippy --workspace --all-targets
-- -D warnings`, and `cargo test --workspace` all pass. See
`docs/phases/phase-19.md` and ADR-059.

`MEASURED` (synthetic, not the real library): `crates/cr-scrape/tests/missing_perf.rs`
times a full gap pass over 22,000 books across 2,000 series (comparable to the
21,599-book real library named below) against an in-memory cache seeded with
60 issues per volume. Release: 66.0 ms. Debug: 354 ms (measured before the
2026-09-19 fix below added the second pass; the fix showed no measurable
release-mode cost, 63.5 ms -> 66.0 ms). Both are well inside a generous
10-second budget and confirm the owned-numbers map (locked decision 5) avoids
the O(S x N) shape that caused the 2026-09-18 idle-CPU incident. `UNKNOWN`: the
pass time on the user's real, multi-tens-of-thousands CIFS library is not yet
observed — ask the user to run Missing Issues ▸ Refresh once and report how
long it took.

`MEASURED` (user, 2026-09-19, found and fixed same day): a smart-list-scoped
refresh reported "0 missing issues" for a series (2000 AD, 2433/2448 owned)
that clearly had gaps. Root cause (`CODE-READ`): `missing_issues_of_library`
built its Comic Vine volume-id vote from the same scoped book slice as its
owned-numbers count; a smart list that happened to exclude every one of the
series' Comic-Vine-linked copies left the vote empty and the whole series was
skipped. Fixed by splitting the two populations — see ADR-059's 2026-09-19
follow-up. A new unit test reproduces the exact scenario; the UI probe and the
synthetic timing gate were re-run and show no regression.

`MEASURED` (user, 2026-09-19, second finding): after the fix above, the
user's "2000 AD" scope STILL read zero. `CODE-READ`: no book of that series
had ever been linked to a Comic Vine volume at all — the fix only widened
where the vote is read from, and there was no vote anywhere to find. The only
existing way to create a `comicvine_volume` link is a real scrape (or a
Comic-Vine-driven fill that already needs an existing vote); an MCL-only
import never sets it. This is a documented limitation (ADR-038, ADR-059,
phase-19.md), not a further bug.

`MEASURED` (local synthetic run, 2026-09-19): the Incoming Comic Vine gap
refresh timing gate improved from the handoff's approximately 535 ms to 196.7
ms for 20,000 books and 2,000 identities. `CODE-READ`: the refresh now reads
only the `comicvine_volume` custom value instead of building a full `BookData`
for each book. `CODE-READ`: `CR_TRACE` now reports times for snapshots, volume-id voting,
owned-number grouping, and cache/diff work. `UNKNOWN`: the exact pass time on
the user's real 21,397-book library.

`MEASURED` (user, 2026-09-19): the real-library retest after the round-2
change works correctly. The repeated long gap refresh and sustained idle-CPU
symptom did not recur. `UNKNOWN`: exact phase durations because the user
supplied no trace values. The four `CR_TRACE` phase records remain available
if the problem returns. No implementation work remains for this issue.

**New feature, 2026-09-19: "Link Series from Cache" (ADR-060).** Built in
response to the finding above: a new context-menu command that spends at
most one Comic Vine search per series (skipped entirely if any visible book
already votes a volume), then links the rest of the CURRENT VIEW's copies of
that series purely from the already-cached issue skeleton — no further
requests. See ADR-060 for the full design. `cargo fmt`/`clippy`/`test` all
pass; the new pure matcher (`crates/cr-scrape/src/cache/link.rs`) has 5 unit
tests. `UNKNOWN`: not yet run against the user's real "2000 AD" library — ask
them to right-click one unlinked issue, confirm exactly one Comic Vine
request fires, pick the volume, and confirm the visible copies get linked and
the Missing Issues report then shows real gaps.

**Follow-up, 2026-09-19: cached series metadata propagation (ADR-063).** Normal
Comic Vine searches and volume queries now retain shared volume metadata.
Complete issue-list queries retain their issue map. A full scrape of a
previously unlinked issue starts a worker that fills blank Publisher, Imprint,
volume year, and Comic Vine links across the matching library series. **Link
Series from Cache** fills the same blank fields in its current-view scope.
Existing values remain unchanged. `MEASURED`: unit tests cover blank-only
updates, existing-value preservation, unmatched issues, and query-to-cache
writes. The required workspace verification passes. `UNKNOWN`: user test 25
has not run on the real library.

**New feature, 2026-09-19: "Find in Incoming" (ADR-061).** The Missing Issues
context menu can match one or more selected gaps against Incoming by normalized
Series, Volume, and Number. The command uses a dedicated Library Organizer Move
profile from Preferences > Libraries. A summary requires the user to select one
copy when several Incoming files match. Confirmed matches use the existing
durable adoption transaction, then refresh Missing Issues. `MEASURED`: the
release `find_incoming_probe` passed under Xvfb with isolated XDG paths. It
selected one of two candidates, adopted only that copy, kept the other copy in
Incoming, inserted the adopted record into the main Library, and removed the
filled gap after refresh. `cargo fmt`/`clippy`/`test` all pass. `UNKNOWN`: user
test 24 has not run on the real library.

**Follow-up, 2026-09-19 (ADR-062).** `MEASURED` (user): Find in Incoming worked,
but its Organizer progress window had an empty main area and only bottom status
text. `CODE-READ`: successful Move and Copy operations sent progress counts but
no log entries. Organizer runs now emit preparation, current-operation, and
completion entries. Incoming > Gap Fills Adopt and Preview Adoption now use the
same dedicated profile as Find in Incoming. Adopt requires a summary
confirmation; Preview runs directly in simulation mode. `MEASURED`: all 24
Organizer mover tests pass, including log ordering. The expanded release
`find_incoming_probe` passed direct Gap Fills preview, no profile selector, no
preview mutation, the Adopt confirmation, and the Find in Incoming adoption.
`UNKNOWN`: the progress rows need confirmation on the user's real library.

`MEASURED`: The UI probe (`crates/cr-ui/examples/missing_issues_probe.rs`) ran
RELEASE under Xvfb with isolated XDG paths (see Environment notes below) and
passed three gates: the Missing Issues node forces Detail mode with the Cover
column hidden and exactly the Series/Number/Title/Year/Comic Vine Issue Id
columns visible; a whole-library refresh shows every seeded series' gaps (3
rows) with the library's book count unchanged; and scoping to a smart list
narrows the row set to that series' gaps alone (2 rows), still with no book
created. Confirmed stable across four consecutive runs. The
`docs/open-user-tests.md` entry for this phase is added but not yet run by
the user.

**Phase 18: Incoming folders.**

The implementation and automated gates are complete. The user tests are open.
See `docs/phases/phase-18.md`, `docs/open-user-tests.md`, and ADR-049 through
ADR-054.

Phase 16, Comic Vine scraper quality of life, is PLANNED. No task started.
Phase 9, the SQLite database backend, is DEFERRED to `docs/backlog.md`.

## Current task

Phase 20 implementation is complete. Run open user test 26 against a real
Comic Vine volume.

The user confirmed these four tests as passed on 2026-09-16:

- The 95,302-book Details view reaches both logical endpoints. Click and
  right-click actions reach rows near the endpoint.
- File opens without filling nested dynamic menus. Recent Books and Open
  Books fill when their own submenus open.
- Bulk deletion stays responsive. Progress, cancel, and the final refresh
  work on the real CIFS library.
- Cold startup shows the window before the watch-folder worker completes.
  The watcher installs later and detects a new file.

Incoming has source-specific duplicate views, persistent custom smart lists,
and side-by-side duplicate resolution. ADR-050 through ADR-055 record these
changes.

Library-duplicate batch handling changed on 2026-09-17. Compare now ranks each
shown pair and marks the preferred pane green and the worse pane red. A Keep
click runs in the background through a serial queue and Compare moves to the
next selected book at once. Accepted actions finish even after the window
closes. The old `DuplicatesIncomingPath` duplicate rule is removed. ADR-056 and
ADR-057 record these changes. The user tests are open (open user test 4 and 18).

`MEASURED`: On 2026-09-18 the app used 100% of one core while idle after
startup. `perf` and `gdb` located the cost in the "Incoming Comic Vine Gaps"
worker (`project_incoming_external_gaps` -> `incoming_volume_ids_for`), not the
GTK main thread. `incoming_volume_ids_for` recomputed `incoming_identity` (which
runs `proposed_cached`, `normalize_series`, and a SipHash) for every book once
per requested identity, an O(identities x books) pass over 21,599 library books
that did not complete in practical time. The fix computes each book's identity
and series key once in a single O(books) pass, then groups the series keys by
identity. The user confirmed idle CPU returns to zero on the real library on
2026-09-18. The `gauges::invalidate`, gap-refresh call/done, and mark-dirty
trace lines stay for future debugging; they fire per event, not in any inner
loop, and are gated by `CR_TRACE`.

`MEASURED`: A real replacement originally took 65.73 seconds. Ten durable
stages each rewrote a 3.34 GB JSON journal. The comic copy took only 152 ms.
ADR-055 replaced the embedded catalog arrays with two one-time sidecar files
and a compact journal. The final real-data test completed replacement in 5.36
seconds. Its 38.4 MB comic copy took 157 ms, and each compact journal update
took 9-11 ms. The replacement moved the selected Incoming file, updated both
catalogs, and produced no stale-scan popup. The user confirmed that the workflow
works much better on 2026-09-17.

`CODE-READ`: Watcher events for exact replacement paths are filtered after a
successful transaction. Unrelated events stay pending. `UNKNOWN`: The supplied
final trace ended before one complete watcher interval, so it does not prove
that no later automatic scan started.

`MEASURED`: A real discard of three Incoming files first took about 82 seconds
and used a lot of CPU, because each `IncomingTransaction` stage rewrote a 2.7 GB
JSON journal (discard still embedded the catalog arrays that ADR-055 removed for
replacement), and the app's own deletes then drove a full Incoming self-scan
that held the mutation guard and blocked close. ADR-058 gives every
`IncomingTransaction` kind (discard, adoption, undo, scan, folder conversion) the
sidecar journal, adds discard's deleted paths to the exact-path watcher
suppression set, moves the catalog serialization off the main thread to a single
worker-side pass, and restores the pre-refresh scroll offset in
`refresh_view_from_list`. A later four-file discard trace measured about 4.7
seconds of worker time, journal writes of 458-1138 bytes, and no post-commit
scan. `UNKNOWN`: The real-data discard speed, the clean close, and the scroll
restore need a user observation on the CIFS library.

## Open user tests

The steps are in `docs/open-user-tests.md`. The user ran a full pass on
2026-09-18. Results below; four items need follow-up in a new context.

1. Library-tree gauge badges. **FAIL**: no orange (Unread) badge; red and
   green always show the same number.
2. Library-tree drag and drop. OK.
3. Library-tree folder sort. OK.
4. Select Worst Duplicates. **PASSABLE**, needs improvement (not yet named).
5. Keyboard navigation and visibility. OK.
6. Watch-folder removal. OK.
7. Permanent delete in the browser. OK.
8. Permanent delete in Files view and delete-failure handling. OK.
9. Library Organizer simulation. **FAIL**: Simulate ran but no report shown.
10. Library Organizer move and destination conflicts. OK.
11. Library Organizer undo and profile import/export. OK.
12. Details mode thumbnail suppression, Ctrl+A, and Delete. OK.
13. macOS archive-junk recovery. OK.
14. Smart-list dialog responsiveness. OK.
15. Detail-column text overflow. OK.
16. Per-list view settings. **PASS**: two smart lists keep separate view
    modes when the user switches between them.
17. Incoming folder setup and review views. **PARTIAL PASS** ("so far so
    good"); full checklist not yet confirmed.
18. Incoming adoption, comparison, and undo. OK.
19. Incoming discard and Comic Vine refresh. OK.
20. Incoming responsiveness and role protection. OK.
21. Incoming smart lists. OK.

## Open work

- Phase 18 implementation is complete. Its five user tests remain open.
- The real-data replacement speed and stale-scan-popup checks passed. The other
  steps in Incoming user test 18 remain open.
- Phase 16 has eight planned Comic Vine scraper tasks. Start with T1 in
  `docs/phases/phase-16.md`.
- The Library Organizer startup auto-run is deferred in `docs/backlog.md`.
- A DirectoryMatcher gauge evaluation measured approximately 2.6 seconds
  over 53,618 books on the GTK thread. No follow-up fix exists.
- `ShowOnlyDuplicates` writes to ComicDb.xml but does not restore per list
  in the UI.
- The `.deb` has no local package-content test because this machine has no
  `dpkg-deb`. The packaging workflow is its first full test.
- The AppStream metadata has no screenshots because no hosted image URLs
  exist.
- Other deferred features are in `docs/backlog.md`. Do not copy that list
  into this file.

## Outstanding issues for a new context

This is the actionable to-do list. Each item names the evidence tag, the file,
and the next step, so a fresh session can start without re-deriving the state.
Follow the hard rules in `AGENTS.md`: measure before you name a cause, and do
not tweak a gate to pass.

1. **Gauge badges: no orange, and red equals green (user test 1, FAIL).**
   `MEASURED` (user, 2026-09-18): The Library tree shows only the red (New) and
   green (Total) badges. The orange (Unread) badge never renders, and red and
   green always show the same number. The next step is to read the gauge-badge
   render path and the count source for New, Unread, and Total, then measure
   which counts the tree receives.

2. **Library Organizer Simulate shows no report (user test 9, FAIL).**
   `MEASURED` (user, 2026-09-18): Simulate ran but no report appeared. The next
   step is to read the Simulate action path and where its report is meant to
   present.

3. **Select Worst Duplicates needs improvement (user test 4, PASSABLE).**
   `UNKNOWN`: The user reports it works but needs improvement. The specific
   improvement is not named. The next step is to ask the user what to improve.

4. **Confirm the Incoming discard fixes on real data (user test).** `UNKNOWN`:
   The discard speed, the clean close, and the scroll restore are proven only by
   local traces and unit tests, not by the CIFS library. Ask the user to: (a)
   discard several Incoming files and confirm the app stays responsive and exits
   normally without `kill -9`; (b) discard from far down a list and confirm the
   view stays put, with no `SCROLL JUMP ... -> 0` line for the in-place refresh;
   (c) confirm a list switch still starts at the top. See ADR-058 and open user
   test 19.

5. **`duplicates_probe` gate F is a stale-read (reported finding, do not tweak).**
   `MEASURED`: Gate F fails on this machine on both the work and the unmodified
   `main`. It reads session settings synchronously right after the Preferences
   OK response, but the Preferences commit lands on a later main-loop tick. Fix
   the probe to wait for the commit, or replace it with a user observation. Do
   not change the expected value to pass.

6. **DirectoryMatcher gauge evaluation runs on the GTK thread.** `MEASURED`:
   About 2.6 seconds over 53,618 books on the main thread (a Rule 9 violation).
   No fix exists. The next step is to measure where the time sits, then move the
   evaluation to a worker with the ADR-019 pump pattern in
   `docs/guides/gtk-and-ui.md`.

7. **`ShowOnlyDuplicates` does not restore per list in the UI.** `CODE-READ`:
   The value is written to ComicDb.xml but the per-list UI state is not restored
   on load. The next step is to read the per-list view-config load path and the
   `ShowOnlyDuplicates` field wiring.

8. **Packaging has no local `.deb` content test.** This machine has no
   `dpkg-deb`, so the packaging workflow is untested locally. The next step is a
   CI or user run of the packaging workflow and a check of the package contents.

9. **AppStream metadata has no screenshots.** No hosted image URLs exist. The
    next step is to host screenshots and add their URLs to the metadata.

10. **License incompatibility risk is unresolved (see Open risk below).**
    Apache-2.0 code (`ring`, `webpki-roots`, the scraper port) against
    GPL-2.0-only under ADR-041. `cargo deny check licenses` is not a CI gate. The
    next step is a licensing decision, not a code change.

## Open risk

Apache-2.0 is incompatible with GPL-2.0-only. The Apache-2.0 scraper port,
`ring`, and `webpki-roots` remain unresolved under ADR-041. The project does
not claim that the present combination is permissible. `cargo deny check
licenses` is not a CI gate.

## Latest verification

The Comic Vine cache manager passed local verification on 2026-09-19.

- `cargo fmt --all`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed.
- `MEASURED`: the release `cache_manager_probe` passed under Xvfb with isolated
  XDG paths.
- `UNKNOWN`: neither API update mode has run against the live Comic Vine API.

The shared Gap Fill adoption profile and Organizer progress-log change passed
local verification on 2026-09-19.

- `cargo fmt --all`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed.
- `MEASURED`: all 24 `cr-organize` mover integration tests pass. The new
  assertions cover preparation, current operation, completion, and the absence
  of a false success after failure.
- `MEASURED`: the expanded release `find_incoming_probe` passed direct Gap
  Fills preview, no general profile selector, no preview mutation, the Adopt
  confirmation, and the existing Find in Incoming adoption.
- `UNKNOWN`: the progress rows need confirmation during a real-library run.

The Find in Incoming feature passed local verification on 2026-09-19.

- `cargo fmt --all`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed.
- `MEASURED`: the release `find_incoming_probe` passed with an ambiguous
  two-copy match, one selected adoption, one retained Incoming copy, a main
  Library insert, and a Missing Issues refresh.
- `UNKNOWN`: user test 24 has not run on the real library.

The round-2 Incoming gap-refresh CPU change passed local verification on
2026-09-19.

- `cargo fmt --all`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed.
- `cargo test -p cr-ui --release
  project_incoming_external_gaps_completes_well_inside_budget -- --nocapture`:
  passed at 196.7 ms for 20,000 books and 2,000 series.
- `MEASURED` (user): the real-library retest works correctly. The repeated
  long refresh and sustained idle-CPU symptom did not recur.
- `UNKNOWN`: exact real-library phase durations. No trace values were supplied.

The idle-CPU fix (single-pass `incoming_volume_ids_for`) passed local
verification on 2026-09-18.

- `cargo fmt --all`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed.
- `MEASURED`: The user confirmed idle CPU returns to zero on the real library
  after the fix. Before the fix, `perf` showed ~99% of cycles in the Incoming
  Comic Vine gap worker; after the fix the gap pass completes and no thread
  stays hot.

The Incoming-transaction compact journal (all kinds), the discard watcher
suppression, the single worker-side catalog serialization, and the refresh
scroll restore passed local verification on 2026-09-18.

- `cargo fmt --all`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed.
- The transaction integration suite passes 33 tests. It covers replacement and
  the `IncomingTransaction` kinds at every durable stage, collisions, copy and
  trash failures, invalid files, discard, conversion, adoption, undo, stale
  epochs, and close behavior. The new
  `discard_keeps_the_journal_compact_and_removes_sidecars` test confirms that a
  discard keeps `current.json` below 4 KiB, writes at least one content sidecar,
  and removes every sidecar after commit.
- `UNKNOWN`: The discard speed, clean close, and scroll restore are not yet
  confirmed on the real CIFS library. See Outstanding issue 1.
- `MEASURED` (reported finding): The release `duplicates_probe` gate F fails on
  this machine on both the current work and the unmodified `main` revision. Gate
  F reads session settings synchronously right after the Preferences OK
  response, but the Preferences commit runs on a worker and lands on a later
  main-loop tick. The read is stale. This is a pre-existing probe or environment
  problem, not a code regression. It needs a probe change or a user
  observation; do not tweak it to pass. See Outstanding issue 2.

## Environment notes

- Run UI probes in RELEASE with Xvfb, `GDK_BACKEND=x11`, `DISPLAY=:99`, and
  isolated XDG paths under `/tmp/opencode/`. Probes must not use the real
  library.
- `scanrefresh` gate E stalls in a DEBUG build on this machine. The base
  revision has the same result. Use RELEASE.
- `newbook` and `exportpage` reach `PROBE DONE`, then their watchdog exits
  with code 2. The base revision has the same result.
- `editor_probe` runs a main loop until an external timeout. This result is
  its normal completion.
