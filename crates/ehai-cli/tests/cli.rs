//! End-to-end CLI tests. Every run gets its own EHAI_HOME and cwd.

use assert_cmd::Command;

struct Env {
    home: tempfile::TempDir,
    cwd: tempfile::TempDir,
}

impl Env {
    fn new() -> Self {
        Self {
            home: tempfile::tempdir().unwrap(),
            cwd: tempfile::tempdir().unwrap(),
        }
    }

    fn cmd(&self) -> Command {
        let mut c = Command::cargo_bin("ehai").unwrap();
        for (k, _) in std::env::vars().filter(|(k, _)| k.starts_with("EHAI_")) {
            c.env_remove(k);
        }
        c.env("EHAI_HOME", self.home.path())
            .env("NO_COLOR", "1")
            .current_dir(self.cwd.path());
        c
    }
}

fn stdout(out: &std::process::Output) -> String {
    String::from_utf8(out.stdout.clone()).unwrap()
}

#[test]
fn version_line() {
    let out = Env::new().cmd().arg("--version").output().unwrap();
    assert!(out.status.success());
    let s = stdout(&out);
    assert!(s.starts_with("ehai 0.1.0-alpha.1 ("), "{s}");
    assert!(s.contains(env!("TARGET_TRIPLE_FOR_TESTS")), "{s}");
}

#[test]
fn help_is_stable() {
    let out = Env::new().cmd().arg("--help").output().unwrap();
    insta::assert_snapshot!(stdout(&out));
}

#[test]
fn doctor_passes_in_sane_env() {
    let env = Env::new();
    let out = env.cmd().arg("doctor").output().unwrap();
    let s = stdout(&out);
    assert!(out.status.success(), "{s}");
    assert!(s.contains("✔ Git"), "{s}");
    assert!(s.contains("All checks passed"), "{s}");
    assert!(env.home.path().join("logs").is_dir(), "log dir created");
}

#[test]
fn doctor_fails_with_fix_hint_on_unwritable_home() {
    let env = Env::new();
    let file = env.home.path().join("not-a-dir");
    std::fs::write(&file, "").unwrap();
    let out = env
        .cmd()
        .env("EHAI_HOME", file.join("sub"))
        .arg("doctor")
        .output()
        .unwrap();
    let s = stdout(&out);
    assert!(!out.status.success(), "{s}");
    assert!(s.contains("✖ Home dir"), "{s}");
    assert!(s.contains("fix:"), "{s}");
}

#[test]
fn doctor_reports_broken_config_and_config_show_errors() {
    let env = Env::new();
    std::fs::write(
        env.home.path().join("config.toml"),
        "[compliance]\nmode = \"loose\"",
    )
    .unwrap();
    let out = env.cmd().arg("doctor").output().unwrap();
    assert!(!out.status.success());
    assert!(stdout(&out).contains("✖ Config"));

    let out = env.cmd().args(["config", "show"]).output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("invalid config"));
}

#[test]
fn config_layers_apply_end_to_end() {
    let env = Env::new();
    std::fs::write(env.home.path().join("config.toml"), "role = \"pm\"").unwrap();
    std::fs::write(
        env.cwd.path().join(".ehai.toml"),
        "[compliance]\nmode = \"strict\"",
    )
    .unwrap();
    let out = env
        .cmd()
        .env("EHAI_BRAND__NAME", "acme")
        .args(["config", "show"])
        .output()
        .unwrap();
    let s = stdout(&out);
    assert!(out.status.success(), "{s}");
    assert!(s.contains("role = \"pm\""), "{s}");
    assert!(s.contains("mode = \"strict\""), "{s}");
    assert!(s.contains("name = \"acme\""), "{s}");
}

#[test]
fn schema_is_valid_json() {
    let out = Env::new()
        .cmd()
        .args(["config", "schema"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(v["properties"]["compliance"].is_object());
}
