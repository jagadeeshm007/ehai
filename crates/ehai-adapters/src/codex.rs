//! Codex CLI adapter.
//!
//! Registers the MCP server via a `# >>> ehai >>>` / `# <<< ehai <<<` marked
//! block inside `~/.codex/config.toml` and writes prompt files to
//! `~/.codex/prompts/`.
//!
//! Paths verified 2026-11-17 (github.com/openai/codex README and source):
//! config `~/.codex/config.toml` (created by `codex` on first run if absent),
//! prompts dir `~/.codex/prompts/` (custom user instructions loaded at startup).
//! See ADR-0026.

use std::path::{Path, PathBuf};

use crate::{render, sync_tree, Ctx, Error, File, Harness, Result, Status};

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

const OPEN: &str = "# >>> ehai >>>";
const CLOSE: &str = "# <<< ehai <<<";

pub struct Codex {
    /// `~/.codex/config.toml`. `EHAI_CODEX_CONFIG` overrides (tests).
    config: Option<PathBuf>,
    /// `~/.codex/prompts/`. `EHAI_CODEX_PROMPTS` overrides (tests).
    prompts_dir: Option<PathBuf>,
}

impl Codex {
    pub fn from_env() -> Self {
        let home = std::env::home_dir();
        let config = std::env::var_os("EHAI_CODEX_CONFIG")
            .map(PathBuf::from)
            .or_else(|| home.as_ref().map(|h| h.join(".codex/config.toml")));
        let prompts_dir = std::env::var_os("EHAI_CODEX_PROMPTS")
            .map(PathBuf::from)
            .or_else(|| home.map(|h| h.join(".codex/prompts")));
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
        self.config
            .as_deref()
            .ok_or(Error::NotDetected("Codex CLI"))
    }

    fn prompts_path(&self) -> Result<&Path> {
        self.prompts_dir
            .as_deref()
            .ok_or(Error::NotDetected("Codex CLI"))
    }

    fn toml_block(ctx: &Ctx) -> String {
        let exe = ctx.exe.to_string_lossy();
        // escape backslashes and quotes in the path for TOML inline string
        let exe_escaped = exe.replace('\\', "\\\\").replace('"', "\\\"");
        format!(
            "{OPEN}\n[mcp_servers.{}]\ncommand = \"{exe_escaped}\"\nargs = [\"mcp\"]\n{CLOSE}\n",
            ctx.brand.name
        )
    }

    fn set_block(content: &str, block: &str) -> String {
        if let (Some(start), Some(end)) = (content.find(OPEN), content.rfind(CLOSE)) {
            let after = end + CLOSE.len();
            let before = content[..start].trim_end_matches('\n');
            let rest = content[after..].trim_start_matches('\n');
            match (before.is_empty(), rest.is_empty()) {
                (true, true) => block.to_string(),
                (true, false) => format!("{block}\n{rest}"),
                (false, true) => format!("{before}\n{block}"),
                (false, false) => format!("{before}\n{block}\n{rest}"),
            }
        } else {
            let trimmed = content.trim_end();
            if trimmed.is_empty() {
                block.to_string()
            } else {
                format!("{trimmed}\n\n{block}")
            }
        }
    }

    fn remove_block(content: &str) -> String {
        if let (Some(start), Some(end)) = (content.find(OPEN), content.rfind(CLOSE)) {
            let after = end + CLOSE.len();
            let before = content[..start].trim_end_matches('\n');
            let rest = content[after..].trim_start_matches('\n');
            if rest.is_empty() {
                if before.is_empty() {
                    String::new()
                } else {
                    format!("{before}\n")
                }
            } else {
                format!("{before}\n\n{rest}")
            }
        } else {
            content.to_string()
        }
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

impl Harness for Codex {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn display_name(&self) -> &'static str {
        "Codex CLI"
    }

    fn detect(&self) -> Option<String> {
        crate::find_on_path("codex")
            .map(|p| p.display().to_string())
            .or_else(|| {
                self.config
                    .as_ref()
                    .filter(|p| p.parent().is_some_and(Path::is_dir))
                    .map(|p| p.display().to_string())
            })
    }

    fn generate(&self, ctx: &Ctx) -> Vec<File> {
        let block = Self::toml_block(ctx);
        let mut files = vec![File {
            path: "config.toml#mcp_servers".into(),
            contents: block,
        }];
        files.extend(Self::prompt_files(ctx));
        files
    }

    fn install(&self, ctx: &Ctx) -> Result<String> {
        let config = self.config_path()?;
        let prompts = self.prompts_path()?;

        let existing = match std::fs::read_to_string(config) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(ehai_core::Error::io(config)(e).into()),
        };
        let block = Self::toml_block(ctx);
        // idempotency: check if the exact block is already present
        if existing.contains(&block) {
            let changed = sync_tree(prompts, &Self::prompt_files(ctx))?;
            if changed == 0 {
                return Ok(format!(
                    "`{}` already registered in Codex CLI config",
                    ctx.brand.name
                ));
            }
            return Ok(format!(
                "`{}` config unchanged; {changed} prompt file(s) updated",
                ctx.brand.name
            ));
        }
        // back up before first edit
        if config.is_file() {
            let backup = config.with_extension("toml.ehai-backup");
            if !backup.exists() {
                std::fs::copy(config, &backup).map_err(ehai_core::Error::io(&backup))?;
            }
        } else if let Some(dir) = config.parent() {
            std::fs::create_dir_all(dir).map_err(ehai_core::Error::io(dir))?;
        }
        let updated = Self::set_block(&existing, &block);
        ehai_core::migrate::write_atomic(config, updated.as_bytes())?;
        let changed = sync_tree(prompts, &Self::prompt_files(ctx))?;
        Ok(format!(
            "registered `{}` in {}; {changed} prompt file(s) updated",
            ctx.brand.name,
            config.display()
        ))
    }

