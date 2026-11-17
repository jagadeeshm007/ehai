//! Thin wrapper over the system `git` binary (no libgit2; see spec §3).

use std::path::Path;
use std::process::Command;

use crate::{Error, Result};

/// Run `git <args>` in `dir`, returning trimmed stdout. Non-zero exit is an error
/// carrying git's stderr.
pub fn run(dir: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(Error::io(dir))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(Error::Git {
            args: args.join(" "),
            stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
        })
    }
}

/// Like [`run`] but `None` on any failure, for optional facts (remote, branch).
pub fn try_run(dir: &Path, args: &[&str]) -> Option<String> {
    run(dir, args).ok().filter(|s| !s.is_empty())
}

/// Root of the git work tree containing `dir`, if any.
pub fn toplevel(dir: &Path) -> Option<std::path::PathBuf> {
    try_run(dir, &["rev-parse", "--show-toplevel"]).map(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toplevel_and_errors() {
        let d = tempfile::tempdir().unwrap();
        assert_eq!(toplevel(d.path()), None);
        run(d.path(), &["init", "-q"]).unwrap();
        let top = toplevel(d.path()).unwrap();
        assert_eq!(
            top.canonicalize().unwrap(),
            d.path().canonicalize().unwrap()
        );
        let err = run(d.path(), &["rev-parse", "--verify", "nope"]).unwrap_err();
        assert!(err.to_string().contains("rev-parse"));
        assert_eq!(try_run(d.path(), &["remote", "get-url", "origin"]), None);
    }
}
