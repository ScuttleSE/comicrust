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
    [--whitelist publisher_whitelist.txt] \
    [--blacklist publisher_blacklist.txt] \
    [--no-backup]
```

A one-off import of a `sqlite_cv_pipeline` `localcv.db`. It migrates the
live file to v4, stages the localcv rows, and merges them. A
timestamped backup is made first unless `--no-backup` is given. Always
try a copy of the live file first.

Mapping and limits (localcv holds no raw per-issue API JSON, no image
blobs, and almost no per-row `date_last_updated`):

- Volumes → `volume` (publisher name joined in).
- Issues → `issue_skeleton` only (never `issue_detail`).
- Credit JSON → `credit` rows plus resource-table rows (`character`,
  `person`, `team`, `location`, `story_arc`).
- `cv_publisher`, `cv_person` → resource rows.
- Stamps: `fetched_at` = import time, `date_last_updated` empty, except
  issues in `cv_issue_last_seen` which take that stamp. A later real
  API fetch always wins on merge.
- Dropped (no v4 target): `associated_images`, publisher `country`,
  and any issue with no `issue_number` (the column is NOT NULL).

## Publisher lists

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
