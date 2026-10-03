use refreshagent::{agent, config::Config, runner, scan, service};
use std::{fs, process::Command, time::Duration};
use tempfile::TempDir;
fn repo() -> TempDir {
    let t = TempDir::new().unwrap();
    for args in [
        vec!["init", "-b", "main"],
        vec!["config", "user.email", "test@example.com"],
        vec!["config", "user.name", "Test"],
    ] {
        assert!(Command::new("git")
            .args(args)
            .current_dir(t.path())
            .status()
            .unwrap()
            .success());
    }
    fs::create_dir(t.path().join("content")).unwrap();
    fs::write(
        t.path().join("content/page.md"),
        "A useful page without metadata.\n",
    )
    .unwrap();
    runner::git(t.path(), &["add", "."]).unwrap();
    runner::git(t.path(), &["commit", "-m", "initial"]).unwrap();
    t
}
fn fake(t: &TempDir, script: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let p = t.path().join("fake-agent");
    fs::write(&p, format!("#!/bin/sh\n{script}\n")).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    p
}
#[test]
fn scope_rejects_traversal_and_prefix_collisions() {
    let mut c = Config {
        content_roots: vec!["content".into()],
        ..Config::default()
    };
    assert!(scan::allowed("content/page.md", &c));
    assert!(!scan::allowed("content-other/page.md", &c));
    c.content_roots = vec!["../outside".into()];
    assert!(c.validate().is_err());
}
#[test]
fn scans_are_stable_and_performance_enriches_without_cloud() {
    let t = repo();
    let mut c = Config {
        content_roots: vec!["content".into()],
        ..Config::default()
    };
    let a = scan::scan(t.path(), &c).unwrap();
    assert_eq!(a.len(), 2);
    assert_eq!(a[0].id, scan::scan(t.path(), &c).unwrap()[0].id);
    fs::write(t.path().join("metrics.json"), r#"[{"path":"content/page.md","clicks":20,"impressions":2000,"position":12,"period_end":"2026-10-01"}]"#).unwrap();
    c.evidence_file = Some("metrics.json".into());
    let b = scan::scan(t.path(), &c).unwrap();
    assert!(b[0].priority > a[0].priority);
    assert!(b[0].evidence.contains("2000"));
}
#[test]
fn isolated_run_validates_and_schedule_waits_for_review() {
    let t = repo();
    let c = Config { content_roots: vec!["content".into()], agent_executable: fake(&t, "printf '# Useful page\\n\\nA factual summary.\\n' > content/page.md\nprintf '{\"type\":\"done\"}\\n'"), validation: vec!["test -s content/page.md".into()], ..Config::default() };
    let result = runner::run(t.path(), &c, None, false, None).unwrap();
    assert!(result.contains("review"));
    assert_eq!(
        fs::read_to_string(t.path().join("content/page.md")).unwrap(),
        "A useful page without metadata.\n"
    );
    let h = runner::history(t.path()).unwrap();
    assert_eq!(h[0].status, "review");
    assert!(h[0].worktree.join("content/page.md").exists());
    let due = Config {
        interval_hours: 1,
        ..c
    };
    assert!(runner::run(t.path(), &due, None, true, None)
        .unwrap()
        .contains("not due"));
}
#[test]
fn unauthorized_diff_and_failed_validation_block_delivery() {
    for script in ["echo bad > outside.txt", "echo changed >> content/page.md"] {
        let t = repo();
        let c = Config {
            content_roots: vec!["content".into()],
            agent_executable: fake(&t, script),
            validation: vec!["false".into()],
            delivery: "commit".into(),
            ..Config::default()
        };
        assert!(runner::run(t.path(), &c, None, false, None).is_err());
        assert_eq!(runner::history(t.path()).unwrap()[0].status, "failed");
        assert_eq!(
            runner::git(t.path(), &["rev-list", "--count", "HEAD"])
                .unwrap()
                .trim(),
            "1"
        );
    }
}
#[test]
fn timeout_kills_a_process_group() {
    let t = TempDir::new().unwrap();
    let mut cmd = Command::new("sh");
    cmd.args(["-c", "sleep 30 & wait"]);
    let start = std::time::Instant::now();
    assert!(agent::execute(cmd, Duration::from_millis(50), &t.path().join("log"), None).is_err());
    assert!(start.elapsed() < Duration::from_secs(3));
}
#[test]
fn renders_escaped_services_with_absolute_agent_environment() {
    let (_, unit) = service::render(
        std::path::Path::new("/tmp/site with spaces"),
        std::path::Path::new("/opt/refreshagent"),
        "linux",
        "/usr/bin:/opt/agents",
    )
    .unwrap();
    assert!(unit.contains("WorkingDirectory=\"/tmp/site with spaces\""));
    assert!(unit.contains("PATH=/usr/bin:/opt/agents"));
    let (_, plist) = service::render(
        std::path::Path::new("/tmp/a&b"),
        std::path::Path::new("/opt/refreshagent"),
        "macos",
        "/usr/bin",
    )
    .unwrap();
    assert!(plist.contains("a&amp;b"));
    assert!(!plist.contains("KeepAlive"));
}

#[test]
fn cloud_adapter_maps_only_explicit_pages_and_rejects_invalid_metrics() {
    use refreshagent::cloud::{normalize, Response};
    let data: Response = serde_json::from_str(r#"{"rows":[{"keys":["https://example.com/page"],"clicks":10,"impressions":100,"position":9},{"keys":["https://other.com/page"],"clicks":5,"impressions":50,"position":3}]}"#).unwrap();
    let map = std::collections::BTreeMap::from([(
        "https://example.com/page".into(),
        "content/page.md".into(),
    )]);
    let rows = normalize(data, &map, "2026-10-01").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].path, "content/page.md");
    let bad: Response = serde_json::from_str(r#"{"rows":[{"keys":["https://example.com/page"],"clicks":200,"impressions":100,"position":9}]}"#).unwrap();
    assert!(normalize(bad, &map, "2026-10-01").is_err());
}

#[test]
fn validation_new_files_and_history_mutations_prevent_delivery() {
    for script in [
        "echo changed >> content/page.md",
        "git commit --allow-empty -m unexpected",
    ] {
        let t = repo();
        let c = Config {
            content_roots: vec!["content".into()],
            agent_executable: fake(&t, script),
            validation: vec!["echo unexpected > new.txt".into()],
            delivery: "commit".into(),
            ..Config::default()
        };
        assert!(runner::run(t.path(), &c, None, false, None).is_err());
        assert_eq!(runner::history(t.path()).unwrap()[0].status, "failed");
    }
}
