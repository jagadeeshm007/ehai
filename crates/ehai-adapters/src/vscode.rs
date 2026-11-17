//! VS Code adapter.
//!
//! Registers the MCP server under `servers.<brand>` in the user-level `mcp.json`
//! file and writes prompt files to the user prompts directory.
//!
//! Paths verified 2026-11-17 (code.visualstudio.com/docs/copilot/chat/mcp-servers):
//! MCP config in user data dir (`mcp.json`), prompts in user data `prompts/`.
//! VS Code uses `servers` (not `mcpServers`) and requires `type: "stdio"`.
//! macOS: `~/Library/Application Support/Code/User/`,
//! Windows: `%APPDATA%\Code\User\`,
//! Linux: `~/.config/Code/User/`.
//! See ADR-0026.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::{json_mcp, render, sync_tree, Ctx, Error, File, Harness, Result, Status};

const COMMANDS: &[(&str, &str)] = &[
    (
        "help",
        include_str!("../../../templates/shared/commands/help.md"),
    ),
    (
        "plan",
        include_str!("../../../templates/shared/commands/plan.md"),
    ),
    (
        "breakdown",
        include_str!("../../../templates/shared/commands/breakdown.md"),
    ),
    (
        "sync",
        include_str!("../../../templates/shared/commands/sync.md"),
    ),
    (
        "status",
        include_str!("../../../templates/shared/commands/status.md"),
    ),
    (
        "triage",
        include_str!("../../../templates/shared/commands/triage.md"),
    ),
    (
        "assign",
        include_str!("../../../templates/shared/commands/assign.md"),
    ),
    (
        "prefs",
        include_str!("../../../templates/shared/commands/prefs.md"),
    ),
];

pub struct VsCode {
    /// `<user-data>/mcp.json`. `EHAI_VSCODE_MCP` overrides (tests).
    mcp_config: Option<PathBuf>,
    /// `<user-data>/prompts/`. `EHAI_VSCODE_PROMPTS` overrides (tests).
    prompts_dir: Option<PathBuf>,
}

impl VsCode {
    pub fn from_env() -> Self {
        let user_dir = std::env::var_os("EHAI_VSCODE_MCP")
            .map(PathBuf::from)
            .map(|p| p.parent().map(PathBuf::from));
        let mcp_config = std::env::var_os("EHAI_VSCODE_MCP")
            .map(PathBuf::from)
            .or_else(|| default_user_dir().map(|d| d.join("mcp.json")));
        let prompts_dir = std::env::var_os("EHAI_VSCODE_PROMPTS")
            .map(PathBuf::from)
            .or_else(|| {
                // suppress unused warning
                let _ = user_dir;
                default_user_dir().map(|d| d.join("prompts"))
            });
        Self {
            mcp_config,
            prompts_dir,
        }
    }

    pub fn at(mcp_config: PathBuf, prompts_dir: PathBuf) -> Self {
        Self {
            mcp_config: Some(mcp_config),
            prompts_dir: Some(prompts_dir),
        }
    }

    fn mcp_path(&self) -> Result<&Path> {
        self.mcp_config
            .as_deref()
            .ok_or(Error::NotDetected("VS Code"))
    }

    fn prompts_path(&self) -> Result<&Path> {
        self.prompts_dir
            .as_deref()
            .ok_or(Error::NotDetected("VS Code"))
    }

    // VS Code MCP requires `type: "stdio"`
    fn entry(ctx: &Ctx) -> Value {
        json!({
            "type": "stdio",
            "command": ctx.exe.to_string_lossy(),
            "args": ["mcp"]
        })
    }

    fn prompt_files(ctx: &Ctx) -> Vec<File> {
        COMMANDS
            .iter()
            .map(|(name, tpl)| File {
                path: format!("{}-{name}.prompt.md", ctx.brand.name),
                contents: render(tpl, ctx),
            })
            .collect()
    }
}

