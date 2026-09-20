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
update pipeline, ADR-075) is done. The cache schema is now **v8**: v5
added the `issue_image` gallery (ADR-073), v6 the ComicTagger cover
hashes (ADR-074), v7 the per-endpoint sync watermark (ADR-075), v8 the
`sync_state.mode` column for the rich-enrichment cursors (ADR-075
amendment). The full `localcv.db` is imported into the live cache
(verified by the user), which the user migrated to v8 by opening the
app. T9 (user tests) is largely verified; the automatcher parity test,
a backup-import round trip, and a cached-series scrape remain. Phase 20
is implemented and its cache-manager user test (26) passed.

## Latest user finding

`DONE` (2026-09-20): the standalone `scripts/cvcache` enrichment
toolset is complete. Six phases, all shipped and verified (63 cargo
suites, 37 script tests, `cvcache_schema_pin` both directions at v8):

1. **Shared budget.** The script rate budget uses the shared
   `request_log` ledger (same as the app), keyed by the request path's
   first segment lowercased; independent cron runs share one durable
   budget. New `usage` command reports per-resource last-hour,
   remaining, total, and last-request time. A list fetch (`issues`) and
   a detail fetch (`issue`) are separate budgets, matching CV's
   per-path cap.
2. **Schema v8: `sync_state (endpoint, mode)`.** Modes
   `list`/`rich_forward`/`rich_backfill`/`hash_backfill`. v7->v8
   migration rebuilds the table and re-inserts old rows as `list`
   (MEASURED on a copy: the four watermarks survive). Cross-crate:
   Rust DDL/migration/accessors/merge and Python
   schema/merge/update/adapter move together.
3. **`rich --mode issues-backfill`.** Fills credits + gallery images
   for skeleton-only issues (the ~12.5k of the newest 20k that the list
   update added without credits). Decomposes the live `/issue/<id>/`
   detail like the localcv import; reuses the credit/issue_image merge.
4. **`rich --mode <resource>-backfill|-forward`** for person,
   character, volume. Fills/refreshes `detail_json` (real_name, powers,
   origin, bio, birth/death, etc.). No schema change — the column
   already existed. Backfill walks never-enriched rows newest-first;
   forward re-fetches only rows changed since the `rich_forward`
   watermark via the list `date_last_updated` filter.
5. **team, location, story_arc** added to the same rich machinery
   (detail path prefixes verified live: team 4060, location 4020,
   story_arc 4045).
6. **`hashes`.** Standalone ComicTagger cover-hash pass. A new
   `imagehasher.py` reimplements average/difference/perception hashes
   on Pillow (the library ComicTagger uses), so values are
   byte-identical to the stored `localcv.db` hashes (MEASURED: Hamming
   0, golden test + real CDN downloads). No ComicTagger code is copied
   (it is Apache-2.0). Image downloads hit the CV CDN, not the API, so
   the pass does NOT spend the API budget (MEASURED: no API counter
   moves). Front cover by default, `--all-images` opt-in. Resumable
   through a `hash_backfill` cursor. Pillow added to
   `scripts/requirements.txt`.

The user is running the long backfills. The `rich` command has a
combined **`all` mode** with an **`--until` deadline** built for one
daily cron job: forward for every resource first (keeps current), then
backfill history for every resource. `all` drives every unit in
stop-on-cap mode and **services one resource per wake**: after a first
full pass it sleeps until the resource whose rolling-hour window frees
soonest (with a 2-minute margin), runs only that one until it caps again,
then re-picks the next-soonest. It ends
when no resource has work left, or at the deadline (ADR-076). A budget
wait that would pass the deadline stops instead of sleeping. **HTTP 420**
(CV's transport throttle) is a per-resource backoff — 3s, 5s, 10s, then a
resumable stop — not a crash (ADR-076). `hashes` is a separate job (CDN,
no API budget). Each pass prints a `remaining=<n>` count so the backlog
scale is visible. The cron setup is documented in
`scripts/cvcache/README.md` "Running as a daily cron job".

## Previous user finding
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

Phase 21 T9 (user tests) is the remaining Phase 21 work. The full
cvcache script enrichment toolset (see "Latest user finding") is done
and verified; the user is running the long backfills.

**The `scripts/cvcache` commands (all standalone Python, resumable,
rate-aware — see `scripts/cvcache/README.md`):**
- `update` — the list-level sweep of all endpoints (publishers,
  people, volumes, issues), shared `request_log` budget, pre-flight
  changed-row counts, `--dry-run`. Also in the app as "Update Comic
  Vine Cache".
- `rich --mode issues-backfill` — issue credits + gallery images.
- `rich --mode <resource>-backfill|-forward` — person / character /
  volume / team / location / story_arc `detail_json`.
- `rich --mode all --until "HH:MM"` — the combined daily job: forward
  every resource, then backfill until the deadline. `--for N` is the
  minutes-from-now alternative. Each pass prints `remaining=<n>`.
- `hashes` — ComicTagger cover hashes (Pillow; no API budget cost;
  run as a separate cron job).
- `usage` — the shared budget report.

**Open, honest, not yet done:**
- `UNKNOWN`: whether the app already computes cover hashes on demand
  when the automatcher first sees an issue. Does not block the script
  `hashes` pass; decides only whether the bulk run is necessary or a
  convenience. One code trace in `cr-scrape` resolves it.
- `UNKNOWN`: the exact CV throttle payload (HTTP 429 / status_code 107
  are handled to the documented forms; a real throttle in a long run
  will confirm the string).
- The plan of record for the enrichment work is
  `/home/scuttle/.opencode/plan/rich-enrichment.md`.

**Task C (the in-app update pipeline) shipped earlier under ADR-075;**
read its "Task C detail" in `docs/phases/phase-21.md` and ADR-075.

**What the in-app update shipped (do not redo):**
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

The cvcache script enrichment toolset and schema v8 (ADR-075
amendments) passed on 2026-09-20.

- `cargo fmt --all`, `cargo clippy --workspace --all-targets --
  -D warnings`, `cargo test --workspace` (63 suites): passed.
- `CR_FORMAT_TESTS=1 cargo test -p cr-scrape --test
  cvcache_schema_pin`: passed (app and scripts agree on the v8 DDL,
  both directions, including `sync_state (endpoint, mode)`).
- `python3 -m unittest discover -s scripts/cvcache/tests`: 43 ok
  (includes the rich decomposition, the resource detail paths, the
  DB-backed shared budget, the `--until` deadline stop, the
  `remaining`/`total` count, and the ComicTagger cover-hash golden test
  at Hamming 0).
- `MEASURED` (live CV, 2026-09-20): stock Pillow reproduces
  `localcv.db` `ct_ahash`/`ct_phash` at Hamming 0, from real CDN
  downloads; image downloads are not counted on any API path (user
  status page).
- `MEASURED` (user session, 2026-09-20): the v7->v8 migration on a copy
  of the live cache preserved the four `list` watermarks; the rich
  passes stored credits/images/detail_json against a copy and resumed
  from their cursors; the singular detail-path budgets (`/character`,
  `/person`, `/issue`) are separate from the list budgets on the CV
  status page.

## Environment notes

- Run UI probes in release mode under Xvfb with `GDK_BACKEND=x11`,
  `DISPLAY=:99`, and isolated XDG paths under `/tmp/opencode/`.
- Never point a probe at the real library.
