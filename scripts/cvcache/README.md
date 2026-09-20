# cvcache scripts

Standard-library Python 3 tools for the Comic Vine cache file
(`cvcache.sqlite`, ADR-037). One merge engine that mirrors
`crates/cr-scrape/src/cache/import.rs`, a schema (currently v8) pinned
to `crates/cr-scrape/src/cache/sqlite.rs` by the `cvcache_schema_pin`
gate, an adapter seam, and the commands below. Run every command from
the repository root so `scripts.cvcache` resolves.

Most commands are standard-library only. Two optional dependencies are
declared in `scripts/requirements.txt`: `rich` (the `update` progress
display) and `Pillow` (the `hashes` pass). Install with
`pip install -r scripts/requirements.txt`.

## Command overview

| Command | What it does | API |
|---|---|---|
| `build` | Fresh cache file from MCL snapshots | none |
| `merge` | Merge an MCL snapshot into a file | none |
| `import-localcv` | One-off import of a `localcv.db` | none |
| `update` | Thin list-sweep: refresh skeletons/stamps (publishers, people, volumes, issues) by `date_last_updated`, no per-item detail | yes |
| `rich` | Fetch per-resource detail (credits, images, `detail_json`); see modes below | yes |
| `hashes` | Download covers, fill ComicTagger hashes | CDN only |
| `usage` | Report the shared `request_log` budget | none |

The `rich` modes:

| `--mode` | Fills | Direction |
|---|---|---|
| `all` | forward every resource, then backfill every resource; cycles resources and drains the whole backlog | both |
| `issues-backfill` | issue `credit` + `issue_image` rows | backfill |
| `person-backfill` `character-backfill` `volume-backfill` `team-backfill` `location-backfill` `story_arc-backfill` | resource `detail_json` for rows that have none | backfill |
| `person-forward` `character-forward` `volume-forward` `team-forward` `location-forward` `story_arc-forward` | resource `detail_json` for rows changed since the watermark | forward |

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

### update — the thin (list-sweep) update

This is the lightweight "keep current" sweep: it refreshes only the
**list-level** rows and their `date_last_updated` stamps. It does **not**
fetch per-item detail — no credits, no image gallery, no `detail_json`,
no cover hashes (those are the `rich` and `hashes` passes). One request
returns 100 rows, so it is far cheaper than the per-item passes.

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
many records each endpoint will fetch. Use `--dry-run` to size a thin
update before running it — it prints the changed-row count per endpoint
and exits without fetching:

```sh
python3 -m scripts.cvcache update --into cvcache.sqlite --api-key YOUR_KEY --dry-run
```

For example `--since 2026-08-20 --dry-run` shows exactly how much a
sweep from that date would fetch. This display uses `rich`; install it
with `pip install -r scripts/requirements.txt`. Without `rich`, or with
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
fatal. It prints `remaining=<n>` per issue; `--max-pages N` caps a
slice and `--until "HH:MM"` / `--for N` set a deadline. This is a slow
background job — one request per issue — meant for a cron slice under
the rate limit.

### rich — enrich resource detail (person, character, volume, team, location, story_arc)

```sh
# initial backfill: fill detail_json for rows that have none, newest first
python3 -m scripts.cvcache rich --into cvcache.sqlite --api-key YOUR_KEY \
    --mode character-backfill --max-pages 190 --on-cap stop

# forward: re-fetch only rows changed since the rich_forward watermark
python3 -m scripts.cvcache rich --into cvcache.sqlite --api-key YOUR_KEY \
    --mode character-forward
```

The six resources — **person, character, volume, team, location,
story_arc** — each have a `-backfill` and a `-forward` mode (see the
mode table above). `<resource>-backfill` fills the `detail_json` column
for rows that have none, walking ids newest-first, resumable through the
per-resource `rich_backfill` cursor. `<resource>-forward` uses the list
endpoint's `date_last_updated` filter to find rows changed since the
`rich_forward` watermark and re-fetches only the rows the cache already
holds, advancing the watermark when caught up. Both store the full CV
detail JSON (real_name, powers, origin, bio, birth/death, etc.) as-is.

Each detail fetch uses the singular path budget (`/character`,
`/person`, `/volume`, `/team`, `/location`, `/story_arc`), separate from
the list budgets, shared through `request_log`. A deleted id is skipped.
Each run prints `remaining=<n>` per item and a `remaining=<n> (of
<total>)` summary. `--max-pages N` caps a slice; `--until "HH:MM"` /
`--for N` set a wall-clock deadline. The initial backfill is large (tens
to hundreds of thousands of rows) and runs as a cron slice over many
sessions; forward is cheap and keeps the data current.

