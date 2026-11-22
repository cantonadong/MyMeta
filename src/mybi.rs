//! Reads the local MyBI database (read-only): datasets → lineage into dashboards (F6.8), scheduled tasks (F10.2).
use crate::{lineage::{Analysis, Ctx, JoinPair, store_analysis}, store};
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};

pub fn path(db: &Connection) -> Value {
    store::setting(db, "mybi_path").unwrap_or(json!("C:\\Program\\MyBI\\data-workbench.db"))
}

fn upsert_node(db: &Connection, key: &str, name: &str, kind: &str, info: &Value) {
    db.execute("INSERT INTO node(key, name, kind, info) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(key) DO UPDATE SET name = excluded.name, kind = excluded.kind, info = excluded.info", (key, name, kind, info.to_string())).ok();
}

/// Rebuilds MyBI nodes and lineage. Returns a summary (or the reason it was skipped).
pub fn rebuild(db: &Connection, columns: &HashMap<String, Vec<String>>, redshift_db: &HashMap<String, bool>, joins: &mut Vec<JoinPair>) -> Value {
    let Some(path) = path(db).as_str().map(str::to_owned).filter(|p| !p.trim().is_empty()) else { return json!({ "skipped": "no path" }) };
    if !std::path::Path::new(&path).exists() { return json!({ "skipped": "not found" }); }
    let mybi = match Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX) { Ok(c) => c, Err(e) => return json!({ "error": e.to_string() }) };
    let read = |sql: &str| -> Vec<Vec<Option<String>>> {
        let Ok(mut stmt) = mybi.prepare(sql) else { return Vec::new() };
        let n = stmt.column_count();
        stmt.query_map([], |r| Ok((0..n).map(|i| r.get::<_, Option<String>>(i).ok().flatten()).collect())).map(|rows| rows.flatten().collect()).unwrap_or_default()
    };
    let parse = |text: &Option<String>| -> Value { text.as_deref().and_then(|t| serde_json::from_str(t).ok()).unwrap_or(Value::Null) };
    let connections: HashMap<String, Value> = read("SELECT id, config FROM saved_connections").into_iter().map(|r| (r[0].clone().unwrap_or_default(), parse(&r[1]))).collect();
    db.execute("DELETE FROM lineage WHERE source IN ('mybi', 'mybi_task')", []).ok();
    db.execute("DELETE FROM node WHERE kind LIKE 'mybi%'", []).ok();
    let target_db = |config: &Value| -> (String, bool) {
        let connection = config["connection_id"].as_str().and_then(|id| connections.get(id));
        let database = config["database"].as_str().filter(|d| !d.is_empty()).or_else(|| connection.and_then(|c| c["database"].as_str())).unwrap_or("").to_owned();
        let redshift = connection.and_then(|c| c["redshift"].as_bool()).or_else(|| redshift_db.get(&database).copied()).unwrap_or(true);
        (database, redshift)
    };
    let (mut datasets, mut tasks, mut reports) = (0, 0, 0);
    // Connected datasets
    for r in read("SELECT d.id, d.name, s.config, s.schedule, s.last_run_at, s.last_error, CAST(s.duration_seconds AS TEXT) FROM dynamic_sources s JOIN datasets d ON d.id = s.dataset_id") {
        let (id, name, config) = (r[0].clone().unwrap_or_default(), r[1].clone().unwrap_or_default(), parse(&r[2]));
        let key = format!("mybi:dataset:{id}");
        let (database, redshift) = target_db(&config);
        upsert_node(db, &key, &name, "mybi_dataset", &json!({ "schedule": parse(&r[3]), "last_run_at": r[4], "last_error": r[5], "duration_seconds": r[6], "mode": config["mode"], "database": database }));
        let ctx = Ctx { database: &database, schema: "public", redshift, columns };
        let analysis = match (config["mode"].as_str(), config["sql"].as_str()) {
            (Some("sql"), Some(sql)) => ctx.analyze(sql, Some(&key)),
            _ => {
                let mut a = Analysis::default();
                let (schema, object) = (config["schema_name"].as_str().unwrap_or("public"), config["object_name"].as_str().unwrap_or(""));
                if !object.is_empty() && !database.is_empty() { a.edges.insert((format!("{database}.{}.{object}", if schema.is_empty() { "public" } else { schema }), key.clone())); }
                a
            }
        };
        // Dataset columns are not database objects; keep table-level edges only.
        let table_only = Analysis { edges: analysis.edges, ..Default::default() };
        store_analysis(db, "mybi", &key, &table_only).ok();
        joins.extend(analysis.joins);
        datasets += 1;
    }
    // Scheduled queries (tasks that write database objects)
    for r in read("SELECT id, name, config, schedule, last_run_at, last_error, CAST(duration_seconds AS TEXT), CAST(row_count AS TEXT) FROM scheduled_queries") {
        let (id, name, config) = (r[0].clone().unwrap_or_default(), r[1].clone().unwrap_or_default(), parse(&r[2]));
        let key = format!("mybi:task:{id}");
        let (database, redshift) = target_db(&config);
        upsert_node(db, &key, &name, "mybi_task", &json!({ "schedule": parse(&r[3]), "last_run_at": r[4], "last_error": r[5], "duration_seconds": r[6], "row_count": r[7], "database": database }));
        if let Some(sql) = config["sql"].as_str() {
            let analysis = Ctx { database: &database, schema: "public", redshift, columns }.analyze(sql, None);
            store_analysis(db, "mybi_task", &key, &analysis).ok();
            joins.extend(analysis.joins);
        }
        tasks += 1;
    }
    // Dashboards → the datasets their charts use
    for r in read("SELECT id, name, document FROM dashboards") {
        let (id, name, document) = (r[0].clone().unwrap_or_default(), r[1].clone().unwrap_or_default(), parse(&r[2]));
        let key = format!("mybi:report:{id}");
        let used: BTreeSet<String> = document["charts"].as_array().into_iter().flatten().filter_map(|c| c["datasetId"].as_str().map(|d| format!("mybi:dataset:{d}"))).collect();
        upsert_node(db, &key, &name, "mybi_report", &json!({ "charts": document["charts"].as_array().map_or(0, Vec::len) }));
        let mut a = Analysis::default();
        for dataset in used {
            let known: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM node WHERE key = ?)", [&dataset], |r| r.get(0)).unwrap_or(false);
            if known { a.edges.insert((dataset, key.clone())); }
        }
        store_analysis(db, "mybi", &key, &a).ok();
        reports += 1;
    }
    json!({ "datasets": datasets, "tasks": tasks, "reports": reports })
}
