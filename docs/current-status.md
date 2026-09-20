# Current status

Update this file at the end of every work session. Replace stale content.
Do not append history. Git and `docs/archive/` hold history.

## Active phase

**Phase 21: Comic Vine cache expansion.**

In progress. The phase file is `docs/phases/phase-21.md`. The decisions
are ADR-069 through ADR-074.

Four parts: schema v3 with every comic resource and row stamps;
local-first scrape reads with a refresh switch and an offline mode; a
backupable, mergeable cache file; the sweep expansion plus Python
build and import scripts.

T1 through T7 are done. T8 (`scripts/cvcache/`) is done. Task C (the
update pipeline, ADR-075) is done: schema v7 `sync_state`, the
all-endpoint update in the app ("Update Comic Vine Cache" now walks
publishers, people, volumes, issues) and the standalone
`scripts/cvcache update` backfill command, and the localcv
`sync_state` seed. A live API probe (MEASURED, user key, 2026-09-20)
confirmed the `date_last_updated` filter narrows all four endpoints.
Schema grew past the phase's original v4: v5 added the `issue_image`
gallery (ADR-073), v6 added ComicTagger cover hashes (ADR-074), v7
added the per-endpoint sync watermark (ADR-075). The full `localcv.db`
is imported into the live cache (verified by the user). T9 (user
tests) stays open. Phase 20 stays implemented with open user test 26.

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

Phase 21 T9 (user tests). Task C (the update pipeline) is implemented
under ADR-075. Read the "Task C detail" section of
`docs/phases/phase-21.md` and ADR-075 for what shipped.

**What Task C shipped (do not redo):**
- Schema v7 `sync_state` (per-endpoint watermark), pinned in the Rust
  migration chain and `scripts/cvcache/schema.py`; the gated
  `cvcache_schema_pin` passes both directions at v7.
- The `sync_state` merge arm in both engines (Rust `import.rs`, Python
  `merge.py`): newer `last_sync` wins.
- The localcv adapter seeds `sync_state` from `cv_sync_metadata`.
- The in-app "Update Comic Vine Cache" command now walks all four
  endpoints (publishers, people, volumes, issues) through
  `cr-scrape` `cache::update::run`, stamping rows with the real API
  `date_last_updated`, resumable through `sync_state.resume_state`,
  gated by offline/refresh (ADR-071), on a worker thread.
- The standalone `python3 -m scripts.cvcache update --into <cache>
  --api-key <key>` command for a slow backfill; `--max-pages`,
  `--since`, `--delay`, `--endpoint`, and the publisher filter apply.

**MEASURED (user key, 2026-09-20):** a live API probe confirmed the
`date_last_updated` filter narrows all four endpoints (publishers 4,
people 126, volumes 144, issues 985 in 2026-08-01|2026-08-05, against
unfiltered 9,855 / 89,713 / 160,561 / 1,139,988; dates in-window).

**Not yet run (T9 user tests):** the app "Update Comic Vine Cache"
against a real cache with the request log watched (does it fill
`date_last_updated` and advance every `sync_state` watermark), and a
standalone backfill slice with `--max-pages`. The automatcher parity
test, a backup-import round trip, and a cached-series scrape stay open
from before.

**Validation done (MEASURED, user session 2026-09-20):** a live-API
check of 18 items (2-3 each of volume, issue, publisher, person,
character, team, story_arc, location) found ZERO wrong values in
cvcache. Every stored field matched the API. The only gaps were fields
localcv never had: `date_added`, `date_last_updated`, and
`api_detail_url` — all empty because localcv carried no stamps. The
18 sampled rows were refreshed from the API as a spot check; the other
~1.4M rows still have empty stamps until an update run fills them.
Coverage gap (API fields cvcache does not pre-populate, by design,
fetched on demand into `detail_json` per ADR-070): `aliases`, `deck`,
`description`, `real_name`, `gender`, `origin`, `powers`,
`birth`/`death`, `country`, publisher `location_*`, and the credit
rollups. Note: CV's live API has typo field names
(`count_of_isssue_appearances`, `isssues_disbanded_in`).

**Task C — the update pipeline: DONE (ADR-075).** The empty
`date_last_updated` across the ~1.4M rows fills on the next update run
(app or script). Schema v7 `sync_state` holds the per-endpoint
watermark, seeded from `cv_sync_metadata` during the localcv import.
The update runs both in the app ("Update Comic Vine Cache", all four
endpoints) and as `scripts/cvcache update`.

**Also noted (separate follow-up, not Task C):** the automatcher can
read `issue_image.ahash` to skip a cover download on a cache hit
(deferred from ADR-074). And T9's config-reference rows for
`MATCH_THRESHOLD`/`MATCH_SIMILARITY_MARGIN` are already written; T9's
remaining part is user-test procedures.

The one-off localcv import is available now:
`python3 -m scripts.cvcache import-localcv --into <cvcache> --source
<localcv.db>`. See `scripts/cvcache/README.md`. Always run it on a copy
of the live cache first. The publisher filter flags are optional and
reserved for the Task C probe workflow.

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
8. Automatcher cover-hash cache read: on a match, read
   `issue_image.ahash` instead of downloading the CV cover (skip the
   fetch). Deferred from ADR-074; the hashes are stored, the matcher
   still recomputes.

## Open risk

Apache-2.0 is incompatible with GPL-2.0-only. The Apache-2.0 scraper port,
`ring`, and `webpki-roots` remain unresolved under ADR-041. The project does
not claim that the present combination is permissible. `cargo deny check
licenses` is not a CI gate.

## Latest verification

Phase 21 Task C / schema v7 (ADR-075, the per-endpoint update
watermark and the all-endpoint update) passed on 2026-09-20.

- `cargo fmt --all`, `cargo clippy --workspace --all-targets --
  -D warnings`, `cargo test --workspace`: passed.
- `CR_FORMAT_TESTS=1 cargo test -p cr-scrape --test
  cvcache_schema_pin`: passed (app and scripts agree on the v7 DDL,
  both directions).
- `python3 -m unittest discover -s scripts/cvcache/tests`: 22 ok
  (adds the `sync_state` merge arm, the localcv seed, and the update
  stagers).
- `MEASURED` (user key, live API, 2026-09-20): the
  `date_last_updated` filter narrows all four update endpoints —
  publishers 4, people 126, volumes 144, issues 985 changed in
  2026-08-01|2026-08-05, against unfiltered 9,855 / 89,713 /
  160,561 / 1,139,988; every returned date fell in-window.
- `UNKNOWN`: the app "Update Comic Vine Cache" against a real cache is
  not yet user-run (does it fill `date_last_updated` and advance every
  `sync_state` watermark). A build does not prove it.

## Environment notes

- Run UI probes in release mode under Xvfb with `GDK_BACKEND=x11`,
  `DISPLAY=:99`, and isolated XDG paths under `/tmp/opencode/`.
- Never point a probe at the real library.
