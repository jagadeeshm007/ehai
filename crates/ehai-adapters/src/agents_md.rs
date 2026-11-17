//! AGENTS.md adapter.
//!
//! Universal fallback for any harness that reads AGENTS.md. Writes a marked
//! block (`<!-- >>> ehai >>> -->` / `<!-- <<< ehai <<< -->`) into `~/AGENTS.md`.
//! This is a "file I'll keep editing" adapter, not an MCP registration — the
//! harness reads the markdown prose; there's no config key to set.
//!
//! Paths / format: N/A (always writes to ~/AGENTS.md). See ADR-0026.

use std::path::{Path, PathBuf};

use crate::{render, Ctx, Error, File, Harness, Result, Status};

const TEMPLATE: &str = include_str!("../../../templates/agents-md/AGENTS.md");
const OPEN: &str = "<!-- >>> ehai >>> -->";
const CLOSE: &str = "<!-- <<< ehai <<< -->";

pub struct AgentsMd {
    /// `~/AGENTS.md` or `EHAI_AGENTS_MD` override (tests).
    path: Option<PathBuf>,
}

impl AgentsMd {
    pub fn from_env() -> Self {
        let path = std::env::var_os("EHAI_AGENTS_MD")
            .map(PathBuf::from)
            .or_else(|| std::env::home_dir().map(|h| h.join("AGENTS.md")));
        Self { path }
    }

    pub fn at(path: PathBuf) -> Self {
        Self { path: Some(path) }
    }

    fn path(&self) -> Result<&Path> {
        self.path
            .as_deref()
            .ok_or(Error::NotDetected("AGENTS.md (home directory not found)"))
    }

    fn block(ctx: &Ctx) -> String {
        let content = render(TEMPLATE, ctx);
        format!("{OPEN}\n{}\n{CLOSE}\n", content.trim_end())
    }

    fn set_block(existing: &str, block: &str) -> String {
        if let (Some(start), Some(end)) = (existing.find(OPEN), existing.rfind(CLOSE)) {
            let after = end + CLOSE.len();
            let before = existing[..start].trim_end_matches('\n');
            let rest = existing[after..].trim_start_matches('\n');
            match (before.is_empty(), rest.is_empty()) {
                (true, true) => block.to_string(),
                (true, false) => format!("{block}\n{rest}"),
                (false, true) => format!("{before}\n{block}"),
                (false, false) => format!("{before}\n{block}\n{rest}"),
            }
        } else {
            let trimmed = existing.trim_end();
            if trimmed.is_empty() {
                block.to_string()
            } else {
                format!("{trimmed}\n\n{block}")
            }
        }
    }

    fn remove_block(existing: &str) -> String {
        if let (Some(start), Some(end)) = (existing.find(OPEN), existing.rfind(CLOSE)) {
            let after = end + CLOSE.len();
            let before = existing[..start].trim_end_matches('\n');
            let rest = existing[after..].trim_start_matches('\n');
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
            existing.to_string()
        }
    }
}

impl Harness for AgentsMd {
    fn id(&self) -> &'static str {
        "agents-md"
    }

    fn display_name(&self) -> &'static str {
        "AGENTS.md (universal)"
    }

    /// Always detected — writing a markdown file never requires a harness.
    fn detect(&self) -> Option<String> {
        self.path.as_ref().map(|p| p.display().to_string())
    }

    fn generate(&self, ctx: &Ctx) -> Vec<File> {
        vec![File {
            path: "AGENTS.md".into(),
            contents: Self::block(ctx),
        }]
    }

    fn install(&self, ctx: &Ctx) -> Result<String> {
        let path = self.path()?;
        let existing = std::fs::read_to_string(path).unwrap_or_default();
        let block = Self::block(ctx);
        let updated = Self::set_block(&existing, &block);
        if updated == existing {
            return Ok(format!("`{}` already in AGENTS.md", ctx.brand.name));
        }
        ehai_core::migrate::write_atomic(path, updated.as_bytes())?;
        Ok(format!(
            "wrote `{}` block to {}",
            ctx.brand.name,
            path.display()
        ))
    }

    fn remove(&self, ctx: &Ctx) -> Result<String> {
        let Ok(path) = self.path() else {
            return Ok("AGENTS.md not found; nothing to remove".into());
        };
        let existing = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(format!("`{}` was not in AGENTS.md", ctx.brand.name))
            }
            Err(e) => return Err(ehai_core::Error::io(path)(e).into()),
        };
        let updated = Self::remove_block(&existing);
        if updated == existing {
            return Ok(format!("`{}` was not in AGENTS.md", ctx.brand.name));
        }
        if updated.trim().is_empty() {
            let _ = std::fs::remove_file(path);
        } else {
            ehai_core::migrate::write_atomic(path, updated.as_bytes())?;
        }
        Ok(format!("removed `{}` from AGENTS.md", ctx.brand.name))
    }

    fn status(&self, ctx: &Ctx) -> Status {
        let Ok(path) = self.path() else {
            return Status::Unavailable;
        };
        let present = std::fs::read_to_string(path)
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
            role: Some("both".into()),
            home: std::env::temp_dir().join("ehai-agents-md-test"),
            exe: "/home/u/.ehai/bin/ehai".into(),
        }
    }

    #[test]
    fn installs_into_empty_file_and_removes() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("AGENTS.md");
        let a = AgentsMd::at(path.clone());
        assert_eq!(a.status(&ctx()), Status::NotInstalled);
        assert!(a.install(&ctx()).unwrap().contains("wrote"));
        assert!(a.install(&ctx()).unwrap().contains("already"), "idempotent");
        assert_eq!(a.status(&ctx()), Status::Installed);
        assert!(std::fs::read_to_string(&path).unwrap().contains(OPEN));
        a.remove(&ctx()).unwrap();
        assert_eq!(a.status(&ctx()), Status::NotInstalled);
        assert!(!path.exists(), "empty file removed");
    }

    #[test]
    fn preserves_surrounding_content() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("AGENTS.md");
        let preamble = "# My project\nSome notes.\n";
        std::fs::write(&path, preamble).unwrap();
        AgentsMd::at(path.clone()).install(&ctx()).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("# My project"), "preamble kept");
        assert!(text.contains(OPEN));
        AgentsMd::at(path.clone()).remove(&ctx()).unwrap();
        let after = std::fs::read_to_string(&path).unwrap();
        assert!(after.trim() == preamble.trim(), "preamble restored");
    }

    #[test]
    fn generate_snapshot() {
        let files = AgentsMd::at("/x/AGENTS.md".into()).generate(&ctx());
        insta::assert_snapshot!(format!("=== {} ===\n{}", files[0].path, files[0].contents));
    }
}
