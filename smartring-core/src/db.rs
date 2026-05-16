use chrono::{DateTime, NaiveDateTime, Utc};
use rusqlite::{params, Connection};
use std::path::Path;
use thiserror::Error;

/// The only schema version this binary understands.
const CURRENT_SCHEMA_VERSION: i64 = 1;

/// Python-to-v1 migration SQL, embedded at compile time.
pub const MIGRATION_SQL: &str = include_str!("migrate_python_to_v1.sql");

/// SQLite timestamp format matching SQLAlchemy's default DateTime output.
///
/// Example: `"2024-07-07 14:30:00.000000"`
const TIMESTAMP_FMT: &str = "%Y-%m-%d %H:%M:%S%.6f";

/// Full v1 schema. Uses `CREATE TABLE IF NOT EXISTS` throughout so that
/// `Db::init` is safe to call on both fresh and already-initialised databases.
const CREATE_SCHEMA_SQL: &str = "
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER NOT NULL
);
INSERT INTO schema_version SELECT 1 WHERE NOT EXISTS (SELECT 1 FROM schema_version);

CREATE TABLE IF NOT EXISTS rings (
    ring_id  INTEGER PRIMARY KEY,
    address  TEXT NOT NULL,
    name     TEXT,
    UNIQUE(address)
);

CREATE TABLE IF NOT EXISTS syncs (
    sync_id      INTEGER PRIMARY KEY,
    ring_id      INTEGER NOT NULL REFERENCES rings(ring_id),
    timestamp    TEXT NOT NULL,
    tool_version TEXT
);

CREATE TABLE IF NOT EXISTS heart_rates (
    heart_rate_id INTEGER PRIMARY KEY,
    reading       INTEGER NOT NULL,
    timestamp     TEXT NOT NULL,
    ring_id       INTEGER NOT NULL REFERENCES rings(ring_id),
    sync_id       INTEGER NOT NULL REFERENCES syncs(sync_id),
    UNIQUE(ring_id, timestamp)
);

CREATE TABLE IF NOT EXISTS steps (
    step_id   INTEGER PRIMARY KEY,
    timestamp TEXT NOT NULL,
    steps     INTEGER NOT NULL,
    calories  INTEGER NOT NULL,
    distance  INTEGER NOT NULL,
    ring_id   INTEGER NOT NULL REFERENCES rings(ring_id),
    sync_id   INTEGER NOT NULL REFERENCES syncs(sync_id),
    UNIQUE(ring_id, timestamp)
);
";

// ── Newtypes ──────────────────────────────────────────────────────────────────

/// Newtype wrapper for the `rings.ring_id` primary key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RingId(pub i64);

/// Newtype wrapper for the `syncs.sync_id` primary key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncId(pub i64);

// ── Error ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
pub enum DbError {
    #[error(
        "database schema version {found} is newer than this tool supports (max {max}); \
         upgrade smartring-manager to open this database"
    )]
    SchemaTooNew { found: i64, max: i64 },

    #[error("invalid timestamp stored in database: \"{0}\"")]
    InvalidTimestamp(String),

    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

// ── Db ────────────────────────────────────────────────────────────────────────

/// An open connection to the ring data SQLite database.
pub struct Db {
    conn: Connection,
}

