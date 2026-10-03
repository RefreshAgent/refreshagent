use refreshagent::update::{self, Asset, Release};
use sha2::{Digest, Sha256};
use std::{fs, os::unix::fs::PermissionsExt};
fn script(version: &str) -> Vec<u8> {
    format!("#!/bin/sh\nprintf 'refreshagent {version}\\n'\n").into_bytes()
}
fn binary() -> (tempfile::TempDir, std::path::PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let p = t.path().join("refreshagent");
    fs::write(&p, script("0.1.0")).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    (t, p)
}
#[test]
fn version_and_target_selection_are_stable_only() {
    assert!(update::newer("v0.1.10", "0.1.2").unwrap());
    assert!(!update::newer("v0.1.1", "0.1.2").unwrap());
    assert!(!update::newer("v0.2.0-beta.1", "0.1.2").unwrap());
    assert!(update::newer("not-a-version", "0.1.2").is_err());
    assert_eq!(
        update::target("macos", "aarch64").unwrap(),
        "aarch64-apple-darwin"
    );
    assert!(update::target("windows", "x86_64").is_err());
}
#[test]
fn corrupted_or_inconsistent_downloads_are_rejected() {
    let bytes = script("0.1.1");
    let digest = format!("{:x}", Sha256::digest(&bytes));
    update::verify(
        &bytes,
        &format!("{digest}  refreshagent"),
        Some(&format!("sha256:{digest}")),
    )
    .unwrap();
    assert!(update::verify(b"corrupt", &digest, None).is_err());
    assert!(update::verify(&bytes, &digest, Some("sha256:wrong")).is_err());
}
#[test]
fn missing_or_foreign_assets_are_rejected() {
    let a = Asset {
        name: "refreshagent-x86_64-unknown-linux-gnu".into(),
        browser_download_url: "https://evil.example/download".into(),
        size: 100,
        digest: None,
    };
    let mut r = Release {
        tag_name: "v0.1.1".into(),
        draft: false,
        prerelease: false,
        assets: vec![a],
    };
    assert!(update::select(&r, "x86_64-unknown-linux-gnu").is_err());
    r.assets.push(Asset {
        name: "refreshagent-x86_64-unknown-linux-gnu.sha256".into(),
        browser_download_url: "https://evil.example/hash".into(),
        size: 64,
        digest: None,
    });
    assert!(update::select(&r, "x86_64-unknown-linux-gnu").is_err());
}
#[test]
fn atomic_install_retains_backup_and_failed_probe_preserves_current() {
    let (_t, p) = binary();
    let original = fs::read(&p).unwrap();
    update::install(&p, &script("0.1.1"), "0.1.1").unwrap();
    assert_eq!(
        fs::read(p.with_file_name("refreshagent.previous")).unwrap(),
        original
    );
    let upgraded = fs::read(&p).unwrap();
    assert!(update::install(&p, &script("9.9.9"), "0.1.2").is_err());
    assert_eq!(fs::read(&p).unwrap(), upgraded);
    assert!(update::install(&p, b"not executable", "0.1.2").is_err());
    assert_eq!(fs::read(&p).unwrap(), upgraded);
}
#[test]
fn package_manager_and_development_installs_are_not_overwritten() {
    assert!(update::protected_install(std::path::Path::new(
        "/repo/target/debug/refreshagent"
    )));
    assert!(update::protected_install(std::path::Path::new(
        "/opt/homebrew/Cellar/refreshagent/1/bin/refreshagent"
    )));
    assert!(!update::protected_install(std::path::Path::new(
        "/home/me/.cargo/bin/refreshagent"
    )));
}
