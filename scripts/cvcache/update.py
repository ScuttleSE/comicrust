"""The standalone update/backfill pipeline (ADR-075).

An update-only run pulls each endpoint's rows that changed since the
per-endpoint `sync_state.last_sync` watermark, stamps them with the
real API `date_last_updated`, and merges them through the shared merge
engine (`merge.py`). It is resumable across runs: a run that stops
mid-window saves its page offset in `sync_state.resume_state`, so the
next run continues instead of paying for the same pages again.

The Comic Vine API is heavily rate-limited (200 requests per resource
per hour), so a user weeks or months behind runs this slowly, over
many sessions, until every endpoint is caught up. `CvClient` counts
requests per endpoint over a rolling hour and enforces the cap itself:
it either waits for the window to free (default) or stops the endpoint
cleanly, saving a resume offset. The in-app update (cr-scrape) shares
the same `sync_state` model and merge engine.

The fetch and merge core is standard-library only; the CLI progress
display (`__main__.py`) uses `rich`. The one proven filter is
`date_last_updated`; a live probe on 2026-09-20 confirmed it narrows
all four endpoints (publishers, people, volumes, issues).
"""

from __future__ import annotations

import io
import json
import sqlite3
import time
import urllib.error
import urllib.parse
import urllib.request
from collections import deque
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path

from . import commands, merge, schema

API_BASE = "https://comicvine.gamespot.com/api"
USER_AGENT = "comicrust-cvcache/0.1"
PAGE_SIZE = 100
# The endpoints an update walks, in cheap-to-expensive order.
ENDPOINTS = ("publishers", "people", "volumes", "issues")
# The CV cap is 200 requests per resource path per rolling hour. A small
# safety margin keeps the script clear of a race with CV's own count.
MAX_PER_HOUR = 200
SAFETY_MARGIN = 5
RATE_WINDOW_SECONDS = 3600.0

# The list-level field_list per endpoint. Only fields the cvcache
# schema stores are requested, to keep the response small.
_FIELD_LISTS = {
    "publishers": "id,name,image,date_added,date_last_updated",
    "people": "id,name,image,date_added,date_last_updated",
    "volumes": (
        "id,name,publisher,start_year,count_of_issues,aliases,"
        "description,image,site_detail_url,date_added,date_last_updated"
    ),
    "issues": (
        "id,issue_number,volume,name,cover_date,deck,description,"
        "store_date,image,date_added,date_last_updated,site_detail_url,"
        "api_detail_url"
    ),
}


class CvError(RuntimeError):
    pass


class NotFound(RuntimeError):
    """CV reports the requested id does not exist (deleted or unknown).
    A rich detail fetch skips it; it is not a fatal error."""

    pass


class RateLimitReached(RuntimeError):
    """One endpoint reached its hourly request budget in `stop` mode, or
    the API returned a throttle response. The caller saves the resume
    offset and continues on a later run."""

    def __init__(self, endpoint: str, detail: str = ""):
        self.endpoint = endpoint
        super().__init__(f"{endpoint}: rate limit reached {detail}".strip())


@dataclass
class CvClient:
    api_key: str
    delay_seconds: float = 1.0
    max_per_hour: int = MAX_PER_HOUR
def resource_of(path: str) -> str:
    """The budget key for a request path: the first non-empty path
    segment, lowercased. Matches the app's `resource_of` (budget.rs), so
    the script and the app share one `request_log` ledger. A list fetch
    (`issues`, `people`) and a detail fetch (`issue`, `person`) are
    separate keys, which matches CV's per-path cap."""
    for seg in path.split("/"):
        if seg:
            return seg.lower()
    return "unknown"


