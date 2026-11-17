//! Claude desktop adapter.
//!
//! Registers the MCP server under `mcpServers.<brand>` in
//! `claude_desktop_config.json`. Only that one key is ever touched; every other
//! key keeps its value and order, and a `.ehai-backup` is taken before the first
//! edit. The PM flows reach the desktop app as MCP *prompts* served by the same
//! server — Claude apps only accept custom skills as ZIP uploads, so prompts are
//! the zero-step route. Claude desktop reads the file at start-up: changes need a
//! full quit and relaunch.
//!
//! Paths verified 2026-11-17 (support.claude.com, modelcontextprotocol.io and the
//! file on Linux): macOS `~/Library/Application Support/Claude/`, Windows
//! `%APPDATA%\Claude\`, Linux `~/.config/Claude/`. See DECISIONS ADR-0021.

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use crate::{Ctx, Error, File, Harness, Result, Status};

pub struct ClaudeDesktop {
    /// `claude_desktop_config.json`. `EHAI_CLAUDE_DESKTOP_CONFIG` overrides (tests).
    config: Option<PathBuf>,
}

impl ClaudeDesktop {
    pub fn from_env() -> Self {
        let config = std::env::var_os("EHAI_CLAUDE_DESKTOP_CONFIG")
            .map(PathBuf::from)
            .or_else(default_path);
        Self { config }
    }

    pub fn at(config: PathBuf) -> Self {
        Self {
            config: Some(config),
        }
    }

    fn path(&self) -> Result<&Path> {
        self.config
            .as_deref()
            .filter(|p| p.parent().is_some_and(Path::is_dir))
            .ok_or(Error::NotDetected("Claude desktop"))
    }

    fn entry(ctx: &Ctx) -> Value {
        json!({ "command": ctx.exe.to_string_lossy(), "args": ["mcp"] })
    }

    fn read(path: &Path) -> Result<Map<String, Value>> {
        match std::fs::read_to_string(path) {
            Ok(text) if text.trim().is_empty() => Ok(Map::new()),
            Ok(text) => match serde_json::from_str::<Value>(&text) {
                Ok(Value::Object(m)) => Ok(m),
                _ => Err(Error::Command {
                    cmd: "read claude_desktop_config.json".into(),
                    stderr: format!(
                        "{} is not a JSON object; fix or move it, then retry",
                        path.display()
                    ),
                }),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
            Err(e) => Err(ehai_core::Error::io(path)(e).into()),
        }
    }

    fn write(path: &Path, map: &Map<String, Value>) -> Result<()> {
        let backup = path.with_extension("json.ehai-backup");
        if path.is_file() && !backup.exists() {
            std::fs::copy(path, &backup).map_err(ehai_core::Error::io(&backup))?;
        }
        let body = serde_json::to_string_pretty(map).expect("json serializes") + "\n";
        ehai_core::migrate::write_atomic(path, body.as_bytes())?;
        Ok(())
    }
}

/// Serialise edits across parallel ehai sessions. The lock lives in ehai's own
/// dir, never next to Claude's config.
fn lock(ctx: &Ctx) -> Result<std::fs::File> {
    let dir = ehai_core::paths::adapters_dir(&ctx.home);
    std::fs::create_dir_all(&dir).map_err(ehai_core::Error::io(&dir))?;
    Ok(ehai_core::migrate::lock(&dir.join("claude-desktop.json"))?)
}

fn default_path() -> Option<PathBuf> {
    let dir = if cfg!(target_os = "macos") {
        std::env::home_dir()?.join("Library/Application Support/Claude")
    } else if cfg!(windows) {
        PathBuf::from(std::env::var_os("APPDATA")?).join("Claude")
    } else {
        std::env::home_dir()?.join(".config/Claude")
    };
    Some(dir.join("claude_desktop_config.json"))
}

impl Harness for ClaudeDesktop {
    fn id(&self) -> &'static str {
        "claude-desktop"
    }

