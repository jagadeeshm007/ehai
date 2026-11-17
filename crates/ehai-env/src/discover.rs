//! Find the gitignored config a fresh worktree needs (spec §6, step 1): walk the
//! source checkout, match known patterns plus `[env]` include/exclude globs, then
//! keep only the ones git actually ignores (or doesn't know about yet).

use std::path::Path;
use std::process::Command;

use crate::patterns::{self, ALWAYS_EXCLUDE, BUILTIN};

/// Never descended into: huge, reproducible from a lockfile, or not source.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    ".venv",
    "venv",
    "__pycache__",
    "target",
    "dist",
    "build",
    ".next",
    ".expo",
    "vendor",
    "Pods",
    ".ehai",
    ".turbo",
    "coverage",
];

/// Relative paths (repo-root-relative, `/`-separated) of files to materialise.
pub fn discover(root: &Path, include: &[String], exclude: &[String]) -> Vec<String> {
    let extra_include: Vec<&str> = include.iter().map(String::as_str).collect();
    let extra_exclude: Vec<&str> = exclude.iter().map(String::as_str).collect();

    let mut candidates = Vec::new();
    walk(root, root, &mut candidates);
    candidates.retain(|rel| {
        !patterns::matches_any(rel, ALWAYS_EXCLUDE)
            && (patterns::matches_any(rel, BUILTIN) || patterns::matches_any(rel, &extra_include))
            && !patterns::matches_any(rel, &extra_exclude)
    });
    candidates.retain(|rel| not_tracked(root, rel));
    candidates.sort();
    candidates.dedup();
    candidates
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if !SKIP_DIRS.contains(&name.as_ref()) {
                walk(root, &path, out);
            }
        } else if let Ok(rel) = path.strip_prefix(root) {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

/// `true` if git doesn't track this path: either ignored, or simply unknown to
/// it. Either way it's safe (and the point) to materialise it into a worktree.
/// Repos with no git available are treated as "nothing tracked" (fail open).
fn not_tracked(root: &Path, rel: &str) -> bool {
    let out = Command::new("git")
        .args(["ls-files", "--error-unmatch", rel])
        .current_dir(root)
        .output();
    match out {
        Ok(o) => !o.status.success(),
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn write(root: &Path, rel: &str, body: &str) {
        let p = root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    }

    fn git(root: &Path, args: &[&str]) {
        assert!(Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .unwrap()
            .status
            .success());
    }

    #[test]
    fn finds_env_files_but_not_tracked_or_example_or_node_modules() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        git(root, &["init", "-q"]);
        write(root, ".gitignore", ".env\n.env.*\n!.env.example\n");
        write(root, ".env", "A=1\n");
        write(root, ".env.example", "A=\n");
        write(root, "apps/web/.env.local", "B=2\n");
        write(root, "README.md", "tracked\n");
        write(root, "node_modules/leftpad/.env", "noise\n");
        git(root, &["add", "README.md", ".gitignore", ".env.example"]);

        let found = discover(root, &[], &[]);
        assert_eq!(
            found,
            vec![".env".to_string(), "apps/web/.env.local".to_string()]
        );
    }

    #[test]
    fn config_include_and_exclude_apply() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "apps/web/config.local.yaml", "x: 1\n");
        write(root, ".env", "A=1\n");

        let found = discover(root, &["apps/*/config.local.yaml".into()], &[]);
        assert!(found.contains(&"apps/web/config.local.yaml".to_string()));
        assert!(found.contains(&".env".to_string()));

        let found = discover(root, &[], &["*.env".into()]);
        assert!(!found.contains(&".env".to_string()));
    }
}
