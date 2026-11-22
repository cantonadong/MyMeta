//! SQL lineage: table- and column-level edges from view definitions, ETL scripts and MyBI datasets (F6, F10.1, F4.3).
use crate::{AppState, api::{ApiResult, fail, rows}, biz, manual, store::{self, NOW}};
use axum::{Json, extract::{Query as Q, State}};
use regex::Regex;
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::{Value, json};
use sqlparser::{
    ast::{BinaryOperator, Expr, Query, SelectItem, SetExpr, TableFactor, visit_expressions, visit_relations},
    dialect::{Dialect, PostgreSqlDialect, RedshiftSqlDialect},
    parser::Parser,
};
use std::{cell::RefCell, collections::{BTreeSet, HashMap, HashSet, VecDeque}, ops::ControlFlow, path::Path, rc::Rc, sync::{Arc, LazyLock}};

/// Catalog lookups used while resolving names.
pub struct Ctx<'a> {
    pub database: &'a str,
    pub schema: &'a str,
    pub redshift: bool,
    pub columns: &'a HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ColumnEdge { pub up: String, pub up_col: String, pub down: String, pub down_col: String, pub expr: Option<String> }
#[derive(Debug, Clone, PartialEq)]
pub struct JoinPair { pub left: String, pub left_col: String, pub right: String, pub right_col: String }

#[derive(Default, Debug)]
pub struct Analysis {
    pub writes: BTreeSet<String>,
    pub reads: BTreeSet<String>,
    pub edges: BTreeSet<(String, String)>,
    pub column_edges: Vec<ColumnEdge>,
    pub joins: Vec<JoinPair>,
    pub errors: Vec<String>,
}

/// Splits SQL text into statements, dropping comments; quotes and $$ bodies are kept intact.
pub fn split_statements(sql: &str) -> Vec<String> {
    let chars: Vec<char> = sql.chars().collect();
    let (mut out, mut current, mut i) = (Vec::new(), String::new(), 0);
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c == '-' && next == Some('-') { while i < chars.len() && chars[i] != '\n' { i += 1; } current.push(' '); continue; }
        if c == '/' && next == Some('*') { i += 2; while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') { i += 1; } i += 2; current.push(' '); continue; }
        if c == '\'' || c == '"' {
            current.push(c); i += 1;
            while i < chars.len() { current.push(chars[i]); if chars[i] == c { if chars.get(i + 1) == Some(&c) { current.push(c); i += 2; continue; } break; } i += 1; }
            i += 1; continue;
        }
        if c == '$' && next == Some('$') {
            current.push_str("$$"); i += 2;
            while i + 1 < chars.len() && !(chars[i] == '$' && chars[i + 1] == '$') { current.push(chars[i]); i += 1; }
            current.push_str("$$"); i += 2; continue;
        }
        if c == ';' { if !current.trim().is_empty() { out.push(current.trim().to_owned()); } current.clear(); i += 1; continue; }
        current.push(c); i += 1;
    }
    if !current.trim().is_empty() { out.push(current.trim().to_owned()); }
    out
}

const NAME: &str = r#"([#\w$]+(?:\.[#\w$]+){0,2}|"[^"]+"(?:\."[^"]+"){0,2})"#;
enum Kind { Insert, CreateTable, CreateView, Modify, Refresh, Append, SelectInto }
static TARGETS: LazyLock<Vec<(Regex, Kind)>> = LazyLock::new(|| {
    let r = |p: &str| Regex::new(&p.replace("NAME", NAME)).unwrap();
    vec![
        (r(r"(?is)^insert\s+into\s+NAME"), Kind::Insert),
        (r(r"(?is)^create\s+(?:or\s+replace\s+)?(?:(?:local\s+)?(?:temp|temporary)\s+)?table\s+(?:if\s+not\s+exists\s+)?NAME"), Kind::CreateTable),
        (r(r"(?is)^create\s+(?:or\s+replace\s+)?(?:materialized\s+)?view\s+(?:if\s+not\s+exists\s+)?NAME"), Kind::CreateView),
        (r(r"(?is)^(?:update|delete\s+from|merge\s+into|truncate(?:\s+table)?)\s+NAME"), Kind::Modify),
        (r(r"(?is)^refresh\s+materialized\s+view\s+NAME"), Kind::Refresh),
        (r(r"(?is)^alter\s+table\s+NAME\s+append\s+from\s+NAME"), Kind::Append),
        (r(r"(?is)^(?:with\b.*?)?select\b.*?\binto\s+(?:(?:temp|temporary)\s+)?(?:table\s+)?NAME\s+from\b"), Kind::SelectInto),
    ]
});
static TEMP: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)^create\s+(?:or\s+replace\s+)?(?:local\s+)?(?:temp|temporary)\s|^(?:with\b.*?)?select\b.*?\binto\s+(?:temp|temporary)\s").unwrap());
static QUERY_START: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)\b(with|select)\b").unwrap());
static INSERT_COLUMNS: LazyLock<Regex> = LazyLock::new(|| Regex::new(&r"(?is)^insert\s+into\s+NAME\s*\(([^)]*)\)".replace("NAME", NAME)).unwrap());
static CTE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r#"(?is)(?:\bwith(?:\s+recursive)?|,)\s*("?[\w$]+"?)\s*(?:\([^()]*\))?\s+as\s*\("#).unwrap());
static FROM_JOIN: LazyLock<Regex> = LazyLock::new(|| Regex::new(&r"(?i)\b(?:from|join|using)\s+NAME".replace("NAME", NAME)).unwrap());
static NO_BINDING: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?is)\s*with\s+no\s+schema\s+binding\s*$").unwrap());

