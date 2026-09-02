# Phase 0 Kickoff — Core Model & Data Compatibility

Goal: prove the hardest compat surface (the library database) with zero UI. Exit gate: **round-trip a real `ComicDb.xml` byte-stably** and provide working `cr-cli` verification tools.

## Task list

Progress notes (2026-09-02): T0-T4 are built and green. Items marked [x] are done. Deferred items keep `[ ]` with a reason.

### T0. Workspace scaffold
- [x] Cargo workspace with crates: `cr-core`, `cr-io`, `cr-image`, `cr-engine`, `cr-script`, `cr-ui`, `cr-cli`, `cr-app` (empty stubs — only `cr-core` + `cr-cli` are worked this phase)
- [x] CI: fmt check, clippy `-D warnings`, test job (Gitea Actions, `.gitea/workflows/ci.yaml`; the checks activate on their own once `Cargo.toml` landed)
- [x] `rust-toolchain.toml`, workspace lints, deny.toml (license/advisory inventory — feeds ADR-009)

### T1. Data model (`cr-core`)
- [x] Map `ComicInfo` (~40 fields, `ComicRack.Engine/ComicInfo.cs`) field-by-field, with exact `[DefaultValue]` suppression (element `model/comic_info.rs`)
- [x] Map `ComicBook` (+~30 state fields, `ComicBook.cs`) — Guid id, timestamps (all three kind suffixes), file info, book* fields, `ValuesStore` custom values codec, sync info, `BitmapAdjustment` color adjustment (element `model/comic_book.rs`, `model/bitmap_adjustment.rs`)
- [x] Map `ComicPageInfo` + the `<Pages>` collection, with the `short`-truncating setters and the `Image`/`Type` renames (element `model/comic_page_info.rs`)
- [x] `MetronInfo` (generated schema, 1,789 LOC) — done in Phase 1 (`cr-core/model/metron_info.rs`, byte-stable round-trip tested).
- [x] **Property registry**: string-name → typed getter/setter for ComicBook properties (element `registry.rs`; foundation for matchers/columns/remote — see risk #7)
- [x] `ComicNameInfo.FromFilePath` regex port + unit tests (element `model/comic_name_info.rs`; NewParser + LegacyParser, RightToLeft emulated)

### T2. Database layer (`cr-core`)
- [x] `ComicDatabase`/`ComicLibrary`/`ComicBookContainer` hierarchy + ComicLists tree (folders, smart lists, their config) — element `database/list_items.rs`, `database/display_config.rs`, `database/comic_database.rs`
- [x] Load: plain XML, save: `.bak` → copy-over-main rotation, corrupt-file quarantine ("Corrupt Database Backup [date].xml") + fresh-DB fallback, `.restore` handling — element `database/comic_database.rs` (`open_with_fallback`, `OpenStatus`)
- [ ] Optional BZip2-compressed variant (SharpZipLib compat) — deferred; only the in-memory `ToByteArray/FromByteArray` path uses it. Low priority.
- [ ] Settings: `ComicRack.ini` / `IniFile` port, `EngineConfiguration`, portable-mode paths (`SystemPaths.cs`) — not started. This is the largest remaining Phase 0 item.

### T3. Golden-file test harness
- [x] 3 fixtures under `tests/golden/`: `db-small.xml` (hand-written), `db-large.xml` (code snapshot, full surface), `db-net-reference.xml` (captured .NET output, corrected against source) — **no real user data committed**
- [x] Byte-stable round-trip test: load → save → byte-compare (`crates/cr-core/tests/golden_roundtrip.rs`), byte-identical on all three fixtures
- [x] Schema-snapshot test: element/attribute inventory check on the large fixture (light form: presence assertions; a full inventory diff can replace it later)
- [x] Negative tests: truncated file, garbage bytes, empty file, wrong root. Zip-of-xml (backup restore path) waits for zip support in `cr-io`.

### T4. `cr-cli` verification tools
- [x] `cr-cli info <comic-file>` — prints parsed ComicBook (proposed-from-filename + metadata) as JSON. ComicInfo.xml input works; archive metadata waits for `cr-io`.
- [x] `cr-cli db-dump <ComicDb.xml>` — validate + JSON summary (book count, list tree, custom values)
- [x] `cr-cli db-roundtrip <ComicDb.xml>` — load/save, byte-diff report (exit 0 identical, exit 1 different, exit 2 error)

## Acceptance criteria (phase exit)

Status as of 2026-09-02 (after real-world validation):

1. [x] `cargo test` green, including golden round-trip on all fixtures (31 tests green as of this date)
2. [x] `cr-cli db-dump` on a real-world database produces a sane summary — validated on the user-supplied `tests/realworld/ComicDb.xml` (255 books; committed with user permission)
3. [x] `cr-cli db-roundtrip` byte-identical on fixtures — including the real-world database, byte for byte; findings in `tests/realworld/README.md` and `tests/golden/README.md`
4. [ ] CI green, ADRs updated with anything learned about the XML format that contradicts planning — CI runs on push (local runs green); ADR-011 records the XML-layer reality; the exit review closes this item

## Explicitly deferred (not Phase 0)

Archive reading (T1 of Phase 1), smart-list *evaluation* (model/serialization only), all UI, scripting.