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


if __name__ == "__main__":
    unittest.main()