fn parts(name: &str) -> Vec<String> {
    name.split('.').map(|p| p.trim_matches('"').to_lowercase()).collect()
}

const SYSTEM_SCHEMAS: [&str; 3] = ["pg_catalog", "information_schema", "pg_internal"];
const SYSTEM_PREFIXES: [&str; 6] = ["svv_", "stl_", "stv_", "svl_", "sys_", "pg_"];

impl Ctx<'_> {
    /// Object key for a (possibly qualified) name; None for system tables.
    pub fn resolve(&self, p: &[String]) -> Option<String> {
        let key = match p {
            // Unknown database (e.g. a MyBI dataset whose connection was removed): find the object in the catalog.
            [t] | [_, t] if self.database.is_empty() => {
                let suffix = format!(".{}.{t}", if p.len() == 2 { &p[0] } else { self.schema });
                self.columns.keys().find(|k| k.ends_with(&suffix)).cloned().unwrap_or_else(|| format!("unknown{suffix}"))
            }
            [t] => format!("{}.{}.{t}", self.database, self.schema), [s, t] => format!("{}.{s}.{t}", self.database), [d, s, t] => format!("{d}.{s}.{t}"), _ => return None };
        let k: Vec<&str> = key.split('.').collect();
        if SYSTEM_SCHEMAS.contains(&k[1]) || (p.len() == 1 && SYSTEM_PREFIXES.iter().any(|x| k[2].starts_with(x))) { return None; }
        Some(key)
    }
    fn dialect(&self) -> Box<dyn Dialect> { if self.redshift { Box::new(RedshiftSqlDialect {}) } else { Box::new(PostgreSqlDialect {}) } }
    fn known(&self, key: &str) -> bool { self.columns.contains_key(key) }
}

type Bases = Vec<(String, String)>;
#[derive(Clone)]
struct Output { name: String, bases: Bases, expr: Option<String> }
enum Src<'q> { Table(String), Query(&'q Query) }

/// Column resolver over a parsed query: maps output columns to base (table, column) pairs.
struct Resolver<'a, 'q> {
    ctx: &'a Ctx<'a>,
    memo: RefCell<HashMap<usize, Rc<Vec<Output>>>>,
    joins: RefCell<Vec<JoinPair>>,
    _q: std::marker::PhantomData<&'q ()>,
}

fn ident(text: &str) -> String { text.trim_matches('"').to_lowercase() }

impl<'a, 'q> Resolver<'a, 'q> {
    fn query(&self, q: &'q Query, ctes: &HashMap<String, &'q Query>, depth: usize) -> Rc<Vec<Output>> {
        let id = q as *const Query as usize;
        if let Some(found) = self.memo.borrow().get(&id) { return found.clone(); }
        if depth > 16 { return Rc::new(Vec::new()); }
        self.memo.borrow_mut().insert(id, Rc::new(Vec::new())); // guards recursive CTEs
        let mut scope = ctes.clone();
        if let Some(with) = &q.with {
            for cte in &with.cte_tables { scope.insert(ident(&cte.alias.name.value), &cte.query); }
            for cte in &with.cte_tables { self.query(&cte.query, &scope, depth + 1); }
        }
        let outputs = Rc::new(self.set(&q.body, &scope, depth));
        self.memo.borrow_mut().insert(id, outputs.clone());
        outputs
    }

    fn set(&self, body: &'q SetExpr, ctes: &HashMap<String, &'q Query>, depth: usize) -> Vec<Output> {
        match body {
            SetExpr::Select(select) => self.select(select, ctes, depth),
            SetExpr::Query(q) => (*self.query(q, ctes, depth + 1)).clone(),
            SetExpr::SetOperation { left, right, .. } => {
                let mut l = self.set(left, ctes, depth);
                for (o, r) in l.iter_mut().zip(self.set(right, ctes, depth)) { o.bases.extend(r.bases); }
                l
            }
            _ => Vec::new(),
        }
    }