@dataclass
class CvClient:
    api_key: str
    delay_seconds: float = 1.0
    max_per_hour: int = MAX_PER_HOUR
    safety_margin: int = SAFETY_MARGIN
    on_cap: str = "wait"  # "wait" (sleep until the window frees) or "stop"
    # A live sqlite connection to the cache. When set, the per-resource
    # hourly budget is read from and written to `request_log`, so
    # independent runs (e.g. a forward cron and a backfill cron) share
    # one durable budget. When None, the budget falls back to an
    # in-memory count (used only in isolated tests).
    ledger: object = None
    _last_call: float = 0.0
    _mem_calls: dict = field(default_factory=dict)  # fallback only
    # Injectable for tests; real code uses the wall/monotonic clock.
    _mono: object = time.monotonic
    _wall: object = time.time
    _sleep: object = time.sleep
    on_wait: object = None  # optional callback(resource, seconds) before a sleep

    def _budget(self) -> int:
        return max(1, self.max_per_hour - self.safety_margin)

    def _used_since(self, resource: str, since: int) -> int:
        if self.ledger is not None:
            row = self.ledger.execute(
                "SELECT COUNT(*) FROM request_log WHERE resource = ? AND at >= ?",
                (resource, since),
            ).fetchone()
            return int(row[0]) if row else 0
        calls = self._mem_calls.setdefault(resource, deque())
        while calls and calls[0] < since:
            calls.popleft()
        return len(calls)

    def _oldest_since(self, resource: str, since: int) -> int | None:
        if self.ledger is not None:
            row = self.ledger.execute(
                "SELECT MIN(at) FROM request_log WHERE resource = ? AND at >= ?",
                (resource, since),
            ).fetchone()
            return int(row[0]) if row and row[0] is not None else None
        calls = self._mem_calls.get(resource)
        return int(calls[0]) if calls else None

    def _record(self, resource: str, now: int) -> None:
        if self.ledger is not None:
            self.ledger.execute(
                "INSERT INTO request_log (resource, at) VALUES (?, ?)",
                (resource, now),
            )
            # Prune lazily so the log cannot grow without end.
            self.ledger.execute(
                "DELETE FROM request_log WHERE at < ?",
                (now - int(RATE_WINDOW_SECONDS) * 2,),
            )
            self.ledger.commit()
        else:
            self._mem_calls.setdefault(resource, deque()).append(now)

    def _throttle_for_budget(self, resource: str) -> None:
        """Enforces the per-resource hourly cap before a request, using
        the shared `request_log` ledger. In `wait` mode it sleeps until
        the window frees; in `stop` mode it raises RateLimitReached."""
        while True:
            now = int(self._wall())
            since = now - int(RATE_WINDOW_SECONDS)
            if self._used_since(resource, since) < self._budget():
                return
            if self.on_cap == "stop":
                raise RateLimitReached(resource, "(hourly budget)")
            oldest = self._oldest_since(resource, since) or now
            wait = oldest + RATE_WINDOW_SECONDS + 1 - now
            if wait <= 0:
                continue
            if self.on_wait is not None:
                self.on_wait(resource, wait)
            self._sleep(wait)

    def get(self, endpoint: str, params: dict) -> dict:
        resource = resource_of(endpoint)
        self._throttle_for_budget(resource)
        query = {"api_key": self.api_key, "format": "json", **params}
        url = f"{API_BASE}/{endpoint}/?" + urllib.parse.urlencode(query)
        # A gentle self-imposed spacing avoids a burst inside the budget.
        wait = self.delay_seconds - (self._mono() - self._last_call)
        if wait > 0:
            self._sleep(wait)
        request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                body = response.read().decode("utf-8", "replace")
        except urllib.error.HTTPError as exc:
            # HTTP 429 is the transport-level throttle. Treat it as a
            # rate-limit stop for this endpoint, not a fatal error.
            if exc.code == 429:
                raise RateLimitReached(resource, "(HTTP 429)") from exc
            raise CvError(f"{endpoint}: HTTP {exc.code}") from exc
        self._last_call = self._mono()
        self._record(resource, int(self._wall()))
        try:
            data = json.loads(body)
        except ValueError as exc:
            raise CvError(f"{endpoint}: bad JSON: {exc}") from exc
        # error is the string "OK" on success (status_code 1).
        error = data.get("error")
        if error not in (None, "OK"):
            # CV signals an over-limit condition through the error field.
            # UNKNOWN: the exact throttle payload is not yet measured; the
            # documented status is "rate limit exceeded" / status_code 107.
            text = str(error).lower()
            if "rate limit" in text or data.get("status_code") == 107:
                raise RateLimitReached(resource, f"({error!r})")
            if "not found" in text or data.get("status_code") == 101:
                # A deleted or unknown id. The caller decides; a detail
                # fetch skips it rather than aborting the whole run.
                raise NotFound(f"{endpoint}: {error!r}")
            raise CvError(f"{endpoint}: API error {error!r}")
        return data

    def get_detail(self, path: str, field_list: str | None = None) -> dict | None:
        """Fetches one resource detail (a singular path like
        `issue/4000-6`). Returns the `results` object, or None when CV
        reports the id is gone (a deleted resource is skipped, not
        fatal). Budget is keyed by the singular path segment, separate
        from the list endpoints."""
        params = {}
        if field_list:
            params["field_list"] = field_list
        try:
            data = self.get(path, params)
        except NotFound:
            return None
        results = data.get("results")
        if isinstance(results, dict):
            return results
        return None


def _image_url(item: dict) -> str | None:
    image = item.get("image")
    if not isinstance(image, dict):
        return None
    for key in ("small_url", "thumb_url", "medium_url", "original_url"):
        value = image.get(key)
        if value:
            return value
    return None


