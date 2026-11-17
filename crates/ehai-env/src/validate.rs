//! Compare materialised env files against `.env.example`/`.env.sample` (spec
//! §6, step 3). Only key *names* are ever read or returned — values never
//! leave the file they're in.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// `KEY=value` lines, comments and blanks ignored. `export KEY=value` (shell
/// style) also parses.
fn keys(path: &Path) -> BTreeSet<String> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return BTreeSet::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| l.strip_prefix("export ").unwrap_or(l).split('=').next())
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .collect()
}

/// Keys present in `example` but in none of `actual`.
pub fn missing_keys(example: &Path, actual: &[PathBuf]) -> Vec<String> {
    let want = keys(example);
    let have: BTreeSet<String> = actual.iter().flat_map(|p| keys(p)).collect();
    want.difference(&have).cloned().collect()
}

/// Walk `root` for every `.env.example`/`.env.sample`, pair it with sibling
/// `.env*` files in the same directory, and report missing keys per example
/// (prefixed with its directory when not at the root, since a monorepo can
/// have one per app).
pub fn missing_keys_in(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    for example in find_examples(root) {
        let dir = example.parent().unwrap_or(root);
        let siblings: Vec<PathBuf> = std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| is_env_file(p) && p != &example)
            .collect();
        let rel_dir = dir
            .strip_prefix(root)
            .unwrap_or(dir)
            .to_string_lossy()
            .replace('\\', "/");
        let prefix = if rel_dir.is_empty() || rel_dir == "." {
            String::new()
        } else {
            format!("{rel_dir}/")
        };
        out.extend(
            missing_keys(&example, &siblings)
                .into_iter()
                .map(|k| format!("{prefix}{k}")),
        );
    }
    out.sort();
    out
}

/// Every non-example `.env*` file under `root`.
pub fn existing_env_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(root, &mut out);
    out
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !matches!(name.as_str(), "node_modules" | ".git" | ".venv" | "target") {
                walk(&path, out);
            }
        } else if is_env_file(&path) {
            out.push(path);
        }
    }
}

fn find_examples(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if !matches!(name.as_str(), "node_modules" | ".git" | ".venv" | "target") {
                    walk(&path, out);
                }
            } else if name == ".env.example" || name == ".env.sample" {
                out.push(path);
            }
        }
    }
    walk(root, &mut out);
    out
}

fn is_env_file(p: &Path) -> bool {
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
    (name == ".env" || name.starts_with(".env.")) && name != ".env.example" && name != ".env.sample"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn finds_missing_keys_without_reading_values_elsewhere() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::write(root.join(".env.example"), "A=\nB=\n# comment\n\nC=\n").unwrap();
        fs::write(root.join(".env"), "A=secret1\nexport B=secret2\n").unwrap();

        let missing = missing_keys_in(root);
        assert_eq!(missing, vec!["C".to_string()]);
    }

    #[test]
    fn per_app_examples_are_reported_with_their_own_prefix() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join("apps/web")).unwrap();
        fs::create_dir_all(root.join("apps/api")).unwrap();
        fs::write(root.join("apps/web/.env.example"), "WEB_KEY=\n").unwrap();
        fs::write(root.join("apps/api/.env.example"), "API_KEY=\n").unwrap();
        fs::write(root.join("apps/api/.env"), "API_KEY=1\n").unwrap();

        let missing = missing_keys_in(root);
        assert_eq!(missing, vec!["apps/web/WEB_KEY".to_string()]);
    }

    #[test]
    fn no_examples_means_nothing_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert!(missing_keys_in(dir.path()).is_empty());
    }
}
