//! Syncs technical metadata for each configured scope and records the differences.
use crate::{catalog, secrets, store::{self, NOW}, AppState};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Deserialize, Serialize, PartialEq)]
pub struct Scope { pub profile: String, pub schema: String }

/// Configured scopes; defaults to MyQuery's browser scopes until the user saves their own.
pub fn scopes(db: &Connection) -> Vec<Scope> {
    if let Some(saved) = store::setting(db, "scopes").and_then(|v| serde_json::from_value(v).ok()) { return saved; }
    myquery_core::config::load().map(|s| s.browser_scopes.into_iter().map(|b| Scope { profile: b.profile, schema: b.schema }).collect()).unwrap_or_default()
}

pub fn interval_minutes(db: &Connection) -> u64 {
    store::setting(db, "sync_interval_minutes").and_then(|v| v.as_u64()).filter(|&m| m > 0).unwrap_or(60)
}

/// Finds the configured scope for a "database.schema" label and its MyQuery connection profile.
pub fn scope_profile(db: &Connection, label: &str) -> Result<(myquery_core::config::Profile, String), String> {
    let profiles = myquery_core::config::load()?.profiles;
    for scope in scopes(db) {
        if let Some(p) = profiles.iter().find(|p| p.name == scope.profile && format!("{}.{}", p.database, scope.schema) == label) { return Ok((p.clone(), scope.schema)); }
    }
    Err(format!("同步范围中没有 {label}"))
}

/// Opens a session for the scope that contains an object / column key ("db.schema.table[.column]").
pub async fn connect_key(state: &AppState, key: &str) -> Result<(myquery_core::db::Session, myquery_core::config::Profile, String), String> {
    let label = key.splitn(3, '.').take(2).collect::<Vec<_>>().join(".");
    let (profile, schema) = { let db = state.db.lock().unwrap(); scope_profile(&db, &label)? };
    let session = myquery_core::db::connect(&profile, &secrets::password(&profile.name)?).await?;
    Ok((session, profile, schema))
}

/// Syncs all scopes (or one). Runs one at a time; a second request waits for the first.
pub async fn run(state: &AppState, only: Option<Scope>, trigger: &str) -> Vec<i64> {
    let _guard = state.sync_lock.lock().await;
    let targets: Vec<Scope> = { let db = state.db.lock().unwrap(); scopes(&db) }.into_iter().filter(|s| only.as_ref().is_none_or(|o| o == s)).collect();
    let mut ids = Vec::new();
    for scope in targets {
        let profile = myquery_core::config::load().ok().and_then(|s| s.profiles.into_iter().find(|p| p.name == scope.profile));
        let label = profile.as_ref().map_or_else(|| format!("{}.{}", scope.profile, scope.schema), |p| format!("{}.{}", p.database, scope.schema));
        let id = {
            let db = state.db.lock().unwrap();
            db.execute(&format!("INSERT INTO sync_run(scope, trigger, started_at, status) VALUES (?1, ?2, {NOW}, 'running')"), params![label, trigger]).ok();
            db.last_insert_rowid()
        };
        let result = async {
            let profile = profile.ok_or_else(|| format!("MyQuery 中找不到连接“{}”", scope.profile))?;
            let password = secrets::password(&profile.name)?;
            let session = myquery_core::db::connect(&profile, &password).await?;
            catalog::read(&session.client, &scope.schema, profile.redshift).await
        }.await;
        let mut db = state.db.lock().unwrap();
        let outcome = result.and_then(|catalog| apply(&mut db, id, &label, catalog));
        let (status, error, summary) = match outcome {
            Ok(summary) => ("ok", None, Some(summary.to_string())),
            Err(error) => ("error", Some(error), None),
        };
        if let Some(error) = &error { eprintln!("sync {label} failed: {error}"); }
        db.execute(&format!("UPDATE sync_run SET status = ?1, error = ?2, summary = ?3, finished_at = {NOW} WHERE id = ?4"), params![status, error, summary, id]).ok();
        ids.push(id);
    }
    if !ids.is_empty() {
        let db = state.db.lock().unwrap();
        let summary = crate::lineage::rebuild_all(&db);
        println!("lineage: {summary}");
    }
    ids
}

