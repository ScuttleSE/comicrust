//! The SQLite implementation of `CvCache` (ADR-037).
//!
//! The connection sits behind a `Mutex`, because the scrape engine,
//! the sweep worker, and the warm worker all share one cache.

use std::path::Path;
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;

use super::import::{self, ImportReport};
use super::resources::{
    self, CreditMarker, CreditRef, OwnerKind, References, ResourceKind, ResourceRef,
};
use super::{
    CacheError, CreditRow, CvCache, IssueSkeleton, ManagedVolume, ResourceRow, SweepState,
    VolumeRow,
};
use crate::cv::queries::parse_image_url;

/// The schema version stored in `PRAGMA user_version`. Raise it and
/// add a migration arm when the schema changes.
const SCHEMA_VERSION: i32 = 5;

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

const SCHEMA_V2: &str = r"
ALTER TABLE volume ADD COLUMN detail_json TEXT;

CREATE TABLE IF NOT EXISTS pending_issue_detail (
    volume_id INTEGER NOT NULL,
    issue_id  INTEGER NOT NULL,
    position  INTEGER NOT NULL,
    PRIMARY KEY (volume_id, issue_id)
);
CREATE INDEX IF NOT EXISTS pending_issue_detail_order
    ON pending_issue_detail (volume_id, position);
";

/// Schema v3 (ADR-070): typed columns on `volume` and `issue_detail`,
/// one table per related resource, and the credit table. The v2→v3
/// migration backfills the typed columns from the stored JSON.
const SCHEMA_V3: &str = r"
ALTER TABLE volume ADD COLUMN aliases TEXT;
ALTER TABLE volume ADD COLUMN deck TEXT;
ALTER TABLE volume ADD COLUMN description TEXT;
ALTER TABLE volume ADD COLUMN image_url TEXT;
ALTER TABLE volume ADD COLUMN api_detail_url TEXT;
ALTER TABLE volume ADD COLUMN site_detail_url TEXT;
ALTER TABLE volume ADD COLUMN date_added TEXT;
ALTER TABLE volume ADD COLUMN first_issue_id INTEGER;
ALTER TABLE volume ADD COLUMN last_issue_id INTEGER;

ALTER TABLE issue_detail ADD COLUMN volume_id INTEGER;
ALTER TABLE issue_detail ADD COLUMN issue_number TEXT;
ALTER TABLE issue_detail ADD COLUMN cover_date TEXT;
ALTER TABLE issue_detail ADD COLUMN name TEXT;
ALTER TABLE issue_detail ADD COLUMN store_date TEXT;
ALTER TABLE issue_detail ADD COLUMN image_url TEXT;
ALTER TABLE issue_detail ADD COLUMN date_added TEXT;
ALTER TABLE issue_detail ADD COLUMN date_last_updated TEXT;

CREATE TABLE IF NOT EXISTS character (
    id                INTEGER PRIMARY KEY,
    name              TEXT,
    image_url         TEXT,
    date_last_updated TEXT,
    date_added        TEXT,
    fetched_at        INTEGER,
    detail_json       TEXT
);
CREATE TABLE IF NOT EXISTS person (
    id                INTEGER PRIMARY KEY,
    name              TEXT,
    image_url         TEXT,
    date_last_updated TEXT,
    date_added        TEXT,
    fetched_at        INTEGER,
    detail_json       TEXT
);
CREATE TABLE IF NOT EXISTS team (
    id                INTEGER PRIMARY KEY,
    name              TEXT,
    image_url         TEXT,
    date_last_updated TEXT,
    date_added        TEXT,
    fetched_at        INTEGER,
    detail_json       TEXT
);
CREATE TABLE IF NOT EXISTS story_arc (
    id                INTEGER PRIMARY KEY,
    name              TEXT,
    image_url         TEXT,
    date_last_updated TEXT,
    date_added        TEXT,
    fetched_at        INTEGER,
    detail_json       TEXT
);
CREATE TABLE IF NOT EXISTS location (
    id                INTEGER PRIMARY KEY,
    name              TEXT,
    image_url         TEXT,
    date_last_updated TEXT,
    date_added        TEXT,
    fetched_at        INTEGER,
    detail_json       TEXT
);
CREATE TABLE IF NOT EXISTS concept (
    id                INTEGER PRIMARY KEY,
    name              TEXT,
    image_url         TEXT,
    date_last_updated TEXT,
    date_added        TEXT,
    fetched_at        INTEGER,
    detail_json       TEXT
);
CREATE TABLE IF NOT EXISTS object (
    id                INTEGER PRIMARY KEY,
    name              TEXT,
    image_url         TEXT,
    date_last_updated TEXT,
    date_added        TEXT,
    fetched_at        INTEGER,
    detail_json       TEXT
);
CREATE TABLE IF NOT EXISTS publisher (
    id                INTEGER PRIMARY KEY,
    name              TEXT,
    image_url         TEXT,
    date_last_updated TEXT,
    date_added        TEXT,
    fetched_at        INTEGER,
    detail_json       TEXT
);

