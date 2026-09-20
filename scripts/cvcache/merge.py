"""The merge engine (ADR-069), a port of
`crates/cr-scrape/src/cache/import.rs`.

The per-table order and the added/updated/skipped/rejected counts
mirror the Rust engine, so an app import and a script import cannot
diverge. The source is a v4 connection; the live is a v4 connection
inside one transaction the caller holds.
"""

from __future__ import annotations

import sqlite3
from dataclasses import dataclass, field
from datetime import datetime, timezone

from . import schema


def parse_api_date(value: str | None) -> int | None:
    """Parses `YYYY-MM-DD HH:MM:SS` or a bare `YYYY-MM-DD` to unix
    seconds (the `parse_api_date` of import.rs)."""
    if value is None:
        return None
    value = value.strip()
    if not value:
        return None
    for fmt in ("%Y-%m-%d %H:%M:%S", "%Y-%m-%d"):
        try:
            dt = datetime.strptime(value, fmt).replace(tzinfo=timezone.utc)
            return int(dt.timestamp())
        except ValueError:
            continue
    return None


def incoming_is_newer(
    incoming_date: str | None,
    incoming_at: int,
    stored_date: str | None,
    stored_at: int,
) -> bool:
    """Both API stamps present -> compare them; otherwise compare
    `fetched_at`. A tie keeps the stored row."""
    a = parse_api_date(incoming_date)
    b = parse_api_date(stored_date)
    if a is not None and b is not None:
        return a > b
    return incoming_at > stored_at


def _empty_text(value) -> bool:
    return value is None or value == ""


def merge_text(stored, incoming, base_is_incoming):
    if _empty_text(incoming):
        return stored
    if _empty_text(stored):
        return incoming
    return incoming if base_is_incoming else stored


def merge_int(stored, incoming, base_is_incoming):
    if incoming is None:
        return stored
    if stored is None:
        return incoming
    return incoming if base_is_incoming else stored


def merge_stamp(stored, incoming, base_is_incoming):
    return incoming if base_is_incoming else stored


@dataclass
class TableReport:
    name: str
    added: int = 0
    updated: int = 0
    skipped: int = 0
    rejected: int = 0


@dataclass
class ImportReport:
    tables: list[TableReport] = field(default_factory=list)

    def table(self, name: str) -> TableReport:
        for t in self.tables:
            if t.name == name:
                return t
        t = TableReport(name)
        self.tables.append(t)
        return t


_VOLUME_COLUMNS = (
    "volume_id, name, publisher, start_year, count_of_issues, "
    "date_last_updated, last_cover_date, fetched_at, detail_json, aliases, "
    "deck, description, image_url, api_detail_url, site_detail_url, "
    "date_added, first_issue_id, last_issue_id"
)

_ISSUE_DETAIL_COLUMNS = (
    "issue_id, json, fetched_at, volume_id, issue_number, cover_date, name, "
    "store_date, image_url, date_added, date_last_updated"
)

_RESOURCE_COLUMNS = (
    "id, name, image_url, date_last_updated, date_added, fetched_at, detail_json"
)


def merge(live: sqlite3.Connection, source: sqlite3.Connection) -> ImportReport:
    report = ImportReport()
    _merge_volumes(live, source, report)
    _merge_skeletons(live, source, report)
    _merge_issue_details(live, source, report)
    _merge_blobs(live, source, report)
    _merge_searches(live, source, report)
    _merge_requests(live, source, report)
    _merge_sweep_state(live, source, report)
    _merge_sync_state(live, source, report)
    _merge_pending(live, source, report)
    for name in schema.RESOURCE_TABLES:
        _merge_resource(live, source, name, report)
    _merge_credits(live, source, report)
    _merge_issue_images(live, source, report)
    return report


