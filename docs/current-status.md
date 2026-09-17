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

Incoming has source-specific duplicate views and persistent custom smart lists.
Compare shows covers and details side by side. It can resolve Library and
Incoming duplicates. A durable transaction supports cross-filesystem Library
replacement. ADR-050 through ADR-053 record these changes. User tests on real
data remain open.

Compare now has one Keep This Copy button below each pane. It stops an active
scan before it rechecks and runs the selected action. Incoming Duplicates also
has the selection-only Select Worst Duplicates context command. ADR-054 records
these changes. User tests on real data remain open.

`MEASURED`: The first real replacement user test moved the Incoming file and
removed its Incoming record. A later Incoming scan reported a stale database
epoch. `UNKNOWN`: The run-time cause and the reported 30-second interval.
`CODE-READ`: `CR_TRACE` now records scan admission, watcher delivery,
mutation-guard waits, operation ownership, replacement stages, and epoch changes
with source call sites. Repeat the replacement once with `CR_TRACE=1` to collect
the deciding measurement.

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

Incoming replacement and rescan instrumentation passed local verification on
2026-09-17.

- `cargo fmt --all`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed.
- The `cr-ui` suite passed 182 tests. Compare tests cover Keep-button action
  mapping, pair revalidation, comparison order, self-exclusion, ranking
  recommendations, and ties. A selection test confirms displayed-book order.
- The `cr-engine` suite passed 149 tests. Incoming-list tests cover separate
  persistence, stable IDs, bases, invalid graphs, duplicate matching, and series
  statistics.
- Three isolated release `incoming_probe` runs passed Gates A-I, A2, E2, E2A,
  E3, and E4. E2A confirms Keep controls and recommendation highlighting. E4
  confirms Select Worst Duplicates in Incoming Duplicates. The other gates
  confirm replacement, navigation, covers, smart lists, and catalog isolation.
- The transaction integration suite passes 31 tests. It covers replacement at
  every durable stage, collisions, copy and trash failures, invalid files,
  discard, conversion, adoption, undo, stale epochs, and close behavior. The
  new test confirms that a replacement epoch invalidates a rescan that waits
  for the mutation guard. A `CR_TRACE=1` focused run contains the epoch
  transition, guard acquisition, and source call sites.

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
