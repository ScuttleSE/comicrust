"""Update-pipeline staging tests (ADR-075). The network is not
exercised; the row mappers are checked against a fresh source and a
merge into a live file, plus the watermark read/write."""

import sqlite3
import unittest

from scripts.cvcache import merge, schema, update


def _fresh():
    conn = sqlite3.connect(":memory:")
    schema.create_schema(conn)
    return conn


class StageTest(unittest.TestCase):
    def test_stage_publisher_and_person(self):
        src = _fresh()
        update._stage_item(
            src, "publishers",
            {"id": 7, "name": "Press", "date_last_updated": "2026-08-02 06:33:59"},
            None,
        )
        update._stage_item(
            src, "people",
            {"id": 30, "name": "Ann", "date_last_updated": "2026-08-01 08:05:20"},
            None,
        )
        self.assertEqual(
            src.execute("SELECT id, name, date_last_updated FROM publisher").fetchone(),
            (7, "Press", "2026-08-02 06:33:59"),
        )
        self.assertEqual(
            src.execute("SELECT id, name FROM person").fetchone(), (30, "Ann")
        )

    def test_stage_volume_and_issue(self):
        src = _fresh()
        update._stage_item(
            src, "volumes",
            {"id": 10, "name": "Ten", "publisher": {"id": 7, "name": "Press"},
             "start_year": "2001", "count_of_issues": 12,
             "date_last_updated": "2026-08-01 12:36:19"},
            None,
        )
        update._stage_item(
            src, "issues",
            {"id": 100, "issue_number": "1", "volume": {"id": 10, "name": "Ten"},
             "name": "The One", "cover_date": "2001-01-01",
             "date_last_updated": "2026-08-01 14:48:15"},
            None,
        )
        self.assertEqual(
            src.execute(
                "SELECT volume_id, name, publisher, start_year, count_of_issues, "
                "date_last_updated FROM volume"
            ).fetchone(),
            (10, "Ten", "Press", 2001, 12, "2026-08-01 12:36:19"),
        )
        self.assertEqual(
            src.execute(
                "SELECT issue_id, volume_id, issue_number, date_last_updated "
                "FROM issue_skeleton"
            ).fetchone(),
            (100, 10, "1", "2026-08-01 14:48:15"),
        )

    def test_numberless_issue_and_missing_id_are_skipped(self):
        src = _fresh()
        self.assertFalse(
            update._stage_item(src, "issues",
                               {"id": 1, "volume": {"id": 10}}, None)
        )
        self.assertFalse(
            update._stage_item(src, "issues",
                               {"issue_number": "1", "volume": {"id": 10}}, None)
        )
        self.assertEqual(
            src.execute("SELECT COUNT(*) FROM issue_skeleton").fetchone()[0], 0
        )

    def test_staged_page_merges_and_stamps(self):
        # A staged page merges into a live file; the real stamp fills an
        # empty stored value.
        live = _fresh()
        live.execute(
            "INSERT INTO publisher (id, name, fetched_at) VALUES (7, 'Press', 1)"
        )
        src = _fresh()
        update._stage_item(
            src, "publishers",
            {"id": 7, "name": "Press", "date_last_updated": "2026-08-02 06:33:59"},
            None,
        )
        merge.merge(live, src)
        self.assertEqual(
            live.execute(
                "SELECT date_last_updated FROM publisher WHERE id=7"
            ).fetchone()[0],
            "2026-08-02 06:33:59",
        )


class WatermarkTest(unittest.TestCase):
    def test_read_write_roundtrip(self):
        live = _fresh()
        # No row: the floor date and offset zero.
        self.assertEqual(update._read_watermark(live, "issues"), ("1970-01-01", 0))
        update._write_watermark(live, "issues", "2026-08-03", {"offset": 200})
        self.assertEqual(update._read_watermark(live, "issues"), ("2026-08-03", 200))
        # A caught-up write clears the resume offset.
        update._write_watermark(live, "issues", "2026-09-20", None)
        self.assertEqual(update._read_watermark(live, "issues"), ("2026-09-20", 0))


class _FakeClock:
    """A wall clock the test advances by hand. sleep() jumps the clock
    forward instead of blocking, and records each sleep."""

    def __init__(self):
        self.t = 1000.0
        self.slept = []

    def now(self):
        return self.t

    def sleep(self, seconds):
        self.slept.append(seconds)
        self.t += seconds


