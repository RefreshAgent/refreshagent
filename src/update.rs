//! Release updates are independent of project/Cloud configuration.
use anyhow::{bail, Context, Result};
use fs2::FileExt;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
const API: &str = "https://api.github.com/repos/RefreshAgent/refreshagent/releases/latest";
const PREFIX: &str = "https://github.com/RefreshAgent/refreshagent/releases/download/";
const LIMIT: u64 = 100_000_000;
#[derive(Debug, Deserialize)]
pub struct Release {
    pub tag_name: String,
    pub draft: bool,
    pub prerelease: bool,
    pub assets: Vec<Asset>,
}
#[derive(Debug, Deserialize)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    pub size: u64,
    pub digest: Option<String>,
}
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
struct Settings {
    disabled: bool,
    last_checked: u64,
}
#[derive(Debug, PartialEq)]
pub enum Outcome {
    Current,
    NoRelease,
    Disabled,
    Deferred,
    Updated(String),
    Available(String),
}
pub fn target(os: &str, arch: &str) -> Result<&'static str> {
    match (os, arch) {
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        _ => bail!("No release binary for {os}/{arch}"),
    }
}
pub fn newer(tag: &str, current: &str) -> Result<bool> {
    let candidate = Version::parse(tag.strip_prefix('v').unwrap_or(tag))?;
    Ok(candidate.pre.is_empty() && candidate > Version::parse(current)?)
}
pub fn select<'a>(release: &'a Release, triple: &str) -> Result<(&'a Asset, &'a Asset)> {
    if release.draft || release.prerelease {
        bail!("Only published stable releases can be installed");
    }
    let version = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name);
    if !Version::parse(version)?.pre.is_empty() {
        bail!("Prerelease tag rejected");
    }
    let name = format!("refreshagent-{triple}");
    let binary = release
        .assets
        .iter()
        .find(|a| a.name == name)
        .context("Matching release binary is missing")?;
    let checksum = release
        .assets
        .iter()
        .find(|a| a.name == format!("{name}.sha256"))
        .context("Release checksum is missing")?;
    for asset in [binary, checksum] {
        let expected = format!("{PREFIX}{}/{}", release.tag_name, asset.name);
        if asset.browser_download_url != expected {
            bail!("Unexpected release asset URL");
        }
    }
    if binary.size == 0 || binary.size > LIMIT || checksum.size > 4096 {
        bail!("Invalid release asset size");
    }
    Ok((binary, checksum))
}
pub fn verify(bytes: &[u8], checksum: &str, digest: Option<&str>) -> Result<()> {
    let expected = checksum
        .split_whitespace()
        .next()
        .context("Empty checksum")?;
    if expected.len() != 64 || !expected.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("Invalid SHA-256 checksum");
    }
    let actual = format!("{:x}", Sha256::digest(bytes));
    if actual != expected.to_lowercase() {
        bail!("Release checksum mismatch; installed binary untouched");
    }
    if let Some(digest) = digest {
        if digest != format!("sha256:{actual}") {
            bail!("GitHub asset digest mismatch");
        }
    }
    Ok(())
}
fn home() -> Result<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .context("HOME is unset")
}
fn settings_path() -> Result<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or(home()?.join(".config"));
    Ok(base.join("refreshagent/update.toml"))
}
fn settings() -> Result<Settings> {
    let path = settings_path()?;
    if path.exists() {
        Ok(toml::from_str(&fs::read_to_string(path)?)?)
    } else {
        Ok(Settings::default())
    }
}
fn save(s: &Settings) -> Result<()> {
    let path = settings_path()?;
    fs::create_dir_all(path.parent().unwrap())?;
    let stage = path.with_extension(format!("{}.tmp", std::process::id()));
    fs::write(&stage, toml::to_string(s)?)?;
    fs::rename(stage, path)?;
    Ok(())
}
fn lock_for(exe: &Path) -> Result<File> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or(home()?.join(".cache"));
    let dir = base.join("refreshagent/updates");
    fs::create_dir_all(&dir)?;
    let name = format!(
        "{:x}.lock",
        Sha256::digest(exe.as_os_str().as_encoded_bytes())
    );
    Ok(OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(dir.join(name))?)
}
pub fn execution_lock() -> Result<File> {
    let file = lock_for(&std::env::current_exe()?.canonicalize()?)?;
    FileExt::lock_shared(&file)?;
    Ok(file)
}
pub fn protected_install(exe: &Path) -> bool {
    exe.components()
        .any(|p| p.as_os_str() == "target" || p.as_os_str() == "Cellar")
        || exe.starts_with("/nix/store")
}
fn client() -> Result<reqwest::blocking::Client> {
    Ok(reqwest::blocking::Client::builder()
        .user_agent(concat!("refreshagent/", env!("CARGO_PKG_VERSION")))
        .timeout(Duration::from_secs(30))
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()?)
}
fn download(client: &reqwest::blocking::Client, asset: &Asset, limit: u64) -> Result<Vec<u8>> {
    let response = client
        .get(&asset.browser_download_url)
        .send()?
        .error_for_status()?;
    let mut bytes = vec![];
    response.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit || bytes.len() as u64 != asset.size {
        bail!("Release download size mismatch");
    }
    Ok(bytes)
}
fn probe(exe: &Path) -> Result<String> {
    let log = exe.with_file_name(format!(".refreshagent-{}-probe.log", std::process::id()));
    let mut command = std::process::Command::new(exe);
    command.arg("--version");
    let checked = crate::agent::execute(command, Duration::from_secs(10), &log, None);
    let output = fs::read_to_string(&log).unwrap_or_default();
    let _ = fs::remove_file(log);
    checked.context("Binary failed its startup check")?;
    Ok(output.trim().to_string())
}
fn backup(exe: &Path) -> PathBuf {
    exe.with_file_name(format!(
        "{}.previous",
        exe.file_name().unwrap().to_string_lossy()
    ))
}
/// Same-directory staging and rename keep the executable path valid on failure.
/// A candidate must pass a bounded --version smoke check before any replacement.
pub fn install(exe: &Path, bytes: &[u8], version: &str) -> Result<()> {
    if fs::symlink_metadata(exe)?.file_type().is_symlink() {
        bail!("Resolve executable symlink before installation");
    }
    let suffix = format!(
        "{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    );
    let staged = exe.with_file_name(format!(".refreshagent-{suffix}.new"));
    let staged_backup = exe.with_file_name(format!(".refreshagent-{suffix}.old"));
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&staged)
            .context("Installation directory is not writable")?;
        file.write_all(bytes)?;
        file.set_permissions(fs::Permissions::from_mode(
            fs::metadata(exe)?.permissions().mode() & 0o777,
        ))?;
        file.sync_all()?;
        drop(file);
        let log = exe.with_file_name(format!(".refreshagent-{suffix}.check"));
        let mut command = std::process::Command::new(&staged);
        command.arg("--version");
        let checked = crate::agent::execute(command, Duration::from_secs(10), &log, None);
        let output = fs::read_to_string(&log).unwrap_or_default();
        let _ = fs::remove_file(log);
        checked.context("Downloaded binary failed its startup check")?;
        if output.trim() != format!("refreshagent {version}") {
            bail!("Downloaded binary version does not match release tag");
        }
        let mut previous = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&staged_backup)?;
        let mut source = File::open(exe)?;
        std::io::copy(&mut source, &mut previous)?;
        previous.set_permissions(fs::metadata(exe)?.permissions())?;
        previous.sync_all()?;
        fs::rename(&staged_backup, backup(exe))?;
        fs::rename(&staged, exe)?;
        if let Ok(directory) = File::open(exe.parent().unwrap()) {
            let _ = directory.sync_all();
        }
        Ok(())
    })();
    let _ = fs::remove_file(staged);
    let _ = fs::remove_file(staged_backup);
    result
}
pub fn check(automatic: bool, check_only: bool) -> Result<Outcome> {
    if automatic && std::env::var_os("REFRESHAGENT_NO_UPDATE").is_some() {
        return Ok(Outcome::Disabled);
    }
    let exe = std::env::current_exe()?.canonicalize()?;
    if cfg!(target_env = "musl") {
        bail!("Release updater requires a GNU Linux or macOS build");
    }
    if protected_install(&exe) && !check_only {
        if automatic {
            return Ok(Outcome::Disabled);
        }
        bail!("Development/package-managed binary: update through its build or package manager");
    }
    let lock = lock_for(&exe)?;
    if lock.try_lock_exclusive().is_err() {
        return Ok(Outcome::Deferred);
    }
    let mut s = settings()?;
    let now = crate::runner::now();
    if automatic && (s.disabled || now.saturating_sub(s.last_checked) < 86400) {
        return Ok(Outcome::Disabled);
    }
    if automatic {
        s.last_checked = now;
        save(&s)?;
    }
    let client = client()?;
    let response = client
        .get(API)
        .header("Accept", "application/vnd.github+json")
        .send()?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(Outcome::NoRelease);
    }
    let response = response.error_for_status()?;
    let mut bytes = vec![];
    response.take(1_000_001).read_to_end(&mut bytes)?;
    if bytes.len() > 1_000_000 {
        bail!("Release metadata is oversized");
    }
    let release: Release = serde_json::from_slice(&bytes)?;
    if !newer(&release.tag_name, env!("CARGO_PKG_VERSION"))? {
        return Ok(Outcome::Current);
    }
    let (binary, checksum) = select(
        &release,
        target(std::env::consts::OS, std::env::consts::ARCH)?,
    )?;
    if check_only {
        return Ok(Outcome::Available(release.tag_name));
    }
    let bytes = download(&client, binary, LIMIT)?;
    let hash = String::from_utf8(download(&client, checksum, 4096)?)?;
    verify(&bytes, &hash, binary.digest.as_deref())?;
    let version = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name);
    install(&exe, &bytes, version)?;
    Ok(Outcome::Updated(release.tag_name))
}
pub fn enable(enabled: bool) -> Result<()> {
    let lock = lock_for(&std::env::current_exe()?.canonicalize()?)?;
    lock.lock_exclusive()?;
    let mut s = settings()?;
    s.disabled = !enabled;
    if enabled {
        s.last_checked = 0;
    }
    save(&s)
}
pub fn rollback() -> Result<()> {
    let exe = std::env::current_exe()?.canonicalize()?;
    if protected_install(&exe) {
        bail!("Use your build/package manager to roll back this installation");
    }
    let lock = lock_for(&exe)?;
    lock.try_lock_exclusive()
        .context("Worker active; rollback deferred")?;
    let bytes = fs::read(backup(&exe)).context("No previous binary is available")?;
    // Obtain the previous version from the locally retained executable.
    let text = probe(&backup(&exe))?;
    let version = text
        .trim()
        .strip_prefix("refreshagent ")
        .context("Invalid previous version output")?;
    Version::parse(version)?;
    install(&exe, &bytes, version)?;
    let mut s = settings()?;
    s.disabled = true;
    save(&s)?;
    println!("Rolled back to {version}; automatic updates disabled until update enable");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn active_worker_defers_installation() {
        let temp = tempfile::tempdir().unwrap();
        let executable = temp.path().join("refreshagent");
        let worker = lock_for(&executable).unwrap();
        FileExt::lock_shared(&worker).unwrap();
        let updater = lock_for(&executable).unwrap();
        assert!(updater.try_lock_exclusive().is_err());
        drop(worker);
        assert!(updater.try_lock_exclusive().is_ok());
    }
}
