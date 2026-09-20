//! Gates for the cache backup and import (ADR-069, Phase 21 T5).

use std::sync::atomic::{AtomicU32, Ordering};

use cr_scrape::cache::{CvCache, SqliteCache, VolumeRow};

/// A unique scratch directory for one test.
fn tempdir(tag: &str) -> std::path::PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let mut p = std::env::temp_dir();
    p.push(format!(
        "cr-scrape-import-{tag}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::remove_dir_all(&p).ok();
    std::fs::create_dir_all(&p).expect("temp dir");
    p
}

/// Builds a source cache file at `dir/source.sqlite` and hands the raw
/// connection to `setup` before the file closes. A real open first
/// creates the current schema; the raw inserts then set the exact
/// rows (and stamp overrides) the test needs.
fn source_file(
    dir: &std::path::Path,
    setup: impl FnOnce(&rusqlite::Connection),
) -> std::path::PathBuf {
    let path = dir.join("source.sqlite");
    drop(SqliteCache::open(&path).expect("create the source schema"));
    let conn = rusqlite::Connection::open(&path).expect("open source file");
    setup(&conn);
    path
}

fn live_cache(dir: &std::path::Path) -> SqliteCache {
    SqliteCache::open(&dir.join("live.sqlite")).expect("open live cache")
}

fn report_of<'a>(
    report: &'a cr_scrape::cache::import::ImportReport,
    name: &str,
) -> &'a cr_scrape::cache::import::TableReport {
    report
        .tables
        .iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("no report for {name}"))
}

