//! Planning model (spec §5.2). The AI harness interviews the PM and writes the
//! content; this crate gives that content a fixed shape, validates it, renders it
//! (Markdown, Mermaid, HTML) and turns it into tracker items.

use std::path::{Path, PathBuf};

pub mod breakdown;
pub mod explore;
pub mod prd;
pub mod report;
pub mod triage;

pub use breakdown::Breakdown;
pub use explore::{find_gaps, PrdGap};
pub use prd::Prd;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid plan: {}", .0.join("; "))]
    Invalid(Vec<String>),
    #[error("no saved plan named `{0}`")]
    NotFound(String),
    #[error(transparent)]
    Core(#[from] ehai_core::Error),
    #[error("plan file is corrupt: {0}")]
    Corrupt(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// `Patient Booking v2!` → `patient-booking-v2` (max 40 chars, never empty).
pub fn slug(title: &str) -> String {
    let mut out = String::new();
    for c in title.chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let s: String = out.trim_matches('-').chars().take(40).collect();
    let s = s.trim_end_matches('-').to_string();
    if s.is_empty() {
        "plan".into()
    } else {
        s
    }
}

/// `1 epic`, `3 epics`.
pub fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// Where plans live: `~/.ehai/plans/<slug>/` (works without a repo, e.g. from
/// Claude desktop). PRDs are also written to `docs/prd/<slug>.md` in a repo.
pub fn plan_dir(home: &Path, slug: &str) -> PathBuf {
    home.join("plans").join(slug)
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(ehai_core::Error::io(dir))?;
    }
    let body = serde_json::to_string_pretty(value).map_err(|e| Error::Corrupt(e.to_string()))?;
    ehai_core::migrate::write_atomic(path, body.as_bytes())?;
    Ok(())
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path, slug: &str) -> Result<T> {
    let text = std::fs::read_to_string(path).map_err(|_| Error::NotFound(slug.into()))?;
    serde_json::from_str(&text).map_err(|e| Error::Corrupt(format!("{}: {e}", path.display())))
}

/// Slugs of every saved plan, newest first.
pub fn list_plans(home: &Path) -> Vec<String> {
    let mut plans: Vec<(std::time::SystemTime, String)> = std::fs::read_dir(home.join("plans"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let modified = e.metadata().ok()?.modified().ok()?;
            Some((modified, e.file_name().to_string_lossy().into_owned()))
        })
        .collect();
    plans.sort_by_key(|p| std::cmp::Reverse(p.0));
    plans.into_iter().map(|(_, s)| s).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(slug("Patient Booking v2!"), "patient-booking-v2");
        assert_eq!(slug("  --  "), "plan");
        assert_eq!(slug("Ünïcode & spaces"), "n-code-spaces");
        assert!(slug(&"a ".repeat(100)).len() <= 40);
        assert!(!slug("x".repeat(39).as_str().to_owned().as_str()).ends_with('-'));
    }
}
