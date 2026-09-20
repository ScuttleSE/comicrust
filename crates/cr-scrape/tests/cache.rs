//! Gates for the Comic Vine disk cache (ADR-037, Phase 15 T1).

use cr_scrape::cache::{
    CreditMarker, CreditRef, CvCache, IssueSkeleton, OwnerKind, ResourceKind, ResourceRef,
    SqliteCache, SweepState, VolumeRow,
};

fn cache() -> SqliteCache {
    SqliteCache::in_memory().expect("in-memory cache opens")
}

fn vol(id: i64) -> VolumeRow {
    VolumeRow {
        volume_id: id,
        ..Default::default()
    }
}

fn issue(id: i64, volume_id: i64, number: &str) -> IssueSkeleton {
    IssueSkeleton {
        issue_id: id,
        volume_id,
        issue_number: number.to_string(),
        ..Default::default()
    }
}

#[test]
fn schema_migrates_from_empty_and_is_idempotent() {
    let dir = tempdir();
    let path = dir.join("cvcache.sqlite");
    {
        let c = SqliteCache::open(&path).expect("first open creates the file");
        c.put_volumes(&[vol(771)]).expect("write");
    }
    // A second open must migrate nothing away and must keep the data.
    let c = SqliteCache::open(&path).expect("second open");
    assert_eq!(c.volume(771).expect("read").map(|v| v.volume_id), Some(771));
    std::fs::remove_dir_all(&dir).ok();
}

/// The full v1 table set, as phase 15 wrote it. The migrations alter
/// `volume` and `issue_detail`, so a v1 fixture carries every v1
/// table.
fn create_v1_schema(conn: &rusqlite::Connection) {
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
         PRAGMA user_version = 1;",
    )
    .expect("create version one schema");
}

#[test]
fn schema_migrates_a_version_one_cache_to_the_manager_schema() {
    let dir = tempdir();
    let path = dir.join("cvcache.sqlite");
    {
        let conn = rusqlite::Connection::open(&path).expect("open version one database");
        create_v1_schema(&conn);
        conn.execute(
            "INSERT INTO volume (volume_id, name) VALUES (806, 'Kept')",
            [],
        )
        .expect("insert volume");
    }

    let cache = SqliteCache::open(&path).expect("migrate version one cache");
    let record = cache
        .managed_volume(806)
        .expect("read migrated cache")
        .expect("kept volume");
    assert_eq!(record.volume.name.as_deref(), Some("Kept"));
    assert_eq!(record.detail_json, None);
    assert!(record.pending_issue_details.is_empty());
    std::fs::remove_dir_all(&dir).ok();
}

// --- schema v3 (ADR-070, Phase 21 T1) ---

/// Builds a version-2 database with one stored volume detail and one
/// stored issue detail — the shape a phase-20 complete update wrote.
/// A real v2 file carries the full v1 schema plus the v2 additions.
fn v2_file_with_details(path: &std::path::Path, volume_json: &str, issue_json: &str) {
    let conn = rusqlite::Connection::open(path).expect("open version two database");
    create_v1_schema(&conn);
    conn.execute_batch(
        "ALTER TABLE volume ADD COLUMN detail_json TEXT;
         CREATE TABLE pending_issue_detail (
            volume_id INTEGER NOT NULL,
            issue_id  INTEGER NOT NULL,
            position  INTEGER NOT NULL,
            PRIMARY KEY (volume_id, issue_id)
         );",
    )
    .expect("upgrade the fixture to version two");
    conn.execute(
        "INSERT INTO volume (volume_id, name, detail_json) VALUES (771, 'Blacksad', ?1)",
        rusqlite::params![volume_json],
    )
    .expect("insert volume detail");
    conn.execute(
        "INSERT INTO issue_detail (issue_id, json, fetched_at) VALUES (92469, ?1, 500)",
        rusqlite::params![issue_json],
    )
    .expect("insert issue detail");
    conn.execute("PRAGMA user_version = 2", [])
        .expect("stamp v2");
}

#[test]
fn open_creates_the_parent_directory() {
    let dir = tempdir();
    let path = dir.join("a").join("b").join("cvcache.sqlite");
    SqliteCache::open(&path).expect("open creates the directory chain");
    assert!(path.exists());
    std::fs::remove_dir_all(&dir).ok();
}