    fn sources(&self, select: &'q sqlparser::ast::Select, ctes: &HashMap<String, &'q Query>, depth: usize) -> Vec<(String, Src<'q>)> {
        let mut out = Vec::new();
        let mut add = |factor: &'q TableFactor| match factor {
            TableFactor::Table { name, alias, .. } => {
                let p = parts(&name.to_string());
                let alias = alias.as_ref().map(|a| ident(&a.name.value)).unwrap_or_else(|| p.last().cloned().unwrap_or_default());
                if p.len() == 1 && let Some(q) = ctes.get(&p[0]) { out.push((alias, Src::Query(*q))); }
                else if let Some(key) = self.ctx.resolve(&p) { out.push((alias, Src::Table(key))); }
            }
            TableFactor::Derived { subquery, alias, .. } => {
                self.query(subquery, ctes, depth + 1);
                out.push((alias.as_ref().map(|a| ident(&a.name.value)).unwrap_or_default(), Src::Query(subquery)));
            }
            _ => {}
        };
        for twj in &select.from { add(&twj.relation); for join in &twj.joins { add(&join.relation); } }
        out
    }

    fn has(&self, src: &Src<'q>, column: &str, ctes: &HashMap<String, &'q Query>, depth: usize) -> bool {
        match src {
            Src::Table(k) => self.ctx.columns.get(k).is_some_and(|c| c.iter().any(|x| x == column)),
            Src::Query(q) => self.query(q, ctes, depth + 1).iter().any(|o| o.name == column),
        }
    }

    fn bases(&self, src: &Src<'q>, column: &str, ctes: &HashMap<String, &'q Query>, depth: usize) -> Bases {
        match src {
            Src::Table(k) => vec![(k.clone(), column.to_owned())],
            Src::Query(q) => self.query(q, ctes, depth + 1).iter().find(|o| o.name == column).map(|o| o.bases.clone()).unwrap_or_default(),
        }
    }

    fn reference(&self, qualifier: Option<&str>, column: &str, srcs: &[(String, Src<'q>)], ctes: &HashMap<String, &'q Query>, depth: usize) -> Bases {
        let candidates: Vec<&Src> = match qualifier {
            Some(q) => srcs.iter().filter(|(a, _)| a == q).map(|(_, s)| s).collect(),
            None if srcs.len() == 1 => vec![&srcs[0].1],
            None => srcs.iter().filter(|(_, s)| self.has(s, column, ctes, depth)).map(|(_, s)| s).collect(),
        };
        candidates.first().map(|s| self.bases(s, column, ctes, depth)).unwrap_or_default()
    }

    fn refs(expr: &Expr) -> Vec<(Option<String>, String)> {
        let mut out = Vec::new();
        let _ = visit_expressions(expr, |e| {
            match e {
                Expr::Identifier(i) => out.push((None, ident(&i.value))),
                Expr::CompoundIdentifier(p) if p.len() >= 2 => out.push((Some(ident(&p[p.len() - 2].value)), ident(&p[p.len() - 1].value))),
                _ => {}
            }
            ControlFlow::<()>::Continue(())
        });
        out
    }

    fn select(&self, select: &'q sqlparser::ast::Select, ctes: &HashMap<String, &'q Query>, depth: usize) -> Vec<Output> {
        let srcs = self.sources(select, ctes, depth);
        let mut outputs = Vec::new();
        let resolve_expr = |expr: &Expr| -> Bases {
            let mut bases: Bases = Self::refs(expr).into_iter().flat_map(|(q, c)| self.reference(q.as_deref(), &c, &srcs, ctes, depth)).collect();
            bases.sort(); bases.dedup(); bases
        };
        for item in &select.projection {
            match item {
                SelectItem::UnnamedExpr(expr) => {
                    let name = match expr { Expr::Identifier(i) => ident(&i.value), Expr::CompoundIdentifier(p) => ident(&p[p.len() - 1].value), _ => continue };
                    outputs.push(Output { name, bases: resolve_expr(expr), expr: None });
                }
                SelectItem::ExprWithAlias { expr, alias } => {
                    let plain = matches!(expr, Expr::Identifier(_) | Expr::CompoundIdentifier(_));
                    let text: String = expr.to_string().chars().take(500).collect();
                    outputs.push(Output { name: ident(&alias.value), bases: resolve_expr(expr), expr: (!plain).then_some(text) });
                }
                other => {
                    let text = other.to_string();
                    let qualifier = text.strip_suffix(".*").map(|q| ident(q.rsplit('.').next().unwrap_or(q)));
                    for (alias, src) in &srcs {
                        if qualifier.as_ref().is_some_and(|q| q != alias) { continue; }
                        match src {
                            Src::Table(k) => for c in self.ctx.columns.get(k).into_iter().flatten() { outputs.push(Output { name: c.clone(), bases: vec![(k.clone(), c.clone())], expr: None }); },
                            Src::Query(q) => outputs.extend(self.query(q, ctes, depth + 1).iter().cloned()),
                        }
                    }
                }
            }
        }
        // Equality predicates between two base tables → relation suggestions (F4.3).
        let check = |e: &Expr| {
            if let Expr::BinaryOp { left, op: BinaryOperator::Eq, right } = e
                && let (Expr::CompoundIdentifier(l), Expr::CompoundIdentifier(r)) = (left.as_ref(), right.as_ref())
                && l.len() >= 2 && r.len() >= 2 {
                let side = |p: &[sqlparser::ast::Ident]| self.reference(Some(&ident(&p[p.len() - 2].value)), &ident(&p[p.len() - 1].value), &srcs, ctes, depth);
                if let (Some(a), Some(b)) = (side(l).pop(), side(r).pop()) && a.0 != b.0 {
                    self.joins.borrow_mut().push(JoinPair { left: a.0, left_col: a.1, right: b.0, right_col: b.1 });
                }
            }
        };
        let _ = visit_expressions(&select.from, |e| { check(e); ControlFlow::<()>::Continue(()) });
        if let Some(selection) = &select.selection { let _ = visit_expressions(selection, |e| { check(e); ControlFlow::<()>::Continue(()) }); }
        outputs
    }
}

impl Ctx<'_> {
    fn parse_query(&self, sql: &str) -> Result<Query, String> {
        let dialect = self.dialect();
        let mut parser = Parser::new(dialect.as_ref()).try_with_sql(sql).map_err(|e| e.to_string())?;
        let query = parser.parse_query().map_err(|e| e.to_string())?;
        Ok(*query)
    }

    /// Analyses SQL text. `default_target` is used when the text is a bare query (view body, MyBI dataset).
    pub fn analyze(&self, text: &str, default_target: Option<&str>) -> Analysis {
        let mut a = Analysis::default();
        let mut temps: HashMap<String, BTreeSet<String>> = HashMap::new();
        for statement in split_statements(text) {
            let statement = NO_BINDING.replace(&statement, "").to_string();
            let mut target: Option<(String, bool)> = None;
            let mut extra_reads = Vec::new();
            let mut query_from = None;
            let mut is_modify = false;
            for (regex, kind) in TARGETS.iter() {
                let Some(m) = regex.captures(&statement) else { continue };
                let name = m.get(1).unwrap().as_str();
                let temp = name.starts_with('#') || TEMP.is_match(&statement);
                target = Some((name.to_owned(), temp));
                match kind {
                    Kind::Append => extra_reads.push(m.get(2).unwrap().as_str().to_owned()),
                    Kind::Modify => is_modify = true,
                    Kind::Refresh => {}
                    Kind::SelectInto => query_from = Some(0),
                    _ => query_from = QUERY_START.find_at(&statement, m.get(0).unwrap().end()).map(|q| q.start()),
                }
                break;
            }
            if target.is_none() {
                match default_target {
                    Some(t) if QUERY_START.find(statement.trim_start_matches(|c: char| c == '(' || c.is_whitespace())).is_some_and(|m| m.start() == 0) => { target = Some((t.to_owned(), false)); query_from = Some(0); }
                    _ => continue, // SET, GRANT, plain SELECT in a script, ...
                }
            }
            let (target_name, temp) = target.unwrap();
            let target_key = if default_target == Some(target_name.as_str()) { Some(target_name.clone()) } else { self.resolve(&parts(&target_name)) };
            let Some(target_key) = target_key else { continue };
            let ctes: HashSet<String> = CTE.captures_iter(&statement).map(|c| ident(&c[1])).collect();
            let mut reads: BTreeSet<String> = BTreeSet::new();
            let mut parsed = None;
            if let Some(start) = query_from {
                let mut body = statement[start..].trim().to_owned();
                // "AS ( SELECT ... )" → drop the wrapping parenthesis
                if statement[..start].trim_end().ends_with('(') && body.ends_with(')') { body.pop(); }
                match self.parse_query(&body) {
                    Ok(q) => {
                        let _ = visit_relations(&q, |r| {
                            let p = parts(&r.to_string());
                            if !(p.len() == 1 && ctes.contains(&p[0])) { if let Some(k) = self.resolve(&p) { reads.insert(k); } }
                            ControlFlow::<()>::Continue(())
                        });
                        parsed = Some(q);
                    }
                    Err(e) => a.errors.push(format!("{}…: {e}", statement.chars().take(60).collect::<String>())),
                }
            }
            if parsed.is_none() && (query_from.is_some() || is_modify) {
                // Fallback: FROM / JOIN / USING names that exist in the catalog (avoids EXTRACT(x FROM col)).
                for m in FROM_JOIN.captures_iter(&statement) {
                    let p = parts(&m[1]);
                    if p.len() == 1 && ctes.contains(&p[0]) { continue; }
                    if let Some(k) = self.resolve(&p) && (self.known(&k) || temps.contains_key(&k)) { reads.insert(k); }
                }
            }
            for name in extra_reads { if let Some(k) = self.resolve(&parts(&name)) { reads.insert(k); } }
            reads.remove(&target_key);
            // Temp tables are intermediate: replace them by their own upstreams.
            let reads: BTreeSet<String> = reads.into_iter().flat_map(|r| temps.get(&r).cloned().unwrap_or_else(|| BTreeSet::from([r]))).collect();
            if temp {
                temps.entry(target_key).or_default().extend(reads);
                continue;
            }
            for r in &reads { a.edges.insert((r.clone(), target_key.clone())); }
            a.reads.extend(reads);
            a.writes.insert(target_key.clone());
            if let Some(query) = &parsed {
                let resolver = Resolver { ctx: self, memo: Default::default(), joins: Default::default(), _q: Default::default() };
                let outputs = resolver.query(query, &HashMap::new(), 0);
                let names: Vec<String> = INSERT_COLUMNS.captures(&statement).map(|c| c[2].split(',').map(ident).map(|s| s.trim().to_owned()).collect()).unwrap_or_default();
                for (i, output) in outputs.iter().enumerate() {
                    let down_col = names.get(i).cloned().unwrap_or_else(|| output.name.clone());
                    for (up, up_col) in &output.bases {
                        if temps.contains_key(up) { continue; }
                        a.column_edges.push(ColumnEdge { up: up.clone(), up_col: up_col.clone(), down: target_key.clone(), down_col: down_col.clone(), expr: output.expr.clone() });
                    }
                }
                a.joins.extend(resolver.joins.into_inner());
            }
        }
        a
    }
}