CREATE TABLE IF NOT EXISTS credit (
    owner_kind    TEXT NOT NULL,
    owner_id      INTEGER NOT NULL,
    resource_kind TEXT NOT NULL,
    resource_id   INTEGER NOT NULL,
    name          TEXT,
    role          TEXT,
    marker        TEXT NOT NULL
);
-- The natural key of ADR-070 with COALESCE on both nullable name
-- parts, so a re-import can never duplicate a row.
CREATE UNIQUE INDEX IF NOT EXISTS credit_natural
    ON credit (owner_kind, owner_id, resource_kind, resource_id,
               COALESCE(name, ''), COALESCE(role, ''), marker);
CREATE INDEX IF NOT EXISTS credit_owner
    ON credit (owner_kind, owner_id);
";

/// Schema v4 (ADR-072): the sweep's list-level fields land on the
/// skeleton. The rolling build may already have written v3 files, so
/// the columns ride a new version instead of editing v3 in place.
const SCHEMA_V4: &str = r"
ALTER TABLE issue_skeleton ADD COLUMN deck TEXT;
ALTER TABLE issue_skeleton ADD COLUMN description TEXT;
ALTER TABLE issue_skeleton ADD COLUMN store_date TEXT;
ALTER TABLE issue_skeleton ADD COLUMN image_url TEXT;
ALTER TABLE issue_skeleton ADD COLUMN date_added TEXT;
ALTER TABLE issue_skeleton ADD COLUMN date_last_updated TEXT;
ALTER TABLE issue_skeleton ADD COLUMN api_detail_url TEXT;
ALTER TABLE issue_skeleton ADD COLUMN site_detail_url TEXT;
ALTER TABLE issue_skeleton ADD COLUMN fetched_at INTEGER NOT NULL DEFAULT 0;
";