    fn display_name(&self) -> &'static str {
        "Claude desktop"
    }

    fn detect(&self) -> Option<String> {
        self.path().ok().map(|p| p.display().to_string())
    }

    /// The single entry ehai owns inside the shared config file.
    fn generate(&self, ctx: &Ctx) -> Vec<File> {
        vec![File {
            path: format!("claude_desktop_config.json#mcpServers.{}", ctx.brand.name),
            contents: serde_json::to_string_pretty(&Self::entry(ctx)).expect("json") + "\n",
        }]
    }

    fn install(&self, ctx: &Ctx) -> Result<String> {
        let path = self.path()?;
        let _lock = lock(ctx)?;
        let mut map = Self::read(path)?;
        let servers = map
            .entry("mcpServers")
            .or_insert_with(|| Value::Object(Map::new()));
        let Value::Object(servers) = servers else {
            return Err(Error::Command {
                cmd: "edit claude_desktop_config.json".into(),
                stderr: "`mcpServers` is not an object".into(),
            });
        };
        let entry = Self::entry(ctx);
        if servers.get(&ctx.brand.name) == Some(&entry) {
            return Ok(format!("`{}` already registered", ctx.brand.name));
        }
        servers.insert(ctx.brand.name.clone(), entry);
        Self::write(path, &map)?;
        Ok(format!(
            "registered `{}` in {} — quit and reopen Claude desktop to load it",
            ctx.brand.name,
            path.display()
        ))
    }

    fn remove(&self, ctx: &Ctx) -> Result<String> {
        let Ok(path) = self.path() else {
            return Ok("Claude desktop not found; nothing to remove".into());
        };
        let _lock = lock(ctx)?;
        let mut map = Self::read(path)?;
        let removed = map
            .get_mut("mcpServers")
            .and_then(Value::as_object_mut)
            .and_then(|s| s.shift_remove(&ctx.brand.name))
            .is_some();
        if removed {
            Self::write(path, &map)?;
            Ok(format!(
                "removed `{}` — quit and reopen Claude desktop",
                ctx.brand.name
            ))
        } else {
            Ok(format!("`{}` was not registered", ctx.brand.name))
        }
    }

    fn status(&self, ctx: &Ctx) -> Status {
        let Ok(path) = self.path() else {
            return Status::Unavailable;
        };
        let present = Self::read(path)
            .ok()
            .and_then(|m| m.get("mcpServers")?.get(&ctx.brand.name).cloned())
            .is_some();
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

    fn ctx(brand: &str) -> Ctx {
        Ctx {
            brand: Brand::new(brand).unwrap(),
            role: None,
            home: std::env::temp_dir().join("ehai-desktop-test"),
            exe: "/home/u/.ehai/bin/ehai".into(),
        }
    }

    #[test]
    fn edits_only_its_key_and_reverses() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("claude_desktop_config.json");
        let original = r#"{
  "preferences": { "theme": "dark" },
  "mcpServers": { "other": { "command": "x" } },
  "zzz": 1
}
"#;
        std::fs::write(&path, original).unwrap();
        let a = ClaudeDesktop::at(path.clone());
        assert_eq!(a.status(&ctx("ehai")), Status::NotInstalled);
        assert!(a.install(&ctx("ehai")).unwrap().contains("reopen"));
        assert!(
            a.install(&ctx("ehai")).unwrap().contains("already"),
            "idempotent"
        );
        assert_eq!(a.status(&ctx("ehai")), Status::Installed);

        let text = std::fs::read_to_string(&path).unwrap();
        let keys: Vec<&str> = ["preferences", "mcpServers", "zzz"]
            .iter()
            .map(|k| text.find(k).map(|_| *k).unwrap())
            .collect();
        assert!(
            text.find("preferences") < text.find("mcpServers")
                && text.find("mcpServers") < text.find("zzz"),
            "order kept: {keys:?}"
        );
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["mcpServers"]["other"]["command"], "x");
        assert_eq!(v["mcpServers"]["ehai"]["args"][0], "mcp");
        assert_eq!(
            std::fs::read_to_string(path.with_extension("json.ehai-backup")).unwrap(),
            original
        );
        let names: Vec<_> = std::fs::read_dir(d.path())
            .unwrap()
            .flatten()
            .map(|e| e.file_name())
            .collect();
        assert_eq!(
            names.len(),
            2,
            "only config + backup in Claude's dir: {names:?}"
        );

        a.remove(&ctx("ehai")).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert!(v["mcpServers"].get("ehai").is_none() && v["mcpServers"]["other"].is_object());
        assert_eq!(a.status(&ctx("ehai")), Status::NotInstalled);
    }

    #[test]
    fn missing_file_is_created_and_bad_json_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("claude_desktop_config.json");
        ClaudeDesktop::at(path.clone())
            .install(&ctx("acme"))
            .unwrap();
        assert!(std::fs::read_to_string(&path).unwrap().contains("\"acme\""));

        std::fs::write(&path, "{ not json").unwrap();
        assert!(ClaudeDesktop::at(path.clone())
            .install(&ctx("acme"))
            .is_err());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{ not json",
            "never overwritten"
        );

        let gone = ClaudeDesktop::at(d.path().join("no-such-dir/claude_desktop_config.json"));
        assert_eq!(gone.status(&ctx("acme")), Status::Unavailable);
        assert!(matches!(
            gone.install(&ctx("acme")),
            Err(Error::NotDetected(_))
        ));
    }

    #[test]
    fn generated_entry_snapshot() {
        let files =
            ClaudeDesktop::at("/x/claude_desktop_config.json".into()).generate(&ctx("ehai"));
        insta::assert_snapshot!(format!("=== {} ===\n{}", files[0].path, files[0].contents));
    }
}
