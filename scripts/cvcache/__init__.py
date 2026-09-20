"""The cvcache script package (ADR-072).

Standard library only. One merge engine that mirrors
`crates/cr-scrape/src/cache/import.rs`, a v4 schema pinned to
`SCHEMA_V4` in `crates/cr-scrape/src/cache/sqlite.rs`, an adapter seam,
and the `build` / `merge` / `import` commands.

The merge rule, per row: the newer stamp picks the base row (the API
`date_last_updated` of both rows parse to timestamps first; where
either side has none the comparison falls back to `fetched_at`). An
empty incoming value never erases a stored value, an empty stored
value takes the incoming value, two non-empty values take the base
row's value, and a tie keeps the stored row. Image blobs compare on
`fetched_at` alone; request rows append as-is; the sweep state takes
the newer `updated_at`.
"""
