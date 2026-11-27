//! Reads technical metadata from Redshift / PostgreSQL system tables.
use myquery_core::db::literal;
use serde_json::{Map, Value, json};
use std::collections::HashMap;
use tokio_postgres::{Client, SimpleQueryMessage};

type Row = Vec<Option<String>>;

pub struct Object {
    pub name: String,
    pub kind: &'static str, // table / view / mv
    pub comment: Option<String>,
    pub definition: Option<String>,
    pub props: Map<String, Value>,
}

pub struct Column {
    pub table: String,
    pub name: String,
    pub ordinal: i64,
    pub data_type: String,
    pub nullable: bool,
    pub comment: Option<String>,
    pub props: Map<String, Value>,
}

pub struct Catalog {
    pub objects: Vec<Object>,
    pub columns: Vec<Column>,
    /// Optional system views that could not be read (missing permission etc.); sync still succeeds.
    pub warnings: Vec<String>,
}

async fn rows(client: &Client, sql: &str) -> Result<Vec<Row>, String> {
    let messages = client.simple_query(sql).await.map_err(myquery_core::db::error)?;
    Ok(messages.into_iter().filter_map(|message| match message {
        SimpleQueryMessage::Row(row) => Some((0..row.len()).map(|i| row.get(i).map(str::to_owned)).collect()),
        _ => None,
    }).collect())
}

/// Optional queries: failure becomes a warning instead of failing the whole sync.
async fn optional(client: &Client, sql: &str, label: &str, warnings: &mut Vec<String>) -> Vec<Row> {
    rows(client, sql).await.unwrap_or_else(|error| { warnings.push(format!("{label}: {error}")); Vec::new() })
}

fn find<'a>(objects: &'a mut [Object], index: &HashMap<String, usize>, name: &str) -> Option<&'a mut Object> { index.get(name).map(|&i| &mut objects[i]) }
// Redshift system views return CHAR columns padded with trailing spaces.
fn text(row: &Row, i: usize) -> String { row[i].as_deref().unwrap_or_default().trim_end().to_owned() }
fn num(row: &Row, i: usize) -> Value { row[i].as_deref().and_then(|v| v.trim().parse::<f64>().ok()).map_or(Value::Null, |v| json!(v)) }
fn non_empty(value: &Option<String>) -> Option<String> { value.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(str::to_owned) }

pub async fn read(client: &Client, schema: &str, redshift: bool) -> Result<Catalog, String> {
    if redshift { redshift_catalog(client, schema).await } else { postgres_catalog(client, schema).await }
}

async fn redshift_catalog(client: &Client, schema: &str) -> Result<Catalog, String> {
    let s = literal(schema, true);
    let mut warnings = Vec::new();
    let mvs: HashMap<String, Row> = optional(client, &format!("SELECT name, state, autorefresh, is_stale FROM svv_mv_info WHERE database_name = current_database() AND schema_name = {s}"), "svv_mv_info", &mut warnings).await
        .into_iter().map(|row| (text(&row, 0), row)).collect();
    let mut objects = Vec::new();
    // MV storage tables (mv_tbl__*) are internal and not user objects.
    for row in rows(client, &format!("SELECT table_name, table_type FROM svv_tables WHERE table_catalog = current_database() AND table_schema = {s} AND LEFT(table_name, 8) <> 'mv_tbl__'")).await? {
        let name = text(&row, 0);
        let kind = if mvs.contains_key(&name) { "mv" } else if text(&row, 1).contains("VIEW") { "view" } else { "table" };
        let mut props = Map::new();
        if let Some(mv) = mvs.get(&name) {
            props.insert("mv_state".into(), json!(text(mv, 1)));
            props.insert("auto_refresh".into(), json!(mv[2].as_deref().is_some_and(|v| v == "t" || v == "true")));
            props.insert("is_stale".into(), json!(text(mv, 3) == "t"));
        }
        objects.push(Object { name, kind, comment: None, definition: None, props });
    }
    let index: HashMap<String, usize> = objects.iter().enumerate().map(|(i, o)| (o.name.clone(), i)).collect();

    for row in rows(client, &format!("SELECT c.relname, d.description FROM pg_description d JOIN pg_class c ON c.oid = d.objoid JOIN pg_namespace n ON n.oid = c.relnamespace WHERE d.objsubid = 0 AND n.nspname = {s}")).await? {
        if let Some(o) = find(&mut objects, &index, &text(&row, 0)) { o.comment = non_empty(&row[1]); }
    }
    for row in rows(client, &format!("SELECT viewname, definition FROM pg_views WHERE schemaname = {s}")).await? {
        if let Some(o) = find(&mut objects, &index, &text(&row, 0)) { o.definition = non_empty(&row[1]); }
    }
    for row in optional(client, &format!("SELECT \"table\", diststyle, sortkey1, sortkey_num, size, tbl_rows, encoded FROM svv_table_info WHERE \"database\" = current_database() AND \"schema\" = {s}"), "svv_table_info", &mut warnings).await {
        if let Some(o) = find(&mut objects, &index, &text(&row, 0)) {
            o.props.insert("diststyle".into(), json!(text(&row, 1)));
            o.props.insert("sortkey".into(), json!(text(&row, 2)));
            o.props.insert("sortkey_num".into(), num(&row, 3));
            o.props.insert("size_bytes".into(), num(&row, 4).as_f64().map_or(Value::Null, |mb| json!(mb * 1048576.0)));
            o.props.insert("rows".into(), num(&row, 5));
            o.props.insert("encoded".into(), json!(text(&row, 6)));
        }
    }
    for row in optional(client, &format!("SELECT mv_name, MAX(end_time) FROM sys_mv_refresh_history WHERE database_name = current_database() AND schema_name = {s} AND status ILIKE '%success%' GROUP BY 1"), "sys_mv_refresh_history", &mut warnings).await {
        if let Some(o) = find(&mut objects, &index, &text(&row, 0)) { o.props.insert("last_refresh".into(), json!(row[1])); }
    }

    let encodings: HashMap<(String, String), Row> = optional(client, &format!("SELECT tablename, \"column\", encoding, distkey, sortkey FROM pg_table_def WHERE schemaname = {s}"), "pg_table_def", &mut warnings).await
        .into_iter().map(|row| ((text(&row, 0), text(&row, 1)), row)).collect();
    let mut columns = Vec::new();
    for row in rows(client, &format!("SELECT table_name, column_name, ordinal_position, data_type, character_maximum_length, numeric_precision, numeric_scale, is_nullable, remarks FROM svv_columns WHERE table_catalog = current_database() AND table_schema = {s}")).await? {
        let table = text(&row, 0);
        if !index.contains_key(&table) { continue; }
        let base = text(&row, 3);
        let lower = base.to_lowercase();
        let data_type = match (&row[4], &row[5], &row[6]) {
            (Some(len), _, _) if lower.contains("char") => format!("{base}({len})"),
            (_, Some(p), Some(s)) if lower == "numeric" || lower == "decimal" => format!("{base}({p},{s})"),
            _ => base,
        };
        let name = text(&row, 1);
        let mut props = Map::new();
        props.insert("redshift".into(), json!(true));
        if let Some(def) = encodings.get(&(table.clone(), name.clone())) {
            props.insert("encoding".into(), json!(text(def, 2)));
            if def[3].as_deref().is_some_and(|v| v == "t" || v == "true") { props.insert("distkey".into(), json!(true)); }
            if let Some(sortkey) = def[4].as_deref().filter(|v| *v != "0") { props.insert("sortkey".into(), json!(sortkey)); }
        }
        columns.push(Column { table, name, ordinal: text(&row, 2).parse().unwrap_or(0), data_type, nullable: text(&row, 7) == "YES", comment: non_empty(&row[8]), props });
    }
    Ok(Catalog { objects, columns, warnings })
}