def _publisher_name(volume: dict) -> str | None:
    publisher = volume.get("publisher")
    if isinstance(publisher, dict):
        return publisher.get("name")
    return None


def _to_int(value) -> int | None:
    if isinstance(value, int) and not isinstance(value, bool):
        return value
    if value is None:
        return None
    try:
        return int(str(value).strip())
    except (ValueError, TypeError):
        return None


def _stage_resource(source: sqlite3.Connection, table: str, item: dict) -> bool:
    rid = _to_int(item.get("id"))
    if rid is None:
        return False
    source.execute(
        f"INSERT OR REPLACE INTO {table} "
        "(id, name, image_url, date_last_updated, date_added, fetched_at) "
        "VALUES (?, ?, ?, ?, ?, ?)",
        (
            rid,
            item.get("name"),
            _image_url(item),
            item.get("date_last_updated"),
            item.get("date_added"),
            int(time.time()),
        ),
    )
    return True


def _stage_volume(source: sqlite3.Connection, item: dict, flt) -> bool:
    vid = _to_int(item.get("id"))
    if vid is None:
        return False
    publisher = item.get("publisher")
    publisher_id = _to_int(publisher.get("id")) if isinstance(publisher, dict) else None
    if flt is not None and getattr(flt, "in_force", False):
        if not flt.allows(publisher_id):
            return False
    source.execute(
        "INSERT OR REPLACE INTO volume "
        "(volume_id, name, publisher, start_year, count_of_issues, "
        "date_last_updated, fetched_at, aliases, description, image_url, "
        "site_detail_url) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        (
            vid,
            item.get("name"),
            _publisher_name(item),
            _to_int(item.get("start_year")),
            _to_int(item.get("count_of_issues")),
            item.get("date_last_updated"),
            int(time.time()),
            item.get("aliases"),
            item.get("description"),
            _image_url(item),
            item.get("site_detail_url"),
        ),
    )
    return True


def _stage_issue(source: sqlite3.Connection, item: dict) -> bool:
    iid = _to_int(item.get("id"))
    volume = item.get("volume")
    vid = _to_int(volume.get("id")) if isinstance(volume, dict) else None
    number = item.get("issue_number")
    if iid is None or vid is None or number is None or str(number) == "":
        # issue_number is NOT NULL in the schema; a numberless issue
        # cannot land.
        return False
    source.execute(
        "INSERT OR REPLACE INTO issue_skeleton "
        "(issue_id, volume_id, issue_number, cover_date, name, deck, "
        "description, store_date, image_url, date_added, date_last_updated, "
        "api_detail_url, site_detail_url, fetched_at) "
        "VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        (
            iid,
            vid,
            str(number),
            item.get("cover_date"),
            item.get("name"),
            item.get("deck"),
            item.get("description"),
            item.get("store_date"),
            _image_url(item),
            item.get("date_added"),
            item.get("date_last_updated"),
            item.get("api_detail_url"),
            item.get("site_detail_url"),
            int(time.time()),
        ),
    )
    return True


def _stage_item(source: sqlite3.Connection, endpoint: str, item: dict, flt) -> bool:
    if endpoint == "publishers":
        return _stage_resource(source, "publisher", item)
    if endpoint == "people":
        return _stage_resource(source, "person", item)
    if endpoint == "volumes":
        return _stage_volume(source, item, flt)
    if endpoint == "issues":
        return _stage_issue(source, item)
    raise CvError(f"unknown endpoint {endpoint}")


# The credit JSON keys of an issue detail payload and their resource
# kind. The person list carries a role; the others do not. Matches the
# localcv adapter, so a live rich fetch produces the same rows.
_CREDIT_FIELDS = (
    ("character_credits", "character", False),
    ("person_credits", "person", True),
    ("team_credits", "team", False),
    ("location_credits", "location", False),
    ("story_arc_credits", "story_arc", False),
)