fn default_user_dir() -> Option<PathBuf> {
    if cfg!(target_os = "macos") {
        std::env::home_dir().map(|h| h.join("Library/Application Support/Code/User"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("Code/User"))
    } else {
        std::env::home_dir().map(|h| h.join(".config/Code/User"))
    }
}

impl Harness for VsCode {
    fn id(&self) -> &'static str {
        "vscode"
    }

    fn display_name(&self) -> &'static str {
        "VS Code"
    }

    fn detect(&self) -> Option<String> {
        crate::find_on_path("code")
            .map(|p| p.display().to_string())
            .or_else(|| {
                #[cfg(target_os = "macos")]
                if std::path::Path::new("/Applications/Visual Studio Code.app").exists() {
                    return Some("/Applications/Visual Studio Code.app".into());
                }
                // user data dir exists
                self.mcp_config
                    .as_ref()
                    .filter(|p| p.parent().is_some_and(Path::is_dir))
                    .map(|p| p.display().to_string())
            })
    }

    fn generate(&self, ctx: &Ctx) -> Vec<File> {
        let mut files = vec![File {
            path: format!("mcp.json#servers.{}", ctx.brand.name),
            contents: serde_json::to_string_pretty(&Self::entry(ctx)).expect("json") + "\n",
        }];
        files.extend(Self::prompt_files(ctx));
        files
    }

    fn install(&self, ctx: &Ctx) -> Result<String> {
        let mcp = self.mcp_path()?;
        let prompts = self.prompts_path()?;
        // VS Code uses `servers` (not `mcpServers`)
        let msg = json_mcp::install(
            mcp,
            "servers",
            &ctx.brand.name,
            Self::entry(ctx),
            &ctx.home,
            "vscode.json",
        )?;
        let changed = sync_tree(prompts, &Self::prompt_files(ctx))?;
        if changed == 0 && msg.contains("already") {
            Ok(format!("`{}` already installed in VS Code", ctx.brand.name))
        } else {
            Ok(format!(
                "{msg}; {changed} prompt file(s) updated in {}",
                prompts.display()
            ))
        }
    }

    fn remove(&self, ctx: &Ctx) -> Result<String> {
        let mcp = self.mcp_path()?;
        let prompts = self.prompts_path()?;
        let msg = json_mcp::remove(mcp, "servers", &ctx.brand.name, &ctx.home, "vscode.json")?;
        sync_tree(prompts, &[])?;
        Ok(msg)
    }

    fn status(&self, ctx: &Ctx) -> Status {
        let Ok(mcp) = self.mcp_path() else {
            return Status::Unavailable;
        };
        if self.detect().is_none() {
            return Status::Unavailable;
        }
        json_mcp::status(mcp, "servers", &ctx.brand.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ehai_core::Brand;

    fn ctx() -> Ctx {
        Ctx {
            brand: Brand::new("ehai").unwrap(),
            role: None,
            home: std::env::temp_dir().join("ehai-vscode-test"),
            exe: "/home/u/.ehai/bin/ehai".into(),
        }
    }

    #[test]
    fn installs_and_removes() {
        let d = tempfile::tempdir().unwrap();
        let mcp = d.path().join("mcp.json");
        let prompts = d.path().join("prompts");
        let v = VsCode::at(mcp.clone(), prompts.clone());
        assert!(v.install(&ctx()).unwrap().contains("registered"));
        assert!(v.install(&ctx()).unwrap().contains("already"), "idempotent");
        let val: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&mcp).unwrap()).unwrap();
        assert_eq!(val["servers"]["ehai"]["type"], "stdio");
        assert_eq!(val["servers"]["ehai"]["args"][0], "mcp");
        assert!(prompts.join("ehai-plan.prompt.md").exists());
        v.remove(&ctx()).unwrap();
        let val: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&mcp).unwrap()).unwrap();
        assert!(val["servers"].get("ehai").is_none());
    }

    #[test]
    fn generate_snapshot() {
        let d = tempfile::tempdir().unwrap();
        let v = VsCode::at(d.path().join("mcp.json"), d.path().join("prompts"));
        let files = v.generate(&ctx());
        let combined: String = files
            .iter()
            .map(|f| format!("=== {} ===\n{}\n", f.path, f.contents))
            .collect();
        insta::assert_snapshot!(combined);
    }
}
