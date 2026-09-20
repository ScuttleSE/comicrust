# cvcache scripts

Standard-library Python 3 tools for the Comic Vine cache file
(`cvcache.sqlite`, ADR-037). One merge engine that mirrors
`crates/cr-scrape/src/cache/import.rs`, a v4 schema pinned to
`SCHEMA_V4` in `crates/cr-scrape/src/cache/sqlite.rs`, an adapter seam,
and three commands. Run every command from the repository root so
`scripts.cvcache` resolves.

## Commands

### build — a fresh file from MCL snapshots

```sh
python3 -m scripts.cvcache build --out cvcache.sqlite snapshot.mcl [more.mcl ...]
```

Skeleton rows only, zero API requests. Reads the MCL format of
ADR-038 (`scripts/cvcache/mcl.py`).

### merge — an MCL snapshot into an existing file

```sh
python3 -m scripts.cvcache merge --into cvcache.sqlite snapshot.mcl
```

Runs through the merge engine (ADR-069). A timestamped `.bak` copy is
made first unless `--no-backup` is given.

### import-localcv — a `localcv.db` into an existing file

```sh
python3 -m scripts.cvcache import-localcv \
    --into  ~/.local/share/comicrust/plugins/comic-vine-scraper/cvcache.sqlite \
    --source /path/to/localcv.db \
    [--no-backup]
```

A one-off import of a `sqlite_cv_pipeline` `localcv.db`. It takes the
whole database, migrates the live file to v4, stages the localcv rows,
and merges them. A timestamped backup is made first unless `--no-backup`
is given. Always try a copy of the live file first.

Mapping and limits (localcv holds no raw per-issue API JSON, no image
blobs, and almost no per-row `date_last_updated`):

- Volumes → `volume` (publisher name joined in).
- Issues → `issue_skeleton` only (never `issue_detail`).
- Credit JSON → `credit` rows plus resource-table rows (`character`,
  `person`, `team`, `location`, `story_arc`).
- `associated_images` → `issue_image` rows (ADR-073, schema v5); the
  ComicTagger cover hashes from `comic_covers` attach as `ahash`/`phash`
  (ADR-074, schema v6).
- `cv_publisher`, `cv_person` → resource rows.
- Stamps: `fetched_at` = import time, `date_last_updated` empty, except
  issues in `cv_issue_last_seen` which take that stamp. A later real
  API fetch always wins on merge.
- Dropped (no target): publisher `country`, and any issue with no
  `issue_number` (the column is NOT NULL); the report lists their ids.
- Seeds `sync_state` (schema v7, ADR-075) from `cv_sync_metadata`, so
  the first `update` run starts at localcv's per-endpoint baseline.

### update — a rate-limited backfill from the CV API

```sh
python3 -m scripts.cvcache update \
    --into cvcache.sqlite --api-key YOUR_KEY \
    [--endpoint issues ...] [--max-pages 50] [--since 2026-08-03] \
    [--delay 1.0] [--max-per-hour 200] [--on-cap wait|stop] \
    [--quiet] [--whitelist ...] [--blacklist ...] [--no-backup]
```

Walks each endpoint (publishers, people, volumes, issues) over
`filter=date_last_updated:<since>|<now>`, stamps rows with the real API
`date_last_updated`, and merges through the engine (ADR-075). This is
what fills the empty `date_last_updated` the localcv import left behind.
It reads the per-endpoint `sync_state` watermark for `<since>` (or
`--since` overrides it) and advances it when an endpoint catches up. It
is resumable: a run stopped by `--max-pages`, the hourly cap, or an API
throttle saves its page offset in `sync_state.resume_state`, so the next
run continues.

The CV API is rate-limited (200 requests per resource per hour). The
script counts requests per resource over a rolling hour and enforces the
cap itself (`--max-per-hour`, default 200). The count is stored in the
shared `request_log` table — the same ledger the app writes — so
independent runs share one durable budget (for example a forward-update
cron and a backfill cron do not exceed the cap between them). The budget
key is the request path's first segment, lowercased, so a list fetch
(`issues`, `people`) and a detail fetch (`issue`, `person`) are separate
budgets, matching CV's per-path cap. At the cap the script either waits
for the window to free (`--on-cap wait`, the default, so a plain run
self-completes over several hours) or stops the endpoint cleanly
(`--on-cap stop`, resumable). A far-behind user can therefore run the
plain command and leave it. The publisher whitelist/blacklist applies at
the volume level. The same update lives in the app as "Update Comic Vine
Cache".