async fn postgres_catalog(client: &Client, schema: &str) -> Result<Catalog, String> {
    let s = literal(schema, false);
    let warnings = Vec::new();
    let relation = format!("pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = {s} AND c.relkind IN ('r','p','v','m','f')");
    let mut objects = Vec::new();
    for row in rows(client, &format!("SELECT c.relname, c.relkind, obj_description(c.oid, 'pg_class'), CASE WHEN c.relkind IN ('v','m') THEN pg_get_viewdef(c.oid, true) END, c.reltuples::bigint, pg_total_relation_size(c.oid) FROM {relation}")).await? {
        let kind = match row[1].as_deref() { Some("v") => "view", Some("m") => "mv", _ => "table" };
        let mut props = Map::new();
        props.insert("rows_estimate".into(), num(&row, 4));
        props.insert("size_bytes".into(), num(&row, 5));
        objects.push(Object { name: text(&row, 0), kind, comment: non_empty(&row[2]), definition: non_empty(&row[3]), props });
    }
    let index: HashMap<String, usize> = objects.iter().enumerate().map(|(i, o)| (o.name.clone(), i)).collect();
    for row in rows(client, &format!("SELECT c.relname, string_agg(a.attname, ',' ORDER BY array_position(con.conkey, a.attnum)) FROM pg_catalog.pg_constraint con JOIN pg_catalog.pg_class c ON c.oid = con.conrelid JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace JOIN pg_catalog.pg_attribute a ON a.attrelid = c.oid AND a.attnum = ANY(con.conkey) WHERE con.contype = 'p' AND n.nspname = {s} GROUP BY c.relname")).await? {
        if let Some(&i) = index.get(&text(&row, 0)) { objects[i].props.insert("primary_key".into(), json!(row[1])); }
    }
    for row in rows(client, &format!("SELECT tablename, string_agg(indexdef, E'\\n' ORDER BY indexname) FROM pg_catalog.pg_indexes WHERE schemaname = {s} GROUP BY tablename")).await? {
        if let Some(&i) = index.get(&text(&row, 0)) { objects[i].props.insert("indexes".into(), json!(row[1])); }
    }
    let mut columns = Vec::new();
    for row in rows(client, &format!("SELECT c.relname, a.attname, a.attnum, format_type(a.atttypid, a.atttypmod), NOT a.attnotnull, col_description(c.oid, a.attnum), t.typlen FROM pg_catalog.pg_attribute a JOIN pg_catalog.pg_class c ON c.oid = a.attrelid JOIN pg_catalog.pg_type t ON t.oid = a.atttypid JOIN pg_catalog.pg_namespace n ON n.oid = c.relnamespace WHERE n.nspname = {s} AND c.relkind IN ('r','p','v','m','f') AND a.attnum > 0 AND NOT a.attisdropped")).await? {
        let props = Map::from_iter([("type_bytes".into(), num(&row, 6))]);
        columns.push(Column { table: text(&row, 0), name: text(&row, 1), ordinal: text(&row, 2).parse().unwrap_or(0), data_type: text(&row, 3), nullable: text(&row, 4) == "t", comment: non_empty(&row[5]), props });
    }
    Ok(Catalog { objects, columns, warnings })
}
