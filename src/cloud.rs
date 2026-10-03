//! Optional managed-data adapter. Model execution remains entirely local.
use crate::{config::Config, scan::Performance};
use anyhow::{bail, Context, Result};
use chrono::{Duration, Utc};
use serde::Deserialize;
use std::{collections::BTreeMap, fs, io::Read, path::Path};
#[derive(Deserialize)]
pub struct Response {
    pub rows: Vec<Row>,
}
#[derive(Deserialize)]
pub struct Row {
    pub keys: Option<Vec<String>>,
    pub clicks: f64,
    pub impressions: f64,
    pub position: f64,
}
pub fn normalize(
    response: Response,
    map: &BTreeMap<String, String>,
    end: &str,
) -> Result<Vec<Performance>> {
    let mut output = vec![];
    for row in response.rows {
        let Some(url) = row.keys.as_ref().and_then(|k| k.first()) else {
            continue;
        };
        let Some(path) = map.get(url) else {
            continue;
        };
        if ![row.clicks, row.impressions, row.position]
            .iter()
            .all(|n| n.is_finite() && *n >= 0.0)
            || row.clicks > row.impressions
        {
            bail!("Cloud returned invalid metrics");
        }
        output.push(Performance {
            path: path.clone(),
            clicks: row.clicks,
            impressions: row.impressions,
            position: row.position,
            period_end: end.into(),
        });
    }
    Ok(output)
}
pub fn connect(root: &Path, property: &str, mapping: &Path) -> Result<()> {
    let mut c = Config::load(root)?;
    let map: BTreeMap<String, String> = serde_json::from_str(&fs::read_to_string(mapping)?)?;
    if map.is_empty() {
        bail!("Provide explicit page URL → repository file mappings");
    }
    for (url, file) in &map {
        let u = reqwest::Url::parse(url)?;
        if !["http", "https"].contains(&u.scheme())
            || !crate::scan::allowed(file, &c)
            || Path::new(file)
                .components()
                .any(|v| !matches!(v, std::path::Component::Normal(_)))
        {
            bail!("Invalid page mapping");
        }
    }
    c.cloud_property = Some(property.into());
    c.page_map = map;
    c.save(root)?;
    println!("Cloud data configured. Set REFRESHAGENT_API_KEY in your environment, then run cloud sync. No key is stored in the project.");
    Ok(())
}
pub fn sync(root: &Path) -> Result<()> {
    let mut c = Config::load(root)?;
    let property = c
        .cloud_property
        .as_ref()
        .context("Run cloud connect first")?;
    let key = std::env::var("REFRESHAGENT_API_KEY")
        .context("Set REFRESHAGENT_API_KEY to your RefreshAgent account API key")?;
    let end = (Utc::now() - Duration::days(3))
        .format("%Y-%m-%d")
        .to_string();
    let start = (Utc::now() - Duration::days(32))
        .format("%Y-%m-%d")
        .to_string();
    let client = reqwest::blocking::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let response = client
        .get("https://refreshagent.com/api/v1/sc/pages")
        .header("X-API-Key", key)
        .header("X-RefreshAgent-Client", "refreshagent-oss")
        .header("X-RefreshAgent-Client-Version", env!("CARGO_PKG_VERSION"))
        .query(&[
            ("site_url", property.as_str()),
            ("start_date", &start),
            ("end_date", &end),
        ])
        .send()
        .context("Cloud request failed")?;
    if !response.status().is_success() {
        bail!(
            "Cloud request returned HTTP {}; existing snapshot preserved",
            response.status()
        );
    }
    let mut bytes = vec![];
    response.take(5_000_001).read_to_end(&mut bytes)?;
    if bytes.len() > 5_000_000 {
        bail!("Cloud response exceeded limit");
    }
    let rows = normalize(serde_json::from_slice(&bytes)?, &c.page_map, &end)?;
    if rows.is_empty() {
        bail!("No mapped pages in Cloud response; existing snapshot preserved");
    }
    let path = root.join(".refreshagent/performance.json");
    let temp = path.with_extension("tmp");
    fs::write(&temp, serde_json::to_vec_pretty(&rows)?)?;
    fs::rename(temp, &path)?;
    c.evidence_file = Some(".refreshagent/performance.json".into());
    c.save(root)?;
    println!("Synced {} mapped pages for {start} to {end}. Local scans now use this snapshot. Run cloud sync again to refresh.", rows.len());
    Ok(())
}