def _stage_issue_detail(source: sqlite3.Connection, detail: dict) -> None:
    """Decomposes one live `/issue/<id>/` detail payload into the same
    rows the localcv import produces: credit rows (plus a resource stub
    per credited id) and issue_image rows. The skeleton scalar fields
    are refreshed too, so a fetched issue's own row stays current."""
    issue_id = _to_int(detail.get("id"))
    if issue_id is None:
        return
    now = int(time.time())
    _stage_issue(source, detail)  # refresh the skeleton row
    seen_resources: set[tuple[str, int]] = set()
    for column, kind, with_role in _CREDIT_FIELDS:
        entries = detail.get(column) or []
        if not isinstance(entries, list):
            continue
        for entry in entries:
            if not isinstance(entry, dict):
                continue
            rid = _to_int(entry.get("id"))
            name = entry.get("name")
            role = None
            if with_role:
                role = entry.get("role")
                if role is not None:
                    role = role.strip() or None
            source.execute(
                "INSERT OR IGNORE INTO credit (owner_kind, owner_id, "
                "resource_kind, resource_id, name, role, marker) "
                "VALUES ('issue', ?, ?, ?, ?, ?, 'credit')",
                (issue_id, kind, rid if rid is not None else 0, name, role),
            )
            if rid is not None and (kind, rid) not in seen_resources:
                seen_resources.add((kind, rid))
                source.execute(
                    f"INSERT OR REPLACE INTO {kind} "
                    "(id, name, image_url, date_last_updated, date_added, "
                    "fetched_at) VALUES (?, ?, NULL, NULL, NULL, ?)",
                    (rid, name, now),
                )
    for entry in detail.get("associated_images") or []:
        if not isinstance(entry, dict):
            continue
        image_id = _to_int(entry.get("id"))
        url = entry.get("original_url")
        if image_id is None or not url:
            continue
        source.execute(
            "INSERT OR REPLACE INTO issue_image (image_id, issue_id, "
            "original_url, caption, image_tags, fetched_at, ahash, dhash, "
            "phash) VALUES (?, ?, ?, ?, ?, ?, NULL, NULL, NULL)",
            (image_id, issue_id, url, entry.get("caption"),
             entry.get("image_tags"), now),
        )


@dataclass
class EndpointReport:
    endpoint: str
    fetched: int = 0
    staged: int = 0
    pages: int = 0
    complete: bool = False
    capped: bool = False
    last_sync: str | None = None


@dataclass
class UpdateReport:
    endpoints: list = field(default_factory=list)
    estimates: list = field(default_factory=list)


def _today() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%d")


def _read_watermark(
    live: sqlite3.Connection, endpoint: str, mode: str = "list"
) -> tuple[str, int]:
    """Returns (since_date, resume_offset). A first run with no row
    starts at a floor date; the caller may override with --since."""
    row = live.execute(
        "SELECT last_sync, resume_state FROM sync_state "
        "WHERE endpoint = ? AND mode = ?",
        (endpoint, mode),
    ).fetchone()
    if row is None:
        return ("1970-01-01", 0)
    since = row[0] or "1970-01-01"
    offset = 0
    if row[1]:
        try:
            offset = int(json.loads(row[1]).get("offset", 0))
        except (ValueError, TypeError):
            offset = 0
    return (since, offset)


@dataclass
class EndpointEstimate:
    endpoint: str
    since: str
    changed: int


@dataclass
class UsageRow:
    resource: str
    last_hour: int
    remaining: int
    last_request_at: int | None
    total: int


def usage(
    live: sqlite3.Connection, max_per_hour: int = MAX_PER_HOUR
) -> list[UsageRow]:
    """Reads the shared `request_log` ledger and reports, per resource,
    the requests in the last rolling hour, the remaining budget, the
    last-touched time, and the lifetime total. This is the same ledger
    the app writes, so it reflects every run (app or script)."""
    now = int(time.time())
    since = now - int(RATE_WINDOW_SECONDS)
    rows = []
    cursor = live.execute(
        "SELECT resource, "
        "SUM(CASE WHEN at >= ? THEN 1 ELSE 0 END) AS last_hour, "
        "MAX(at) AS last_at, "
        "COUNT(*) AS total "
        "FROM request_log GROUP BY resource ORDER BY resource",
        (since,),
    )
    for r in cursor:
        last_hour = int(r[1] or 0)
        rows.append(
            UsageRow(
                resource=r[0],
                last_hour=last_hour,
                remaining=max(0, max_per_hour - last_hour),
                last_request_at=int(r[2]) if r[2] is not None else None,
                total=int(r[3] or 0),
            )
        )
    return rows


def preflight(
    live: sqlite3.Connection,
    client: CvClient,
    endpoints=ENDPOINTS,
    since_override: str | None = None,
) -> list[EndpointEstimate]:
    """A cheap pre-flight: one `limit=1` request per endpoint reads the
    changed-row count since each watermark, so the caller sees the scale
    before a full run (ADR-075). `number_of_total_results` is the total
    matching the filter, independent of the page. Costs one request per
    endpoint."""
    now = _today()
    out = []
    for endpoint in endpoints:
        since, _ = _read_watermark(live, endpoint)
        if since_override is not None:
            since = since_override
        data = client.get(
            endpoint,
            {
                "field_list": "id",
                "filter": f"date_last_updated:{since}|{now}",
                "limit": 1,
                "offset": 0,
            },
        )
        changed = _to_int(data.get("number_of_total_results")) or 0
        out.append(EndpointEstimate(endpoint, since, changed))
    return out


