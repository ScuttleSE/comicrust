# Phase 10 — CBR/RAR write-back

Status: ACTIVE (2026-09-09). T1-T4 IMPLEMENTED (commit cbbd689 +
9eb9df1; fmt/clippy/400 tests green); BOTH USER TESTS PENDING —
steps 1-4 (rar write-back) and steps 5-8 (export post-processing) at
the end of this file. User decision: real in-archive metadata writes
for CBR/RAR. The xattr (`NtfsInfoStorage`) parity write-back was
OFFERED and DECLINED — DB stays the master copy for users without
the `rar` binary.

## Source study (the C# facts)

- `CbrComicProvider.cs:5` and `Rar5ComicProvider` carry NO
  `EnableUpdate` — the C# never writes into RAR archives. Only CBZ,
  CBT, CB7, CBW are updatable (`FileFormatAttribute.EnableUpdate`).
- `ComicProvider.StoreInfo` (ComicProvider.cs:82) always also writes
  the NTFS ADS stream (`NtfsInfoStorage.StoreInfo`) — that is how CBR
  books persist edits on Windows. Our port has that xattr chain for
  reads (ADR-006) and `ComicProvider::store_info` keeps it as a write
  fallback; the app's `update_book_file` does not use it (user
  decision above).
- No open-source RAR compressor exists. 7-Zip and libarchive decode
  only; unrar is extract-only and GPL-incompatible. The only writer
  is RARLAB's `rar` CLI (freeware, closed source). Subprocess-only
  use of a user-installed binary follows the `7z` precedent
  (ADR-007); the addition is recorded in ADR-030.

## T1 — the `rar` writer (`cr-io`)

- `rar.rs`: `find_rar()` (`CR_RAR` env override, then `rar` on PATH;
  never `unrar`) + `add_files()` — one `rar a -p- -y <archive>
  <files>` with cwd at the staging directory so entries land at the
  archive root under bare names. `-p-` fails instead of prompting (a
  prompted subprocess would hang); stdin is null.
- `write.rs`: `store_info_scoped` handles `ids::CBR | ids::RAR5` —
  stage the ComicInfo.xml/ComicBook.xml pairs in one temp dir, one
  `rar a` call, success reports changed (mirrors the 7z update path).
  `with_book_info` scoping as for CB7.
- `supports_update` stays FALSE for RAR formats in the registry (C#
  parity: RAR is not unconditionally updatable). Consumers checked:
  only the write gate + `ComicProvider::store_info`.
- Failure semantics: `rar` missing or nonzero exit → `Error::Access`.
  The app path (`update_book_file`) surfaces it; the queue path
  swallows (the C# silent-failure shape) and the book stays in
  "Files to update".
- Works for RAR4 and RAR5 targets (`rar a` keeps the archive's
  existing format version).

## T2 — tests

- `tests/rar_gated.rs`: gated on `CR_RAR_TESTS=1` + `rar` (+ `7z`
  for the read side) present. Builds a `.cbr` fixture with `rar a`
  in-test (no committed RAR data), writes ComicInfo through
  `store_info_scoped`, re-reads through `load_info(Slow)` (in-archive
  wins), asserts the round-trip and that pages survive.
- CI-runnable: the missing-binary error path (runs wherever `rar` is
  absent).

## T3 — records

- ADR-030 (the beyond-parity addition + license hygiene: never
  bundled, linked, or distributed).
- README format table row: CBR/RAR read via 7z; write-back needs the
  RARLAB `rar` CLI.
- User install notes: Arch (AUR `rar`), Debian/Ubuntu (`non-free`),
  Fedora (RPM Fusion), or the RARLAB static tarball; `CR_RAR` overrides.

## T4 — export post-processing (the rar→zip conversion path)

Added 2026-09-09 (user request "convert rar-files to zip"). The C#
converts formats through Export with target "Replace source": the
`QueueManager.ExportComic` post-export block (QueueManager.cs:455-508)
re-points the book, trashes the old file, and manages the database.
The port carried the dialog flags and the engine but consumed
neither.

- `cr-core`: `ComicInfo::set_info` (the ComicInfo.cs:1210-1418
  field-by-field port with the per-type empty rules) +
  `ComicBook::set_info` (the ComicBook.cs:2662 reading-position
  clamps). Unit-tested.
- `cr-io`: `export_book`/`export_books_combined` return the output
  path; `build_export_info` is public (`ComicExporter.ComicInfo`).
- `cr-ui`: `library::export_post_process` (+ `_with` with the trash
  step injected for tests) — replace-source re-point +
  `refresh_file_info_basic` + info set-back + the FromComic
  color-adjustment reset + dirty clears (the `wasReplaced` rule
  covers the same-path overwrite); delete-original; add-to-library
  (`ComicBookFactory.Create` parity via the `open_book` shape).
  Sources are filtered `!= outPath` before both branches; the
  by-path DB removal cannot hit the re-pointed key book (the C#
  order — write-back first). A failed trash skips only that source's
  removal and is reported (the C# `ShellFile.DeleteFile` throw
  shape); `trash_path` keeps the Phase 7 guards (empty path /
  non-file never reach gio).
- Dialog: the OK path runs the surgery per group (combine = one
  group) and surfaces surgery errors in the error label;
  "Add to library" disables when target = Replace source
  (ExportComicsDialog.cs:204).
- Deviations: the C# `RefreshInfoFromFile` pre-export pass and the
  `FileIsInDatabase` duplicate-target guard are not ported
  (pre-existing export-engine scope).
- Gate: `cr-ui/tests/export_surgery.rs` (isolated XDG; a fake trash
  because `gio` refuses tmpfs; the three flows + reading state +
  dirty rule). USER TEST below.

## User test (T4 addition)

5. Select a `.cbr` book → "Export…" → Target = "Replace source",
   Format = eComic (ZIP) → OK.
6. Verify: a `.cbz` sits next to the old file, the `.cbr` is in the
   trash, the library book now points at the `.cbz` (opens fine,
   reading position kept), and the book left "Files to update".
7. Repeat with "Delete original files after export" unchecked and
   Target = "Export to new folder": both files remain, the library
   book still points at the `.cbr`.
8. "Add exported files to the library" with a new folder: the library
   gains a second book for the export.

## User test

1. Install `rar` (see T3 notes), set nothing else (`CR_RAR` optional).
2. Open a library with a `.cbr`, edit a field (Properties…), run
   "Update Book File(s)".
3. Verify: no error; the book leaves "Files to update"; `7z l` on the
   file (or a fresh re-import on another machine) shows the edited
   ComicInfo.xml inside the archive.
4. Unset `rar` from PATH (or `CR_RAR=/nonexistent`) and repeat the
   edit + update: expect a clear "rar executable not found" error on
   the manual command and no data loss.
