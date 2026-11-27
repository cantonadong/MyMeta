use crate::{AppState, biz, jobs, lineage, manual, naming, store::{self, NOW}, sync::{self, Scope}, transfer};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{get, post},
};
use rusqlite::{Connection, ToSql, types::ValueRef};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::sync::Arc;

type St = State<Arc<AppState>>;
pub type ApiResult = Result<Json<Value>, (StatusCode, String)>;
const PAGE: i64 = 10;
/// Text columns that hold JSON and are returned as parsed values.
const JSON_COLUMNS: [&str; 8] = ["props", "summary", "naming", "key_pairs", "used_by", "writes", "reads", "info"];

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/connections", get(connections))
        .route("/api/settings", get(get_settings).put(put_settings))
        .route("/api/sync", post(run_sync))
        .route("/api/sync/status", get(sync_status))
        .route("/api/sync/runs", get(sync_runs))
        .route("/api/objects", get(objects).post(ordered_objects))
        .route("/api/object", get(biz::object))
        .route("/api/comment-sql", get(biz::comments))
        .route("/api/sql-template", get(biz::sql_template))
        .route("/api/sample", post(biz::sample))
        .route("/api/table-samples", post(biz::table_samples))
        .route("/api/freshness", post(biz::refresh_freshness))
        .route("/api/rename", post(biz::rename))
        .route("/api/comment-conflict", post(biz::resolve_comment))
        .route("/api/m/{table}", get(manual::list).put(manual::save))
        .route("/api/lineage/graph", get(lineage::graph))
        .route("/api/lineage/impact", get(lineage::impact))
        .route("/api/lineage/violations", get(lineage::violations))
        .route("/api/lineage/rebuild", post(lineage::rebuild))
        .route("/api/scripts", get(lineage::scripts))
        .route("/api/refresh/objects", get(lineage::refresh_objects))
        .route("/api/refresh/mybi", get(lineage::mybi_tasks))
        .route("/api/relations", get(relations))
        .route("/api/relations/graph", get(relation_graph))
        .route("/api/cleanup", get(jobs::cleanup_preview).post(jobs::cleanup_now))
        .route("/api/freshness/all", post(jobs::freshness_now))
        .route("/api/backup", post(jobs::backup_now))
        .route("/api/export.json", get(transfer::export_json))
        .route("/api/import.json", post(transfer::import_json))
        .route("/api/export.xlsx", get(transfer::export_xlsx))
        .route("/api/import.xlsx", post(transfer::import_xlsx))
        .route("/api/m/{table}/{pk}", axum::routing::delete(manual::remove))
        .route("/api/columns", get(columns).post(visible_columns))
        .route("/api/todos", get(todos))
        .route("/api/todos/{id}", post(update_todo))
        .route("/api/changes", get(changes))
}

pub fn fail(error: impl ToString) -> (StatusCode, String) { (StatusCode::INTERNAL_SERVER_ERROR, error.to_string()) }

/// Runs a query and returns rows as JSON objects keyed by column name.
pub fn rows(db: &Connection, sql: &str, args: &[&dyn ToSql]) -> Result<Vec<Value>, (StatusCode, String)> {
    let mut stmt = db.prepare(sql).map_err(fail)?;
    let names: Vec<String> = stmt.column_names().into_iter().map(str::to_owned).collect();
    let mut result = Vec::new();
    let mut query = stmt.query(args).map_err(fail)?;
    while let Some(row) = query.next().map_err(fail)? {
        let mut item = Map::new();
        for (i, name) in names.iter().enumerate() {
            let value = match row.get_ref(i).map_err(fail)? {
                ValueRef::Null => Value::Null,
                ValueRef::Integer(v) => json!(v),
                ValueRef::Real(v) => json!(v),
                ValueRef::Text(v) | ValueRef::Blob(v) => {
                    let text = String::from_utf8_lossy(v).into_owned();
                    // props columns hold JSON
                    if JSON_COLUMNS.contains(&name.as_str()) { serde_json::from_str(&text).unwrap_or(Value::String(text)) } else { Value::String(text) }
                }
            };
            item.insert(name.clone(), value);
        }
        result.push(Value::Object(item));
    }
    Ok(result)
}

