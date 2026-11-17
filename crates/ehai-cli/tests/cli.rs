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

#[test]
fn init_non_interactive_writes_config_and_repo_profile() {
    let env = Env::new();
    // a git repo with a Vite app as the cwd
    let g = |args: &[&str]| {
        assert!(std::process::Command::new("git")
            .args(args)
            .current_dir(env.cwd.path())
            .status()
            .unwrap()
            .success())
    };
    g(&["init", "-q"]);
    std::fs::write(
        env.cwd.path().join("package.json"),
        r#"{"dependencies":{"react":"19"},"devDependencies":{"vite":"6"}}"#,
    )
    .unwrap();

    let out = env
        .cmd()
        .args([
            "init",
            "--yes",
            "--role",
            "pm",
            "--compliance",
            "strict",
            "--harness",
            "none",
        ])
        .output()
        .unwrap();
    let s = stdout(&out);
    assert!(s.contains("You're ready."), "{s}");
    assert!(s.contains("/ehai:help"), "pm next steps shown: {s}");

    let cfg = std::fs::read_to_string(env.home.path().join("config.toml")).unwrap();
    assert!(cfg.contains("role = \"pm\""), "{cfg}");
    assert!(cfg.contains("mode = \"strict\""), "{cfg}");
    let profile = std::fs::read_to_string(env.cwd.path().join(".ehai/profile.toml")).unwrap();
    assert!(profile.contains("vite-react"), "{profile}");

    // rerun is idempotent
    let again = env
        .cmd()
        .args(["init", "--yes", "--harness", "none"])
        .output()
        .unwrap();
    assert!(again.status.success(), "{}", stdout(&again));
    let cfg2 = std::fs::read_to_string(env.home.path().join("config.toml")).unwrap();
    assert!(
        cfg2.contains("role = \"pm\""),
        "kept previous role as default: {cfg2}"
    );
}

#[test]
fn adapters_list_and_unknown_id() {
    let env = Env::new();
    let out = env.cmd().args(["adapters", "list"]).output().unwrap();
    assert!(out.status.success());
    assert!(stdout(&out).contains("claude-code"));
    let bad = env
        .cmd()
        .args(["adapters", "install", "vim"])
        .output()
        .unwrap();
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("available: claude-code"));
}

#[test]
fn repo_init_writes_profile_and_requires_a_repo() {
    let env = Env::new();
    let out = env.cmd().args(["repo", "init", "--yes"]).output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("not inside a git repository"));

    assert!(std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(env.cwd.path())
        .status()
        .unwrap()
        .success());
    std::fs::create_dir_all(env.cwd.path().join("api")).unwrap();
    std::fs::write(env.cwd.path().join("api/go.mod"), "module api\n").unwrap();
    let out = env.cmd().args(["repo", "init", "--yes"]).output().unwrap();
    let s = stdout(&out);
    assert!(out.status.success(), "{s}");
    assert!(s.contains("api → Go"), "{s}");
    assert!(env.cwd.path().join(".ehai/profile.toml").is_file());
}

/// Full lifecycle in a sandboxed HOME with a fake `claude` CLI:
/// self install → connect → disconnect → reconnect → uninstall leaves no traces.
#[cfg(unix)]
#[test]
fn install_connect_disconnect_uninstall_leaves_no_traces() {
    use std::os::unix::fs::PermissionsExt;
    let env = Env::new();
    let user_home = env.home.path().join("user");
    let ehai_home = user_home.join(".ehai");
    std::fs::create_dir_all(&user_home).unwrap();
    let bashrc = user_home.join(".bashrc");
    std::fs::write(&bashrc, "alias ll='ls -l'\n").unwrap();

    let fake = env.home.path().join("fakebin/claude");
    std::fs::create_dir_all(fake.parent().unwrap()).unwrap();
    std::fs::write(
        &fake,
        r#"#!/bin/sh
S="$(dirname "$0")"
case "$*" in
  "plugin marketplace list --json") cat "$S/mps.json" 2>/dev/null || echo "[]" ;;
  "plugin list --json") cat "$S/plugins.json" 2>/dev/null || echo "[]" ;;
  "plugin marketplace add "*) printf '[{"name":"ehai-local","path":"%s"}]' "$4" > "$S/mps.json" ;;
  "plugin install "*) printf '[{"id":"%s"}]' "$3" > "$S/plugins.json" ;;
  "plugin marketplace update "*|"plugin update "*) : ;;
  "plugin marketplace remove "*) echo "[]" > "$S/mps.json"; echo "[]" > "$S/plugins.json" ;;
  *) exit 1 ;;
esac
"#,
    )
    .unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();

    let run = |args: &[&str]| {
        let out = env
            .cmd()
            .env("HOME", &user_home)
            .env("EHAI_HOME", &ehai_home)
            .env("SHELL", "/bin/bash")
            .env("EHAI_CLAUDE_BIN", &fake)
            .args(args)
            .output()
            .unwrap();
        let s = stdout(&out);
        assert!(
            out.status.success(),
            "{args:?}: {s}{}",
            String::from_utf8_lossy(&out.stderr)
        );
        s
    };
    let plugins =
        || std::fs::read_to_string(fake.with_file_name("plugins.json")).unwrap_or_default();

    run(&["self", "install"]);
    run(&["self", "install"]);
    assert!(ehai_home.join("bin/ehai").is_file());
    let rc = std::fs::read_to_string(&bashrc).unwrap();
    assert_eq!(rc.matches(">>> ehai >>>").count(), 1, "{rc}");
    assert!(rc.contains(&ehai_home.join("bin").display().to_string()));

    run(&["connect", "claude-code"]);
    assert!(plugins().contains("ehai@ehai-local"));
    let mcp =
        std::fs::read_to_string(ehai_home.join("adapters/claude-code/plugins/ehai/.mcp.json"))
            .unwrap();
    assert!(
        mcp.contains(".ehai/bin/ehai"),
        "plugin points at the installed binary: {mcp}"
    );

    let s = run(&["disconnect", "claude-code"]);
    assert!(s.contains("removed"), "{s}");
    assert!(!plugins().contains("ehai@"));
    assert!(
        !run(&["config", "show"]).contains("claude-code"),
        "disabled in config"
    );

    run(&["connect", "claude-code"]);
    let s = run(&["uninstall", "--yes"]);
    assert!(s.contains("uninstalled"), "{s}");
    assert!(!plugins().contains("ehai@"), "harness disconnected");
    assert!(!ehai_home.exists(), "~/.ehai gone");
    assert_eq!(
        std::fs::read_to_string(&bashrc).unwrap(),
        "alias ll='ls -l'\n",
        "rc restored"
    );
}

#[test]
fn uninstall_without_yes_in_non_tty_does_nothing() {
    let env = Env::new();
    std::fs::write(env.home.path().join("config.toml"), "").unwrap();
    let out = env.cmd().arg("uninstall").output().unwrap();
    assert!(!out.status.success());
    assert!(stdout(&out).contains("Nothing removed"));
    assert!(env.home.path().join("config.toml").exists());
}

#[test]
fn disconnect_requires_an_id_or_all() {
    let env = Env::new();
    let out = env.cmd().arg("disconnect").output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--all"));
}
