//! M2: business metadata views, COMMENT ON generation, standard query template, value sampling, data freshness, rename migration.
use crate::{AppState, api::{ApiResult, fail, rows}, manual, store::NOW, sync};
use axum::{Json, extract::{Query, State}, http::StatusCode};
use myquery_core::db::{identifier, literal};
use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::HashMap, sync::Arc, time::Duration};

type St = State<Arc<AppState>>;
fn bad(message: impl Into<String>) -> (StatusCode, String) { (StatusCode::BAD_REQUEST, message.into()) }

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct KeyQuery { pub key: String, pub scope: String }

const DATE_NAMES: [&str; 12] = ["date", "event_date", "dt", "day", "report_date", "stat_date", "install_date", "created_date", "created_at", "event_time", "timestamp", "time"];

/// Date column used for freshness: manual setting ("" disables) or the first date-typed column with a well-known name.
pub fn date_column(db: &Connection, key: &str) -> Option<String> {
    let manual: Option<Option<String>> = db.query_row("SELECT date_column FROM object_biz WHERE key = ? AND date_column IS NOT NULL", [key], |r| r.get(0)).optional().ok().flatten();
    if let Some(manual) = manual { return manual.filter(|c| !c.is_empty() && c != "-"); }
    let mut stmt = db.prepare("SELECT name, data_type FROM column_tech WHERE object_key = ? AND is_deleted = 0").ok()?;
    let columns: Vec<(String, String)> = stmt.query_map([key], |r| Ok((r.get(0)?, r.get(1)?))).ok()?.flatten().collect();
    DATE_NAMES.iter().find_map(|name| columns.iter().find(|(c, t)| c == name && (t.contains("date") || t.contains("timestamp"))).map(|(c, _)| c.clone()))
}

pub async fn object(State(s): St, Query(q): Query<KeyQuery>) -> ApiResult {
    let db = s.db.lock().unwrap();
    let object = rows(&db, "SELECT * FROM object_tech WHERE key = ?", &[&q.key])?.pop().ok_or((StatusCode::NOT_FOUND, "对象不存在".into()))?;
    let biz = rows(&db, "SELECT * FROM object_biz WHERE key = ?", &[&q.key])?.pop();
    let columns = rows(&db, "SELECT c.*, b.meaning, b.unit, b.currency, b.timezone, b.is_sensitive, b.notes, (SELECT COUNT(*) FROM rule r WHERE r.target_key = c.key) AS rule_count FROM column_tech c LEFT JOIN column_biz b ON b.key = c.key WHERE c.object_key = ? ORDER BY c.is_deleted, c.ordinal", &[&q.key])?;
    let prefix = format!("{}.", q.key);
    let rules = rows(&db, "SELECT * FROM rule WHERE target_key = ?1 OR substr(target_key, 1, length(?2)) = ?2 ORDER BY target_key, kind, id", &[&q.key, &prefix])?;
    let freshness = rows(&db, &format!("SELECT f.*, f.max_date < date('now', '-1 day') AS delayed, date('now', '-1 day') AS expected FROM freshness f WHERE key = ?"), &[&q.key])?.pop();
    let refreshed_by = rows(&db, "SELECT DISTINCT l.source, l.origin, n.name, n.info FROM lineage l LEFT JOIN node n ON n.key = l.origin WHERE l.downstream = ? AND l.upstream_column IS NULL AND l.source IN ('script', 'mybi_task')", &[&q.key])?;
    let counts = rows(&db, "SELECT (SELECT COUNT(DISTINCT upstream) FROM lineage WHERE downstream = ?1) AS upstream, (SELECT COUNT(DISTINCT downstream) FROM lineage WHERE upstream = ?1) AS downstream, (SELECT COUNT(*) FROM relation WHERE (left_key = ?1 OR right_key = ?1) AND status = 'confirmed') AS relations", &[&q.key])?.pop();
    let guessed_date = date_column(&db, &q.key);
    Ok(Json(json!({ "object": object, "biz": biz, "columns": columns, "rules": rules, "freshness": freshness, "date_column": guessed_date, "refreshed_by": refreshed_by, "counts": counts })))
}

fn quoted(key: &str) -> (String, String) {
    let parts: Vec<&str> = key.splitn(4, '.').collect();
    (format!("{}.{}", identifier(parts[1]), identifier(parts[2])), parts.get(3).map(|c| identifier(c)).unwrap_or_default())
}

