# Current status

Update this file at the end of every work session. Replace stale content.
Do not append history. Git and `docs/archive/` hold history.

## Active phase

**Phase 21: Comic Vine cache expansion.**

In progress. The phase file is `docs/phases/phase-21.md`. The decisions
are ADR-069 through ADR-073.

Four parts: schema v3 with every comic resource and row stamps;
local-first scrape reads with a refresh switch and an offline mode; a
backupable, mergeable cache file; the sweep expansion plus Python
build and import scripts.

T1 through T7 are done. T8 (`scripts/cvcache/`) is done for the merge
engine, `build`/`merge`, the localcv import adapter (Task A), and the
publisher filter (Task B); the gated `cvcache_schema_pin` cargo test
passes both directions. Task C (an update pipeline that maintains
`cvcache.sqlite` and an in-app incremental refresh) and T9 (docs and
user tests) stay open. Phase 20 stays implemented with open user test
26.

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

Phase 21 Task C and T9. Task C (ADR-072 follow-up): rebuild the update
half of `sqlite_cv_pipeline_1.1.0.py` (reference only) to maintain
`cvcache.sqlite` — an update-only run that fetches
`/issues?filter=date_last_updated:...` since the last sync, stamps rows
with the real API date, and merges through the same engine; the same
update must also live in the app, wired to the "Update Comic Vine
Cache" command and gated by the T4 switches. Design under a new ADR.
T9: `docs/config-reference.md` rows for the two new keys, and the
user-test procedures. Read the "T8 detail" section of
`docs/phases/phase-21.md` for the recorded answers and the data gap.

The one-off localcv import is available now:
`python3 -m scripts.cvcache import-localcv --into <cvcache> --source
<localcv.db> [--whitelist FILE] [--blacklist FILE]`. See
`scripts/cvcache/README.md`. Always run it on a copy of the live cache
first.

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

Phase 21 T8 + schema v5 (ADR-073) passed on 2026-09-20.

- `cargo fmt --all`, `cargo clippy --workspace --all-targets --
  -D warnings`, `cargo test --workspace` (61 suites ok): passed.
- `CR_FORMAT_TESTS=1 cargo test -p cr-scrape --test
  cvcache_schema_pin`: passed (app and scripts agree on the v5 DDL,
  both directions).
- `python3 -m unittest discover -s scripts/cvcache/tests`: 15 ok.
- `MEASURED`: a full localcv import on a copy of the live cache filled
  the v5 `issue_image` table (156,084 gallery rows; one multi-image
  issue carried 80), reported the 28 numberless issue ids, and left
  `PRAGMA integrity_check = ok` at `user_version` 5.
- `UNKNOWN`: the exact inline sub-fields of the `volume` object in
  `/issues` responses. The reader takes `name` and a `publisher`
  sub-object when present; a real response settles the rest.
- Still open from T6: the release-probe acceptance (a probe drives
  all three dialog operations).

## Environment notes

- Run UI probes in release mode under Xvfb with `GDK_BACKEND=x11`,
  `DISPLAY=:99`, and isolated XDG paths under `/tmp/opencode/`.
- Never point a probe at the real library.
