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
script counts requests per endpoint over a rolling hour and enforces the
cap itself (`--max-per-hour`, default 200). At the cap it either waits
for the window to free (`--on-cap wait`, the default, so a plain run
self-completes over several hours) or stops the endpoint cleanly
(`--on-cap stop`, resumable). A far-behind user can therefore run the
plain command and leave it. The publisher whitelist/blacklist applies at
the volume level. The same update lives in the app as "Update Comic Vine
Cache".

The `update` command shows a live per-endpoint progress display (page,
rows fetched and staged, the hourly budget, and a wait countdown). This
uses `rich`; install it with `pip install -r scripts/requirements.txt`.
Without `rich`, or with `--quiet`, the command prints plain progress
lines instead.

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