/// COMMENT ON statements where the tool's text differs from the database comment (only generated, never executed).
pub fn comment_sql(db: &Connection, object_key: Option<&str>, scope: Option<&str>) -> Result<String, String> {
    let filter = if object_key.is_some() { "o.key = ?1" } else { "o.scope = ?1" };
    let arg = object_key.or(scope).unwrap_or_default();
    let label = arg.splitn(3, '.').take(2).collect::<Vec<_>>().join(".");
    let redshift = sync::scope_profile(db, &label).map(|(p, _)| p.redshift).unwrap_or(true);
    let mut out = Vec::new();
    let mut stmt = db.prepare(&format!("SELECT o.key, o.kind, b.description FROM object_tech o JOIN object_biz b ON b.key = o.key WHERE {filter} AND o.is_deleted = 0 AND b.description IS NOT NULL AND b.description IS NOT COALESCE(o.comment, '') ORDER BY o.key")).map_err(|e| e.to_string())?;
    for row in stmt.query_map([arg], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))).map_err(|e| e.to_string())? {
        let (key, kind, text) = row.map_err(|e| e.to_string())?;
        let keyword = match (kind.as_str(), redshift) { ("view", _) | ("mv", true) => "VIEW", ("mv", false) => "MATERIALIZED VIEW", _ => "TABLE" };
        out.push(format!("COMMENT ON {keyword} {} IS {};", quoted(&key).0, literal(&text, redshift)));
    }
    let mut stmt = db.prepare(&format!("SELECT c.key, b.meaning FROM column_tech c JOIN object_tech o ON o.key = c.object_key JOIN column_biz b ON b.key = c.key WHERE {filter} AND c.is_deleted = 0 AND b.meaning IS NOT NULL AND b.meaning IS NOT COALESCE(c.comment, '') ORDER BY c.object_key, c.ordinal")).map_err(|e| e.to_string())?;
    for row in stmt.query_map([arg], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).map_err(|e| e.to_string())? {
        let (key, text) = row.map_err(|e| e.to_string())?;
        let (table, column) = quoted(&key);
        out.push(format!("COMMENT ON COLUMN {table}.{column} IS {};", literal(&text, redshift)));
    }
    Ok(out.join("\n"))
}

pub async fn comments(State(s): St, Query(q): Query<KeyQuery>) -> ApiResult {
    let db = s.db.lock().unwrap();
    let sql = comment_sql(&db, (!q.key.is_empty()).then_some(q.key.as_str()), (!q.scope.is_empty()).then_some(q.scope.as_str())).map_err(fail)?;
    Ok(Json(json!({ "sql": sql })))
}

fn sql_value(value: &str) -> String {
    if value.parse::<f64>().is_ok() { value.to_owned() } else { format!("'{}'", value.replace('\'', "''")) }
}

