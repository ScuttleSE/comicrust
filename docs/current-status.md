# Current status

Update this file at the end of every work session. Replace stale content.
Do not append history. Git and `docs/archive/` hold history.

## Active phase

**Phase 21: Comic Vine cache expansion.**

In progress. The phase file is `docs/phases/phase-21.md`. The decisions
are ADR-069 through ADR-072.

Four parts: schema v3 with every comic resource and row stamps;
local-first scrape reads with a refresh switch and an offline mode; a
backupable, mergeable cache file; the sweep expansion plus Python
build and import scripts.

T1 (schema v3) and T2 (inline credits and resource upserts) are done.
Commits `57d4302` and `f6cd8a8`. Phase 20 stays implemented with open
user test 26.

## Latest user finding

`MEASURED` (user, 2026-09-19): The new trace identified the CPU and UI stall.
The Incoming gap worker parsed 88,129 paths while it held the global proposed-
value cache mutex for 7.25 seconds. GTK operations waited on that mutex for up
to 6.62 seconds. The 100,000-entry cache cleared repeatedly while the active
Library and Incoming catalogs contained up to 105,825 paths. Cached Comic Vine
series propagation itself changed three books in approximately 7 ms.

`CODE-READ`: the fix removes the fixed cache limit, reserves for all active
paths at startup, and parses through per-path `OnceLock` values outside the
global map lock. String matchers request proposed values only for Series,
Title, and Format. Gauge evaluation runs on a worker. A new generation cancels
obsolete Incoming gap work.

`MEASURED` (user, 2026-09-19): The same real-library workflow completed in one
to two seconds after the fix. The user reports that it is much better. The
scrape matched the selected book and three other books in the same series.
`UNKNOWN`: peak memory use was not measured.

## Current task for the next context

Phase 21 T3: local-first reads (ADR-071). Route search, series
details, issue detail, and images through the cache first in
`cv/queries.rs` and `engine.rs`. Acceptance: a mock-server test
scrapes a fully cached series with ZERO API requests. Follow
`docs/phases/phase-21.md`.

When the user tests first: the `2000 AD` number `2498` retest (test 24)
and the Missing Issues scope test (test 22) stay first in line, and
user test 26 (the phase-20 cache manager against a real volume) is
still open.

## Open user tests

The procedures are in `docs/open-user-tests.md`.

- Test 1: Library-tree gauge badges failed. No orange Unread badge appeared,
  and red New equaled green Total.
- Test 4: Select Worst Duplicates is passable. `UNKNOWN`: the requested
  improvement is not specified.
- Test 9: Library Organizer simulation showed no report.
- Test 17: Incoming folder setup and review views have a partial pass.
- Test 22: Missing Issues gap report is not complete.
- Test 23: Link Series from Cache is not complete.
- Test 24: Find Missing Issues in Incoming has a bounded-match fix. The real
  `2000 AD` number `2498` retest is pending.
- Test 25: Cached series metadata propagation is not complete. The Proposed
  Number regression now passes on the real library.
- Test 26: Comic Vine cache manager is not complete. Test both API modes
  against a real volume.

## Other open work

1. `CODE-READ`: `ShowOnlyDuplicates` writes to ComicDb.xml but does not restore
   per-list UI state.
2. `MEASURED`: `duplicates_probe` gate F reads settings before the Preferences
   worker commits. Fix the probe wait. Do not change the expected value.
3. `UNKNOWN`: Confirm Incoming discard speed, clean close, and scroll restore
   on the real CIFS library. See ADR-058.
4. The `.deb` package has no local content test because this machine has no
   `dpkg-deb`.
5. AppStream metadata has no hosted screenshot URLs.
6. Phase 16, Comic Vine scraper quality of life, remains planned.
7. Phase 9, the library SQLite backend, remains deferred.

## Open risk

Apache-2.0 is incompatible with GPL-2.0-only. The Apache-2.0 scraper port,
`ring`, and `webpki-roots` remain unresolved under ADR-041. The project does
not claim that the present combination is permissible. `cargo deny check
licenses` is not a CI gate.

## Latest verification

Phase 21 T1 and T2 passed on 2026-09-20.

- `cargo fmt --all`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed (59 suites ok).
- `MEASURED`: the v2→v3 migration keeps the stored JSON byte-identical
  and backfills the typed columns; the v1→v2→v3 chain passes; malformed
  detail JSON keeps NULL columns and still migrates.
- `MEASURED`: a mock-server complete update stores every credit marker
  (`credit`, `first_appearance`, `died_in`, `disbanded`) and an
  identical re-import stores the same rows.
- `MEASURED`, one gate flake: `cr-engine --test incoming_transaction`
  failed once inside a full workspace run and passed in two later runs
  (alone and in the full workspace) on identical code. No cr-engine
  code changed. The failing test name was not captured. Untreated.
- `UNKNOWN`: the real API JSON shape of the `volume` `aliases` field
  (string vs array). The v3 column stores it verbatim, which is
  lossless either way. A real response settles it.
- `MEASURED` (docs page, 2026-09-20): the issue resource documents
  `characters_died_in`, and BOTH `teams_disbanded_in` and
  `disbanded_teams` with the same meaning. The extractor reads both
  disbanded keys and the first-appearance keys.

## Environment notes

- Run UI probes in release mode under Xvfb with `GDK_BACKEND=x11`,
  `DISPLAY=:99`, and isolated XDG paths under `/tmp/opencode/`.
- Never point a probe at the real library.