fn column_map(db: &Connection) -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    let mut stmt = db.prepare("SELECT object_key, name FROM column_tech WHERE is_deleted = 0 ORDER BY object_key, ordinal").unwrap();
    for (k, c) in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))).unwrap().flatten() { map.entry(k).or_default().push(c); }
    // Objects without columns (rare) still count as known.
    let mut stmt = db.prepare("SELECT key FROM object_tech WHERE is_deleted = 0").unwrap();
    for k in stmt.query_map([], |r| r.get::<_, String>(0)).unwrap().flatten() { map.entry(k).or_default(); }
    map
}

/// Writes an analysis as lineage rows for one origin (replacing that origin's previous rows).
pub fn store_analysis(db: &Connection, source: &str, origin: &str, a: &Analysis) -> rusqlite::Result<()> {
    db.execute("DELETE FROM lineage WHERE source = ?1 AND origin = ?2", (source, origin))?;
    let mut insert = db.prepare(&format!("INSERT INTO lineage(source, origin, upstream, downstream, upstream_column, downstream_column, expression, parsed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, {NOW})"))?;
    for (up, down) in &a.edges { insert.execute((source, origin, up, down, None::<String>, None::<String>, None::<String>))?; }
    for e in &a.column_edges { insert.execute((source, origin, &e.up, &e.down, &e.up_col, &e.down_col, &e.expr))?; }
    Ok(())
}

