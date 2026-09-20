"""The `build`, `merge`, and `import` operations.

- `build`: MCL files in, a fresh distributable `cvcache.sqlite` out —
  skeleton rows only, zero API requests.
- `merge`: an MCL snapshot into an existing file through the merge
  engine.
- `import`: an adapter source into an existing file through the merge
  engine, with a backup and a live-file migration first (ADR-069).
"""

from __future__ import annotations

import shutil
import sqlite3
import time
from pathlib import Path

from . import adapter, mcl, merge, schema


def open_v4(path: Path, create: bool = False) -> sqlite3.Connection:
    """Opens a cache file and brings it to v4. A missing file is an
    error unless `create` is set."""
    if not create and not path.exists():
        raise FileNotFoundError(path)
    conn = sqlite3.connect(path)
    conn.execute("PRAGMA foreign_keys = OFF")
    schema.require_supported_version(conn)
    if schema.user_version(conn) < schema.SCHEMA_VERSION:
        _migrate_to_v4(conn)
    return conn


def _migrate_to_v4(conn: sqlite3.Connection) -> None:
    """Runs the v4 DDL over a file at any version <= 4. The
    `IF NOT EXISTS` DDL is idempotent, so a v2 or v3 file gains the
    missing tables and columns without a backfill (the scripts do not
    need the v3 typed-column backfill; the app already did it)."""
    version = schema.user_version(conn)
    schema.create_schema(conn)
    _add_missing_columns(conn, version)
    conn.execute(f"PRAGMA user_version = {schema.SCHEMA_VERSION}")


def _add_missing_columns(conn: sqlite3.Connection, from_version: int) -> None:
    """`create_schema` uses CREATE TABLE IF NOT EXISTS, so an existing
    table keeps its old column set. Add the columns a pre-v4 table
    lacks."""
    wanted = {
        "volume": [
            ("detail_json", "TEXT"),
            ("aliases", "TEXT"),
            ("deck", "TEXT"),
            ("description", "TEXT"),
            ("image_url", "TEXT"),
            ("api_detail_url", "TEXT"),
            ("site_detail_url", "TEXT"),
            ("date_added", "TEXT"),
            ("first_issue_id", "INTEGER"),
            ("last_issue_id", "INTEGER"),
        ],
        "issue_skeleton": [
            ("deck", "TEXT"),
            ("description", "TEXT"),
            ("store_date", "TEXT"),
            ("image_url", "TEXT"),
            ("date_added", "TEXT"),
            ("date_last_updated", "TEXT"),
            ("api_detail_url", "TEXT"),
            ("site_detail_url", "TEXT"),
            ("fetched_at", "INTEGER NOT NULL DEFAULT 0"),
        ],
        "issue_detail": [
            ("volume_id", "INTEGER"),
            ("issue_number", "TEXT"),
            ("cover_date", "TEXT"),
            ("name", "TEXT"),
            ("store_date", "TEXT"),
            ("image_url", "TEXT"),
            ("date_added", "TEXT"),
            ("date_last_updated", "TEXT"),
        ],
    }
    for table, columns in wanted.items():
        existing = {
            r[1] for r in conn.execute(f"PRAGMA table_info({table})").fetchall()
        }
        for column, decl in columns:
            if column not in existing:
                conn.execute(f"ALTER TABLE {table} ADD COLUMN {column} {decl}")


def backup(path: Path) -> Path:
    """Copies the file next to itself with a timestamp suffix, so a
    failed run never loses the original."""
    stamp = time.strftime("%Y%m%d-%H%M%S")
    target = path.with_name(f"{path.name}.{stamp}.bak")
    shutil.copy2(path, target)
    return target


def build(mcl_paths: list[Path], out_path: Path) -> mcl.MclReport:
    """Builds a fresh v4 file from MCL snapshots. Skeleton rows only."""
    conn = sqlite3.connect(out_path)
    try:
        schema.create_schema(conn)
        fetched_at = int(time.time())
        final = mcl.MclReport()
        with conn:
            for mcl_path in mcl_paths:
                with mcl_path.open("r", encoding="utf-8", errors="replace") as handle:
                    for volume, report in mcl.read(handle):
                        final = report
                        if volume is None:
                            continue
                        _insert_mcl_volume(conn, volume, fetched_at)
        return final
    finally:
        conn.close()


def _insert_mcl_volume(conn, volume, fetched_at):
    conn.execute(
        "INSERT OR IGNORE INTO volume (volume_id, fetched_at) VALUES (?, ?)",
        (volume.volume_id, fetched_at),
    )
    for issue in volume.issues:
        conn.execute(
            "INSERT OR IGNORE INTO issue_skeleton "
            "(issue_id, volume_id, issue_number, fetched_at) VALUES (?, ?, ?, ?)",
            (issue.issue_id, volume.volume_id, issue.issue_number, fetched_at),
        )


def merge_mcl(mcl_paths: list[Path], live_path: Path, make_backup: bool = True):
    """Builds a temp file from MCL then merges it into the live file."""
    tmp = live_path.with_suffix(live_path.suffix + ".mclsrc")
    if tmp.exists():
        tmp.unlink()
    try:
        build(mcl_paths, tmp)
        return _merge_source_file(tmp, live_path, make_backup)
    finally:
        if tmp.exists():
            tmp.unlink()


def import_adapter(the_adapter, live_path: Path, make_backup: bool = True):
    """Stages an adapter into an in-memory source, then merges it into
    the live file. Returns `(validation_outcome, import_report,
    backup_path)`."""
    if make_backup:
        backup_path = backup(live_path)
    else:
        backup_path = None
    source = sqlite3.connect(":memory:")
    try:
        outcome = adapter.stage(source, the_adapter)
        source.commit()
        report = _merge_into_live(source, live_path)
    finally:
        source.close()
    return outcome, report, backup_path


def _merge_source_file(source_path: Path, live_path: Path, make_backup: bool):
    backup_path = backup(live_path) if make_backup else None
    source = sqlite3.connect(source_path)
    try:
        report = _merge_into_live(source, live_path)
    finally:
        source.close()
    return report, backup_path


def _merge_into_live(source: sqlite3.Connection, live_path: Path):
    live = open_v4(live_path)
    try:
        with live:
            report = merge.merge(live, source)
        return report
    finally:
        live.close()
