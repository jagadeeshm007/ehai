//! Filesystem locations. Everything lives under `~/.ehai` unless `EHAI_HOME` is set
//! (tests always set it, so they never touch the real home directory).

use std::path::{Path, PathBuf};

use crate::{Error, Result};

pub const REPO_CONFIG_FILE: &str = ".ehai.toml";

pub fn ehai_home() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("EHAI_HOME").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    std::env::home_dir()
        .map(|h| h.join(".ehai"))
        .ok_or(Error::NoHome)
}

pub fn global_config(home: &Path) -> PathBuf {
    home.join("config.toml")
}

pub fn logs_dir(home: &Path) -> PathBuf {
    home.join("logs")
}

pub fn bin_dir(home: &Path) -> PathBuf {
    home.join("bin")
}

pub fn audit_dir(home: &Path) -> PathBuf {
    home.join("audit")
}

pub fn prefs_dir(home: &Path) -> PathBuf {
    home.join("prefs")
}

/// Generated harness files (plugins, marketplaces) live under here.
pub fn adapters_dir(home: &Path) -> PathBuf {
    home.join("adapters")
}

/// Per-repo state dir (`<repo>/.ehai`).
pub fn repo_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(".ehai")
}

/// Cached team list for one tracker project (spec: roles-and-team plan §2),
/// refreshed at most once a day, or on demand with `ehai team refresh`.
pub fn team_cache(home: &Path, owner: &str, number: u64) -> PathBuf {
    // owner logins are `[A-Za-z0-9-]`, so this is always a valid filename.
    home.join("cache/team")
        .join(format!("{owner}-{number}.json"))
}

/// Global registry of ports allocated to worktrees (spec §6), locked on every edit.
pub fn ports_registry(home: &Path) -> PathBuf {
    home.join("ports.json")
}

/// When the background update check last ran (spec §11: at most once a day,
/// notify-only unless `update.auto`).
pub fn update_check_cache(home: &Path) -> PathBuf {
    home.join("update-check.json")
}

/// Nearest `.ehai.toml` walking up from `start`.
pub fn find_repo_config(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .map(|dir| dir.join(REPO_CONFIG_FILE))
        .find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_nearest_repo_config() {
        let root = tempfile::tempdir().unwrap();
        let nested = root.path().join("a/b/c");
        std::fs::create_dir_all(&nested).unwrap();
        assert_eq!(find_repo_config(&nested), None);

        std::fs::write(root.path().join(REPO_CONFIG_FILE), "").unwrap();
        std::fs::write(root.path().join("a/b").join(REPO_CONFIG_FILE), "").unwrap();
        assert_eq!(
            find_repo_config(&nested),
            Some(root.path().join("a/b").join(REPO_CONFIG_FILE))
        );
    }
}
