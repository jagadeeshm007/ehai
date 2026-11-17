//! `ehai poc new|iterate|list|discard|compare|open`: POC worktree management (spec §5.4).
//! Thin wrappers over `ehai-poc`; repo root is resolved from the current directory.

use anyhow::{Context, Result};
use ehai_core::Config;

fn repo_root() -> Result<std::path::PathBuf> {
    let cwd = std::env::current_dir()?;
    ehai_core::git::toplevel(&cwd).context("not inside a git repository")
}

pub fn new_poc(
    home: &std::path::Path,
    config: &Config,
    feature: &str,
    prompt: &str,
) -> Result<String> {
    let root = repo_root()?;
    let created = ehai_poc::poc_new(&root, home, feature, prompt, &config.env)?;
    Ok(format!(
        "Created poc-{n} on branch {branch}\n\
         Worktree: {wt}\n\
         Port:     {port}\n\
         URL:      http://localhost:{port}\n\n\
         Design context:\n{ctx}",
        n = created.entry.n,
        branch = created.entry.branch,
        wt = created.entry.worktree.display(),
        port = created.entry.port,
        ctx = created.design_context,
    ))
}

pub fn iterate_poc(
    home: &std::path::Path,
    config: &Config,
    feature: &str,
    parent: u32,
    feedback: &str,
) -> Result<String> {
    let root = repo_root()?;
    let created = ehai_poc::poc_iterate(&root, home, feature, parent, feedback, &config.env)?;
    Ok(format!(
        "Created poc-{n} (↑ from poc-{parent}) on branch {branch}\n\
         Worktree: {wt}\n\
         Port:     {port}",
        n = created.entry.n,
        branch = created.entry.branch,
        wt = created.entry.worktree.display(),
        port = created.entry.port,
    ))
}

pub fn list_pocs(config: &Config, feature: Option<&str>) -> Result<String> {
    let _ = config; // future: role gate
    let root = repo_root()?;
    let entries = ehai_poc::poc_list(&root, feature)?;
    if entries.is_empty() {
        return Ok("No POCs found.".into());
    }
    let mut out = String::new();
    for e in &entries {
        let parent = e
            .parent
            .map(|p| format!(" (↑ poc-{p})"))
            .unwrap_or_default();
        out += &format!(
            "poc-{n} [{feature}]{parent}  {status:?}  port {port}  {date}\n  {prompt}\n",
            n = e.n,
            feature = e.feature,
            status = e.status,
            port = e.port,
            date = e.created_at,
            prompt = e.prompt,
        );
    }
    Ok(out)
}

pub fn discard_poc(
    home: &std::path::Path,
    config: &Config,
    feature: &str,
    n: u32,
) -> Result<String> {
    let _ = config;
    let root = repo_root()?;
    ehai_poc::poc_discard(&root, home, feature, n)?;
    let archive = format!("ehai-archive/{}/poc-{n}", ehai_poc::feature_slug(feature));
    Ok(format!("Archived as {archive}"))
}

pub fn compare_pocs(
    home: &std::path::Path,
    config: &Config,
    feature: &str,
    ns: &[u32],
) -> Result<String> {
    let _ = config;
    let root = repo_root()?;
    let path = ehai_poc::poc_compare(&root, home, feature, ns)?;
    // Best-effort browser open; failure is non-fatal
    let _ = std::process::Command::new("open")
        .arg(&path)
        .status()
        .or_else(|_| std::process::Command::new("xdg-open").arg(&path).status());
    Ok(format!("Report: {}", path.display()))
}

pub fn open_poc(config: &Config, feature: &str, n: u32) -> Result<String> {
    let _ = config;
    let root = repo_root()?;
    let info = ehai_poc::poc_open_info(&root, feature, n)?;
    Ok(format!(
        "Worktree: {wt}\nBranch:   {branch}\nPort:     {port}\nURL:      {url}\n\nRun inside the worktree:\n  {cmd}",
        wt = info.worktree.display(),
        branch = info.branch,
        port = info.port,
        url = info.url,
        cmd = info.command,
    ))
}
