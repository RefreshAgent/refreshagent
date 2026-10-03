use crate::config::Config;
use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashMap, fs, path::Path, process::Command};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Opportunity {
    pub id: String,
    pub path: String,
    pub issue: String,
    pub evidence: String,
    pub priority: f64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Performance {
    pub path: String,
    pub clicks: f64,
    pub impressions: f64,
    pub position: f64,
    pub period_end: String,
}
pub fn allowed(path: &str, config: &Config) -> bool {
    config
        .content_roots
        .iter()
        .any(|r| Path::new(path).starts_with(r))
}
pub fn scan(root: &Path, c: &Config) -> Result<Vec<Opportunity>> {
    let out = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .output()?;
    if !out.status.success() {
        bail!("Cannot list repository content");
    }
    let mut opportunities = vec![];
    let mut titles: HashMap<String, String> = HashMap::new();
    for bytes in out.stdout.split(|b| *b == 0).filter(|b| !b.is_empty()) {
        let path = String::from_utf8_lossy(bytes).into_owned();
        if !allowed(&path, c) {
            continue;
        }
        let p = root.join(&path);
        if fs::symlink_metadata(&p)?.file_type().is_symlink() {
            continue;
        }
        let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("");
        if !["md", "mdx", "html", "astro"].contains(&ext) || fs::metadata(&p)?.len() > 1_000_000 {
            continue;
        }
        let text = fs::read_to_string(&p)?;
        let title = text
            .lines()
            .find_map(|l| l.strip_prefix("# ").or_else(|| l.strip_prefix("title:")))
            .map(|s| s.trim().trim_matches('"').to_lowercase());
        let mut add = |issue: &str, evidence: String, priority: f64| {
            let fingerprint = format!("{path}:{issue}:{text}");
            let id = format!("{:x}", Sha256::digest(fingerprint.as_bytes()))[..16].to_string();
            opportunities.push(Opportunity {
                id,
                path: path.clone(),
                issue: issue.into(),
                evidence,
                priority,
            });
        };
        if ["md", "mdx"].contains(&ext) && title.is_none() {
            add("missing-page-title", "No Markdown H1 or frontmatter title detected; verify the template before adding one.".into(), 60.0);
        }
        if let Some(t) = title {
            if let Some(other) = titles.insert(t.clone(), path.clone()) {
                add("duplicate-title", format!("Title '{t}' also appears in {other}; inspect whether these pages serve distinct intent."), 70.0);
            }
        }
        if ["md", "mdx"].contains(&ext) && !text.contains("description:") {
            add("missing-description", "No frontmatter description detected; verify metadata generation and write a factual summary if needed.".into(), 40.0);
        }
        if text.contains("]()") || text.contains("href=\"\"") {
            add(
                "empty-link",
                "An empty Markdown or HTML link destination was found.".into(),
                80.0,
            );
        }
    }
    if let Some(file) = &c.evidence_file {
        let p = if file.is_absolute() {
            file.clone()
        } else {
            root.join(file)
        };
        let metrics: Vec<Performance> = serde_json::from_str(&fs::read_to_string(p)?)?;
        for o in &mut opportunities {
            if let Some(m) = metrics.iter().find(|m| m.path == o.path) {
                if ![m.clicks, m.impressions, m.position]
                    .iter()
                    .all(|n| n.is_finite() && *n >= 0.0)
                    || m.clicks > m.impressions
                {
                    bail!("Invalid performance metrics for {}", m.path);
                }
                o.priority += (m.impressions + 1.0).log10().min(6.0) * 5.0;
                o.evidence.push_str(&format!(" Performance period ending {}: {} impressions, {} clicks, position {}. Observations do not establish causality.", m.period_end, m.impressions, m.clicks, m.position));
            }
        }
    }
    opportunities.sort_by(|a, b| b.priority.total_cmp(&a.priority).then(a.path.cmp(&b.path)));
    Ok(opportunities)
}