fn upstream_set(db: &Connection, source: &str, origin: &str) -> BTreeSet<String> {
    let mut stmt = db.prepare("SELECT DISTINCT upstream FROM lineage WHERE source = ?1 AND origin = ?2 AND upstream_column IS NULL").unwrap();
    stmt.query_map((source, origin), |r| r.get(0)).unwrap().flatten().collect()
}

/// Rebuilds view lineage for all scopes, script lineage and MyBI lineage, then refreshes JOIN-based relation suggestions.
pub fn rebuild_all(db: &Connection) -> Value {
    let columns = column_map(db);
    let mut joins = Vec::new();
    let mut errors = 0;
    let profiles = myquery_core::config::load().map(|c| c.profiles).unwrap_or_default();
    let redshift_db: HashMap<String, bool> = profiles.iter().map(|p| (p.database.clone(), p.redshift)).collect();
    // Views / MVs
    let views: Vec<(String, String, String)> = {
        let mut stmt = db.prepare("SELECT key, scope, definition FROM object_tech WHERE is_deleted = 0 AND definition IS NOT NULL").unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().flatten().collect()
    };
    let current: HashSet<&String> = views.iter().map(|v| &v.0).collect();
    let stale: Vec<String> = { let mut stmt = db.prepare("SELECT DISTINCT origin FROM lineage WHERE source = 'view'").unwrap(); stmt.query_map([], |r| r.get(0)).unwrap().flatten().collect() };
    for origin in stale.iter().filter(|o| !current.contains(o)) { db.execute("DELETE FROM lineage WHERE source = 'view' AND origin = ?", [origin]).ok(); }
    for (key, scope, definition) in &views {
        let (database, schema) = scope.split_once('.').unwrap_or((scope, "public"));
        let ctx = Ctx { database, schema, redshift: *redshift_db.get(database).unwrap_or(&true), columns: &columns };
        let analysis = ctx.analyze(definition, Some(key));
        errors += analysis.errors.len();
        let before = upstream_set(db, "view", key);
        store_analysis(db, "view", key, &analysis).ok();
        let after = upstream_set(db, "view", key);
        if !before.is_empty() && before != after {
            let join = |s: &BTreeSet<String>| s.iter().cloned().collect::<Vec<_>>().join(", ");
            db.execute(&format!("INSERT INTO change_log(entity, key, field, before, after, source, actor, at) VALUES ('object', ?1, 'lineage', ?2, ?3, 'sync', 'sync', {NOW})"), (key, join(&before), join(&after))).ok();
        }
        joins.extend(analysis.joins);
    }
    // Scripts
    let folders: Vec<Value> = store::setting(db, "script_folders").and_then(|v| v.as_array().cloned()).unwrap_or_default();
    let mut seen = HashSet::new();
    for folder in &folders {
        let (Some(path), Some(database), Some(schema)) = (folder["path"].as_str(), folder["database"].as_str(), folder["schema"].as_str()) else { continue };
        let ctx = Ctx { database, schema, redshift: *redshift_db.get(database).unwrap_or(&true), columns: &columns };
        let mut files = Vec::new();
        collect_sql(Path::new(path), &mut files, 0);
        for file in files {
            let key = file.to_string_lossy().into_owned();
            seen.insert(key.clone());
            let modified = std::fs::metadata(&file).ok().and_then(|m| m.modified().ok()).map(|t| t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)).unwrap_or(0);
            let text = match std::fs::read(&file) { Ok(bytes) if bytes.len() <= 4 << 20 => String::from_utf8_lossy(&bytes).into_owned(), Ok(_) => { record_script(db, &key, path, modified, &Analysis::default(), Some("文件超过 4 MB，已跳过")); continue; } Err(e) => { record_script(db, &key, path, modified, &Analysis::default(), Some(&e.to_string())); continue; } };
            let analysis = ctx.analyze(&text, None);
            errors += analysis.errors.len();
            store_analysis(db, "script", &key, &analysis).ok();
            record_script(db, &key, path, modified, &analysis, analysis.errors.first().map(String::as_str));
            joins.extend(analysis.joins);
        }
    }
    let known: Vec<String> = { let mut stmt = db.prepare("SELECT path FROM script").unwrap(); stmt.query_map([], |r| r.get(0)).unwrap().flatten().collect() };
    for path in known.iter().filter(|p| !seen.contains(*p)) {
        db.execute("DELETE FROM script WHERE path = ?", [path]).ok();
        db.execute("DELETE FROM lineage WHERE source = 'script' AND origin = ?", [path]).ok();
    }
    let mybi = crate::mybi::rebuild(db, &columns, &redshift_db, &mut joins);
    let suggestions = suggest_relations(db, &joins);
    json!({ "views": views.len(), "scripts": seen.len(), "parse_errors": errors, "mybi": mybi, "suggestions": suggestions })
}

fn collect_sql(dir: &Path, out: &mut Vec<std::path::PathBuf>, depth: usize) {
    if depth > 8 { return; }
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() { if !entry.file_name().to_string_lossy().starts_with('.') { collect_sql(&path, out, depth + 1); } }
        else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("sql")) { out.push(path); }
    }
}

