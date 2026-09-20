"""localcv adapter, publisher filter, and MCL reader tests."""

import json
import sqlite3
import tempfile
import unittest
from pathlib import Path

from scripts.cvcache import commands, mcl
from scripts.cvcache.adapters.localcv import LocalCvAdapter
from scripts.cvcache.publishers import PublisherFilter


def _make_localcv(path: Path):
    c = sqlite3.connect(path)
    c.executescript(
        """
        CREATE TABLE cv_publisher(id INTEGER PRIMARY KEY, name TEXT,
            image_url TEXT, site_detail_url TEXT, country TEXT);
        CREATE TABLE cv_person(id INTEGER PRIMARY KEY, name TEXT);
        CREATE TABLE cv_volume(id INTEGER PRIMARY KEY, name TEXT, aliases TEXT,
            start_year TEXT, publisher_id INTEGER, count_of_issues INTEGER,
            description TEXT, image_url TEXT, site_detail_url TEXT);
        CREATE TABLE cv_issue(id INTEGER PRIMARY KEY, volume_id INTEGER, name TEXT,
            issue_number TEXT, cover_date TEXT, store_date TEXT, description TEXT,
            image_url TEXT, site_detail_url TEXT, character_credits TEXT,
            person_credits TEXT, team_credits TEXT, location_credits TEXT,
            story_arc_credits TEXT, associated_images TEXT);
        CREATE TABLE cv_issue_last_seen(issue_id INTEGER PRIMARY KEY,
            date_last_updated TEXT);
        """
    )
    c.execute("INSERT INTO cv_publisher VALUES (31,'Marvel','i','s','US')")
    c.execute("INSERT INTO cv_publisher VALUES (99,'Block',NULL,NULL,NULL)")
    c.execute("INSERT INTO cv_person VALUES (5,'Stan Lee')")
    c.execute(
        "INSERT INTO cv_volume VALUES (100,'Amazing','','1963',31,10,'d','vi','vs')"
    )
    c.execute("INSERT INTO cv_volume VALUES (200,'Blocked','','2000',99,3,'x',NULL,NULL)")
    chars = json.dumps([{"id": 67267, "name": "Hercules"}])
    people = json.dumps([{"id": 5, "name": "Stan Lee", "role": "writer "}])
    c.execute(
        "INSERT INTO cv_issue VALUES (1000,100,'Origin','1','1963-03-01','','d',"
        "'ii','is',?,?,'[]','[]','[]','[{\"id\":7}]')",
        (chars, people),
    )
    c.execute(
        "INSERT INTO cv_issue VALUES (1001,100,NULL,'',NULL,NULL,NULL,NULL,NULL,"
        "'[]','[]','[]','[]','[]',NULL)"
    )
    c.execute(
        "INSERT INTO cv_issue VALUES (1002,200,'x','1',NULL,NULL,NULL,NULL,NULL,"
        "'[]','[]','[]','[]','[]',NULL)"
    )
    c.execute("INSERT INTO cv_issue_last_seen VALUES (1000,'2026-07-28 04:47:13')")
    c.commit()
    c.close()


class LocalCvImportTest(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.src = self.tmp / "localcv.db"
        _make_localcv(self.src)
        self.live = self.tmp / "cvcache.sqlite"
        commands.open_v4(self.live, create=True).close()

    def _import(self, flt=None):
        ad = LocalCvAdapter(
            db_path=self.src, fetched_at=1_000_000_000, publisher_filter=flt
        )
        outcome, report, _ = commands.import_adapter(
            ad, self.live, make_backup=False
        )
        return ad, outcome, report

    def test_whitelist_and_credit_mapping(self):
        ad, outcome, _ = self._import(PublisherFilter(whitelist={31}))
        self.assertEqual(outcome.rejected, 0)
        v = sqlite3.connect(self.live)
        self.assertEqual(
            v.execute("SELECT volume_id, publisher FROM volume").fetchall(),
            [(100, "Marvel")],
        )
        # numberless issue dropped, blocked-publisher issue filtered out.
        self.assertEqual(
            v.execute("SELECT issue_id FROM issue_skeleton").fetchall(), [(1000,)]
        )
        self.assertEqual(ad.dropped.get("issue_no_number"), 1)
        # last_seen stamp landed on the issue.
        self.assertEqual(
            v.execute(
                "SELECT date_last_updated FROM issue_skeleton WHERE issue_id=1000"
            ).fetchone()[0],
            "2026-07-28 04:47:13",
        )
        # credit role trimmed; resource tables seeded.
        self.assertEqual(
            v.execute(
                "SELECT role FROM credit WHERE resource_kind='person'"
            ).fetchone()[0],
            "writer",
        )
        self.assertEqual(v.execute("SELECT id FROM character").fetchall(), [(67267,)])
        v.close()

    def test_reimport_is_idempotent(self):
        self._import(PublisherFilter(whitelist={31}))
        _, _, report = self._import(PublisherFilter(whitelist={31}))
        added = sum(t.added for t in report.tables)
        updated = sum(t.updated for t in report.tables)
        self.assertEqual((added, updated), (0, 0))

    def test_blacklist(self):
        self._import(PublisherFilter(blacklist={31}))
        v = sqlite3.connect(self.live)
        ids = {r[0] for r in v.execute("SELECT volume_id FROM volume")}
        self.assertNotIn(100, ids)
        self.assertIn(200, ids)
        v.close()


class PublisherFilterTest(unittest.TestCase):
    def test_whitelist_then_blacklist(self):
        flt = PublisherFilter(whitelist={1, 2, 3}, blacklist={2})
        self.assertTrue(flt.allows(1))
        self.assertFalse(flt.allows(2))
        self.assertFalse(flt.allows(9))

    def test_load_ids_skips_comments(self):
        tmp = Path(tempfile.mkdtemp()) / "list.txt"
        tmp.write_text("# ID, Name\n31, Marvel\n10, DC Comics\n\nnot a row\n")
        from scripts.cvcache.publishers import load_publisher_ids

        self.assertEqual(load_publisher_ids(tmp), {31, 10})


class MclReaderTest(unittest.TestCase):
    def test_space_comma_and_escape(self):
        import io

        text = "Missing;2026-08-26\n900;10,11;v. 1, no. 01,x.&@1y,\n"
        volumes = [v for v, _ in mcl.read(io.StringIO(text)) if v is not None]
        self.assertEqual(len(volumes), 1)
        numbers = [i.issue_number for i in volumes[0].issues]
        self.assertEqual(numbers, ["v. 1, no. 01", "x,y"])


if __name__ == "__main__":
    unittest.main()
