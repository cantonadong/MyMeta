//! Generic create / update / delete for manually maintained tables, with field-level change history (F8.3).
use crate::{AppState, api::{ApiResult, fail, rows}, store::NOW};
use axum::{Json, extract::{Path, Query, State}, http::StatusCode};
use rusqlite::{Connection, ToSql, types::Value as SqlValue};
use serde_json::{Map, Value, json};
use std::{collections::HashMap, sync::Arc};

pub struct Spec {
    pub table: &'static str,
    /// "key" (text business key) or "id" (integer)
    pub pk: &'static str,
    pub columns: &'static [&'static str],
    /// Column whose value is used as change_log.key
    pub log_key: &'static str,
    pub required: &'static [&'static str],
}

pub const SPECS: &[Spec] = &[
    Spec { table: "object_biz", pk: "key", columns: &["cn_name", "description", "product", "layer", "grain", "primary_key", "source_tables", "refresh_method", "maintainer", "is_temporary", "date_column"], log_key: "key", required: &[] },
    Spec { table: "column_biz", pk: "key", columns: &["meaning", "unit", "currency", "timezone", "is_sensitive", "notes"], log_key: "key", required: &[] },
    Spec { table: "rule", pk: "id", columns: &["target_key", "kind", "value", "condition", "note"], log_key: "target_key", required: &["target_key", "kind"] },
    Spec { table: "relation", pk: "id", columns: &["left_key", "right_key", "key_pairs", "kind", "note", "diff_note", "status", "origin"], log_key: "left_key", required: &["left_key", "right_key", "key_pairs"] },
    Spec { table: "flow_step", pk: "id", columns: &["product", "flow", "step_no", "name", "object_key", "columns", "note"], log_key: "flow", required: &["product", "flow", "step_no", "name"] },
    Spec { table: "metric", pk: "id", columns: &["name_cn", "name_en", "definition", "formula", "numerator", "denominator", "object_key", "column_name", "used_by"], log_key: "name_cn", required: &["name_cn"] },
];

fn spec(table: &str) -> Result<&'static Spec, (StatusCode, String)> {
    SPECS.iter().find(|s| s.table == table).ok_or((StatusCode::NOT_FOUND, "未知数据表".into()))
}

/// JSON → SQLite value; arrays / objects are stored as JSON text, empty strings as NULL.
fn to_sql(value: &Value) -> SqlValue {
    match value {
        Value::Null => SqlValue::Null,
        Value::Bool(b) => SqlValue::Integer(*b as i64),
        Value::Number(n) => n.as_i64().map(SqlValue::Integer).unwrap_or_else(|| SqlValue::Real(n.as_f64().unwrap_or(0.0))),
        Value::String(s) if s.trim().is_empty() => SqlValue::Null,
        Value::String(s) => SqlValue::Text(s.clone()),
        other => SqlValue::Text(other.to_string()),
    }
}

fn text(value: &SqlValue) -> Option<String> {
    match value { SqlValue::Null => None, SqlValue::Integer(i) => Some(i.to_string()), SqlValue::Real(f) => Some(f.to_string()), SqlValue::Text(t) => Some(t.clone()), SqlValue::Blob(_) => None }
}

pub fn log(db: &Connection, entity: &str, key: &str, field: &str, before: Option<&str>, after: Option<&str>) -> rusqlite::Result<()> {
    db.execute(&format!("INSERT INTO change_log(entity, key, field, before, after, source, actor, at) VALUES (?1, ?2, ?3, ?4, ?5, 'manual', 'admin', {NOW})"), (entity, key, field, before, after)).map(|_| ())
}