#[test]
fn a_newer_volume_wins_field_by_field() {
    let dir = tempdir("newer-wins");
    let live = live_cache(&dir);
    live.put_volumes(&[VolumeRow {
        volume_id: 771,
        name: Some("Old Name".into()),
        publisher: Some("Old Publisher".into()),
        date_last_updated: Some("2024-01-01 00:00:00".into()),
        fetched_at: 100,
        ..Default::default()
    }])
    .expect("seed live volume");
    let source = source_file(&dir, |conn| {
        conn.execute_batch(
            "INSERT INTO volume (volume_id, name, count_of_issues, date_last_updated, fetched_at, aliases)
             VALUES (771, 'New Name', 6, '2025-01-01 00:00:00', 50, 'Alias');",
        )
        .expect("seed source volume");
    });

    let report = live.import(&source).expect("import");

    let v = live.volume(771).expect("read").expect("present");
    // Both non-empty: the newer (incoming) row is the base.
    assert_eq!(v.name.as_deref(), Some("New Name"));
    // An empty incoming value never erases.
    assert_eq!(v.publisher.as_deref(), Some("Old Publisher"));
    // An empty stored value takes the incoming value.
    assert_eq!(v.count_of_issues, Some(6));
    // The stamp fields take the base row's own stamp: the incoming
    // row is the base, stamp and all.
    assert_eq!(v.date_last_updated.as_deref(), Some("2025-01-01 00:00:00"));
    assert_eq!(v.fetched_at, 50);
    let (aliases,): (Option<String>,) = rusqlite::Connection::open(dir.join("live.sqlite"))
        .and_then(|c| {
            c.query_row(
                "SELECT aliases FROM volume WHERE volume_id = 771",
                [],
                |r| Ok((r.get(0)?,)),
            )
        })
        .expect("read column");
    assert_eq!(aliases.as_deref(), Some("Alias"));
    let volume = report_of(&report, "volume");
    assert_eq!((volume.added, volume.updated, volume.skipped), (0, 1, 0));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn an_older_import_never_erases_and_skips() {
    let dir = tempdir("older");
    let live = live_cache(&dir);
    live.put_volumes(&[VolumeRow {
        volume_id: 771,
        name: Some("Kept".into()),
        count_of_issues: Some(6),
        date_last_updated: Some("2025-01-01 00:00:00".into()),
        fetched_at: 200,
        ..Default::default()
    }])
    .expect("seed");
    let source = source_file(&dir, |conn| {
        conn.execute_batch(
            "INSERT INTO volume (volume_id, name, date_last_updated, fetched_at)
             VALUES (771, 'Older', '2020-01-01 00:00:00', 50);",
        )
        .expect("seed source");
    });

    let report = live.import(&source).expect("import");

    let v = live.volume(771).expect("read").expect("present");
    assert_eq!(v.name.as_deref(), Some("Kept"));
    assert_eq!(v.count_of_issues, Some(6));
    let volume = report_of(&report, "volume");
    assert_eq!(volume.skipped, 1);
    assert_eq!(volume.updated, 0);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_tie_keeps_the_stored_row() {
    let dir = tempdir("tie");
    let live = live_cache(&dir);
    live.put_volumes(&[VolumeRow {
        volume_id: 771,
        name: Some("Stored".into()),
        date_last_updated: Some("2025-01-01 00:00:00".into()),
        fetched_at: 200,
        ..Default::default()
    }])
    .expect("seed");
    let source = source_file(&dir, |conn| {
        conn.execute_batch(
            "INSERT INTO volume (volume_id, name, date_last_updated, fetched_at)
             VALUES (771, 'Incoming', '2025-01-01 00:00:00', 900);",
        )
        .expect("seed source");
    });

    let report = live.import(&source).expect("import");

    let v = live.volume(771).expect("read").expect("present");
    assert_eq!(v.name.as_deref(), Some("Stored"));
    let volume = report_of(&report, "volume");
    assert_eq!(volume.skipped, 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_stamps_fall_back_to_fetched_at_when_a_date_is_missing() {
    let dir = tempdir("stamp-fallback");
    let live = live_cache(&dir);
    live.put_volumes(&[VolumeRow {
        volume_id: 771,
        name: Some("Stored".into()),
        date_last_updated: None,
        fetched_at: 100,
        ..Default::default()
    }])
    .expect("seed");
    let source = source_file(&dir, |conn| {
        // No date_last_updated either: fetched_at alone decides.
        conn.execute_batch(
            "INSERT INTO volume (volume_id, name, fetched_at) VALUES (771, 'Newer', 200);",
        )
        .expect("seed source");
    });

    live.import(&source).expect("import");

    let v = live.volume(771).expect("read").expect("present");
    assert_eq!(v.name.as_deref(), Some("Newer"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_newer_schema_is_rejected_and_the_live_file_stays() {
    let dir = tempdir("newer-schema");
    let live = live_cache(&dir);
    live.put_volumes(&[VolumeRow {
        volume_id: 771,
        name: Some("Kept".into()),
        ..Default::default()
    }])
    .expect("seed");
    let source = source_file(&dir, |conn| {
        conn.execute_batch(
            "CREATE TABLE whatever (id INTEGER);
             INSERT INTO volume (volume_id, name) VALUES (771, 'From The Future');
             PRAGMA user_version = 99;",
        )
        .expect("seed source");
    });

    let error = live.import(&source).expect_err("a newer schema rejects");
    assert!(error.to_string().contains("newer"), "{error}");
    // The live file is untouched.
    let v = live.volume(771).expect("read").expect("present");
    assert_eq!(v.name.as_deref(), Some("Kept"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn a_v1_file_migrates_in_the_temp_copy() {
    let dir = tempdir("v1-chain");
    let live = live_cache(&dir);
    // The source carries the true v1 schema, so the temp copy runs
    // the whole v1→v2→v3 chain before the merge.
    let source = dir.join("source-v1.sqlite");
    {
        let conn = rusqlite::Connection::open(&source).expect("open v1 source");
        conn.execute_batch(
            "CREATE TABLE volume (
                volume_id INTEGER PRIMARY KEY, name TEXT, publisher TEXT,
                start_year INTEGER, count_of_issues INTEGER,
                date_last_updated TEXT, last_cover_date TEXT,
                fetched_at INTEGER NOT NULL DEFAULT 0
             );
             CREATE TABLE issue_skeleton (
                issue_id INTEGER PRIMARY KEY, volume_id INTEGER NOT NULL,
                issue_number TEXT NOT NULL, cover_date TEXT, name TEXT
             );
             CREATE TABLE issue_detail (
                issue_id INTEGER PRIMARY KEY, json TEXT NOT NULL,
                fetched_at INTEGER NOT NULL
             );
             CREATE TABLE image_blob (
                url TEXT PRIMARY KEY, bytes BLOB NOT NULL,
                fetched_at INTEGER NOT NULL
             );
             CREATE TABLE search_result (
                terms TEXT PRIMARY KEY, json TEXT NOT NULL,
                fetched_at INTEGER NOT NULL
             );
             CREATE TABLE request_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                resource TEXT NOT NULL, at INTEGER NOT NULL
             );
             CREATE TABLE sweep_state (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                start_date TEXT NOT NULL, end_date TEXT NOT NULL,
                offset INTEGER NOT NULL, total INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
             );
             INSERT INTO volume (volume_id, name) VALUES (806, 'Version One');
             PRAGMA user_version = 1;",
        )
        .expect("seed a v1 source");
    }

    let report = live.import(&source).expect("import");

    let v = live.volume(806).expect("read").expect("present");
    assert_eq!(v.name.as_deref(), Some("Version One"));
    let volume = report_of(&report, "volume");
    assert_eq!(volume.added, 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn image_blobs_compare_on_fetched_at_alone() {
    let dir = tempdir("blobs");
    // The live stamps are set by hand: `kept` holds an older stamp
    // than the incoming blob, `older` a newer one.
    SqliteCache::open(&dir.join("live.sqlite")).expect("create the live schema");
    {
        let conn = rusqlite::Connection::open(dir.join("live.sqlite")).expect("open live file");
        conn.execute_batch(
            "INSERT INTO image_blob (url, bytes, fetched_at) VALUES ('http://cv/kept.jpg', x'0101', 100);
             INSERT INTO image_blob (url, bytes, fetched_at) VALUES ('http://cv/older.jpg', x'0909', 5000);",
        )
        .expect("seed live blobs");
    }
    let cache = SqliteCache::open(&dir.join("live.sqlite")).expect("reopen the live cache");
    let source = source_file(&dir, |conn| {
        conn.execute_batch(
            "INSERT INTO image_blob (url, bytes, fetched_at) VALUES ('http://cv/kept.jpg', x'0202', 900);
             INSERT INTO image_blob (url, bytes, fetched_at) VALUES ('http://cv/older.jpg', x'0303', 10);",
        )
        .expect("seed source blobs");
    });

    let report = cache.import(&source).expect("import");

    assert_eq!(
        cache.image("http://cv/kept.jpg").expect("read"),
        Some(vec![2, 2]),
        "the newer blob wins"
    );
    assert_eq!(
        cache.image("http://cv/older.jpg").expect("read"),
        Some(vec![9, 9]),
        "the stored blob keeps its newer stamp"
    );
    let blob = report_of(&report, "image_blob");
    assert_eq!((blob.added, blob.updated, blob.skipped), (0, 1, 1));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn requests_append_and_sweep_state_takes_the_newer() {
    let dir = tempdir("accounting");
    let live = live_cache(&dir);
    for at in [100, 200] {
        live.log_request("issues", at).expect("log live");
    }
    live.put_sweep_state(&cr_scrape::cache::SweepState {
        start_date: "2026-08-26".into(),
        end_date: "2026-09-01".into(),
        offset: 10,
        total: 100,
        updated_at: 1_000,
    })
    .expect("seed live sweep state");
    let source = source_file(&dir, |conn| {
        conn.execute_batch(
            "INSERT INTO request_log (resource, at) VALUES ('issues', 300);
             INSERT INTO sweep_state (id, start_date, end_date, offset, total, updated_at)
               VALUES (1, '2026-08-26', '2026-09-12', 200, 4200, 2_000);",
        )
        .expect("seed source accounting");
    });

    let report = live.import(&source).expect("import");

    assert_eq!(live.requests_since("issues", 0).expect("count"), 3);
    let log = report_of(&report, "request_log");
    assert_eq!((log.added, log.rejected), (1, 0));
    let state = live.sweep_state().expect("read").expect("present");
    assert_eq!(state.offset, 200);
    assert_eq!(state.updated_at, 2_000);
    let sweep = report_of(&report, "sweep_state");
    assert_eq!(sweep.updated, 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn resources_and_credits_union_and_respect_the_stamps() {
    let dir = tempdir("resources");
    // The live character row carries an older stamp, so the incoming
    // row is the base.
    SqliteCache::open(&dir.join("live.sqlite")).expect("create the live schema");
    {
        let conn = rusqlite::Connection::open(dir.join("live.sqlite")).expect("open live file");
        conn.execute_batch(
            "INSERT INTO character (id, name, fetched_at) VALUES (9, 'Stored Hero', 100);",
        )
        .expect("seed live resource");
    }
    let live = SqliteCache::open(&dir.join("live.sqlite")).expect("reopen the live cache");
    live.put_credits(
        cr_scrape::cache::OwnerKind::Issue,
        5,
        &[cr_scrape::cache::CreditRef {
            kind: cr_scrape::cache::ResourceKind::Person,
            resource_id: Some(12),
            name: Some("Writer".into()),
            role: Some("writer".into()),
            marker: cr_scrape::cache::CreditMarker::Credit,
        }],
    )
    .expect("seed live credit");
    let source = source_file(&dir, |conn| {
        conn.execute_batch(
            "INSERT INTO character (id, name, fetched_at) VALUES (9, 'Newer Hero', 500);
             INSERT INTO person (id, name, fetched_at) VALUES (13, 'New Person', 500);
             INSERT INTO credit (owner_kind, owner_id, resource_kind, resource_id, name, role, marker)
               VALUES ('issue', 5, 'person', 12, 'Writer', 'writer', 'credit');
             INSERT INTO credit (owner_kind, owner_id, resource_kind, resource_id, name, role, marker)
               VALUES ('issue', 5, 'person', 13, 'Inker', 'inker', 'credit');",
        )
        .expect("seed source resources");
    });

    let report = live.import(&source).expect("import");

    let hero = live
        .resource(cr_scrape::cache::ResourceKind::Character, 9)
        .expect("read")
        .expect("resource");
    assert_eq!(hero.name.as_deref(), Some("Newer Hero"));
    let person = live
        .resource(cr_scrape::cache::ResourceKind::Person, 13)
        .expect("read")
        .expect("added resource");
    assert_eq!(person.name.as_deref(), Some("New Person"));
    let credits = live
        .credits_of(cr_scrape::cache::OwnerKind::Issue, 5)
        .expect("read");
    assert_eq!(credits.len(), 2, "the credits union");
    let character = report_of(&report, "character");
    assert_eq!(character.updated, 1);
    let person_report = report_of(&report, "person");
    assert_eq!(person_report.added, 1);
    let credit = report_of(&report, "credit");
    assert_eq!((credit.added, credit.skipped), (1, 1));

    // An identical re-import changes nothing (idempotent).
    let report = live.import(&source).expect("re-import");
    let credit = report_of(&report, "credit");
    assert_eq!(credit.added, 0);
    assert_eq!(credit.skipped, 2);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_backup_round_trips_through_an_import() {
    let dir = tempdir("backup");
    let live = live_cache(&dir);
    live.put_volumes(&[VolumeRow {
        volume_id: 771,
        name: Some("Blacksad".into()),
        count_of_issues: Some(6),
        ..Default::default()
    }])
    .expect("seed");
    live.put_issues(&[cr_scrape::cache::IssueSkeleton {
        issue_id: 92_469,
        volume_id: 771,
        issue_number: "1".into(),
        name: Some("Quelque part entre les ombres".into()),
        ..Default::default()
    }])
    .expect("seed issues");
    let backup = dir.join("backup.sqlite");
    live.backup(&backup).expect("backup");
    assert!(backup.is_file());
    // A second backup to the same path fails: the target must not exist.
    assert!(live.backup(&backup).is_err());

    // A fresh cache imports the snapshot whole.
    let restored = SqliteCache::open(&dir.join("restored.sqlite")).expect("fresh cache");
    let report = restored.import(&backup).expect("import the backup");
    let v = restored.volume(771).expect("read").expect("present");
    assert_eq!(v.name.as_deref(), Some("Blacksad"));
    assert_eq!(v.count_of_issues, Some(6));
    let issues = restored.issues_of_volume(771).expect("read");
    assert_eq!(issues.len(), 1);
    assert_eq!(issues[0].issue_number, "1");
    assert_eq!(report_of(&report, "volume").added, 1);
    assert_eq!(report_of(&report, "issue_skeleton").added, 1);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_checkpoint_leaves_a_complete_main_file() {
    let dir = tempdir("checkpoint");
    let path = dir.join("cvcache.sqlite");
    {
        let cache = SqliteCache::open(&path).expect("open");
        cache
            .put_volumes(&[VolumeRow {
                volume_id: 771,
                name: Some("Blacksad".into()),
                ..Default::default()
            }])
            .expect("seed");
        cache.checkpoint().expect("checkpoint");
    }
    // The main file alone holds the data: no -wal side file needed.
    let conn = rusqlite::Connection::open(&path).expect("reopen");
    let name: Option<String> = conn
        .query_row("SELECT name FROM volume WHERE volume_id = 771", [], |r| {
            r.get(0)
        })
        .expect("read");
    assert_eq!(name.as_deref(), Some("Blacksad"));
    std::fs::remove_dir_all(&dir).ok();
}