    fn remove(&self, ctx: &Ctx) -> Result<String> {
        let Ok(config) = self.config_path() else {
            return Ok("Codex CLI not found; nothing to remove".into());
        };
        let Ok(prompts) = self.prompts_path() else {
            return Ok("Codex CLI not found; nothing to remove".into());
        };
        let existing = match std::fs::read_to_string(config) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(format!("`{}` was not registered", ctx.brand.name))
            }
            Err(e) => return Err(ehai_core::Error::io(config)(e).into()),
        };
        if !existing.contains(OPEN) {
            return Ok(format!("`{}` was not registered", ctx.brand.name));
        }
        let updated = Self::remove_block(&existing);
        if updated.trim().is_empty() {
            let _ = std::fs::remove_file(config);
        } else {
            ehai_core::migrate::write_atomic(config, updated.as_bytes())?;
        }
        sync_tree(prompts, &[])?;
        Ok(format!(
            "removed `{}` from Codex CLI config",
            ctx.brand.name
        ))
    }

    fn status(&self, ctx: &Ctx) -> Status {
        let Ok(config) = self.config_path() else {
            return Status::Unavailable;
        };
        if self.detect().is_none() {
            return Status::Unavailable;
        }
        let present = std::fs::read_to_string(config)
            .ok()
            .map(|s| s.contains(OPEN) && s.contains(&ctx.brand.name))
            .unwrap_or(false);
        if present {
            Status::Installed
        } else {
            Status::NotInstalled
        }
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
            home: std::env::temp_dir().join("ehai-codex-test"),
            exe: "/home/u/.ehai/bin/ehai".into(),
        }
    }

    #[test]
    fn installs_into_empty_and_existing_config() {
        let d = tempfile::tempdir().unwrap();
        let config = d.path().join("config.toml");
        let prompts = d.path().join("prompts");
        let c = Codex::at(config.clone(), prompts.clone());

        assert!(c.install(&ctx()).unwrap().contains("registered"));
        assert!(c.install(&ctx()).unwrap().contains("already"), "idempotent");
        // detect() returns Some because the config's parent tempdir exists
        assert_eq!(c.status(&ctx()), Status::Installed);
        let text = std::fs::read_to_string(&config).unwrap();
        assert!(text.contains("[mcp_servers.ehai]"), "block present");
        assert!(text.contains("command = "));
        assert!(prompts.join("ehai-plan.md").exists());

        // preserves surrounding content
        std::fs::write(&config, format!("[model]\nname = \"o3\"\n\n{text}")).unwrap();
        c.remove(&ctx()).unwrap();
        let after = std::fs::read_to_string(&config).unwrap();
        assert!(after.contains("[model]"), "other content preserved");
        assert!(!after.contains("[mcp_servers.ehai]"), "block removed");
    }

    #[test]
    fn backup_created_before_first_edit() {
        let d = tempfile::tempdir().unwrap();
        let config = d.path().join("config.toml");
        std::fs::write(&config, "[model]\nname = \"o3\"\n").unwrap();
        let c = Codex::at(config.clone(), d.path().join("p"));
        c.install(&ctx()).unwrap();
        assert!(config.with_extension("toml.ehai-backup").exists());
    }

    #[test]
    fn generate_snapshot() {
        let d = tempfile::tempdir().unwrap();
        let c = Codex::at(d.path().join("config.toml"), d.path().join("prompts"));
        let files = c.generate(&ctx());
        let combined: String = files
            .iter()
            .map(|f| format!("=== {} ===\n{}\n", f.path, f.contents))
            .collect();
        insta::assert_snapshot!(combined);
    }
}
