//! Naming rule `{产品前缀}[cd_]{层级}_{主题}[_{粒度}][_v或_mv]`: parse product / maintainer / layer / grain / type and check `_cd_` objects.
use crate::store;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;

pub const LAYERS: [&str; 6] = ["dim", "dtl", "fct", "tpc", "app", "tmp"];

#[derive(Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    pub products: BTreeMap<String, String>,
    pub maintainers: BTreeMap<String, String>,
    pub layer_synonyms: BTreeMap<String, String>,
    pub grains: Vec<String>,
    pub grain_synonyms: BTreeMap<String, String>,
}

impl Default for Config {
    fn default() -> Self {
        let map = |pairs: &[(&str, &str)]| pairs.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect();
        Config {
            products: map(&[("btd6mobile", "BTD6"), ("blitzmobile", "Blitz"), ("zamobile", "ZAR")]),
            maintainers: map(&[("cd", "Carl Dong")]),
            layer_synonyms: map(&[("detail", "dtl"), ("topic", "tpc"), ("fact", "fct")]),
            grains: ["hourly", "daily", "weekly", "monthly", "yearly", "latest", "user", "snapshot"].map(String::from).to_vec(),
            grain_synonyms: map(&[("d", "daily"), ("w", "weekly"), ("m", "monthly")]),
        }
    }
}

pub fn config(db: &Connection) -> Config {
    store::setting(db, "naming").and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default()
}

#[derive(Default, Serialize, PartialEq, Debug)]
pub struct Parsed {
    pub product: Option<String>,
    pub maintainer: Option<String>,
    pub layer: Option<String>,
    pub subject: Option<String>,
    pub grain: Option<String>,
    pub suffix: Option<&'static str>,
    pub temporary: bool,
    pub tmp_date: Option<String>,
    /// Naming check issues (codes, translated in the UI); only `[maintainer]_` objects are checked.
    pub issues: Vec<&'static str>,
}

pub fn valid_date(text: &str) -> bool {
    if text.len() != 8 || !text.bytes().all(|b| b.is_ascii_digit()) { return false; }
    let (y, m, d): (u32, u32, u32) = (text[..4].parse().unwrap(), text[4..6].parse().unwrap(), text[6..].parse().unwrap());
    let days = match m { 1 | 3 | 5 | 7 | 8 | 10 | 12 => 31, 4 | 6 | 9 | 11 => 30, 2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29, 2 => 28, _ => return false };
    y >= 2000 && (1..=days).contains(&d)
}

pub fn parse(name: &str, kind: &str, cfg: &Config) -> Parsed {
    let tokens: Vec<&str> = name.split('_').collect();
    let mut p = Parsed { product: cfg.products.get(tokens[0]).cloned(), temporary: name.contains("_cd_tmp_"), ..Default::default() };
    let Some(maintainer) = tokens.get(1).and_then(|t| cfg.maintainers.get(*t)) else { return p };
    p.maintainer = Some(maintainer.clone());
    if name.len() > 63 { p.issues.push("name_too_long"); }
    let mut rest: Vec<&str> = tokens[2..].to_vec();
    if rest.is_empty() { p.issues.push("layer_unknown"); return p; }
    let layer = rest.remove(0);
    if LAYERS.contains(&layer) { p.layer = Some(layer.into()); }
    else if let Some(standard) = cfg.layer_synonyms.get(layer) { p.layer = Some(standard.clone()); p.issues.push("layer_synonym"); }
    else { p.issues.push("layer_unknown"); }
    p.temporary = p.layer.as_deref() == Some("tmp");
    match rest.last().copied() {
        Some("v") => { p.suffix = Some("view"); rest.pop(); }
        Some("mv") => { p.suffix = Some("mv"); rest.pop(); }
        Some("view") => { p.suffix = Some("view"); rest.pop(); p.issues.push("suffix_nonstandard"); }
        Some("mview") => { p.suffix = Some("mv"); rest.pop(); p.issues.push("suffix_nonstandard"); }
        _ => {}
    }
    if p.temporary {
        match rest.last() { Some(d) if valid_date(d) => { p.tmp_date = Some(d.to_string()); rest.pop(); } _ => p.issues.push("tmp_no_date") }
    } else if let Some(&last) = rest.last() {
        if cfg.grains.iter().any(|g| g == last) { p.grain = Some(last.into()); rest.pop(); }
        else if let Some(standard) = cfg.grain_synonyms.get(last) { p.grain = Some(standard.clone()); rest.pop(); p.issues.push("grain_synonym"); }
    }
    if rest.is_empty() { p.issues.push("subject_missing"); } else { p.subject = Some(rest.join("_")); }
    let expected = match kind { "view" => Some("view"), "mv" => Some("mv"), _ => None };
    if expected != p.suffix { p.issues.push("suffix_kind_mismatch"); }
    p
}

/// Recomputes the stored naming parse for objects (all, or one scope) after a sync or a settings change.
pub fn refresh(db: &Connection, scope: Option<&str>) -> rusqlite::Result<()> {
    let cfg = config(db);
    let mut select = db.prepare("SELECT key, name, kind FROM object_tech WHERE ?1 IS NULL OR scope = ?1")?;
    let rows: Vec<(String, String, String)> = select.query_map([scope], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
    let mut update = db.prepare("UPDATE object_tech SET naming = ?2 WHERE key = ?1")?;
    for (key, name, kind) in rows { update.execute((key, json!(parse(&name, &kind, &cfg)).to_string()))?; }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_names() {
        let c = Config::default();
        let p = parse("zamobile_cd_topic_retention_daily", "mv", &c);
        assert_eq!((p.product.as_deref(), p.layer.as_deref(), p.grain.as_deref(), p.subject.as_deref()), (Some("ZAR"), Some("tpc"), Some("daily"), Some("retention")));
        assert_eq!(p.issues, vec!["layer_synonym", "suffix_kind_mismatch"]);
        let p = parse("btd6mobile_cd_topic_paragon_unlock_daily_mv", "mv", &c);
        assert_eq!((p.grain.as_deref(), p.suffix, p.subject.as_deref()), (Some("daily"), Some("mv"), Some("paragon_unlock")));
        assert_eq!(p.issues, vec!["layer_synonym"]);
        let p = parse("btd6mobile_cd_dim_platform", "table", &c);
        assert!(p.issues.is_empty());
        let p = parse("blitzmobile_cd_topic_iap_usd_view", "view", &c);
        assert_eq!(p.issues, vec!["layer_synonym", "suffix_nonstandard"]);
        let p = parse("blitzmobile_cd_app_kpi_game_d", "table", &c);
        assert_eq!((p.layer.as_deref(), p.grain.as_deref()), (Some("app"), Some("daily")));
        let p = parse("btd6mobile_cd_tmp_retention_20261120_mv", "mv", &c);
        assert_eq!((p.temporary, p.tmp_date.as_deref(), p.subject.as_deref()), (true, Some("20261120"), Some("retention")));
        assert!(p.issues.is_empty());
        assert_eq!(parse("btd6mobile_cd_tmp_cp_params_v", "view", &c).issues, vec!["tmp_no_date"]);
        let p = parse("btd6mobile_sessions", "table", &c);
        assert_eq!((p.product.as_deref(), p.maintainer, p.issues.len()), (Some("BTD6"), None, 0));
        assert!(parse(&format!("zamobile_cd_dim_{}", "x".repeat(60)), "table", &c).issues.contains(&"name_too_long"));
    }

    #[test]
    fn dates() {
        assert!(valid_date("20240229") && !valid_date("20230229") && !valid_date("20261340") && !valid_date("2026112"));
    }
}
