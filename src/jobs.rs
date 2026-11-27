//! Daily jobs: metadata backup, data freshness (F1.4), expired temporary object cleanup (F9.2).
use crate::{AppState, api::{ApiResult, fail, valid_time}, biz, manual, store::{self, NOW}, sync};
use axum::{Json, extract::State, http::StatusCode};
use regex::Regex;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

pub const DEFAULT_PATTERN: &str = r"^[a-z0-9]+_cd_tmp_[a-z0-9_]+_(\d{8})(_mv)?$";

pub fn cleanup_config(db: &Connection) -> Value {
    let mut c = store::setting(db, "cleanup").unwrap_or(json!({}));
    for (k, v) in [("enabled", json!(false)), ("pattern", json!(DEFAULT_PATTERN)), ("days", json!(1)), ("time", json!("02:00")), ("confirmed", Value::Null)] {
        if c.get(k).is_none() { c[k] = v; }
    }
    c["effective"] = json!(c["enabled"] == json!(true) && c["confirmed"] == json!({ "pattern": c["pattern"], "days": c["days"] }));
    c
}

pub fn validate_cleanup(v: &Value) -> Result<(), String> {
    let pattern = v["pattern"].as_str().ok_or("识别规则不能为空")?;
    let re = Regex::new(pattern).map_err(|e| format!("识别规则无效：{e}"))?;
    if re.captures_len() < 2 { return Err("识别规则需要用第一个括号捕获 yyyymmdd 日期".into()); }
    if !v["days"].as_u64().is_some_and(|d| (1..=30).contains(&d)) { return Err("超期天数需在 1–30 之间".into()); }
    if !valid_time(&v["time"]) { return Err("时间格式应为 HH:MM".into()); }
    Ok(())
}

/// Saving keeps the previous confirmation unless `confirm` is true; a changed rule therefore pauses cleanup until confirmed again.
pub fn merge_cleanup(db: &Connection, incoming: &Value) -> Value {
    let current = cleanup_config(db);
    let confirmed = if incoming["confirm"] == json!(true) { json!({ "pattern": incoming["pattern"], "days": incoming["days"] }) } else { current["confirmed"].clone() };
    if incoming["confirm"] == json!(true) { manual::log(db, "system", "cleanup", "confirmed", None, Some(&confirmed.to_string())).ok(); }
    json!({ "enabled": incoming["enabled"].as_bool().unwrap_or(false), "pattern": incoming["pattern"], "days": incoming["days"], "time": incoming["time"], "confirmed": confirmed })
}

/// Candidates among `_cd_tmp_` objects in the sync scopes: drop (expired), keep (not yet expired) or todo (not recognised / views).
pub fn preview(db: &Connection) -> Result<Vec<Value>, String> {
    let config = cleanup_config(db);
    let re = Regex::new(config["pattern"].as_str().unwrap_or(DEFAULT_PATTERN)).map_err(|e| e.to_string())?;
    let days = config["days"].as_i64().unwrap_or(1);
    let cutoff: String = db.query_row(&format!("SELECT strftime('%Y%m%d', 'now', '-{} day')", days - 1), [], |r| r.get(0)).map_err(|e| e.to_string())?;
    let labels: Vec<String> = sync::scopes(db).into_iter().filter_map(|s| myquery_core::config::load().ok()?.profiles.into_iter().find(|p| p.name == s.profile).map(|p| format!("{}.{}", p.database, s.schema))).collect();
    let mut stmt = db.prepare("SELECT key, scope, name, kind FROM object_tech WHERE is_deleted = 0 AND name LIKE '%\\_cd\\_tmp\\_%' ESCAPE '\\' ORDER BY name").map_err(|e| e.to_string())?;
    let objects: Vec<(String, String, String, String)> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))).map_err(|e| e.to_string())?.flatten().collect();
    Ok(objects.into_iter().filter(|o| labels.contains(&o.1)).map(|(key, _, name, kind)| {
        let date = re.captures(&name).and_then(|c| c.get(1)).map(|m| m.as_str().to_owned());
        let (action, reason) = match &date {
            None => ("todo", "unrecognized"),
            Some(d) if !crate::naming::valid_date(d) => ("todo", "invalid_date"),
            Some(_) if kind == "view" => ("todo", "view"),
            Some(d) if d.as_str() < cutoff.as_str() => ("drop", "expired"),
            Some(_) => ("keep", "not_expired"),
        };
        json!({ "key": key, "name": name, "kind": kind, "date": date, "action": action, "reason": reason })
    }).collect())
}