/// Standard query template (F3.5): all columns, plus every required filter and invalid-value exclusion.
pub fn template(db: &Connection, key: &str) -> Result<String, String> {
    let name: (String, Option<String>) = db.query_row("SELECT o.key, COALESCE(b.cn_name, b.description, o.comment) FROM object_tech o LEFT JOIN object_biz b ON b.key = o.key WHERE o.key = ?", [key], |r| Ok((r.get(0)?, r.get(1)?))).map_err(|_| "对象不存在".to_string())?;
    let mut lines = vec![format!("-- {}{}", name.0, name.1.map(|n| format!("  {n}")).unwrap_or_default())];
    let prefix = format!("{key}.");
    let mut stmt = db.prepare("SELECT target_key, kind, value, condition, note FROM rule WHERE target_key = ?1 OR substr(target_key, 1, length(?2)) = ?2 ORDER BY kind, target_key, id").map_err(|e| e.to_string())?;
    let rules: Vec<(String, String, Option<String>, Option<String>, Option<String>)> = stmt.query_map([key, &prefix], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))).map_err(|e| e.to_string())?.flatten().collect();
    for (target, _, _, _, note) in rules.iter().filter(|r| r.1 == "note") {
        if let Some(note) = note { lines.push(format!("-- {}{}", if target == key { String::new() } else { format!("{}: ", &target[prefix.len()..]) }, note.replace('\n', " "))); }
    }
    let mut stmt = db.prepare("SELECT c.name, COALESCE(b.meaning, c.comment) FROM column_tech c LEFT JOIN column_biz b ON b.key = c.key WHERE c.object_key = ? AND c.is_deleted = 0 ORDER BY c.ordinal").map_err(|e| e.to_string())?;
    let columns: Vec<(String, Option<String>)> = stmt.query_map([key], |r| Ok((r.get(0)?, r.get(1)?))).map_err(|e| e.to_string())?.flatten().collect();
    lines.push("SELECT".into());
    for (i, (column, meaning)) in columns.iter().enumerate() {
        let comma = if i + 1 < columns.len() { "," } else { "" };
        lines.push(format!("    {}{comma}{}", identifier(column), meaning.as_ref().map(|m| format!("  -- {}", m.replace('\n', " "))).unwrap_or_default()));
    }
    lines.push(format!("FROM {}", quoted(key).0));
    let mut conditions: Vec<String> = Vec::new();
    for (target, kind, value, condition, note) in &rules {
        let comment = note.as_ref().map(|n| format!("  -- {}", n.replace('\n', " "))).unwrap_or_default();
        match kind.as_str() {
            "filter" => if let Some(c) = condition { conditions.push(format!("{c}{comment}")) },
            "invalid" => {
                let column = target.get(prefix.len()..).map(identifier);
                match (condition, value, column) {
                    (Some(c), _, _) => conditions.push(format!("{c}{comment}")),
                    (None, Some(v), Some(col)) if v.eq_ignore_ascii_case("null") => conditions.push(format!("{col} IS NOT NULL{comment}")),
                    (None, Some(v), Some(col)) => conditions.push(format!("{col} <> {}{comment}", sql_value(v))),
                    _ => {}
                }
            }
            _ => {}
        }
    }
    for (i, condition) in conditions.iter().enumerate() { lines.push(format!("{} {condition}", if i == 0 { "WHERE" } else { "  AND" })); }
    // ";" on its own line so it never lands inside a trailing "--" comment
    Ok(lines.join("\n") + "\n;")
}

pub async fn sql_template(State(s): St, Query(q): Query<KeyQuery>) -> ApiResult {
    let db = s.db.lock().unwrap();
    Ok(Json(json!({ "sql": template(&db, &q.key).map_err(fail)? })))
}

#[derive(Deserialize)]
pub struct SampleBody { key: String, n: Option<u32> }

/// Top-N values of a column (F3.1), for importing valid values after manual confirmation.
pub async fn sample(State(s): St, Json(body): Json<SampleBody>) -> ApiResult {
    let column = body.key.rsplit_once('.').map(|(_, c)| c.to_owned()).ok_or_else(|| bad("字段键无效"))?;
    let object = body.key[..body.key.len() - column.len() - 1].to_owned();
    let (session, profile, _) = sync::connect_key(&s, &body.key).await.map_err(bad)?;
    let n = body.n.unwrap_or(20).clamp(1, 200);
    let (table, _) = quoted(&object);
    let timeout = if profile.redshift { "SET statement_timeout TO 60000" } else { "SET statement_timeout = 60000" };
    session.client.simple_query(timeout).await.map_err(|e| bad(myquery_core::db::error(e)))?;
    let sql = format!("SELECT CAST({col} AS VARCHAR(500)), COUNT(*) FROM {table} GROUP BY 1 ORDER BY 2 DESC LIMIT {n}", col = identifier(&column));
    let messages = session.client.simple_query(&sql).await.map_err(|e| bad(myquery_core::db::error(e)))?;
    let values: Vec<Value> = messages.into_iter().filter_map(|m| match m { tokio_postgres::SimpleQueryMessage::Row(r) => Some(json!({ "value": r.get(0), "count": r.get(1).and_then(|c| c.parse::<i64>().ok()) })), _ => None }).collect();
    Ok(Json(json!(values)))
}

/// MAX(date column) for one object; stores the result (or error) in `freshness`.
pub async fn check_freshness(state: &AppState, session: &myquery_core::db::Session, key: &str) -> Result<(), String> {
    let column = { let db = state.db.lock().unwrap(); date_column(&db, key) };
    let Some(column) = column else {
        state.db.lock().unwrap().execute("DELETE FROM freshness WHERE key = ?", [key]).map_err(|e| e.to_string())?;
        return Ok(());
    };
    let sql = format!("SELECT CAST(MAX({}) AS DATE) FROM {}", identifier(&column), quoted(key).0);
    let result = tokio::time::timeout(Duration::from_secs(120), session.client.simple_query(&sql)).await;
    let (max_date, error) = match result {
        Err(_) => (None, Some("timeout".to_owned())),
        Ok(Err(e)) => (None, Some(myquery_core::db::error(e))),
        Ok(Ok(messages)) => (messages.into_iter().find_map(|m| match m { tokio_postgres::SimpleQueryMessage::Row(r) => Some(r.get(0).map(str::to_owned)), _ => None }).flatten(), None),
    };
    state.db.lock().unwrap().execute(&format!("INSERT INTO freshness(key, date_column, max_date, checked_at, error) VALUES (?1, ?2, ?3, {NOW}, ?4) ON CONFLICT(key) DO UPDATE SET date_column = excluded.date_column, max_date = excluded.max_date, checked_at = excluded.checked_at, error = excluded.error"), (key, &column, &max_date, &error)).map_err(|e| e.to_string())?;
    Ok(())
}