def _merge_volumes(live, source, report):
    table = report.table("volume")
    cols = _VOLUME_COLUMNS
    for row in source.execute(f"SELECT {cols} FROM volume").fetchall():
        volume_id = row[0]
        if volume_id is None:
            table.rejected += 1
            continue
        stored = live.execute(
            f"SELECT {cols} FROM volume WHERE volume_id = ?", (volume_id,)
        ).fetchone()
        if stored is None:
            live.execute(
                f"INSERT INTO volume ({cols}) VALUES ({_placeholders(18)})", row
            )
            table.added += 1
            continue
        base = incoming_is_newer(row[5], row[7], stored[5], stored[7])
        merged = _merge_volume_row(stored, row, base)
        if merged == stored:
            table.skipped += 1
        else:
            live.execute(
                f"INSERT INTO volume ({cols}) VALUES ({_placeholders(18)}) "
                "ON CONFLICT(volume_id) DO UPDATE SET "
                "name=?2, publisher=?3, start_year=?4, count_of_issues=?5, "
                "date_last_updated=?6, last_cover_date=?7, fetched_at=?8, "
                "detail_json=?9, aliases=?10, deck=?11, description=?12, "
                "image_url=?13, api_detail_url=?14, site_detail_url=?15, "
                "date_added=?16, first_issue_id=?17, last_issue_id=?18",
                merged,
            )
            table.updated += 1


def _merge_volume_row(stored, incoming, base):
    return (
        stored[0],
        merge_text(stored[1], incoming[1], base),
        merge_text(stored[2], incoming[2], base),
        merge_int(stored[3], incoming[3], base),
        merge_int(stored[4], incoming[4], base),
        merge_text(stored[5], incoming[5], base),
        merge_text(stored[6], incoming[6], base),
        merge_stamp(stored[7], incoming[7], base),
        merge_text(stored[8], incoming[8], base),
        merge_text(stored[9], incoming[9], base),
        merge_text(stored[10], incoming[10], base),
        merge_text(stored[11], incoming[11], base),
        merge_text(stored[12], incoming[12], base),
        merge_text(stored[13], incoming[13], base),
        merge_text(stored[14], incoming[14], base),
        merge_text(stored[15], incoming[15], base),
        merge_int(stored[16], incoming[16], base),
        merge_int(stored[17], incoming[17], base),
    )


def _merge_skeletons(live, source, report):
    table = report.table("issue_skeleton")
    rows = source.execute(
        "SELECT issue_id, volume_id, issue_number, cover_date, name, deck, "
        "description, store_date, image_url, date_added, date_last_updated, "
        "api_detail_url, site_detail_url, fetched_at FROM issue_skeleton"
    ).fetchall()
    for row in rows:
        issue_id, volume_id, issue_number = row[0], row[1], row[2]
        if issue_id is None or volume_id is None or issue_number is None:
            table.rejected += 1
            continue
        stored = live.execute(
            "SELECT issue_id, volume_id, issue_number, cover_date, name, deck, "
            "description, store_date, image_url, date_added, date_last_updated, "
            "api_detail_url, site_detail_url, fetched_at "
            "FROM issue_skeleton WHERE issue_id = ?",
            (issue_id,),
        ).fetchone()
        if stored is None:
            live.execute(
                "INSERT INTO issue_skeleton (issue_id, volume_id, issue_number, "
                "cover_date, name, deck, description, store_date, image_url, "
                "date_added, date_last_updated, api_detail_url, site_detail_url, "
                f"fetched_at) VALUES ({_placeholders(14)})",
                row,
            )
            table.added += 1
            continue
        base = incoming_is_newer(row[10], row[13], stored[10], stored[13])
        merged = (
            issue_id,
            volume_id,
            issue_number,
            merge_text(stored[3], row[3], base),
            merge_text(stored[4], row[4], base),
            merge_text(stored[5], row[5], base),
            merge_text(stored[6], row[6], base),
            merge_text(stored[7], row[7], base),
            merge_text(stored[8], row[8], base),
            merge_text(stored[9], row[9], base),
            merge_text(stored[10], row[10], base),
            merge_text(stored[11], row[11], base),
            merge_text(stored[12], row[12], base),
            merge_stamp(stored[13], row[13], base),
        )
        if merged == stored:
            table.skipped += 1
        else:
            live.execute(
                "UPDATE issue_skeleton SET cover_date=?4, name=?5, deck=?6, "
                "description=?7, store_date=?8, image_url=?9, date_added=?10, "
                "date_last_updated=?11, api_detail_url=?12, site_detail_url=?13, "
                "fetched_at=?14 WHERE issue_id=?1",
                merged,
            )
            table.updated += 1


