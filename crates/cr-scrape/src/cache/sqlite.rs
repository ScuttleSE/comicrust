//! The SQLite implementation of `CvCache` (ADR-037).
//!
//! The connection sits behind a `Mutex`, because the scrape engine,
//! the sweep worker, and the warm worker all share one cache.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};

use super::{CacheError, CvCache, IssueSkeleton, SweepState, VolumeRow};

/// The schema version stored in `PRAGMA user_version`. Raise it and
/// add a migration arm when the schema changes.
const SCHEMA_VERSION: i32 = 1;

const SCHEMA_V1: &str = r"
CREATE TABLE IF NOT EXISTS volume (
    volume_id         INTEGER PRIMARY KEY,
    name              TEXT,
    publisher         TEXT,
    start_year        INTEGER,
    count_of_issues   INTEGER,
    date_last_updated TEXT,
    last_cover_date   TEXT,
    fetched_at        INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS issue_skeleton (
    issue_id     INTEGER PRIMARY KEY,
    volume_id    INTEGER NOT NULL,
    issue_number TEXT NOT NULL,
    cover_date   TEXT,
    name         TEXT
);
CREATE INDEX IF NOT EXISTS issue_skeleton_volume
    ON issue_skeleton (volume_id, issue_id);

CREATE TABLE IF NOT EXISTS issue_detail (
    issue_id   INTEGER PRIMARY KEY,
    json       TEXT NOT NULL,
    fetched_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS image_blob (
    url        TEXT PRIMARY KEY,
    bytes      BLOB NOT NULL,
    fetched_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS search_result (
    terms      TEXT PRIMARY KEY,
    json       TEXT NOT NULL,
    fetched_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS request_log (
    id       INTEGER PRIMARY KEY AUTOINCREMENT,
    resource TEXT NOT NULL,
    at       INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS request_log_resource
    ON request_log (resource, at);

CREATE TABLE IF NOT EXISTS sweep_state (
    id         INTEGER PRIMARY KEY CHECK (id = 1),
    start_date TEXT NOT NULL,
    end_date   TEXT NOT NULL,
    offset     INTEGER NOT NULL,
    total      INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
";

fn db(e: rusqlite::Error) -> CacheError {
    CacheError::Db(e.to_string())
}

/// The Comic Vine cache on disk.
pub struct SqliteCache {
    conn: Mutex<Connection>,
}

impl SqliteCache {
    /// Opens (and creates) the cache file. The parent directory is
    /// created when it is absent.
    pub fn open(path: &Path) -> Result<Self, CacheError> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| CacheError::Path(format!("{}: {e}", dir.display())))?;
        }
        let conn = Connection::open(path).map_err(db)?;
        Self::from_connection(conn)
    }

    /// A private database for the tests. It holds the real schema, so
    /// the tests exercise the real SQL.
    pub fn in_memory() -> Result<Self, CacheError> {
        Self::from_connection(Connection::open_in_memory().map_err(db)?)
    }

    fn from_connection(conn: Connection) -> Result<Self, CacheError> {
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(db)?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(db)?;
        let cache = SqliteCache {
            conn: Mutex::new(conn),
        };
        cache.migrate()?;
        Ok(cache)
    }

    fn migrate(&self) -> Result<(), CacheError> {
        let conn = self.lock();
        let version: i32 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(db)?;
        if version < 1 {
            conn.execute_batch(SCHEMA_V1).map_err(db)?;
        }
        if version != SCHEMA_VERSION {
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)
                .map_err(db)?;
        }
        Ok(())
    }

    /// A poisoned cache mutex must not stop a scrape, so the guard is
    /// taken back from the poison.
    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl CvCache for SqliteCache {
    fn put_volumes(&self, volumes: &[VolumeRow]) -> Result<(), CacheError> {
        let mut conn = self.lock();
        let tx = conn.transaction().map_err(db)?;
        {
            // COALESCE keeps a stored value when the new row has no
            // value for that field (the merge rule in `CvCache`).
            let mut stmt = tx
                .prepare(
                    "INSERT INTO volume
                       (volume_id, name, publisher, start_year, count_of_issues,
                        date_last_updated, last_cover_date, fetched_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                     ON CONFLICT(volume_id) DO UPDATE SET
                       name              = COALESCE(excluded.name, name),
                       publisher         = COALESCE(excluded.publisher, publisher),
                       start_year        = COALESCE(excluded.start_year, start_year),
                       count_of_issues   = COALESCE(excluded.count_of_issues, count_of_issues),
                       date_last_updated = COALESCE(excluded.date_last_updated, date_last_updated),
                       last_cover_date   = COALESCE(excluded.last_cover_date, last_cover_date),
                       fetched_at        = MAX(excluded.fetched_at, fetched_at)",
                )
                .map_err(db)?;
            for v in volumes {
                stmt.execute(params![
                    v.volume_id,
                    v.name,
                    v.publisher,
                    v.start_year,
                    v.count_of_issues,
                    v.date_last_updated,
                    v.last_cover_date,
                    v.fetched_at,
                ])
                .map_err(db)?;
            }
        }
        tx.commit().map_err(db)
    }

    fn volume(&self, volume_id: i64) -> Result<Option<VolumeRow>, CacheError> {
        self.lock()
            .query_row(
                "SELECT volume_id, name, publisher, start_year, count_of_issues,
                        date_last_updated, last_cover_date, fetched_at
                   FROM volume WHERE volume_id = ?1",
                params![volume_id],
                |r| {
                    Ok(VolumeRow {
                        volume_id: r.get(0)?,
                        name: r.get(1)?,
                        publisher: r.get(2)?,
                        start_year: r.get(3)?,
                        count_of_issues: r.get(4)?,
                        date_last_updated: r.get(5)?,
                        last_cover_date: r.get(6)?,
                        fetched_at: r.get(7)?,
                    })
                },
            )
            .optional()
            .map_err(db)
    }

    fn put_issues(&self, issues: &[IssueSkeleton]) -> Result<(), CacheError> {
        let mut conn = self.lock();
        let tx = conn.transaction().map_err(db)?;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO issue_skeleton
                       (issue_id, volume_id, issue_number, cover_date, name)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(issue_id) DO UPDATE SET
                       volume_id    = excluded.volume_id,
                       issue_number = excluded.issue_number,
                       cover_date   = COALESCE(excluded.cover_date, cover_date),
                       name         = COALESCE(excluded.name, name)",
                )
                .map_err(db)?;
            for i in issues {
                stmt.execute(params![
                    i.issue_id,
                    i.volume_id,
                    i.issue_number,
                    i.cover_date,
                    i.name,
                ])
                .map_err(db)?;
            }
        }
        tx.commit().map_err(db)
    }

    fn issues_of_volume(&self, volume_id: i64) -> Result<Vec<IssueSkeleton>, CacheError> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare(
                "SELECT issue_id, volume_id, issue_number, cover_date, name
                   FROM issue_skeleton WHERE volume_id = ?1 ORDER BY issue_id",
            )
            .map_err(db)?;
        let rows = stmt
            .query_map(params![volume_id], |r| {
                Ok(IssueSkeleton {
                    issue_id: r.get(0)?,
                    volume_id: r.get(1)?,
                    issue_number: r.get(2)?,
                    cover_date: r.get(3)?,
                    name: r.get(4)?,
                })
            })
            .map_err(db)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db)
    }

    fn issue_count(&self, volume_id: i64) -> Result<i64, CacheError> {
        self.lock()
            .query_row(
                "SELECT COUNT(*) FROM issue_skeleton WHERE volume_id = ?1",
                params![volume_id],
                |r| r.get(0),
            )
            .map_err(db)
    }

    fn put_issue_detail(&self, issue_id: i64, json: &str) -> Result<(), CacheError> {
        self.lock()
            .execute(
                "INSERT INTO issue_detail (issue_id, json, fetched_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(issue_id) DO UPDATE SET
                   json = excluded.json, fetched_at = excluded.fetched_at",
                params![issue_id, json, now()],
            )
            .map(|_| ())
            .map_err(db)
    }

    fn issue_detail(&self, issue_id: i64) -> Result<Option<(String, i64)>, CacheError> {
        self.lock()
            .query_row(
                "SELECT json, fetched_at FROM issue_detail WHERE issue_id = ?1",
                params![issue_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db)
    }

    fn put_image(&self, url: &str, bytes: &[u8]) -> Result<(), CacheError> {
        self.lock()
            .execute(
                "INSERT INTO image_blob (url, bytes, fetched_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(url) DO UPDATE SET
                   bytes = excluded.bytes, fetched_at = excluded.fetched_at",
                params![url, bytes, now()],
            )
            .map(|_| ())
            .map_err(db)
    }

    fn image(&self, url: &str) -> Result<Option<Vec<u8>>, CacheError> {
        self.lock()
            .query_row(
                "SELECT bytes FROM image_blob WHERE url = ?1",
                params![url],
                |r| r.get(0),
            )
            .optional()
            .map_err(db)
    }

    fn put_search(&self, terms: &str, json: &str) -> Result<(), CacheError> {
        self.lock()
            .execute(
                "INSERT INTO search_result (terms, json, fetched_at)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(terms) DO UPDATE SET
                   json = excluded.json, fetched_at = excluded.fetched_at",
                params![terms, json, now()],
            )
            .map(|_| ())
            .map_err(db)
    }

    fn search(&self, terms: &str) -> Result<Option<(String, i64)>, CacheError> {
        self.lock()
            .query_row(
                "SELECT json, fetched_at FROM search_result WHERE terms = ?1",
                params![terms],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(db)
    }

    fn log_request(&self, resource: &str, at: i64) -> Result<(), CacheError> {
        self.lock()
            .execute(
                "INSERT INTO request_log (resource, at) VALUES (?1, ?2)",
                params![resource, at],
            )
            .map(|_| ())
            .map_err(db)
    }

    fn requests_since(&self, resource: &str, since: i64) -> Result<i64, CacheError> {
        self.lock()
            .query_row(
                "SELECT COUNT(*) FROM request_log WHERE resource = ?1 AND at >= ?2",
                params![resource, since],
                |r| r.get(0),
            )
            .map_err(db)
    }

    fn oldest_request_since(&self, resource: &str, since: i64) -> Result<Option<i64>, CacheError> {
        self.lock()
            .query_row(
                "SELECT MIN(at) FROM request_log WHERE resource = ?1 AND at >= ?2",
                params![resource, since],
                |r| r.get::<_, Option<i64>>(0),
            )
            .optional()
            .map(Option::flatten)
            .map_err(db)
    }

    fn prune_requests(&self, before: i64) -> Result<(), CacheError> {
        self.lock()
            .execute("DELETE FROM request_log WHERE at < ?1", params![before])
            .map(|_| ())
            .map_err(db)
    }

    fn sweep_state(&self) -> Result<Option<SweepState>, CacheError> {
        self.lock()
            .query_row(
                "SELECT start_date, end_date, offset, total, updated_at
                   FROM sweep_state WHERE id = 1",
                [],
                |r| {
                    Ok(SweepState {
                        start_date: r.get(0)?,
                        end_date: r.get(1)?,
                        offset: r.get(2)?,
                        total: r.get(3)?,
                        updated_at: r.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(db)
    }

    fn put_sweep_state(&self, state: &SweepState) -> Result<(), CacheError> {
        self.lock()
            .execute(
                "INSERT INTO sweep_state (id, start_date, end_date, offset, total, updated_at)
                 VALUES (1, ?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(id) DO UPDATE SET
                   start_date = excluded.start_date,
                   end_date   = excluded.end_date,
                   offset     = excluded.offset,
                   total      = excluded.total,
                   updated_at = excluded.updated_at",
                params![
                    state.start_date,
                    state.end_date,
                    state.offset,
                    state.total,
                    state.updated_at
                ],
            )
            .map(|_| ())
            .map_err(db)
    }
}

/// The current time in unix seconds.
fn now() -> i64 {
    chrono::Utc::now().timestamp()
}
