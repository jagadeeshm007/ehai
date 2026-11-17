//! Harness adapters. Each one can detect its harness, generate its files, install,
//! verify (status) and remove — idempotently and reversibly (spec §10).
//! M1 ships Claude Code; M7 adds Codex, Gemini CLI, Cursor, OpenCode, VS Code,
//! Windsurf and AGENTS.md behind the same trait.

use std::path::{Path, PathBuf};

use ehai_core::Brand;

pub mod claude_code;
mod templates;

pub use templates::render;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0} is not installed or not on PATH")]
    NotDetected(&'static str),
    #[error("`{cmd}` failed: {stderr}")]
    Command { cmd: String, stderr: String },
    #[error(transparent)]
    Core(#[from] ehai_core::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Inputs every adapter renders with.
#[derive(Debug, Clone)]
pub struct Ctx {
    pub brand: Brand,
    /// `pm`, `dev`, `both`, or `None` when not chosen yet.
    pub role: Option<String>,
    pub home: PathBuf,
    /// Absolute path of the `ehai` binary harnesses should spawn (`<exe> mcp`).
    pub exe: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct File {
    /// Path relative to the adapter's output root, `/`-separated.
    pub path: String,
    pub contents: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Harness not found on this machine.
    Unavailable,
    NotInstalled,
    Installed,
}

pub trait Harness {
    /// Stable id used on the CLI and in config, e.g. `claude-code`.
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    /// Is the harness present on this machine? `Some(detail)` when found.
    fn detect(&self) -> Option<String>;
    /// Every file this adapter owns, rendered for `ctx`. Pure; snapshot-tested.
    fn generate(&self, ctx: &Ctx) -> Vec<File>;
    /// Write files and register with the harness. Safe to run repeatedly.
    fn install(&self, ctx: &Ctx) -> Result<String>;
    /// Undo everything `install` did.
    fn remove(&self, ctx: &Ctx) -> Result<String>;
    fn status(&self, ctx: &Ctx) -> Status;
}

/// Adapters available in this build.
pub fn all() -> Vec<Box<dyn Harness>> {
    vec![Box::new(claude_code::ClaudeCode::from_env())]
}

pub fn by_id(id: &str) -> Option<Box<dyn Harness>> {
    all().into_iter().find(|h| h.id() == id)
}

/// Make `root` contain exactly `files`: write what changed, delete what is no
/// longer generated. `root` must be a directory ehai owns outright.
/// Returns the number of files written or deleted (0 = already up to date).
pub fn sync_tree(root: &Path, files: &[File]) -> Result<usize> {
    use ehai_core::Error as E;
    let mut changes = 0;
    for f in files {
        let p = root.join(&f.path);
        if std::fs::read_to_string(&p).ok().as_deref() == Some(f.contents.as_str()) {
            continue;
        }
        if let Some(dir) = p.parent() {
            std::fs::create_dir_all(dir).map_err(E::io(dir))?;
        }
        ehai_core::migrate::write_atomic(&p, f.contents.as_bytes())?;
        changes += 1;
    }
    let wanted: std::collections::BTreeSet<PathBuf> =
        files.iter().map(|f| root.join(&f.path)).collect();
    for stale in list_files(root).into_iter().filter(|p| !wanted.contains(p)) {
        std::fs::remove_file(&stale).map_err(E::io(&stale))?;
        changes += 1;
    }
    remove_empty_dirs(root);
    Ok(changes)
}

fn list_files(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => out.extend(list_files(&p)),
            Ok(_) => out.push(p),
            Err(_) => {}
        }
    }
    out
}

fn remove_empty_dirs(dir: &Path) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            remove_empty_dirs(&e.path());
            let _ = std::fs::remove_dir(e.path()); // only succeeds when empty
        }
    }
}

/// First match for `bin` on `PATH` (with `.exe`/`.cmd` on Windows).
pub fn find_on_path(bin: &str) -> Option<PathBuf> {
    let exts: &[&str] = if cfg!(windows) {
        &[".exe", ".cmd", ".bat", ""]
    } else {
        &[""]
    };
    std::env::split_paths(&std::env::var_os("PATH")?).find_map(|dir| {
        exts.iter()
            .map(|ext| dir.join(format!("{bin}{ext}")))
            .find(|p| p.is_file())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(path: &str, contents: &str) -> File {
        File {
            path: path.into(),
            contents: contents.into(),
        }
    }

    #[test]
    fn sync_tree_is_idempotent_and_prunes() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path().join("out");
        let v1 = vec![f("a.txt", "1"), f("x/b.txt", "2"), f("old/c.txt", "3")];
        assert_eq!(sync_tree(&r, &v1).unwrap(), 3);
        assert_eq!(sync_tree(&r, &v1).unwrap(), 0, "second run changes nothing");

        let v2 = vec![f("a.txt", "1"), f("x/b.txt", "22")];
        assert_eq!(sync_tree(&r, &v2).unwrap(), 2, "one rewrite, one delete");
        assert!(!r.join("old").exists(), "empty dirs pruned");
        assert_eq!(std::fs::read_to_string(r.join("x/b.txt")).unwrap(), "22");
    }
}