def _write_watermark(
    live: sqlite3.Connection, endpoint: str, last_sync: str,
    resume: dict | None, mode: str = "list",
) -> None:
    resume_json = json.dumps(resume) if resume is not None else None
    live.execute(
        "INSERT INTO sync_state (endpoint, mode, last_sync, resume_state) "
        "VALUES (?, ?, ?, ?) "
        "ON CONFLICT(endpoint, mode) DO UPDATE SET "
        "last_sync = ?, resume_state = ?",
        (endpoint, mode, last_sync, resume_json, last_sync, resume_json),
    )


def update_endpoint(
    live: sqlite3.Connection,
    client: CvClient,
    endpoint: str,
    now: str,
    flt,
    max_pages: int | None,
    since_override: str | None,
    progress=None,
) -> EndpointReport:
    """Pages one endpoint over `date_last_updated:<since>|<now>`, stages
    each page into an in-memory v-current source, merges it into the
    live file, and advances the watermark. Resumable through the stored
    page offset. `progress`, if given, is called after each page and on
    a rate-limit stop with (report, total)."""
    report = EndpointReport(endpoint)
    since, offset = _read_watermark(live, endpoint)
    if since_override is not None:
        since = since_override
        offset = 0
    field_list = _FIELD_LISTS[endpoint]
    total = 0
    while True:
        if max_pages is not None and report.pages >= max_pages:
            # Stopped early: save the offset so the next run resumes.
            _write_watermark(live, endpoint, since, {"offset": offset})
            live.commit()
            report.last_sync = since
            report.capped = True
            if progress is not None:
                progress(report, total)
            return report
        try:
            data = client.get(
                endpoint,
                {
                    "filter": f"date_last_updated:{since}|{now}",
                    "field_list": field_list,
                    "sort": "date_last_updated:asc",
                    "limit": PAGE_SIZE,
                    "offset": offset,
                },
            )
        except RateLimitReached:
            # The hourly budget or an API throttle stopped this endpoint.
            # Hold `since`, save the offset, report resumable, do not crash.
            _write_watermark(live, endpoint, since, {"offset": offset})
            live.commit()
            report.last_sync = since
            report.capped = True
            if progress is not None:
                progress(report, total)
            return report
        results = data.get("results") or []
        report.pages += 1
        report.fetched += len(results)

        source = sqlite3.connect(":memory:")
        try:
            schema.create_schema(source)
            for item in results:
                if _stage_item(source, endpoint, item, flt):
                    report.staged += 1
            source.commit()
            merge.merge(live, source)
        finally:
            source.close()

        offset += len(results)
        total = _to_int(data.get("number_of_total_results")) or 0
        # A short page or reaching the total ends the window.
        if len(results) < PAGE_SIZE or offset >= total:
            # Caught up: the watermark advances to `now` and the resume
            # offset clears.
            _write_watermark(live, endpoint, now, None)
            live.commit()
            report.complete = True
            report.last_sync = now
            if progress is not None:
                progress(report, total)
            return report
        # Mid-window: hold `since`, save the offset, keep the live file
        # durable after every page.
        _write_watermark(live, endpoint, since, {"offset": offset})
        live.commit()
        if progress is not None:
            progress(report, total)


def run(
    live_path: Path,
    api_key: str,
    endpoints=ENDPOINTS,
    max_pages: int | None = None,
    since: str | None = None,
    delay_seconds: float = 1.0,
    publisher_filter=None,
    make_backup: bool = True,
    max_per_hour: int = MAX_PER_HOUR,
    on_cap: str = "wait",
    on_page=None,
    on_wait=None,
    on_endpoint_start=None,
    on_preflight=None,
    dry_run: bool = False,
) -> UpdateReport:
    report = UpdateReport()
    live = commands.open_v4(live_path)
    client = CvClient(
        api_key=api_key,
        delay_seconds=delay_seconds,
        max_per_hour=max_per_hour,
        on_cap=on_cap,
        on_wait=on_wait,
        ledger=live,
    )
    try:
        # A cheap probe first: report the changed-row count per endpoint
        # so the run shows fetched-of-total, not just fetched-of-page.
        estimates = preflight(live, client, endpoints, since)
        report.estimates = estimates
        if on_preflight is not None:
            on_preflight(estimates)
        if dry_run:
            return report
    finally:
        if dry_run:
            live.close()
    if make_backup:
        commands.backup(live_path)
    now = _today()
    try:
        for endpoint in endpoints:
            if on_endpoint_start is not None:
                on_endpoint_start(endpoint)
            report.endpoints.append(
                update_endpoint(
                    live, client, endpoint, now, publisher_filter,
                    max_pages, since, progress=on_page,
                )
            )
    finally:
        live.close()
    return report


