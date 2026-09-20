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

T1 (schema v3) through T7 (the sweep expansion, schema v4) are done.
Phase 20 stays implemented with open user test 26. The user declined
the offered real-cache user tests (2026-09-20); the real v2-file
migration check remains unperformed.

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

Phase 21 T8: the Python scripts (ADR-072). `scripts/cvcache/` with the
merge engine, build, merge, one adapter shape, and the schema pin
test; the gated CI hook. Acceptance: a script-built file opens and
merges in the app; an app-written file opens in a script. Follow
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

Phase 21 T7 passed on 2026-09-20.

- `cargo fmt --all`: passed.
- `cargo clippy --workspace --all-targets -- -D warnings`: passed.
- `cargo test --workspace`: passed (60 suites ok).
- `MEASURED`: the widened sweep page fills every list-level skeleton
  column with ONE request for the page, records the inline volume
  name, and upserts the inline publisher as a resource row.
- `MEASURED`: the v1→v2→v3→v4 chain passes; a fresh open lands on
  `user_version` 4; the v4 columns read back cleanly over pre-v4
  rows (`fetched_at` is NOT NULL with a 0 default).
- `UNKNOWN`: the exact inline sub-fields of the `volume` object in
  `/issues` responses. The reader takes `name` and a `publisher`
  sub-object when present; a real response settles the rest.
- Still open from T6: the release-probe acceptance (a probe drives
  all three dialog operations).

## Environment notes

- Run UI probes in release mode under Xvfb with `GDK_BACKEND=x11`,
  `DISPLAY=:99`, and isolated XDG paths under `/tmp/opencode/`.
- Never point a probe at the real library.
