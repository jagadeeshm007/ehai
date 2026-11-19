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
        // never reach real GitHub from tests
        c.env("EHAI_NO_GH", "1")
            .env_remove("GITHUB_TOKEN")
            .env_remove("GH_TOKEN")
            .env("EHAI_GITHUB_API", "http://127.0.0.1:9")
            .env("EHAI_RELEASES_API", "http://127.0.0.1:9")
            // never touch the developer's real Claude desktop config
            .env(
                "EHAI_CLAUDE_DESKTOP_CONFIG",
                self.home
                    .path()
                    .join("no-claude-desktop/claude_desktop_config.json"),
            );
        c.env("EHAI_HOME", self.home.path())
            // `ehai init`'s auto-install step (and `ehai self install`/
            // `uninstall`) edit $HOME's shell rc files — sandbox it here, by
            // default, for every test, rather than per-test. A real machine's
            // .bashrc getting an `export PATH=...` line pointing at a deleted
            // tempdir is exactly the kind of thing a missing default like
            // this causes.
            .env("HOME", self.home.path())
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
    let want = format!("ehai {} (", env!("CARGO_PKG_VERSION"));
    assert!(s.starts_with(&want), "{s}");
    assert!(s.contains(env!("TARGET_TRIPLE_FOR_TESTS")), "{s}");
}

#[test]
fn help_is_stable() {
    let out = Env::new().cmd().arg("--help").output().unwrap();
    // Normalize "ehai.exe" → "ehai" so the snapshot is identical on all OSes.
    let text = stdout(&out).replace("ehai.exe", "ehai");
    insta::assert_snapshot!(text);
}

#[test]
fn doctor_passes_in_sane_env() {
    let env = Env::new();
    let out = env.cmd().arg("doctor").output().unwrap();
    let s = stdout(&out);
    assert!(out.status.success(), "{s}");
    assert!(s.contains("✔ Git"), "{s}");
    assert!(s.contains("✔ Role") && s.contains("dev"), "{s}");
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

/// `tracker set/status/schema`, doctor and `init --tracker-project` against a fake
/// GitHub (no network). Commands run on a blocking thread so the fake keeps serving.
#[tokio::test(flavor = "multi_thread")]
async fn tracker_commands_against_fake_github() {
    let server = wiremock::MockServer::start().await;
    let fake = ehai_tracker::fake::mount(&server).await;
    let env = std::sync::Arc::new(Env::new());
    let env_home = env.home.path().to_path_buf();
    let api = server.uri();
    let run = move |args: &'static [&'static str]| {
        let env = env.clone();
        let api = api.clone();
        async move {
            tokio::task::spawn_blocking(move || {
                let out = env
                    .cmd()
                    .env("EHAI_GITHUB_API", &api)
                    .env("GITHUB_TOKEN", "test-token")
                    .args(args)
                    .output()
                    .unwrap();
                (
                    out.status.success(),
                    stdout(&out) + &String::from_utf8_lossy(&out.stderr),
                )
            })
            .await
            .unwrap()
        }
    };

    let (ok, out) = run(&["tracker", "status"]).await;
    assert!(
        !ok && out.contains("tracker set"),
        "unconfigured status explains what to do: {out}"
    );

    let (ok, out) = run(&[
        "tracker",
        "set",
        "--owner",
        "octo",
        "--project",
        "1",
        "--repo",
        "octo/app",
        "--yes",
    ])
    .await;
    assert!(ok, "{out}");
    assert!(out.contains("octo (via $GITHUB_TOKEN)"), "{out}");
    assert!(
        out.contains("created Priority, Size, Work type, Iteration"),
        "{out}"
    );
    let fields = fake.field_names();
    for f in ["Status", "Priority", "Size", "Work type", "Iteration"] {
        assert!(fields.iter().any(|x| x == f), "{f} in {fields:?}");
    }

    let (ok, out) = run(&["tracker", "schema"]).await;
    assert!(ok && out.contains("already set up"), "{out}");
    let (ok, out) = run(&["tracker", "status"]).await;
    assert!(
        ok && out.contains("octo #1 (0 items), issues in octo/app"),
        "{out}"
    );
    let (ok, out) = run(&["doctor"]).await;
    assert!(
        ok && out.contains("✔ Tracker (octo #1) — reachable, 0 items"),
        "{out}"
    );

    let (ok, out) = run(&[
        "init",
        "--yes",
        "--harness",
        "none",
        "--no-repo",
        "--tracker-project",
        "1",
    ])
    .await;
    assert!(ok && out.contains("GitHub project — octo #1"), "{out}");

    // a project that doesn't exist fails *before* anything is saved
    let before = std::fs::read_to_string(env_home.join("config.toml")).unwrap();
    let (ok, out) = run(&[
        "tracker",
        "set",
        "--owner",
        "octo",
        "--project",
        "99",
        "--no-repo",
        "--yes",
    ])
    .await;
    assert!(!ok && out.contains("not found"), "{out}");
    assert_eq!(
        std::fs::read_to_string(env_home.join("config.toml")).unwrap(),
        before,
        "config untouched"
    );

    let (ok, out) = run(&["tracker", "set", "linear"]).await;
    assert!(!ok && out.contains("isn't available yet"), "{out}");
}

#[cfg(unix)]
#[test]
fn closed_stdout_pipe_exits_quietly() {
    let env = Env::new();
    // `ehai config schema | head -1`: the reader exits after one line
    let mut child = std::process::Command::new(assert_cmd::cargo::cargo_bin("ehai"))
        .args(["config", "schema"])
        .env("EHAI_HOME", env.home.path())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take()); // close the read end before ehai writes
    let out = child.wait_with_output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(!err.contains("panicked"), "{err}");
    assert!(out.status.success(), "{err}");
}

/// `poc run` from the repo root executes inside the POC worktree with its
/// allocated port, and passes the command's exit code through.
#[cfg(unix)]
#[test]
fn poc_run_executes_in_worktree_with_ports_and_exit_code() {
    let env = Env::new();
    let git = |args: &[&str]| {
        assert!(std::process::Command::new("git")
            .args(args)
            .current_dir(env.cwd.path())
            .status()
            .unwrap()
            .success())
    };
    git(&["init", "-q", "-b", "main"]);
    std::fs::write(env.cwd.path().join("README.md"), "demo\n").unwrap();
    git(&["add", "-A"]);
    git(&[
        "-c",
        "user.email=t@t",
        "-c",
        "user.name=t",
        "commit",
        "-qm",
        "init",
    ]);

    let out = env
        .cmd()
        .args(["poc", "new", "checkout", "demo"])
        .output()
        .unwrap();
    let s = stdout(&out);
    assert!(out.status.success(), "{s}");
    assert!(
        s.contains("★ Insights") && s.contains("ehai poc run checkout 1"),
        "{s}"
    );

    let out = env
        .cmd()
        .args(["poc", "run", "checkout", "1", "--", "sh", "-c"])
        .arg("pwd; echo PORT=$PORT; exit 7")
        .output()
        .unwrap();
    let s = stdout(&out);
    assert_eq!(out.status.code(), Some(7), "{s}");
    assert!(s.contains("checkout/poc-1"), "ran in the worktree: {s}");
    assert!(s.contains("PORT=2"), "port overlay loaded: {s}");
}
