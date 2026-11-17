//! Marked blocks in files ehai shares with the user (shell rc files, later harness
//! configs): everything ehai owns sits between `# >>> ehai >>>` and
//! `# <<< ehai <<<`, so it can be updated or removed without touching the user's
//! own lines (spec §10). A `<file>.ehai-backup` is taken before the first edit.

use std::path::Path;

use crate::{migrate::write_atomic, Error, Result};

pub const START: &str = "# >>> ehai >>>";
pub const END: &str = "# <<< ehai <<<";

/// Insert or replace the ehai block in `path` (created if missing).
/// Returns `true` when the file changed.
pub fn upsert(path: &Path, body: &str) -> Result<bool> {
    let old = std::fs::read_to_string(path).unwrap_or_default();
    let block = format!("{START}\n{}\n{END}\n", body.trim_end());
    let (before, after) = split(&old);
    let mut new = before.to_string();
    if !new.is_empty() && !new.ends_with('\n') {
        new.push('\n');
    }
    new.push_str(&block);
    new.push_str(after);
    if new == old {
        return Ok(false);
    }
    backup_once(path)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(Error::io(dir))?;
    }
    write_atomic(path, new.as_bytes())?;
    Ok(true)
}

/// Remove the ehai block from `path`, if present. Returns `true` when it changed.
pub fn remove(path: &Path) -> Result<bool> {
    let Ok(old) = std::fs::read_to_string(path) else {
        return Ok(false);
    };
    if !old.contains(START) {
        return Ok(false);
    }
    let (before, after) = split(&old);
    write_atomic(path, format!("{before}{after}").as_bytes())?;
    Ok(true)
}

pub fn contains(path: &Path) -> bool {
    std::fs::read_to_string(path).is_ok_and(|s| s.contains(START))
}

/// Text before the block and after it (block plus its trailing newline dropped).
/// Without a block: the whole text, then nothing.
fn split(text: &str) -> (&str, &str) {
    let Some(s) = text.find(START) else {
        return (text, "");
    };
    let after_start = &text[s..];
    let e = after_start
        .find(END)
        .map_or(text.len(), |e| s + e + END.len());
    let rest = &text[e..];
    (&text[..s], rest.strip_prefix('\n').unwrap_or(rest))
}

fn backup_once(path: &Path) -> Result<()> {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".ehai-backup");
    let backup = path.with_file_name(name);
    if path.is_file() && !backup.exists() {
        std::fs::copy(path, &backup).map_err(Error::io(&backup))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_is_idempotent_and_remove_restores() {
        let d = tempfile::tempdir().unwrap();
        let rc = d.path().join(".bashrc");
        let original = "alias ll='ls -l'\nexport EDITOR=vim";
        std::fs::write(&rc, original).unwrap();

        assert!(upsert(&rc, "export PATH=\"$HOME/.ehai/bin:$PATH\"").unwrap());
        assert!(!upsert(&rc, "export PATH=\"$HOME/.ehai/bin:$PATH\"").unwrap());
        let text = std::fs::read_to_string(&rc).unwrap();
        assert_eq!(text.matches(START).count(), 1);
        assert!(text.starts_with(original));

        // user adds lines after our block; an update keeps them
        std::fs::write(&rc, format!("{text}alias gs='git status'\n")).unwrap();
        assert!(upsert(&rc, "export PATH=/new:$PATH").unwrap());
        let text = std::fs::read_to_string(&rc).unwrap();
        assert!(text.contains("/new") && text.contains("alias gs") && !text.contains(".ehai/bin"));

        assert!(remove(&rc).unwrap());
        assert!(!remove(&rc).unwrap());
        assert_eq!(
            std::fs::read_to_string(&rc).unwrap(),
            "alias ll='ls -l'\nexport EDITOR=vim\nalias gs='git status'\n"
        );
        assert_eq!(
            std::fs::read_to_string(d.path().join(".bashrc.ehai-backup")).unwrap(),
            original,
            "backup holds the pre-ehai file"
        );
    }

    #[test]
    fn creates_missing_file() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("conf.d/ehai.fish");
        assert!(upsert(&f, "fish_add_path ~/.ehai/bin").unwrap());
        assert!(contains(&f));
    }
}
