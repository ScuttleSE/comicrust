# Phase 2 Kickoff — Engine: Smart Lists, Queues, Scanner, Backup

Goal: comicrust evaluates ComicRack smart lists identically, runs the background queue machinery, rescans libraries unattended, and manages database backups. Exit gate: **smart lists from the migrated real-world library evaluate identically to the C# matchers (fixture tests); the ProcessingQueue/QueueManager port runs page/thumbnail work through the five C# queues; the scanner and backup manager run unattended** — all headless, zero UI.

Phase 1 remains the reference for doc style: see `phase-1-kickoff.md`. The Phase 0 gate (byte-stable ComicDb.xml round-trip) and the Phase 1 acceptance state must stay green; do not regress them.

## Status (2026-09-02)

Not started. The entry points that already exist: the property registry (`cr-core/registry.rs`), the list tree with raw matcher storage (`cr-core/database/list_items.rs` — matchers are captured, not parsed), `ComicBook`/`ComicInfo` models, and the Phase 1 `cr-image` cache/key layer that the queues will drive.

## Scope from the roadmap

Port plan Phase 2 "Engine": query tokenizer + 76 matchers + comparers/groupers, smart-list persistence, QueueManager (5 queues), scanner, watch folders, backup manager. Estimated 8-10 weeks solo.

## The one non-negotiable invariant