/// Schema v5 (ADR-073): the per-issue image gallery. The Comic Vine
/// issue detail carries `associated_images`; each entry is keyed on
/// its own globally unique image id. URLs only; `image_blob` stays the
/// on-demand byte store.
const SCHEMA_V5: &str = r"
CREATE TABLE IF NOT EXISTS issue_image (
    image_id     INTEGER PRIMARY KEY,
    issue_id     INTEGER NOT NULL,
    original_url TEXT NOT NULL,
    caption      TEXT,
    image_tags   TEXT,
    fetched_at   INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS issue_image_issue
    ON issue_image (issue_id);
";

fn db(e: rusqlite::Error) -> CacheError {
    CacheError::Db(e.to_string())
}

/// The migration chain over one connection, shared by the live open
/// and the import's temp copy (ADR-069). A version newer than the
/// build's schema is the caller's business; this chain only moves up.
pub(crate) fn migrate_connection(conn: &Connection) -> Result<(), CacheError> {
    let version: i32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .map_err(db)?;
    if version < 1 {
        conn.execute_batch(SCHEMA_V1).map_err(db)?;
    }
    if version < 2 {
        conn.execute_batch(SCHEMA_V2).map_err(db)?;
    }
    if version < 3 {
        conn.execute_batch(SCHEMA_V3).map_err(db)?;
        backfill_v3(conn)?;
    }
    if version < 4 {
        conn.execute_batch(SCHEMA_V4).map_err(db)?;
    }
    if version < 5 {
        conn.execute_batch(SCHEMA_V5).map_err(db)?;
    }
    if version != SCHEMA_VERSION {
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(db)?;
    }
    Ok(())
}

/// Fills the v3 typed columns from the stored detail JSON (the
/// v2→v3 backfill of ADR-070). The raw JSON text never moves or
/// rewrites. A row whose JSON does not parse keeps NULL columns
/// and does not fail the migration.
fn backfill_v3(conn: &Connection) -> Result<(), CacheError> {
    let volume_rows: Vec<(i64, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT volume_id, detail_json FROM volume
                  WHERE detail_json IS NOT NULL",
            )
            .map_err(db)?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(db)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db)?
    };
    {
        let mut stmt = conn
            .prepare(
                "UPDATE volume SET
                    aliases = ?2, deck = ?3, description = ?4,
                    image_url = ?5, api_detail_url = ?6,
                    site_detail_url = ?7, date_added = ?8,
                    first_issue_id = ?9, last_issue_id = ?10
                  WHERE volume_id = ?1",
            )
            .map_err(db)?;
        for (volume_id, json) in &volume_rows {
            let Some(columns) = volume_detail_columns(json) else {
                continue;
            };
            stmt.execute(params![
                volume_id,
                columns.aliases,
                columns.deck,
                columns.description,
                columns.image_url,
                columns.api_detail_url,
                columns.site_detail_url,
                columns.date_added,
                columns.first_issue_id,
                columns.last_issue_id,
            ])
            .map_err(db)?;
        }
    }

    let issue_rows: Vec<(i64, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT issue_id, json FROM issue_detail
                  WHERE json IS NOT NULL",
            )
            .map_err(db)?;
        let rows = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(db)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db)?
    };
    {
        let mut stmt = conn
            .prepare(
                "UPDATE issue_detail SET
                    volume_id = ?2, issue_number = ?3, cover_date = ?4,
                    name = ?5, store_date = ?6, image_url = ?7,
                    date_added = ?8, date_last_updated = ?9
                  WHERE issue_id = ?1",
            )
            .map_err(db)?;
        for (issue_id, json) in &issue_rows {
            let Some(columns) = issue_detail_columns(json) else {
                continue;
            };
            stmt.execute(params![
                issue_id,
                columns.volume_id,
                columns.issue_number,
                columns.cover_date,
                columns.name,
                columns.store_date,
                columns.image_url,
                columns.date_added,
                columns.date_last_updated,
            ])
            .map_err(db)?;
        }
    }
    Ok(())
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
        migrate_connection(&conn)
    }

    /// A poisoned cache mutex must not stop a scrape, so the guard is
    /// taken back from the poison.
    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Writes one complete, standalone snapshot of the cache to
    /// `path` while the live connection stays open (ADR-069). The
    /// snapshot carries no `-wal`/`-shm` companions, and the target
    /// must not exist.
    pub fn backup(&self, path: &Path) -> Result<(), CacheError> {
        if path.exists() {
            return Err(CacheError::Path(format!(
                "{}: the backup file already exists",
                path.display()
            )));
        }
        let target = path.to_string_lossy().to_string();
        self.lock()
            .execute("VACUUM INTO ?1", params![target])
            .map(|_| ())
            .map_err(db)
    }

    /// Checkpoints the WAL, so a plain file copy of the cache is
    /// complete afterwards (ADR-069).
    pub fn checkpoint(&self) -> Result<(), CacheError> {
        let conn = self.lock();
        conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(2)?))
        })
        .map_err(db)?;
        Ok(())
    }

    /// Imports a cache file (ADR-069): the file copies to a temp
    /// path, the copy migrates through the normal chain, and the
    /// newer-stamp merge applies. The live file stays untouched until
    /// the merge succeeds. A file whose schema is newer than this
    /// build is rejected.
    pub fn import(&self, path: &Path) -> Result<ImportReport, CacheError> {
        if !path.is_file() {
            return Err(CacheError::Import(format!(
                "{} is not a file",
                path.display()
            )));
        }
        let temp = std::env::temp_dir().join(format!(
            "comicrust-cache-import-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::copy(path, &temp).map_err(|e| CacheError::Import(format!("copy: {e}")))?;
        let result = self.import_from_temp(&temp);
        let _ = std::fs::remove_file(&temp);
        result
    }

    fn import_from_temp(&self, temp: &Path) -> Result<ImportReport, CacheError> {
        let source = Connection::open(temp).map_err(db)?;
        let version: i32 = source
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(db)?;
        if version > SCHEMA_VERSION {
            return Err(CacheError::Import(format!(
                "the file uses cache schema {version}, newer than this build's schema {SCHEMA_VERSION}"
            )));
        }
        // An older file migrates inside the temp copy first.
        migrate_connection(&source)?;
        let mut conn = self.lock();
        let tx = conn.transaction().map_err(db)?;
        let report = import::merge(&tx, &source)?;
        tx.commit().map_err(db)?;
        Ok(report)
    }
    /// Reads one cache-manager record. An unknown id returns `None`.
    pub fn managed_volume(&self, volume_id: i64) -> Result<Option<ManagedVolume>, CacheError> {
        let Some(volume) = <Self as CvCache>::volume(self, volume_id)? else {
            return Ok(None);
        };
        let detail_json = self
            .lock()
            .query_row(
                "SELECT detail_json FROM volume WHERE volume_id = ?1",
                params![volume_id],
                |row| row.get(0),
            )
            .map_err(db)?;
        let issues = <Self as CvCache>::issues_of_volume(self, volume_id)?;
        let pending_issue_details = self.pending_issue_details(volume_id)?;
        Ok(Some(ManagedVolume {
            volume,
            detail_json,
            issues,
            pending_issue_details,
        }))
    }

    /// Writes the three user-editable fields exactly. Empty values clear them.
    pub fn update_volume_metadata(
        &self,
        volume_id: i64,
        name: Option<&str>,
        publisher: Option<&str>,
        start_year: Option<i32>,
    ) -> Result<(), CacheError> {
        self.lock()
            .execute(
                "INSERT INTO volume (volume_id, name, publisher, start_year)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(volume_id) DO UPDATE SET
                   name = excluded.name,
                   publisher = excluded.publisher,
                   start_year = excluded.start_year",
                params![volume_id, name, publisher, start_year],
            )
            .map(|_| ())
            .map_err(db)
    }

    /// Sets the cover-date summary exactly after a complete detail pass.
    pub fn set_volume_last_cover_date(
        &self,
        volume_id: i64,
        last_cover_date: Option<&str>,
    ) -> Result<(), CacheError> {
        self.lock()
            .execute(
                "UPDATE volume SET last_cover_date = ?2 WHERE volume_id = ?1",
                params![volume_id, last_cover_date],
            )
            .map(|_| ())
            .map_err(db)
    }

    /// Atomically replaces one volume's API metadata and issue membership.
    /// Existing detail JSON for issue ids remains in `issue_detail`.
    pub fn replace_volume_snapshot(
        &self,
        volume: &VolumeRow,
        detail_json: &str,
        issues: &[IssueSkeleton],
    ) -> Result<(), CacheError> {
        let mut conn = self.lock();
        let tx = conn.transaction().map_err(db)?;
        tx.execute(
            "INSERT INTO volume
               (volume_id, name, publisher, start_year, count_of_issues,
                date_last_updated, last_cover_date, fetched_at, detail_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(volume_id) DO UPDATE SET
               name = excluded.name,
               publisher = excluded.publisher,
               start_year = excluded.start_year,
               count_of_issues = excluded.count_of_issues,
               date_last_updated = excluded.date_last_updated,
               last_cover_date = excluded.last_cover_date,
               fetched_at = excluded.fetched_at,
               detail_json = excluded.detail_json",
            params![
                volume.volume_id,
                volume.name,
                volume.publisher,
                volume.start_year,
                volume.count_of_issues,
                volume.date_last_updated,
                volume.last_cover_date,
                volume.fetched_at,
                detail_json,
            ],
        )
        .map_err(db)?;
        tx.execute(
            "DELETE FROM issue_skeleton WHERE volume_id = ?1",
            params![volume.volume_id],
        )
        .map_err(db)?;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO issue_skeleton
                       (issue_id, volume_id, issue_number, cover_date, name)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(issue_id) DO UPDATE SET
                       volume_id = excluded.volume_id,
                       issue_number = excluded.issue_number,
                       cover_date = excluded.cover_date,
                       name = excluded.name",
                )
                .map_err(db)?;
            for issue in issues {
                stmt.execute(params![
                    issue.issue_id,
                    issue.volume_id,
                    issue.issue_number,
                    issue.cover_date,
                    issue.name,
                ])
                .map_err(db)?;
            }
        }
        // The volume detail response carries its own credit lists and
        // its inline publisher (ADR-070). Zero extra requests.
        if let Some(references) = resources::extract_volume_references(detail_json) {
            store_references_tx(&tx, OwnerKind::Volume, volume.volume_id, &references)?;
        }
        tx.commit().map_err(db)
    }

    /// Replaces the durable queue for one volume.
    pub fn set_pending_issue_details(
        &self,
        volume_id: i64,
        issue_ids: &[i64],
    ) -> Result<(), CacheError> {
        let mut conn = self.lock();
        let tx = conn.transaction().map_err(db)?;
        tx.execute(
            "DELETE FROM pending_issue_detail WHERE volume_id = ?1",
            params![volume_id],
        )
        .map_err(db)?;
        {
            let mut stmt = tx
                .prepare(
                    "INSERT INTO pending_issue_detail (volume_id, issue_id, position)
                     VALUES (?1, ?2, ?3)",
                )
                .map_err(db)?;
            for (position, issue_id) in issue_ids.iter().enumerate() {
                stmt.execute(params![volume_id, issue_id, position as i64])
                    .map_err(db)?;
            }
        }
        tx.commit().map_err(db)
    }

    /// The unfinished issue-detail ids, in refresh order.
    pub fn pending_issue_details(&self, volume_id: i64) -> Result<Vec<i64>, CacheError> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare(
                "SELECT issue_id FROM pending_issue_detail
                 WHERE volume_id = ?1 ORDER BY position",
            )
            .map_err(db)?;
        let rows = stmt
            .query_map(params![volume_id], |row| row.get(0))
            .map_err(db)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(db)
    }

    /// Stores one complete issue and removes it from the durable queue.
    pub fn complete_issue_detail(
        &self,
        issue: &IssueSkeleton,
        json: &str,
    ) -> Result<(), CacheError> {
        let mut conn = self.lock();
        let tx = conn.transaction().map_err(db)?;
        tx.execute(
            "INSERT INTO issue_detail
               (issue_id, json, fetched_at, volume_id, issue_number,
                cover_date, name)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(issue_id) DO UPDATE SET
               json = excluded.json, fetched_at = excluded.fetched_at,
               volume_id = excluded.volume_id,
               issue_number = excluded.issue_number,
               cover_date = excluded.cover_date,
               name = excluded.name",
            params![
                issue.issue_id,
                json,
                now(),
                issue.volume_id,
                issue.issue_number,
                issue.cover_date,
                issue.name,
            ],
        )
        .map_err(db)?;
        tx.execute(
            "UPDATE issue_skeleton SET
               issue_number = ?2, cover_date = ?3, name = ?4
             WHERE issue_id = ?1",
            params![
                issue.issue_id,
                issue.issue_number,
                issue.cover_date,
                issue.name,
            ],
        )
        .map_err(db)?;
        tx.execute(
            "DELETE FROM pending_issue_detail
             WHERE volume_id = ?1 AND issue_id = ?2",
            params![issue.volume_id, issue.issue_id],
        )
        .map_err(db)?;
        // The issue detail response carries its credit lists (ADR-070).
        // Zero extra requests.
        if let Some(references) = resources::extract_issue_references(json) {
            store_references_tx(&tx, OwnerKind::Issue, issue.issue_id, &references)?;
        }
        tx.commit().map_err(db)
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
                       (issue_id, volume_id, issue_number, cover_date, name,
                        deck, description, store_date, image_url, date_added,
                        date_last_updated, api_detail_url, site_detail_url, fetched_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)
                     ON CONFLICT(issue_id) DO UPDATE SET
                       volume_id    = excluded.volume_id,
                       issue_number = excluded.issue_number,
                       cover_date   = COALESCE(excluded.cover_date, cover_date),
                       name         = COALESCE(excluded.name, name),
                       deck         = COALESCE(excluded.deck, deck),
                       description  = COALESCE(excluded.description, description),
                       store_date   = COALESCE(excluded.store_date, store_date),
                       image_url    = COALESCE(excluded.image_url, image_url),
                       date_added   = COALESCE(excluded.date_added, date_added),
                       date_last_updated = COALESCE(excluded.date_last_updated, date_last_updated),
                       api_detail_url = COALESCE(excluded.api_detail_url, api_detail_url),
                       site_detail_url = COALESCE(excluded.site_detail_url, site_detail_url),
                       fetched_at   = MAX(excluded.fetched_at, fetched_at)",
                )
                .map_err(db)?;
            for i in issues {
                stmt.execute(params![
                    i.issue_id,
                    i.volume_id,
                    i.issue_number,
                    i.cover_date,
                    i.name,
                    i.deck,
                    i.description,
                    i.store_date,
                    i.image_url,
                    i.date_added,
                    i.date_last_updated,
                    i.api_detail_url,
                    i.site_detail_url,
                    i.fetched_at,
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
                "SELECT issue_id, volume_id, issue_number, cover_date, name,
                        deck, description, store_date, image_url, date_added,
                        date_last_updated, api_detail_url, site_detail_url, fetched_at
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
                    deck: r.get(5)?,
                    description: r.get(6)?,
                    store_date: r.get(7)?,
                    image_url: r.get(8)?,
                    date_added: r.get(9)?,
                    date_last_updated: r.get(10)?,
                    api_detail_url: r.get(11)?,
                    site_detail_url: r.get(12)?,
                    fetched_at: r.get(13)?,
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
        // The typed columns and the inline references extract from
        // the same response at zero extra cost (ADR-070). The merge
        // rule stays store-wide: an empty value never erases.
        let columns = issue_detail_columns(json).unwrap_or_default();
        let mut conn = self.lock();
        let tx = conn.transaction().map_err(db)?;
        tx.execute(
            "INSERT INTO issue_detail
               (issue_id, json, fetched_at, volume_id, issue_number,
                cover_date, name, store_date, image_url, date_added,
                date_last_updated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(issue_id) DO UPDATE SET
               json = excluded.json, fetched_at = excluded.fetched_at,
               volume_id = COALESCE(excluded.volume_id, volume_id),
               issue_number = COALESCE(excluded.issue_number, issue_number),
               cover_date = COALESCE(excluded.cover_date, cover_date),
               name = COALESCE(excluded.name, name),
               store_date = COALESCE(excluded.store_date, store_date),
               image_url = COALESCE(excluded.image_url, image_url),
               date_added = COALESCE(excluded.date_added, date_added),
               date_last_updated = COALESCE(excluded.date_last_updated, date_last_updated)",
            params![
                issue_id,
                json,
                now(),
                columns.volume_id,
                columns.issue_number,
                columns.cover_date,
                columns.name,
                columns.store_date,
                columns.image_url,
                columns.date_added,
                columns.date_last_updated,
            ],
        )
        .map_err(db)?;
        if let Some(references) = resources::extract_issue_references(json) {
            store_references_tx(&tx, OwnerKind::Issue, issue_id, &references)?;
        }
        store_issue_images_tx(&tx, issue_id, &resources::extract_issue_images(json))?;
        tx.commit().map_err(db)
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

    fn put_resources(&self, resources: &[ResourceRef]) -> Result<(), CacheError> {
        let mut conn = self.lock();
        let tx = conn.transaction().map_err(db)?;
        let fetched_at = now();
        for resource in resources {
            upsert_resource_tx(&tx, resource, fetched_at)?;
        }
        tx.commit().map_err(db)
    }

    fn put_credits(
        &self,
        owner: OwnerKind,
        owner_id: i64,
        credits: &[CreditRef],
    ) -> Result<(), CacheError> {
        let mut conn = self.lock();
        let tx = conn.transaction().map_err(db)?;
        replace_credits_tx(&tx, owner, owner_id, credits)?;
        tx.commit().map_err(db)
    }

    fn credits_of(&self, owner: OwnerKind, owner_id: i64) -> Result<Vec<CreditRow>, CacheError> {
        let conn = self.lock();
        let mut stmt = conn
            .prepare(
                "SELECT resource_kind, resource_id, name, role, marker
                   FROM credit WHERE owner_kind = ?1 AND owner_id = ?2
                  ORDER BY marker, resource_kind, resource_id, name, role",
            )
            .map_err(db)?;
        let rows = stmt
            .query_map(params![owner.as_str(), owner_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .map_err(db)?;
        let mut out = Vec::new();
        for row in rows {
            let (kind, resource_id, name, role, marker) = row.map_err(db)?;
            let kind = parse_resource_kind(&kind)
                .ok_or_else(|| CacheError::Db(format!("unknown resource kind {kind}")))?;
            let marker = parse_credit_marker(&marker)
                .ok_or_else(|| CacheError::Db(format!("unknown credit marker {marker}")))?;
            out.push(CreditRow {
                owner_kind: owner,
                owner_id,
                kind,
                resource_id,
                name,
                role,
                marker,
            });
        }
        Ok(out)
    }

    fn resource(&self, kind: ResourceKind, id: i64) -> Result<Option<ResourceRow>, CacheError> {
        let conn = self.lock();
        let table = kind.as_str();
        conn.query_row(
            &format!(
                "SELECT id, name, image_url, date_last_updated, date_added,
                        fetched_at, detail_json
                   FROM {table} WHERE id = ?1"
            ),
            params![id],
            |r| {
                Ok(ResourceRow {
                    kind,
                    id: r.get(0)?,
                    name: r.get(1)?,
                    image_url: r.get(2)?,
                    date_last_updated: r.get(3)?,
                    date_added: r.get(4)?,
                    fetched_at: r.get(5)?,
                    detail_json: r.get(6)?,
                })
            },
        )
        .optional()
        .map_err(db)
    }

    fn put_resource_detail(&self, row: &ResourceRow) -> Result<(), CacheError> {
        let table = row.kind.as_str();
        self.lock()
            .execute(
                &format!(
                    "INSERT INTO {table} (id, name, image_url, date_last_updated,
                        date_added, fetched_at, detail_json)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT(id) DO UPDATE SET
                       name = COALESCE(excluded.name, name),
                       image_url = COALESCE(excluded.image_url, image_url),
                       date_last_updated = COALESCE(excluded.date_last_updated, date_last_updated),
                       date_added = COALESCE(excluded.date_added, date_added),
                       fetched_at = MAX(excluded.fetched_at, fetched_at),
                       detail_json = COALESCE(excluded.detail_json, detail_json)"
                ),
                params![
                    row.id,
                    row.name,
                    row.image_url,
                    row.date_last_updated,
                    row.date_added,
                    row.fetched_at,
                    row.detail_json,
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

// --- inline reference storage (ADR-070) ---
//
// The helpers take a connection so the write paths can run them
// inside their own transaction. The table names come from the closed
// `ResourceKind` enum, never from data.

fn upsert_resource_tx(
    conn: &Connection,
    resource: &ResourceRef,
    fetched_at: i64,
) -> Result<(), CacheError> {
    let table = resource.kind.as_str();
    conn.execute(
        &format!(
            "INSERT INTO {table} (id, name, fetched_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET
               name = COALESCE(excluded.name, name),
               fetched_at = MAX(excluded.fetched_at, fetched_at)"
        ),
        params![resource.id, resource.name, fetched_at],
    )
    .map(|_| ())
    .map_err(db)
}

fn replace_credits_tx(
    conn: &Connection,
    owner: OwnerKind,
    owner_id: i64,
    credits: &[CreditRef],
) -> Result<(), CacheError> {
    conn.execute(
        "DELETE FROM credit WHERE owner_kind = ?1 AND owner_id = ?2",
        params![owner.as_str(), owner_id],
    )
    .map(|_| ())
    .map_err(db)?;
    let mut stmt = conn
        .prepare(
            "INSERT INTO credit
               (owner_kind, owner_id, resource_kind, resource_id, name, role, marker)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .map_err(db)?;
    for credit in credits {
        stmt.execute(params![
            owner.as_str(),
            owner_id,
            credit.kind.as_str(),
            credit.resource_id.unwrap_or(0),
            credit.name,
            credit.role,
            credit.marker.as_str(),
        ])
        .map_err(db)?;
    }
    Ok(())
}

/// Upserts the identified resources and replaces the credit rows of
/// one owner, all on the caller's transaction.
fn store_references_tx(
    conn: &Connection,
    owner: OwnerKind,
    owner_id: i64,
    references: &References,
) -> Result<(), CacheError> {
    let fetched_at = now();
    for resource in &references.resources {
        upsert_resource_tx(conn, resource, fetched_at)?;
    }
    replace_credits_tx(conn, owner, owner_id, &references.credits)
}

/// Upserts an issue's `associated_images` gallery (ADR-073). The image
/// id is the primary key; a re-scrape refreshes the URL and keeps the
/// newer `fetched_at`. An empty gallery leaves the stored rows.
fn store_issue_images_tx(
    conn: &Connection,
    issue_id: i64,
    images: &[resources::IssueImage],
) -> Result<(), CacheError> {
    if images.is_empty() {
        return Ok(());
    }
    let fetched_at = now();
    let mut stmt = conn
        .prepare(
            "INSERT INTO issue_image
               (image_id, issue_id, original_url, caption, image_tags, fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(image_id) DO UPDATE SET
               issue_id = excluded.issue_id,
               original_url = excluded.original_url,
               caption = COALESCE(excluded.caption, caption),
               image_tags = COALESCE(excluded.image_tags, image_tags),
               fetched_at = MAX(excluded.fetched_at, fetched_at)",
        )
        .map_err(db)?;
    for image in images {
        stmt.execute(params![
            image.image_id,
            issue_id,
            image.original_url,
            image.caption,
            image.image_tags,
            fetched_at,
        ])
        .map_err(db)?;
    }
    Ok(())
}

fn parse_resource_kind(name: &str) -> Option<ResourceKind> {
    match name {
        "character" => Some(ResourceKind::Character),
        "person" => Some(ResourceKind::Person),
        "team" => Some(ResourceKind::Team),
        "story_arc" => Some(ResourceKind::StoryArc),
        "location" => Some(ResourceKind::Location),
        "concept" => Some(ResourceKind::Concept),
        "object" => Some(ResourceKind::Object),
        "publisher" => Some(ResourceKind::Publisher),
        _ => None,
    }
}

fn parse_credit_marker(name: &str) -> Option<CreditMarker> {
    match name {
        "credit" => Some(CreditMarker::Credit),
        "first_appearance" => Some(CreditMarker::FirstAppearance),
        "died_in" => Some(CreditMarker::DiedIn),
        "disbanded" => Some(CreditMarker::Disbanded),
        _ => None,
    }
}

// --- typed-column extraction (ADR-070) ---
//
// Both stored JSON shapes are the serialized `results` object of one
// API response (ADR-064). The extraction never rewrites the stored
// text; the write paths and the v2→v3 backfill share these helpers.

/// The typed `volume` columns of ADR-070.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct VolumeDetailColumns {
    pub aliases: Option<String>,
    pub deck: Option<String>,
    pub description: Option<String>,
    pub image_url: Option<String>,
    pub api_detail_url: Option<String>,
    pub site_detail_url: Option<String>,
    pub date_added: Option<String>,
    pub first_issue_id: Option<i64>,
    pub last_issue_id: Option<i64>,
}

/// Extracts the typed columns of one volume from its detail JSON.
/// `None` when the text does not parse to an object. `aliases` is
/// stored verbatim: a string stays a string and an array serializes
/// back. The real API shape is UNKNOWN (the docs page does not state
/// it); verbatim is lossless either way.
pub(crate) fn volume_detail_columns(json: &str) -> Option<VolumeDetailColumns> {
    let value = serde_json::from_str::<Value>(json).ok()?;
    Some(VolumeDetailColumns {
        aliases: aliases_text(value.get("aliases")),
        deck: string_value(value.get("deck")),
        description: string_value(value.get("description")),
        image_url: parse_image_url(&value),
        api_detail_url: string_value(value.get("api_detail_url")),
        site_detail_url: string_value(value.get("site_detail_url")),
        date_added: string_value(value.get("date_added")),
        first_issue_id: value_i64(value.pointer("/first_issue/id")),
        last_issue_id: value_i64(value.pointer("/last_issue/id")),
    })
}

/// The typed `issue_detail` columns of ADR-070.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct IssueDetailColumns {
    pub volume_id: Option<i64>,
    pub issue_number: Option<String>,
    pub cover_date: Option<String>,
    pub name: Option<String>,
    pub store_date: Option<String>,
    pub image_url: Option<String>,
    pub date_added: Option<String>,
    pub date_last_updated: Option<String>,
}

/// Extracts the typed columns of one issue from its detail JSON.
/// `None` when the text does not parse to an object. `issue_number`
/// is trimmed, like every live extraction path.
pub(crate) fn issue_detail_columns(json: &str) -> Option<IssueDetailColumns> {
    let value = serde_json::from_str::<Value>(json).ok()?;
    Some(IssueDetailColumns {
        volume_id: value_i64(value.pointer("/volume/id")),
        issue_number: value
            .get("issue_number")
            .and_then(Value::as_str)
            .map(str::trim)
            .map(str::to_string),
        cover_date: string_value(value.get("cover_date")),
        name: string_value(value.get("name")),
        store_date: string_value(value.get("store_date")),
        image_url: parse_image_url(&value),
        date_added: string_value(value.get("date_added")),
        date_last_updated: string_value(value.get("date_last_updated")),
    })
}

fn aliases_text(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(text)) => Some(text.clone()),
        Some(Value::Array(items)) => serde_json::to_string(items).ok(),
        _ => None,
    }
}

fn string_value(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_string)
}

/// The C# dom values are strings; the JSON API returns numbers for
/// ids. Both parse (the same union behavior as `queries.rs`).
fn value_i64(value: Option<&Value>) -> Option<i64> {
    match value {
        Some(Value::Number(number)) => number.as_i64(),
        Some(Value::String(text)) => text.trim().parse().ok(),
        _ => None,
    }
}
