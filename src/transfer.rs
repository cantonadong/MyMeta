//! Import / export of manual metadata (F9.3): JSON for backup and V1 → V2 migration, Excel for reading and bulk editing.
use crate::{AppState, api::{ApiResult, fail, rows}, jobs, manual, naming, store::{self, NOW}};
use axum::{Json, body::Bytes, extract::State, http::{StatusCode, header}, response::{IntoResponse, Response}};
use calamine::{Data, Reader, Xlsx};
use rusqlite::{Connection, ToSql};
use serde_json::{Map, Value, json};
use std::sync::Arc;

type St = State<Arc<AppState>>;
const TABLES: [&str; 6] = ["object_biz", "column_biz", "rule", "relation", "flow_step", "metric"];
const SETTINGS: [&str; 7] = ["scopes", "sync_interval_minutes", "naming", "script_folders", "freshness_time", "mybi_path", "rename_threshold"];

fn download(bytes: Vec<u8>, kind: &str, name: &str) -> Response {
    ([(header::CONTENT_TYPE, kind.to_owned()), (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\""))], bytes).into_response()
}

fn date(db: &Connection) -> String { db.query_row("SELECT strftime('%Y%m%d', 'now')", [], |r| r.get(0)).unwrap_or_default() }

pub async fn export_json(State(s): St) -> Result<Response, (StatusCode, String)> {
    let db = s.db.lock().unwrap();
    let mut tables = Map::new();
    for table in TABLES { tables.insert(table.into(), json!(rows(&db, &format!("SELECT * FROM {table}"), &[])?)); }
    let settings: Map<String, Value> = SETTINGS.iter().filter_map(|k| store::setting(&db, k).map(|v| (k.to_string(), v))).collect();
    let body = json!({ "format": "mymeta", "version": 1, "exported_at": db.query_row(&format!("SELECT {NOW}"), [], |r| r.get::<_, String>(0)).map_err(fail)?, "settings": settings, "tables": tables });
    Ok(download(serde_json::to_vec_pretty(&body).map_err(fail)?, "application/json", &format!("mymeta-{}.json", date(&db))))
}

/// Replaces all manual content with the file's content (restore semantics). A backup is taken first.
pub async fn import_json(State(s): St, Json(body): Json<Value>) -> ApiResult {
    if body["format"] != "mymeta" || !body["tables"].is_object() { return Err((StatusCode::BAD_REQUEST, "不是 MyMeta 导出文件".into())); }
    let backup = jobs::backup(&s, "-before-import").map_err(fail)?;
    let mut db = s.db.lock().unwrap();
    let tx = db.transaction().map_err(fail)?;
    let mut counts = Map::new();
    for table in TABLES {
        let columns: Vec<String> = { let mut stmt = tx.prepare(&format!("SELECT name FROM pragma_table_info('{table}')")).map_err(fail)?; stmt.query_map([], |r| r.get(0)).map_err(fail)?.flatten().collect() };
        tx.execute(&format!("DELETE FROM {table}"), []).map_err(fail)?;
        let items = body["tables"][table].as_array().cloned().unwrap_or_default();
        for item in &items {
            let present: Vec<&String> = columns.iter().filter(|c| item.get(c.as_str()).is_some()).collect();
            if present.is_empty() { continue; }
            let values: Vec<rusqlite::types::Value> = present.iter().map(|c| match &item[c.as_str()] {
                Value::Null => rusqlite::types::Value::Null,
                Value::Number(n) => n.as_i64().map(rusqlite::types::Value::Integer).unwrap_or(rusqlite::types::Value::Real(n.as_f64().unwrap_or(0.0))),
                Value::String(t) => rusqlite::types::Value::Text(t.clone()),
                Value::Bool(b) => rusqlite::types::Value::Integer(*b as i64),
                other => rusqlite::types::Value::Text(other.to_string()),
            }).collect();
            tx.execute(&format!("INSERT INTO {table} ({}) VALUES ({})", present.iter().map(|c| c.as_str()).collect::<Vec<_>>().join(", "), vec!["?"; present.len()].join(", ")), rusqlite::params_from_iter(values.iter())).map_err(|e| (StatusCode::BAD_REQUEST, format!("{table}: {e}")))?;
        }
        counts.insert(table.into(), json!(items.len()));
    }
    if let Some(settings) = body["settings"].as_object() {
        for (k, v) in settings { if SETTINGS.contains(&k.as_str()) { store::set_setting(&tx, k, v).map_err(fail)?; } }
    }
    naming::refresh(&tx, None).map_err(fail)?;
    manual::log(&tx, "system", "import", "json", None, Some(&Value::Object(counts.clone()).to_string())).map_err(fail)?;
    tx.commit().map_err(fail)?;
    Ok(Json(json!({ "counts": counts, "backup": backup })))
}

