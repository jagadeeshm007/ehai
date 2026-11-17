//! POC engine: isolated git worktrees with design context for AI-driven UI/UX prototyping.
//!
//! Each POC lives on its own branch (`ehai/poc/<feature>/poc-<n>`) in a worktree outside
//! the repo (`~/.ehai/worktrees/<repo-id>/<feature>/poc-<n>`), provisioned by `ehai-env`.
//! The manifest at `.ehai/pocs/<feature>/manifest.json` tracks every POC's lineage.
//!
//! Code generation is AI-driven: `poc_new` returns design context; Claude writes the code.
//! ponytail: Chrome CLI screenshot over chromiumoxide — same Chrome, 100x fewer deps; upgrade when programmatic navigation is needed.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use ehai_core::{Error, Result};
use serde::{Deserialize, Serialize};

// ── Types ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PocStatus {
    #[default]
    Draft,
    Liked,
    Rejected,
    Promoted,
    Archived,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PocEntry {
    pub n: u32,
    pub feature: String,
    pub branch: String,
    pub worktree: PathBuf,
    pub port: u16,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<u32>,
    /// For composed POCs: (parent_n, description of what was taken).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<(u32, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feedback: Option<String>,
    pub status: PocStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub screenshot_paths: Vec<PathBuf>,
    pub created_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PocManifest {
    pub feature: String,
    pub entries: Vec<PocEntry>,
}

impl PocManifest {
    fn next_n(&self) -> u32 {
        self.entries.iter().map(|e| e.n).max().unwrap_or(0) + 1
    }

    fn get(&self, n: u32) -> Option<&PocEntry> {
        self.entries.iter().find(|e| e.n == n)
    }

    fn get_mut(&mut self, n: u32) -> Option<&mut PocEntry> {
        self.entries.iter_mut().find(|e| e.n == n)
    }
}

/// Result returned by `poc_new` / `poc_iterate` — everything Claude needs to write the POC.
#[derive(Debug)]
pub struct PocCreated {
    pub entry: PocEntry,
    pub design_context: String,
}

/// Info returned by `poc_open_info` — what to run to preview a POC.
#[derive(Debug)]
pub struct PocOpenInfo {
    pub worktree: PathBuf,
    pub port: u16,
    pub branch: String,
    /// Suggested dev-server command to run inside the worktree.
    pub command: String,
    pub url: String,
}

// ── Path helpers ───────────────────────────────────────────────────────────

/// 16-hex-char stable id for a repo root (used in the worktree path).
pub fn repo_id(root: &Path) -> String {
    let mut h = DefaultHasher::new();
    root.canonicalize()
        .unwrap_or_else(|_| root.to_path_buf())
        .hash(&mut h);
    format!("{:016x}", h.finish())
}

/// Slugify a feature name: lowercase, spaces → `-`, strip everything else.
pub fn feature_slug(name: &str) -> String {
    let s: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    // Collapse multiple dashes and strip leading/trailing dashes
    s.split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

fn poc_branch(feature: &str, n: u32) -> String {
    format!("ehai/poc/{feature}/poc-{n}")
}

fn archive_branch(feature: &str, n: u32) -> String {
    format!("ehai-archive/{feature}/poc-{n}")
}

fn worktree_path(home: &Path, root: &Path, feature: &str, n: u32) -> PathBuf {
    home.join("worktrees")
        .join(repo_id(root))
        .join(feature)
        .join(format!("poc-{n}"))
}

pub fn manifest_path(root: &Path, feature: &str) -> PathBuf {
    ehai_core::paths::repo_dir(root)
        .join("pocs")
        .join(feature)
        .join("manifest.json")
}

// ── Manifest I/O ───────────────────────────────────────────────────────────

fn load_manifest(root: &Path, feature: &str) -> Result<PocManifest> {
    let path = manifest_path(root, feature);
    if !path.exists() {
        return Ok(PocManifest {
            feature: feature.to_string(),
            entries: vec![],
        });
    }
    let bytes = std::fs::read(&path).map_err(Error::io(&path))?;
    serde_json::from_slice::<PocManifest>(&bytes)
        .map_err(|e| Error::Invalid(format!("manifest parse error: {e}")))
}

fn save_manifest(root: &Path, manifest: &PocManifest) -> Result<()> {
    let path = manifest_path(root, &manifest.feature);
    let dir = path.parent().unwrap();
    std::fs::create_dir_all(dir).map_err(Error::io(dir))?;
    ensure_pocs_gitignored(root)?;
    let json = serde_json::to_vec_pretty(manifest).map_err(|e| Error::Invalid(e.to_string()))?;
    ehai_core::migrate::write_atomic(&path, &json)
}

/// Add `.ehai/pocs/` to `.ehai/.gitignore` (idempotent).
fn ensure_pocs_gitignored(root: &Path) -> Result<()> {
    let gi = ehai_core::paths::repo_dir(root).join(".gitignore");
    let content = if gi.exists() {
        std::fs::read_to_string(&gi).map_err(Error::io(&gi))?
    } else {
        String::new()
    };
    if content.contains("pocs/") {
        return Ok(());
    }
    std::fs::write(&gi, format!("{content}pocs/\n")).map_err(Error::io(&gi))
}

// ── Core operations ────────────────────────────────────────────────────────

fn sandbox_check(worktree: &Path, home: &Path, mode: ehai_core::config::SandboxMode) -> Result<()> {
    use ehai_core::config::SandboxMode;
    if mode == SandboxMode::Restricted && !worktree.starts_with(home.join("worktrees")) {
        return Err(Error::Invalid(format!(
            "sandbox: worktree {} is outside {}/worktrees; blocked in Restricted mode",
            worktree.display(),
            home.display()
        )));
    }
    Ok(())
}

/// Create a new POC: branch + worktree + env provisioning + design context.
pub fn poc_new(
    root: &Path,
    home: &Path,
    feature: &str,
    prompt: &str,
    env_config: &ehai_core::config::Env,
    sandbox_mode: ehai_core::config::SandboxMode,
) -> Result<PocCreated> {
    let feature = feature_slug(feature);
    create_poc(root, home, &feature, prompt, None, env_config, sandbox_mode)
}

/// Create a new iteration of an existing POC.
pub fn poc_iterate(
    root: &Path,
    home: &Path,
    feature: &str,
    parent_n: u32,
    feedback: &str,
    env_config: &ehai_core::config::Env,
    sandbox_mode: ehai_core::config::SandboxMode,
) -> Result<PocCreated> {
    let feature = feature_slug(feature);
    // Verify parent exists
    let manifest = load_manifest(root, &feature)?;
    if manifest.get(parent_n).is_none() {
        return Err(Error::Invalid(format!(
            "poc-{parent_n} not found in feature '{feature}'"
        )));
    }
    create_poc(
        root,
        home,
        &feature,
        feedback,
        Some(parent_n),
        env_config,
        sandbox_mode,
    )
}

fn create_poc(
    root: &Path,
    home: &Path,
    feature: &str,
    prompt: &str,
    parent: Option<u32>,
    env_config: &ehai_core::config::Env,
    sandbox_mode: ehai_core::config::SandboxMode,
) -> Result<PocCreated> {
    let mut manifest = load_manifest(root, feature)?;
    let n = manifest.next_n();
    let branch = poc_branch(feature, n);
    let wt_path = worktree_path(home, root, feature, n);
    sandbox_check(&wt_path, home, sandbox_mode)?;

    // Create parent directories
    if let Some(parent_dir) = wt_path.parent() {
        std::fs::create_dir_all(parent_dir).map_err(Error::io(parent_dir))?;
    }

    // Create branch + worktree in one git command
    ehai_core::git::run(
        root,
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            wt_path.to_str().unwrap_or_default(),
        ],
    )?;
    tracing::info!(branch = %branch, path = %wt_path.display(), "poc worktree created");

    // Provision the worktree (env files, ports, deps, smoke)
    let source = ehai_env::source_root(&wt_path);
    let wt_id = ehai_env::worktree_id(&wt_path);
    let env_report = ehai_env::sync(&source, &wt_path, &wt_id, home, env_config)
        .map_err(|e| Error::Invalid(format!("env provisioning failed: {e}")))?;
    let port = env_report.ports.first().copied().unwrap_or(3000);
    tracing::info!(port = port, "poc env provisioned");

    // Extract design profile from the SOURCE (main) worktree
    let design_profile = ehai_stack::load_or_extract(&source);
    let design_context = design_profile.to_context_text();

    let entry = PocEntry {
        n,
        feature: feature.to_string(),
        branch: branch.clone(),
        worktree: wt_path,
        port,
        prompt: prompt.to_string(),
        parent,
        parts: vec![],
        feedback: None,
        status: PocStatus::Draft,
        screenshot_paths: vec![],
        created_at: ehai_core::date::today_utc(),
    };

    manifest.entries.push(entry.clone());
    save_manifest(root, &manifest)?;

    Ok(PocCreated {
        entry,
        design_context,
    })
}

/// Synthesise a new POC from parts of two or more existing POCs.
///
/// Each `source` is `(parent_n, description_of_what_to_take)`. Claude reads the
/// referenced POCs' diffs and resynthesises the described parts; ehai just records
/// lineage in `PocEntry::parts`.
pub fn poc_compose(
    root: &Path,
    home: &Path,
    feature: &str,
    description: &str,
    sources: Vec<(u32, String)>,
    env_config: &ehai_core::config::Env,
    sandbox_mode: ehai_core::config::SandboxMode,
) -> Result<PocCreated> {
    let feature = feature_slug(feature);
    // Verify every source n exists
    let manifest = load_manifest(root, &feature)?;
    for (n, _) in &sources {
        if manifest.get(*n).is_none() {
            return Err(Error::Invalid(format!(
                "poc-{n} not found in feature '{feature}'"
            )));
        }
    }
    let mut created = create_poc(
        root,
        home,
        &feature,
        description,
        None,
        env_config,
        sandbox_mode,
    )?;
    // Patch lineage and re-save
    created.entry.parts = sources;
    let mut manifest = load_manifest(root, &feature)?;
    if let Some(e) = manifest.get_mut(created.entry.n) {
        e.parts.clone_from(&created.entry.parts);
    }
    save_manifest(root, &manifest)?;
    Ok(created)
}

/// Promote a POC to main by opening a GitHub pull request.
///
/// Updates the manifest entry to `Promoted` and returns the PR HTML URL.
#[allow(clippy::too_many_arguments)]
pub async fn poc_promote(
    root: &Path,
    home: &Path,
    feature: &str,
    n: u32,
    client: &ehai_tracker::github::Client,
    repo_slug: &str,
    default_branch: &str,
    sandbox_mode: ehai_core::config::SandboxMode,
) -> Result<String> {
    let feature = feature_slug(feature);
    let mut manifest = load_manifest(root, &feature)?;
    let entry = manifest
        .get(n)
        .ok_or_else(|| Error::Invalid(format!("poc-{n} not found in '{feature}'")))?;
    if matches!(entry.status, PocStatus::Promoted) {
        return Err(Error::Invalid(format!("poc-{n} is already promoted")));
    }
    if matches!(entry.status, PocStatus::Archived) {
        return Err(Error::Invalid(format!(
            "poc-{n} is archived; restore it first"
        )));
    }
    sandbox_check(&entry.worktree, home, sandbox_mode)?;

    let branch = entry.branch.clone();
    let prompt_preview: String = entry.prompt.chars().take(100).collect();
    let parent_line = entry
        .parent
        .map(|p| format!("\n\nParent: poc-{p}"))
        .unwrap_or_default();
    let pr_title = format!("feat({feature}): poc-{n}");
    let pr_body = format!(
        "## POC `{feature}/poc-{n}`\n\n\
         **Prompt:** {prompt_preview}{parent_line}\n\n\
         _Auto-generated by ehai — review before merging._"
    );

    let pr_url = client
        .create_pull_request(repo_slug, &branch, default_branch, &pr_title, &pr_body)
        .await
        .map_err(|e| Error::Invalid(e.to_string()))?;

    if let Some(e) = manifest.get_mut(n) {
        e.status = PocStatus::Promoted;
    }
    save_manifest(root, &manifest)?;
    tracing::info!(n, feature = %feature, pr_url = %pr_url, "poc promoted");
    Ok(pr_url)
}

/// Archive a POC branch and remove its worktree.
pub fn poc_discard(root: &Path, home: &Path, feature: &str, n: u32) -> Result<()> {
    let feature = feature_slug(feature);
    let mut manifest = load_manifest(root, &feature)?;
    let entry = manifest
        .get(n)
        .ok_or_else(|| Error::Invalid(format!("poc-{n} not found in '{feature}'")))?;
    let branch = entry.branch.clone();
    let wt_path = entry.worktree.clone();
    let archive = archive_branch(&feature, n);

    // Create archive branch (idempotent — ignore if it already exists)
    let _ = ehai_core::git::run(root, &["branch", &archive, &branch]);

    // Remove worktree (--force handles uncommitted changes)
    if wt_path.exists() {
        if let Err(e) = ehai_core::git::run(
            root,
            &[
                "worktree",
                "remove",
                "--force",
                wt_path.to_str().unwrap_or_default(),
            ],
        ) {
            // If git doesn't know about this worktree anymore, clean up manually
            tracing::warn!(error = %e, "git worktree remove failed; cleaning up manually");
            let _ = std::fs::remove_dir_all(&wt_path);
        }
        // Also clean up the worktrees parent dir if it's now empty
        if let Some(parent) = wt_path.parent() {
            let _ = std::fs::remove_dir(parent); // fails silently if non-empty
        }
    }

    // Delete the POC branch (it's no longer checked out)
    let _ = ehai_core::git::run(root, &["branch", "-D", &branch]);

    // Remove empty worktree dirs under ~/.ehai/worktrees/<repo-id>/<feature>/
    let wt_feature_dir = home.join("worktrees").join(repo_id(root)).join(&feature);
    let _ = std::fs::remove_dir(&wt_feature_dir); // fails silently if non-empty

    // Update manifest
    if let Some(e) = manifest.get_mut(n) {
        e.status = PocStatus::Archived;
    }
    save_manifest(root, &manifest)?;
    tracing::info!(n = n, feature = %feature, archive_branch = %archive, "poc discarded");
    Ok(())
}

/// List POCs for one feature or all features.
pub fn poc_list(root: &Path, feature: Option<&str>) -> Result<Vec<PocEntry>> {
    let pocs_dir = ehai_core::paths::repo_dir(root).join("pocs");
    if !pocs_dir.exists() {
        return Ok(vec![]);
    }
    if let Some(f) = feature {
        let slug = feature_slug(f);
        return Ok(load_manifest(root, &slug)?.entries);
    }
    // All features
    let mut all: Vec<PocEntry> = Vec::new();
    let Ok(entries) = std::fs::read_dir(&pocs_dir) else {
        return Ok(vec![]);
    };
    let mut features: Vec<String> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| e.file_name().into_string().ok())
        .collect();
    features.sort();
    for f in features {
        let m = load_manifest(root, &f)?;
        all.extend(m.entries);
    }
    Ok(all)
}

