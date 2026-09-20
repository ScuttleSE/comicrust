"""The localcv.db adapter (phase-21 Task A).

Turns the `sqlite_cv_pipeline` `localcv.db` into staged v4 rows. The
source has typed volume/issue columns and credit lists stored as JSON
text; it has no raw per-issue API JSON, no image blobs, and almost no
per-row `date_last_updated`. So:

- Issues land as `issue_skeleton` rows only, never `issue_detail`.
- The credit JSON becomes `credit` rows plus resource-table rows
  (character, person, team, location, story_arc).
- `cv_publisher` and `cv_person` become resource rows.
- The `associated_images` JSON list becomes `issue_image` rows (ADR-073),
  with the ComicTagger cover hashes from `comic_covers` attached as
  `ahash`/`phash` (ADR-074).
- Every row's `date_last_updated` is empty and `fetched_at` is the
  import time, except issues found in `cv_issue_last_seen`, which take
  that stamp. A later real API fetch always wins on merge.
- Dropped, with no v4/v5 target: publisher `country`, and issues with
  no `issue_number` (the column is NOT NULL); their ids are reported.
"""

from __future__ import annotations

import json
import sqlite3
from dataclasses import dataclass, field
from pathlib import Path
from typing import Iterable

from ..adapter import StagedRow

# The credit JSON columns of cv_issue and their v4 resource kind. The
# person list carries a role; the others do not.
_CREDIT_FIELDS = (
    ("character_credits", "character", False),
    ("person_credits", "person", True),
    ("team_credits", "team", False),
    ("location_credits", "location", False),
    ("story_arc_credits", "story_arc", False),
)