struct Diff<'a> { tx: &'a Transaction<'a>, run: i64, baseline: bool, changed: u64 }

impl Diff<'_> {
    fn log(&mut self, entity: &str, key: &str, field: &str, before: Option<&str>, after: Option<&str>) -> rusqlite::Result<()> {
        if self.baseline { return Ok(()); }
        self.changed += 1;
        self.tx.execute(&format!("INSERT INTO change_log(entity, key, field, before, after, source, actor, at, run_id) VALUES (?1, ?2, ?3, ?4, ?5, 'sync', 'sync', {NOW}, ?6)"), params![entity, key, field, before, after, self.run]).map(|_| ())
    }
    fn todo(&self, kind: &str, key: &str) -> rusqlite::Result<()> { self.todo_reason(kind, key, None) }
    fn todo_reason(&self, kind: &str, key: &str, reason: Option<&str>) -> rusqlite::Result<()> {
        if self.baseline { return Ok(()); }
        self.tx.execute(&format!("INSERT OR IGNORE INTO todo(kind, key, reason, created_at, run_id) VALUES (?1, ?2, ?3, {NOW}, ?4)"), params![kind, key, reason, self.run]).map(|_| ())
    }
    /// DB comment changed while the tool holds a different manual value → let a person choose which side wins.
    fn comment_conflict(&self, sql: &str, key: &str, comment: &Option<String>) -> rusqlite::Result<()> {
        let manual: Option<String> = self.tx.query_row(sql, [key], |r| r.get(0)).optional()?.flatten();
        if manual.is_some() && &manual != comment { self.todo("comment_conflict", key)?; }
        Ok(())
    }
}

fn apply(db: &mut Connection, run: i64, scope: &str, catalog: catalog::Catalog) -> Result<Value, String> {
    let tx = db.transaction().map_err(|e| e.to_string())?;
    let summary = diff(&tx, run, scope, &catalog).map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(summary)
}

