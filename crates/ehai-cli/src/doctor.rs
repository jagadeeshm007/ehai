//! `ehai doctor`: health checks with a fix hint for every failure. M0 stub; later
//! milestones add auth, tracker reachability, adapters, ports and auto-fixes.

use std::io;
use std::path::Path;
use std::process::Command;

use ehai_core::Config;

use crate::ui::{Prompter, StepError, StepResult};

const MIN_GIT: (u32, u32) = (2, 20); // reliable `git worktree` support

pub fn run(
    p: &mut dyn Prompter,
    home: &Path,
    config: &ehai_core::Result<Config>,
    post_update: bool,
) -> io::Result<bool> {
    let brand = config.as_ref().map_or("ehai", |c| c.brand.name.as_str());
    p.intro(&format!(
        "{brand} doctor{}",
        if post_update { " (post-update)" } else { "" }
    ))?;

    let mut ok = true;
    ok &= p.step("Binary", &mut || Ok(crate::VERSION_LINE.to_string()))?;
    ok &= p.step("Config", &mut || match config {
        Ok(_) => {
            let path = ehai_core::paths::global_config(home);
            Ok(if path.is_file() {
                path.display().to_string()
            } else {
                "defaults (no config file yet)".into()
            })
        }
        Err(e) => Err(fail(
            e.to_string(),
            "fix the file, or move it aside to fall back to defaults",
        )),
    })?;
    ok &= p.step("Git", &mut check_git)?;
    ok &= p.step("Home dir", &mut || check_writable(home))?;
    ok &= p.step("GitHub CLI", &mut || {
        Ok(tool_version("gh", &["--version"])
            .unwrap_or_else(|| "not found (optional; used to reuse GitHub auth)".into()))
    })?;

    p.outro(if ok {
        "All checks passed"
    } else {
        "Some checks failed"
    })?;
    Ok(ok)
}

fn fail(message: impl Into<String>, fix: &str) -> StepError {
    StepError {
        message: message.into(),
        fix: Some(fix.into()),
    }
}

fn tool_version(bin: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(bin).args(args).output().ok()?;
    out.status.success().then(|| {
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .to_string()
    })
}

fn check_git() -> StepResult {
    let line = tool_version("git", &["--version"])
        .ok_or_else(|| fail("git not found", "install git 2.20 or newer"))?;
    match parse_git_version(&line) {
        Some(v) if v >= MIN_GIT => Ok(line),
        Some(_) => Err(fail(
            format!("{line} is too old"),
            "upgrade git to 2.20 or newer",
        )),
        None => Err(fail(
            format!("unrecognised version: {line}"),
            "check `git --version`",
        )),
    }
}

/// `git version 2.43.0.windows.1` → `(2, 43)`.
fn parse_git_version(line: &str) -> Option<(u32, u32)> {
    let ver = line
        .split_whitespace()
        .find(|w| w.starts_with(|c: char| c.is_ascii_digit()))?;
    let mut parts = ver.split('.').map(|p| p.parse::<u32>().ok());
    Some((parts.next()??, parts.next()??))
}

fn check_writable(home: &Path) -> StepResult {
    let fix = "make sure the directory is writable, or set EHAI_HOME to another location";
    std::fs::create_dir_all(home).map_err(|e| fail(format!("{}: {e}", home.display()), fix))?;
    let probe = home.join(".doctor-probe");
    std::fs::write(&probe, b"ok").map_err(|e| fail(format!("{}: {e}", home.display()), fix))?;
    let _ = std::fs::remove_file(&probe);
    Ok(home.display().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_git_versions() {
        assert_eq!(parse_git_version("git version 2.53.0"), Some((2, 53)));
        assert_eq!(
            parse_git_version("git version 2.43.0.windows.1"),
            Some((2, 43))
        );
        assert_eq!(
            parse_git_version("git version 2.39.3 (Apple Git-146)"),
            Some((2, 39))
        );
        assert_eq!(parse_git_version("nonsense"), None);
        assert!(parse_git_version("git version 2.19.1").unwrap() < MIN_GIT);
    }

    #[test]
    fn unwritable_home_fails_with_fix() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("file");
        std::fs::write(&file, "").unwrap();
        let err = check_writable(&file.join("sub")).unwrap_err();
        assert!(err.fix.is_some());
    }
}