@dataclass
class LocalCvAdapter:
    db_path: Path
    fetched_at: int
    publisher_filter: object | None = None  # PublisherFilter or None
    name: str = "localcv"
    dropped: dict = field(default_factory=dict)
    numberless_issue_ids: list = field(default_factory=list)

    def rows(self) -> Iterable[StagedRow]:
        conn = sqlite3.connect(f"file:{self.db_path}?mode=ro", uri=True)
        conn.row_factory = sqlite3.Row
        try:
            self._load_cover_hashes(conn)
            allowed_volumes = self._emit_volumes_and_publishers(conn)
            yield from self._pending_publishers
            yield from self._pending_volumes
            yield from self._emit_people(conn)
            yield from self._emit_issues(conn, allowed_volumes)
            yield from self._emit_sync_state(conn)
        finally:
            conn.close()

    def _load_cover_hashes(self, conn) -> None:
        """Builds a cover-URL -> (ahash, phash) map from comic_covers
        (ADR-074). The hashes are ComicTagger average/perception
        hashes, stored per cover image URL."""
        self._cover_hashes = {}
        try:
            cursor = conn.execute(
                "SELECT cv_url, ct_ahash, ct_phash FROM comic_covers "
                "WHERE cv_url IS NOT NULL"
            )
        except sqlite3.OperationalError:
            # A localcv without the cover-hash table: no hashes to add.
            return
        for r in cursor:
            # The last write wins for a duplicated url; the hashes of one
            # image are identical whichever row carries them.
            self._cover_hashes[r["cv_url"]] = (r["ct_ahash"], r["ct_phash"])

    # -- volumes + their publishers --

    def _emit_volumes_and_publishers(self, conn) -> set[int] | None:
        """Stages publisher resource rows and volume rows. Returns the
        allowed volume-id set when the publisher filter is in force,
        else None (all volumes pass)."""
        pub_names = {
            r["id"]: r["name"]
            for r in conn.execute("SELECT id, name FROM cv_publisher")
        }
        flt = self.publisher_filter
        in_force = flt is not None and getattr(flt, "in_force", False)

        self._pending_publishers = []
        for r in conn.execute(
            "SELECT id, name, image_url, site_detail_url FROM cv_publisher"
        ):
            if in_force and not flt.allows(r["id"]):
                continue
            self._pending_publishers.append(
                StagedRow(
                    "publisher",
                    {
                        "id": r["id"],
                        "name": r["name"],
                        "image_url": r["image_url"],
                        "date_last_updated": None,
                        "date_added": None,
                        "fetched_at": self.fetched_at,
                    },
                )
            )

        allowed: set[int] | None = set() if in_force else None
        self._pending_volumes = []
        for r in conn.execute(
            "SELECT id, name, aliases, start_year, publisher_id, "
            "count_of_issues, description, image_url, site_detail_url "
            "FROM cv_volume"
        ):
            if in_force and not flt.allows(r["publisher_id"]):
                continue
            if allowed is not None:
                allowed.add(r["id"])
            self._pending_volumes.append(
                StagedRow(
                    "volume",
                    {
                        "volume_id": r["id"],
                        "name": r["name"],
                        "publisher": pub_names.get(r["publisher_id"]),
                        "start_year": _to_int(r["start_year"]),
                        "count_of_issues": _to_int(r["count_of_issues"]),
                        "date_last_updated": None,
                        "last_cover_date": None,
                        "fetched_at": self.fetched_at,
                        "aliases": r["aliases"],
                        "description": r["description"],
                        "image_url": r["image_url"],
                        "site_detail_url": r["site_detail_url"],
                    },
                )
            )
        return allowed

    # -- people --

    def _emit_people(self, conn) -> Iterable[StagedRow]:
        for r in conn.execute("SELECT id, name FROM cv_person"):
            yield StagedRow(
                "person",
                {
                    "id": r["id"],
                    "name": r["name"],
                    "image_url": None,
                    "date_last_updated": None,
                    "date_added": None,
                    "fetched_at": self.fetched_at,
                },
            )

    # -- issues + credits --

    def _emit_issues(self, conn, allowed_volumes) -> Iterable[StagedRow]:
        last_seen = {
            r["issue_id"]: r["date_last_updated"]
            for r in conn.execute(
                "SELECT issue_id, date_last_updated FROM cv_issue_last_seen"
            )
        }
        seen_resources: set[tuple[str, int]] = set()
        cursor = conn.execute(
            "SELECT id, volume_id, name, issue_number, cover_date, store_date, "
            "description, image_url, site_detail_url, character_credits, "
            "person_credits, team_credits, location_credits, story_arc_credits, "
            "associated_images FROM cv_issue"
        )
        for r in cursor:
            if allowed_volumes is not None and r["volume_id"] not in allowed_volumes:
                continue
            issue_number = r["issue_number"]
            if issue_number is None or issue_number == "":
                # issue_number is NOT NULL in v4; a numberless issue is
                # dropped, counted, and its id reported.
                self._drop("issue_no_number")
                if len(self.numberless_issue_ids) < 1000:
                    self.numberless_issue_ids.append(r["id"])
                continue
            date_updated = last_seen.get(r["id"])
            yield StagedRow(
                "issue_skeleton",
                {
                    "issue_id": r["id"],
                    "volume_id": r["volume_id"],
                    "issue_number": issue_number,
                    "cover_date": r["cover_date"],
                    "name": r["name"],
                    "description": r["description"],
                    "store_date": r["store_date"],
                    "image_url": r["image_url"],
                    "date_last_updated": date_updated,
                    "site_detail_url": r["site_detail_url"],
                    "fetched_at": self.fetched_at,
                },
            )
            yield from self._emit_credits(r, seen_resources)
            yield from self._emit_images(r)

    def _emit_credits(self, issue_row, seen_resources) -> Iterable[StagedRow]:
        issue_id = issue_row["id"]
        for column, kind, with_role in _CREDIT_FIELDS:
            entries = _parse_json_list(issue_row[column])
            for entry in entries:
                rid = _to_int(entry.get("id"))
                name = entry.get("name")
                role = None
                if with_role:
                    role = entry.get("role")
                    if role is not None:
                        role = role.strip() or None
                yield StagedRow(
                    "credit",
                    {
                        "owner_kind": "issue",
                        "owner_id": issue_id,
                        "resource_kind": kind,
                        "resource_id": rid if rid is not None else 0,
                        "name": name,
                        "role": role,
                        "marker": "credit",
                    },
                )
                # A credited resource with an id seeds its resource
                # table once (name and url only; localcv carries no
                # stamps for these).
                if rid is not None and (kind, rid) not in seen_resources:
                    seen_resources.add((kind, rid))
                    yield StagedRow(
                        kind,
                        {
                            "id": rid,
                            "name": name,
                            "image_url": None,
                            "date_last_updated": None,
                            "date_added": None,
                            "fetched_at": self.fetched_at,
                        },
                    )

    def _emit_images(self, issue_row):
        """Stages the issue's associated_images gallery as issue_image
        rows (ADR-073). An entry with no id or no original_url is
        skipped; the table keys on the image id and needs the URL."""
        issue_id = issue_row["id"]
        for entry in _parse_json_list(issue_row["associated_images"]):
            image_id = _to_int(entry.get("id"))
            url = entry.get("original_url")
            if image_id is None or not url:
                continue
            ahash, phash = self._cover_hashes.get(url, (None, None))
            yield StagedRow(
                "issue_image",
                {
                    "image_id": image_id,
                    "issue_id": issue_id,
                    "original_url": url,
                    "caption": entry.get("caption"),
                    "image_tags": entry.get("image_tags"),
                    "fetched_at": self.fetched_at,
                    "ahash": str(ahash) if ahash is not None else None,
                    "dhash": None,
                    "phash": str(phash) if phash is not None else None,
                },
            )

    def _drop(self, reason: str) -> None:
        self.dropped[reason] = self.dropped.get(reason, 0) + 1

    # -- sync watermark (ADR-075) --

    # The cvcache update endpoints. localcv's cv_sync_metadata also
    # carries internal bookkeeping rows (issues_quarterly_*) that are
    # not cvcache endpoints; those are skipped.
    _SYNC_ENDPOINTS = ("publishers", "people", "volumes", "issues")

    def _emit_sync_state(self, conn):
        """Seeds the per-endpoint watermark from cv_sync_metadata so the
        first update run knows the baseline instead of re-scanning from
        the start. Maps endpoint/last_sync_date/resume_state across."""
        try:
            cursor = conn.execute(
                "SELECT endpoint, last_sync_date, resume_state "
                "FROM cv_sync_metadata"
            )
        except sqlite3.OperationalError:
            # A localcv without the sync table: no baseline to seed.
            return
        for r in cursor:
            endpoint = r["endpoint"]
            if endpoint not in self._SYNC_ENDPOINTS:
                self._drop(f"sync_state endpoint {endpoint}")
                continue
            last_sync = r["last_sync_date"]
            if not last_sync:
                continue
            yield StagedRow(
                "sync_state",
                {
                    "endpoint": endpoint,
                    "mode": "list",
                    "last_sync": last_sync,
                    "resume_state": r["resume_state"],
                },
            )


def _to_int(value) -> int | None:
    if value is None:
        return None
    if isinstance(value, int) and not isinstance(value, bool):
        return value
    try:
        return int(str(value).strip())
    except (ValueError, TypeError):
        return None


def _parse_json_list(text) -> list:
    if not text:
        return []
    try:
        value = json.loads(text)
    except (ValueError, TypeError):
        return []
    if isinstance(value, list):
        return [v for v in value if isinstance(v, dict)]
    if isinstance(value, dict):
        return [value]
    return []