/// Daily job: freshness for every table / MV in every scope (views are skipped: MAX over a view re-runs its query).
pub async fn freshness_all(state: &AppState) {
    let labels: Vec<String> = { let db = state.db.lock().unwrap(); sync::scopes(&db).into_iter().filter_map(|s| myquery_core::config::load().ok()?.profiles.into_iter().find(|p| p.name == s.profile).map(|p| format!("{}.{}", p.database, s.schema))).collect() };
    for label in labels {
        let keys: Vec<String> = { let db = state.db.lock().unwrap(); let mut stmt = db.prepare("SELECT key FROM object_tech WHERE scope = ? AND is_deleted = 0 AND kind IN ('table', 'mv') AND name NOT LIKE '%\\_cd\\_tmp\\_%' ESCAPE '\\' ORDER BY key").unwrap(); stmt.query_map([&label], |r| r.get(0)).unwrap().flatten().collect() };
        let session = match sync::connect_key(state, &format!("{label}.x")).await { Ok((session, ..)) => session, Err(error) => { eprintln!("freshness {label}: {error}"); continue; } };
        for key in keys { if let Err(error) = check_freshness(state, &session, &key).await { eprintln!("freshness {key}: {error}"); } }
    }
}

pub async fn refresh_freshness(State(s): St, Json(body): Json<KeyQuery>) -> ApiResult {
    let (session, ..) = sync::connect_key(&s, &body.key).await.map_err(bad)?;
    check_freshness(&s, &session, &body.key).await.map_err(fail)?;
    let db = s.db.lock().unwrap();
    Ok(Json(json!(rows(&db, "SELECT f.*, f.max_date < date('now', '-1 day') AS delayed, date('now', '-1 day') AS expected FROM freshness f WHERE key = ?", &[&body.key])?.pop())))
}

#[derive(Deserialize)]
pub struct RenameBody { from: String, to: String, todo: Option<i64> }

/// Moves all manual content from an old object key to its renamed successor (5.3 表重命名).
pub fn migrate(db: &Connection, from: &str, to: &str) -> rusqlite::Result<()> {
    let (old_prefix, new_prefix) = (format!("{from}."), format!("{to}."));
    let swap = "CASE WHEN {c} = ?1 THEN ?2 WHEN substr({c}, 1, length(?3)) = ?3 THEN ?4 || substr({c}, length(?3) + 1) ELSE {c} END";
    let args = (from, to, &old_prefix, &new_prefix);
    // Keep the new object's own manual rows if they already exist.
    db.execute("DELETE FROM object_biz WHERE key = ?2 AND EXISTS (SELECT 1 FROM object_biz WHERE key = ?1)", (from, to))?;
    db.execute("UPDATE object_biz SET key = ?2 WHERE key = ?1", (from, to))?;
    db.execute("DELETE FROM column_biz WHERE substr(key, 1, length(?2)) = ?2 AND EXISTS (SELECT 1 FROM column_biz o WHERE o.key = ?1 || substr(column_biz.key, length(?2) + 1))", (&old_prefix, &new_prefix))?;
    for (table, column) in [("column_biz", "key"), ("rule", "target_key"), ("relation", "left_key"), ("relation", "right_key"), ("flow_step", "object_key"), ("metric", "object_key")] {
        db.execute(&format!("UPDATE OR IGNORE {table} SET {column} = {}", swap.replace("{c}", column)), args)?;
    }
    for (table, column) in [("relation", "key_pairs"), ("metric", "used_by")] {
        db.execute(&format!("UPDATE {table} SET {column} = replace(replace({column}, '\"' || ?1 || '\"', '\"' || ?2 || '\"'), '\"' || ?3, '\"' || ?4)"), args)?;
    }
    manual::log(db, "object", to, "renamed", Some(from), Some(to))
}

