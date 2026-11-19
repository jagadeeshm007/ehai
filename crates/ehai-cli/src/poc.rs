//! `ehai poc new|iterate|compose|promote|list|discard|compare|open|run|shell`: POC worktree management (spec §5.4).
//! Thin wrappers over `ehai-poc`; repo root is resolved from the current directory.

use anyhow::{Context, Result};
use ehai_core::Config;

use crate::ui::insights;

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
    let created = ehai_poc::poc_new(
        &root,
        home,
        feature,
        prompt,
        &config.env,
        config.sandbox.mode,
    )?;
    Ok(format!(
        "Created poc-{n} on branch {branch}\n\
         Worktree: {wt}\n\
         Port:     {port}\n\
         URL:      http://localhost:{port}\n\n\
         Design context:\n{ctx}{tips}",
        n = created.entry.n,
        branch = created.entry.branch,
        wt = created.entry.worktree.display(),
        port = created.entry.port,
        ctx = created.design_context,
        tips = run_tips(feature, created.entry.n),
    ))
}

/// Next-step footer shared by every command that leaves a POC ready to run.
fn run_tips(feature: &str, n: u32) -> String {
    let f = ehai_poc::feature_slug(feature);
    insights(&[
        format!("Start it:      ehai poc run {f} {n}"),
        format!("Any command:   ehai poc run {f} {n} -- <cmd>   (e.g. pnpm test)"),
        format!("Shell there:   ehai poc shell {f} {n}   (type `exit` to come back)"),
        "No `cd` needed: commands run in the worktree with its own ports loaded.".into(),
        "Note: main is untouched; nothing is merged until you `ehai poc promote`.".into(),
    ])
}

pub fn iterate_poc(
    home: &std::path::Path,
    config: &Config,
    feature: &str,
    parent: u32,
    feedback: &str,
) -> Result<String> {
    let root = repo_root()?;
    let created = ehai_poc::poc_iterate(
        &root,
        home,
        feature,
        parent,
        feedback,
        &config.env,
        config.sandbox.mode,
    )?;
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
    let last = entries.last().map(|e| (e.feature.clone(), e.n));
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
    if let Some((f, n)) = last {
        out += &insights(&[
            format!("{} POC(s). Run one: ehai poc run {f} {n}", entries.len()),
            format!("Compare: ehai poc compare {f} <n> <n>   ·   Shell: ehai poc shell {f} {n}"),
        ]);
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
    // Best-effort browser open; failure is non-fatal. Pick the opener per OS:
    // on many Linux distros `open` is `openvt`, which "succeeds" without opening anything.
    let mut cmd = if cfg!(target_os = "macos") {
        std::process::Command::new("open")
    } else if cfg!(windows) {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]);
        c
    } else {
        std::process::Command::new("xdg-open")
    };
    let _ = cmd.arg(&path).status();
    Ok(format!("Report: {}", path.display()))
}

pub fn compose_poc(
    home: &std::path::Path,
    config: &Config,
    feature: &str,
    description: &str,
    sources: Vec<(u32, String)>,
) -> Result<String> {
    let root = repo_root()?;
    let created = ehai_poc::poc_compose(
        &root,
        home,
        feature,
        description,
        sources,
        &config.env,
        config.sandbox.mode,
    )?;
    let parents = created
        .entry
        .parts
        .iter()
        .map(|(n, d)| format!("poc-{n}: {d}"))
        .collect::<Vec<_>>()
        .join(", ");
    Ok(format!(
        "Created poc-{n} (compose from {parents}) on branch {branch}\n\
         Worktree: {wt}\n\
         Port:     {port}",
        n = created.entry.n,
        branch = created.entry.branch,
        wt = created.entry.worktree.display(),
        port = created.entry.port,
    ))
}

pub fn promote_poc(
    home: &std::path::Path,
    config: &Config,
    feature: &str,
    n: u32,
) -> Result<String> {
    let (_, token_obj) = ehai_tracker::github::client_from_env()?;
    let repo_slug = config
        .tracker
        .github
        .repo
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("no repo configured; run `ehai tracker set` first"))?;
    let root = repo_root()?;
    let pr_url = crate::tracker::block_on(ehai_poc::poc_promote(
        &root,
        home,
        feature,
        n,
        &ehai_tracker::github::Client::new(&ehai_tracker::github::api_base(), &token_obj.value)?,
        repo_slug,
        "main",
        config.sandbox.mode,
    ))?;
    Ok(format!("PR created: {pr_url}"))
}

pub fn open_poc(config: &Config, feature: &str, n: u32) -> Result<String> {
    let _ = config;
    let root = repo_root()?;
    let info = ehai_poc::poc_open_info(&root, feature, n)?;
    Ok(format!(
        "Worktree: {wt}\nBranch:   {branch}\nPort:     {port}\nURL:      {url}\nDev cmd:  {cmd}{tips}",
        wt = info.worktree.display(),
        branch = info.branch,
        port = info.port,
        url = info.url,
        cmd = info.command,
        tips = run_tips(feature, n),
    ))
}

/// `ehai poc run`: run `cmd` (or the detected dev server) inside the POC
/// worktree with its `.env.ehai.local` ports applied — no `cd` needed.
/// Returns the command's exit code.
pub fn run_in_poc(feature: &str, n: u32, cmd: &[String]) -> Result<i32> {
    let root = repo_root()?;
    let info = ehai_poc::poc_open_info(&root, feature, n)?;
    let mut c = match cmd.split_first() {
        Some((prog, args)) => {
            let mut c = std::process::Command::new(prog);
            c.args(args);
            c
        }
        // The dev command is a shell string ("pnpm run dev").
        None if cfg!(windows) => {
            let mut c = std::process::Command::new("cmd");
            c.args(["/C", &info.command]);
            c
        }
        None => {
            let mut c = std::process::Command::new("sh");
            c.args(["-c", &info.command]);
            c
        }
    };
    let shown = if cmd.is_empty() {
        info.command.clone()
    } else {
        cmd.join(" ")
    };
    eprintln!("→ {shown}   (in {})", info.worktree.display());
    if cmd.is_empty() {
        eprintln!("  {}", info.url);
    }
    let status = c
        .current_dir(&info.worktree)
        .envs(ehai_env::read_env_overlay(&info.worktree))
        .status()
        .with_context(|| format!("could not start `{shown}` — is it installed?"))?;
    Ok(status.code().unwrap_or(1))
}

/// `ehai poc shell`: open the user's shell inside the POC worktree with its
/// ports loaded. `exit` returns to where they were.
pub fn shell_in_poc(feature: &str, n: u32) -> Result<i32> {
    let root = repo_root()?;
    let info = ehai_poc::poc_open_info(&root, feature, n)?;
    let shell = if cfg!(windows) {
        std::env::var("COMSPEC").unwrap_or_else(|_| "cmd".into())
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "sh".into())
    };
    eprintln!(
        "→ {shell} in {} — dev server: `{}`, `exit` to leave",
        info.worktree.display(),
        info.command
    );
    let status = std::process::Command::new(&shell)
        .current_dir(&info.worktree)
        .envs(ehai_env::read_env_overlay(&info.worktree))
        .env(
            "EHAI_POC",
            format!("{}/poc-{n}", ehai_poc::feature_slug(feature)),
        )
        .status()
        .with_context(|| format!("could not start shell `{shell}`"))?;
    Ok(status.code().unwrap_or(0))
}
