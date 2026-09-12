# Phase 14: Right-click rescans for books and lists

## Status

COMPLETE. User-tested 2026-09-12.

## Goal

Give the user a direct rescan of the books that failed their scan, and a
rescan of the books in one list. The scan timeout (ADR-035) leaves failed
books in the library with a marker. The normal rescan skips a known-bad
unchanged file, so only an explicit forced retry re-reads them.

## Scope

- One UI scan request that carries explicit file paths, a label, and a
  one-shot forced retry. Folder scans keep their shape.
- Book context menu: "Rescan Book File(s)" over the selected linked books.
- Right-click selection parity with the C# `UpdateSelectionFromMouse`
  (ItemView.cs:3855-3900): right-click on an unselected book selects
  only that book; right-click on a selected book keeps the
  multi-selection. The browser grid and the Files grid share the fix.
- Navigator context menu: "Scan List Contents" for smart lists and
  reading lists. The command evaluates the list the same way the browser
  fills it and scans the distinct non-empty file paths.
- Engine and UI unit tests; probe gates.

## Exclusions

- The LIFO scan-queue defect (queued requests run in reverse arrival
  order) stays out of scope. Each new command submits ONE request.
- The multi-root early-summary timing stays out of scope.
- No new configuration key. `ScanFileTimeoutSeconds` and
  `ScanRetryFailedFiles` keep their meanings; the commands override the
  retry for one request only.

## Locked decisions

- ADR-036 — an explicit scan is ONE request with a one-shot forced
  retry. The list scan is a port addition; the C# has no working
  list-scan command.

## Tasks

- [x] T1 — `cr-ui` scan request: `scan_files(paths, label, force_retry,
      done)`; the queue entry carries items + limits
      (`cr-ui/src/library.rs`).
- [x] T2 — the C# right-click selection rule in
      `ItemView::emit_context` (the browser grid and the Files grid
      share it).
- [x] T3 — the book menu row "Rescan Book File(s)"
      (`cr-ui/src/browser/shell.rs`); the "selection plus clicked
      target" unions are deleted — the selection is the target set.
- [x] T4 — the navigator row "Scan List Contents" (smart + reading
      lists; the Library root and folders never show it) and the
      `run_list_command` arm (`app.rs`).
- [x] T5 — tests: the engine explicit-path batch with the forced
      retry (`scan_status.rs`), the distinct-path collection
      (`library.rs` tests); probe gates: `contextmenu` S1/S2, the
      `scanmarker` E rescan round, `navpages` F/G/H/H2.

## Verification

Recorded 2026-09-12:

- `cargo fmt --all` — green.
- `cargo clippy --workspace --all-targets -- -D warnings` — green.
- `cargo test --workspace` — 563 pass (was 561).
- Probes (release, Xvfb): `scanmarker` A-E green (E = the book-menu
  rescan re-reads a known-bad book; a known-bad skip would show no
  summary), `contextmenu` ALL PASS (S1/S2 the selection rule), and
  `navpages` ALL PASS (F/G/H/H2 the menu rows per node kind).
- The navigator's ScanList dispatch into `run_list_command` is app
  code (`cr-app`); no probe drives it — the user test covers it.

## Open issues

None. The phase waits on the user test.

## Completion record

COMPLETE. User-tested 2026-09-12.

The book menu's "Rescan Book File(s)" re-reads the selected files with
a one-shot forced retry, so a known-bad unchanged file re-reads. The
navigator's "Scan List Contents" scans a smart or reading list. The
right-click selection follows the C# `UpdateSelectionFromMouse` rule in
both the browser grid and the Files grid.