fn diff(tx: &Transaction, run: i64, scope: &str, catalog: &catalog::Catalog) -> rusqlite::Result<Value> {
    // First successful sync of a scope is the baseline: no to-dos or change history.
    let baseline = !tx.query_row("SELECT EXISTS(SELECT 1 FROM sync_run WHERE scope = ? AND status = 'ok')", [scope], |r| r.get::<_, bool>(0))?;
    tx.execute("UPDATE sync_run SET baseline = ?1 WHERE id = ?2", params![baseline, run])?;
    let mut d = Diff { tx, run, baseline, changed: 0 };

    type Obj = (String, Option<String>, Option<String>, bool);
    let mut old_objects: HashMap<String, Obj> = HashMap::new();
    {
        let mut stmt = tx.prepare("SELECT key, kind, comment, definition, is_deleted FROM object_tech WHERE scope = ?")?;
        let rows = stmt.query_map([scope], |r| Ok((r.get::<_, String>(0)?, (r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))))?;
        for row in rows { let (k, v) = row?; old_objects.insert(k, v); }
    }
    type Col = (String, bool, Option<String>, bool);
    let mut old_columns: HashMap<String, (String, Col)> = HashMap::new();
    {
        let mut stmt = tx.prepare("SELECT c.key, c.object_key, c.data_type, c.nullable, c.comment, c.is_deleted FROM column_tech c JOIN object_tech o ON o.key = c.object_key WHERE o.scope = ?")?;
        let rows = stmt.query_map([scope], |r| Ok((r.get::<_, String>(0)?, (r.get(1)?, (r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))))?;
        for row in rows { let (k, v) = row?; old_columns.insert(k, v); }
    }

    let (mut added_objects, mut deleted_objects, mut added_columns, mut deleted_columns) = (0, 0, 0, 0);
    let mut seen = HashSet::new();
    let mut new_objects = HashSet::new();
    for o in &catalog.objects {
        let key = format!("{scope}.{}", o.name);
        let props = Value::Object(o.props.clone()).to_string();
        match old_objects.get(&key) {
            None => {
                tx.execute(&format!("INSERT INTO object_tech(key, scope, name, kind, comment, definition, props, first_seen, synced_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, {NOW}, {NOW})"), params![key, scope, o.name, o.kind, o.comment, o.definition, props])?;
                d.log("object", &key, "status", None, Some("created"))?;
                d.todo("new_object", &key)?;
                new_objects.insert(key.clone());
                added_objects += 1;
            }
            Some((kind, comment, definition, deleted)) => {
                tx.execute(&format!("UPDATE object_tech SET kind = ?2, comment = ?3, definition = ?4, props = ?5, synced_at = {NOW}, is_deleted = 0 WHERE key = ?1"), params![key, o.kind, o.comment, o.definition, props])?;
                if *deleted { d.log("object", &key, "status", Some("deleted"), Some("restored"))?; }
                if kind != o.kind { d.log("object", &key, "kind", Some(kind), Some(o.kind))?; }
                if comment != &o.comment {
                    d.log("object", &key, "comment", comment.as_deref(), o.comment.as_deref())?;
                    d.comment_conflict("SELECT description FROM object_biz WHERE key = ?", &key, &o.comment)?;
                }
                if definition != &o.definition { d.log("object", &key, "definition", definition.as_deref(), o.definition.as_deref())?; }
            }
        }
        seen.insert(key);
    }
    let mut gone_objects = HashSet::new();
    for (key, (_, _, _, deleted)) in &old_objects {
        if *deleted || seen.contains(key) { continue; }
        tx.execute("UPDATE object_tech SET is_deleted = 1 WHERE key = ?", [key])?;
        d.log("object", key, "status", Some("present"), Some("deleted"))?;
        d.todo("deleted_object", key)?;
        gone_objects.insert(key.clone());
        deleted_objects += 1;
    }

    let mut seen_columns = HashSet::new();
    for c in &catalog.columns {
        let object_key = format!("{scope}.{}", c.table);
        let key = format!("{object_key}.{}", c.name);
        let props = Value::Object(c.props.clone()).to_string();
        match old_columns.get(&key) {
            None => {
                tx.execute(&format!("INSERT INTO column_tech(key, object_key, name, ordinal, data_type, nullable, comment, props, first_seen) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, {NOW})"), params![key, object_key, c.name, c.ordinal, c.data_type, c.nullable, c.comment, props])?;
                // Columns of a brand-new object are covered by the object's own to-do.
                if !new_objects.contains(&object_key) {
                    d.log("column", &key, "status", None, Some("created"))?;
                    d.todo("new_column", &key)?;
                    added_columns += 1;
                }
            }
            Some((_, (data_type, nullable, comment, deleted))) => {
                tx.execute("UPDATE column_tech SET ordinal = ?2, data_type = ?3, nullable = ?4, comment = ?5, props = ?6, is_deleted = 0 WHERE key = ?1", params![key, c.ordinal, c.data_type, c.nullable, c.comment, props])?;
                if *deleted { d.log("column", &key, "status", Some("deleted"), Some("restored"))?; }
                if data_type != &c.data_type {
                    d.log("column", &key, "data_type", Some(data_type), Some(&c.data_type))?;
                    let downstream: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM lineage WHERE upstream = ?)", [&object_key], |r| r.get(0))?;
                    if downstream { d.todo("check_downstream", &key)?; }
                }
                if *nullable != c.nullable { d.log("column", &key, "nullable", Some(&nullable.to_string()), Some(&c.nullable.to_string()))?; }
                if comment != &c.comment {
                    d.log("column", &key, "comment", comment.as_deref(), c.comment.as_deref())?;
                    d.comment_conflict("SELECT meaning FROM column_biz WHERE key = ?", &key, &c.comment)?;
                }
            }
        }
        seen_columns.insert(key);
    }
    for (key, (object_key, (_, _, _, deleted))) in &old_columns {
        if *deleted || seen_columns.contains(key) { continue; }
        tx.execute("UPDATE column_tech SET is_deleted = 1 WHERE key = ?", [key])?;
        if !gone_objects.contains(object_key) {
            d.log("column", key, "status", Some("present"), Some("deleted"))?;
            d.todo("deleted_column", key)?;
            deleted_columns += 1;
        }
    }
    // Possible renames: a new object whose column names overlap a removed object's by at least the threshold.
    let threshold = store::setting(tx, "rename_threshold").and_then(|v| v.as_f64()).unwrap_or(0.8);
    let column_set = |pairs: &mut dyn Iterator<Item = (String, String)>, keys: &HashSet<String>| {
        let mut sets: HashMap<String, HashSet<String>> = HashMap::new();
        for (object, column) in pairs { if keys.contains(&object) { sets.entry(object).or_default().insert(column); } }
        sets
    };
    let new_sets = column_set(&mut catalog.columns.iter().map(|c| (format!("{scope}.{}", c.table), c.name.clone())), &new_objects);
    let gone_sets = column_set(&mut old_columns.iter().map(|(k, (o, _))| (o.clone(), k[o.len() + 1..].to_owned())), &gone_objects);
    for (new_key, new_columns) in &new_sets {
        let best = gone_sets.iter().map(|(old_key, old_columns)| (old_key, new_columns.intersection(old_columns).count() as f64 / new_columns.len().max(old_columns.len()) as f64))
            .filter(|(_, score)| *score >= threshold).max_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((old_key, _)) = best { d.todo_reason("possible_rename", new_key, Some(old_key))?; }
    }
    crate::naming::refresh(tx, Some(scope))?;
    Ok(json!({
        "objects": catalog.objects.len(), "columns": catalog.columns.len(),
        "added_objects": added_objects, "deleted_objects": deleted_objects,
        "added_columns": added_columns, "deleted_columns": deleted_columns,
        "changes": d.changed, "warnings": catalog.warnings,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::{Catalog, Column, Object};

    fn catalog(columns: &[(&str, &str)]) -> Catalog {
        let mut tables: Vec<&str> = columns.iter().map(|c| c.0).collect();
        tables.dedup();
        Catalog {
            objects: tables.iter().map(|t| Object { name: t.to_string(), kind: "table", comment: None, definition: None, props: Default::default() }).collect(),
            columns: columns.iter().enumerate().map(|(i, (t, c))| Column { table: t.to_string(), name: c.to_string(), ordinal: i as i64, data_type: "int".into(), nullable: true, comment: None, props: Default::default() }).collect(),
            warnings: vec![],
        }
    }

    fn sync(db: &mut Connection, c: Catalog) -> Value {
        db.execute(&format!("INSERT INTO sync_run(scope, trigger, started_at, status) VALUES ('db.public', 'test', {NOW}, 'running')"), []).unwrap();
        let id = db.last_insert_rowid();
        let summary = apply(db, id, "db.public", c).unwrap();
        db.execute("UPDATE sync_run SET status = 'ok' WHERE id = ?", [id]).unwrap();
        summary
    }

    #[test]
    fn baseline_then_diff() {
        let mut db = store::open(std::path::Path::new(":memory:")).unwrap();
        let first = sync(&mut db, catalog(&[("a", "id"), ("a", "x"), ("b", "id")]));
        assert_eq!(first["added_objects"], 2);
        let todos = |db: &Connection| db.query_row("SELECT COUNT(*) FROM todo", [], |r| r.get::<_, i64>(0)).unwrap();
        assert_eq!(todos(&db), 0, "baseline creates no to-dos");

        // b dropped, a.x dropped, a.y added, c added
        let second = sync(&mut db, catalog(&[("a", "id"), ("a", "y"), ("c", "id")]));
        assert_eq!((second["added_objects"].as_i64(), second["deleted_objects"].as_i64()), (Some(1), Some(1)));
        assert_eq!((second["added_columns"].as_i64(), second["deleted_columns"].as_i64()), (Some(1), Some(1)));
        // new_object c, deleted_object b, new_column a.y, deleted_column a.x, possible_rename b → c (same columns)
        assert_eq!(todos(&db), 5);
        let rename: String = db.query_row("SELECT reason FROM todo WHERE kind = 'possible_rename' AND key = 'db.public.c'", [], |r| r.get(0)).unwrap();
        assert_eq!(rename, "db.public.b");
        let deleted: i64 = db.query_row("SELECT is_deleted FROM object_tech WHERE key = 'db.public.b'", [], |r| r.get(0)).unwrap();
        assert_eq!(deleted, 1);

        // same again: nothing new
        let third = sync(&mut db, catalog(&[("a", "id"), ("a", "y"), ("c", "id")]));
        assert_eq!(third["changes"], 0);
        assert_eq!(todos(&db), 5);
    }
}
