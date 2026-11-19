//! Worktree environment provisioning (spec §6): a fresh git worktree lacks
//! gitignored config, installed dependencies and free ports, so it can't run.
//! This crate discovers and materialises what's missing, validates it against
//! `.env.example`, allocates ports, installs dependencies, and runs a health
//! check — in that order, each step logged into a [`Report`].

mod discover;
mod error;
mod materialize;
mod patterns;
mod ports;
mod smoke;
mod validate;

pub use error::{Error, Result};
pub use materialize::Materialized;
pub use ports::{read_env_overlay, PortRegistry};
pub use smoke::{SmokeOutcome, SmokeResult};
pub use validate::missing_keys;

use std::path::{Path, PathBuf};

use ehai_core::config::{Env, EnvStrategy};

/// The checkout gitignored config is copied *from*: the main worktree of the
/// same repo, found via the common `.git` dir. Falls back to `target` itself
/// (a harmless no-op: nothing to discover that isn't already there).
pub fn source_root(target: &Path) -> PathBuf {
    ehai_core::git::try_run(
        target,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    )
    .and_then(|common| {
        let common = PathBuf::from(common);
        if common.file_name()? == ".git" {
            common.parent().map(|p| p.to_path_buf())
        } else {
            None
        }
    })
    .unwrap_or_else(|| target.to_path_buf())
}

/// A stable id for the port registry: the target's own canonical path, so a
/// re-sync of the same worktree keeps the same ports.
pub fn worktree_id(target: &Path) -> String {
    target
        .canonicalize()
        .unwrap_or_else(|_| target.to_path_buf())
        .display()
        .to_string()
}

/// What happened for one worktree, step by step. Nothing here ever contains a
/// secret value — only file names and keys.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct Report {
    pub materialized: Vec<String>,
    pub missing_keys: Vec<String>,
    pub ports: Vec<u16>,
    pub installed: Vec<String>,
    pub skipped: Vec<String>,
    pub smoke: Vec<SmokeResult>,
}

impl Report {
    pub fn is_ready(&self) -> bool {
        self.smoke.iter().all(|s| s.outcome == SmokeOutcome::Ok)
    }
}

/// Discover and materialise gitignored config, validate it, allocate ports and
/// write them, install dependencies (unless disabled) and run the smoke check.
/// `worktree_id` scopes the port registry; reuse it across calls so a re-sync
/// keeps the same ports.
pub fn sync(
    source_root: &Path,
    worktree_root: &Path,
    worktree_id: &str,
    home: &Path,
    env: &Env,
) -> Result<Report> {
    let mut report = Report::default();

    let files = discover::discover(source_root, &env.include, &env.exclude);
    let strategy = match env.strategy {
        EnvStrategy::Copy => materialize::Strategy::Copy,
        EnvStrategy::Symlink => materialize::Strategy::Symlink,
    };
    let materialized = materialize::materialize(source_root, worktree_root, &files, strategy)?;
    report.materialized = materialized.into_iter().map(|m| m.rel).collect();

    report.missing_keys = validate::missing_keys_in(worktree_root);

    let apps = ehai_stack::RepoScan::scan(worktree_root).apps;
    let needed = apps.len().max(1);
    let registry = ports::PortRegistry::load(home)?;
    report.ports = registry.allocate(home, worktree_id, needed)?;

    let multi = apps.len() > 1;
    let mut vars: Vec<(String, u16)> = Vec::new();
    for (i, port) in report.ports.iter().enumerate() {
        let base = apps.get(i).map_or("PORT", |a| port_var(a.stack));
        let name = if multi {
            format!("{}_{base}", app_prefix(&apps[i].path))
        } else {
            base.to_string()
        };
        vars.push((name, *port));
    }
    ports::write_env_overlay(worktree_root, worktree_id, &vars)?;

    if env.install_deps {
        for app in &apps {
            match ehai_stack_deps_install(worktree_root, app) {
                InstallOutcome::NoManager => report
                    .skipped
                    .push(format!("{}: no known package manager", app.path)),
                InstallOutcome::ToolMissing(cmd) => report
                    .skipped
                    .push(format!("{}: {cmd} not found", app.path)),
                InstallOutcome::Ran(label) => {
                    report.installed.push(format!("{}: {label}", app.path))
                }
                InstallOutcome::Failed(label) => {
                    report.skipped.push(format!("{}: {label}", app.path))
                }
            }
        }
    }

    for (path, cmd) in &env.smoke {
        report.smoke.push(smoke::run(worktree_root, path, cmd));
    }

    Ok(report)
}

