//! Gemini CLI adapter.
//!
//! Registers the MCP server under `mcpServers.<brand>` in
//! `~/.gemini/settings.json` and writes TOML command files to
//! `~/.gemini/commands/`.
//!
//! Paths verified 2026-11-17 (google-gemini/gemini-cli README and docs):
//! config `~/.gemini/settings.json`, commands dir `~/.gemini/commands/`.
//! See ADR-0026.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::{json_mcp, render, sync_tree, Ctx, Error, File, Harness, Result, Status};

const COMMANDS: &[(&str, &str)] = &[
    (
        "help",
        include_str!("../../../templates/gemini/commands/help.toml"),
    ),
    (
        "plan",
        include_str!("../../../templates/gemini/commands/plan.toml"),
    ),
    (
        "breakdown",
        include_str!("../../../templates/gemini/commands/breakdown.toml"),
    ),
    (
        "sync",
        include_str!("../../../templates/gemini/commands/sync.toml"),
    ),
    (
        "status",
        include_str!("../../../templates/gemini/commands/status.toml"),
    ),
    (
        "triage",
        include_str!("../../../templates/gemini/commands/triage.toml"),
    ),
    (
        "assign",
        include_str!("../../../templates/gemini/commands/assign.toml"),
    ),
    (
        "prefs",
        include_str!("../../../templates/gemini/commands/prefs.toml"),
    ),
];

pub struct Gemini {
    /// `~/.gemini/settings.json`. `EHAI_GEMINI_CONFIG` overrides (tests).
    config: Option<PathBuf>,
    /// `~/.gemini/commands/`. `EHAI_GEMINI_COMMANDS` overrides (tests).
    commands_dir: Option<PathBuf>,
}

impl Gemini {
    pub fn from_env() -> Self {
        let home = std::env::home_dir();
        let config = std::env::var_os("EHAI_GEMINI_CONFIG")
            .map(PathBuf::from)
            .or_else(|| home.as_ref().map(|h| h.join(".gemini/settings.json")));
        let commands_dir = std::env::var_os("EHAI_GEMINI_COMMANDS")
            .map(PathBuf::from)
            .or_else(|| home.map(|h| h.join(".gemini/commands")));
        Self {
            config,
            commands_dir,
        }
    }

    pub fn at(config: PathBuf, commands_dir: PathBuf) -> Self {
        Self {
            config: Some(config),
            commands_dir: Some(commands_dir),
        }
    }

    fn config_path(&self) -> Result<&Path> {
        self.config
            .as_deref()
            .ok_or(Error::NotDetected("Gemini CLI"))
    }

    fn commands_path(&self) -> Result<&Path> {
        self.commands_dir
            .as_deref()
            .ok_or(Error::NotDetected("Gemini CLI"))
    }

    fn entry(ctx: &Ctx) -> Value {
        json!({ "command": ctx.exe.to_string_lossy(), "args": ["mcp"] })
    }

    fn command_files(ctx: &Ctx) -> Vec<File> {
        COMMANDS
            .iter()
            .map(|(name, tpl)| File {
                path: format!("{}-{name}.toml", ctx.brand.name),
                contents: render(tpl, ctx),
            })
            .collect()
    }
}

impl Harness for Gemini {
    fn id(&self) -> &'static str {
        "gemini"
    }

    fn display_name(&self) -> &'static str {
        "Gemini CLI"
    }

    fn detect(&self) -> Option<String> {
        crate::find_on_path("gemini")
            .map(|p| p.display().to_string())
            .or_else(|| {
                // also "detected" if the config dir already exists
                self.config
                    .as_ref()
                    .filter(|p| p.parent().is_some_and(Path::is_dir))
                    .map(|p| p.display().to_string())
            })
    }

    fn generate(&self, ctx: &Ctx) -> Vec<File> {
        let mut files = vec![File {
            path: format!("settings.json#mcpServers.{}", ctx.brand.name),
            contents: serde_json::to_string_pretty(&Self::entry(ctx)).expect("json") + "\n",
        }];
        files.extend(Self::command_files(ctx));
        files
    }

    fn install(&self, ctx: &Ctx) -> Result<String> {
        let config_path = self.config_path()?;
        let commands_path = self.commands_path()?;
        let msg = json_mcp::install(
            config_path,
            "mcpServers",
            &ctx.brand.name,
            Self::entry(ctx),
            &ctx.home,
            "gemini.json",
        )?;
        let changed = sync_tree(commands_path, &Self::command_files(ctx))?;
        if changed == 0 && msg.contains("already") {
            Ok(format!(
                "`{}` already installed in Gemini CLI",
                ctx.brand.name
            ))
        } else {
            Ok(format!(
                "{msg}; {changed} command file(s) updated in {}",
                commands_path.display()
            ))
        }
    }

    fn remove(&self, ctx: &Ctx) -> Result<String> {
        let config_path = self.config_path()?;
        let commands_path = self.commands_path()?;
        let msg = json_mcp::remove(
            config_path,
            "mcpServers",
            &ctx.brand.name,
            &ctx.home,
            "gemini.json",
        )?;
        // Remove all brand command files
        sync_tree(commands_path, &[])?;
        Ok(msg)
    }

    fn status(&self, ctx: &Ctx) -> Status {
        let Ok(config_path) = self.config_path() else {
            return Status::Unavailable;
        };
        if self.detect().is_none() {
            return Status::Unavailable;
        }
        json_mcp::status(config_path, "mcpServers", &ctx.brand.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ehai_core::Brand;

    fn ctx() -> Ctx {
        Ctx {
            brand: Brand::new("ehai").unwrap(),
            role: Some("both".into()),
            home: std::env::temp_dir().join("ehai-gemini-test"),
            exe: "/home/u/.ehai/bin/ehai".into(),
        }
    }

    #[test]
    fn installs_and_removes() {
        let d = tempfile::tempdir().unwrap();
        let config = d.path().join("settings.json");
        let cmds = d.path().join("commands");
        let g = Gemini::at(config.clone(), cmds.clone());
        assert!(g.install(&ctx()).unwrap().contains("registered"));
        assert!(g.install(&ctx()).unwrap().contains("already"), "idempotent");
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
        assert_eq!(v["mcpServers"]["ehai"]["args"][0], "mcp");
        assert!(cmds.join("ehai-plan.toml").exists());
        g.remove(&ctx()).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
        assert!(v["mcpServers"].get("ehai").is_none());
        assert!(!cmds.exists() || cmds.read_dir().unwrap().next().is_none());
    }

    #[test]
    fn generate_snapshot() {
        let d = tempfile::tempdir().unwrap();
        let g = Gemini::at(d.path().join("s.json"), d.path().join("cmds"));
        let files = g.generate(&ctx());
        let combined: String = files
            .iter()
            .map(|f| format!("=== {} ===\n{}\n", f.path, f.contents))
            .collect();
        insta::assert_snapshot!(combined);
    }
}
