//! `ehai prefs list|set|delete`: user preference management.

use anyhow::Result;
use ehai_core::Config;
use ehai_prefs::{Prefs, Scope, Source};

fn repo_root() -> Option<std::path::PathBuf> {
    std::env::current_dir()
        .ok()
        .and_then(|cwd| ehai_core::git::toplevel(&cwd))
}

pub fn list_prefs(home: &std::path::Path, config: &Config) -> Result<String> {
    let _ = config;
    let root = repo_root();
    let prefs = Prefs::merged(home, root.as_deref());
    if prefs.entries.is_empty() {
        return Ok("No preferences set.".into());
    }
    let mut out = String::new();
    for p in &prefs.entries {
        let explicit = if p.source == Source::Explicit {
            " (explicit)"
        } else {
            ""
        };
        let scope = format!("{:?}", p.scope).to_lowercase();
        out.push_str(&format!("[{scope}] {} = {}{}\n", p.key, p.value, explicit));
    }
    Ok(out)
}

pub fn set_pref(
    home: &std::path::Path,
    config: &Config,
    key: &str,
    value: &str,
    scope: &str,
) -> Result<String> {
    let _ = config;
    if scope == "repo" {
        let root = repo_root()
            .ok_or_else(|| anyhow::anyhow!("not inside a git repository; use --scope global"))?;
        let mut prefs = Prefs::load_repo(&root);
        prefs.set(key, value, Scope::Repo, Source::Explicit);
        prefs.save_repo(&root)?;
    } else {
        let mut prefs = Prefs::load_global(home);
        prefs.set(key, value, Scope::Global, Source::Explicit);
        prefs.save_global(home)?;
    }
    Ok(format!("Set [{scope}] {key} = {value}"))
}

pub fn delete_pref(
    home: &std::path::Path,
    config: &Config,
    key: &str,
    scope: &str,
) -> Result<String> {
    let _ = config;
    let deleted = if scope == "repo" {
        let root = repo_root()
            .ok_or_else(|| anyhow::anyhow!("not inside a git repository; use --scope global"))?;
        let mut prefs = Prefs::load_repo(&root);
        let d = prefs.delete(key);
        if d {
            prefs.save_repo(&root)?;
        }
        d
    } else {
        let mut prefs = Prefs::load_global(home);
        let d = prefs.delete(key);
        if d {
            prefs.save_global(home)?;
        }
        d
    };
    if deleted {
        Ok(format!("Deleted [{scope}] {key}"))
    } else {
        Ok(format!("No [{scope}] pref named '{key}'"))
    }
}