/// Executes DROP for expired temporary objects (never CASCADE), logs every statement, then re-syncs the affected scopes.
pub async fn cleanup(state: &AppState, manual_run: bool) -> Result<Value, String> {
    let (candidates, effective) = { let db = state.db.lock().unwrap(); (preview(&db)?, cleanup_config(&db)["effective"] == json!(true)) };
    if !effective && !manual_run { return Ok(json!({ "skipped": "not confirmed" })); }
    if manual_run { let db = state.db.lock().unwrap(); let c = cleanup_config(&db); if c["confirmed"] != json!({ "pattern": c["pattern"], "days": c["days"] }) { return Err("清理规则尚未确认".into()); } }
    let (mut dropped, mut failed) = (0, 0);
    let mut scopes = std::collections::BTreeSet::new();
    for c in &candidates {
        let key = c["key"].as_str().unwrap();
        if c["action"] == "todo" { state.db.lock().unwrap().execute(&format!("INSERT OR IGNORE INTO todo(kind, key, reason, created_at) VALUES ('tmp_unrecognized', ?1, ?2, {NOW})"), (key, c["reason"].as_str())).ok(); continue; }
        if c["action"] != "drop" { continue; }
        let parts: Vec<&str> = key.splitn(3, '.').collect();
        let keyword = if c["kind"] == "mv" { "MATERIALIZED VIEW" } else { "TABLE" };
        let sql = format!("DROP {keyword} {}.{}", myquery_core::db::identifier(parts[1]), myquery_core::db::identifier(parts[2]));
        let result = match sync::connect_key(state, key).await {
            Ok((session, ..)) => session.client.simple_query(&sql).await.map(|_| ()).map_err(myquery_core::db::error),
            Err(e) => Err(e),
        };
        let db = state.db.lock().unwrap();
        let outcome = match &result { Ok(()) => { dropped += 1; "ok".to_owned() } Err(e) => { failed += 1; e.clone() } };
        db.execute(&format!("INSERT INTO change_log(entity, key, field, before, after, source, actor, at) VALUES ('object', ?1, 'drop', ?2, ?3, 'cleanup', 'cleanup', {NOW})"), (key, &sql, &outcome)).ok();
        if result.is_err() { db.execute(&format!("INSERT OR IGNORE INTO todo(kind, key, reason, created_at) VALUES ('tmp_drop_failed', ?1, ?2, {NOW})"), (key, &outcome)).ok(); }
        scopes.insert(format!("{}.{}", parts[0], parts[1]));
    }
    for label in scopes {
        let scope = { let db = state.db.lock().unwrap(); sync::scope_profile(&db, &label).ok().map(|(p, schema)| sync::Scope { profile: p.name, schema, display_name: None }) };
        if let Some(scope) = scope { sync::run(state, Some(scope), "cleanup").await; }
    }
    Ok(json!({ "dropped": dropped, "failed": failed }))
}

/// Copies the metadata database to data/backups (VACUUM INTO) and keeps the newest 14 daily copies.
pub fn backup(state: &AppState, label: &str) -> Result<String, String> {
    let dir = state.data_dir.join("backups");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let db = state.db.lock().unwrap();
    let date: String = db.query_row("SELECT strftime('%Y-%m-%d', 'now')", [], |r| r.get(0)).map_err(|e| e.to_string())?;
    let file = dir.join(format!("mymeta-{date}{label}.db"));
    if file.exists() { std::fs::remove_file(&file).map_err(|e| e.to_string())?; }
    db.execute("VACUUM INTO ?", [file.to_string_lossy()]).map_err(|e| e.to_string())?;
    let mut daily: Vec<_> = std::fs::read_dir(&dir).map_err(|e| e.to_string())?.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.starts_with("mymeta-") && n.len() == "mymeta-2026-01-01.db".len()).collect();
    daily.sort();
    while daily.len() > 14 { std::fs::remove_file(dir.join(daily.remove(0))).ok(); }
    Ok(file.to_string_lossy().into_owned())
}