def _merge_issue_details(live, source, report):
    table = report.table("issue_detail")
    cols = _ISSUE_DETAIL_COLUMNS
    for row in source.execute(f"SELECT {cols} FROM issue_detail").fetchall():
        issue_id = row[0]
        if issue_id is None:
            table.rejected += 1
            continue
        stored = live.execute(
            f"SELECT {cols} FROM issue_detail WHERE issue_id = ?", (issue_id,)
        ).fetchone()
        if stored is None:
            live.execute(
                f"INSERT INTO issue_detail ({cols}) VALUES ({_placeholders(11)})",
                row,
            )
            table.added += 1
            continue
        base = incoming_is_newer(row[10], row[2], stored[10], stored[2])
        merged = (
            issue_id,
            merge_text(stored[1], row[1], base),
            merge_stamp(stored[2], row[2], base),
            merge_int(stored[3], row[3], base),
            merge_text(stored[4], row[4], base),
            merge_text(stored[5], row[5], base),
            merge_text(stored[6], row[6], base),
            merge_text(stored[7], row[7], base),
            merge_text(stored[8], row[8], base),
            merge_text(stored[9], row[9], base),
            merge_text(stored[10], row[10], base),
        )
        if merged == stored:
            table.skipped += 1
        else:
            live.execute(
                f"INSERT INTO issue_detail ({cols}) VALUES ({_placeholders(11)}) "
                "ON CONFLICT(issue_id) DO UPDATE SET json=?2, fetched_at=?3, "
                "volume_id=?4, issue_number=?5, cover_date=?6, name=?7, "
                "store_date=?8, image_url=?9, date_added=?10, date_last_updated=?11",
                merged,
            )
            table.updated += 1


def _merge_blobs(live, source, report):
    table = report.table("image_blob")
    for url, blob, fetched_at in source.execute(
        "SELECT url, bytes, fetched_at FROM image_blob"
    ).fetchall():
        if url is None or blob is None:
            table.rejected += 1
            continue
        stored = live.execute(
            "SELECT fetched_at FROM image_blob WHERE url = ?", (url,)
        ).fetchone()
        if stored is None:
            live.execute(
                "INSERT INTO image_blob (url, bytes, fetched_at) VALUES (?, ?, ?)",
                (url, blob, fetched_at),
            )
            table.added += 1
        elif fetched_at > stored[0]:
            live.execute(
                "UPDATE image_blob SET bytes = ?, fetched_at = ? WHERE url = ?",
                (blob, fetched_at, url),
            )
            table.updated += 1
        else:
            table.skipped += 1


def _merge_searches(live, source, report):
    table = report.table("search_result")
    for terms, json_text, fetched_at in source.execute(
        "SELECT terms, json, fetched_at FROM search_result"
    ).fetchall():
        if terms is None or json_text is None:
            table.rejected += 1
            continue
        stored = live.execute(
            "SELECT fetched_at FROM search_result WHERE terms = ?", (terms,)
        ).fetchone()
        if stored is None:
            live.execute(
                "INSERT INTO search_result (terms, json, fetched_at) VALUES (?, ?, ?)",
                (terms, json_text, fetched_at),
            )
            table.added += 1
        elif fetched_at > stored[0]:
            live.execute(
                "UPDATE search_result SET json = ?, fetched_at = ? WHERE terms = ?",
                (json_text, fetched_at, terms),
            )
            table.updated += 1
        else:
            table.skipped += 1