fn record_script(db: &Connection, path: &str, folder: &str, modified: u64, a: &Analysis, error: Option<&str>) {
    db.execute(&format!("INSERT INTO script(path, folder, modified, writes, reads, error, parsed_at) VALUES (?1, ?2, strftime('%Y-%m-%dT%H:%M:%SZ', ?3, 'unixepoch'), ?4, ?5, ?6, {NOW}) ON CONFLICT(path) DO UPDATE SET folder = excluded.folder, modified = excluded.modified, writes = excluded.writes, reads = excluded.reads, error = excluded.error, parsed_at = excluded.parsed_at"),
        (path, folder, modified as i64, json!(a.writes).to_string(), json!(a.reads).to_string(), error)).ok();
}

/// JOIN equality pairs become "suggested" relations unless the pair already exists in any status (confirmed or rejected).
fn suggest_relations(db: &Connection, joins: &[JoinPair]) -> usize {
    let mut grouped: HashMap<(String, String), BTreeSet<(String, String)>> = HashMap::new();
    for j in joins {
        let (l, r) = if (&j.left, &j.left_col) <= (&j.right, &j.right_col) { ((&j.left, &j.left_col), (&j.right, &j.right_col)) } else { ((&j.right, &j.right_col), (&j.left, &j.left_col)) };
        grouped.entry((l.0.clone(), r.0.clone())).or_default().insert((format!("{}.{}", l.0, l.1), format!("{}.{}", r.0, r.1)));
    }
    let mut added = 0;
    for ((left, right), pairs) in grouped {
        let exists: bool = db.query_row("SELECT EXISTS(SELECT 1 FROM relation WHERE (left_key = ?1 AND right_key = ?2) OR (left_key = ?2 AND right_key = ?1))", (&left, &right), |r| r.get(0)).unwrap_or(true);
        if exists { continue; }
        let pairs: Vec<[String; 2]> = pairs.into_iter().map(|(a, b)| [a, b]).collect();
        added += db.execute(&format!("INSERT OR IGNORE INTO relation(left_key, right_key, key_pairs, status, origin, updated_at) VALUES (?1, ?2, ?3, 'suggested', 'join', {NOW})"), (&left, &right, json!(pairs).to_string())).unwrap_or(0);
    }
    added
}

// ---------- API ----------

type St = State<Arc<AppState>>;

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct GraphQuery { key: String, column: String, up: Option<usize>, down: Option<usize>, tmp: bool }

/// Table-level edges (any source), with the list of sources per edge.
fn table_edges(db: &Connection) -> Vec<(String, String, String)> {
    let mut stmt = db.prepare("SELECT upstream, downstream, group_concat(DISTINCT source) FROM lineage WHERE upstream_column IS NULL GROUP BY upstream, downstream").unwrap();
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().flatten().collect()
}

pub fn node_info(db: &Connection, keys: &[(String, i64)]) -> Vec<Value> {
    let layers = biz::layers(db);
    keys.iter().map(|(key, depth)| {
        let object: Option<(String, String, String, i64, Option<String>)> = db.query_row("SELECT o.name, o.kind, o.scope, o.is_deleted, b.cn_name FROM object_tech o LEFT JOIN object_biz b ON b.key = o.key WHERE o.key = ?", [key], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))).ok();
        match object {
            Some((name, kind, scope, deleted, cn_name)) => json!({ "key": key, "name": name, "kind": kind, "scope": scope, "deleted": deleted == 1, "cn_name": cn_name, "layer": layers.get(key), "depth": depth }),
            None => {
                let node: Option<(String, String)> = db.query_row("SELECT name, kind FROM node WHERE key = ?", [key], |r| Ok((r.get(0)?, r.get(1)?))).ok();
                match node {
                    Some((name, kind)) => json!({ "key": key, "name": name, "kind": kind, "depth": depth, "external": true }),
                    None => json!({ "key": key, "name": key.rsplit('.').next(), "kind": "unknown", "scope": key.rsplitn(2, '.').nth(1), "depth": depth, "external": true }),
                }
            }
        }
    }).collect()
}

const MAX_NODES: usize = 200;

pub async fn graph(State(s): St, Q(q): Q<GraphQuery>) -> ApiResult {
    let db = s.db.lock().unwrap();
    let edges = table_edges(&db);
    let hidden = |k: &str| !q.tmp && k.contains("_cd_tmp_") && k != q.key;
    let mut depth: HashMap<String, i64> = HashMap::from([(q.key.clone(), 0)]);
    let mut kept = Vec::new();
    let mut truncated = false;
    for (down, limit) in [(false, q.up.unwrap_or(3)), (true, q.down.unwrap_or(3))] {
        let mut queue = VecDeque::from([(q.key.clone(), 0usize)]);
        let mut visited = HashSet::from([q.key.clone()]);
        while let Some((key, d)) = queue.pop_front() {
            if d >= limit { continue; }
            for (up, dn, sources) in &edges {
                let (from, next) = if down { (up, dn) } else { (dn, up) };
                if from != &key || hidden(next) { continue; }
                kept.push(json!({ "from": up, "to": dn, "sources": sources }));
                if visited.insert(next.clone()) {
                    if depth.len() >= MAX_NODES { truncated = true; continue; }
                    depth.entry(next.clone()).or_insert(if down { d as i64 + 1 } else { -(d as i64) - 1 });
                    queue.push_back((next.clone(), d + 1));
                }
            }
        }
    }
    kept.retain(|e| depth.contains_key(e["from"].as_str().unwrap()) && depth.contains_key(e["to"].as_str().unwrap()));
    kept.sort_by_key(|e| e.to_string()); kept.dedup();
    let nodes = node_info(&db, &depth.into_iter().collect::<Vec<_>>());
    let column_edges = if q.column.is_empty() { json!([]) } else {
        json!(rows(&db, "SELECT DISTINCT upstream, upstream_column, downstream, downstream_column, expression FROM lineage WHERE (upstream = ?1 AND upstream_column = ?2) OR (downstream = ?1 AND downstream_column = ?2)", &[&q.key, &q.column])?)
    };
    Ok(Json(json!({ "nodes": nodes, "edges": kept, "truncated": truncated, "column_edges": column_edges })))
}

