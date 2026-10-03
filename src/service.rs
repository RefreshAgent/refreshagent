use crate::config::Config;
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
pub fn name(root: &Path) -> String {
    format!(
        "refreshagent-{:x}",
        Sha256::digest(root.as_os_str().as_encoded_bytes())
    )[..29]
        .to_string()
}
fn quote(s: &str) -> String {
    format!(
        "\"{}\"",
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
    )
}
fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}
pub fn render(root: &Path, exe: &Path, platform: &str, path: &str) -> Result<(String, String)> {
    let id = name(root);
    let root = root.to_str().context("Non UTF-8 project path")?;
    let exe = exe.to_str().context("Non UTF-8 executable path")?;
    if [root, exe, path]
        .iter()
        .any(|s| s.contains(['\n', '\r', '\0']))
    {
        bail!("Unsupported control character in service paths");
    }
    match platform {
        "linux" => Ok((format!("{id}.service"), format!("[Unit]\nDescription=RefreshAgent autonomous SEO worker\n\n[Service]\nType=oneshot\nWorkingDirectory={}\nEnvironment={}\nExecStart={} --project {} tick\nKillMode=control-group\nTimeoutStartSec=infinity\n", quote(root), quote(&format!("PATH={path}")), quote(exe), quote(root)))),
        "macos" => Ok((format!("com.refreshagent.{id}.plist"), format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>Label</key><string>com.refreshagent.{id}</string><key>ProgramArguments</key><array><string>{}</string><string>--project</string><string>{}</string><string>tick</string></array><key>WorkingDirectory</key><string>{}</string><key>EnvironmentVariables</key><dict><key>PATH</key><string>{}</string></dict><key>StartInterval</key><integer>900</integer><key>RunAtLoad</key><true/><key>StandardOutPath</key><string>{}/.refreshagent/service.log</string><key>StandardErrorPath</key><string>{}/.refreshagent/service-error.log</string></dict></plist>\n", xml(exe), xml(root), xml(root), xml(path), xml(root), xml(root)))),
        _ => bail!("Only Linux systemd and macOS launchd are supported"),
    }
}
fn invoke(program: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(program).args(args).status()?;
    if !status.success() {
        bail!("{program} failed with {status}");
    }
    Ok(())
}
fn home() -> Result<PathBuf> {
    Ok(PathBuf::from(
        std::env::var_os("HOME").context("HOME is unset")?,
    ))
}
pub fn install(root: &Path, preview: bool) -> Result<()> {
    let c = Config::load(root)?;
    if c.content_mode != "repository" || c.validation.is_empty() {
        bail!("Configure repository content and validation before scheduling");
    }
    let platform = std::env::consts::OS;
    let path = std::env::var("PATH")?;
    let (file, contents) = render(root, &std::env::current_exe()?, platform, &path)?;
    let id = name(root);
    let timer = format!("[Unit]\nDescription=Check RefreshAgent schedule\n[Timer]\nOnBootSec=2min\nOnUnitActiveSec=15min\nUnit={id}.service\n[Install]\nWantedBy=timers.target\n");
    if preview {
        println!("{file}\n{contents}");
        if platform == "linux" {
            println!("{id}.timer\n{timer}");
        }
        return Ok(());
    }
    if platform == "linux" {
        let dir = home()?.join(".config/systemd/user");
        fs::create_dir_all(&dir)?;
        fs::write(dir.join(file), contents)?;
        fs::write(dir.join(format!("{id}.timer")), timer)?;
        invoke("systemctl", &["--user", "daemon-reload"])?;
        invoke(
            "systemctl",
            &["--user", "enable", "--now", &format!("{id}.timer")],
        )?;
    } else {
        let dir = home()?.join("Library/LaunchAgents");
        fs::create_dir_all(&dir)?;
        let dest = dir.join(file);
        fs::write(&dest, contents)?;
        let domain = format!("gui/{}", unsafe { libc::getuid() });
        invoke(
            "launchctl",
            &[
                "bootstrap",
                &domain,
                dest.to_str().context("Invalid service path")?,
            ],
        )?;
    }
    println!("Installed {id}. Uses existing local agent authentication. Check refreshagent service status.");
    Ok(())
}
pub fn manage(root: &Path, action: &str) -> Result<()> {
    let id = name(root);
    if std::env::consts::OS == "linux" {
        if action == "status" {
            return invoke("systemctl", &["--user", "status", &format!("{id}.timer")]);
        }
        invoke(
            "systemctl",
            &["--user", "disable", "--now", &format!("{id}.timer")],
        )?;
        invoke("systemctl", &["--user", "stop", &format!("{id}.service")])?;
        let dir = home()?.join(".config/systemd/user");
        for ext in ["service", "timer"] {
            let p = dir.join(format!("{id}.{ext}"));
            if p.exists() {
                fs::remove_file(p)?;
            }
        }
        invoke("systemctl", &["--user", "daemon-reload"])
    } else if std::env::consts::OS == "macos" {
        let domain = format!("gui/{}", unsafe { libc::getuid() });
        let label = format!("com.refreshagent.{id}");
        if action == "status" {
            return invoke("launchctl", &["print", &format!("{domain}/{label}")]);
        }
        invoke("launchctl", &["bootout", &format!("{domain}/{label}")])?;
        fs::remove_file(
            home()?
                .join("Library/LaunchAgents")
                .join(format!("{label}.plist")),
        )?;
        Ok(())
    } else {
        bail!("Unsupported service platform");
    }
}
