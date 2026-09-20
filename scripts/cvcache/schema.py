"""The v6 cvcache schema, pinned to `SCHEMA_V6` in
`crates/cr-scrape/src/cache/sqlite.rs`.

A file the app wrote and a file a script wrote must open in both
directions, so the DDL here mirrors the Rust migration chain
(`migrate_connection`). The schema pin test drives both directions.
"""

import sqlite3

SCHEMA_VERSION = 6

# The migration chain of `migrate_connection`, flattened to the v4
# end state. `CREATE TABLE IF NOT EXISTS` matches the Rust arms.
_SCHEMA_V4_DDL = """
CREATE TABLE IF NOT EXISTS volume (
    volume_id         INTEGER PRIMARY KEY,
    name              TEXT,
    publisher         TEXT,
    start_year        INTEGER,
    count_of_issues   INTEGER,
    date_last_updated TEXT,
    last_cover_date   TEXT,
    fetched_at        INTEGER NOT NULL DEFAULT 0,
    detail_json       TEXT,
    aliases           TEXT,
    deck              TEXT,
    description       TEXT,
    image_url         TEXT,
    api_detail_url    TEXT,
    site_detail_url   TEXT,
    date_added        TEXT,
    first_issue_id    INTEGER,
    last_issue_id     INTEGER
);

CREATE TABLE IF NOT EXISTS issue_skeleton (
    issue_id          INTEGER PRIMARY KEY,
    volume_id         INTEGER NOT NULL,
    issue_number      TEXT NOT NULL,
    cover_date        TEXT,
    name              TEXT,
    deck              TEXT,
    description       TEXT,
    store_date        TEXT,
    image_url         TEXT,
    date_added        TEXT,
    date_last_updated TEXT,
    api_detail_url    TEXT,
    site_detail_url   TEXT,
    fetched_at        INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS issue_skeleton_volume
    ON issue_skeleton (volume_id, issue_id);

CREATE TABLE IF NOT EXISTS issue_detail (
    issue_id          INTEGER PRIMARY KEY,
    json              TEXT NOT NULL,
    fetched_at        INTEGER NOT NULL,
    volume_id         INTEGER,
    issue_number      TEXT,
    cover_date        TEXT,
    name              TEXT,
    store_date        TEXT,
    image_url         TEXT,
    date_added        TEXT,
    date_last_updated TEXT
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

CREATE TABLE IF NOT EXISTS pending_issue_detail (
    volume_id INTEGER NOT NULL,
    issue_id  INTEGER NOT NULL,
    position  INTEGER NOT NULL,
    PRIMARY KEY (volume_id, issue_id)
);
CREATE INDEX IF NOT EXISTS pending_issue_detail_order
    ON pending_issue_detail (volume_id, position);

CREATE TABLE IF NOT EXISTS credit (
    owner_kind    TEXT NOT NULL,
    owner_id      INTEGER NOT NULL,
    resource_kind TEXT NOT NULL,
    resource_id   INTEGER NOT NULL,
    name          TEXT,
    role          TEXT,
    marker        TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS credit_natural
    ON credit (owner_kind, owner_id, resource_kind, resource_id,
               COALESCE(name, ''), COALESCE(role, ''), marker);
CREATE INDEX IF NOT EXISTS credit_owner
    ON credit (owner_kind, owner_id);

CREATE TABLE IF NOT EXISTS issue_image (
    image_id     INTEGER PRIMARY KEY,
    issue_id     INTEGER NOT NULL,
    original_url TEXT NOT NULL,
    caption      TEXT,
    image_tags   TEXT,
    fetched_at   INTEGER NOT NULL DEFAULT 0,
    ahash        TEXT,
    dhash        TEXT,
    phash        TEXT
);
CREATE INDEX IF NOT EXISTS issue_image_issue
    ON issue_image (issue_id);
CREATE INDEX IF NOT EXISTS issue_image_ahash
    ON issue_image (ahash);
CREATE INDEX IF NOT EXISTS issue_image_phash
    ON issue_image (phash);
"""

# One table per related resource. The columns match RESOURCE_COLUMNS in
# import.rs.
RESOURCE_TABLES = (
    "character",
    "person",
    "team",
    "story_arc",
    "location",
    "concept",
    "object",
    "publisher",
)

_RESOURCE_DDL = """
CREATE TABLE IF NOT EXISTS {name} (
    id                INTEGER PRIMARY KEY,
    name              TEXT,
    image_url         TEXT,
    date_last_updated TEXT,
    date_added        TEXT,
    fetched_at        INTEGER,
    detail_json       TEXT
);
"""


def create_schema(conn: sqlite3.Connection) -> None:
    """Creates the v4 schema on a fresh connection and stamps
    `user_version`."""
    conn.executescript(_SCHEMA_V4_DDL)
    for name in RESOURCE_TABLES:
        conn.executescript(_RESOURCE_DDL.format(name=name))
    conn.execute(f"PRAGMA user_version = {SCHEMA_VERSION}")


def user_version(conn: sqlite3.Connection) -> int:
    return int(conn.execute("PRAGMA user_version").fetchone()[0])


def require_supported_version(conn: sqlite3.Connection) -> None:
    """Rejects a file whose schema is newer than this build knows. A
    file the app wrote at v4 opens; a hypothetical newer file is the
    app's business, so the scripts refuse it (ADR-069)."""
    version = user_version(conn)
    if version > SCHEMA_VERSION:
        raise ValueError(
            f"cache schema v{version} is newer than script v{SCHEMA_VERSION}"
        )
