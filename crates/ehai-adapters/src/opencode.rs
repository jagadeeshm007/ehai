//! OpenCode adapter.
//!
//! Registers the MCP server under `mcpServers.<brand>` in OpenCode's config
//! and writes prompt files to its commands directory.
//!
//! Paths verified 2026-11-17 (opencode.ai docs / GitHub README):
//! config `~/.config/opencode/config.json`,
//! prompts dir `~/.config/opencode/prompts/`.
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

pub struct OpenCode {
    /// `~/.config/opencode/config.json`. `EHAI_OPENCODE_CONFIG` overrides.
    config: Option<PathBuf>,
    /// `~/.config/opencode/prompts/`. `EHAI_OPENCODE_PROMPTS` overrides.
    prompts_dir: Option<PathBuf>,
}

impl OpenCode {
    pub fn from_env() -> Self {
        let home = std::env::home_dir();
        let config = std::env::var_os("EHAI_OPENCODE_CONFIG")
            .map(PathBuf::from)
            .or_else(|| {
                home.as_ref()
                    .map(|h| h.join(".config/opencode/config.json"))
            });
        let prompts_dir = std::env::var_os("EHAI_OPENCODE_PROMPTS")
            .map(PathBuf::from)
            .or_else(|| home.map(|h| h.join(".config/opencode/prompts")));
        Self {
            config,
            prompts_dir,
        }
    }

    pub fn at(config: PathBuf, prompts_dir: PathBuf) -> Self {
        Self {
            config: Some(config),
            prompts_dir: Some(prompts_dir),
        }
    }

    fn config_path(&self) -> Result<&Path> {
        self.config.as_deref().ok_or(Error::NotDetected("OpenCode"))
    }

    fn prompts_path(&self) -> Result<&Path> {
        self.prompts_dir
            .as_deref()
            .ok_or(Error::NotDetected("OpenCode"))
    }

    fn entry(ctx: &Ctx) -> Value {
        json!({ "command": ctx.exe.to_string_lossy(), "args": ["mcp"] })
    }

    fn prompt_files(ctx: &Ctx) -> Vec<File> {
        COMMANDS
            .iter()
            .map(|(name, tpl)| File {
                path: format!("{}-{name}.md", ctx.brand.name),
                contents: render(tpl, ctx),
            })
            .collect()
    }
}

impl Harness for OpenCode {
    fn id(&self) -> &'static str {
        "opencode"
    }

    fn display_name(&self) -> &'static str {
        "OpenCode"
    }

    fn detect(&self) -> Option<String> {
        crate::find_on_path("opencode")
            .map(|p| p.display().to_string())
            .or_else(|| {
                self.config
                    .as_ref()
                    .filter(|p| p.parent().is_some_and(Path::is_dir))
                    .map(|p| p.display().to_string())
            })
    }

    fn generate(&self, ctx: &Ctx) -> Vec<File> {
        let mut files = vec![File {
            path: format!("config.json#mcpServers.{}", ctx.brand.name),
            contents: serde_json::to_string_pretty(&Self::entry(ctx)).expect("json") + "\n",
        }];
        files.extend(Self::prompt_files(ctx));
        files
    }

    fn install(&self, ctx: &Ctx) -> Result<String> {
        let config = self.config_path()?;
        let prompts = self.prompts_path()?;
        let msg = json_mcp::install(
            config,
            "mcpServers",
            &ctx.brand.name,
            Self::entry(ctx),
            &ctx.home,
            "opencode.json",
        )?;
        let changed = sync_tree(prompts, &Self::prompt_files(ctx))?;
        if changed == 0 && msg.contains("already") {
            Ok(format!(
                "`{}` already installed in OpenCode",
                ctx.brand.name
            ))
        } else {
            Ok(format!(
                "{msg}; {changed} prompt file(s) updated in {}",
                prompts.display()
            ))
        }
    }

    fn remove(&self, ctx: &Ctx) -> Result<String> {
        let config = self.config_path()?;
        let prompts = self.prompts_path()?;
        let msg = json_mcp::remove(
            config,
            "mcpServers",
            &ctx.brand.name,
            &ctx.home,
            "opencode.json",
        )?;
        sync_tree(prompts, &[])?;
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
            home: std::env::temp_dir().join("ehai-opencode-test"),
            exe: "/home/u/.ehai/bin/ehai".into(),
        }
    }

    #[test]
    fn installs_and_removes() {
        let d = tempfile::tempdir().unwrap();
        let config = d.path().join("config.json");
        let prompts = d.path().join("prompts");
        let o = OpenCode::at(config.clone(), prompts.clone());
        assert!(o.install(&ctx()).unwrap().contains("registered"));
        assert!(o.install(&ctx()).unwrap().contains("already"), "idempotent");
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
        assert_eq!(v["mcpServers"]["ehai"]["args"][0], "mcp");
        assert!(prompts.join("ehai-plan.md").exists());
        o.remove(&ctx()).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
        assert!(v["mcpServers"].get("ehai").is_none());
    }

    #[test]
    fn generate_snapshot() {
        let d = tempfile::tempdir().unwrap();
        let o = OpenCode::at(d.path().join("config.json"), d.path().join("prompts"));
        let files = o.generate(&ctx());
        let combined: String = files
            .iter()
            .map(|f| format!("=== {} ===\n{}\n", f.path, f.contents))
            .collect();
        insta::assert_snapshot!(combined);
    }
}