/// Impact analysis (F6.5): every downstream object (all depths); with `column`, also downstream columns.
pub async fn impact(State(s): St, Q(q): Q<GraphQuery>) -> ApiResult {
    let db = s.db.lock().unwrap();
    let edges = table_edges(&db);
    let mut depth: Vec<(String, i64)> = Vec::new();
    let mut via: HashMap<String, String> = HashMap::new();
    let mut seen = HashSet::from([q.key.clone()]);
    let mut queue = VecDeque::from([(q.key.clone(), 0i64)]);
    while let Some((key, d)) = queue.pop_front() {
        for (up, down, sources) in &edges {
            if up == &key && seen.insert(down.clone()) { depth.push((down.clone(), d + 1)); via.insert(down.clone(), format!("{key}|{sources}")); queue.push_back((down.clone(), d + 1)); }
        }
    }
    let mut objects = node_info(&db, &depth);
    for o in &mut objects { if let Some(v) = via.get(o["key"].as_str().unwrap()) { let (from, sources) = v.split_once('|').unwrap(); o["via"] = json!(from); o["sources"] = json!(sources); } }
    let mut columns = Vec::new();
    if !q.column.is_empty() {
        let mut stmt = db.prepare("SELECT DISTINCT upstream, upstream_column, downstream, downstream_column, expression FROM lineage WHERE upstream_column IS NOT NULL").map_err(fail)?;
        let col_edges: Vec<(String, String, String, String, Option<String>)> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))).map_err(fail)?.flatten().collect();
        let start = (q.key.clone(), q.column.clone());
        let mut seen = HashSet::from([start.clone()]);
        let mut queue = VecDeque::from([(start, 0)]);
        while let Some(((k, c), d)) = queue.pop_front() {
            for (up, up_col, down, down_col, expr) in &col_edges {
                if up == &k && up_col == &c && seen.insert((down.clone(), down_col.clone())) {
                    columns.push(json!({ "key": down, "column": down_col, "depth": d + 1, "from": format!("{up}.{up_col}"), "expression": expr }));
                    queue.push_back(((down.clone(), down_col.clone()), d + 1));
                }
            }
        }
    }
    Ok(Json(json!({ "objects": objects, "columns": columns })))
}

const RANK: [(&str, i32); 4] = [("dtl", 1), ("fct", 2), ("tpc", 3), ("app", 4)];

/// Layer dependency check (F6.4): reverse references (e.g. fct reading tpc) and app reading app.
pub async fn violations(State(s): St) -> ApiResult {
    let db = s.db.lock().unwrap();
    let layers = biz::layers(&db);
    let rank = |k: &str| layers.get(k).and_then(|l| RANK.iter().find(|(n, _)| n == l)).map(|(_, r)| *r);
    let mut out = Vec::new();
    for (up, down, sources) in table_edges(&db) {
        let (Some(a), Some(b)) = (rank(&up), rank(&down)) else { continue };
        let kind = if a == 4 && b == 4 { "app_to_app" } else if a > b { "reverse" } else { continue };
        out.push(json!({ "upstream": up, "downstream": down, "upstream_layer": layers.get(&up), "downstream_layer": layers.get(&down), "kind": kind, "sources": sources }));
    }
    Ok(Json(json!(out)))
}

pub async fn rebuild(State(s): St) -> ApiResult {
    let db = s.db.lock().unwrap();
    let result = rebuild_all(&db);
    manual::log(&db, "system", "lineage", "rebuild", None, Some(&result.to_string())).map_err(fail)?;
    Ok(Json(result))
}

pub async fn scripts(State(s): St) -> ApiResult {
    let db = s.db.lock().unwrap();
    Ok(Json(json!(rows(&db, "SELECT * FROM script ORDER BY path", &[])?)))
}

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct RefreshQuery { delayed: bool, q: String, page: i64 }

/// Refresh overview by object (F10.3): refresh source, schedule, last refresh, freshness.
pub async fn refresh_objects(State(s): St, Q(q): Q<RefreshQuery>) -> ApiResult {
    let db = s.db.lock().unwrap();
    let like = format!("%{}%", q.q.trim());
    let from = format!("FROM object_tech o LEFT JOIN freshness f ON f.key = o.key WHERE o.is_deleted = 0 AND (o.kind = 'mv' OR f.key IS NOT NULL OR EXISTS (SELECT 1 FROM lineage l WHERE l.downstream = o.key AND l.source IN ('script', 'mybi_task'))) AND o.name LIKE ?1{}", if q.delayed { " AND f.max_date < date('now', '-1 day')" } else { "" });
    let total: i64 = db.query_row(&format!("SELECT COUNT(*) {from}"), [&like], |r| r.get(0)).map_err(fail)?;
    let mut items = rows(&db, &format!("SELECT o.key, o.name, o.kind, o.scope, json_extract(o.props, '$.last_refresh') AS mv_last_refresh, json_extract(o.props, '$.auto_refresh') AS auto_refresh, f.date_column, f.max_date, f.error AS freshness_error, f.max_date < date('now', '-1 day') AS delayed {from} ORDER BY delayed DESC, o.name LIMIT 10 OFFSET {}", q.page.max(0) * 10), &[&like])?;
    for item in &mut items {
        let key = item["key"].as_str().unwrap_or_default().to_owned();
        item["sources"] = json!(rows(&db, "SELECT DISTINCT l.source, l.origin, n.name, n.info FROM lineage l LEFT JOIN node n ON n.key = l.origin WHERE l.downstream = ? AND l.upstream_column IS NULL AND l.source IN ('script', 'mybi_task')", &[&key])?);
    }
    Ok(Json(json!({ "total": total, "rows": items, "expected": db.query_row("SELECT date('now', '-1 day')", [], |r| r.get::<_, String>(0)).map_err(fail)? })))
}

