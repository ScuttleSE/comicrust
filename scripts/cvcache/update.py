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
    safety_margin: int = SAFETY_MARGIN
    on_cap: str = "wait"  # "wait" (sleep until the window frees) or "stop"
    _last_call: float = 0.0
    # A rolling one-hour deque of request timestamps per endpoint path.
    _calls: dict = field(default_factory=dict)
    # Injectable for tests; real code uses the wall/monotonic clock.
    _now: object = time.monotonic
    _sleep: object = time.sleep
    on_wait: object = None  # optional callback(endpoint, seconds) before a sleep

    def _budget(self) -> int:
        return max(1, self.max_per_hour - self.safety_margin)

    def _prune(self, endpoint: str, now: float) -> deque:
        calls = self._calls.setdefault(endpoint, deque())
        while calls and now - calls[0] >= RATE_WINDOW_SECONDS:
            calls.popleft()
        return calls

    def _throttle_for_budget(self, endpoint: str) -> None:
        """Enforces the per-endpoint hourly cap before a request. In
        `wait` mode it sleeps until the oldest call in the window ages
        out; in `stop` mode it raises RateLimitReached."""
        while True:
            now = self._now()
            calls = self._prune(endpoint, now)
            if len(calls) < self._budget():
                return
            if self.on_cap == "stop":
                raise RateLimitReached(endpoint, "(hourly budget)")
            # wait: sleep until the oldest call leaves the window.
            wait = RATE_WINDOW_SECONDS - (now - calls[0]) + 0.1
            if self.on_wait is not None:
                self.on_wait(endpoint, wait)
            self._sleep(wait)

    def get(self, endpoint: str, params: dict) -> dict:
        self._throttle_for_budget(endpoint)
        query = {"api_key": self.api_key, "format": "json", **params}
        url = f"{API_BASE}/{endpoint}/?" + urllib.parse.urlencode(query)
        # A gentle self-imposed spacing avoids a burst inside the budget.
        wait = self.delay_seconds - (self._now() - self._last_call)
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
                raise RateLimitReached(endpoint, "(HTTP 429)") from exc
            raise CvError(f"{endpoint}: HTTP {exc.code}") from exc
        now = self._now()
        self._last_call = now
        self._calls.setdefault(endpoint, deque()).append(now)
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
                raise RateLimitReached(endpoint, f"({error!r})")
            raise CvError(f"{endpoint}: API error {error!r}")
        return data


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


def _today() -> str:
    return datetime.now(timezone.utc).strftime("%Y-%m-%d")


def _read_watermark(live: sqlite3.Connection, endpoint: str) -> tuple[str, int]:
    """Returns (since_date, resume_offset). A first run with no row
    starts at a floor date; the caller may override with --since."""
    row = live.execute(
        "SELECT last_sync, resume_state FROM sync_state WHERE endpoint = ?",
        (endpoint,),
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


def _write_watermark(
    live: sqlite3.Connection, endpoint: str, last_sync: str, resume: dict | None
) -> None:
    resume_json = json.dumps(resume) if resume is not None else None
    live.execute(
        "INSERT INTO sync_state (endpoint, last_sync, resume_state) "
        "VALUES (?, ?, ?) "
        "ON CONFLICT(endpoint) DO UPDATE SET last_sync = ?, resume_state = ?",
        (endpoint, last_sync, resume_json, last_sync, resume_json),
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
) -> UpdateReport:
    if make_backup:
        commands.backup(live_path)
    client = CvClient(
        api_key=api_key,
        delay_seconds=delay_seconds,
        max_per_hour=max_per_hour,
        on_cap=on_cap,
        on_wait=on_wait,
    )
    now = _today()
    report = UpdateReport()
    live = commands.open_v4(live_path)
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