pub async fn rename(State(s): St, Json(body): Json<RenameBody>) -> ApiResult {
    let mut db = s.db.lock().unwrap();
    let tx = db.transaction().map_err(fail)?;
    migrate(&tx, &body.from, &body.to).map_err(fail)?;
    if let Some(todo) = body.todo { tx.execute(&format!("UPDATE todo SET status = 'done', handler = 'admin', handled_at = {NOW} WHERE id = ?"), [todo]).map_err(fail)?; }
    tx.commit().map_err(fail)?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct ConflictBody { key: String, keep: String, todo: Option<i64> }

/// Comment conflict: keep the database comment (copy into the tool) or the tool's text (COMMENT ON is generated for a person to run).
pub async fn resolve_comment(State(s): St, Json(body): Json<ConflictBody>) -> ApiResult {
    let db = s.db.lock().unwrap();
    let column = body.key.split('.').count() == 4;
    let mut sql = String::new();
    if body.keep == "db" {
        let comment: Option<String> = db.query_row(if column { "SELECT comment FROM column_tech WHERE key = ?" } else { "SELECT comment FROM object_tech WHERE key = ?" }, [&body.key], |r| r.get(0)).map_err(fail)?;
        let (table, field) = if column { ("column_biz", "meaning") } else { ("object_biz", "description") };
        let spec = manual::SPECS.iter().find(|s| s.table == table).unwrap();
        let mut row = serde_json::Map::new();
        row.insert("key".into(), json!(body.key));
        row.insert(field.into(), json!(comment));
        manual::upsert(&db, spec, &row).map_err(fail)?;
    } else {
        let object = if column { body.key.rsplit_once('.').unwrap().0.to_owned() } else { body.key.clone() };
        sql = comment_sql(&db, Some(&object), None).map_err(fail)?;
    }
    if let Some(todo) = body.todo { db.execute(&format!("UPDATE todo SET status = 'done', handler = 'admin', handled_at = {NOW} WHERE id = ?"), [todo]).map_err(fail)?; }
    Ok(Json(json!({ "sql": sql })))
}

/// Effective (manual or parsed) layer per object key, used by lineage checks and filters.
pub fn layers(db: &Connection) -> HashMap<String, String> {
    let mut stmt = db.prepare("SELECT o.key, COALESCE(b.layer, json_extract(o.naming, '$.layer')) FROM object_tech o LEFT JOIN object_biz b ON b.key = o.key").unwrap();
    stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))).unwrap().flatten().filter_map(|(k, l)| l.map(|l| (k, l))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rename_moves_manual_content() {
        let db = crate::store::open(std::path::Path::new(":memory:")).unwrap();
        db.execute_batch("
            INSERT INTO object_biz(key, cn_name) VALUES ('d.s.old', '旧表');
            INSERT INTO column_biz(key, meaning) VALUES ('d.s.old.uid', '用户'), ('d.s.oldx.uid', '其他表');
            INSERT INTO rule(target_key, kind, value) VALUES ('d.s.old.uid', 'valid', '1'), ('d.s.old', 'filter', NULL);
            INSERT INTO relation(left_key, right_key, key_pairs) VALUES ('d.s.dim', 'd.s.old', '[[\"d.s.dim.uid\",\"d.s.old.uid\"]]');
            INSERT INTO metric(name_cn, object_key, used_by) VALUES ('m', 'd.s.old', '[\"d.s.old\"]');").unwrap();
        migrate(&db, "d.s.old", "d.s.new").unwrap();
        let one = |sql: &str| db.query_row(sql, [], |r| r.get::<_, String>(0)).unwrap();
        assert_eq!(one("SELECT key FROM object_biz"), "d.s.new");
        assert_eq!(one("SELECT group_concat(key) FROM (SELECT key FROM column_biz ORDER BY key)"), "d.s.new.uid,d.s.oldx.uid", "similar prefixes untouched");
        assert_eq!(one("SELECT group_concat(target_key) FROM (SELECT target_key FROM rule ORDER BY id)"), "d.s.new.uid,d.s.new");
        assert_eq!(one("SELECT right_key || ' ' || key_pairs FROM relation"), "d.s.new [[\"d.s.dim.uid\",\"d.s.new.uid\"]]");
        assert_eq!(one("SELECT object_key || ' ' || used_by FROM metric"), "d.s.new [\"d.s.new\"]");
    }
}