**Smart-list query language must parse and match identically** (compat invariant #4 in `AGENTS.md`). Saved smart lists in ComicDb.xml store the query as a `Match` string (`ComicSmartListItem` → `ComicBookGroupMatcher.ConvertParametersToQuery`). The grammar, the property names, the operators, and the evaluation semantics are the spec — a real ComicDb.xml fixture decides any dispute, like Phase 0.

## Task list

### T1. Query language (`cr-engine`)

The C# spec lives in `ComicRack.Engine/ComicBookGroupMatcher.cs` (259 LOC — `ConvertParametersToQuery` and the parse side) and `cYo.Common/Text/Tokenizer.cs`.

- [ ] Tokenizer port: `Tokenizer.cs` semantics (quoting, escapes, separators)
- [ ] Grammar: matcher name → parameters (`property`, `operator`, `value`), group nesting (AND/OR via `ComicBookGroupMatcher`), the `ComicSmartListItem.MatcherMode` interplay
- [ ] Parse a `Match` string into a matcher tree; render a matcher tree back to a `Match` string — **round-trip must be byte-stable on the real-world database's saved lists** (they live in `tests/realworld/ComicDb.xml`, read the fixture README before touching anything)
- [ ] Document the grammar in the module header (the C# has no formal grammar; derive it from `ConvertParametersToQuery` + `Parse`)

### T2. Matcher framework + the 76 matchers (`cr-engine`)

The C# spec lives in `ComicRack.Engine/ComicBookMatcher.cs` (base class: `Not` flag, `TRMatcher` names, property lists) and `ComicRack.Engine/Metadata/ComicBook/Matcher/` (76 files). Series stats live in `ComicBookSeriesStatistics.cs` (property prefix `Stats`).

- [ ] Base port: `ComicBookMatcher` — `Not`, match dispatch, the `ComicProperties`/`SeriesStatsProperties` name lists fed by the `cr-core` property registry (Phase 0 built it for exactly this)
- [ ] Typed comparison families: string (wildcards), numeric (`Number`/`Count` style range ops), date (`ComicBookDateMatcher`), bool/`YesNo`/`MangaYesNo`, enum, guid
- [ ] The 76 concrete matchers, generated where the C# is mechanical (most are one comparison over one registry property); keep the C# class names as Rust module/type names for traceability
- [ ] `ComicBookGroupMatcher` (AND/OR trees) and the `ComicBookAllPropertiesMatcher` wildcard
- [ ] Series-statistics matchers (`SmartListSeries*` in `Database/`): need the series-statistics engine (`ComicBookSeriesStatistics.cs`) — group books by series and compute the aggregate values; the `Stats` property prefix routes into it
- [ ] Matcher evaluation fixtures: for each matcher family, a table-driven test over synthetic `ComicBook`s

### T3. Smart-list persistence + evaluation (`cr-engine`)

The C# spec lives in `ComicRack.Engine/Database/ComicSmartListItem.cs` (450 LOC) and `ComicLibrary.cs` (`InitializeDefaultLists`, line 254).

- [ ] `ComicSmartListItem` evaluation: filter a book set through the matcher tree; `LimitType`/`LimitSelectionType` (order + count caps)
- [ ] Wire evaluation into the loaded `ComicDb.xml` list tree (`cr-core/database/list_items.rs` currently stores matchers raw; parsing lands here and matches T1's grammar)
- [ ] **Default lists**: port `ComicLibrary.InitializeDefaultLists` into `cr-core/database/comic_database.rs::create_new()` (this closes the Phase 0 tail item). The real-world fixture shows the exact default list set (My Favorites, Recently Added, Recently Read, Never Read, Reading, Read, Files to update, Temporary Lists); localized names use the English defaults first
- [ ] Acceptance fixture: evaluate every saved smart list in `tests/realworld/ComicDb.xml` against its own books; results must match the C# semantics (hand-verified against a real ComicRack run, like the Phase 0 golden flow)

### T4. ProcessingQueue + QueueManager (`cr-engine`)

The C# spec lives in `cYo.Common/Threading/ProcessingQueue.cs` (460 LOC) and `ComicRack.Engine/QueueManager.cs` (712 LOC).

- [ ] `ProcessingQueue` port: named worker threads (configurable count), `AddItem` with `ProcessingQueueAddMode` (AddToTop etc.), bounded capacity, `IsActive`/`PendingItems`, idle callbacks
- [ ] `QueueManager` port: the five background queues (fast/slow page, fast/slow thumbnail, slow-thumbnail-unlimited — see `ImagePool.cs:111` for the exact construction), thread priorities, and the idle/`IsWorking` semantics
- [ ] Wire the Phase 1 `cr-image` pools/keys in: queue items are `ImageKey`s; the processing functions render pages/thumbnails exactly like `ImagePool`'s workers (decode → adjust → cache; the C# `OnCreateImage` chain)
- [ ] Crossbeam-channel + scoped worker threads per the technology mapping (no thread aborts — the C# `ThreadUtility.Abort` paths become cooperative shutdown)
- [ ] Concurrency tests: ordering (AddToTop), capacity drops, idle detection, graceful shutdown

### T5. Scanner + watch folders (`cr-engine`)

The C# spec lives in `ComicRack.Engine/ComicScanner.cs` (237 LOC), `Database/WatchFolder.cs`/`WatchFolderCollection.cs`, and the file-info chain in `ComicBook.cs` (the `FileInfoRetrieved`/`CopyFromFile` region).

- [ ] Scanner port: walk the library folders, map files → `ComicBook`s (provider lookup by extension, Phase 1 `cr-io`), add/remove/keep decisions, file-info update (size/times/page list/`CreateHashFromImageList` hash) into the DB
- [ ] New-book default values (the C# `ComicBook` defaults when a file first appears) — including `ComicNameInfo` filename parsing (already ported in `cr-core`)
- [ ] Watch folders: inotify via the `notify` crate mapping `WatchFolder` settings (watch/recursion/blacklist per `WatchFolderCollection`); debounce into scanner runs
- [ ] Unattended run test: build a synthetic library on disk (Phase 1 fixture helpers), scan it, mutate the disk (add/rename/delete), rescan, assert the DB diff

### T6. Backup manager (`cr-engine`)

The C# spec lives in `ComicRack.Engine/DatabaseManager.cs` (283 LOC) — the `.bak` rotation and corrupt-file quarantine are already ported in Phase 0 (`cr-core/database/comic_database.rs::open_with_fallback`).

- [ ] The remaining `DatabaseManager` pieces: the `.crplugin`/zip backup creation and restore flow (Phase 1's `zip` crate covers the container), backup file naming/rotation, `Corrupt Database Backup [date].xml` handling parity
- [ ] Tests: round-trip a DB through backup create → destroy main → restore

### T7. Comparers/groupers (`cr-engine`)

The C# spec lives in `ComicRack.Engine/Metadata/` (`IBookGrouper.cs`, `Metadata/ComicBook/*Sorter*`/grouper surfaces) and `cYo.Common/ComponentModel/IGrouper.cs`.

- [ ] Sort comparers for every browser column key (registry-driven, like the C# reflection property names)
- [ ] Groupers: the grouping keys the browser uses (series, folder, year, format...) — the *values* feed Phase 4's ItemView; keep the port registry-driven and table-tested
- [ ] This task can start last and slip into early Phase 4 without hurting the exit gate (the browser is the consumer)

## Test data policy

- Never commit user library data (invariant in `AGENTS.md`). `tests/realworld/ComicDb.xml` is the user-approved exception — read its README; byte identity is the test, do not edit or reformat it.
- Matcher/query fixtures: synthetic `ComicBook`s in test code (table-driven), plus the real-world DB's saved `Match` strings for grammar round-trips.
- Scanner/watch-folder tests build synthetic libraries in temp dirs from the Phase 1 fixture helpers (generated PNGs + `zip`/`tar`).
- Queue tests use real threads with short timeouts; no sleeps-in-asserts.

## Acceptance criteria (phase exit)

1. `cargo test` green; Phase 0 golden round-trips and Phase 1 acceptance state unchanged (regression guard).
2. Every saved smart list in `tests/realworld/ComicDb.xml` parses from its `Match` string, re-renders byte-identically, and evaluates to the C#-verified book set.
3. The five-queue machinery processes page/thumbnail requests with C# ordering semantics (AddToTop, priorities) and shuts down cleanly.
4. The scanner builds a library from a synthetic folder tree unattended (add/update/remove parity), and watch folders trigger rescans.
5. Backup create → destroy → restore round-trips.
6. CI green; ADRs updated with anything learned that contradicts planning.

## Explicitly deferred (not Phase 2)

- The browser/list UI consumption of sorters/groupers (Phase 4).
- Remote/sync statistics and the net.tcp replacement (Phase 7).
- Scripting hooks that fire on library events (Phase 6).

## Dependencies and sequencing notes

1. T1 + T2 are the core: everything in T3 depends on them; start there.
2. T3's default-lists port closes the last Phase 0 tail item besides the settings port.
3. The settings port (`IniFile`/`EngineConfiguration`/`SystemPaths` — Phase 0 tail) is NOT a blocker for T1-T5 (C# defaults are hard-coded in the Phase 1/2 ports with comments); wire it when it lands.
4. T4 needs the cr-image pool/cache layer (done in Phase 1); the queue port should land before T5 so the scanner can reuse the same worker machinery.
5. T7 is isolated and may slip into Phase 4.
