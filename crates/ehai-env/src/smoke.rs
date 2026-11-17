//! Health check (spec §6, step 6): run the configured smoke command for an
//! app and report ready or exactly what's broken.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum SmokeOutcome {
    Ok,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct SmokeResult {
    /// App path the command ran in (relative to the worktree root).
    pub app: String,
    pub outcome: SmokeOutcome,
    /// Command output tail on failure; empty on success.
    pub detail: String,
}

/// Run `cmd` via the shell in `worktree_root.join(app_path)`.
pub fn run(worktree_root: &Path, app_path: &str, cmd: &str) -> SmokeResult {
    let dir = worktree_root.join(app_path);
    let (shell, flag) = if cfg!(windows) {
        ("cmd", "/C")
    } else {
        ("sh", "-c")
    };
    let mut command = Command::new(shell);
    command.arg(flag).arg(cmd).current_dir(&dir);
    let out = run_with_timeout(command, TIMEOUT);
    match out {
        Ok(o) if o.status.success() => SmokeResult {
            app: app_path.to_string(),
            outcome: SmokeOutcome::Ok,
            detail: String::new(),
        },
        Ok(o) => SmokeResult {
            app: app_path.to_string(),
            outcome: SmokeOutcome::Failed,
            detail: tail(&String::from_utf8_lossy(&o.stderr)),
        },
        Err(e) => SmokeResult {
            app: app_path.to_string(),
            outcome: SmokeOutcome::Failed,
            detail: e.to_string(),
        },
    }
}

fn run_with_timeout(mut cmd: Command, timeout: Duration) -> std::io::Result<std::process::Output> {
    use std::sync::mpsc;
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(cmd.output());
    });
    rx.recv_timeout(timeout).unwrap_or_else(|_| {
        Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!("smoke command exceeded {}s", timeout.as_secs()),
        ))
    })
}

fn tail(s: &str) -> String {
    let lines: Vec<&str> = s.lines().collect();
    lines[lines.len().saturating_sub(10)..].join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn succeeds_and_fails_as_expected() {
        let dir = tempfile::tempdir().unwrap();
        let ok = run(dir.path(), "", "exit 0");
        assert_eq!(ok.outcome, SmokeOutcome::Ok);
        assert!(ok.detail.is_empty());

        // cmd.exe doesn't support bash's `>&2` or `;` chaining; use plain exit 1.
        let fail_cmd = if cfg!(windows) {
            "exit 1"
        } else {
            "echo boom >&2; exit 1"
        };
        let failed = run(dir.path(), "", fail_cmd);
        assert_eq!(failed.outcome, SmokeOutcome::Failed, "{failed:?}");
    }

    #[test]
    fn runs_in_the_app_subdirectory() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("apps/web")).unwrap();
        std::fs::write(dir.path().join("apps/web/marker"), "").unwrap();
        // `test -f` is POSIX only; cmd.exe uses `if exist`.
        let check = if cfg!(windows) {
            "if exist marker (exit 0) else (exit 1)"
        } else {
            "test -f marker"
        };
        let ok = run(dir.path(), "apps/web", check);
        assert_eq!(ok.outcome, SmokeOutcome::Ok, "{ok:?}");
    }
}