def _client(clock, on_cap="wait", max_per_hour=10, safety_margin=0, ledger=None):
    c = update.CvClient(
        api_key="k",
        delay_seconds=0.0,
        max_per_hour=max_per_hour,
        safety_margin=safety_margin,
        on_cap=on_cap,
        ledger=ledger,
    )
    c._wall = clock.now
    c._mono = clock.now
    c._sleep = clock.sleep
    return c


class RateLimitTest(unittest.TestCase):
    def test_stop_mode_raises_at_budget(self):
        clock = _FakeClock()
        c = _client(clock, on_cap="stop", max_per_hour=3)
        for _ in range(3):
            c._mem_calls.setdefault("issues", update.deque()).append(int(clock.now()))
        with self.assertRaises(update.RateLimitReached) as ctx:
            c._throttle_for_budget("issues")
        self.assertEqual(ctx.exception.endpoint, "issues")

    def test_wait_mode_sleeps_until_window_frees(self):
        clock = _FakeClock()
        c = _client(clock, on_cap="wait", max_per_hour=2)
        c._mem_calls["publishers"] = update.deque([1000, 1000])
        c._throttle_for_budget("publishers")
        self.assertTrue(clock.slept)
        self.assertGreaterEqual(clock.slept[0], update.RATE_WINDOW_SECONDS)

    def test_budget_is_per_endpoint(self):
        clock = _FakeClock()
        c = _client(clock, on_cap="stop", max_per_hour=2)
        c._mem_calls["issues"] = update.deque([1000, 1000])
        c._throttle_for_budget("publishers")  # must not raise
        with self.assertRaises(update.RateLimitReached):
            c._throttle_for_budget("issues")

    def test_old_calls_leave_the_window(self):
        clock = _FakeClock()
        c = _client(clock, on_cap="stop", max_per_hour=1)
        c._mem_calls["people"] = update.deque([1000])
        clock.t = 1000 + update.RATE_WINDOW_SECONDS + 1
        c._throttle_for_budget("people")  # must not raise


class Http420Test(unittest.TestCase):
    def _client_with_transport(self, clock, responses):
        # responses: list of either an HTTPError code (int) to raise, or
        # a dict body (served as JSON). Consumed in order.
        c = _client(clock, on_cap="wait", max_per_hour=100)
        calls = {"i": 0}

        class _Resp:
            def __init__(self, body):
                self._body = body

            def read(self):
                return self._body.encode("utf-8")

            def __enter__(self):
                return self

            def __exit__(self, *a):
                return False

        def fake_urlopen(request, timeout=60):
            i = calls["i"]
            calls["i"] += 1
            item = responses[i]
            if isinstance(item, int):
                raise update.urllib.error.HTTPError(
                    "u", item, "throttle", {}, None)
            return _Resp(update.json.dumps(item))

        c._urlopen = fake_urlopen
        return c, calls

    def test_420_backs_off_on_the_ladder_then_succeeds(self):
        clock = _FakeClock()
        ok = {"error": "OK", "results": {"id": 1}}
        c, _ = self._client_with_transport(clock, [420, 420, ok])
        orig = update.urllib.request.urlopen
        update.urllib.request.urlopen = c._urlopen
        try:
            data = c.get("issue/4000-1", {})
        finally:
            update.urllib.request.urlopen = orig
        self.assertEqual(data["results"]["id"], 1)
        self.assertEqual(clock.slept, [3.0, 5.0])

    def test_420_past_ladder_raises_rate_limit(self):
        clock = _FakeClock()
        c, _ = self._client_with_transport(clock, [420, 420, 420, 420])
        orig = update.urllib.request.urlopen
        update.urllib.request.urlopen = c._urlopen
        try:
            with self.assertRaises(update.RateLimitReached):
                c.get("issue/4000-1", {})
        finally:
            update.urllib.request.urlopen = orig
        self.assertEqual(clock.slept, [3.0, 5.0, 10.0])

    def test_420_ladder_is_per_resource_and_resets_on_success(self):
        clock = _FakeClock()
        ok = {"error": "OK", "results": {"id": 1}}
        c, _ = self._client_with_transport(clock, [420, ok, 420, ok])
        orig = update.urllib.request.urlopen
        update.urllib.request.urlopen = c._urlopen
        try:
            c.get("issue/4000-1", {})
            c.get("issue/4000-2", {})
        finally:
            update.urllib.request.urlopen = orig
        # Each success reset the ladder, so the second 420 also slept 3s.
        self.assertEqual(clock.slept, [3.0, 3.0])

    def test_420_backoff_past_deadline_raises_without_sleeping(self):
        clock = _FakeClock()
        c, _ = self._client_with_transport(clock, [420, {"error": "OK"}])
        c.deadline = clock.now() + 1  # a 3s backoff would pass it
        orig = update.urllib.request.urlopen
        update.urllib.request.urlopen = c._urlopen
        try:
            with self.assertRaises(update.RateLimitReached):
                c.get("issue/4000-1", {})
        finally:
            update.urllib.request.urlopen = orig
        self.assertEqual(clock.slept, [])


