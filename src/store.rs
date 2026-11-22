//! SQLite metadata store. Sync writes *_tech / sync_run; manual content lives in separate tables (M2+).
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use std::{path::Path, sync::Mutex};

pub type Db = Mutex<Connection>;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS setting (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS object_tech (
  key TEXT PRIMARY KEY, scope TEXT NOT NULL, name TEXT NOT NULL, kind TEXT NOT NULL,
  comment TEXT, definition TEXT, props TEXT NOT NULL DEFAULT '{}',
  first_seen TEXT NOT NULL, synced_at TEXT NOT NULL, is_deleted INTEGER NOT NULL DEFAULT 0);
CREATE INDEX IF NOT EXISTS object_scope ON object_tech(scope);
CREATE TABLE IF NOT EXISTS column_tech (
  key TEXT PRIMARY KEY, object_key TEXT NOT NULL, name TEXT NOT NULL, ordinal INTEGER NOT NULL,
  data_type TEXT NOT NULL, nullable INTEGER NOT NULL, comment TEXT, props TEXT NOT NULL DEFAULT '{}',
  first_seen TEXT NOT NULL, is_deleted INTEGER NOT NULL DEFAULT 0);
CREATE INDEX IF NOT EXISTS column_object ON column_tech(object_key);
CREATE TABLE IF NOT EXISTS sync_run (
  id INTEGER PRIMARY KEY, scope TEXT NOT NULL, trigger TEXT NOT NULL, started_at TEXT NOT NULL, finished_at TEXT,
  status TEXT NOT NULL, error TEXT, summary TEXT, baseline INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS change_log (
  id INTEGER PRIMARY KEY, entity TEXT NOT NULL, key TEXT NOT NULL, field TEXT NOT NULL,
  before TEXT, after TEXT, source TEXT NOT NULL, actor TEXT NOT NULL, at TEXT NOT NULL, run_id INTEGER);
CREATE INDEX IF NOT EXISTS change_key ON change_log(key);
CREATE INDEX IF NOT EXISTS change_run ON change_log(run_id);
CREATE TABLE IF NOT EXISTS todo (
  id INTEGER PRIMARY KEY, kind TEXT NOT NULL, key TEXT NOT NULL, reason TEXT, status TEXT NOT NULL DEFAULT 'open',
  handler TEXT, handled_at TEXT, created_at TEXT NOT NULL, run_id INTEGER);
CREATE UNIQUE INDEX IF NOT EXISTS todo_open ON todo(kind, key) WHERE status = 'open';
-- Manual business content (written only through the UI / import; sync never touches these).
CREATE TABLE IF NOT EXISTS object_biz (
  key TEXT PRIMARY KEY, cn_name TEXT, description TEXT, product TEXT, layer TEXT, grain TEXT, primary_key TEXT,
  source_tables TEXT, refresh_method TEXT, maintainer TEXT, is_temporary INTEGER, date_column TEXT, updated_at TEXT);
CREATE TABLE IF NOT EXISTS column_biz (
  key TEXT PRIMARY KEY, meaning TEXT, unit TEXT, currency TEXT, timezone TEXT, is_sensitive INTEGER, notes TEXT, updated_at TEXT);
CREATE TABLE IF NOT EXISTS rule (
  id INTEGER PRIMARY KEY, target_key TEXT NOT NULL, kind TEXT NOT NULL, value TEXT, condition TEXT, note TEXT, updated_at TEXT);
CREATE INDEX IF NOT EXISTS rule_target ON rule(target_key);
CREATE TABLE IF NOT EXISTS relation (
  id INTEGER PRIMARY KEY, left_key TEXT NOT NULL, right_key TEXT NOT NULL, key_pairs TEXT NOT NULL, kind TEXT,
  note TEXT, diff_note TEXT, status TEXT NOT NULL DEFAULT 'confirmed', origin TEXT, updated_at TEXT);
CREATE UNIQUE INDEX IF NOT EXISTS relation_pair ON relation(left_key, right_key, key_pairs);
CREATE TABLE IF NOT EXISTS flow_step (
  id INTEGER PRIMARY KEY, product TEXT NOT NULL, flow TEXT NOT NULL, step_no INTEGER NOT NULL, name TEXT NOT NULL,
  object_key TEXT, columns TEXT, note TEXT, updated_at TEXT);
CREATE TABLE IF NOT EXISTS metric (
  id INTEGER PRIMARY KEY, name_cn TEXT NOT NULL, name_en TEXT, definition TEXT, formula TEXT, numerator TEXT, denominator TEXT,
  object_key TEXT, column_name TEXT, used_by TEXT, builtin INTEGER NOT NULL DEFAULT 0, updated_at TEXT);
-- Derived data (rebuilt by jobs / parsers).
CREATE TABLE IF NOT EXISTS freshness (key TEXT PRIMARY KEY, date_column TEXT, max_date TEXT, checked_at TEXT, error TEXT);
CREATE TABLE IF NOT EXISTS lineage (
  id INTEGER PRIMARY KEY, source TEXT NOT NULL, origin TEXT NOT NULL, upstream TEXT NOT NULL, downstream TEXT NOT NULL,
  upstream_column TEXT, downstream_column TEXT, expression TEXT, parsed_at TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS lineage_up ON lineage(upstream);
CREATE INDEX IF NOT EXISTS lineage_down ON lineage(downstream);
CREATE INDEX IF NOT EXISTS lineage_origin ON lineage(source, origin);
CREATE TABLE IF NOT EXISTS script (path TEXT PRIMARY KEY, folder TEXT, modified TEXT, writes TEXT, reads TEXT, error TEXT, parsed_at TEXT);
-- Nodes that are not database objects (MyBI datasets, reports, scheduled tasks).
CREATE TABLE IF NOT EXISTS node (key TEXT PRIMARY KEY, name TEXT NOT NULL, kind TEXT NOT NULL, info TEXT NOT NULL DEFAULT '{}');
";

pub const NOW: &str = "strftime('%Y-%m-%dT%H:%M:%SZ','now')";

pub fn open(path: &Path) -> Result<Connection, String> {
    let db = Connection::open(path).map_err(|e| e.to_string())?;
    db.execute_batch(&format!("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON; {SCHEMA}")).map_err(|e| e.to_string())?;
    // Columns added after M1: (table, column, type)
    for (table, column, kind) in [("object_tech", "naming", "TEXT NOT NULL DEFAULT '{}'")] {
        let exists: bool = db.query_row(&format!("SELECT EXISTS(SELECT 1 FROM pragma_table_info('{table}') WHERE name = '{column}')"), [], |r| r.get(0)).map_err(|e| e.to_string())?;
        if !exists { db.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {kind}")).map_err(|e| e.to_string())?; }
    }
    Ok(db)
}

pub fn setting(db: &Connection, key: &str) -> Option<Value> {
    db.query_row("SELECT value FROM setting WHERE key = ?", [key], |row| row.get::<_, String>(0)).optional().ok().flatten()
        .and_then(|text| serde_json::from_str(&text).ok())
}

pub fn set_setting(db: &Connection, key: &str, value: &Value) -> Result<(), String> {
    db.execute("INSERT INTO setting(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", params![key, value.to_string()])
        .map(|_| ()).map_err(|e| e.to_string())
}