/// Paginated list: `from_where` (FROM ... WHERE ...) is shared by count and page queries.
/// Arguments beyond those used in `from_where` (e.g. ORDER BY parameters) must come last; the count query skips them.
fn page(db: &Connection, select: &str, from_where: &str, order: &str, args: &[&dyn ToSql], page: i64) -> ApiResult {
    let used = (1..=args.len()).rev().find(|n| from_where.contains(&format!("?{n}"))).unwrap_or(0);
    let total: i64 = db.query_row(&format!("SELECT COUNT(*) {from_where}"), &args[..used], |r| r.get(0)).map_err(fail)?;
    let items = rows(db, &format!("SELECT {select} {from_where} ORDER BY {order} LIMIT {PAGE} OFFSET {}", page.max(0) * PAGE), args)?;
    Ok(Json(json!({ "total": total, "rows": items })))
}

fn like(q: &str) -> String { format!("%{}%", q.trim().replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")) }
const TMP: &str = "name LIKE '%\\_cd\\_tmp\\_%' ESCAPE '\\'";

async fn connections() -> ApiResult {
    let settings = myquery_core::config::load().map_err(fail)?;
    Ok(Json(json!(settings.profiles.iter().map(|p| json!({ "name": p.name, "redshift": p.redshift, "database": p.database })).collect::<Vec<_>>())))
}

fn settings_json(db: &Connection) -> Value {
    json!({
        "scopes": sync::scopes(db),
        "sync_interval_minutes": sync::interval_minutes(db),
        "naming": naming::config(db),
        "script_folders": store::setting(db, "script_folders").unwrap_or(json!([])),
        "freshness_time": store::setting(db, "freshness_time").unwrap_or(json!("03:00")),
        "cleanup": crate::jobs::cleanup_config(db),
        "mybi_path": crate::mybi::path(db),
        "rename_threshold": store::setting(db, "rename_threshold").unwrap_or(json!(0.8)),
    })
}

async fn get_settings(State(s): St) -> ApiResult {
    let db = s.db.lock().unwrap();
    Ok(Json(settings_json(&db)))
}

pub fn valid_time(v: &Value) -> bool { v.as_str().is_some_and(|t| t.len() == 5 && t[..2].parse::<u8>().is_ok_and(|h| h < 24) && &t[2..3] == ":" && t[3..].parse::<u8>().is_ok_and(|m| m < 60)) }

/// Partial update: only keys present in the body are validated and saved.
async fn put_settings(State(s): St, Json(body): Json<Map<String, Value>>) -> ApiResult {
    let bad = |m: &str| Err((StatusCode::BAD_REQUEST, m.to_owned()));
    for (key, value) in &body {
        match key.as_str() {
            "sync_interval_minutes" => if !value.as_u64().is_some_and(|m| (1..=1440).contains(&m)) { return bad("同步间隔需在 1–1440 分钟之间"); },
            "scopes" => match serde_json::from_value::<Vec<Scope>>(value.clone()) {
                Ok(scopes) if scopes.iter().all(|s| !s.profile.trim().is_empty() && !s.schema.trim().is_empty()) => {
                    if scopes.iter().any(|s| s.display_name.as_ref().is_some_and(|name| name.trim().chars().count() > 80)) {
                        return bad("范围显示名称不能超过 80 个字符");
                    }
                }
                _ => return bad("连接和模式不能为空"),
            },
            "freshness_time" => if !valid_time(value) { return bad("时间格式应为 HH:MM"); },
            "rename_threshold" => if !value.as_f64().is_some_and(|v| (0.5..=1.0).contains(&v)) { return bad("相似度阈值需在 0.5–1 之间"); },
            "naming" => if serde_json::from_value::<naming::Config>(value.clone()).is_err() { return bad("命名规则格式无效"); },
            "script_folders" => if !value.as_array().is_some_and(|a| a.iter().all(|f| f["path"].as_str().is_some_and(|p| !p.trim().is_empty()) && f["database"].as_str().is_some_and(|d| !d.is_empty()) && f["schema"].as_str().is_some_and(|d| !d.is_empty()))) { return bad("脚本目录、库和模式不能为空"); },
            "cleanup" => if let Err(e) = crate::jobs::validate_cleanup(value) { return bad(&e); },
            "mybi_path" => if value.as_str().is_none() { return bad("MyBI 路径无效"); },
            _ => return bad("未知设置项"),
        }
    }
    let db = s.db.lock().unwrap();
    for (key, value) in &body {
        let value = if key == "cleanup" { crate::jobs::merge_cleanup(&db, value) } else { value.clone() };
        store::set_setting(&db, key, &value).map_err(fail)?;
    }
    if body.contains_key("naming") { naming::refresh(&db, None).map_err(fail)?; }
    s.schedule_changed.notify_one();
    Ok(Json(settings_json(&db)))
}

async fn run_sync(State(s): St, body: Option<Json<Scope>>) -> ApiResult {
    let ids = sync::run(&s, body.map(|Json(scope)| scope), "manual").await;
    let db = s.db.lock().unwrap();
    let runs = ids.iter().map(|id| rows(&db, "SELECT * FROM sync_run WHERE id = ?", &[id]).map(|mut r| r.pop().unwrap_or_default())).collect::<Result<Vec<_>, _>>()?;
    Ok(Json(json!(runs)))
}

async fn sync_status(State(s): St) -> ApiResult {
    let running = s.sync_lock.try_lock().is_err();
    let db = s.db.lock().unwrap();
    let profiles = myquery_core::config::load().map(|c| c.profiles).unwrap_or_default();
    let mut scopes = Vec::new();
    for scope in sync::scopes(&db) {
        let database = profiles.iter().find(|p| p.name == scope.profile).map(|p| p.database.clone());
        let label = format!("{}.{}", database.as_deref().unwrap_or(&scope.profile), scope.schema);
        let last = rows(&db, "SELECT * FROM sync_run WHERE scope = ? ORDER BY id DESC LIMIT 1", &[&label])?.pop();
        let counts = rows(&db, "SELECT COUNT(*) AS objects, (SELECT COUNT(*) FROM column_tech c JOIN object_tech o ON o.key = c.object_key WHERE o.scope = ?1 AND c.is_deleted = 0) AS columns FROM object_tech WHERE scope = ?1 AND is_deleted = 0", &[&label])?.pop();
        let display_name = scope.display_name.as_deref().map(str::trim).filter(|name| !name.is_empty());
        scopes.push(json!({ "profile": scope.profile, "schema": scope.schema, "label": label, "display_name": display_name, "connection_found": database.is_some(), "last_run": last, "counts": counts }));
    }
    Ok(Json(json!({ "running": running, "interval_minutes": sync::interval_minutes(&db), "scopes": scopes })))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ListQuery { q: String, terms: String, scope: String, kind: String, product: String, layer: String, status: String, key: String, run: Option<i64>, tmp: bool, deleted: bool, page: i64, sort: String }

pub(crate) fn search_terms(text: &str, preset: &str) -> Result<Vec<String>, (StatusCode, String)> {
    let terms: Vec<String> = if preset.is_empty() { Vec::new() } else {
        serde_json::from_str(preset).map_err(|_| (StatusCode::BAD_REQUEST, "快筛条件格式无效".into()))?
    };
    if terms.len() > 32 { return Err((StatusCode::BAD_REQUEST, "快筛最多包含 32 项搜索条件".into())); }
    let mut keywords = Vec::new();
    for keyword in terms.iter().map(String::as_str).chain(std::iter::once(text)).flat_map(str::split_whitespace) {
        if keywords.iter().any(|saved: &String| saved == keyword) { continue; }
        if keywords.len() == 64 { return Err((StatusCode::BAD_REQUEST, "搜索关键词不能超过 64 个".into())); }
        keywords.push(keyword.to_owned());
    }
    Ok(keywords)
}

pub(crate) fn column_search(terms: &[String], args: &mut Vec<Box<dyn ToSql>>) -> String {
    let mut filter = String::new();
    for term in terms {
        args.push(Box::new(like(term)));
        let n = args.len();
        filter.push_str(&format!(" AND c.name LIKE ?{n} ESCAPE '\\'"));
    }
    filter
}

async fn sync_runs(State(s): St, Query(q): Query<ListQuery>) -> ApiResult {
    let db = s.db.lock().unwrap();
    page(&db, "*", "FROM sync_run", "id DESC", &[], q.page)
}

/// Shared WHERE for object / column lists. `alias` is the object_tech alias.
fn object_filters(q: &ListQuery, alias: &str, hidden: &[String], args: &mut Vec<Box<dyn ToSql>>) -> String {
    let mut sql = String::new();
    if !q.deleted { sql.push_str(&format!(" AND {alias}.is_deleted = 0")); }
    if !q.tmp { sql.push_str(&format!(" AND NOT {alias}.{TMP}")); }
    if !q.scope.is_empty() { args.push(Box::new(q.scope.clone())); sql.push_str(&format!(" AND {alias}.scope = ?{}", args.len())); }
    if !q.kind.is_empty() { args.push(Box::new(q.kind.clone())); sql.push_str(&format!(" AND {alias}.kind = ?{}", args.len())); }
    for (value, field) in [(&q.product, "product"), (&q.layer, "layer")] {
        if value.is_empty() { continue; }
        args.push(Box::new(value.clone()));
        sql.push_str(&format!(" AND COALESCE((SELECT {field} FROM object_biz WHERE key = {alias}.key), json_extract({alias}.naming, '$.{field}')) = ?{}", args.len()));
    }
    if !hidden.is_empty() {
        args.push(Box::new(json!(hidden).to_string()));
        sql.push_str(&format!(" AND {alias}.key NOT IN (SELECT value FROM json_each(?{}))", args.len()));
    }
    sql
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct CatalogView { order: Vec<String>, hidden: Vec<String>, bottom: Vec<String> }

impl CatalogView {
    fn validate(&self) -> Result<(), (StatusCode, String)> {
        for (keys, message) in [(&self.order, "对象排序列表过长"), (&self.hidden, "隐藏对象列表过长"), (&self.bottom, "移到底部的对象列表过长")] {
            if keys.len() > 50_000 || keys.iter().map(String::len).sum::<usize>() > 4_000_000 {
                return Err((StatusCode::BAD_REQUEST, message.into()));
            }
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum CatalogBody { Order(Vec<String>), View(CatalogView) }

async fn objects(State(s): St, Query(q): Query<ListQuery>) -> ApiResult {
    object_page(&s, q, &CatalogView::default())
}

async fn ordered_objects(State(s): St, Query(q): Query<ListQuery>, Json(body): Json<CatalogBody>) -> ApiResult {
    let view = match body { CatalogBody::Order(order) => CatalogView { order, ..Default::default() }, CatalogBody::View(view) => view };
    view.validate()?;
    object_page(&s, q, &view)
}

fn object_page(s: &AppState, q: ListQuery, view: &CatalogView) -> ApiResult {
    let mut args: Vec<Box<dyn ToSql>> = Vec::new();
    let mut filter = object_filters(&q, "o", &view.hidden, &mut args);
    let sort = match q.sort.as_str() {
        "name-asc" => Some("o.name, o.key"),
        "name-desc" => Some("o.name DESC, o.key"),
        "columns-desc" => Some("column_count DESC, o.name, o.key"),
        "rows-desc" => Some("NULLIF(COALESCE(json_extract(o.props, '$.rows'), json_extract(o.props, '$.rows_estimate')), -1) DESC, o.name, o.key"),
        "size-desc" => Some("json_extract(o.props, '$.size_bytes') DESC, o.name, o.key"),
        _ => None,
    };
    let mut order = sort.unwrap_or("o.name, o.key").to_owned();
    let terms = search_terms(&q.q, &q.terms)?;
    for term in &terms {
        args.push(Box::new(like(term)));
        let n = args.len();
        filter.push_str(&format!(" AND o.name LIKE ?{n} ESCAPE '\\'"));
    }
    if sort.is_none() && let Some(term) = terms.last() {
        let n = args.len();
        args.push(Box::new(term.to_lowercase()));
        order = format!("lower(o.name) <> ?{}, o.name LIKE ?{n} ESCAPE '\\' DESC, o.name, o.key", args.len());
    }
    if !view.order.is_empty() {
        args.push(Box::new(json!(view.order).to_string()));
        // Keep the order in the request. Materialize the key lookup once for all matching rows.
        order = format!("COALESCE((WITH ranks AS MATERIALIZED (SELECT value, CAST(key AS INTEGER) AS pos FROM json_each(?{})) SELECT pos FROM ranks WHERE value = o.key LIMIT 1), 2147483647), {order}", args.len());
    }
    if !view.bottom.is_empty() {
        args.push(Box::new(json!(view.bottom).to_string()));
        order = format!("COALESCE((WITH bottom AS MATERIALIZED (SELECT value, CAST(key AS INTEGER) + 1 AS pos FROM json_each(?{})) SELECT pos FROM bottom WHERE value = o.key LIMIT 1), 0), {order}", args.len());
    }
    let refs: Vec<&dyn ToSql> = args.iter().map(|a| a.as_ref()).collect();
    let db = s.db.lock().unwrap();
    page(&db, "o.key, o.scope, o.name, o.kind, o.comment, o.props, o.naming, o.is_deleted, o.synced_at, b.cn_name, b.description, COALESCE(b.layer, json_extract(o.naming, '$.layer')) AS layer, COALESCE(b.product, json_extract(o.naming, '$.product')) AS product, (SELECT COUNT(*) FROM column_tech c WHERE c.object_key = o.key AND c.is_deleted = 0) AS column_count",
        &format!("FROM object_tech o LEFT JOIN object_biz b ON b.key = o.key WHERE 1 = 1{filter}"), &order, &refs, q.page)
}

async fn columns(State(s): St, Query(q): Query<ListQuery>) -> ApiResult {
    column_page(&s, q, &[])
}

async fn visible_columns(State(s): St, Query(q): Query<ListQuery>, Json(view): Json<CatalogView>) -> ApiResult {
    view.validate()?;
    column_page(&s, q, &view.hidden)
}

fn column_page(s: &AppState, q: ListQuery, hidden: &[String]) -> ApiResult {
    let mut args: Vec<Box<dyn ToSql>> = Vec::new();
    let mut filter = object_filters(&q, "o", hidden, &mut args);
    if !q.deleted { filter.push_str(" AND c.is_deleted = 0"); }
    let mut order = "c.name, o.name, c.key".to_owned();
    let terms = search_terms(&q.q, &q.terms)?;
    filter.push_str(&column_search(&terms, &mut args));
    if let Some(term) = terms.last() {
        args.push(Box::new(term.to_lowercase()));
        order = format!("lower(c.name) <> ?{}, c.name, o.name, c.key", args.len());
    }
    let refs: Vec<&dyn ToSql> = args.iter().map(|a| a.as_ref()).collect();
    let db = s.db.lock().unwrap();
    page(&db, "c.key, c.name, c.data_type, c.props, c.nullable, c.comment, c.is_deleted, cb.meaning, cb.notes, cb.sample, o.key AS object_key, o.name AS object_name, o.scope, o.kind",
        &format!("FROM column_tech c JOIN object_tech o ON o.key = c.object_key LEFT JOIN column_biz cb ON cb.key = c.key WHERE 1 = 1{filter}"), &order, &refs, q.page)
}

async fn todos(State(s): St, Query(q): Query<ListQuery>) -> ApiResult {
    let status = if q.status.is_empty() { "open".to_owned() } else { q.status.clone() };
    let mut where_sql = "FROM todo t LEFT JOIN object_tech o ON o.key = t.key WHERE t.status = ?1".to_owned();
    let mut args: Vec<&dyn ToSql> = vec![&status];
    if !q.kind.is_empty() { where_sql.push_str(" AND t.kind = ?2"); args.push(&q.kind); }
    let db = s.db.lock().unwrap();
    page(&db, "t.*, o.kind AS object_kind", &where_sql, "t.id DESC", &args, q.page)
}

#[derive(Deserialize)]
struct TodoBody { status: String }

async fn update_todo(State(s): St, Path(id): Path<i64>, Json(body): Json<TodoBody>) -> ApiResult {
    if !["open", "done", "ignored"].contains(&body.status.as_str()) { return Err((StatusCode::BAD_REQUEST, "无效状态".into())); }
    let db = s.db.lock().unwrap();
    // Reopening fails if an open to-do for the same item already exists (unique index).
    db.execute(&format!("UPDATE todo SET status = ?1, handler = CASE WHEN ?1 = 'open' THEN NULL ELSE 'admin' END, handled_at = CASE WHEN ?1 = 'open' THEN NULL ELSE {NOW} END WHERE id = ?2"), rusqlite::params![body.status, id])
        .map_err(|_| (StatusCode::CONFLICT, "该对象已有未处理的同类待办".to_owned()))?;
    Ok(Json(json!({ "ok": true })))
}

async fn changes(State(s): St, Query(q): Query<ListQuery>) -> ApiResult {
    let mut where_sql = "FROM change_log WHERE 1 = 1".to_owned();
    let mut args: Vec<&dyn ToSql> = Vec::new();
    // key filter matches the object itself and all its columns
    let prefix = format!("{}.", q.key);
    if !q.key.is_empty() { where_sql.push_str(" AND (key = ?1 OR substr(key, 1, length(?2)) = ?2)"); args.push(&q.key); args.push(&prefix); }
    if let Some(run) = &q.run { where_sql.push_str(&format!(" AND run_id = ?{}", args.len() + 1)); args.push(run); }
    let db = s.db.lock().unwrap();
    page(&db, "*", &where_sql, "id DESC", &args, q.page)
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct RelationQuery { key: String, status: String, depth: usize }

/// Relations with object names; `key` limits to relations touching that object.
async fn relations(State(s): St, Query(q): Query<RelationQuery>) -> ApiResult {
    let status = if q.status.is_empty() { "confirmed".to_owned() } else { q.status.clone() };
    let db = s.db.lock().unwrap();
    let items = rows(&db, "SELECT r.*, l.name AS left_name, rt.name AS right_name, l.scope <> rt.scope AS cross_db FROM relation r LEFT JOIN object_tech l ON l.key = r.left_key LEFT JOIN object_tech rt ON rt.key = r.right_key WHERE r.status = ?1 AND (?2 = '' OR r.left_key = ?2 OR r.right_key = ?2) ORDER BY r.updated_at DESC, r.id DESC", &[&status, &q.key])?;
    Ok(Json(json!(items)))
}

/// One- or two-degree relation graph around an object (F4.4).
async fn relation_graph(State(s): St, Query(q): Query<RelationQuery>) -> ApiResult {
    let db = s.db.lock().unwrap();
    let all = rows(&db, "SELECT id, left_key, right_key, kind, key_pairs, diff_note FROM relation WHERE status = 'confirmed'", &[])?;
    let mut depth: std::collections::HashMap<String, i64> = std::collections::HashMap::from([(q.key.clone(), 0)]);
    let mut frontier = vec![q.key.clone()];
    for d in 1..=q.depth.clamp(1, 2) as i64 {
        let mut next = Vec::new();
        for r in &all {
            let (l, rt) = (r["left_key"].as_str().unwrap_or_default(), r["right_key"].as_str().unwrap_or_default());
            for (a, b) in [(l, rt), (rt, l)] {
                if frontier.iter().any(|f| f == a) && !depth.contains_key(b) { depth.insert(b.to_owned(), d); next.push(b.to_owned()); }
            }
        }
        frontier = next;
    }
    let edges: Vec<&Value> = all.iter().filter(|r| depth.contains_key(r["left_key"].as_str().unwrap_or_default()) && depth.contains_key(r["right_key"].as_str().unwrap_or_default())).collect();
    let nodes = lineage::node_info(&db, &depth.into_iter().collect::<Vec<_>>());
    Ok(Json(json!({ "nodes": nodes, "edges": edges })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn catalog_search_filters_before_pagination_and_filters_expanded_fields() {
        let db = store::open(std::path::Path::new(":memory:")).unwrap();
        db.execute_batch("
            WITH RECURSIVE seq(n) AS (SELECT 1 UNION ALL SELECT n+1 FROM seq WHERE n<24)
            INSERT INTO object_tech(key,scope,name,kind,first_seen,synced_at)
            SELECT printf('demo.public.a_%02d',n),'demo.public',printf('a_%02d',n),'table','x','x' FROM seq;
            INSERT INTO object_tech(key,scope,name,kind,first_seen,synced_at) VALUES
              ('demo.public.z_target','demo.public','z_target','table','x','x'),
              ('other.public.z_target','other.public','z_target','table','x','x');
            INSERT INTO column_tech(key,object_key,name,ordinal,data_type,nullable,first_seen) VALUES
              ('demo.public.z_target.needle','demo.public.z_target','needle',1,'text',0,'x'),
              ('demo.public.z_target.extra','demo.public.z_target','extra',2,'text',0,'x'),
              ('other.public.z_target.needle','other.public.z_target','needle',1,'text',0,'x');
        ").unwrap();
        let state = Arc::new(AppState {
            db: db.into(), sync_lock: Default::default(), sample_lock: Default::default(), schedule_changed: Default::default(), data_dir: Default::default(),
        });
        let query = |q: &str, terms: &str| ListQuery {
            scope: "demo.public".into(), q: q.into(), terms: terms.into(), ..Default::default()
        };
        let Json(browse) = object_page(&state, query("", ""), &CatalogView::default()).unwrap();
        assert_eq!(browse["total"], 25);
        assert!(browse["rows"].as_array().unwrap().iter().all(|row| row["name"] != "z_target"));
        let preset = r#"["z_target"]"#;
        let Json(field_only) = object_page(&state, query("needle", preset), &CatalogView::default()).unwrap();
        assert_eq!(field_only["total"], 0);
        let Json(objects) = object_page(&state, query("z_target", ""), &CatalogView {
            order: vec!["demo.public.a_01".into()], ..Default::default()
        }).unwrap();
        assert_eq!(objects["total"], 1);
        assert_eq!(objects["rows"][0]["key"], "demo.public.z_target");
        {
            let db = state.db.lock().unwrap();
            db.execute("UPDATE object_tech SET comment = 'database comment' WHERE key = 'demo.public.z_target'", []).unwrap();
            let spec = crate::manual::SPECS.iter().find(|spec| spec.table == "object_biz").unwrap();
            crate::manual::upsert(&db, spec, json!({"key": "demo.public.z_target", "description": ""}).as_object().unwrap()).unwrap();
        }
        let Json(cleared) = object_page(&state, query("z_target", ""), &CatalogView::default()).unwrap();
        assert_eq!(cleared["rows"][0]["description"], "");
        assert_eq!(cleared["rows"][0]["comment"], "database comment");
        let Json(table_only) = column_page(&state, query("z_target", ""), &[]).unwrap();
        assert_eq!(table_only["total"], 0);
        let Json(columns) = column_page(&state, query("needle", ""), &[]).unwrap();
        assert_eq!(columns["total"], 1);
        let Json(detail) = biz::object(State(state), Query(biz::ObjectQuery {
            key: "demo.public.z_target".into(), q: "needle".into(), terms: String::new(),
        })).await.unwrap();
        assert_eq!(detail["columns"].as_array().unwrap().len(), 1);
        assert_eq!(detail["columns"][0]["key"], columns["rows"][0]["key"]);
    }
}
