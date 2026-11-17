//! Windsurf adapter.
//!
//! Registers the MCP server under `mcpServers.<brand>` in Windsurf's MCP
//! config and writes workflow files to the global Windsurf workflows dir.
//!
//! Paths verified 2026-11-17 (codeium.com/windsurf/mcp and Windsurf settings):
//! MCP config `~/.codeium/windsurf/mcp_server_config.json`,
//! workflows dir `~/.codeium/windsurf/workflows/`.
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

pub struct Windsurf {
    /// `~/.codeium/windsurf/mcp_server_config.json`. `EHAI_WINDSURF_CONFIG` overrides.
    config: Option<PathBuf>,
    /// `~/.codeium/windsurf/workflows/`. `EHAI_WINDSURF_WORKFLOWS` overrides.
    workflows_dir: Option<PathBuf>,
}

impl Windsurf {
    pub fn from_env() -> Self {
        let home = std::env::home_dir();
        let config = std::env::var_os("EHAI_WINDSURF_CONFIG")
            .map(PathBuf::from)
            .or_else(|| {
                home.as_ref()
                    .map(|h| h.join(".codeium/windsurf/mcp_server_config.json"))
            });
        let workflows_dir = std::env::var_os("EHAI_WINDSURF_WORKFLOWS")
            .map(PathBuf::from)
            .or_else(|| home.map(|h| h.join(".codeium/windsurf/workflows")));
        Self {
            config,
            workflows_dir,
        }
    }

    pub fn at(config: PathBuf, workflows_dir: PathBuf) -> Self {
        Self {
            config: Some(config),
            workflows_dir: Some(workflows_dir),
        }
    }

    fn config_path(&self) -> Result<&Path> {
        self.config.as_deref().ok_or(Error::NotDetected("Windsurf"))
    }

    fn workflows_path(&self) -> Result<&Path> {
        self.workflows_dir
            .as_deref()
            .ok_or(Error::NotDetected("Windsurf"))
    }

    fn entry(ctx: &Ctx) -> Value {
        json!({ "command": ctx.exe.to_string_lossy(), "args": ["mcp"] })
    }

    fn workflow_files(ctx: &Ctx) -> Vec<File> {
        COMMANDS
            .iter()
            .map(|(name, tpl)| File {
                path: format!("{}-{name}.md", ctx.brand.name),
                contents: render(tpl, ctx),
            })
            .collect()
    }
}

impl Harness for Windsurf {
    fn id(&self) -> &'static str {
        "windsurf"
    }

    fn display_name(&self) -> &'static str {
        "Windsurf"
    }

    fn detect(&self) -> Option<String> {
        crate::find_on_path("windsurf")
            .map(|p| p.display().to_string())
            .or_else(|| {
                #[cfg(target_os = "macos")]
                if std::path::Path::new("/Applications/Windsurf.app").exists() {
                    return Some("/Applications/Windsurf.app".into());
                }
                self.config
                    .as_ref()
                    .filter(|p| p.parent().is_some_and(Path::is_dir))
                    .map(|p| p.display().to_string())
            })
    }

    fn generate(&self, ctx: &Ctx) -> Vec<File> {
        let mut files = vec![File {
            path: format!("mcp_server_config.json#mcpServers.{}", ctx.brand.name),
            contents: serde_json::to_string_pretty(&Self::entry(ctx)).expect("json") + "\n",
        }];
        files.extend(Self::workflow_files(ctx));
        files
    }

    fn install(&self, ctx: &Ctx) -> Result<String> {
        let config = self.config_path()?;
        let workflows = self.workflows_path()?;
        let msg = json_mcp::install(
            config,
            "mcpServers",
            &ctx.brand.name,
            Self::entry(ctx),
            &ctx.home,
            "windsurf.json",
        )?;
        let changed = sync_tree(workflows, &Self::workflow_files(ctx))?;
        if changed == 0 && msg.contains("already") {
            Ok(format!(
                "`{}` already installed in Windsurf",
                ctx.brand.name
            ))
        } else {
            Ok(format!(
                "{msg}; {changed} workflow file(s) updated in {}",
                workflows.display()
            ))
        }
    }

    fn remove(&self, ctx: &Ctx) -> Result<String> {
        let config = self.config_path()?;
        let workflows = self.workflows_path()?;
        let msg = json_mcp::remove(
            config,
            "mcpServers",
            &ctx.brand.name,
            &ctx.home,
            "windsurf.json",
        )?;
        sync_tree(workflows, &[])?;
        Ok(msg)
    }

    fn status(&self, ctx: &Ctx) -> Status {
        let Ok(config) = self.config_path() else {
            return Status::Unavailable;
        };
        if self.detect().is_none() {
            return Status::Unavailable;
        }
        json_mcp::status(config, "mcpServers", &ctx.brand.name)
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
            home: std::env::temp_dir().join("ehai-windsurf-test"),
            exe: "/home/u/.ehai/bin/ehai".into(),
        }
    }

    #[test]
    fn installs_and_removes() {
        let d = tempfile::tempdir().unwrap();
        let config = d.path().join("mcp_server_config.json");
        let wf = d.path().join("workflows");
        let w = Windsurf::at(config.clone(), wf.clone());
        assert!(w.install(&ctx()).unwrap().contains("registered"));
        assert!(w.install(&ctx()).unwrap().contains("already"), "idempotent");
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
        assert_eq!(v["mcpServers"]["ehai"]["args"][0], "mcp");
        assert!(wf.join("ehai-plan.md").exists());
        w.remove(&ctx()).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
        assert!(v["mcpServers"].get("ehai").is_none());
    }

    #[test]
    fn generate_snapshot() {
        let d = tempfile::tempdir().unwrap();
        let w = Windsurf::at(d.path().join("mcp.json"), d.path().join("wf"));
        let files = w.generate(&ctx());
        let combined: String = files
            .iter()
            .map(|f| format!("=== {} ===\n{}\n", f.path, f.contents))
            .collect();
        insta::assert_snapshot!(combined);
    }
}