class ResourceKeyTest(unittest.TestCase):
    def test_resource_of_first_segment_lowercased(self):
        self.assertEqual(update.resource_of("issues"), "issues")
        self.assertEqual(update.resource_of("/issue/4000-6/"), "issue")
        self.assertEqual(update.resource_of("Character/4005-1"), "character")


class SharedLedgerTest(unittest.TestCase):
    def test_two_clients_share_the_request_log(self):
        # A budget written by one client is seen by another on the same
        # db, so a forward run and a backfill run share one budget.
        live = _fresh()
        clock = _FakeClock()
        a = _client(clock, on_cap="stop", max_per_hour=3, ledger=live)
        b = _client(clock, on_cap="stop", max_per_hour=3, ledger=live)
        # Client A records three requests via the ledger.
        for _ in range(3):
            a._record("character", int(clock.now()))
        # Client B, sharing the db, now sees the full budget and stops.
        with self.assertRaises(update.RateLimitReached):
            b._throttle_for_budget("character")

    def test_usage_report_counts_from_request_log(self):
        live = _fresh()
        now = int(update.time.time())
        live.executemany(
            "INSERT INTO request_log (resource, at) VALUES (?, ?)",
            [("issue", now), ("issue", now), ("person", now),
             ("issue", now - 999999)],  # old row: total but not last hour
        )
        live.commit()
        rows = {r.resource: r for r in update.usage(live, max_per_hour=200)}
        self.assertEqual(rows["issue"].last_hour, 2)
        self.assertEqual(rows["issue"].total, 3)
        self.assertEqual(rows["issue"].remaining, 198)
        self.assertEqual(rows["person"].last_hour, 1)


class _FakeApiClient:
    """A stand-in CvClient for the loop: serves canned pages, or raises
    RateLimitReached on the configured call index."""

    def __init__(self, pages, raise_on=None):
        self._pages = pages
        self._raise_on = raise_on
        self.calls = 0

    def get(self, endpoint, params):
        idx = self.calls
        self.calls += 1
        if self._raise_on is not None and idx == self._raise_on:
            raise update.RateLimitReached(endpoint, "(test)")
        return self._pages[idx]


class LoopCapTest(unittest.TestCase):
    def test_cap_midwindow_saves_resume_offset(self):
        live = _fresh()
        # Page 0 returns a full page (100) of a larger window; the second
        # call raises the rate limit.
        page0 = {
            "results": [
                {"id": i, "name": f"p{i}",
                 "date_last_updated": "2026-08-05 00:00:00"}
                for i in range(100)
            ],
            "number_of_total_results": 500,
        }
        client = _FakeApiClient([page0], raise_on=1)
        seen = []
        report = update.update_endpoint(
            live, client, "publishers", "2026-09-20", None,
            max_pages=None, since_override="2026-08-02",
            progress=lambda r, t: seen.append((r.pages, r.capped)),
        )
        self.assertTrue(report.capped)
        self.assertFalse(report.complete)
        # The resume offset is saved at 100; the watermark holds `since`.
        self.assertEqual(
            update._read_watermark(live, "publishers"), ("2026-08-02", 100)
        )
        self.assertTrue(seen)


