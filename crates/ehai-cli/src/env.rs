//! `ehai env sync|check|doctor`: make the worktree you're standing in runnable
//! (spec §6). `sync`/`check` default to the current directory; `path` picks
//! another one (e.g. a POC worktree).

use std::path::Path;

use anyhow::{Context, Result};
use ehai_core::Config;

pub fn sync(home: &Path, config: &Config, path: Option<&Path>) -> Result<String> {
    let cwd = std::env::current_dir()?;
    let target = path.map_or_else(|| cwd.clone(), Path::to_path_buf);
    let source = ehai_env::source_root(&target);
    let id = ehai_env::worktree_id(&target);
    let report = ehai_env::sync(&source, &target, &id, home, &config.env)
        .with_context(|| format!("syncing {}", target.display()))?;
    Ok(describe(&report))
}

pub fn check(path: Option<&Path>) -> Result<String> {
    let cwd = std::env::current_dir()?;
    let target = path.map_or_else(|| cwd.clone(), Path::to_path_buf);
    let missing = ehai_env::check(&target);
    Ok(if missing.is_empty() {
        "All keys present".into()
    } else {
        format!("Missing keys:\n  {}", missing.join("\n  "))
    })
}

/// Read-only: missing keys plus the configured smoke checks, without
/// materialising, allocating ports, or installing anything.
pub fn doctor(config: &Config, path: Option<&Path>) -> Result<String> {
    let cwd = std::env::current_dir()?;
    let target = path.map_or_else(|| cwd.clone(), Path::to_path_buf);
    let mut out = check(Some(&target))?;
    for s in ehai_env::smoke_check(&target, &config.env) {
        let mark = if s.outcome == ehai_env::SmokeOutcome::Ok {
            "✔"
        } else {
            "✖"
        };
        out += &format!("\n{mark} smoke ({}): {}", s.app, s.detail);
    }
    Ok(out)
}

fn describe(report: &ehai_env::Report) -> String {
    let mut out = String::new();
    out += &format!("{} file(s) materialised", report.materialized.len());
    if !report.materialized.is_empty() {
        out += &format!(": {}", report.materialized.join(", "));
    }
    if !report.missing_keys.is_empty() {
        out += &format!("\nMissing keys: {}", report.missing_keys.join(", "));
    }
    if !report.ports.is_empty() {
        out += &format!(
            "\nPorts: {}",
            report
                .ports
                .iter()
                .map(u16::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    for line in &report.installed {
        out += &format!("\n✔ {line}");
    }
    for line in &report.skipped {
        out += &format!("\n- {line}");
    }
    for s in &report.smoke {
        let mark = if s.outcome == ehai_env::SmokeOutcome::Ok {
            "✔"
        } else {
            "✖"
        };
        out += &format!("\n{mark} smoke ({}): {}", s.app, s.detail);
    }
    out
}