@dataclass
class RichReport:
    fetched: int = 0
    credited: int = 0
    skipped_missing: int = 0
    stopped_capped: bool = False


def _issue_api_path(detail_url: str | None, issue_id: int) -> str:
    """The singular issue detail path CV expects, e.g. `issue/4000-6`.
    Prefers the stored api_detail_url tail; falls back to the id form."""
    if detail_url:
        # .../api/issue/4000-6/  ->  issue/4000-6
        parts = [p for p in detail_url.split("/") if p]
        if len(parts) >= 2:
            return f"{parts[-2]}/{parts[-1]}"
    return f"issue/4000-{issue_id}"


def rich_issue_backfill(
    live_path: Path,
    api_key: str,
    max_pages: int | None = None,
    delay_seconds: float = 1.0,
    max_per_hour: int = MAX_PER_HOUR,
    on_cap: str = "wait",
    make_backup: bool = True,
    on_progress=None,
    on_wait=None,
) -> RichReport:
    """Fills credits and images for issues that have a skeleton row but
    no credit rows (the post-localcv update added skeleton-only rows).
    For each such issue it fetches the live `/issue/<id>/` detail and
    decomposes it into credit + issue_image rows, matching the localcv
    shape. Resumable through the `rich_backfill` cursor (the last
    issue_id done); rate-limited through the shared request_log budget.
    `max_pages` here caps the number of issues fetched in this run."""
    report = RichReport()
    live = commands.open_v4(live_path)
    if make_backup:
        commands.backup(live_path)
    client = CvClient(
        api_key=api_key,
        delay_seconds=delay_seconds,
        max_per_hour=max_per_hour,
        on_cap=on_cap,
        on_wait=on_wait,
        ledger=live,
    )
    field_list = (
        "id,issue_number,volume,name,cover_date,deck,description,store_date,"
        "image,date_added,date_last_updated,site_detail_url,api_detail_url,"
        "character_credits,person_credits,team_credits,location_credits,"
        "story_arc_credits,associated_images"
    )
    try:
        # The cursor: the highest issue_id already backfilled. We walk
        # issue ids downward from the top, so new (high-id) issues fill
        # first; the cursor holds the lowest id reached.
        _, cursor = _read_watermark(live, "issues", "rich_backfill")
        floor = cursor if cursor else 1 << 62
        while True:
            if max_pages is not None and report.fetched >= max_pages:
                report.stopped_capped = True
                break
            row = live.execute(
                "SELECT issue_id, api_detail_url FROM issue_skeleton s "
                "WHERE issue_id < ? "
                "AND NOT EXISTS (SELECT 1 FROM credit c "
                "  WHERE c.owner_kind='issue' AND c.owner_id = s.issue_id) "
                "ORDER BY issue_id DESC LIMIT 1",
                (floor,),
            ).fetchone()
            if row is None:
                break
            issue_id, detail_url = int(row[0]), row[1]
            path = _issue_api_path(detail_url, issue_id)
            try:
                detail = client.get_detail(path, field_list)
            except RateLimitReached:
                report.stopped_capped = True
                break
            report.fetched += 1
            floor = issue_id
            if detail is None:
                report.skipped_missing += 1
            else:
                source = sqlite3.connect(":memory:")
                try:
                    schema.create_schema(source)
                    _stage_issue_detail(source, detail)
                    source.commit()
                    merge.merge(live, source)
                finally:
                    source.close()
                report.credited += 1
            # Persist the cursor after every issue so a kill resumes.
            _write_watermark(live, "issues", "", {"offset": floor},
                             "rich_backfill")
            live.commit()
            if on_progress is not None:
                on_progress(report, issue_id)
    finally:
        live.close()
    return report


# Rich resource config: local table -> (list endpoint for forward,
# detail path prefix). CV detail paths use a resource-type prefix
# (person 4040, character 4005, volume 4050); the stored resource rows
# have no api_detail_url, so we build the path from the id prefix.
_RICH_RESOURCES = {
    "person": {"list": "people", "prefix": "4040", "path": "person"},
    "character": {"list": "characters", "prefix": "4005", "path": "character"},
    "volume": {"list": "volumes", "prefix": "4050", "path": "volume"},
    "team": {"list": "teams", "prefix": "4060", "path": "team"},
    "location": {"list": "locations", "prefix": "4020", "path": "location"},
    "story_arc": {"list": "story_arcs", "prefix": "4045", "path": "story_arc"},
}


