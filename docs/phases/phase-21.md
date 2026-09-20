# Phase 21: Comic Vine cache expansion

## Status

In progress. T1 and T2 are implemented and verified (commits `57d4302`,
`f6cd8a8`). T3 through T9 remain.

## Goal

Expand the local Comic Vine cache (ADR-037) in four parts. Schema v3
holds every comic resource with a last-updated stamp per row. The normal
scrape reads the cache first, with a refresh switch and an offline mode.
The cache file becomes backupable and mergeable. The sweep fills list
fields, and Python scripts build and import cache files.

## Start here (for an agent with no context)

1. Read `AGENTS.md`, `docs/current-status.md`, and this file.
2. Read ADR-037, ADR-038, ADR-063, ADR-064, ADR-069, ADR-070, ADR-071,
   and ADR-072 in `docs/decisions.md`. They carry the full design; this
   file only scopes and orders the work.
3. Read every file in the code map below before you change it.
4. There is no C# reference for this phase. It is a port addition, like
   ADR-064. The Comic Vine API documentation page is the spec:
   https://comicvine.gamespot.com/api/documentation
5. Rules that bite here: cache and network work never runs on the GTK
   thread (Rule 9); every claim about run-time behavior needs a tag
   (Rule 10); report the three states at every step (Rule 2).

## The cache today (verified 2026-09-20)

| Fact | Where |
|---|---|
| One SQLite file, WAL mode, `user_version` 2 | `crates/cr-scrape/src/cache/sqlite.rs:15,116` |
| Tables: `volume`, `issue_skeleton`, `issue_detail`, `image_blob`, `search_result`, `request_log`, `sweep_state`, `pending_issue_detail` | `sqlite.rs:17-86` |
| The volume merge uses COALESCE (an empty value never erases) | `sqlite.rs:362-399` |
| The cache-manager update stores the full volume JSON and per-issue JSON | `sqlite.rs:213-278,324-358`, `cache/manage.rs` |
| `search_result` and `image_blob` have no production writer or reader | searched `crates/` on 2026-09-20; only tests touch them |
| The scrape always goes online: search, issue list, issue detail, images | `cv/queries.rs:134,271,400,413` |
| `query_issue` sends NO `field_list`, so the API returns ALL issue fields | `cv/queries.rs:400-404` |
| `series_details` requests `/volume/4050-N/` once per series per run | `cv/queries.rs:693-742` |
| The freshness rule: closed volumes serve fresh, open volumes probe after 24 h | `cache/freshness.rs:60-102` |
| The budget chokepoint and the per-resource request log | `cache/budget.rs`, `cv/connection.rs:148-158` |
| The sweep pages `/issues` with `field_list=id,issue_number,volume` | `cache/sweep.rs:153-166` |
| The "Update Comic Vine Cache" command | `crates/cr-ui/src/browser/shell.rs:5143` |
| The cache-manager dialog | `crates/cr-ui/src/dialogs/cache_manager.rs` |
| The scraper config dialog: flag check boxes + an advanced KEY=VALUE text view | `crates/cr-ui/src/dialogs/scrape_config.rs:156,208-223` |
| The advanced settings parser and the CACHE_* key list | `crates/cr-scrape/src/config.rs:55-72,249,282-287` |
| The cv job kinds and the single-job slot | `crates/cr-ui/src/library.rs:77-79,242` |

## API facts (MEASURED 2026-09-20, the documentation page)

- Comic-relevant resources: `volume`, `issue`, `publisher`,
  `character`, `person`, `team`, `story_arc`, `location`, `concept`,
  `object`. Lookup lists: `origin`, `power`. Not comic metadata:
  `chat`, `episode`, `movie`, `series`, `video*`, `promo`, `types`,
  `search`.
- Every comic-relevant resource carries `date_last_updated` and
  `date_added`. The `origin` lookup list does not.
- The `/issues` LIST resource documents: `aliases`, `api_detail_url`,
  `cover_date`, `date_added`, `date_last_updated`, `deck`,
  `description`, `has_staff_review`, `id`, `image`, `issue_number`,
  `name`, `site_detail_url`, `store_date`, `volume`. The credit lists
  are DETAIL-only fields.
- The `/volume` detail carries the 21-field list ADR-064 already
  fetches (`cache/manage.rs:17`).
- Known caveat (ADR-037, ADR-038): the docs page renders its Sort and
  Filter marks as images. The page text does not state which fields are
  filterable. The one proven filter is `date_last_updated` on `/issues`
  (the Update Missing script uses it in production).

## Scope

- Schema v3: the resource tables, the credit table, the typed columns,
  the row stamps, and the v2→v3 backfill (ADR-070).
- Local-first reads for search, series details, issue list, issue
  detail, and images (ADR-071).
- The `CACHE_REFRESH_MODE` switch (default manual) and the
  `CACHE_OFFLINE_ONLY` toggle (ADR-071).
- The backup command, the checkpoint on close, and the import with the
  newer-stamp merge (ADR-069).
