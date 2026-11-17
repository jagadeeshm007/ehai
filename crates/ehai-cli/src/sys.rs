//! Facts about the machine, shared by `doctor` and `init`.

use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

/// First line of `<bin> <args>` stdout, if the command exists and succeeds.
pub fn tool_version(bin: &str, args: &[&str]) -> Option<String> {
    let out =
        ehai_core::process::output(Command::new(bin).args(args), Duration::from_secs(10)).ok()?;
    out.status.success().then(|| {
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .to_string()
    })
}

pub fn gh_authenticated() -> bool {
    ehai_core::process::output(
        Command::new("gh").args(["auth", "status"]),
        Duration::from_secs(10),
    )
    .is_ok_and(|o| o.status.success())
}

fn home() -> Option<PathBuf> {
    std::env::home_dir()
}

pub fn claude_desktop_installed() -> bool {
    let dir = if cfg!(target_os = "macos") {
        home().map(|h| h.join("Library/Application Support/Claude"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Claude"))
    } else {
        home().map(|h| h.join(".config/Claude"))
    };
    dir.is_some_and(|d| d.is_dir())
}

pub fn chrome() -> Option<PathBuf> {
    let fixed: &[&str] = if cfg!(target_os = "macos") {
        &[
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/Applications/Chromium.app/Contents/MacOS/Chromium",
        ]
    } else if cfg!(windows) {
        &[
            r"C:\Program Files\Google\Chrome\Application\chrome.exe",
            r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
        ]
    } else {
        &[]
    };
    fixed
        .iter()
        .map(PathBuf::from)
        .find(|p| p.is_file())
        .or_else(|| {
            [
                "google-chrome",
                "google-chrome-stable",
                "chromium",
                "chromium-browser",
                "chrome",
            ]
            .iter()
            .find_map(|b| ehai_adapters::find_on_path(b))
        })
}

/// Harnesses we can see but don't have an adapter for yet (M7).
pub fn other_harnesses() -> Vec<&'static str> {
    let home = home().unwrap_or_default();
    [
        ("Codex CLI", "codex", ".codex"),
        ("Gemini CLI", "gemini", ".gemini"),
        ("Cursor", "cursor", ".cursor"),
        ("OpenCode", "opencode", ".config/opencode"),
        ("VS Code", "code", ".vscode"),
        ("Windsurf", "windsurf", ".codeium/windsurf"),
    ]
    .into_iter()
    .filter(|(_, bin, dir)| ehai_adapters::find_on_path(bin).is_some() || home.join(dir).is_dir())
    .map(|(name, _, _)| name)
    .collect()
}
