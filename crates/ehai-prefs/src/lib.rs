//! User preference store. Two files, same [[entries]] TOML format:
//! - `~/.ehai/prefs/user.toml` (global)
//! - `<repo>/.ehai/prefs/project.toml` (repo; gitignored)
//!
//! Merged view: repo entries override global by key.

use std::path::Path;

use ehai_core::{Error, Result};
use serde::{Deserialize, Serialize};

// ── Types ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    #[default]
    Global,
    Repo,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    #[default]
    Explicit,
    Observed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pref {
    pub key: String,
    pub value: String,
    pub scope: Scope,
    pub source: Source,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Wrapper to serialize as `{ entries = [[..]] }` for TOML.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PrefsFile {
    #[serde(default)]
    entries: Vec<Pref>,
}

#[derive(Debug, Clone, Default)]
pub struct Prefs {
    pub entries: Vec<Pref>,
}

// ── Paths ──────────────────────────────────────────────────────────────────

fn global_path(home: &Path) -> std::path::PathBuf {
    ehai_core::paths::prefs_dir(home).join("user.toml")
}

fn repo_path(root: &Path) -> std::path::PathBuf {
    ehai_core::paths::repo_dir(root)
        .join("prefs")
        .join("project.toml")
}

// ── Load / save ────────────────────────────────────────────────────────────

fn read_prefs_file(path: &Path) -> Prefs {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Prefs::default();
    };
    toml::from_str::<PrefsFile>(&text)
        .map(|f| Prefs { entries: f.entries })
        .unwrap_or_else(|e| {
            tracing::warn!(path = %path.display(), error = %e, "prefs file parse error; ignoring");
            Prefs::default()
        })
}

fn write_prefs_file(path: &Path, prefs: &Prefs) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(Error::io(parent))?;
    }
    let file = PrefsFile {
        entries: prefs.entries.clone(),
    };
    let text = toml::to_string_pretty(&file).map_err(|e| Error::Invalid(e.to_string()))?;
    ehai_core::migrate::write_atomic(path, text.as_bytes())
}

impl Prefs {
    pub fn load_global(home: &Path) -> Prefs {
        read_prefs_file(&global_path(home))
    }

    pub fn load_repo(root: &Path) -> Prefs {
        read_prefs_file(&repo_path(root))
    }

    /// Merged: global first, then repo overrides by key.
    pub fn merged(home: &Path, root: Option<&Path>) -> Prefs {
        let mut prefs = Self::load_global(home);
        if let Some(r) = root {
            for rep in Self::load_repo(r).entries {
                if let Some(pos) = prefs.entries.iter().position(|p| p.key == rep.key) {
                    prefs.entries[pos] = rep;
                } else {
                    prefs.entries.push(rep);
                }
            }
        }
        prefs
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .find(|p| p.key == key)
            .map(|p| p.value.as_str())
    }

    /// Add or replace a preference by key.
    pub fn set(&mut self, key: &str, value: &str, scope: Scope, source: Source) {
        if let Some(pos) = self.entries.iter().position(|p| p.key == key) {
            let e = &mut self.entries[pos];
            e.value = value.to_string();
            e.scope = scope;
            e.source = source;
        } else {
            self.entries.push(Pref {
                key: key.to_string(),
                value: value.to_string(),
                scope,
                source,
                note: None,
            });
        }
    }

    /// Remove an entry by key. Returns `true` if it existed.
    pub fn delete(&mut self, key: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|p| p.key != key);
        self.entries.len() < before
    }

    pub fn save_global(&self, home: &Path) -> Result<()> {
        write_prefs_file(&global_path(home), self)
    }

    pub fn save_repo(&self, root: &Path) -> Result<()> {
        // ensure .ehai/prefs/ is in .ehai/.gitignore (idempotent)
        let gi = ehai_core::paths::repo_dir(root).join(".gitignore");
        let content = std::fs::read_to_string(&gi).unwrap_or_default();
        if !content.contains("prefs/") {
            let updated = format!("{content}prefs/\n");
            let _ = std::fs::write(&gi, updated);
        }
        write_prefs_file(&repo_path(root), self)
    }

    /// Compact summary block for prompt injection.
    pub fn to_context_text(&self) -> String {
        if self.entries.is_empty() {
            return "User preferences: (none set)".to_string();
        }
        let mut out = "User preferences:\n".to_string();
        for p in &self.entries {
            if p.source == Source::Explicit {
                out.push_str(&format!("- {} = {} (explicit)\n", p.key, p.value));
            } else {
                out.push_str(&format!("- {} = {}\n", p.key, p.value));
            }
        }
        out
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn roundtrip_global() {
        let d = tempdir().unwrap();
        let mut p = Prefs::default();
        p.set("poc.style", "dense", Scope::Global, Source::Explicit);
        p.set("output.detail", "brief", Scope::Global, Source::Observed);
        p.save_global(d.path()).unwrap();
        let loaded = Prefs::load_global(d.path());
        assert_eq!(loaded.get("poc.style"), Some("dense"));
        assert_eq!(loaded.get("output.detail"), Some("brief"));
        assert!(loaded.entries[0].source == Source::Explicit);
    }

    #[test]
    fn merged_repo_overrides_global() {
        let d = tempdir().unwrap();
        let root = d.path().join("repo");
        std::fs::create_dir_all(&root).unwrap();

        let mut global = Prefs::default();
        global.set("theme", "dark", Scope::Global, Source::Observed);
        global.save_global(d.path()).unwrap();

        let mut repo = Prefs::default();
        repo.set("theme", "light", Scope::Repo, Source::Explicit);
        repo.save_repo(&root).unwrap();

        let merged = Prefs::merged(d.path(), Some(&root));
        assert_eq!(merged.get("theme"), Some("light")); // repo wins
    }

    #[test]
    fn delete_removes_entry() {
        let mut p = Prefs::default();
        p.set("k", "v", Scope::Global, Source::Explicit);
        assert!(p.delete("k"));
        assert!(!p.delete("k")); // already gone
        assert_eq!(p.get("k"), None);
    }

    #[test]
    fn to_context_text_snapshot() {
        let mut p = Prefs::default();
        p.set("poc.style", "dense", Scope::Global, Source::Observed);
        p.set(
            "planning.granularity",
            "story",
            Scope::Global,
            Source::Explicit,
        );
        let text = p.to_context_text();
        insta::assert_snapshot!(text);
    }
}