/// Inserts or updates one row and logs every changed field. Returns the row's primary key.
pub fn upsert(db: &Connection, spec: &Spec, body: &Map<String, Value>) -> Result<Value, String> {
    let pk_value = body.get(spec.pk).filter(|v| !v.is_null() && v.as_str() != Some(""));
    let values: Vec<(&str, SqlValue)> = spec.columns.iter().filter(|c| body.contains_key(**c)).map(|c| (*c, to_sql(&body[*c]))).collect();
    let old: Option<HashMap<String, SqlValue>> = match pk_value {
        Some(pk) => {
            let mut stmt = db.prepare(&format!("SELECT * FROM {} WHERE {} = ?", spec.table, spec.pk)).map_err(|e| e.to_string())?;
            let names: Vec<String> = stmt.column_names().into_iter().map(str::to_owned).collect();
            let mut found = stmt.query([to_sql(pk)]).map_err(|e| e.to_string())?;
            match found.next().map_err(|e| e.to_string())? {
                Some(row) => Some(names.iter().enumerate().map(|(i, n)| (n.clone(), row.get::<_, SqlValue>(i).unwrap_or(SqlValue::Null))).collect()),
                None => None,
            }
        }
        None => None,
    };
    if old.is_none() {
        for required in spec.required {
            if values.iter().find(|(c, _)| c == required).is_none_or(|(_, v)| matches!(v, SqlValue::Null)) { return Err(format!("缺少必填项：{required}")); }
        }
    }
    let log_key = |row: &HashMap<String, SqlValue>| text(row.get(spec.log_key).unwrap_or(&SqlValue::Null)).unwrap_or_default();
    match old {
        Some(old) => {
            let changed: Vec<&(&str, SqlValue)> = values.iter().filter(|(c, v)| old.get(*c).map(text) != Some(text(v))).collect();
            if changed.is_empty() { return Ok(pk_value.cloned().unwrap()); }
            let sets = changed.iter().map(|(c, _)| format!("{c} = ?")).collect::<Vec<_>>().join(", ");
            let mut args: Vec<&dyn ToSql> = changed.iter().map(|(_, v)| v as &dyn ToSql).collect();
            let pk = to_sql(pk_value.unwrap());
            args.push(&pk);
            db.execute(&format!("UPDATE {} SET {sets}, updated_at = {NOW} WHERE {} = ?", spec.table, spec.pk), args.as_slice()).map_err(|e| e.to_string())?;
            let key = log_key(&old);
            for (column, value) in changed { log(db, spec.table, &key, column, old.get(*column).and_then(text).as_deref(), text(value).as_deref()).map_err(|e| e.to_string())?; }
            Ok(pk_value.cloned().unwrap())
        }
        None => {
            let mut columns: Vec<&str> = values.iter().map(|(c, _)| *c).collect();
            let mut args: Vec<SqlValue> = values.iter().map(|(_, v)| v.clone()).collect();
            if let Some(pk) = pk_value { columns.push(spec.pk); args.push(to_sql(pk)); }
            let marks = vec!["?"; columns.len()].join(", ");
            let sql = format!("INSERT INTO {} ({}{}updated_at) VALUES ({marks}{}{NOW})", spec.table, columns.join(", "), if columns.is_empty() { "" } else { ", " }, if columns.is_empty() { "" } else { ", " });
            db.execute(&sql, rusqlite::params_from_iter(args.iter())).map_err(|e| if e.to_string().contains("UNIQUE") { "已存在相同记录".to_owned() } else { e.to_string() })?;
            let pk = pk_value.cloned().unwrap_or_else(|| json!(db.last_insert_rowid()));
            let row: HashMap<String, SqlValue> = values.into_iter().map(|(c, v)| (c.to_owned(), v)).chain([(spec.pk.to_owned(), to_sql(&pk))]).collect();
            log(db, spec.table, &log_key(&row), "status", None, Some("created")).map_err(|e| e.to_string())?;
            Ok(pk)
        }
    }
}

pub fn delete_row(db: &Connection, spec: &Spec, pk: &str) -> Result<(), String> {
    let key: Option<String> = db.query_row(&format!("SELECT {} FROM {} WHERE {} = ?", spec.log_key, spec.table, spec.pk), [pk], |r| r.get(0)).ok();
    db.execute(&format!("DELETE FROM {} WHERE {} = ?", spec.table, spec.pk), [pk]).map_err(|e| e.to_string())?;
    if let Some(key) = key { log(db, spec.table, &key, "status", Some("present"), Some("deleted")).map_err(|e| e.to_string())?; }
    Ok(())
}

type St = State<Arc<AppState>>;

/// GET /api/m/{table}?column=value — exact-match filters on allowed columns; `prefix` matches target_key / key prefixes.
pub async fn list(State(s): St, Path(table): Path<String>, Query(filters): Query<HashMap<String, String>>) -> ApiResult {
    let spec = spec(&table)?;
    let mut sql = format!("SELECT * FROM {} WHERE 1 = 1", spec.table);
    let mut args: Vec<String> = Vec::new();
    for (column, value) in &filters {
        if column == "prefix" {
            let target = if spec.table == "rule" { "target_key" } else { spec.pk };
            args.push(value.clone()); args.push(format!("{value}."));
            sql.push_str(&format!(" AND ({target} = ?{} OR substr({target}, 1, length(?{n})) = ?{n})", args.len() - 1, n = args.len()));
        } else if spec.columns.contains(&column.as_str()) || column == spec.pk {
            args.push(value.clone());
            sql.push_str(&format!(" AND {column} = ?{}", args.len()));
        }
    }
    sql.push_str(&format!(" ORDER BY {}", if spec.table == "flow_step" { "product, flow, step_no" } else { spec.pk }));
    let refs: Vec<&dyn ToSql> = args.iter().map(|a| a as &dyn ToSql).collect();
    let db = s.db.lock().unwrap();
    Ok(Json(json!(rows(&db, &sql, &refs)?)))
}

pub async fn save(State(s): St, Path(table): Path<String>, Json(body): Json<Map<String, Value>>) -> ApiResult {
    let spec = spec(&table)?;
    let db = s.db.lock().unwrap();
    let pk = upsert(&db, spec, &body).map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(json!({ spec.pk: pk })))
}

pub async fn remove(State(s): St, Path((table, pk)): Path<(String, String)>) -> ApiResult {
    let spec = spec(&table)?;
    let db = s.db.lock().unwrap();
    delete_row(&db, spec, &pk).map_err(fail)?;
    Ok(Json(json!({ "ok": true })))
}