/// Return info needed to start the dev server for a POC.
pub fn poc_open_info(root: &Path, feature: &str, n: u32) -> Result<PocOpenInfo> {
    let feature = feature_slug(feature);
    let manifest = load_manifest(root, &feature)?;
    let entry = manifest
        .get(n)
        .ok_or_else(|| Error::Invalid(format!("poc-{n} not found in '{feature}'")))?;
    let scan = ehai_stack::RepoScan::scan(&entry.worktree);
    let command = dev_server_command(&scan);
    Ok(PocOpenInfo {
        worktree: entry.worktree.clone(),
        port: entry.port,
        branch: entry.branch.clone(),
        command,
        url: format!("http://localhost:{}", entry.port),
    })
}

/// Pick a reasonable dev-server command from the detected stack.
fn dev_server_command(scan: &ehai_stack::RepoScan) -> String {
    use ehai_stack::Stack;
    let Some(app) = scan.apps.first() else {
        return "pnpm dev".to_string();
    };
    let pm = app.package_manager.as_deref().unwrap_or("npm");
    match app.stack {
        Stack::Expo => format!("{pm} run start"),
        Stack::ReactNative => format!("{pm} run android"),
        Stack::Next | Stack::ViteReact | Stack::Cra | Stack::React => {
            format!("{pm} run dev")
        }
        Stack::Go => "go run ./...".to_string(),
        Stack::Fastapi => "uvicorn main:app --reload".to_string(),
        Stack::Django => "python manage.py runserver".to_string(),
        Stack::Flask => "flask run".to_string(),
        Stack::Python => "python main.py".to_string(),
    }
}

