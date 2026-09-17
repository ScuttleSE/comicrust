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

## Open user tests

The steps are in `docs/open-user-tests.md`.

1. Library-tree gauge badges.
2. Library-tree drag and drop.
3. Library-tree folder sort.
4. Select Worst Duplicates and the Incoming path rule.
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

## Open risk

Apache-2.0 is incompatible with GPL-2.0-only. The Apache-2.0 scraper port,
`ring`, and `webpki-roots` remain unresolved under ADR-041. The project does
not claim that the present combination is permissible. `cargo deny check
licenses` is not a CI gate.

## Latest verification

The compact replacement journal and watcher suppression passed local
verification on 2026-09-17.

- `cargo fmt --all`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed.
- The `cr-ui` suite passed 183 tests. Compare tests cover Keep-button action
  mapping, pair revalidation, comparison order, self-exclusion, ranking
  recommendations, and ties. A focused run confirms source-specific Keep-button
  mapping. A selection test confirms displayed-book order. The new watcher test
  confirms that rescan events stay pending during scans and Incoming operations.
- The `cr-engine` suite passed 150 tests. Incoming-list tests cover separate
  persistence, stable IDs, bases, invalid graphs, duplicate matching, and series
  statistics.
- Three isolated release `incoming_probe` runs passed Gates A-I, A2, E2, E2A,
  E3, and E4. E2A confirms Keep controls and recommendation highlighting. E4
  confirms Select Worst Duplicates in Incoming Duplicates. The other gates
  confirm replacement, navigation, covers, smart lists, and catalog isolation.
- The transaction integration suite passes 32 tests. It covers replacement at
  every durable stage, collisions, copy and trash failures, invalid files,
  discard, conversion, adoption, undo, stale epochs, and close behavior. The
  new test confirms that a replacement epoch invalidates a rescan that waits
  for the mutation guard. A `CR_TRACE=1` focused run contains the epoch
  transition, guard acquisition, source call sites, journal stages, and catalog
  installation stages.
- The compact-journal test confirms that replacement stores two one-time
  after-images, keeps `current.json` below 4 KiB for the test transaction, and
  removes all three files after commit. A watcher test confirms that exact
  transaction paths are filtered while unrelated events remain.
- An isolated release `incoming_probe` run passed Gates A-I, A2, E2, E2A, E3,
  and E4 with the compact journal.

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
