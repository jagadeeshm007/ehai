//! Cursor adapter.
//!
//! Registers the MCP server under `mcpServers.<brand>` in `~/.cursor/mcp.json`
//! and writes rule files (`.mdc`) to `~/.cursor/rules/`.
//!
//! Paths verified 2026-11-17 (cursor.com/docs/cmdk/mcp and cursor.com/docs/rules):
//! MCP config `~/.cursor/mcp.json`, rules dir `~/.cursor/rules/`.
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

/// Cursor `.mdc` frontmatter + body.
fn mdc(_name: &str, description: &str, body: &str) -> String {
    format!("---\ndescription: {description}\nglobs: \nalwaysApply: false\n---\n\n{body}")
}

pub struct Cursor {
    /// `~/.cursor/mcp.json`. `EHAI_CURSOR_CONFIG` overrides (tests).
    config: Option<PathBuf>,
    /// `~/.cursor/rules/`. `EHAI_CURSOR_RULES` overrides (tests).
    rules_dir: Option<PathBuf>,
}

impl Cursor {
    pub fn from_env() -> Self {
        let home = std::env::home_dir();
        let config = std::env::var_os("EHAI_CURSOR_CONFIG")
            .map(PathBuf::from)
            .or_else(|| home.as_ref().map(|h| h.join(".cursor/mcp.json")));
        let rules_dir = std::env::var_os("EHAI_CURSOR_RULES")
            .map(PathBuf::from)
            .or_else(|| home.map(|h| h.join(".cursor/rules")));
        Self { config, rules_dir }
    }

    pub fn at(config: PathBuf, rules_dir: PathBuf) -> Self {
        Self {
            config: Some(config),
            rules_dir: Some(rules_dir),
        }
    }

    fn config_path(&self) -> Result<&Path> {
        self.config.as_deref().ok_or(Error::NotDetected("Cursor"))
    }

    fn rules_path(&self) -> Result<&Path> {
        self.rules_dir
            .as_deref()
            .ok_or(Error::NotDetected("Cursor"))
    }

    fn entry(ctx: &Ctx) -> Value {
        json!({ "command": ctx.exe.to_string_lossy(), "args": ["mcp"] })
    }

    fn rule_files(ctx: &Ctx) -> Vec<File> {
        COMMANDS
            .iter()
            .map(|(name, tpl)| {
                let body = render(tpl, ctx);
                // extract first line as description (strip leading "# ")
                let desc = body
                    .lines()
                    .find(|l| l.starts_with("# "))
                    .map(|l| l.trim_start_matches("# "))
                    .unwrap_or(*name);
                File {
                    path: format!("{}-{name}.mdc", ctx.brand.name),
                    contents: mdc(name, desc, &body),
                }
            })
            .collect()
    }
}

impl Harness for Cursor {
    fn id(&self) -> &'static str {
        "cursor"
    }

    fn display_name(&self) -> &'static str {
        "Cursor"
    }

    fn detect(&self) -> Option<String> {
        crate::find_on_path("cursor")
            .map(|p| p.display().to_string())
            .or_else(|| {
                // macOS app
                #[cfg(target_os = "macos")]
                if std::path::Path::new("/Applications/Cursor.app").exists() {
                    return Some("/Applications/Cursor.app".into());
                }
                // config dir already exists
                self.config
                    .as_ref()
                    .filter(|p| p.parent().is_some_and(Path::is_dir))
                    .map(|p| p.display().to_string())
            })
    }

    fn generate(&self, ctx: &Ctx) -> Vec<File> {
        let mut files = vec![File {
            path: format!("mcp.json#mcpServers.{}", ctx.brand.name),
            contents: serde_json::to_string_pretty(&Self::entry(ctx)).expect("json") + "\n",
        }];
        files.extend(Self::rule_files(ctx));
        files
    }

    fn install(&self, ctx: &Ctx) -> Result<String> {
        let config = self.config_path()?;
        let rules = self.rules_path()?;
        let msg = json_mcp::install(
            config,
            "mcpServers",
            &ctx.brand.name,
            Self::entry(ctx),
            &ctx.home,
            "cursor.json",
        )?;
        let changed = sync_tree(rules, &Self::rule_files(ctx))?;
        if changed == 0 && msg.contains("already") {
            Ok(format!("`{}` already installed in Cursor", ctx.brand.name))
        } else {
            Ok(format!(
                "{msg}; {changed} rule file(s) updated in {}",
                rules.display()
            ))
        }
    }

    fn remove(&self, ctx: &Ctx) -> Result<String> {
        let config = self.config_path()?;
        let rules = self.rules_path()?;
        let msg = json_mcp::remove(
            config,
            "mcpServers",
            &ctx.brand.name,
            &ctx.home,
            "cursor.json",
        )?;
        sync_tree(rules, &[])?;
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
            home: std::env::temp_dir().join("ehai-cursor-test"),
            exe: "/home/u/.ehai/bin/ehai".into(),
        }
    }

    #[test]
    fn installs_and_removes() {
        let d = tempfile::tempdir().unwrap();
        let config = d.path().join("mcp.json");
        let rules = d.path().join("rules");
        let c = Cursor::at(config.clone(), rules.clone());
        assert!(c.install(&ctx()).unwrap().contains("registered"));
        assert!(c.install(&ctx()).unwrap().contains("already"), "idempotent");
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
        assert_eq!(v["mcpServers"]["ehai"]["args"][0], "mcp");
        assert!(rules.join("ehai-plan.mdc").exists());
        c.remove(&ctx()).unwrap();
        let v: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&config).unwrap()).unwrap();
        assert!(v["mcpServers"].get("ehai").is_none());
    }

    #[test]
    fn generate_snapshot() {
        let d = tempfile::tempdir().unwrap();
        let c = Cursor::at(d.path().join("mcp.json"), d.path().join("rules"));
        let files = c.generate(&ctx());
        let combined: String = files
            .iter()
            .map(|f| format!("=== {} ===\n{}\n", f.path, f.contents))
            .collect();
        insta::assert_snapshot!(combined);
    }
}