pub async fn mybi_tasks(State(s): St) -> ApiResult {
    let db = s.db.lock().unwrap();
    Ok(Json(json!(rows(&db, "SELECT n.*, (SELECT COUNT(DISTINCT upstream) FROM lineage WHERE downstream = n.key) AS reads, (SELECT group_concat(DISTINCT downstream) FROM lineage WHERE origin = n.key AND source = 'mybi_task') AS writes FROM node n WHERE n.kind IN ('mybi_dataset', 'mybi_task') ORDER BY n.kind DESC, n.name", &[])?)))
}


#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(columns: &HashMap<String, Vec<String>>) -> Ctx<'_> { Ctx { database: "db", schema: "public", redshift: true, columns } }
    fn cols(list: &[(&str, &[&str])]) -> HashMap<String, Vec<String>> { list.iter().map(|(k, c)| (format!("db.public.{k}"), c.iter().map(|s| s.to_string()).collect())).collect() }

    #[test]
    fn splits_and_strips_comments() {
        let s = split_statements("-- head; x\nSELECT 'a;b' FROM t; /* c; */ INSERT INTO x SELECT 1;");
        assert_eq!(s, vec!["SELECT 'a;b' FROM t", "INSERT INTO x SELECT 1"]);
    }

    #[test]
    fn view_body_with_ctes_and_columns() {
        let c = cols(&[("a", &["id", "amount", "dt"]), ("b", &["id", "country"])]);
        let a = ctx(&c).analyze("WITH x AS (SELECT id, SUM(amount) AS total FROM a GROUP BY id) SELECT x.id, x.total * 2 AS doubled, b.country FROM x JOIN b ON b.id = x.id", Some("db.public.v"));
        assert_eq!(a.edges, BTreeSet::from([("db.public.a".into(), "db.public.v".into()), ("db.public.b".into(), "db.public.v".into())]));
        let doubled: Vec<_> = a.column_edges.iter().filter(|e| e.down_col == "doubled").collect();
        assert_eq!(doubled.len(), 1);
        assert_eq!((doubled[0].up.as_str(), doubled[0].up_col.as_str()), ("db.public.a", "amount"));
        assert!(a.column_edges.iter().any(|e| e.down_col == "country" && e.up == "db.public.b"));
        assert_eq!(a.joins.len(), 1);
        assert_eq!((a.joins[0].left_col.as_str(), a.joins[0].right_col.as_str()), ("id", "id"));
    }

    #[test]
    fn redshift_script_with_temp_tables_and_mv() {
        let c = cols(&[("src", &["d", "v"]), ("dim_x", &["d"]), ("out", &["d", "v"])]);
        let sql = "CREATE TEMP TABLE #stage DISTKEY(d) AS SELECT d, v FROM src;\n\
                   DELETE FROM out WHERE d >= CURRENT_DATE - 1;\n\
                   INSERT INTO out (d, v) SELECT s.d, s.v FROM #stage s JOIN dim_x x ON x.d = s.d WHERE EXTRACT(year FROM s.d) > 2020;\n\
                   CREATE MATERIALIZED VIEW mv_out SORTKEY(d) AUTO REFRESH NO AS SELECT d, COUNT(*) AS n FROM out GROUP BY d;\n\
                   REFRESH MATERIALIZED VIEW mv_out;\nGRANT SELECT ON out TO grp;";
        let a = ctx(&c).analyze(sql, None);
        assert_eq!(a.writes, BTreeSet::from(["db.public.mv_out".into(), "db.public.out".into()]));
        assert!(a.edges.contains(&("db.public.src".into(), "db.public.out".into())), "temp table resolved to its source: {:?}", a.edges);
        assert!(a.edges.contains(&("db.public.dim_x".into(), "db.public.out".into())));
        assert!(a.edges.contains(&("db.public.out".into(), "db.public.mv_out".into())));
        assert!(!a.reads.iter().any(|r| r.contains("stage") || r.contains("year")));
    }

    #[test]
    fn late_binding_and_qualified_names() {
        let c = cols(&[("t", &["a"])]);
        let a = ctx(&c).analyze("SELECT a FROM public.t UNION ALL SELECT a FROM other.u WITH NO SCHEMA BINDING", Some("db.public.v"));
        assert_eq!(a.reads, BTreeSet::from(["db.other.u".into(), "db.public.t".into()]));
        assert!(ctx(&c).analyze("SELECT * FROM pg_catalog.pg_class", Some("db.public.v")).reads.is_empty());
        assert_eq!(ctx(&c).analyze("((SELECT a FROM t) UNION (SELECT a FROM t))", Some("db.public.v")).reads, BTreeSet::from(["db.public.t".into()]));
        let unknown = Ctx { database: "", schema: "public", redshift: true, columns: &c };
        assert_eq!(unknown.analyze("SELECT a FROM t JOIN x ON 1 = 1", Some("ds")).reads, BTreeSet::from(["db.public.t".into(), "unknown.public.x".into()]));
    }
}
