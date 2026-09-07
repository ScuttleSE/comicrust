# Phase 9 Kickoff — Database Backend (SQLite canonical)

Goal: move the library store from whole-file `ComicDb.xml` to SQLite.
The byte-stable XML codec stays. After migration it is the ComicRack
import format and the on-demand "Export for ComicRack…" writer.

Origin: Phase 8 T7 (the user-reported item 4, 2026-09-06) spun out
into this phase on 2026-09-07. The exploration + the user decisions
of that session are the recorded basis below. This phase starts after
Phase 8 closes.

## Locked decisions (user, 2026-09-07)

- **SQLite becomes the canonical store** after an explicit user
  migration. Full design: hot columns + per-book dirty tracking +
  incremental row updates (not a document-store stopgap).
- **Postgres: rejected** for the local store. A single-user desktop
  app needs no server, no daemon, no credentials. The server-shaped
  feature was already descoped once (ADR-028). Record the reasoning
  in ADR-029 regardless.
- **Interop re-scope:** ComicRack CE round-trip matters for the
  INITIAL IMPORT only. After migration, XML is an import + on-demand
  export format. The "Export for ComicRack…" item regenerates the
  byte-stable XML (the existing Emitter, unchanged).
- **Migration UX:** a one-time Settings switch ("Migrate to
  SQLite…"). The XML is archived read-only at migration
  (`ComicDb.xml.imported`).
- **Fresh installs keep the XML default** (today's behavior) until
  the user migrates. No XML defaults are created after a migration.
- **Scale target:** 10k-100k books (today's reality: 255 books,
  584 KB).

## Constraints

- **Invariant 1 re-scoped, not dropped:** the ComicDb.xml
  Emitter/reader stays byte-stable and stays golden-tested. It
  changes role: canonical store → import/export codec.
- **The database is the one artifact users cannot lose.** The
  migration and every save path need verification gates before any
  switch.
- **ADR-029 is REQUIRED before any product code** (the Phase 8 T7
  rule carries over). The spike (T1) fills the numbers into the ADR;
  the user signs off at T2.
- **This phase is a recorded deviation from C# parity.** The C# keeps
  XML as the canonical store. The deviation is a user decision; the
  ADR records it.

## Facts recorded (the 2026-09-07 exploration)

### Coupling audit

**Format coupling is LOW — one seam.** The XML file format lives
behind `cr-core/src/database/comic_database.rs:243-343`
(`load`/`open_with_fallback`/`save`/`save_bytes`/`create_new`).
Five production files call that seam:

- `cr-engine/src/library.rs` (`Library::open`/`save`/`save_if_dirty`)
- `cr-engine/src/backup.rs`
- `cr-ui/src/library.rs` + `cr-ui/src/app.rs` (close-request save +
  the 600 s timer)
- `cr-cli/src/main.rs` (`db-dump`, `db-roundtrip`, `lists`)

Everything else operates on the in-memory `ComicDatabase` struct.
The model layer is format-blind.

**Semantic coupling is HIGH — the persistence design assumes
whole-file snapshots:**

- One dirty bool for the whole DB (~20 `mark_dirty` sites), not
  per-book.
- Save points: exit + the 600 s timer (`app.rs:85-93`). A save
  serializes the whole file and writes it twice (`<path>.bak` then
  copy, `comic_database.rs:262-276`).
- The `.restore` → main → `.bak` → quarantine corruption chain
  assumes a file artifact.
- `backup.rs` zips `ComicDb.xml` + `Thumbnails/*`
  (`BACKUP_DATABASE_NAME = "ComicDb.xml"`).
- The whole test corpus is XML-fixture-based (goldens, realworld
  tests). Those stay as codec gates; the new store adds its own.

### Size and cost baseline

- ~2.3 KB/book (255 books = 584 KB real fixture). 10k ≈ 23 MB,
  100k ≈ 230 MB.
- A save re-serializes everything + the `.bak` copy. At 100k books
  that is ~460 MB per 600 s sweep.
- Every page turn, book open, editor commit, and page-size cache
  event only sets the dirty bool between save points.
- All lookups are linear scans over `Vec<ComicBook>`.

### Out of scope here

- The Phase 8 T3/T4 perf pains (CBL import, fileless delete) are NOT
  XML-save costs. Phase 8 owns them.
- Lazy/streaming load for 100k+ books (the in-memory snapshot stays;
  record as future work).
- Any device-sync/remote use of the store (ADR-028 backlog).

## Work breakdown (the full-design estimate)

| Slice | Work | Est. |
|---|---|---|
| SQLite backend (cr-core) | Schema, ComicBook↔row (hot columns + lossless payload), list tree as payload rows, watch folders/blacklist, `schema_version` | ~1.5-2k LOC + tests |
| Store dispatch seam | `open`/`save` dispatch by store kind; the XML codec functions stay untouched | ~200-400 LOC |
| Dirty tracking redesign | `mark_dirty` sites → a dirty-id set, a debounced flusher, the exit flush | ~300-500 LOC touched |
| Migration + Export UX | Settings migrate flow, verify + archive, "Export for ComicRack…" via `save_bytes` | ~400-600 LOC |
| Backup/restore | `VACUUM INTO` snapshot in the zip, restore path | ~150-250 LOC |
| cr-cli + gates | Backend-aware commands, sqlite→XML byte-compare, WAL crash test, migration probe | ~200-400 LOC + test work |

Total: ~3-4k LOC product code + gate work ≈ 3-5 weeks solo.

### Risk centers + mitigations

1. **Lists/custom-values fidelity.** Mitigation: the payload-blob
   strategy. Each row carries the exact serialization the current
   writer emits, so losslessness is structural, not field-mapped.
2. **Dirty-semantics regressions.** A mutation that forgets to mark
   a book dirty loses updates. Mitigation: keep whole-db-dirty as
   the fallback default; only classify the hot per-book paths.
3. **Concurrency.** Mitigation: one writer connection owned by the
   session on the main loop, WAL mode, no pool, no async. The
   in-memory snapshot model stays.

## Prototype schema shape (the spike validates it)

- `books`: guid PK, file_path, hot columns (current_page,
  last_page_read, opened/open_count/opened_time, added_time,
  file_size/file_missing/modified/created, the dirty/read flags) +
  a lossless payload column (the `<Book>` element bytes the current
  writer emits — reuse the cr-core scalar codecs).
- `lists`: guid, parent, kind, payload blob (matcher tree, limits,
  `CacheStorage`, `<Display>` config).
- `watch_folders`, `blacklist`, `meta` (db id/name, schema_version).
- `schema_version` table for migration discipline.

## Task list

### T1. Spike: generator + measurements (read-only, no app code)

- Generator: synthetic ComicDb.xml at 10k/50k/100k books
  (realistic fill incl. page lists). A cr-cli subcommand or a test
  generator writing under `/tmp` — no full-size fixture committed.
- Measure XML: open (parse), save (serialize + `.bak`), memory.
- Prototype the SQLite schema (rusqlite, bundled). Measure: bulk
  insert (migration), full load, single-row page-turn UPDATE,
  path/guid lookup.
- Output: the numbers table in the ADR-029 draft.
- Gate: documented numbers; zero product code.

### T2. ADR-029 + user approval gate

- Decision record: SQLite canonical + XML import/export; Postgres
  rejected; fresh-install XML default; migration UX; the schema
  shape; the dirty-tracking redesign; the backup change.
- **NO code lands before user sign-off.**

### T3. SQLite store implementation (cr-core)

- The backend under the `ComicDatabase` load/save surface. cr-engine
  + cr-ui stay on the model.
- Schema per ADR; `schema_version` discipline.
- Gate: sqlite round-trip tests (save → load → identical model);
  the XML goldens stay green.

### T4. Incremental save (cr-engine/cr-ui)

- Dirty bool → dirty-id set; debounced single-row UPDATEs; exit
  flush; the 600 s sweep becomes a checkpoint pass.
- Gate: a page-turn → one-row-write evidence test; a kill-mid-write
  recovery test (WAL); the exit flush gate.

### T5. Migration + export UX (cr-ui)

- Settings ▸ "Migrate to SQLite…": write `library.sqlite`, verify
  (book count + spot checksum), archive `ComicDb.xml.imported`,
  switch store. Idempotent; a failure leaves the XML untouched.
- File ▸ "Export for ComicRack…": the byte-stable XML write.
- Gate: migration probe on `tests/realworld/ComicDb.xml` (255
  books); export byte-compare against the golden fixture.

### T6. Backup/restore swap (cr-engine)

- `backup.rs`: a `VACUUM INTO` snapshot + `Thumbnails/*` in the
  zip; the restore path updated.
- Gate: the backup tests reworked + green.

### T7. Tooling + docs

- cr-cli: backend-aware `db-dump`/`db-roundtrip`/`lists`; a
  sqlite → export XML → bytes golden test.
- README: data paths, migration, export.
- `risk-register.md` row 1 re-scope (the XML-fidelity risk narrows
  to the codec).

## Order

T1 → T2 (approval gate) → T3 → T4 → T5 → T6 → T7. T5 may run
parallel to T4 after T3. Starts after Phase 8 closes.

## Status

- Written 2026-09-07 (spun out of Phase 8 T7). T1 next.