The `update` command shows a live per-endpoint progress display (page,
rows fetched of the changed total, percent, staged, the hourly budget,
and a wait countdown). Before fetching, a cheap pre-flight probe (one
`limit=1` request per endpoint) reads `number_of_total_results` for the
`date_last_updated:<since>|<now>` window, so the run knows and shows how
many records each endpoint will fetch. Use `--dry-run` to print that
count per endpoint and exit without fetching — for example
`--since 2026-08-20 --dry-run` shows exactly how much a backfill from
that date would fill. This display uses `rich`; install it with
`pip install -r scripts/requirements.txt`. Without `rich`, or with
`--quiet`, the command prints plain progress lines instead.

### usage — show the shared API request budget

```sh
python3 -m scripts.cvcache usage --into cvcache.sqlite
```

Reads the `request_log` ledger and prints, per resource, the requests in
the last rolling hour, the remaining budget, the lifetime total, and the
last-request time. Because the app and every script run write the same
ledger, this shows the true shared usage — useful before starting a cron
job so it knows how much budget is left.

### rich — backfill per-issue credits and images

```sh
python3 -m scripts.cvcache rich \
    --into cvcache.sqlite --api-key YOUR_KEY \
    --mode issues-backfill [--max-pages 200] [--delay 1.0] \
    [--max-per-hour 200] [--on-cap wait|stop] [--quiet] [--no-backup]
```

Fills credits and images for issues that have a skeleton row but no
credit rows — the skeleton-only issues the `update` command adds (the
list endpoint carries no credits). For each such issue it fetches the
live `/issue/<id>/` detail and decomposes it into `credit` and
`issue_image` rows, matching the localcv import shape. It walks issue
ids from newest down, so recent issues fill first, and is resumable
through the `rich_backfill` cursor in `sync_state`: a run stopped by
`--max-pages`, the hourly cap, or a kill continues from the last issue
done. The detail fetch uses the singular `/issue` path, a separate
hourly budget from the `/issues` list, and shares the `request_log`
ledger with every other run. A deleted or unknown id is skipped, not
fatal. This is a slow background job — one request per issue — meant for
a cron slice under the rate limit.

### rich — enrich person, character, and volume detail

```sh
# initial backfill: fill detail_json for rows that have none, newest first
python3 -m scripts.cvcache rich --into cvcache.sqlite --api-key YOUR_KEY \
    --mode character-backfill --max-pages 190 --on-cap stop

# forward: re-fetch only rows changed since the rich_forward watermark
python3 -m scripts.cvcache rich --into cvcache.sqlite --api-key YOUR_KEY \
    --mode character-forward
```

`<resource>-backfill` (person, character, volume) fills the `detail_json`
column for rows that have none, walking ids newest-first, resumable
through the per-resource `rich_backfill` cursor. `<resource>-forward`
uses the list endpoint's `date_last_updated` filter to find rows changed
since the `rich_forward` watermark and re-fetches only the rows the cache
already holds, advancing the watermark when caught up. Both store the
full CV detail JSON (real_name, powers, origin, bio, birth/death, etc.)
as-is. Each detail fetch uses the singular path budget (`/character`,
`/person`, `/volume`), separate from the list budgets, shared through
`request_log`. A deleted id is skipped. The initial backfill is large
(tens to hundreds of thousands of rows) and is meant to run as a cron
slice over many sessions; forward is cheap and keeps the data current.

## Publisher lists (optional, for a future probe workflow)

The batch import does not need these; it takes the whole database. The
filter exists for a later probe-the-CV-API workflow (Task C).

`--whitelist` keeps only the listed publisher ids; `--blacklist` drops
them; both together apply the whitelist first, then the blacklist. The
file format is the reference `# ID, Name` header, `#` comments, then
`ID, Name` rows. The `Publisher_Master_List_*` and
`Publisher_List_Top 75%*` files use the same format.

## Tests

```sh
python3 -m unittest discover -s scripts/cvcache/tests -t .
```

The schema pin also rides a gated cargo test that opens files both
directions:

```sh
CR_FORMAT_TESTS=1 cargo test -p cr-scrape --test cvcache_schema_pin
```
