use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    pub site_url: String,
    pub content_mode: String,
    pub content_roots: Vec<String>,
    pub agent: String,
    pub agent_executable: PathBuf,
    pub validation: Vec<String>,
    pub delivery: String,
    pub interval_hours: u64,
    pub timeout_minutes: u64,
    pub max_consecutive_failures: usize,
    pub paused: bool,
    pub evidence_file: Option<PathBuf>,
    pub cloud_property: Option<String>,
    pub page_map: std::collections::BTreeMap<String, String>,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            version: 1,
            site_url: String::new(),
            content_mode: "repository".into(),
            content_roots: vec![
                "content".into(),
                "src/content".into(),
                "src/pages".into(),
                "pages".into(),
                "app".into(),
            ],
            agent: "codex".into(),
            agent_executable: "codex".into(),
            validation: vec![],
            delivery: "review".into(),
            interval_hours: 24,
            timeout_minutes: 30,
            max_consecutive_failures: 3,
            paused: false,
            evidence_file: None,
            cloud_property: None,
            page_map: Default::default(),
        }
    }
}
impl Config {
    pub fn path(root: &Path) -> PathBuf {
        root.join(".refreshagent/config.toml")
    }
    pub fn load(root: &Path) -> Result<Self> {
        let config: Self = toml::from_str(
            &fs::read_to_string(Self::path(root)).context("Run refreshagent init first")?,
        )?;
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            bail!("Unsupported config version");
        }
        if !["codex", "claude"].contains(&self.agent.as_str()) {
            bail!("Agent must be codex or claude");
        }
        if !["repository", "api", "mixed"].contains(&self.content_mode.as_str()) {
            bail!("Invalid content mode");
        }
        if !["review", "commit", "pull_request"].contains(&self.delivery.as_str()) {
            bail!("Invalid delivery policy");
        }
        if self.interval_hours == 0
            || self.timeout_minutes == 0
            || self.max_consecutive_failures == 0
        {
            bail!("Budgets must be positive");
        }
        if self.content_roots.is_empty() {
            bail!("At least one content root is required");
        }
        for root in &self.content_roots {
            let p = Path::new(root);
            if root.is_empty()
                || root == "."
                || p.is_absolute()
                || p.components()
                    .any(|c| !matches!(c, std::path::Component::Normal(_)))
            {
                bail!("Content roots must be relative directories without traversal: {root}");
            }
        }
        Ok(())
    }
    pub fn save(&self, root: &Path) -> Result<()> {
        self.validate()?;
        fs::create_dir_all(root.join(".refreshagent"))?;
        let path = Self::path(root);
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, toml::to_string_pretty(self)?)?;
        fs::rename(tmp, path)?;
        Ok(())
    }
}
pub fn repo_root(path: &Path) -> Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(path)
        .output()?;
    if !output.status.success() {
        bail!("Run inside a Git repository with an initial commit");
    }
    Ok(PathBuf::from(String::from_utf8(output.stdout)?.trim()).canonicalize()?)
}
pub fn executable(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|v| {
        std::env::split_paths(&v)
            .map(|p| p.join(name))
            .find(|p| p.is_file())
    })
}
pub fn discover(root: &Path) -> (Config, String) {
    let mut c = Config::default();
    let frameworks = [
        ("astro.config.mjs", "Astro"),
        ("next.config.js", "Next.js"),
        ("next.config.ts", "Next.js"),
        ("hugo.toml", "Hugo"),
        ("Cargo.toml", "Rust"),
        ("package.json", "JavaScript"),
    ];
    let framework = frameworks
        .iter()
        .find(|(p, _)| root.join(p).exists())
        .map(|(_, n)| *n)
        .unwrap_or("Unknown");
    c.content_roots.retain(|p| root.join(p).is_dir());
    if c.content_roots.is_empty() {
        c.content_roots = vec!["content".into()];
    }
    if let Some(p) = executable("codex") {
        c.agent_executable = p;
    } else if let Some(p) = executable("claude") {
        c.agent = "claude".into();
        c.agent_executable = p;
    }
    if let Ok(s) = fs::read_to_string(root.join("package.json")) {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&s) {
            if v["scripts"]["build"].is_string() {
                c.validation.push("npm run build".into());
            }
        }
    }
    (c, framework.into())
}
