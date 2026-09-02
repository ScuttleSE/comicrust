# Phase 0 Kickoff — Core Model & Data Compatibility

Goal: prove the hardest compat surface (the library database) with zero UI. Exit gate: **round-trip a real `ComicDb.xml` byte-stably** and provide working `cr-cli` verification tools.

## Task list

### T0. Workspace scaffold
- [ ] Cargo workspace with crates: `cr-core`, `cr-io`, `cr-image`, `cr-engine`, `cr-script`, `cr-ui`, `cr-cli`, `cr-app` (empty stubs; only `cr-core` + `cr-cli` are worked this phase)
- [ ] CI: fmt check, clippy `-D warnings`, test job (GitHub Actions or Hemmalab equivalent)
- [ ] `rust-toolchain.toml`, workspace lints, deny.toml (license/advisory inventory — feeds ADR-009)

### T1. Data model (`cr-core`)
- [ ] Map `ComicInfo` (~40 fields, `ComicRack.Engine/ComicInfo.cs`) field-by-field to serde structs; note XML element/attribute choice per field
- [ ] Map `ComicBook` (+~60 state fields, `ComicBook.cs`) — Guid id, timestamps, file info, book* fields, `ValuesStore` custom values, sync info, `BitmapAdjustment` color adjustment
- [ ] Map `ComicPageInfo` + `ComicPageInfoCollection`, `MetronInfo` (generated schema, 1,789 LOC)
- [ ] **Property registry**: string-name → typed getter/setter for every ComicBook property (foundation for matchers/columns/remote — see risk #7)
- [ ] `ComicNameInfo.FromFilePath` regex port + unit tests

### T2. Database layer (`cr-core`)
- [ ] `ComicDatabase`/`ComicLibrary`/`ComicBookContainer` hierarchy + ComicLists tree (folders, smart lists, their config)
- [ ] Load: plain XML; save: `.bak` → copy-over-main rotation; corrupt-file quarantine ("Corrupt Database Backup [date].xml") + fresh-DB fallback; `.restore` handling
- [ ] Optional BZip2-compressed variant (SharpZipLib compat)
- [ ] Settings: `ComicRack.ini` / `IniFile` port, `EngineConfiguration`, portable-mode paths (`SystemPaths.cs`)

### T3. Golden-file test harness
- [ ] Synthesize/anonymize 3+ fixture libraries (small/medium/large, with smart lists, custom values, missing files) under `tests/golden/` — **never commit real user data**
- [ ] Byte-stable round-trip test: load → save → byte-compare (modulo documented, justified diffs)
- [ ] Schema-snapshot test: dumped element/attribute inventory diffed against C#-written reference output
- [ ] Negative tests: truncated file, garbage bytes, zip-of-xml (backup restore path)

### T4. `cr-cli` verification tools
- [ ] `cr-cli info <comic-file>` — print parsed ComicBook (proposed-from-filename + metadata) as JSON
- [ ] `cr-cli db-dump <ComicDb.xml>` — validate + pretty-print database summary (book count, lists, custom values)
- [ ] `cr-cli db-roundtrip <ComicDb.xml>` — load/save in place to temp, byte-diff report

## Acceptance criteria (phase exit)

1. `cargo test` green, including golden round-trip on all fixtures
2. `cr-cli db-dump` on a real-world database (user-provided, not committed) produces a sane summary
3. `cr-cli db-roundtrip` byte-identical on fixtures; any diffs are enumerated and justified in a `tests/golden/README.md`
4. CI green; ADRs updated with anything learned about the XML format that contradicts planning

## Explicitly deferred (not Phase 0)

Archive reading (T1 of Phase 1), smart-list *evaluation* (model/serialization only), all UI, scripting.
