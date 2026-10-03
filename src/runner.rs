use crate::{
    agent,
    config::Config,
    scan::{self, Opportunity},
};
use anyhow::{bail, Context, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc::Sender,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub opportunity: Opportunity,
    pub started: u64,
    pub finished: Option<u64>,
    pub status: String,
    pub detail: String,
    pub worktree: PathBuf,
    pub branch: String,
}
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn git(root: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git").args(args).current_dir(root).output()?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.first().unwrap_or(&""),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(String::from_utf8(out.stdout)?)
}
pub fn state_dir(root: &Path) -> Result<PathBuf> {
    let common = git(root, &["rev-parse", "--git-common-dir"])?;
    let p = PathBuf::from(common.trim());
    Ok(if p.is_absolute() { p } else { root.join(p) }
        .canonicalize()?
        .join("refreshagent"))
}
pub fn history(root: &Path) -> Result<Vec<Run>> {
    let path = state_dir(root)?.join("runs");
    if !path.exists() {
        return Ok(vec![]);
    }
    let mut runs = vec![];
    for entry in fs::read_dir(path)? {
        let p = entry?.path().join("run.json");
        if p.exists() {
            runs.push(serde_json::from_str::<Run>(&fs::read_to_string(p)?)?);
        }
    }
    runs.sort_by(|a, b| b.started.cmp(&a.started).then(b.id.cmp(&a.id)));
    Ok(runs)
}
fn save(path: &Path, run: &Run) -> Result<()> {
    let tmp = path.join("run.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(run)?)?;
    fs::rename(tmp, path.join("run.json"))?;
    Ok(())
}
pub fn run(
    root: &Path,
    c: &Config,
    selected: Option<&str>,
    scheduled: bool,
    events: Option<&Sender<String>>,
) -> Result<String> {
    c.validate()?;
    agent::reset_cancellation();
    if c.content_mode != "repository" {
        bail!("API/mixed writes are not implemented yet. Repository execution requires content_mode=repository.");
    }
    let state = state_dir(root)?;
    fs::create_dir_all(&state)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(state.join("worker.lock"))?;
    lock.try_lock_exclusive()
        .context("Another RefreshAgent run is active for this repository")?;
    let runs = history(root)?;
    if scheduled {
        if c.paused {
            return Ok("Schedule paused".into());
        }
        if runs.iter().any(|r| r.status == "running") {
            bail!("An interrupted run needs review; use recover before resuming");
        }
        if runs.iter().take_while(|r| r.status == "failed").count() >= c.max_consecutive_failures {
            return Ok("Failure circuit breaker active; inspect history and recover".into());
        }
        if runs.first().is_some_and(|r| {
            now().saturating_sub(r.started) < c.interval_hours.saturating_mul(3600)
        }) {
            return Ok("Next run is not due".into());
        }
        if runs.iter().any(|r| r.status == "review") {
            return Ok(
                "A validated change is awaiting review; resolve it before more scheduled work"
                    .into(),
            );
        }
    }
    if !git(root, &["status", "--porcelain", "--untracked-files=no"])?
        .trim()
        .is_empty()
    {
        bail!("Commit or stash tracked changes before running");
    }
    git(root, &["rev-parse", "HEAD"])?;
    let ops = scan::scan(root, c)?;
    let opportunity = ops.into_iter().find(|o| {
        selected.map(|id| o.id == id).unwrap_or_else(|| {
            !runs.iter().any(|r| {
                r.opportunity.id == o.id
                    && ["review", "committed", "pull_request", "no_change"]
                        .contains(&r.status.as_str())
            })
        })
    });
    let Some(opportunity) = opportunity else {
        return Ok("No eligible evidence-backed improvement found".into());
    };
    if c.validation.is_empty() {
        bail!("Configure at least one validation command before execution");
    }
    let id = format!("{}-{}", now(), std::process::id());
    let dir = state.join("runs").join(&id);
    fs::create_dir_all(&dir)?;
    let work = state.join("worktrees").join(&id);
    fs::create_dir_all(work.parent().unwrap())?;
    let branch = format!("refreshagent/{id}");
    let base = git(root, &["rev-parse", "HEAD"])?;
    git(
        root,
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            work.to_str().context("Non UTF-8 worktree path")?,
            "HEAD",
        ],
    )?;
    let mut record = Run {
        id: id.clone(),
        opportunity,
        started: now(),
        finished: None,
        status: "running".into(),
        detail: String::new(),
        worktree: work.clone(),
        branch,
    };
    save(&dir, &record)?;
    let result = (|| -> Result<String> {
        let deadline =
            std::time::Instant::now() + Duration::from_secs(c.timeout_minutes.saturating_mul(60));
        let task = agent::prompt(c, &record.opportunity);
        fs::write(dir.join("task.txt"), &task)?;
        agent::execute(
            agent::command(c, &work, &task),
            deadline.saturating_duration_since(std::time::Instant::now()),
            &dir.join("events.jsonl"),
            events,
        )?;
        if git(&work, &["rev-parse", "HEAD"])?.trim() != base.trim() {
            bail!("Agent changed Git history; no delivery performed");
        }
        // Include new files in the diff without staging their content yet.
        let untracked = git(&work, &["ls-files", "--others", "--exclude-standard", "-z"])?;
        for path in untracked.split('\0').filter(|s| !s.is_empty()) {
            if !scan::allowed(path, c) {
                bail!("Agent created file outside content roots: {path}");
            }
            git(&work, &["add", "-N", "--", path])?;
        }
        check_scope(&work, c)?;
        let before = git(&work, &["diff", "HEAD", "--"])?;
        fs::write(dir.join("change.diff"), &before)?;
        if before.is_empty() {
            return Ok("no_change".into());
        }
        for (i, validation) in c.validation.iter().enumerate() {
            let mut cmd = Command::new("sh");
            cmd.args(["-c", validation]).current_dir(&work);
            agent::execute(
                cmd,
                deadline.saturating_duration_since(std::time::Instant::now()),
                &dir.join(format!("validation-{i}.log")),
                events,
            )?;
        }
        check_scope(&work, c)?;
        if git(&work, &["diff", "HEAD", "--"])? != before {
            bail!("Validation mutated content; inspect the worktree before delivery");
        }
        if agent::cancelled() || events.is_some_and(|s| s.send(String::new()).is_err()) {
            bail!("Run cancelled before delivery");
        }
        if c.delivery == "review" {
            return Ok("review".into());
        }
        git(&work, &["add", "--all"])?;
        git(
            &work,
            &[
                "commit",
                "-m",
                &format!(
                    "seo: address {} in {}",
                    record.opportunity.issue, record.opportunity.path
                ),
            ],
        )?;
        if c.delivery == "pull_request" {
            git(&work, &["push", "-u", "origin", &record.branch])?;
            let mut cmd = Command::new("gh");
            let body = format!("Addresses {}.\n\nEvidence: {}\n\nValidation passed: {:?}\n\nHypothesis and full local logs: refreshagent history", record.opportunity.issue, record.opportunity.evidence, c.validation);
            cmd.args([
                "pr",
                "create",
                "--head",
                &record.branch,
                "--title",
                &format!("SEO: {}", record.opportunity.issue),
                "--body",
                &body,
            ])
            .current_dir(&work);
            agent::execute(
                cmd,
                deadline.saturating_duration_since(std::time::Instant::now()),
                &dir.join("delivery.log"),
                events,
            )?;
            Ok("pull_request".into())
        } else {
            Ok("committed".into())
        }
    })();
    record.finished = Some(now());
    match result {
        Ok(status) => {
            record.status = status;
            record.detail = format!("Worktree: {}", work.display());
        }
        Err(e) => {
            record.status = "failed".into();
            record.detail = format!("{e:#}");
        }
    }
    save(&dir, &record)?;
    let message = format!("{}: {} — {}", record.id, record.status, record.detail);
    if record.status == "failed" {
        bail!("{message}");
    }
    Ok(message)
}
fn check_scope(work: &Path, c: &Config) -> Result<()> {
    let new_files = git(work, &["ls-files", "--others", "--exclude-standard", "-z"])?;
    if !new_files.is_empty() {
        bail!("Unexpected new files appeared after diff capture; inspect before delivery");
    }
    for path in git(work, &["diff", "HEAD", "--name-only", "-z", "--no-renames"])?
        .split('\0')
        .filter(|s| !s.is_empty())
    {
        if !scan::allowed(path, c) {
            bail!("Change outside approved roots: {path}");
        }
        let p = work.join(path);
        if p.exists() && fs::symlink_metadata(p)?.file_type().is_symlink() {
            bail!("Symlink changes are forbidden");
        }
    }
    Ok(())
}
pub fn resolve(root: &Path, id: &str, status: &str) -> Result<()> {
    if !["dismissed", "accepted"].contains(&status) {
        bail!("Invalid resolution");
    }
    let state = state_dir(root)?;
    let lock = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(state.join("worker.lock"))?;
    lock.try_lock_exclusive()
        .context("Cannot recover while a worker is active")?;
    let mut r = history(root)?
        .into_iter()
        .find(|r| r.id == id)
        .context("Unknown run")?;
    r.status = status.into();
    r.finished = Some(now());
    save(&state.join("runs").join(id), &r)
}