def _merge_requests(live, source, report):
    table = report.table("request_log")
    for resource, at in source.execute(
        "SELECT resource, at FROM request_log"
    ).fetchall():
        if resource is None or at is None:
            table.rejected += 1
            continue
        live.execute(
            "INSERT INTO request_log (resource, at) VALUES (?, ?)", (resource, at)
        )
        table.added += 1


def _merge_sweep_state(live, source, report):
    table = report.table("sweep_state")
    incoming = source.execute(
        "SELECT start_date, end_date, offset, total, updated_at "
        "FROM sweep_state WHERE id = 1"
    ).fetchone()
    if incoming is None:
        return
    stored = live.execute(
        "SELECT updated_at FROM sweep_state WHERE id = 1"
    ).fetchone()
    if stored is None:
        live.execute(
            "INSERT INTO sweep_state (id, start_date, end_date, offset, total, "
            "updated_at) VALUES (1, ?, ?, ?, ?, ?)",
            incoming,
        )
        table.added += 1
    elif incoming[4] > stored[0]:
        live.execute(
            "UPDATE sweep_state SET start_date=?, end_date=?, offset=?, total=?, "
            "updated_at=? WHERE id = 1",
            incoming,
        )
        table.updated += 1
    else:
        table.skipped += 1


def _merge_sync_state(live, source, report):
    table = report.table("sync_state")
    rows = source.execute(
        "SELECT endpoint, mode, last_sync, resume_state FROM sync_state"
    ).fetchall()
    for endpoint, mode, last_sync, resume_state in rows:
        if endpoint is None or last_sync is None:
            table.rejected += 1
            continue
        if mode is None:
            mode = "list"
        stored = live.execute(
            "SELECT last_sync FROM sync_state WHERE endpoint = ? AND mode = ?",
            (endpoint, mode),
        ).fetchone()
        if stored is None:
            live.execute(
                "INSERT INTO sync_state (endpoint, mode, last_sync, resume_state) "
                "VALUES (?, ?, ?, ?)",
                (endpoint, mode, last_sync, resume_state),
            )
            table.added += 1
        # The API `YYYY-MM-DD` form sorts lexically; the newer wins.
        elif last_sync > stored[0]:
            live.execute(
                "UPDATE sync_state SET last_sync = ?, resume_state = ? "
                "WHERE endpoint = ? AND mode = ?",
                (last_sync, resume_state, endpoint, mode),
            )
            table.updated += 1
        else:
            table.skipped += 1


def _merge_pending(live, source, report):
    table = report.table("pending_issue_detail")
    for volume_id, issue_id, position in source.execute(
        "SELECT volume_id, issue_id, position FROM pending_issue_detail"
    ).fetchall():
        if volume_id is None or issue_id is None:
            table.rejected += 1
            continue
        cur = live.execute(
            "INSERT OR IGNORE INTO pending_issue_detail (volume_id, issue_id, "
            "position) VALUES (?, ?, ?)",
            (volume_id, issue_id, position if position is not None else 0),
        )
        if cur.rowcount > 0:
            table.added += 1
        else:
            table.skipped += 1