- The sweep `field_list` expansion (ADR-072).
- `scripts/cvcache/`: the merge engine, build, merge, the adapter
  shape, and the schema pin test (ADR-072).
- The per-volume related-resources fetch in the cache manager
  (ADR-070).

## Exclusions

- No ComicDb.xml change. The cache stays disposable (ADR-037).
- No automatic related-resource detail fetch. On demand, per volume,
  only.
- No per-field stamps. The API provides per-row stamps only.
- The sweep never downloads images and never fetches per-issue details.
- No populated cache file ships before the terms check (ADR-072).
- No other scraper source (Metron or similar) in this phase.

## Locked decisions

- ADR-069: backup = `VACUUM INTO`; checkpoint on close; import =
  temp-copy migration + newer-stamp merge; an empty value never erases.
- ADR-070: schema v3; resource tables; the credit table; row stamps;
  related-resource details on demand, per volume.
- ADR-071: local-first per data kind; `CACHE_REFRESH_MODE=manual` is
  the default; `CACHE_OFFLINE_ONLY` blocks at the client chokepoint.
- ADR-072: the sweep widens its `field_list` at zero extra requests;
  Python scripts with one merge engine and pluggable adapters; the
  schema pin test; a populated file waits for the terms check.

## Tasks

- [x] **T1 — Schema v3** (`crates/cr-scrape/src/cache/sqlite.rs`): the
      new tables and columns of ADR-070, plus the migration with the
      `detail_json` backfill. Acceptance: a v2 file with stored details
      migrates; the raw JSON text stays byte-identical; the v1→v2→v3
      chain passes; the new columns fill.
- [x] **T2 — Inline credits and resource upserts**: extract the credit
      lists from issue and volume detail JSON into `credit` and the
      resource tables; extend the `CvCache` trait; make the complete
      issue write path store them. Acceptance: mock-server tests cover
      every credit marker and an idempotent re-import.
- [ ] **T3 — Local-first reads** (ADR-071): route search, series
      details, issue detail, and images through the cache first.
      `cv/queries.rs` and `engine.rs` change; `parse_issue` parses
      stored JSON. Acceptance: a mock-server test scrapes a fully
      cached series with ZERO API requests.
- [ ] **T4 — Refresh switch and offline mode**: the two config keys
      with defaults and parse rules; the check boxes in the config
      dialog; `CvError::Offline` at the chokepoint; the warm, sweep,
      and cache-manager disabled states. Acceptance: in manual mode an
      open volume makes no probe request; in offline mode no request
      leaves the process.
- [ ] **T5 — Backup and import** (ADR-069): the `VACUUM INTO` command,
      the close checkpoint, and the temp-copy import with validation,
      the merge, and the per-table report. Acceptance: unit tests cover
      newer-wins, empty-never-erases, the blob rule, a tie that keeps
      the stored row, a rejected newer schema, and a v1 file that
      migrates in the temp copy.
- [ ] **T6 — Cache-manager dialog**: "Back up cache…", "Import cache
      from file…", and the per-volume related-resources fetch
      (budget-bound, cancellable, resumable through the pending-queue
      pattern). Offline mode disables the API buttons. Acceptance: a
      release probe drives all three operations.
- [ ] **T7 — Sweep expansion** (ADR-072): the widened `field_list`, the
      new column fills, image URL storage, and volume name records.
      Acceptance: a mock-server test holds the request count at one per
      page and shows the new columns filled.
- [ ] **T8 — Python scripts** (ADR-072): `scripts/cvcache/` with the
      merge engine, build, merge, one adapter shape, and the schema pin
      test; the gated CI hook. Acceptance: a script-built file opens
      and merges in the app; an app-written file opens in a script.
- [ ] **T9 — Docs and user tests**: `docs/config-reference.md` rows for
      the two new keys; user-test procedures in
      `docs/open-user-tests.md`; `docs/current-status.md` updated.

## Verification

- `cargo fmt --all`, `cargo clippy --workspace --all-targets --
  -D warnings`, `cargo test --workspace`.
- A release UI probe under Xvfb with isolated XDG paths drives the
  cache-manager operations (the phase-20 probe pattern).
- User tests: a backup-import round trip on a real cache; a
  cached-series scrape with the request log watched; offline mode with
  the network cut; an import of a script-built file.

## Open issues

- `UNKNOWN`: whether the Comic Vine API terms permit redistribution of
  a populated cache file. Check before any populated file ships.
- `UNKNOWN`: the inline sub-fields of the `volume` object inside
  `/issues` responses. One real response settles it at T7.
- `UNKNOWN`: the detail URL prefixes for the resources beyond `volume`
  (4050) and `issue` (4000). Read them from real responses or the docs
  at T2.
- `UNKNOWN`: whether the CI image carries `python3`. Try the gated test
  at T8.
- The image layer can grow large once the scrape stores every cover
  (one blob per issue). A prune command is NOT in this phase. Record
  the need when a measurement shows it.

## Completion record

Fill this in only when the phase closes.