class PreflightTest(unittest.TestCase):
    def test_preflight_reports_changed_count_per_endpoint(self):
        live = _fresh()
        update._write_watermark(live, "publishers", "2026-08-02", None)
        totals = {"publishers": 4, "people": 126, "volumes": 144, "issues": 985}

        class _C:
            def get(self, endpoint, params):
                assert params["limit"] == 1
                assert params["field_list"] == "id"
                return {"number_of_total_results": totals[endpoint]}

        est = update.preflight(live, _C(), update.ENDPOINTS)
        self.assertEqual(
            [(e.endpoint, e.changed) for e in est],
            [("publishers", 4), ("people", 126), ("volumes", 144),
             ("issues", 985)],
        )
        self.assertEqual(est[0].since, "2026-08-02")

    def test_preflight_honors_since_override(self):
        live = _fresh()
        update._write_watermark(live, "issues", "2026-08-02", None)
        seen = {}

        class _C:
            def get(self, endpoint, params):
                seen[endpoint] = params["filter"]
                return {"number_of_total_results": 1}

        update.preflight(live, _C(), ("issues",), since_override="2026-08-20")
        self.assertIn("date_last_updated:2026-08-20|", seen["issues"])


class RichDetailTest(unittest.TestCase):
    _DETAIL = {
        "id": 6, "issue_number": "13", "volume": {"id": 1487, "name": "V"},
        "name": "The Lost Race", "cover_date": "1952-10-01",
        "date_last_updated": "2022-07-11 23:51:22",
        "person_credits": [
            {"id": 2756, "name": "Bob Powell", "role": " writer, penciler "},
        ],
        "character_credits": [{"id": 67267, "name": "Hercules"}],
        "location_credits": [{"id": 55778, "name": "France"}],
        "team_credits": [], "story_arc_credits": [],
        "associated_images": [
            {"id": 7, "original_url": "http://x/7.jpg", "caption": None,
             "image_tags": "All Images"},
        ],
    }

    def test_decompose_issue_detail_into_credits_and_images(self):
        src = _fresh()
        update._stage_issue_detail(src, self._DETAIL)
        # The skeleton row is refreshed.
        self.assertEqual(
            src.execute("SELECT issue_number, name FROM issue_skeleton "
                        "WHERE issue_id=6").fetchone(),
            ("13", "The Lost Race"),
        )
        # Credits: one per credited resource, role stripped.
        credits = src.execute(
            "SELECT resource_kind, resource_id, name, role FROM credit "
            "WHERE owner_id=6 ORDER BY resource_kind"
        ).fetchall()
        self.assertIn(("character", 67267, "Hercules", None), credits)
        self.assertIn(("location", 55778, "France", None), credits)
        self.assertIn(("person", 2756, "Bob Powell", "writer, penciler"), credits)
        # A resource stub row is seeded once.
        self.assertEqual(
            src.execute("SELECT name FROM character WHERE id=67267").fetchone()[0],
            "Hercules",
        )
        # The image gallery row lands.
        self.assertEqual(
            src.execute("SELECT issue_id, original_url FROM issue_image "
                        "WHERE image_id=7").fetchone(),
            (6, "http://x/7.jpg"),
        )

    def test_api_path_prefers_detail_url_tail(self):
        self.assertEqual(
            update._issue_api_path(
                "https://comicvine.gamespot.com/api/issue/4000-6/", 6),
            "issue/4000-6",
        )
        self.assertEqual(update._issue_api_path(None, 99), "issue/4000-99")

    def test_resource_detail_path(self):
        self.assertEqual(
            update._detail_path(update._RICH_RESOURCES["character"], 1443),
            "character/4005-1443",
        )
        self.assertEqual(
            update._detail_path(update._RICH_RESOURCES["person"], 40450),
            "person/4040-40450",
        )
        self.assertEqual(
            update._detail_path(update._RICH_RESOURCES["volume"], 85759),
            "volume/4050-85759",
        )


class DeadlineTest(unittest.TestCase):
    def test_backfill_stops_at_a_past_deadline_without_fetching(self):
        # A deadline already in the past stops the loop before any
        # request, even with rows that need enriching.
        import tempfile, os, sqlite3 as _sq
        fd, path = tempfile.mkstemp(suffix=".sqlite")
        os.close(fd)
        try:
            c = _sq.connect(path)
            schema.create_schema(c)
            c.execute("INSERT INTO character (id, name) VALUES (5, 'X')")
            c.commit()
            c.close()
            report = update.rich_resource_backfill(
                __import__("pathlib").Path(path),
                api_key="k", resource="character",
                make_backup=False, deadline=update.time.time() - 1,
            )
            self.assertEqual(report.fetched, 0)
            self.assertTrue(report.stopped_capped)
        finally:
            os.remove(path)