/// Generate a side-by-side HTML comparison report.
/// Returns the path of the written HTML file at `~/.ehai/reports/compare-<feature>.html`.
pub fn poc_compare(root: &Path, home: &Path, feature: &str, ns: &[u32]) -> Result<PathBuf> {
    let feature = feature_slug(feature);
    let manifest = load_manifest(root, &feature)?;
    let entries: Vec<&PocEntry> = ns.iter().filter_map(|n| manifest.get(*n)).collect();
    if entries.is_empty() {
        return Err(Error::Invalid("no matching POCs found".to_string()));
    }
    let cards: String = entries
        .iter()
        .map(|e| {
            let screenshot = e
                .screenshot_paths
                .first()
                .map(|p| format!(r#"<img src="{}" alt="poc-{}">"#, p.display(), e.n))
                .unwrap_or_else(|| {
                    r#"<div class="no-screenshot">No screenshot yet — run <code>ehai poc open</code> to capture one</div>"#.to_string()
                });
            let parent = e
                .parent
                .map(|p| format!(r#" <span class="parent">↑ from poc-{p}</span>"#))
                .unwrap_or_default();
            format!(
                r#"<div class="poc">
  <h3>poc-{n}{parent}</h3>
  <p class="meta"><strong>Prompt:</strong> {prompt}</p>
  {screenshot}
</div>"#,
                n = e.n,
                prompt = html_escape(&e.prompt),
            )
        })
        .collect::<Vec<_>>()
        .join("\n    ");
    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<title>POC Comparison: {feature}</title>
<style>
body {{font-family:system-ui,sans-serif;margin:0;background:#f9fafb}}
.header {{padding:1rem 2rem;background:#fff;border-bottom:1px solid #e5e7eb}}
.header h1 {{margin:0;font-size:1.25rem}}
.grid {{display:flex;gap:1.5rem;padding:1.5rem 2rem;overflow-x:auto}}
.poc {{flex:0 0 400px;background:#fff;border:1px solid #e5e7eb;border-radius:8px;padding:1rem}}
.poc h3 {{margin:0 0 .5rem;font-size:1rem}}
.parent {{color:#9ca3af;font-weight:400;font-size:.875rem}}
.meta {{color:#4b5563;font-size:.875rem;margin:.25rem 0 .75rem}}
.poc img {{width:100%;border-radius:4px;border:1px solid #e5e7eb}}
.no-screenshot {{height:220px;background:#f3f4f6;border:1px dashed #d1d5db;border-radius:4px;display:flex;align-items:center;justify-content:center;color:#9ca3af;font-size:.8rem;text-align:center;padding:1rem}}
</style>
</head>
<body>
<div class="header"><h1>POC Comparison: {feature}</h1></div>
<div class="grid">
    {cards}
</div>
</body>
</html>
"#,
    );
    let dir = home.join("reports");
    std::fs::create_dir_all(&dir).map_err(Error::io(&dir))?;
    let path = dir.join(format!("compare-{feature}.html"));
    ehai_core::migrate::write_atomic(&path, html.as_bytes())?;
    Ok(path)
}

/// Capture a screenshot of `http://localhost:<port>` using the system Chrome CLI.
/// Returns `None` with a debug log if Chrome is not found or the server isn't reachable.
pub fn capture_screenshot(port: u16, out: &Path) -> Option<PathBuf> {
    let chrome = find_chrome()?;
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent).ok()?;
    }
    let status = std::process::Command::new(&chrome)
        .args([
            "--headless",
            "--no-sandbox",
            "--disable-gpu",
            &format!("--screenshot={}", out.display()),
            "--window-size=1280,720",
            &format!("http://localhost:{port}"),
        ])
        .output()
        .ok()?;
    if status.status.success() && out.exists() {
        tracing::info!(path = %out.display(), "screenshot captured");
        Some(out.to_path_buf())
    } else {
        tracing::debug!("screenshot failed: Chrome returned non-zero or file missing");
        None
    }
}

fn find_chrome() -> Option<String> {
    let candidates = [
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
        // macOS
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        // Windows
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
    ];
    for c in &candidates {
        if std::path::Path::new(c).exists() {
            return Some(c.to_string());
        }
        // Try PATH lookup for simple names
        if !c.contains('/') && !c.contains('\\') {
            let found = std::process::Command::new("which")
                .arg(c)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            if found.is_some() {
                return Some(c.to_string());
            }
        }
    }
    tracing::debug!("Chrome/Chromium not found — skipping screenshot");
    None
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn feature_slug_normalises() {
        assert_eq!(feature_slug("Login Screen"), "login-screen");
        assert_eq!(feature_slug("user--profile!"), "user-profile");
        assert_eq!(feature_slug("  gaps  "), "gaps");
        assert_eq!(feature_slug("already-fine"), "already-fine");
    }

    #[test]
    fn manifest_next_n() {
        let mut m = PocManifest {
            feature: "f".into(),
            entries: vec![],
        };
        assert_eq!(m.next_n(), 1);
        m.entries.push(PocEntry {
            n: 1,
            feature: "f".into(),
            branch: "b".into(),
            worktree: PathBuf::from("/tmp/w"),
            port: 3000,
            prompt: "p".into(),
            parent: None,
            parts: vec![],
            feedback: None,
            status: PocStatus::Draft,
            screenshot_paths: vec![],
            created_at: "2026-01-01".into(),
        });
        assert_eq!(m.next_n(), 2);
    }

    #[test]
    fn manifest_roundtrip() {
        let m = PocManifest {
            feature: "login".into(),
            entries: vec![PocEntry {
                n: 1,
                feature: "login".into(),
                branch: "ehai/poc/login/poc-1".into(),
                worktree: PathBuf::from("/home/user/.ehai/worktrees/abc/login/poc-1"),
                port: 3001,
                prompt: "Add a login form".into(),
                parent: None,
                parts: vec![],
                feedback: None,
                status: PocStatus::Draft,
                screenshot_paths: vec![],
                created_at: "2026-11-17".into(),
            }],
        };
        let json = serde_json::to_string_pretty(&m).unwrap();
        let m2: PocManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(m.entries[0].n, m2.entries[0].n);
        assert_eq!(m.entries[0].port, m2.entries[0].port);
        assert_eq!(m.entries[0].status, m2.entries[0].status);
    }

    #[test]
    fn html_compare_generates_valid_html() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        // Write a minimal manifest
        let manifest = PocManifest {
            feature: "login".into(),
            entries: vec![
                PocEntry {
                    n: 1,
                    feature: "login".into(),
                    branch: "ehai/poc/login/poc-1".into(),
                    worktree: r.join("wt1"),
                    port: 3001,
                    prompt: "Add login form".into(),
                    parent: None,
                    parts: vec![],
                    feedback: None,
                    status: PocStatus::Draft,
                    screenshot_paths: vec![],
                    created_at: "2026-11-17".into(),
                },
                PocEntry {
                    n: 2,
                    feature: "login".into(),
                    branch: "ehai/poc/login/poc-2".into(),
                    worktree: r.join("wt2"),
                    port: 3002,
                    prompt: "Iterate: add social login".into(),
                    parent: Some(1),
                    parts: vec![],
                    feedback: None,
                    status: PocStatus::Liked,
                    screenshot_paths: vec![],
                    created_at: "2026-11-17".into(),
                },
            ],
        };
        save_manifest(r, &manifest).unwrap();

        let home = d.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let path = poc_compare(r, &home, "login", &[1, 2]).unwrap();
        let html = std::fs::read_to_string(&path).unwrap();
        assert!(html.contains("poc-1"));
        assert!(html.contains("poc-2"));
        assert!(html.contains("Add login form"));
        assert!(html.contains("↑ from poc-1"));
        assert!(html.contains("<!DOCTYPE html>"));
    }

    #[test]
    fn repo_id_stable() {
        let d = tempfile::tempdir().unwrap();
        let id1 = repo_id(d.path());
        let id2 = repo_id(d.path());
        assert_eq!(id1, id2);
        assert_eq!(id1.len(), 16);
    }

    #[test]
    fn sandbox_check_restricted_blocks_outside_worktrees() {
        use ehai_core::config::SandboxMode;
        let home = std::path::Path::new("/home/user/.ehai");
        // allowed: inside worktrees
        assert!(sandbox_check(
            &home.join("worktrees/abc/feature/poc-1"),
            home,
            SandboxMode::Restricted
        )
        .is_ok());
        // blocked: outside worktrees
        assert!(sandbox_check(
            std::path::Path::new("/tmp/evil"),
            home,
            SandboxMode::Restricted
        )
        .is_err());
        // off mode: always ok
        assert!(sandbox_check(
            std::path::Path::new("/tmp/anything"),
            home,
            SandboxMode::Off
        )
        .is_ok());
    }
}