/// The typed `volume` columns as one raw read of the migrated file.
type VolumeColumns = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<i64>,
);

/// The typed `issue_detail` columns as one raw read of the migrated file.
type IssueColumns = (
    Option<i64>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

#[test]
fn a_v2_cache_with_stored_details_backfills_the_typed_columns() {
    let dir = tempdir();
    let path = dir.join("cvcache.sqlite");
    let volume_json = r#"{"id":771,"name":"Blacksad","deck":"John Blacksad's cases","description":"A description","aliases":"Blacksad\nJohn Blacksad","image":{"small_url":"http://cv/small.jpg","medium_url":""},"api_detail_url":"http://api/volume/4050-771/","site_detail_url":"http://comics/volume/771","date_added":"2020-01-02 03:04:05","date_last_updated":"2021-02-03 04:05:06","first_issue":{"id":92469},"last_issue":{"id":165276},"count_of_issues":6}"#;
    let issue_json = r#"{"id":92469,"volume":{"id":771},"issue_number":" 1 ","cover_date":"2000-11-01","name":"Quelque part entre les ombres","store_date":"2000-10-15","image":{"small_url":"http://cv/i-small.jpg","super_url":"http://cv/i-super.jpg"},"date_added":"2020-06-07 08:09:10","date_last_updated":"2021-11-12 13:14:15"}"#;
    v2_file_with_details(&path, volume_json, issue_json);

    let cache = SqliteCache::open(&path).expect("migrate the v2 cache");
    // The raw JSON text stays byte-identical through the migration.
    let record = cache.managed_volume(771).expect("read").expect("volume");
    assert_eq!(record.detail_json.as_deref(), Some(volume_json));
    let (stored, at) = cache.issue_detail(92469).expect("read").expect("detail");
    assert_eq!(stored, issue_json);
    assert_eq!(at, 500);
    // A second open changes nothing.
    drop(cache);
    let cache = SqliteCache::open(&path).expect("reopen the migrated cache");
    let record = cache.managed_volume(771).expect("read").expect("volume");
    assert_eq!(record.detail_json.as_deref(), Some(volume_json));
    drop(cache);

    let conn = rusqlite::Connection::open(&path).expect("inspect the migrated file");
    let version: i32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .expect("version");
    assert_eq!(version, 4);

    let (aliases, deck, description, image_url, api_url, site_url, date_added, first_id, last_id): VolumeColumns =
        conn
        .query_row(
            "SELECT aliases, deck, description, image_url, api_detail_url,
                    site_detail_url, date_added, first_issue_id, last_issue_id
               FROM volume WHERE volume_id = 771",
            [],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                ))
            },
        )
        .expect("volume columns");
    assert_eq!(aliases.as_deref(), Some("Blacksad\nJohn Blacksad"));
    assert_eq!(deck.as_deref(), Some("John Blacksad's cases"));
    assert_eq!(description.as_deref(), Some("A description"));
    // The first non-empty URL in the scrape's own order wins.
    assert_eq!(image_url.as_deref(), Some("http://cv/small.jpg"));
    assert_eq!(api_url.as_deref(), Some("http://api/volume/4050-771/"));
    assert_eq!(site_url.as_deref(), Some("http://comics/volume/771"));
    assert_eq!(date_added.as_deref(), Some("2020-01-02 03:04:05"));
    assert_eq!(first_id, Some(92_469));
    assert_eq!(last_id, Some(165_276));

    let (
        volume_id,
        issue_number,
        cover_date,
        name,
        store_date,
        issue_image,
        issue_added,
        issue_updated,
    ): IssueColumns = conn
        .query_row(
            "SELECT volume_id, issue_number, cover_date, name, store_date,
                    image_url, date_added, date_last_updated
               FROM issue_detail WHERE issue_id = 92469",
            [],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                ))
            },
        )
        .expect("issue columns");
    assert_eq!(volume_id, Some(771));
    assert_eq!(issue_number.as_deref(), Some("1"));
    assert_eq!(cover_date.as_deref(), Some("2000-11-01"));
    assert_eq!(name.as_deref(), Some("Quelque part entre les ombres"));
    assert_eq!(store_date.as_deref(), Some("2000-10-15"));
    assert_eq!(issue_image.as_deref(), Some("http://cv/i-small.jpg"));
    assert_eq!(issue_added.as_deref(), Some("2020-06-07 08:09:10"));
    assert_eq!(issue_updated.as_deref(), Some("2021-11-12 13:14:15"));

    // The resource tables and the credit table exist.
    let mut stmt = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table'")
        .expect("list tables");
    let tables: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .expect("query tables")
        .collect::<Result<_, _>>()
        .expect("table names");
    for name in [
        "character",
        "person",
        "team",
        "story_arc",
        "location",
        "concept",
        "object",
        "publisher",
        "credit",
    ] {
        assert!(tables.iter().any(|t| t == name), "the {name} table exists");
    }
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn the_v1_to_v4_chain_backfills_issue_details() {
    let dir = tempdir();
    let path = dir.join("cvcache.sqlite");
    {
        let conn = rusqlite::Connection::open(&path).expect("open version one database");
        create_v1_schema(&conn);
        conn.execute(
            "INSERT INTO issue_detail (issue_id, json, fetched_at)
               VALUES (11, '{\"id\":11,\"volume\":{\"id\":42},\"issue_number\":\"2\",\"cover_date\":\"1998-03-01\",\"name\":\"The Case\"}', 100)",
            [],
        )
        .expect("insert issue detail");
    }
    SqliteCache::open(&path).expect("the v1 file migrates through v2, v3, and v4");
    drop(SqliteCache::open(&path).expect("reopen"));

    let conn = rusqlite::Connection::open(&path).expect("inspect");
    let version: i32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .expect("version");
    assert_eq!(version, 4);
    let (volume_id, issue_number, cover_date, name): (
        Option<i64>,
        Option<String>,
        Option<String>,
        Option<String>,
    ) = conn
        .query_row(
            "SELECT volume_id, issue_number, cover_date, name
               FROM issue_detail WHERE issue_id = 11",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .expect("columns");
    assert_eq!(volume_id, Some(42));
    assert_eq!(issue_number.as_deref(), Some("2"));
    assert_eq!(cover_date.as_deref(), Some("1998-03-01"));
    assert_eq!(name.as_deref(), Some("The Case"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn malformed_detail_json_keeps_null_columns_and_still_migrates() {
    let dir = tempdir();
    let path = dir.join("cvcache.sqlite");
    v2_file_with_details(&path, "{not json", "[");

    let cache = SqliteCache::open(&path).expect("the migration survives bad JSON");
    let record = cache.managed_volume(771).expect("read").expect("volume");
    assert_eq!(record.detail_json.as_deref(), Some("{not json"));
    let (stored, _) = cache.issue_detail(92469).expect("read").expect("detail");
    assert_eq!(stored, "[");
    drop(cache);

    let conn = rusqlite::Connection::open(&path).expect("inspect");
    let version: i32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .expect("version");
    assert_eq!(version, 4);
    let (aliases, first_id): (Option<String>, Option<i64>) = conn
        .query_row(
            "SELECT aliases, first_issue_id FROM volume WHERE volume_id = 771",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("volume columns");
    assert_eq!(aliases, None);
    assert_eq!(first_id, None);
    let (issue_number, volume_id): (Option<String>, Option<i64>) = conn
        .query_row(
            "SELECT issue_number, volume_id FROM issue_detail WHERE issue_id = 92469",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("issue columns");
    assert_eq!(issue_number, None);
    assert_eq!(volume_id, None);
    std::fs::remove_dir_all(&dir).ok();
}

// --- inline references (ADR-070, Phase 21 T2) ---

#[test]
fn the_reference_extraction_reads_all_three_list_shapes() {
    use cr_scrape::cache::resources;

    // the plain array form
    let refs =
        resources::extract_issue_references(r#"{"character_credits": [{"id":1,"name":"A"}]}"#)
            .expect("extract");
    assert_eq!(refs.resources.len(), 1);
    assert_eq!(refs.credits.len(), 1);
    assert_eq!(refs.credits[0].marker.as_str(), "credit");

    // the wrapped object form, with a role for people
    let refs = resources::extract_issue_references(
        r#"{"person_credits": {"person": [{"id":2,"name":"B","role":"writer"}]}}"#,
    )
    .expect("extract");
    assert_eq!(refs.credits[0].role.as_deref(), Some("writer"));

    // a lone reference object
    let refs =
        resources::extract_issue_references(r#"{"team_credits": {"team": {"id":3,"name":"T"}}}"#)
            .expect("extract");
    assert_eq!(refs.credits.len(), 1);
    assert_eq!(refs.credits[0].kind, ResourceKind::Team);

    // a name-only reference becomes a credit without a resource id
    let refs = resources::extract_issue_references(
        r#"{"character_credits": {"character": [{"name":"Name Only"}]}}"#,
    )
    .expect("extract");
    assert!(refs.resources.is_empty());
    assert_eq!(refs.credits[0].resource_id, None);

    // absent fields read as empty
    let refs = resources::extract_issue_references(r#"{"id":5}"#).expect("extract");
    assert!(refs.resources.is_empty());
    assert!(refs.credits.is_empty());

    // the volume extraction reads the publisher object
    let refs = resources::extract_volume_references(
        r#"{"publisher": {"id":7,"name":"Press"}, "team_credits": {"team": []}}"#,
    )
    .expect("extract");
    assert_eq!(refs.resources[0].kind, ResourceKind::Publisher);
    assert_eq!(refs.resources[0].id, Some(7));
    assert!(refs.credits.is_empty());
}

#[test]
fn a_credit_without_an_id_stores_with_resource_id_zero() {
    let c = cache();
    c.put_credits(
        OwnerKind::Issue,
        5,
        &[CreditRef {
            kind: ResourceKind::Character,
            resource_id: None,
            name: Some("Name Only".into()),
            role: None,
            marker: CreditMarker::Credit,
        }],
    )
    .expect("write");
    let rows = c.credits_of(OwnerKind::Issue, 5).expect("read");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].resource_id, 0);
    // No resource row exists for an unidentified reference.
    assert!(c
        .resource(ResourceKind::Character, 0)
        .expect("read")
        .is_none());
}

#[test]
fn a_resource_upsert_never_erases_a_stored_name() {
    let c = cache();
    c.put_resources(&[ResourceRef {
        kind: ResourceKind::Character,
        id: Some(9),
        name: Some("Hero".into()),
    }])
    .expect("write");
    c.put_resources(&[ResourceRef {
        kind: ResourceKind::Character,
        id: Some(9),
        name: None,
    }])
    .expect("write");
    let row = c
        .resource(ResourceKind::Character, 9)
        .expect("read")
        .expect("row");
    assert_eq!(row.name.as_deref(), Some("Hero"));
    assert!(row.detail_json.is_none());
}

#[test]
fn a_reimport_of_the_same_credits_is_idempotent() {
    let c = cache();
    let credits = vec![
        CreditRef {
            kind: ResourceKind::Person,
            resource_id: Some(12),
            name: Some("Writer".into()),
            role: Some("writer".into()),
            marker: CreditMarker::Credit,
        },
        CreditRef {
            kind: ResourceKind::Character,
            resource_id: Some(10),
            name: Some("Sidekick".into()),
            role: None,
            marker: CreditMarker::DiedIn,
        },
    ];
    c.put_credits(OwnerKind::Issue, 5, &credits).expect("write");
    // A changed list replaces the rows of its owner only.
    c.put_credits(OwnerKind::Issue, 5, &credits[..1])
        .expect("rewrite");
    let rows = c.credits_of(OwnerKind::Issue, 5).expect("read");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name.as_deref(), Some("Writer"));
    // Another owner keeps its rows.
    c.put_credits(OwnerKind::Volume, 7, &credits)
        .expect("write");
    c.put_credits(OwnerKind::Issue, 5, &credits).expect("write");
    assert_eq!(c.credits_of(OwnerKind::Volume, 7).expect("read").len(), 2);
    assert_eq!(c.credits_of(OwnerKind::Issue, 5).expect("read").len(), 2);
}

#[test]
fn a_fresh_cache_opens_at_version_four() {
    let dir = tempdir();
    let path = dir.join("cvcache.sqlite");
    {
        let cache = SqliteCache::open(&path).expect("fresh open");
        cache
            .put_volumes(&[vol(771)])
            .expect("write through the current schema");
    }
    let conn = rusqlite::Connection::open(&path).expect("inspect");
    let version: i32 = conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .expect("version");
    assert_eq!(version, 4);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn unknown_volume_and_issue_read_as_none() {
    let c = cache();
    assert_eq!(c.volume(1).expect("read"), None);
    assert!(c.issues_of_volume(1).expect("read").is_empty());
    assert_eq!(c.issue_count(1).expect("count"), 0);
    assert_eq!(c.issue_detail(1).expect("read"), None);
    assert_eq!(c.image("u").expect("read"), None);
    assert_eq!(c.search("t").expect("read"), None);
    assert_eq!(c.sweep_state().expect("read"), None);
}

#[test]
fn a_cheap_volume_write_does_not_erase_an_expensive_one() {
    let c = cache();
    // The detail query found everything.
    c.put_volumes(&[VolumeRow {
        volume_id: 771,
        name: Some("Blacksad".into()),
        publisher: Some("Dark Horse".into()),
        start_year: Some(2010),
        count_of_issues: Some(6),
        date_last_updated: Some("2020-01-02 03:04:05".into()),
        last_cover_date: Some("2013-06-01".into()),
        fetched_at: 500,
    }])
    .expect("write");
    // An MCL import knows only the id. It must erase nothing.
    c.put_volumes(&[vol(771)]).expect("write");
    let v = c.volume(771).expect("read").expect("present");
    assert_eq!(v.name.as_deref(), Some("Blacksad"));
    assert_eq!(v.publisher.as_deref(), Some("Dark Horse"));
    assert_eq!(v.start_year, Some(2010));
    assert_eq!(v.count_of_issues, Some(6));
    assert_eq!(v.last_cover_date.as_deref(), Some("2013-06-01"));
    // `fetched_at` keeps the newer of the two.
    assert_eq!(v.fetched_at, 500);
}

#[test]
fn a_newer_volume_write_wins_field_by_field() {
    let c = cache();
    c.put_volumes(&[vol(771)]).expect("write");
    c.put_volumes(&[VolumeRow {
        volume_id: 771,
        count_of_issues: Some(7),
        fetched_at: 900,
        ..Default::default()
    }])
    .expect("write");
    let v = c.volume(771).expect("read").expect("present");
    assert_eq!(v.count_of_issues, Some(7));
    assert_eq!(v.fetched_at, 900);
    assert_eq!(v.name, None);
}

#[test]
fn issues_return_in_issue_id_order() {
    let c = cache();
    // The MCL order is by issue id, not by issue number (ADR-038).
    c.put_issues(&[
        issue(165_276, 771, "3"),
        issue(92_469, 771, "1"),
        issue(92_470, 771, "2"),
        issue(11, 999, "1"),
    ])
    .expect("write");
    let got = c.issues_of_volume(771).expect("read");
    assert_eq!(
        got.iter().map(|i| i.issue_id).collect::<Vec<_>>(),
        vec![92_469, 92_470, 165_276]
    );
    assert_eq!(
        got.iter()
            .map(|i| i.issue_number.as_str())
            .collect::<Vec<_>>(),
        vec!["1", "2", "3"]
    );
    assert_eq!(c.issue_count(771).expect("count"), 3);
    assert_eq!(c.issue_count(999).expect("count"), 1);
}

#[test]
fn an_issue_number_keeps_its_text_form() {
    let c = cache();
    // Volume 77901 holds the issue number `1,5` (ADR-038).
    c.put_issues(&[issue(1, 77_901, "1,5"), issue(2, 77_901, "v. 1, no. 01")])
        .expect("write");
    let got = c.issues_of_volume(77_901).expect("read");
    assert_eq!(got[0].issue_number, "1,5");
    assert_eq!(got[1].issue_number, "v. 1, no. 01");
}

#[test]
fn an_issue_can_move_to_another_volume() {
    let c = cache();
    c.put_issues(&[issue(5, 100, "1")]).expect("write");
    c.put_issues(&[issue(5, 200, "1a")]).expect("write");
    assert_eq!(c.issue_count(100).expect("count"), 0);
    let got = c.issues_of_volume(200).expect("read");
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].issue_number, "1a");
}

#[test]
fn a_cheap_issue_write_does_not_erase_the_cover_date() {
    let c = cache();
    c.put_issues(&[IssueSkeleton {
        issue_id: 5,
        volume_id: 100,
        issue_number: "1".into(),
        cover_date: Some("1998-03-01".into()),
        name: Some("Somewhere Within the Shadows".into()),
        ..Default::default()
    }])
    .expect("write");
    // A cheap write (the old sweep's `id,issue_number,volume` list)
    // never erases the list fields (ADR-072).
    c.put_issues(&[issue(5, 100, "1")]).expect("write");
    let got = c.issues_of_volume(100).expect("read");
    assert_eq!(got[0].cover_date.as_deref(), Some("1998-03-01"));
    assert_eq!(got[0].name.as_deref(), Some("Somewhere Within the Shadows"));
}

#[test]
fn detail_image_and_search_round_trip_and_overwrite() {
    let c = cache();
    c.put_issue_detail(5, r#"{"id":5}"#).expect("write");
    let (json, at) = c.issue_detail(5).expect("read").expect("present");
    assert_eq!(json, r#"{"id":5}"#);
    assert!(at > 0, "fetched_at is stamped");
    c.put_issue_detail(5, r#"{"id":5,"name":"x"}"#)
        .expect("overwrite");
    assert_eq!(
        c.issue_detail(5).expect("read").expect("present").0,
        r#"{"id":5,"name":"x"}"#
    );

    c.put_image("http://cv/a.jpg", &[1, 2, 3]).expect("write");
    assert_eq!(
        c.image("http://cv/a.jpg").expect("read"),
        Some(vec![1, 2, 3])
    );
    c.put_image("http://cv/a.jpg", &[9]).expect("overwrite");
    assert_eq!(c.image("http://cv/a.jpg").expect("read"), Some(vec![9]));

    c.put_search("blacksad", "[]").expect("write");
    assert_eq!(
        c.search("blacksad").expect("read").map(|(j, _)| j),
        Some("[]".to_string())
    );
}

#[test]
fn request_accounting_counts_per_resource() {
    let c = cache();
    for at in [100, 200, 300] {
        c.log_request("issues", at).expect("log");
    }
    c.log_request("volumes", 250).expect("log");
    assert_eq!(c.requests_since("issues", 0).expect("count"), 3);
    assert_eq!(c.requests_since("issues", 200).expect("count"), 2);
    assert_eq!(c.requests_since("issues", 400).expect("count"), 0);
    // The budget is per resource, not a total.
    assert_eq!(c.requests_since("volumes", 0).expect("count"), 1);
    assert_eq!(c.requests_since("search", 0).expect("count"), 0);
}

#[test]
fn the_oldest_request_gives_the_resume_time() {
    let c = cache();
    assert_eq!(c.oldest_request_since("issues", 0).expect("read"), None);
    for at in [300, 100, 200] {
        c.log_request("issues", at).expect("log");
    }
    assert_eq!(
        c.oldest_request_since("issues", 0).expect("read"),
        Some(100)
    );
    assert_eq!(
        c.oldest_request_since("issues", 150).expect("read"),
        Some(200)
    );
    assert_eq!(c.oldest_request_since("issues", 999).expect("read"), None);
}

#[test]
fn pruning_removes_only_the_old_records() {
    let c = cache();
    for at in [100, 200, 300] {
        c.log_request("issues", at).expect("log");
    }
    c.prune_requests(250).expect("prune");
    assert_eq!(c.requests_since("issues", 0).expect("count"), 1);
    assert_eq!(
        c.oldest_request_since("issues", 0).expect("read"),
        Some(300)
    );
}

#[test]
fn the_sweep_state_holds_one_row_and_resumes() {
    let c = cache();
    let first = SweepState {
        start_date: "2026-08-26".into(),
        end_date: "2026-09-12".into(),
        offset: 100,
        total: 4200,
        updated_at: 1_000,
    };
    c.put_sweep_state(&first).expect("write");
    assert_eq!(c.sweep_state().expect("read"), Some(first));

    let resumed = SweepState {
        start_date: "2026-08-26".into(),
        end_date: "2026-09-12".into(),
        offset: 200,
        total: 4200,
        updated_at: 1_100,
    };
    c.put_sweep_state(&resumed).expect("write");
    assert_eq!(c.sweep_state().expect("read"), Some(resumed));
}

/// A private directory for one test. The crate has no `tempfile`
/// dependency and this keeps it that way. The counter makes the name
/// unique inside the process, so two tests never share a directory.
fn tempdir() -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let mut p = std::env::temp_dir();
    p.push(format!(
        "cr-scrape-cache-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::remove_dir_all(&p).ok();
    std::fs::create_dir_all(&p).expect("temp dir");
    p
}

// --- MCL import (ADR-038, Phase 15 T2) ---

fn sample_mcl() -> std::io::BufReader<std::fs::File> {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/testdata/mcl/sample.mcl");
    std::io::BufReader::new(std::fs::File::open(path).expect("fixture opens"))
}

#[test]
fn an_mcl_import_seeds_the_skeleton() {
    let c = cache();
    let report = cr_scrape::cache::mcl::import(&c, sample_mcl()).expect("import");
    assert_eq!(report.date, "2026-08-26");
    // The `broken;x;y` line is skipped; the other five load.
    assert_eq!(report.volumes, 5);
    assert_eq!(report.skipped, 1);
    assert_eq!(report.issues, 11);

    assert_eq!(c.issue_count(771).expect("count"), 6);
    let got = c.issues_of_volume(771).expect("read");
    assert_eq!(got[0].issue_id, 92_469);
    assert_eq!(got[5].issue_number, "6");

    // The escape reverses: volume 77901 holds the issue number `1,5`.
    let odd = c.issues_of_volume(77_901).expect("read");
    assert_eq!(odd[0].issue_number, "1,5");
    assert_eq!(odd[1].issue_number, "2");

    // The quoted form reads, and a comma that a space follows stays.
    let quoted = c.issues_of_volume(18_166).expect("read");
    assert_eq!(quoted[0].issue_number, "v. 1, no. 01");
    assert_eq!(quoted[1].issue_number, "v. 1, no. 02");

    // A volume line with no issues still creates the volume.
    assert!(c.volume(4050).expect("read").is_some());
    assert_eq!(c.issue_count(4050).expect("count"), 0);
}

#[test]
fn an_mcl_import_erases_no_api_data() {
    let c = cache();
    c.put_volumes(&[VolumeRow {
        volume_id: 771,
        name: Some("Blacksad".into()),
        count_of_issues: Some(6),
        fetched_at: 500,
        ..Default::default()
    }])
    .expect("write");
    c.put_issues(&[IssueSkeleton {
        issue_id: 92_469,
        volume_id: 771,
        issue_number: "1".into(),
        cover_date: Some("2000-11-01".into()),
        name: Some("Quelque part entre les ombres".into()),
        ..Default::default()
    }])
    .expect("write");

    cr_scrape::cache::mcl::import(&c, sample_mcl()).expect("import");

    let v = c.volume(771).expect("read").expect("present");
    assert_eq!(v.name.as_deref(), Some("Blacksad"));
    assert_eq!(v.count_of_issues, Some(6));
    let i = c.issues_of_volume(771).expect("read");
    assert_eq!(i[0].cover_date.as_deref(), Some("2000-11-01"));
    assert_eq!(i[0].name.as_deref(), Some("Quelque part entre les ombres"));
}

#[test]
fn an_export_of_an_import_agrees_on_the_data() {
    let c = cache();
    cr_scrape::cache::mcl::import(&c, sample_mcl()).expect("import");

    let mut volumes = Vec::new();
    for id in [771, 2127, 4050, 18_166, 77_901] {
        volumes.push(cr_scrape::cache::mcl::MclVolume {
            volume_id: id,
            issues: c
                .issues_of_volume(id)
                .expect("read")
                .into_iter()
                .map(|i| cr_scrape::cache::mcl::MclIssue {
                    issue_id: i.issue_id,
                    issue_number: i.issue_number,
                })
                .collect(),
        });
    }
    let mut out = Vec::new();
    cr_scrape::cache::mcl::write(&mut out, "2026-09-12", volumes.clone()).expect("write");

    let mut back = Vec::new();
    let report = cr_scrape::cache::mcl::read(out.as_slice(), |v| back.push(v)).expect("read");
    assert_eq!(report.skipped, 0);
    assert_eq!(back, volumes);
}