/// Read-only: run every configured smoke command without touching files,
/// ports or dependencies first. `ehai env doctor`.
pub fn smoke_check(worktree_root: &Path, env: &Env) -> Vec<SmokeResult> {
    env.smoke
        .iter()
        .map(|(path, cmd)| smoke::run(worktree_root, path, cmd))
        .collect()
}

/// Read-only: what [`sync`] would find missing, without writing anything.
/// `ehai env check`.
pub fn check(worktree_root: &Path) -> Vec<String> {
    if validate::existing_env_files(worktree_root).is_empty() {
        return vec!["no .env* files found yet — run `env sync` first".into()];
    }
    validate::missing_keys_in(worktree_root)
}

/// The env var a dev server reads its port from, by stack.
fn port_var(stack: ehai_stack::Stack) -> &'static str {
    use ehai_stack::Stack::*;
    match stack {
        Expo | ReactNative => "RCT_METRO_PORT",
        ViteReact | Cra | React => "VITE_PORT",
        Next | Go | Fastapi | Django | Flask | Python => "PORT",
    }
}

/// Uppercased last path segment, for disambiguating ports in a monorepo
/// (`apps/web` -> `WEB`).
fn app_prefix(path: &str) -> String {
    path.rsplit('/')
        .next()
        .unwrap_or(path)
        .to_ascii_uppercase()
        .replace(['-', '.'], "_")
}

enum InstallOutcome {
    NoManager,
    ToolMissing(String),
    Ran(String),
    Failed(String),
}

fn ehai_stack_deps_install(worktree_root: &Path, app: &ehai_stack::App) -> InstallOutcome {
    let dir = worktree_root.join(&app.path);
    let (cmd, args): (&str, &[&str]) = if dir.join("pnpm-lock.yaml").is_file() {
        ("pnpm", &["install"])
    } else if dir.join("yarn.lock").is_file() {
        ("yarn", &["install"])
    } else if dir.join("package-lock.json").is_file() {
        ("npm", &["install"])
    } else if dir.join("bun.lockb").is_file() {
        ("bun", &["install"])
    } else if dir.join("go.mod").is_file() {
        ("go", &["mod", "download"])
    } else if dir.join("uv.lock").is_file() {
        ("uv", &["sync"])
    } else if dir.join("poetry.lock").is_file() {
        ("poetry", &["install"])
    } else if dir.join("requirements.txt").is_file() {
        ("pip", &["install", "-r", "requirements.txt"])
    } else {
        return InstallOutcome::NoManager;
    };
    if which(cmd).is_none() {
        return InstallOutcome::ToolMissing(cmd.to_string());
    }
    let out = std::process::Command::new(cmd)
        .args(args)
        .current_dir(&dir)
        .output();
    let label = format!("{cmd} {}", args.join(" "));
    match out {
        Ok(o) if o.status.success() => InstallOutcome::Ran(label),
        Ok(o) => InstallOutcome::Failed(format!(
            "{label} failed: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        )),
        Err(e) => InstallOutcome::Failed(format!("{label} failed to start: {e}")),
    }
}

/// `EHAI_ENV_PATH` overrides `PATH` for this lookup only — lets a test force
/// "no package manager on PATH" deterministically (not by gambling on which
/// ones happen to be absent from whatever machine runs the suite) without
/// mutating the real `PATH` env var, which `cargo test`'s default
/// multi-threaded, single-process execution would race with every other
/// test in the same binary.
pub fn which(cmd: &str) -> Option<std::path::PathBuf> {
    let exe = if cfg!(windows) {
        format!("{cmd}.exe")
    } else {
        cmd.to_string()
    };
    let path = std::env::var_os("EHAI_ENV_PATH").or_else(|| std::env::var_os("PATH"))?;
    std::env::split_paths(&path).find_map(|dir| {
        let p = dir.join(&exe);
        p.is_file().then_some(p)
    })
}