def _merge_resource(live, source, name, report):
    table = report.table(name)
    cols = _RESOURCE_COLUMNS
    for row in source.execute(f"SELECT {cols} FROM {name}").fetchall():
        rid = row[0]
        if rid is None:
            table.rejected += 1
            continue
        stored = live.execute(
            f"SELECT {cols} FROM {name} WHERE id = ?", (rid,)
        ).fetchone()
        if stored is None:
            live.execute(
                f"INSERT INTO {name} ({cols}) VALUES ({_placeholders(7)})", row
            )
            table.added += 1
            continue
        base = incoming_is_newer(row[3], row[5], stored[3], stored[5])
        merged = (
            rid,
            merge_text(stored[1], row[1], base),
            merge_text(stored[2], row[2], base),
            merge_text(stored[3], row[3], base),
            merge_text(stored[4], row[4], base),
            merge_stamp(stored[5], row[5], base),
            merge_text(stored[6], row[6], base),
        )
        if merged == stored:
            table.skipped += 1
        else:
            live.execute(
                f"INSERT INTO {name} ({cols}) VALUES ({_placeholders(7)}) "
                "ON CONFLICT(id) DO UPDATE SET name=?2, image_url=?3, "
                "date_last_updated=?4, date_added=?5, fetched_at=?6, detail_json=?7",
                merged,
            )
            table.updated += 1


def _merge_credits(live, source, report):
    table = report.table("credit")
    for row in source.execute(
        "SELECT owner_kind, owner_id, resource_kind, resource_id, name, role, "
        "marker FROM credit"
    ).fetchall():
        owner_kind, owner_id, resource_kind, resource_id = row[0], row[1], row[2], row[3]
        marker = row[6]
        if None in (owner_kind, owner_id, resource_kind, resource_id, marker):
            table.rejected += 1
            continue
        cur = live.execute(
            "INSERT OR IGNORE INTO credit (owner_kind, owner_id, resource_kind, "
            "resource_id, name, role, marker) VALUES (?, ?, ?, ?, ?, ?, ?)",
            row,
        )
        if cur.rowcount > 0:
            table.added += 1
        else:
            table.skipped += 1


def _merge_issue_images(live, source, report):
    table = report.table("issue_image")
    rows = source.execute(
        "SELECT image_id, issue_id, original_url, caption, image_tags, fetched_at, "
        "ahash, dhash, phash FROM issue_image"
    ).fetchall()
    for (
        image_id,
        issue_id,
        original_url,
        caption,
        image_tags,
        fetched_at,
        ahash,
        dhash,
        phash,
    ) in rows:
        if image_id is None or issue_id is None or _empty_text(original_url):
            table.rejected += 1
            continue
        stored = live.execute(
            "SELECT issue_id, original_url, caption, image_tags, fetched_at, "
            "ahash, dhash, phash FROM issue_image WHERE image_id = ?",
            (image_id,),
        ).fetchone()
        if stored is None:
            live.execute(
                "INSERT INTO issue_image (image_id, issue_id, original_url, caption, "
                "image_tags, fetched_at, ahash, dhash, phash) "
                "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
                (
                    image_id,
                    issue_id,
                    original_url,
                    caption,
                    image_tags,
                    fetched_at,
                    ahash,
                    dhash,
                    phash,
                ),
            )
            table.added += 1
            continue
        # No API stamp: fetched_at alone decides, empty never erases.
        base = fetched_at > stored[4]
        merged = (
            merge_int(stored[0], issue_id, base),
            merge_text(stored[1], original_url, base),
            merge_text(stored[2], caption, base),
            merge_text(stored[3], image_tags, base),
            fetched_at if base else stored[4],
            merge_text(stored[5], ahash, base),
            merge_text(stored[6], dhash, base),
            merge_text(stored[7], phash, base),
        )
        if merged == stored:
            table.skipped += 1
        else:
            live.execute(
                "UPDATE issue_image SET issue_id = ?, original_url = ?, caption = ?, "
                "image_tags = ?, fetched_at = ?, ahash = ?, dhash = ?, phash = ? "
                "WHERE image_id = ?",
                (*merged, image_id),
            )
            table.updated += 1


def _placeholders(count: int) -> str:
    # sqlite3 accepts ?1.. numbered params; the ON CONFLICT arms reuse
    # them, so number every position.
    return ", ".join(f"?{i}" for i in range(1, count + 1))