// Excel layout: sheet name, query, (header, column) pairs. The first column is the business key used on import.
const OBJECT_COLUMNS: [(&str, &str); 14] = [("业务键", "key"), ("类型", "kind"), ("数据库注释", "comment"), ("中文名", "cn_name"), ("用途说明", "description"), ("产品", "product"), ("层级", "layer"), ("粒度", "grain"), ("主键/业务键", "primary_key"), ("来源表", "source_tables"), ("刷新方式", "refresh_method"), ("维护人", "maintainer"), ("是否临时对象", "is_temporary"), ("日期字段", "date_column")];
const COLUMN_COLUMNS: [(&str, &str); 11] = [("业务键", "key"), ("字段类型", "data_type"), ("数据库注释", "comment"), ("业务含义", "meaning"), ("单位", "unit"), ("币种", "currency"), ("时区与切日", "timezone"), ("是否敏感", "is_sensitive"), ("注意事项", "notes"), ("所在对象", "object_key"), ("示例值", "sample")];
const RULE_COLUMNS: [(&str, &str); 6] = [("ID", "id"), ("目标（对象或字段）", "target_key"), ("规则类型", "kind"), ("取值", "value"), ("SQL 条件", "condition"), ("说明", "note")];
const READ_ONLY: [&str; 4] = ["kind", "comment", "data_type", "object_key"];

pub async fn export_xlsx(State(s): St) -> Result<Response, (StatusCode, String)> {
    let db = s.db.lock().unwrap();
    let mut book = rust_xlsxwriter::Workbook::new();
    let bold = rust_xlsxwriter::Format::new().set_bold();
    let sheets: [(&str, &[(&str, &str)], &str); 3] = [
        ("对象", &OBJECT_COLUMNS, "SELECT o.key, o.kind, o.comment, b.cn_name, b.description, b.product, b.layer, b.grain, b.primary_key, b.source_tables, b.refresh_method, b.maintainer, b.is_temporary, b.date_column FROM object_tech o LEFT JOIN object_biz b ON b.key = o.key WHERE o.is_deleted = 0 ORDER BY o.key"),
        ("字段", &COLUMN_COLUMNS, "SELECT c.key, c.data_type, c.comment, b.meaning, b.unit, b.currency, b.timezone, b.is_sensitive, b.notes, c.object_key, b.sample FROM column_tech c JOIN object_tech o ON o.key = c.object_key LEFT JOIN column_biz b ON b.key = c.key WHERE c.is_deleted = 0 AND o.is_deleted = 0 ORDER BY c.object_key, c.ordinal"),
        ("规则", &RULE_COLUMNS, "SELECT id, target_key, kind, value, condition, note FROM rule ORDER BY target_key, id"),
    ];
    for (name, columns, sql) in sheets {
        let sheet = book.add_worksheet().set_name(name).map_err(fail)?;
        for (i, (header, _)) in columns.iter().enumerate() { sheet.write_string_with_format(0, i as u16, *header, &bold).map_err(fail)?; sheet.set_column_width(i as u16, if i == 0 { 48 } else { 18 }).map_err(fail)?; }
        let mut stmt = db.prepare(sql).map_err(fail)?;
        let mut result = stmt.query([]).map_err(fail)?;
        let mut r = 1u32;
        while let Some(row) = result.next().map_err(fail)? {
            for i in 0..columns.len() {
                match row.get_ref(i).map_err(fail)? {
                    rusqlite::types::ValueRef::Integer(v) => { sheet.write_number(r, i as u16, v as f64).map_err(fail)?; }
                    rusqlite::types::ValueRef::Real(v) => { sheet.write_number(r, i as u16, v).map_err(fail)?; }
                    rusqlite::types::ValueRef::Text(t) => { sheet.write_string(r, i as u16, String::from_utf8_lossy(t)).map_err(fail)?; }
                    _ => {}
                }
            }
            r += 1;
        }
        sheet.set_freeze_panes(1, 1).map_err(fail)?;
    }
    let bytes = book.save_to_buffer().map_err(fail)?;
    Ok(download(bytes, "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", &format!("mymeta-{}.xlsx", date(&db))))
}

fn cell(data: &Data) -> Value {
    match data {
        Data::Empty => Value::Null,
        Data::Int(i) => json!(i),
        Data::Float(f) if f.fract() == 0.0 => json!(*f as i64),
        Data::Float(f) => json!(f),
        Data::Bool(b) => json!(*b as i64),
        other => json!(other.to_string()),
    }
}