def _detail_path(cfg: dict, rid: int) -> str:
    return f"{cfg['path']}/{cfg['prefix']}-{rid}"


@dataclass
class HashReport:
    hashed: int = 0
    failed: int = 0
    stopped_capped: bool = False


def _download(url: str, timeout: int = 60) -> bytes:
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    with urllib.request.urlopen(request, timeout=timeout) as response:
        return response.read()


def hash_backfill(
    live_path: Path,
    max_images: int | None = None,
    delay_seconds: float = 0.3,
    all_images: bool = False,
    make_backup: bool = True,
    on_progress=None,
) -> HashReport:
    """Downloads issue cover images and fills the ComicTagger ahash,
    dhash, and phash on `issue_image` rows that have none. By default it
    hashes only the front cover of each issue (the lowest image_id per
    issue, which is what ComicTagger cover-matching uses); `all_images`
    hashes every gallery image. Image downloads hit the CV CDN, not the
    API, so this pass does NOT spend the API budget (MEASURED: no API
    path counter moves). It paces itself with a fixed delay and is
    resumable through the `hash_backfill` cursor (the last image_id
    done). Requires Pillow."""
    from . import imagehasher

    if not imagehasher.PIL_AVAILABLE:
        raise CvError(
            "the hashes pass needs Pillow (pip install -r scripts/requirements.txt)"
        )
    report = HashReport()
    live = commands.open_v4(live_path)
    if make_backup:
        commands.backup(live_path)
    try:
        _, cursor = _read_watermark(live, "issue_image", "hash_backfill")
        floor = cursor if cursor else 1 << 62
        # Front cover only: the lowest image_id per issue. `all_images`
        # drops that restriction.
        front_clause = (
            "" if all_images else
            "AND image_id = (SELECT MIN(i2.image_id) FROM issue_image i2 "
            "  WHERE i2.issue_id = issue_image.issue_id) "
        )
        last = 0.0
        while True:
            if max_images is not None and report.hashed + report.failed >= max_images:
                report.stopped_capped = True
                break
            row = live.execute(
                "SELECT image_id, original_url FROM issue_image "
                "WHERE image_id < ? AND ahash IS NULL "
                "AND original_url IS NOT NULL "
                + front_clause +
                "ORDER BY image_id DESC LIMIT 1",
                (floor,),
            ).fetchone()
            if row is None:
                break
            image_id, url = int(row[0]), row[1]
            floor = image_id
            wait = delay_seconds - (time.monotonic() - last)
            if wait > 0:
                time.sleep(wait)
            last = time.monotonic()
            try:
                data = _download(url)
                image = imagehasher.Image.open(io.BytesIO(data))
                ahash = str(imagehasher.average_hash(image))
                dhash = str(imagehasher.difference_hash(image))
                phash = str(imagehasher.perception_hash(image))
            except Exception:
                # A dead url or an undecodable image: skip, record, go on.
                report.failed += 1
                _write_watermark(live, "issue_image", "", {"offset": floor},
                                 "hash_backfill")
                live.commit()
                continue
            live.execute(
                "UPDATE issue_image SET ahash = ?, dhash = ?, phash = ? "
                "WHERE image_id = ?",
                (ahash, dhash, phash, image_id),
            )
            report.hashed += 1
            _write_watermark(live, "issue_image", "", {"offset": floor},
                             "hash_backfill")
            live.commit()
            if on_progress is not None:
                on_progress(report, image_id)
    finally:
        live.close()
    return report