To run every resource in one invocation, use `--mode all` (see "Running
as a daily cron job").

### hashes — fill ComicTagger cover hashes

```sh
python3 -m scripts.cvcache hashes --into cvcache.sqlite \
    [--max 2000] [--delay 0.3] [--all-images] [--quiet] [--no-backup]
```

Downloads cover images and fills the ComicTagger `ahash`, `dhash`, and
`phash` on `issue_image` rows that have none. By default it hashes only
the front cover of each issue (the lowest `image_id` per issue, what
ComicTagger cover-matching uses); `--all-images` hashes the whole
gallery. The hashes are byte-identical to the values in the reference
`localcv.db` (MEASURED: Hamming 0), because this uses Pillow — the same
library ComicTagger runs on. Image downloads hit the CV image CDN, not
the API, so this pass does **not** spend the API rate-limit budget
(MEASURED: no API path counter moves); it paces itself with `--delay`
and is resumable through a `hash_backfill` cursor. Requires Pillow;
install it with `pip install -r scripts/requirements.txt`.

## First run: establishing the forward watermarks

The **backfill** modes fill the detail for rows already in your cache —
that is the bulk of the initial work, and they need no setup (each walks
its rows and stops at a cursor). The **forward** modes are different:
they refresh rows changed since a per-resource `rich_forward` watermark.

On a cache that has never done a rich-forward, that watermark is unset
and falls back to the floor date `1970-01-01`, so a first forward run
would filter `date_last_updated:1970-01-01|<today>` — essentially the
whole endpoint, a huge list walk that only stores rows you already hold.
That is wasteful. Seed each forward watermark to a recent date first, so
forward starts caught-up and only ever fetches genuine recent changes:

```sh
# One cheap forward run per resource pinned to a recent date. Each
# fetches only that window's changes and stamps the watermark = today.
for r in person character volume team location story_arc; do
  python3 -m scripts.cvcache rich --into cvcache.sqlite --api-key YOUR_KEY \
      --mode "$r-forward" --since "$(date -d yesterday +%F)"
done
```

After this, plain `--mode all` (or any `<resource>-forward`) is cheap:
forward only touches what changed. Do the heavy filling with the
**backfill** modes (or `--mode all`, which backfills after forward).

## Running as a daily cron job

The `rich` command has a combined `all` mode built for one cron entry:
it runs the **forward** pass for every resource first (keeps the
enriched data current — cheap, finishes in minutes), then **backfills**
history for every resource. It drives every unit in stop-on-cap mode and
cycles the units that still have work: when one resource reaches its
hourly cap it moves to the next resource, and it sleeps only when every
remaining resource is capped, until the earliest window frees. With no
`--until` it drains the whole backlog in one long-running process; with
`--until` it stops cleanly at the deadline (resumable).

```sh
# 05:00 daily: forward everything, then backfill until 04:00 (one hour
# before tomorrow's run). Resumable; stops cleanly at the deadline.
python3 -m scripts.cvcache rich --into cvcache.sqlite --api-key YOUR_KEY \
    --mode all --until "04:00"
```

> Run the "First run" watermark seeding above once before the first
> `--mode all`, or its forward phase will start from the 1970 floor and
> do a large list walk on that first run.

- **`all` drains every resource in one run.** A resource that reaches its
  hourly cap yields to the next resource instead of blocking; the run
  sleeps only when all remaining resources are capped, then resumes at
  the earliest freed window. Forward units run before backfill units, so
  "stay current" wins the budget. `--on-cap` does not apply to `all`; the
  scheduler owns cap handling. (For the single-resource modes, `--on-cap
  wait` sleeps until the window frees and `--on-cap stop` returns a
  resumable stop.)
- **HTTP 420** is CV's transport throttle. The client backs off per
  resource on a fixed ladder — 3s, then 5s, then 10s — and retries; a
  420 past the last step is treated as a rate-limit stop for that
  resource (resumable), not a crash. A successful request resets the
  ladder.
- `--until "HH:MM"` that is already past today rolls to tomorrow, so a
  05:00 job with `--until "04:00"` targets the next 04:00, not an instant
  stop. `--for <minutes>` is an alternative (stop N minutes from now).
- **There is no API-counter reset, and none is needed.** The 200/hour is
  CV's own rolling-hour count; `request_log` only mirrors it. Requests
  age out of the window 3600s after they are made, so stopping the
  backfill an hour before the next forward run leaves the budget nearly
  full when forward starts. Timing, not resetting, is the mechanism.
- The backlog drains across many days; each run continues from the
  per-resource cursors. When every resource is caught up, `all` finishes
  quickly and only the forward pass does real work.
- Progress is visible: each pass prints `remaining=<n>` per item and a
  `remaining=<n> (of <total>)` summary, where the total is the rows still
  needing enrichment (a `detail_json IS NULL` / credit-less count). So a
  first run of `character-backfill` shows `remaining` near 167k and
  counts down over the days.

Example crontab (daily forward+backfill at 05:00, hashes at 06:00):

```cron
0 5 * * *  cd /path/to/comicrust && python3 -m scripts.cvcache rich \
    --into ~/.local/share/comicrust/plugins/comic-vine-scraper/cvcache.sqlite \
    --api-key YOUR_KEY --mode all --until "04:00" --quiet >> ~/cvcache.log 2>&1
0 6 * * *  cd /path/to/comicrust && python3 -m scripts.cvcache hashes \
    --into ~/.local/share/comicrust/plugins/comic-vine-scraper/cvcache.sqlite \
    --delay 0.3 --quiet >> ~/cvhash.log 2>&1
```

Check the shared budget any time (the app and every run write the same
`request_log` ledger):

```sh
python3 -m scripts.cvcache usage --into cvcache.sqlite
```

`hashes` is a separate job (the 06:00 line above) because it uses the
image CDN, not the API, so it does not share or spend the API budget.

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
