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
    """A monotonic clock the test advances by hand. sleep() jumps the
    clock forward instead of blocking, and records each sleep."""

    def __init__(self):
        self.t = 1000.0
        self.slept = []

    def now(self):
        return self.t

    def sleep(self, seconds):
        self.slept.append(seconds)
        self.t += seconds


def _client(clock, on_cap="wait", max_per_hour=10, safety_margin=0):
    c = update.CvClient(
        api_key="k",
        delay_seconds=0.0,
        max_per_hour=max_per_hour,
        safety_margin=safety_margin,
        on_cap=on_cap,
    )
    c._now = clock.now
    c._sleep = clock.sleep
    return c


class RateLimitTest(unittest.TestCase):
    def test_stop_mode_raises_at_budget(self):
        clock = _FakeClock()
        c = _client(clock, on_cap="stop", max_per_hour=3)
        # Fill the budget by recording three timestamps directly.
        for _ in range(3):
            c._calls.setdefault("issues", update.deque()).append(clock.now())
        with self.assertRaises(update.RateLimitReached) as ctx:
            c._throttle_for_budget("issues")
        self.assertEqual(ctx.exception.endpoint, "issues")

    def test_wait_mode_sleeps_until_window_frees(self):
        clock = _FakeClock()
        c = _client(clock, on_cap="wait", max_per_hour=2)
        # Two calls at t=1000 fill the budget of 2.
        c._calls["publishers"] = update.deque([1000.0, 1000.0])
        c._throttle_for_budget("publishers")
        # It must have slept about one full window past the oldest call.
        self.assertTrue(clock.slept)
        self.assertGreaterEqual(clock.slept[0], update.RATE_WINDOW_SECONDS)

    def test_budget_is_per_endpoint(self):
        clock = _FakeClock()
        c = _client(clock, on_cap="stop", max_per_hour=2)
        c._calls["issues"] = update.deque([1000.0, 1000.0])
        # issues is full, but publishers has its own empty budget.
        c._throttle_for_budget("publishers")  # must not raise
        with self.assertRaises(update.RateLimitReached):
            c._throttle_for_budget("issues")

    def test_old_calls_leave_the_window(self):
        clock = _FakeClock()
        c = _client(clock, on_cap="stop", max_per_hour=1)
        c._calls["people"] = update.deque([1000.0])
        # Advance past the window; the stale call is pruned, budget frees.
        clock.t = 1000.0 + update.RATE_WINDOW_SECONDS + 1
        c._throttle_for_budget("people")  # must not raise


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


if __name__ == "__main__":
    unittest.main()
