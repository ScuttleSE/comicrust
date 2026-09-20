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

T1 through T7 are done. T8 (`scripts/cvcache/`) is done: the merge
engine, `build`/`merge`, the localcv import adapter (Task A), and the
publisher filter (Task B); the gated `cvcache_schema_pin` test passes
both directions. Schema grew past the phase's original v4: v5 added the
`issue_image` gallery (ADR-073), v6 added ComicTagger cover hashes and
switched the app to one hash algorithm (ADR-074). The full `localcv.db`
is imported into the live cache at v6 (verified by the user). Task C
(an update pipeline that maintains `cvcache.sqlite` plus an in-app
incremental refresh) and T9 (user tests) stay open. Phase 20 stays
implemented with open user test 26.

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

Phase 21 Task C (the update pipeline) and T9 (user tests). Read the
"T8 detail" and "Task C detail" sections of `docs/phases/phase-21.md`
first; ADR-069 through ADR-074 carry the decisions.

**What is done (do not redo):**
- The full `localcv.db` is imported into the live cache at
  `user_version` 6. Galleries, credits, metadata, and ComicTagger
  cover hashes (`issue_image.ahash`/`phash`, ~154k of 156k rows) are
  all present. `PRAGMA integrity_check = ok`.
- The app and the scripts share one hash algorithm (ComicTagger,
  ADR-074). `MATCH_THRESHOLD` and `MATCH_SIMILARITY_MARGIN` are
  config-tunable.

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

**Task C — the update pipeline (needs a new ADR).** Rebuild the update
half of `sqlite_cv_pipeline_1.1.0.py` (reference only) to maintain
`cvcache.sqlite`, not `localcv.db`:
1. Add a schema **v7 `sync_state`** table (per-endpoint last-sync
   watermark), modeled on localcv's `cv_sync_metadata`
   (`endpoint, last_sync, resume_state`). cvcache has NO equivalent
   today: `sweep_state` is a single-row issue-paging cursor, not a
   per-endpoint change watermark. Seed `sync_state` from
   `cv_sync_metadata` during the localcv import so the first update
   knows the 2026-08-03 baseline.
2. An update-only run fetches `/issues?filter=date_last_updated:<since>
   |<now>` (and the same for other endpoints), stamps rows with the
   real API date, and merges through the existing engine. This is what
   fills the empty `date_last_updated` across the library.
3. The same update must live in the app, wired to the "Update Comic
   Vine Cache" command (`crates/cr-ui/src/browser/shell.rs:5143`) and
   gated by the T4 offline/refresh switches.
4. The publisher whitelist/blacklist (`scripts/cvcache/publishers.py`,
   Task B) applies here at the volume level — this is its intended use.

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

Phase 21 schema v6 (ADR-074, ComicTagger cover hashes) passed on
2026-09-20.

- `cargo fmt --all`, `cargo clippy --workspace --all-targets --
  -D warnings`, `cargo test --workspace` (62 suites ok): passed.
- `CR_FORMAT_TESTS=1 cargo test -p cr-scrape --test
  cvcache_schema_pin`: passed (app and scripts agree on the v6 DDL).
- `python3 -m unittest discover -s scripts/cvcache/tests`: 15 ok.
- `MEASURED`: the ComicTagger hash port reproduces the reference
  `localcv.db` hashes to Hamming distance 0 (ahash) / <=2 (phash,
  decoder tolerance) on two real covers — golden test in
  `cr-image/tests/comictagger_hash.rs`.
- `MEASURED`: a full localcv import on a copy of the live cache filled
  `issue_image.ahash`/`phash` on 154,395 of 156,084 gallery rows;
  image 7's imported ahash equals localcv's stored value;
  `PRAGMA integrity_check = ok` at `user_version` 6.
- `UNKNOWN`: whether the automatcher still auto-matches correctly with
  the ComicTagger hash. Needs a user test (parity is not proven by a
  build). The automatcher reading the cached hash to skip a cover
  download is a noted follow-up.

## Environment notes

- Run UI probes in release mode under Xvfb with `GDK_BACKEND=x11`,
  `DISPLAY=:99`, and isolated XDG paths under `/tmp/opencode/`.
- Never point a probe at the real library.
