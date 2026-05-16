-- migrate_python_to_v1.sql
-- Run against an existing colmi_r02_client database to bring it up to the Rust schema.
--
-- The CREATE TABLE IF NOT EXISTS and INSERT ... WHERE NOT EXISTS statements are
-- idempotent and safe to re-run. The ALTER TABLE statements are one-time operations
-- and will error if the column already exists — run this script only once per database.
--
-- Usage:
--   sqlite3 ring_data.sqlite < migrate_python_to_v1.sql

PRAGMA foreign_keys = ON;

ALTER TABLE rings ADD COLUMN name TEXT;

ALTER TABLE syncs ADD COLUMN tool_version TEXT;

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

CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER NOT NULL
);
INSERT INTO schema_version SELECT 1 WHERE NOT EXISTS (SELECT 1 FROM schema_version);