def rich_resource_backfill(
    live_path: Path,
    api_key: str,
    resource: str,
    max_pages: int | None = None,
    delay_seconds: float = 1.0,
    max_per_hour: int = MAX_PER_HOUR,
    on_cap: str = "wait",
    make_backup: bool = True,
    on_progress=None,
    on_wait=None,
) -> RichReport:
    """Fills `detail_json` for resource rows that have none yet (person,
    character, volume). Walks ids downward, resumable through the
    `<resource>` `rich_backfill` cursor. One detail request per row,
    keyed to the singular detail path budget, shared through
    request_log."""
    cfg = _RICH_RESOURCES[resource]
    report = RichReport()
    live = commands.open_v4(live_path)
    if make_backup:
        commands.backup(live_path)
    client = CvClient(
        api_key=api_key,
        delay_seconds=delay_seconds,
        max_per_hour=max_per_hour,
        on_cap=on_cap,
        on_wait=on_wait,
        ledger=live,
    )
    id_col = "volume_id" if resource == "volume" else "id"
    try:
        _, cursor = _read_watermark(live, resource, "rich_backfill")
        floor = cursor if cursor else 1 << 62
        while True:
            if max_pages is not None and report.fetched >= max_pages:
                report.stopped_capped = True
                break
            row = live.execute(
                f"SELECT {id_col} FROM {resource} "
                f"WHERE {id_col} < ? AND detail_json IS NULL "
                f"ORDER BY {id_col} DESC LIMIT 1",
                (floor,),
            ).fetchone()
            if row is None:
                break
            rid = int(row[0])
            try:
                detail = client.get_detail(_detail_path(cfg, rid))
            except RateLimitReached:
                report.stopped_capped = True
                break
            report.fetched += 1
            floor = rid
            if detail is None:
                report.skipped_missing += 1
            else:
                live.execute(
                    f"UPDATE {resource} SET detail_json = ?, "
                    "date_last_updated = COALESCE(?, date_last_updated), "
                    f"fetched_at = ? WHERE {id_col} = ?",
                    (json.dumps(detail, separators=(",", ":")),
                     detail.get("date_last_updated"), int(time.time()), rid),
                )
                report.credited += 1
            _write_watermark(live, resource, "", {"offset": floor},
                             "rich_backfill")
            live.commit()
            if on_progress is not None:
                on_progress(report, rid)
    finally:
        live.close()
    return report


def rich_resource_forward(
    live_path: Path,
    api_key: str,
    resource: str,
    since: str | None = None,
    max_pages: int | None = None,
    delay_seconds: float = 1.0,
    max_per_hour: int = MAX_PER_HOUR,
    on_cap: str = "wait",
    make_backup: bool = True,
    on_progress=None,
    on_wait=None,
) -> RichReport:
    """Re-fetches `detail_json` for resource rows changed since the
    `rich_forward` watermark, using the list endpoint's
    `date_last_updated` filter to find them. Advances the watermark to
    today when caught up. Keeps enriched data current after the initial
    backfill."""
    cfg = _RICH_RESOURCES[resource]
    report = RichReport()
    live = commands.open_v4(live_path)
    if make_backup:
        commands.backup(live_path)
    client = CvClient(
        api_key=api_key,
        delay_seconds=delay_seconds,
        max_per_hour=max_per_hour,
        on_cap=on_cap,
        on_wait=on_wait,
        ledger=live,
    )
    id_col = "volume_id" if resource == "volume" else "id"
    now = _today()
    try:
        watermark, offset = _read_watermark(live, resource, "rich_forward")
        if since is not None:
            watermark = since
            offset = 0
        while True:
            if max_pages is not None and report.fetched >= max_pages:
                report.stopped_capped = True
                _write_watermark(live, resource, watermark, {"offset": offset},
                                 "rich_forward")
                live.commit()
                break
            try:
                page = client.get(cfg["list"], {
                    "field_list": "id",
                    "filter": f"date_last_updated:{watermark}|{now}",
                    "sort": "date_last_updated:asc",
                    "limit": PAGE_SIZE,
                    "offset": offset,
                })
            except RateLimitReached:
                report.stopped_capped = True
                _write_watermark(live, resource, watermark, {"offset": offset},
                                 "rich_forward")
                live.commit()
                break
            results = page.get("results") or []
            for item in results:
                rid = _to_int(item.get("id"))
                if rid is None:
                    continue
                have = live.execute(
                    f"SELECT 1 FROM {resource} WHERE {id_col} = ?", (rid,)
                ).fetchone()
                if have is None:
                    continue
                try:
                    detail = client.get_detail(_detail_path(cfg, rid))
                except RateLimitReached:
                    report.stopped_capped = True
                    _write_watermark(live, resource, watermark,
                                     {"offset": offset}, "rich_forward")
                    live.commit()
                    return report
                report.fetched += 1
                if detail is None:
                    report.skipped_missing += 1
                    continue
                live.execute(
                    f"UPDATE {resource} SET detail_json = ?, "
                    "date_last_updated = COALESCE(?, date_last_updated), "
                    f"fetched_at = ? WHERE {id_col} = ?",
                    (json.dumps(detail, separators=(",", ":")),
                     detail.get("date_last_updated"), int(time.time()), rid),
                )
                report.credited += 1
                if on_progress is not None:
                    on_progress(report, rid)
            offset += len(results)
            total = _to_int(page.get("number_of_total_results")) or 0
            if len(results) < PAGE_SIZE or offset >= total:
                _write_watermark(live, resource, now, None, "rich_forward")
                live.commit()
                break
            _write_watermark(live, resource, watermark, {"offset": offset},
                             "rich_forward")
            live.commit()
    finally:
        live.close()
    return report
