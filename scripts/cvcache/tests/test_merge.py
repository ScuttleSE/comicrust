"""Merge-engine tests, mirroring the rules of import.rs."""

import sqlite3
import unittest

from scripts.cvcache import merge, schema


def _fresh():
    conn = sqlite3.connect(":memory:")
    schema.create_schema(conn)
    return conn


class MergeRuleTest(unittest.TestCase):
    def test_parse_api_date(self):
        self.assertIsNotNone(merge.parse_api_date("2026-07-28 04:47:13"))
        self.assertIsNotNone(merge.parse_api_date("2026-07-28"))
        self.assertIsNone(merge.parse_api_date(""))
        self.assertIsNone(merge.parse_api_date(None))

    def test_newer_by_api_date(self):
        self.assertTrue(
            merge.incoming_is_newer("2026-01-02", 0, "2026-01-01", 999)
        )
        self.assertFalse(
            merge.incoming_is_newer("2026-01-01", 999, "2026-01-01", 0)
        )

    def test_newer_falls_back_to_fetched_at(self):
        self.assertTrue(merge.incoming_is_newer(None, 10, None, 5))
        self.assertFalse(merge.incoming_is_newer(None, 5, None, 5))  # tie keeps stored

    def test_empty_incoming_never_erases(self):
        self.assertEqual(merge.merge_text("stored", None, True), "stored")
        self.assertEqual(merge.merge_text("stored", "", True), "stored")

    def test_empty_stored_takes_incoming(self):
        self.assertEqual(merge.merge_text(None, "in", False), "in")
        self.assertEqual(merge.merge_text("", "in", False), "in")

    def test_two_values_take_base(self):
        self.assertEqual(merge.merge_text("s", "i", True), "i")
        self.assertEqual(merge.merge_text("s", "i", False), "s")


class MergeVolumeTest(unittest.TestCase):
    def test_newer_incoming_wins_but_empty_never_erases(self):
        live = _fresh()
        source = _fresh()
        live.execute(
            "INSERT INTO volume (volume_id, name, publisher, date_last_updated, "
            "fetched_at) VALUES (1, 'Old', 'Marvel', '2026-01-01', 100)"
        )
        source.execute(
            "INSERT INTO volume (volume_id, name, publisher, date_last_updated, "
            "fetched_at) VALUES (1, 'New', NULL, '2026-06-01', 50)"
        )
        report = merge.merge(live, source)
        row = live.execute(
            "SELECT name, publisher FROM volume WHERE volume_id = 1"
        ).fetchone()
        # newer incoming -> name updates; empty publisher never erases.
        self.assertEqual(row, ("New", "Marvel"))
        vol = next(t for t in report.tables if t.name == "volume")
        self.assertEqual(vol.updated, 1)

    def test_add_and_skip(self):
        live = _fresh()
        source = _fresh()
        source.execute(
            "INSERT INTO volume (volume_id, name, fetched_at) VALUES (2, 'X', 10)"
        )
        first = merge.merge(live, source)
        self.assertEqual(next(t for t in first.tables if t.name == "volume").added, 1)
        # Re-merge the same source: a tie keeps the stored row -> skipped.
        second = merge.merge(live, source)
        self.assertEqual(
            next(t for t in second.tables if t.name == "volume").skipped, 1
        )


class MergeCreditTest(unittest.TestCase):
    def test_credit_union_is_idempotent(self):
        live = _fresh()
        source = _fresh()
        source.execute(
            "INSERT INTO credit (owner_kind, owner_id, resource_kind, resource_id, "
            "name, role, marker) VALUES ('issue', 1, 'person', 5, 'Stan', 'writer', "
            "'credit')"
        )
        merge.merge(live, source)
        report = merge.merge(live, source)
        credit = next(t for t in report.tables if t.name == "credit")
        self.assertEqual(credit.skipped, 1)
        self.assertEqual(
            live.execute("SELECT COUNT(*) FROM credit").fetchone()[0], 1
        )


if __name__ == "__main__":
    unittest.main()
