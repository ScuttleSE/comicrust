"""The adapter seam (ADR-072).

An adapter turns one source format into staged v4 rows. It yields
`StagedRow`s; a staging pass writes them into a fresh in-file v4
database (the "source"), and the merge engine applies that source into
the live cache. A validation pass runs first: a failed row is
reported, never staged.

Contract:
- Input is file paths plus options.
- Output is `StagedRow`s in the v4 shape.
- Stamps come from the source's own last-updated field when it has
  one, else the adapter sets `fetched_at` to the import time and
  leaves `date_last_updated` empty (the localcv case).
"""

from __future__ import annotations

import sqlite3
from dataclasses import dataclass
from typing import Iterable, Protocol

from . import schema

# The column list per staged table. The staging INSERT uses these
# names, so an adapter fills a subset and leaves the rest to the
# column defaults.
STAGE_COLUMNS = {
    "volume": (
        "volume_id",
        "name",
        "publisher",
        "start_year",
        "count_of_issues",
        "date_last_updated",
        "last_cover_date",
        "fetched_at",
        "aliases",
        "description",
        "image_url",
        "site_detail_url",
    ),
    "issue_skeleton": (
        "issue_id",
        "volume_id",
        "issue_number",
        "cover_date",
        "name",
        "description",
        "store_date",
        "image_url",
        "date_last_updated",
        "site_detail_url",
        "fetched_at",
    ),
    "credit": (
        "owner_kind",
        "owner_id",
        "resource_kind",
        "resource_id",
        "name",
        "role",
        "marker",
    ),
    "issue_image": (
        "image_id",
        "issue_id",
        "original_url",
        "caption",
        "image_tags",
        "fetched_at",
        "ahash",
        "dhash",
        "phash",
    ),
}
for _name in schema.RESOURCE_TABLES:
    STAGE_COLUMNS[_name] = (
        "id",
        "name",
        "image_url",
        "date_last_updated",
        "date_added",
        "fetched_at",
    )


@dataclass
class StagedRow:
    table: str
    values: dict


class Adapter(Protocol):
    name: str

    def rows(self) -> Iterable[StagedRow]:
        ...


@dataclass
class ValidationOutcome:
    staged: int = 0
    rejected: int = 0
    rejects: list = None

    def __post_init__(self):
        if self.rejects is None:
            self.rejects = []


def _validate(row: StagedRow) -> str | None:
    """Returns an error string when the row is unfit to stage, else
    `None`. Checks the required keys and id shapes of the v4 tables."""
    if row.table not in STAGE_COLUMNS:
        return f"unknown table {row.table!r}"
    values = row.values
    allowed = set(STAGE_COLUMNS[row.table])
    unknown = set(values) - allowed
    if unknown:
        return f"unknown columns {sorted(unknown)} for {row.table}"
    if row.table == "volume":
        if not _is_int(values.get("volume_id")):
            return "volume_id is not an integer"
    elif row.table == "issue_skeleton":
        if not _is_int(values.get("issue_id")):
            return "issue_id is not an integer"
        if not _is_int(values.get("volume_id")):
            return "volume_id is not an integer"
        if _empty(values.get("issue_number")):
            return "issue_number is required"
    elif row.table == "credit":
        for key in ("owner_kind", "owner_id", "resource_kind", "resource_id", "marker"):
            if _empty(values.get(key)):
                return f"credit {key} is required"
    elif row.table == "issue_image":
        if not _is_int(values.get("image_id")):
            return "image_id is not an integer"
        if not _is_int(values.get("issue_id")):
            return "issue_id is not an integer"
        if _empty(values.get("original_url")):
            return "original_url is required"
    else:
        if not _is_int(values.get("id")):
            return "resource id is not an integer"
    return None


def _is_int(value) -> bool:
    return isinstance(value, int) and not isinstance(value, bool)


def _empty(value) -> bool:
    return value is None or value == ""


def stage(source: sqlite3.Connection, adapter: Adapter) -> ValidationOutcome:
    """Validates and writes the adapter rows into a fresh v4 source
    connection. Returns the staged/rejected outcome."""
    schema.create_schema(source)
    outcome = ValidationOutcome()
    inserts: dict[str, str] = {}
    for table, columns in STAGE_COLUMNS.items():
        placeholders = ", ".join("?" for _ in columns)
        col_list = ", ".join(columns)
        # Every staged table has a unique key (primary key, or the
        # credit natural index), so a duplicate within one adapter run
        # is ignored rather than raised.
        conflict = " OR IGNORE"
        inserts[table] = (
            f"INSERT{conflict} INTO {table} ({col_list}) VALUES ({placeholders})"
        )
    for row in adapter.rows():
        error = _validate(row)
        if error is not None:
            outcome.rejected += 1
            if len(outcome.rejects) < 50:
                outcome.rejects.append((row.table, error))
            continue
        columns = STAGE_COLUMNS[row.table]
        params = tuple(row.values.get(col) for col in columns)
        source.execute(inserts[row.table], params)
        outcome.staged += 1
    return outcome
