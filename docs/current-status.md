# Current status

Update this file at the end of every work session. Replace stale content.
Do not append history. Git and `docs/archive/` hold history.

## Active phase

**Phase 21: Comic Vine cache expansion.**

In progress. The phase file is `docs/phases/phase-21.md`. The decisions
are ADR-069 through ADR-075.

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
tests) is largely verified; the automatcher parity test, a backup-import
round trip, and a cached-series scrape remain. Phase 20 is implemented
and its cache-manager user test (26) passed.

## Latest user finding

`PASS` (user, 2026-09-20): a batch of Phase 19-21 user tests passed —
Missing Issues gap report and its scoped-series regression (test 22),
Link Series from Cache (23), Incoming setup and review (17), Find
Missing Issues in Incoming including the real `2000 AD` `2498` retest
(24), the Comic Vine cache manager in both API modes (26), and the new
"Update Comic Vine Cache…" pre-flight, capped run, and run-to-completion
(27). No library book changed in any case.

`MEASURED` (user key, live API, 2026-09-20): the `date_last_updated`
filter narrows all four update endpoints (publishers 4, people 126,
volumes 144, issues 985 in 2026-08-01|2026-08-05, against unfiltered
9,855 / 89,713 / 160,561 / 1,139,988; dates in-window). This is the
evidence the all-endpoint update rests on.

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
- The in-app "Update Comic Vine Cache…" command now walks all four
  endpoints (publishers, people, volumes, issues) through
  `cr-scrape` `cache::update::run`, stamping rows with the real API
  `date_last_updated`, resumable through `sync_state.resume_state`,
  gated by offline/refresh (ADR-071), on a worker thread. It is
  pre-flighted and bounded: a cheap probe shows the changed-row count
  and a time estimate, and a per-endpoint page cap
  (`CACHE_UPDATE_MAX_PAGES`, default 20) stops a run that a far-behind
  user would otherwise leave running for hours. The cap persists.
- The standalone `python3 -m scripts.cvcache update --into <cache>
  --api-key <key>` command for a slow backfill; `--max-pages`,
  `--since`, `--delay`, `--endpoint`, and the publisher filter apply.

**MEASURED (user key, 2026-09-20):** a live API probe confirmed the
`date_last_updated` filter narrows all four endpoints (publishers 4,
people 126, volumes 144, issues 985 in 2026-08-01|2026-08-05, against
unfiltered 9,855 / 89,713 / 160,561 / 1,139,988; dates in-window).

**T9 user tests — Update command verified (user, 2026-09-20):** the app
"Update Comic Vine Cache…" pre-flight, a capped run, and a
run-to-completion passed on a real cache with no library book changed
(user test 27). Remaining T9 user tests: the automatcher parity test
(does the ComicTagger hash still auto-match correctly, ADR-074), a
backup-import round trip, and a cached-series scrape.

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

When the user tests first: tests 1, 4, 9, and 25 stay open. Test 25's
Proposed Number regression already passes on the real library; the rest
of test 25 (the propagation report) still needs a check.

## Open user tests

The procedures are in `docs/open-user-tests.md`.

- Test 1: Library-tree gauge badges failed. No orange Unread badge appeared,
  and red New equaled green Total.
- Test 4: Select Worst Duplicates is passable. `UNKNOWN`: the requested
  improvement is not specified.
- Test 9: Library Organizer simulation showed no report.
- Test 25: Cached series metadata propagation is not complete. The Proposed
  Number regression now passes on the real library.

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

Phase 21 Task C / schema v7 (ADR-075) and the bounded, pre-flighted
"Update Comic Vine Cache…" command passed on 2026-09-20.

- `cargo fmt --all`, `cargo clippy --workspace --all-targets --
  -D warnings`, `cargo test --workspace` (63 suites): passed.
- `CR_FORMAT_TESTS=1 cargo test -p cr-scrape --test
  cvcache_schema_pin`: passed (app and scripts agree on the v7 DDL,
  both directions).
- `python3 -m unittest discover -s scripts/cvcache/tests`: 22 ok.
- New tests: the page cap stops an endpoint and holds its watermark,
  the pre-flight probe reports the changed count per endpoint
  (`cr-scrape/tests/update.rs`); `CACHE_UPDATE_MAX_PAGES` parses and
  clamps (`config.rs`); the dialog time estimate
  (`cr-ui` `dialogs::cv_update`).
- `MEASURED` (user key, live API, 2026-09-20): the
  `date_last_updated` filter narrows all four update endpoints —
  publishers 4, people 126, volumes 144, issues 985 changed in
  2026-08-01|2026-08-05.
- `UNKNOWN`: the app "Update Comic Vine Cache…" dialog and run against
  a real cache are not yet user-run. A build does not prove the UI.

## Environment notes

- Run UI probes in release mode under Xvfb with `GDK_BACKEND=x11`,
  `DISPLAY=:99`, and isolated XDG paths under `/tmp/opencode/`.
- Never point a probe at the real library.
