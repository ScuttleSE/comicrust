# Current status

Update this file at the end of every work session. Replace stale content.
Do not append history. Git and `docs/archive/` hold history.

## Active phase

**Phase 18: Incoming folders.**

The implementation and automated gates are complete. The user tests are open.
See `docs/phases/phase-18.md`, `docs/open-user-tests.md`, and ADR-049.

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

## Open work

- Phase 18 implementation is complete. Its four user tests remain open.
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

The Incoming-folder implementation passed local verification on 2026-09-17.

- `cargo fmt --all`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed.
- The release `incoming_probe` passed Gates A-I. It confirms isolated storage,
  navigator structure, configuration, scan isolation, review classification,
  transactional conversion, ID-preserving adoption, simulation immutability,
  operation serialization, and reload persistence.
- The transaction integration suite passed 21 tests. It covers recovery,
  overwrite, discard, conversion, stale epochs, and close behavior.

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