class UntilParseTest(unittest.TestCase):
    def test_for_minutes(self):
        from scripts.cvcache.__main__ import _parse_deadline
        import time as _t
        d = _parse_deadline(None, 30)
        self.assertAlmostEqual(d, _t.time() + 1800, delta=5)

    def test_hhmm_rolls_to_tomorrow_when_past(self):
        from scripts.cvcache.__main__ import _parse_deadline
        import datetime as _dt
        d = _parse_deadline("00:00", None)  # midnight already passed today
        self.assertGreater(d, _dt.datetime.now().timestamp())

    def test_none_is_none(self):
        from scripts.cvcache.__main__ import _parse_deadline
        self.assertIsNone(_parse_deadline(None, None))


class RemainingTest(unittest.TestCase):
    def test_report_remaining_is_total_minus_fetched(self):
        r = update.RichReport(total=10, fetched=3)
        self.assertEqual(r.remaining, 7)
        r.fetched = 12
        self.assertEqual(r.remaining, 0)  # never negative

    def test_backfill_counts_rows_needing_enrichment(self):
        import tempfile, os, sqlite3 as _sq
        fd, path = tempfile.mkstemp(suffix=".sqlite")
        os.close(fd)
        try:
            c = _sq.connect(path)
            schema.create_schema(c)
            # Two need enrichment (detail_json NULL), one already done.
            c.execute("INSERT INTO character (id, name) VALUES (1, 'A')")
            c.execute("INSERT INTO character (id, name) VALUES (2, 'B')")
            c.execute("INSERT INTO character (id, name, detail_json) "
                      "VALUES (3, 'C', '{}')")
            c.commit()
            c.close()
            # A past deadline stops before fetching, but total is counted.
            report = update.rich_resource_backfill(
                __import__("pathlib").Path(path),
                api_key="k", resource="character",
                make_backup=False, deadline=update.time.time() - 1,
            )
            self.assertEqual(report.total, 2)
            self.assertEqual(report.remaining, 2)
        finally:
            os.remove(path)


class ForwardTotalTest(unittest.TestCase):
    def test_forward_total_is_set_before_first_progress(self):
        import tempfile, os, sqlite3 as _sq
        fd, path = tempfile.mkstemp(suffix=".sqlite")
        os.close(fd)
        try:
            c = _sq.connect(path)
            schema.create_schema(c)
            c.execute("INSERT INTO person (id, name) VALUES (5, 'A')")
            c.commit()
            c.close()

            class _FakeClient:
                def __init__(self, *a, **k):
                    self.n = 0

                def get(self, endpoint, params):
                    self.n += 1
                    if params.get("limit") == 1:
                        # the up-front probe
                        return {"number_of_total_results": 3, "results": []}
                    # the page: one holdable person, short page ends it
                    return {"number_of_total_results": 3,
                            "results": [{"id": 5}]}

                def get_detail(self, path, field_list=None):
                    return {"id": 5, "real_name": "X",
                            "date_last_updated": "2026-09-19 00:00:00"}

            seen = []
            orig = update.CvClient
            update.CvClient = _FakeClient
            try:
                update.rich_resource_forward(
                    __import__("pathlib").Path(path),
                    api_key="k", resource="person", since="2026-09-19",
                    make_backup=False,
                    on_progress=lambda r, rid: seen.append(r.total),
                )
            finally:
                update.CvClient = orig
            self.assertTrue(seen, "progress should have fired")
            self.assertEqual(seen[0], 3, "total must be set before item 1")
        finally:
            os.remove(path)


