//! `env_sync`/`env_check` through the MCP tools: a fresh worktree reaches
//! "ready" and a key missing from `.env` (but present in `.env.example`) is
//! reported by name only.

use std::fs;
use std::path::Path;

use ehai_core::Config;
use ehai_mcp::{Context, EhaiServer, Github};
use rmcp::handler::server::wrapper::Parameters;
use serde_json::{json, Map, Value};

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
}

fn git(dir: &Path, args: &[&str]) -> bool {
    std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
        .status
        .success()
}

/// A source repo with a go API app, plus a fresh worktree cloned from it (as
/// `sync()` would receive for a new POC/worktree).
fn source_and_worktree() -> (tempfile::TempDir, tempfile::TempDir) {
    let source = tempfile::tempdir().unwrap();
    let root = source.path();
    write(root, ".gitignore", ".env*\n!.env.example\n");
    write(root, "go.mod", "module example.com/api\n\ngo 1.22\n");
    write(root, ".env.example", "DB_DSN=\nAPI_KEY=\n");
    write(root, ".env", "DB_DSN=postgres://x\n");
    assert!(git(root, &["init", "-q"]));
    assert!(git(root, &["config", "user.email", "t@t"]));
    assert!(git(root, &["config", "user.name", "t"]));
    assert!(git(root, &["add", "-A"]));
    assert!(git(root, &["commit", "-q", "-m", "init"]));

    let worktree = tempfile::tempdir().unwrap();
    let path = worktree.path();
    std::fs::remove_dir(path).unwrap(); // `git worktree add` creates it
    assert!(git(
        root,
        &["worktree", "add", "-q", "--detach", path.to_str().unwrap()]
    ));
    (source, worktree)
}

fn server(home: &Path) -> EhaiServer {
    EhaiServer::new(Context {
        config: Config::default(),
        home: home.into(),
        cwd: None,
        github: Github::Fixed {
            api: String::new(),
            token: String::new(),
        },
    })
}

fn p<T: serde::de::DeserializeOwned>(v: Value) -> Parameters<T> {
    Parameters(serde_json::from_value(v).unwrap())
}

fn val(r: Result<rmcp::Json<Map<String, Value>>, String>) -> Value {
    Value::Object(r.expect("tool ok").0)
}

#[tokio::test]
async fn env_sync_then_env_check_through_the_mcp_tools() {
    let (_source, worktree) = source_and_worktree();
    let home = tempfile::tempdir().unwrap();
    let s = server(home.path());

    let sync = val(s
        .env_sync(p(json!({ "repo_path": worktree.path().to_str().unwrap() })))
        .await);
    assert_eq!(sync["status"], "ok", "{sync:?}");
    let report = &sync["report"];
    assert_eq!(report["materialized"], json!([".env"]));
    assert_eq!(report["missing_keys"], json!(["API_KEY"]));
    assert_eq!(report["ports"].as_array().unwrap().len(), 1);

    let check = val(s
        .env_check(p(json!({ "repo_path": worktree.path().to_str().unwrap() })))
        .await);
    assert_eq!(check["missing_keys"], json!(["API_KEY"]));
}

#[tokio::test]
async fn env_sync_requires_a_repo_path_outside_a_repo() {
    let home = tempfile::tempdir().unwrap();
    let s = server(home.path());
    let err = s.env_sync(p(json!({}))).await.err().expect("tool error");
    assert!(err.contains("repo_path"), "{err}");
}
