# Phase 10 — CBR/RAR write-back

Status: ACTIVE (2026-09-09). User decision: real in-archive metadata
writes for CBR/RAR. The xattr (`NtfsInfoStorage`) parity write-back
was OFFERED and DECLINED — DB stays the master copy for users without
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