/// Updates business fields from an exported workbook (matched by key); empty cells clear values. Changes are logged.
pub async fn import_xlsx(State(s): St, body: Bytes) -> ApiResult {
    let mut book: Xlsx<_> = Xlsx::new(std::io::Cursor::new(body.to_vec())).map_err(|e| (StatusCode::BAD_REQUEST, format!("无法读取 Excel：{e}")))?;
    let backup = jobs::backup(&s, "-before-import").map_err(fail)?;
    let mut db = s.db.lock().unwrap();
    let tx = db.transaction().map_err(fail)?;
    let mut counts = Map::new();
    for (sheet, columns, table) in [("对象", &OBJECT_COLUMNS[..], "object_biz"), ("字段", &COLUMN_COLUMNS[..], "column_biz"), ("规则", &RULE_COLUMNS[..], "rule")] {
        let Ok(range) = book.worksheet_range(sheet) else { continue };
        let mut lines = range.rows();
        let Some(header) = lines.next() else { continue };
        let index: Vec<Option<&str>> = header.iter().map(|h| columns.iter().find(|(name, _)| *name == h.to_string().trim()).map(|(_, c)| *c)).collect();
        let spec = manual::SPECS.iter().find(|s| s.table == table).unwrap();
        let mut n = 0;
        for line in lines {
            let mut row = Map::new();
            for (i, column) in index.iter().enumerate() {
                if let Some(column) = column && !READ_ONLY.contains(column) { row.insert(column.to_string(), line.get(i).map(cell).unwrap_or(Value::Null)); }
            }
            let key_field = if table == "rule" { "target_key" } else { "key" };
            if row.get(key_field).is_none_or(Value::is_null) { continue; }
            if table != "rule" {
                // Skip rows that carry no business content and have nothing stored yet.
                let empty = row.iter().all(|(k, v)| k == "key" || v.is_null());
                let exists: bool = tx.query_row(&format!("SELECT EXISTS(SELECT 1 FROM {table} WHERE key = ?)"), [row["key"].as_str().unwrap_or_default()], |r| r.get(0)).map_err(fail)?;
                if empty && !exists { continue; }
            }
            manual::upsert(&tx, spec, &row).map_err(|e| (StatusCode::BAD_REQUEST, format!("{sheet}: {e}")))?;
            n += 1;
        }
        counts.insert(table.into(), json!(n));
    }
    manual::log(&tx, "system", "import", "xlsx", None, Some(&Value::Object(counts.clone()).to_string())).map_err(fail)?;
    tx.commit().map_err(fail)?;
    Ok(Json(json!({ "counts": counts, "backup": backup })))
}

/// Built-in industry metric definitions (F7.3), inserted once.
pub fn seed_metrics(db: &Connection) {
    let seeded: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM metric WHERE builtin = 1)", [], |r| r.get(0)).unwrap_or(true);
    if seeded { return; }
    let metrics: [(&str, &str, &str, &str, &str, &str); 10] = [
        ("DAU", "Daily active users", "当日有任意活跃行为的去重用户数", "COUNT(DISTINCT user_id)", "", ""),
        ("新增用户", "New users", "当日首次活跃（首次启动）的去重用户数", "COUNT(DISTINCT user_id) WHERE first_active_date = dt", "", ""),
        ("D1 留存率", "D1 retention", "某日新增用户中，第 1 天（次日）再次活跃的比例", "D1 活跃新增用户数 / 当日新增用户数", "第 1 天仍活跃的当日新增用户数", "当日新增用户数"),
        ("D7 留存率", "D7 retention", "某日新增用户中，第 7 天再次活跃的比例", "D7 活跃新增用户数 / 当日新增用户数", "第 7 天仍活跃的当日新增用户数", "当日新增用户数"),
        ("D30 留存率", "D30 retention", "某日新增用户中，第 30 天再次活跃的比例", "D30 活跃新增用户数 / 当日新增用户数", "第 30 天仍活跃的当日新增用户数", "当日新增用户数"),
        ("付费率", "Payer rate", "活跃用户中当期有付费的用户比例", "付费用户数 / 活跃用户数", "付费用户数", "活跃用户数"),
        ("ARPU", "Average revenue per user", "活跃用户平均收入", "收入（USD）/ 活跃用户数", "收入（USD）", "活跃用户数"),
        ("ARPPU", "Average revenue per paying user", "付费用户平均收入", "收入（USD）/ 付费用户数", "收入（USD）", "付费用户数"),
        ("ARPDAU", "Average revenue per daily active user", "每日活跃用户平均收入", "当日收入（USD）/ DAU", "当日收入（USD）", "DAU"),
        ("收入", "Revenue", "IAP 等收入，统一换算为 USD", "SUM(revenue_usd)", "", ""),
    ];
    for (cn, en, definition, formula, numerator, denominator) in metrics {
        let none = |s: &str| (!s.is_empty()).then(|| s.to_owned());
        let args: [&dyn ToSql; 6] = [&cn, &en, &definition, &formula, &none(numerator), &none(denominator)];
        db.execute(&format!("INSERT INTO metric(name_cn, name_en, definition, formula, numerator, denominator, builtin, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, {NOW})"), args.as_slice()).ok();
    }
}