/// True once per UTC day after `time`; records the run date in settings.
fn due(db: &Connection, name: &str, time: &str) -> bool {
    let (date, now): (String, String) = db.query_row("SELECT date('now'), strftime('%H:%M', 'now')", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap_or_default();
    let key = format!("job_last_{name}");
    if now.as_str() < time || store::setting(db, &key).and_then(|v| v.as_str().map(str::to_owned)) == Some(date.clone()) { return false; }
    store::set_setting(db, &key, &json!(date)).is_ok()
}

pub async fn scheduler(state: Arc<AppState>) {
    loop {
        if due(&state.db.lock().unwrap(), "backup", "00:00") && let Err(e) = backup(&state, "") { eprintln!("backup failed: {e}"); }
        let freshness_time = { let db = state.db.lock().unwrap(); store::setting(&db, "freshness_time").and_then(|v| v.as_str().map(str::to_owned)).unwrap_or("03:00".into()) };
        if due(&state.db.lock().unwrap(), "freshness", &freshness_time) { biz::freshness_all(&state).await; }
        let cleanup_time = { let db = state.db.lock().unwrap(); let c = cleanup_config(&db); (c["effective"] == json!(true)).then(|| c["time"].as_str().unwrap_or("02:00").to_owned()) };
        if let Some(time) = cleanup_time && due(&state.db.lock().unwrap(), "cleanup", &time) {
            match cleanup(&state, false).await { Ok(r) => println!("cleanup: {r}"), Err(e) => eprintln!("cleanup failed: {e}") }
        }
        tokio::time::sleep(Duration::from_secs(60)).await;
    }
}

type St = State<Arc<AppState>>;

pub async fn cleanup_preview(State(s): St) -> ApiResult {
    let db = s.db.lock().unwrap();
    Ok(Json(json!({ "config": cleanup_config(&db), "candidates": preview(&db).map_err(fail)? })))
}

pub async fn cleanup_now(State(s): St) -> ApiResult {
    cleanup(&s, true).await.map(Json).map_err(|e| (StatusCode::BAD_REQUEST, e))
}

pub async fn freshness_now(State(s): St) -> ApiResult {
    biz::freshness_all(&s).await;
    Ok(Json(json!({ "ok": true })))
}

pub async fn backup_now(State(s): St) -> ApiResult {
    Ok(Json(json!({ "file": backup(&s, "-manual").map_err(fail)? })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_rule() {
        let re = Regex::new(DEFAULT_PATTERN).unwrap();
        assert_eq!(re.captures("btd6mobile_cd_tmp_retention_20261120_mv").unwrap()[1].to_owned(), "20261120");
        assert_eq!(re.captures("zamobile_cd_tmp_x_20261121").unwrap()[1].to_owned(), "20261121");
        assert!(re.captures("btd6mobile_cd_tmp_cp_params_v").is_none());
        assert!(validate_cleanup(&json!({ "pattern": "^x_cd_tmp_", "days": 1, "time": "02:00" })).is_err(), "pattern without a date group");
    }

    #[test]
    fn changed_rule_needs_confirmation() {
        let db = store::open(std::path::Path::new(":memory:")).unwrap();
        let rule = json!({ "enabled": true, "pattern": DEFAULT_PATTERN, "days": 1, "time": "02:00" });
        store::set_setting(&db, "cleanup", &merge_cleanup(&db, &rule)).unwrap();
        assert_eq!(cleanup_config(&db)["effective"], json!(false));
        let mut confirm = rule.clone(); confirm["confirm"] = json!(true);
        store::set_setting(&db, "cleanup", &merge_cleanup(&db, &confirm)).unwrap();
        assert_eq!(cleanup_config(&db)["effective"], json!(true));
        let mut changed = rule.clone(); changed["days"] = json!(2);
        store::set_setting(&db, "cleanup", &merge_cleanup(&db, &changed)).unwrap();
        assert_eq!(cleanup_config(&db)["effective"], json!(false), "paused until the new rule is confirmed");
    }
}