class RunAllSchedulerTest(unittest.TestCase):
    def _tmpdb(self):
        import tempfile, os, sqlite3 as _sq
        fd, path = tempfile.mkstemp(suffix=".sqlite")
        os.close(fd)
        c = _sq.connect(path)
        schema.create_schema(c)
        c.commit()
        c.close()
        return path

    def test_cycles_resources_then_sleeps_until_all_done(self):
        import os
        from pathlib import Path
        path = self._tmpdb()
        # Scripted returns per resource key. Each entry is a list of
        # RichReport results served in call order.
        R = update.RichReport
        script = {
            # forward units: all immediately done (nothing to enrich).
            "story_arc-f": [R(total=0)],
            "location-f": [R(total=0)],
            "team-f": [R(total=0)],
            "person-f": [R(total=0)],
            "volume-f": [R(total=0)],
            "character-f": [R(total=0)],
            # issues backfill: capped with no room left this hour, then
            # done after the window frees.
            "issues-b": [R(total=5, fetched=0, credited=0, stopped_capped=True),
                         R(total=5, fetched=5, credited=5)],
            # resource backfills: all done first try.
            "story_arc-b": [R(total=0)],
            "location-b": [R(total=0)],
            "team-b": [R(total=0)],
            "person-b": [R(total=0)],
            "volume-b": [R(total=0)],
            "character-b": [R(total=0)],
        }
        idx = {k: 0 for k in script}
        order = []

        def _serve(key):
            i = idx[key]
            idx[key] = min(i + 1, len(script[key]) - 1)
            order.append(key)
            return script[key][i]

        orig = (update.rich_resource_forward, update.rich_issue_backfill,
                update.rich_resource_backfill, update.CvClient.next_free_at)
        clock = _FakeClock()
        update.rich_resource_forward = (
            lambda live_path, resource, **k: _serve(f"{resource}-f"))
        update.rich_issue_backfill = (
            lambda live_path, **k: _serve("issues-b"))
        update.rich_resource_backfill = (
            lambda live_path, resource, **k: _serve(f"{resource}-b"))
        # When issues is capped, report it frees 100s from now.
        update.CvClient.next_free_at = (
            lambda self, res: clock.now() + 100 if res == "issue" else None)
        try:
            rep = update.run_all(
                Path(path), api_key="k", make_backup=False,
                _sleep=clock.sleep, _wall=clock.now,
            )
        finally:
            (update.rich_resource_forward, update.rich_issue_backfill,
             update.rich_resource_backfill, update.CvClient.next_free_at) = orig
            os.remove(path)
        # It slept once, for the issue window (100s).
        self.assertEqual(clock.slept, [100.0])
        # Issues backfill ran twice (capped, then done after the sleep).
        self.assertEqual(sum(1 for k in order if k == "issues-b"), 2)
        # The combined issue total credited across cycles.
        self.assertEqual(rep.backfill["issues"].credited, 5)
        self.assertFalse(rep.reached_deadline)

    def test_a_capped_unit_is_not_rerun_before_its_window_frees(self):
        # A cycle where one resource drains while another stays capped
        # must sleep before re-running the capped unit, not spin on it
        # and burn requests rediscovering the cap.
        import os
        from pathlib import Path
        path = self._tmpdb()
        R = update.RichReport
        # All forwards done. person backfill is done on the first try;
        # character backfill is capped once, then done. So cycle 1 makes
        # progress (person) yet leaves character capped.
        script = {
            "story_arc-f": [R(total=0)], "location-f": [R(total=0)],
            "team-f": [R(total=0)], "person-f": [R(total=0)],
            "volume-f": [R(total=0)], "character-f": [R(total=0)],
            "issues-b": [R(total=0)],
            "story_arc-b": [R(total=0)], "location-b": [R(total=0)],
            "team-b": [R(total=0)],
            "person-b": [R(total=4, fetched=4, credited=4)],
            "volume-b": [R(total=0)],
            "character-b": [R(total=6, fetched=2, credited=2,
                              stopped_capped=True),
                            R(total=4, fetched=4, credited=4)],
        }
        idx = {k: 0 for k in script}
        order = []

        def _serve(key):
            i = idx[key]
            idx[key] = min(i + 1, len(script[key]) - 1)
            order.append(key)
            return script[key][i]

        orig = (update.rich_resource_forward, update.rich_issue_backfill,
                update.rich_resource_backfill, update.CvClient.next_free_at)
        clock = _FakeClock()
        update.rich_resource_forward = (
            lambda live_path, resource, **k: _serve(f"{resource}-f"))
        update.rich_issue_backfill = (
            lambda live_path, **k: _serve("issues-b"))
        update.rich_resource_backfill = (
            lambda live_path, resource, **k: _serve(f"{resource}-b"))
        update.CvClient.next_free_at = (
            lambda self, res: clock.now() + 50 if res == "character" else None)
        try:
            update.run_all(
                Path(path), api_key="k", make_backup=False,
                _sleep=clock.sleep, _wall=clock.now,
            )
        finally:
            (update.rich_resource_forward, update.rich_issue_backfill,
             update.rich_resource_backfill, update.CvClient.next_free_at) = orig
            os.remove(path)
        # It slept once (the character window) before re-running it.
        self.assertEqual(clock.slept, [50.0])
        # character backfill ran exactly twice: capped, then done after
        # the sleep — never spun mid-cycle.
        self.assertEqual(sum(1 for k in order if k == "character-b"), 2)


if __name__ == "__main__":
    unittest.main()