# Current status

Update this file at the end of every work session. Replace stale content.
Do not append history. Git and `docs/archive/` hold history.

## Active phase

**Phase 18: Incoming folders.**

The implementation and automated gates are complete. The user tests are open.
See `docs/phases/phase-18.md`, `docs/open-user-tests.md`, and ADR-049 through
ADR-054.

Phase 16, Comic Vine scraper quality of life, is PLANNED. No task started.
Phase 9, the SQLite database backend, is DEFERRED to `docs/backlog.md`.

## Current task

The Incoming workflow is implemented. The user procedure is in
`docs/guides/incoming-folders.md`.

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

The steps are in `docs/open-user-tests.md`.

1. Library-tree gauge badges.
2. Library-tree drag and drop.
3. Library-tree folder sort.
4. Select Worst Duplicates.
5. Keyboard navigation and visibility.
6. Watch-folder removal.
7. Permanent delete in the browser.
8. Permanent delete in Files view and delete-failure handling.
9. Library Organizer simulation.
10. Library Organizer move and destination conflicts.
11. Library Organizer undo and profile import/export.
12. Details mode thumbnail suppression, Ctrl+A, and Delete.
13. macOS archive-junk recovery.
14. Smart-list dialog responsiveness.
15. Detail-column text overflow.
16. Per-list view settings.
17. Incoming folder setup and review views.
18. Incoming adoption, comparison, and undo.
19. Incoming discard and Comic Vine refresh.
20. Incoming responsiveness and role protection.
21. Incoming smart lists.

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

1. **Confirm the Incoming discard fixes on real data (user test).** `UNKNOWN`:
   The discard speed, the clean close, and the scroll restore are proven only by
   local traces and unit tests, not by the CIFS library. Ask the user to: (a)
   discard several Incoming files and confirm the app stays responsive and exits
   normally without `kill -9`; (b) discard from far down a list and confirm the
   view stays put, with no `SCROLL JUMP ... -> 0` line for the in-place refresh;
   (c) confirm a list switch still starts at the top. See ADR-058 and open user
   test 19.

2. **`duplicates_probe` gate F is a stale-read (reported finding, do not tweak).**
   `MEASURED`: Gate F fails on this machine on both the work and the unmodified
   `main`. It reads session settings synchronously right after the Preferences
   OK response, but the Preferences commit lands on a later main-loop tick. Fix
   the probe to wait for the commit, or replace it with a user observation. Do
   not change the expected value to pass.

3. **DirectoryMatcher gauge evaluation runs on the GTK thread.** `MEASURED`:
   About 2.6 seconds over 53,618 books on the main thread (a Rule 9 violation).
   No fix exists. The next step is to measure where the time sits, then move the
   evaluation to a worker with the ADR-019 pump pattern in
   `docs/guides/gtk-and-ui.md`.

4. **`ShowOnlyDuplicates` does not restore per list in the UI.** `CODE-READ`:
   The value is written to ComicDb.xml but the per-list UI state is not restored
   on load. The next step is to read the per-list view-config load path and the
   `ShowOnlyDuplicates` field wiring.

5. **Packaging has no local `.deb` content test.** This machine has no
   `dpkg-deb`, so the packaging workflow is untested locally. The next step is a
   CI or user run of the packaging workflow and a check of the package contents.

6. **AppStream metadata has no screenshots.** No hosted image URLs exist. The
   next step is to host screenshots and add their URLs to the metadata.

7. **License incompatibility risk is unresolved (see Open risk below).**
   Apache-2.0 code (`ring`, `webpki-roots`, the scraper port) against
   GPL-2.0-only under ADR-041. `cargo deny check licenses` is not a CI gate. The
   next step is a licensing decision, not a code change.

## Open risk

Apache-2.0 is incompatible with GPL-2.0-only. The Apache-2.0 scraper port,
`ring`, and `webpki-roots` remain unresolved under ADR-041. The project does
not claim that the present combination is permissible. `cargo deny check
licenses` is not a CI gate.

## Latest verification

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
