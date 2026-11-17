//! End-to-end `sync()` against synthetic fixtures for every stack the spec
//! names (§13): RN Expo, bare RN, Vite React, Next.js, Go API, FastAPI, and a
//! pnpm monorepo. Synthetic, no network: enough files for stack detection and
//! env discovery to fire, with `install_deps` off and a trivial smoke command
//! so "ready" doesn't depend on a real toolchain being installed.

use std::fs;
use std::path::Path;

use ehai_core::config::Env;

fn git_init(root: &Path) {
    let run = |args: &[&str]| {
        assert!(std::process::Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap()
            .status
            .success());
    };
    run(&["init", "-q"]);
    run(&["config", "user.email", "t@t"]);
    run(&["config", "user.name", "t"]);
}

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
}

/// A fresh clone of `source`, as `sync()` would receive for a new worktree:
/// same tracked files (incl. `.gitignore`), no local env.
fn fresh_worktree(source: &Path) -> tempfile::TempDir {
    let dst = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(source)
            .output()
            .unwrap()
            .status
            .success()
    };
    assert!(run(&["add", "-A"]));
    assert!(run(&["commit", "-q", "-m", "init"]));
    assert!(std::process::Command::new("git")
        .args([
            "worktree",
            "add",
            "-q",
            "--detach",
            dst.path().to_str().unwrap()
        ])
        .current_dir(source)
        .output()
        .unwrap()
        .status
        .success());
    dst
}

fn synced(source: &Path, app_dirs: &[&str]) -> (ehai_env::Report, tempfile::TempDir) {
    let worktree = fresh_worktree(source);
    let home = tempfile::tempdir().unwrap();
    let mut env = Env {
        install_deps: false,
        ..Default::default()
    };
    for dir in app_dirs {
        env.smoke.insert((*dir).into(), "exit 0".into());
    }
    let report = ehai_env::sync(source, worktree.path(), "fixture-test", home.path(), &env)
        .expect("sync should succeed on every fixture");
    assert!(report.is_ready(), "{report:?}");
    (report, worktree)
}

#[test]
fn expo_app_reaches_ready() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, ".gitignore", ".env*\n!.env.example\n");
    write(root, "package.json", r#"{"dependencies":{"expo":"1.0.0"}}"#);
    write(root, "pnpm-lock.yaml", "");
    write(root, ".env.example", "API_URL=\n");
    write(root, ".env", "API_URL=http://localhost\n");
    git_init(root);
    synced(root, &["."]);
}

#[test]
fn bare_react_native_reaches_ready() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, ".gitignore", ".env*\n");
    write(
        root,
        "package.json",
        r#"{"dependencies":{"react-native":"1.0.0"}}"#,
    );
    write(root, ".env", "A=1\n");
    git_init(root);
    synced(root, &["."]);
}

#[test]
fn vite_react_reaches_ready() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, ".gitignore", ".env*\n!.env.example\n");
    write(
        root,
        "package.json",
        r#"{"dependencies":{"react":"18.0.0"},"devDependencies":{"vite":"1.0.0"}}"#,
    );
    write(root, ".env.example", "VITE_API_URL=\n");
    write(root, ".env", "VITE_API_URL=http://localhost\n");
    git_init(root);
    let (report, _wt) = synced(root, &["."]);
    assert_eq!(
        report.ports.len(),
        1,
        "vite+react must be detected as one app"
    );
}

#[test]
fn nextjs_reaches_ready() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, ".gitignore", ".env*\n");
    write(root, "package.json", r#"{"dependencies":{"next":"1.0.0"}}"#);
    write(root, ".env.local", "NEXT_PUBLIC_API=http://localhost\n");
    git_init(root);
    synced(root, &["."]);
}

#[test]
fn go_api_reaches_ready() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, ".gitignore", ".env*\n");
    write(root, "go.mod", "module example.com/api\n\ngo 1.22\n");
    write(root, ".env", "DB_DSN=postgres://x\n");
    git_init(root);
    synced(root, &["."]);
}

#[test]
fn fastapi_reaches_ready() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, ".gitignore", ".env*\n.venv/\n");
    write(root, "pyproject.toml", "[project]\nname = \"api\"\n");
    write(root, "requirements.txt", "fastapi\n");
    write(root, ".env", "SECRET=x\n");
    git_init(root);
    synced(root, &["."]);
}

#[test]
fn pnpm_monorepo_reaches_ready_for_every_app() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, ".gitignore", ".env*\n!.env.example\n");
    write(root, "pnpm-workspace.yaml", "packages:\n  - 'apps/*'\n");
    write(
        root,
        "apps/web/package.json",
        r#"{"dependencies":{"react":"18.0.0"},"devDependencies":{"vite":"1.0.0"}}"#,
    );
    write(root, "apps/web/.env.example", "VITE_API_URL=\n");
    write(root, "apps/web/.env", "VITE_API_URL=http://localhost\n");
    write(
        root,
        "apps/api/go.mod",
        "module example.com/api\n\ngo 1.22\n",
    );
    write(root, "apps/api/.env", "DB_DSN=postgres://x\n");
    git_init(root);
    let (report, worktree) = synced(root, &["apps/web", "apps/api"]);
    assert_eq!(report.ports.len(), 2);

    // two apps -> ports are disambiguated by app name, not generic VITE_PORT/PORT.
    let overlay = fs::read_to_string(worktree.path().join(".env.ehai.local")).unwrap();
    assert!(overlay.contains("WEB_VITE_PORT="), "{overlay}");
    assert!(overlay.contains("API_PORT="), "{overlay}");
}

#[test]
fn materialized_env_files_are_never_staged() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, ".gitignore", ".env*\n");
    write(root, "go.mod", "module example.com/api\n\ngo 1.22\n");
    write(root, ".env", "SECRET=x\n");
    git_init(root);
    let (_report, worktree) = synced(root, &[]);

    let status = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(worktree.path())
        .output()
        .unwrap();
    let out = String::from_utf8_lossy(&status.stdout);
    assert!(
        !out.contains(".env"),
        "materialized env file must stay untracked: {out}"
    );
}

#[test]
fn install_deps_skips_gracefully_when_the_package_manager_is_missing() {
    // yarn is not expected to be on this machine's PATH, unlike pnpm/npm/go/uv/pip.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    write(root, ".gitignore", ".env*\n");
    write(root, "package.json", r#"{"dependencies":{"next":"1.0.0"}}"#);
    write(root, "yarn.lock", "");
    git_init(root);
    let worktree = fresh_worktree(root);
    let home = tempfile::tempdir().unwrap();
    let env = Env {
        install_deps: true,
        ..Default::default()
    };
    let report = ehai_env::sync(root, worktree.path(), "no-yarn-test", home.path(), &env).unwrap();
    assert!(report.installed.is_empty(), "{:?}", report.installed);
    assert_eq!(report.skipped.len(), 1);
    assert!(
        report.skipped[0].contains("yarn not found"),
        "{:?}",
        report.skipped
    );
}