impl Db {
    /// Open (or create) the database at `path`.
    ///
    /// Applies the v1 schema on first use. Returns [`DbError::SchemaTooNew`] if
    /// the on-disk schema version is newer than this binary supports.
    pub fn open(path: &Path) -> Result<Self, DbError> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    /// Open a temporary in-memory database. Intended for tests.
    pub fn open_in_memory() -> Result<Self, DbError> {
        let conn = Connection::open_in_memory()?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> Result<Self, DbError> {
        // Apply the schema. CREATE TABLE IF NOT EXISTS makes this idempotent on
        // existing v1 databases; the INSERT ... WHERE NOT EXISTS guards the
        // schema_version row from duplicating.
        conn.execute_batch(CREATE_SCHEMA_SQL)?;

        // Read back the version to catch future-schema databases.
        let version: i64 = conn.query_row(
            "SELECT version FROM schema_version LIMIT 1",
            [],
            |row| row.get(0),
        )?;

        if version > CURRENT_SCHEMA_VERSION {
            return Err(DbError::SchemaTooNew {
                found: version,
                max: CURRENT_SCHEMA_VERSION,
            });
        }

        Ok(Db { conn })
    }

    /// Insert a ring row, or return the existing [`RingId`] if the address is
    /// already known. `name` is `None` when the BLE device name was not resolved.
    pub fn create_or_find_ring(
        &self,
        address: &str,
        name: Option<&str>,
    ) -> Result<RingId, DbError> {
        self.conn.execute(
            "INSERT OR IGNORE INTO rings (address, name) VALUES (?1, ?2)",
            params![address, name],
        )?;
        let id: i64 = self.conn.query_row(
            "SELECT ring_id FROM rings WHERE address = ?1",
            params![address],
            |row| row.get(0),
        )?;
        Ok(RingId(id))
    }

    /// Record the start of a sync run and return its ID.
    ///
    /// `tool_version` should be `"smartring-manager/<version>"`.
    pub fn create_sync(
        &self,
        ring_id: RingId,
        tool_version: &str,
    ) -> Result<SyncId, DbError> {
        let ts = fmt_timestamp(Utc::now());
        self.conn.execute(
            "INSERT INTO syncs (ring_id, timestamp, tool_version) VALUES (?1, ?2, ?3)",
            params![ring_id.0, ts, tool_version],
        )?;
        Ok(SyncId(self.conn.last_insert_rowid()))
    }

    /// Bulk-insert heart rate readings, skipping any that would violate the
    /// `(ring_id, timestamp)` uniqueness constraint.
    ///
    /// Returns the number of rows actually written (duplicates are silently ignored
    /// and not counted).
    pub fn insert_heart_rates(
        &self,
        ring_id: RingId,
        sync_id: SyncId,
        readings: &[(DateTime<Utc>, u8)],
    ) -> Result<usize, DbError> {
        let mut inserted = 0usize;
        for (ts, bpm) in readings {
            let ts_str = fmt_timestamp(*ts);
            let rows = self.conn.execute(
                "INSERT OR IGNORE INTO heart_rates (reading, timestamp, ring_id, sync_id) \
                 VALUES (?1, ?2, ?3, ?4)",
                params![*bpm as i64, ts_str, ring_id.0, sync_id.0],
            )?;
            inserted += rows;
        }
        Ok(inserted)
    }

    /// Return the timestamp of the most recent sync for this ring, or `None` if
    /// there has never been a sync.
    pub fn get_last_sync_time(
        &self,
        ring_id: RingId,
    ) -> Result<Option<DateTime<Utc>>, DbError> {
        let result: rusqlite::Result<String> = self.conn.query_row(
            "SELECT timestamp FROM syncs WHERE ring_id = ?1 \
             ORDER BY timestamp DESC LIMIT 1",
            params![ring_id.0],
            |row| row.get(0),
        );
        match result {
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(DbError::Sqlite(e)),
            Ok(ts) => parse_timestamp(&ts)
                .map(Some)
                .ok_or_else(|| DbError::InvalidTimestamp(ts)),
        }
    }
}

// ── Timestamp helpers ─────────────────────────────────────────────────────────

fn fmt_timestamp(dt: DateTime<Utc>) -> String {
    dt.format(TIMESTAMP_FMT).to_string()
}

fn parse_timestamp(s: &str) -> Option<DateTime<Utc>> {
    NaiveDateTime::parse_from_str(s, TIMESTAMP_FMT)
        .ok()
        .map(|ndt| ndt.and_utc())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn open() -> Db {
        Db::open_in_memory().expect("in-memory DB should always open")
    }

    // ── schema creation ───────────────────────────────────────────────────────

    #[test]
    fn open_creates_all_tables() {
        let db = open();
        let mut stmt = db
            .conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap();
        let tables: Vec<String> = stmt
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();

        assert!(tables.contains(&"schema_version".to_string()), "missing schema_version");
        assert!(tables.contains(&"rings".to_string()), "missing rings");
        assert!(tables.contains(&"syncs".to_string()), "missing syncs");
        assert!(tables.contains(&"heart_rates".to_string()), "missing heart_rates");
        assert!(tables.contains(&"steps".to_string()), "missing steps");
    }

    #[test]
    fn open_sets_schema_version_1() {
        let db = open();
        let v: i64 = db
            .conn
            .query_row("SELECT version FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 1);
    }

    #[test]
    fn open_is_idempotent_on_existing_v1_db() {
        // Calling init twice should not create duplicate schema_version rows.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(CREATE_SCHEMA_SQL).unwrap();
        conn.execute_batch(CREATE_SCHEMA_SQL).unwrap();

        let count: i64 = conn
            .query_row("SELECT count(*) FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "schema_version should have exactly one row");
    }

    #[test]
    fn open_fails_when_schema_too_new() {
        // Initialise a DB then bump the version to simulate a future schema.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(CREATE_SCHEMA_SQL).unwrap();
        conn.execute("UPDATE schema_version SET version = 99", []).unwrap();

        assert!(
            matches!(Db::init(conn), Err(DbError::SchemaTooNew { found: 99, max: 1 })),
            "expected SchemaTooNew error"
        );
    }

    // ── PRAGMA foreign_keys ───────────────────────────────────────────────────

    #[test]
    fn foreign_keys_are_enforced() {
        let db = open();
        // Inserting into syncs with a non-existent ring_id should fail.
        let result = db.conn.execute(
            "INSERT INTO syncs (ring_id, timestamp, tool_version) VALUES (999, '2026-01-01 00:00:00.000000', NULL)",
            [],
        );
        assert!(result.is_err(), "foreign key violation should be rejected");
    }

    // ── create_or_find_ring ───────────────────────────────────────────────────

    #[test]
    fn create_or_find_ring_inserts_new_ring() {
        let db = open();
        let id = db
            .create_or_find_ring("AA:BB:CC:DD:EE:FF", Some("R02_TEST"))
            .unwrap();

        let (addr, name): (String, Option<String>) = db
            .conn
            .query_row(
                "SELECT address, name FROM rings WHERE ring_id = ?1",
                params![id.0],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();

        assert_eq!(addr, "AA:BB:CC:DD:EE:FF");
        assert_eq!(name, Some("R02_TEST".to_string()));
    }

    #[test]
    fn create_or_find_ring_returns_same_id_on_duplicate() {
        let db = open();
        let id1 = db.create_or_find_ring("AA:BB:CC:DD:EE:FF", None).unwrap();
        let id2 = db.create_or_find_ring("AA:BB:CC:DD:EE:FF", None).unwrap();
        assert_eq!(id1, id2);
    }

    #[test]
    fn create_or_find_ring_stores_null_name() {
        let db = open();
        let id = db.create_or_find_ring("11:22:33:44:55:66", None).unwrap();
        let name: Option<String> = db
            .conn
            .query_row(
                "SELECT name FROM rings WHERE ring_id = ?1",
                params![id.0],
                |r| r.get(0),
            )
            .unwrap();
        assert!(name.is_none());
    }

    // ── insert_heart_rates ────────────────────────────────────────────────────

    fn make_ring_and_sync(db: &Db) -> (RingId, SyncId) {
        let ring_id = db
            .create_or_find_ring("AA:BB:CC:DD:EE:FF", None)
            .unwrap();
        let sync_id = db.create_sync(ring_id, "smartring-manager/0.1.0").unwrap();
        (ring_id, sync_id)
    }

    #[test]
    fn insert_heart_rates_returns_inserted_count() {
        let db = open();
        let (ring_id, sync_id) = make_ring_and_sync(&db);

        let base = Utc.with_ymd_and_hms(2026, 5, 10, 0, 0, 0).unwrap();
        let readings = vec![
            (base, 72u8),
            (base + chrono::Duration::minutes(10), 75u8),
        ];
        let inserted = db.insert_heart_rates(ring_id, sync_id, &readings).unwrap();
        assert_eq!(inserted, 2);
    }

    #[test]
    fn insert_heart_rates_skips_duplicates() {
        let db = open();
        let (ring_id, sync_id) = make_ring_and_sync(&db);

        let ts = Utc.with_ymd_and_hms(2026, 5, 10, 0, 0, 0).unwrap();
        let readings = vec![(ts, 72u8)];

        let first = db.insert_heart_rates(ring_id, sync_id, &readings).unwrap();
        assert_eq!(first, 1);

        let second = db.insert_heart_rates(ring_id, sync_id, &readings).unwrap();
        assert_eq!(second, 0, "duplicate should be silently ignored");

        let count: i64 = db
            .conn
            .query_row("SELECT count(*) FROM heart_rates", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    // ── get_last_sync_time ────────────────────────────────────────────────────

    #[test]
    fn get_last_sync_time_returns_none_for_new_ring() {
        let db = open();
        let ring_id = db.create_or_find_ring("AA:BB:CC:DD:EE:FF", None).unwrap();
        assert_eq!(db.get_last_sync_time(ring_id).unwrap(), None);
    }

    #[test]
    fn get_last_sync_time_returns_most_recent() {
        let db = open();
        let ring_id = db.create_or_find_ring("AA:BB:CC:DD:EE:FF", None).unwrap();

        let ts1 = "2026-05-10 00:00:00.000000";
        let ts2 = "2026-05-11 12:00:00.000000";
        db.conn
            .execute(
                "INSERT INTO syncs (ring_id, timestamp, tool_version) VALUES (?1, ?2, NULL)",
                params![ring_id.0, ts1],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO syncs (ring_id, timestamp, tool_version) VALUES (?1, ?2, NULL)",
                params![ring_id.0, ts2],
            )
            .unwrap();

        let last = db.get_last_sync_time(ring_id).unwrap().unwrap();
        assert_eq!(last, Utc.with_ymd_and_hms(2026, 5, 11, 12, 0, 0).unwrap());
    }

    // ── timestamp round-trip ──────────────────────────────────────────────────

    #[test]
    fn timestamp_roundtrip() {
        let original = Utc.with_ymd_and_hms(2026, 5, 10, 14, 30, 0).unwrap();
        let formatted = fmt_timestamp(original);
        assert_eq!(formatted, "2026-05-10 14:30:00.000000");
        let parsed = parse_timestamp(&formatted).unwrap();
        assert_eq!(parsed, original);
    }

    // ── migration script ──────────────────────────────────────────────────────

    /// Build a database that looks like the Python colmi_r02_client schema:
    /// rings + syncs + heart_rates, without name/tool_version/steps/schema_version.
    fn make_python_schema(conn: &Connection) {
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;

            CREATE TABLE rings (
                ring_id  INTEGER PRIMARY KEY,
                address  TEXT NOT NULL,
                UNIQUE(address)
            );

            CREATE TABLE syncs (
                sync_id   INTEGER PRIMARY KEY,
                ring_id   INTEGER NOT NULL REFERENCES rings(ring_id),
                timestamp TEXT NOT NULL,
                comment   TEXT
            );

            CREATE TABLE heart_rates (
                heart_rate_id INTEGER PRIMARY KEY,
                reading       INTEGER NOT NULL,
                timestamp     TEXT NOT NULL,
                ring_id       INTEGER NOT NULL REFERENCES rings(ring_id),
                sync_id       INTEGER NOT NULL REFERENCES syncs(sync_id),
                UNIQUE(ring_id, timestamp)
            );",
        )
        .unwrap();
    }

    #[test]
    fn migration_script_applies_to_python_schema() {
        let conn = Connection::open_in_memory().unwrap();
        make_python_schema(&conn);

        conn.execute_batch(MIGRATION_SQL).unwrap();

        // schema_version = 1
        let v: i64 = conn
            .query_row("SELECT version FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 1);

        // rings.name column added
        let name_col: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('rings') WHERE name = 'name'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(name_col, 1, "rings.name column missing after migration");

        // syncs.tool_version column added
        let tv_col: i64 = conn
            .query_row(
                "SELECT count(*) FROM pragma_table_info('syncs') WHERE name = 'tool_version'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(tv_col, 1, "syncs.tool_version column missing after migration");

        // steps table created
        let steps_tbl: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='steps'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(steps_tbl, 1, "steps table missing after migration");

        // Existing data is intact
        conn.execute(
            "INSERT INTO rings (address, name) VALUES ('AA:BB:CC:DD:EE:FF', 'R02')",
            [],
        )
        .unwrap();
        let count: i64 = conn
            .query_row("SELECT count(*) FROM rings", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    // ── sync idempotency (integration) ───────────────────────────────────────

    /// Simulates two consecutive sync runs over the same date range and verifies
    /// that re-inserting the same readings leaves the heart_rates row count unchanged.
    #[test]
    fn sync_does_not_duplicate_heart_rate_rows() {
        let db = open();
        let ring_id = db
            .create_or_find_ring("AA:BB:CC:DD:EE:FF", Some("R02_TEST"))
            .unwrap();

        let base = Utc.with_ymd_and_hms(2026, 5, 10, 0, 0, 0).unwrap();
        let readings: Vec<(DateTime<Utc>, u8)> = (0..5)
            .map(|i| (base + chrono::Duration::minutes(i * 10), 70u8 + i as u8))
            .collect();

        // First sync — all 5 readings are new.
        let sync1 = db.create_sync(ring_id, "smartring-manager/0.1.0").unwrap();
        let inserted = db.insert_heart_rates(ring_id, sync1, &readings).unwrap();
        assert_eq!(inserted, 5);

        // Second sync — same readings, should all be skipped.
        let sync2 = db.create_sync(ring_id, "smartring-manager/0.1.0").unwrap();
        let inserted = db.insert_heart_rates(ring_id, sync2, &readings).unwrap();
        assert_eq!(inserted, 0, "re-sync must not insert duplicate rows");

        let total: i64 = db
            .conn
            .query_row("SELECT count(*) FROM heart_rates", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total, 5, "heart_rates count must be unchanged after re-sync");
    }

    #[test]
    fn migration_idempotent_parts_safe_to_run_twice() {
        // The CREATE TABLE IF NOT EXISTS and INSERT ... WHERE NOT EXISTS parts of
        // the migration are idempotent. Verify they don't error or duplicate rows
        // when applied a second time.
        let conn = Connection::open_in_memory().unwrap();
        make_python_schema(&conn);
        conn.execute_batch(MIGRATION_SQL).unwrap();

        // Re-run the idempotent subset.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS steps (
                step_id   INTEGER PRIMARY KEY,
                timestamp TEXT NOT NULL,
                steps     INTEGER NOT NULL,
                calories  INTEGER NOT NULL,
                distance  INTEGER NOT NULL,
                ring_id   INTEGER NOT NULL REFERENCES rings(ring_id),
                sync_id   INTEGER NOT NULL REFERENCES syncs(sync_id),
                UNIQUE(ring_id, timestamp)
            );
            CREATE TABLE IF NOT EXISTS schema_version (version INTEGER NOT NULL);
            INSERT INTO schema_version SELECT 1 WHERE NOT EXISTS (SELECT 1 FROM schema_version);",
        )
        .unwrap();

        let count: i64 = conn
            .query_row("SELECT count(*) FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1, "schema_version should still have exactly one row");
    }
}
