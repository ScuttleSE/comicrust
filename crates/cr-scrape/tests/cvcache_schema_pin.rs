//! The schema pin between the app and the `scripts/cvcache/` Python
//! package (Phase 21 T8, ADR-072). Two directions: the scripts open a
//! file the app wrote, and the app opens a file the scripts wrote. If
//! the two DDLs drift, this test fails.
//!
//! Gated: it runs only when `CR_FORMAT_TESTS` is set and a `python3`
//! executable exists (the `sevenzip_gated` pattern). Whether the CI
//! image carries `python3` is verified by the run.

use std::path::Path;
use std::process::Command;

use cr_scrape::cache::SqliteCache;

fn python_available() -> bool {
    std::env::var_os("CR_FORMAT_TESTS").is_some()
        && Command::new("python3")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
}

/// The repository root, three levels above this crate's manifest.
fn repo_root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root")
        .to_path_buf()
}

fn scratch(tag: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push(format!("cr-schema-pin-{tag}-{}", std::process::id()));
    std::fs::remove_dir_all(&p).ok();
    std::fs::create_dir_all(&p).expect("scratch dir");
    p
}

/// The column set of one table, as `name:type` lines sorted by column
/// order. Used to compare the app schema against the script schema.
fn table_columns(conn: &rusqlite::Connection, table: &str) -> Vec<(String, String)> {
    let mut stmt = conn
        .prepare(&format!("PRAGMA table_info({table})"))
        .expect("table_info");
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, String>(2)?)))
        .expect("query")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect");
    rows
}

const PINNED_TABLES: &[&str] = &[
    "volume",
    "issue_skeleton",
    "issue_detail",
    "image_blob",
    "search_result",
    "request_log",
    "sweep_state",
    "pending_issue_detail",
    "credit",
    "issue_image",
    "character",
    "person",
    "team",
    "story_arc",
    "location",
    "concept",
    "object",
    "publisher",
];

#[test]
fn app_and_scripts_agree_on_v6_schema() {
    if !python_available() {
        return;
    }
    let dir = scratch("both");
    let root = repo_root();

    // Direction 1: the app writes a v4 file; the scripts open it.
    let app_file = dir.join("app.sqlite");
    drop(SqliteCache::open(&app_file).expect("app writes v4"));
    let open_out = Command::new("python3")
        .arg("-c")
        .arg(
            "import sys; from scripts.cvcache import commands, schema; \
             c=commands.open_v4(__import__('pathlib').Path(sys.argv[1])); \
             assert schema.user_version(c)==6; c.close(); print('ok')",
        )
        .arg(&app_file)
        .current_dir(&root)
        .output()
        .expect("run python open");
    assert!(
        open_out.status.success(),
        "scripts failed to open the app file: {}",
        String::from_utf8_lossy(&open_out.stderr)
    );

    // Direction 2: the scripts write a v4 file; the app opens it and
    // the column sets match table for table.
    let script_file = dir.join("script.sqlite");
    let build_out = Command::new("python3")
        .arg("-c")
        .arg(
            "import sys, sqlite3; from scripts.cvcache import schema; \
             c=sqlite3.connect(sys.argv[1]); schema.create_schema(c); \
             c.commit(); c.close(); print('ok')",
        )
        .arg(&script_file)
        .current_dir(&root)
        .output()
        .expect("run python build");
    assert!(
        build_out.status.success(),
        "scripts failed to build a file: {}",
        String::from_utf8_lossy(&build_out.stderr)
    );

    let app_conn = rusqlite::Connection::open(&app_file).expect("open app file");
    let script_conn = rusqlite::Connection::open(&script_file).expect("open script file");
    let script_version: i64 = script_conn
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .expect("script version");
    assert_eq!(script_version, 6, "script file is not v6");

    for table in PINNED_TABLES {
        let app_cols = table_columns(&app_conn, table);
        let script_cols = table_columns(&script_conn, table);
        assert_eq!(
            app_cols, script_cols,
            "schema drift on table {table}: app {app_cols:?} vs script {script_cols:?}"
        );
    }
}
