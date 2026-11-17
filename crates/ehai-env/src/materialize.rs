//! Copy or symlink discovered files into a worktree (spec §6, step 2).

use std::path::Path;

use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// Safer: a POC's edits can't leak back into the source checkout.
    Copy,
    Symlink,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Materialized {
    pub rel: String,
}

/// Materialise `files` (relative paths) from `source_root` into `target_root`.
/// Refuses to touch a path git already tracks in the target — that would mean
/// a file meant to be the worktree's own copy of local config somehow matches
/// something checked in, which is never what this is for.
pub fn materialize(
    source_root: &Path,
    target_root: &Path,
    files: &[String],
    strategy: Strategy,
) -> Result<Vec<Materialized>> {
    let mut done = Vec::new();
    for rel in files {
        let src = source_root.join(rel);
        let dst = target_root.join(rel);
        if is_tracked(target_root, rel) {
            tracing::warn!(rel, "skipped: already tracked in the target worktree");
            continue;
        }
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent).map_err(Error::io(parent))?;
        }
        if dst.exists() || dst.is_symlink() {
            std::fs::remove_file(&dst).map_err(Error::io(&dst))?;
        }
        match strategy {
            Strategy::Copy => {
                std::fs::copy(&src, &dst).map_err(Error::io(&dst))?;
            }
            Strategy::Symlink => symlink(&src, &dst)?,
        }
        done.push(Materialized { rel: rel.clone() });
    }
    Ok(done)
}

fn is_tracked(root: &Path, rel: &str) -> bool {
    std::process::Command::new("git")
        .args(["ls-files", "--error-unmatch", rel])
        .current_dir(root)
        .output()
        .is_ok_and(|o| o.status.success())
}

#[cfg(unix)]
fn symlink(src: &Path, dst: &Path) -> Result<()> {
    std::os::unix::fs::symlink(src, dst).map_err(Error::io(dst))
}

#[cfg(windows)]
fn symlink(src: &Path, dst: &Path) -> Result<()> {
    std::os::windows::fs::symlink_file(src, dst).map_err(Error::io(dst))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn copies_files_and_creates_parent_dirs() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        fs::write(src.path().join(".env"), "A=1\n").unwrap();
        fs::create_dir_all(src.path().join("apps/web")).unwrap();
        fs::write(src.path().join("apps/web/.env.local"), "B=2\n").unwrap();

        let files = vec![".env".to_string(), "apps/web/.env.local".to_string()];
        let done = materialize(src.path(), dst.path(), &files, Strategy::Copy).unwrap();
        assert_eq!(done.len(), 2);
        assert_eq!(
            fs::read_to_string(dst.path().join(".env")).unwrap(),
            "A=1\n"
        );
        assert_eq!(
            fs::read_to_string(dst.path().join("apps/web/.env.local")).unwrap(),
            "B=2\n"
        );
    }

    #[test]
    fn refuses_to_overwrite_a_tracked_file_in_the_target() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        fs::write(src.path().join(".env"), "SOURCE\n").unwrap();
        fs::write(dst.path().join(".env"), "TRACKED\n").unwrap();
        let git = |args: &[&str]| {
            assert!(std::process::Command::new("git")
                .args(args)
                .current_dir(dst.path())
                .output()
                .unwrap()
                .status
                .success());
        };
        git(&["init", "-q"]);
        git(&["add", ".env"]);

        let done = materialize(
            src.path(),
            dst.path(),
            &[".env".to_string()],
            Strategy::Copy,
        )
        .unwrap();
        assert!(done.is_empty(), "tracked file must be left untouched");
        assert_eq!(
            fs::read_to_string(dst.path().join(".env")).unwrap(),
            "TRACKED\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlink_strategy_links_instead_of_copying() {
        let src = tempfile::tempdir().unwrap();
        let dst = tempfile::tempdir().unwrap();
        fs::write(src.path().join(".env"), "A=1\n").unwrap();
        materialize(
            src.path(),
            dst.path(),
            &[".env".to_string()],
            Strategy::Symlink,
        )
        .unwrap();
        assert!(dst.path().join(".env").is_symlink());
        assert_eq!(
            fs::read_to_string(dst.path().join(".env")).unwrap(),
            "A=1\n"
        );
    }
}
