//! Claude desktop adapter.
//!
//! Registers the MCP server under `mcpServers.<brand>` in
//! `claude_desktop_config.json`. Only that one key is ever touched; every other
//! key keeps its value and order, and a `.ehai-backup` is taken before the first
//! edit.
//!
//! Additionally generates a skills ZIP at
//! `~/.ehai/adapters/claude-desktop/<brand>-skills.zip` with the 5 critical PM
//! flow skills (plan, breakdown, sync, status, triage). Upload it via
//! Claude Desktop → Settings → Skills to enable `/ehai:plan` etc. See ADR-0028.
//!
//! Paths verified 2026-11-17 (support.claude.com, modelcontextprotocol.io and the
//! file on Linux): macOS `~/Library/Application Support/Claude/`, Windows
//! `%APPDATA%\Claude\`, Linux `~/.config/Claude/`. See DECISIONS ADR-0021.

use std::io::Write as _;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use crate::{Ctx, Error, File, Harness, Result, Status};

/// The 5 PM skill names bundled into the Claude Desktop skills ZIP.
const PM_SKILLS: &[&str] = &["plan", "breakdown", "sync", "status", "triage"];

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

    fn zip_path(ctx: &Ctx) -> PathBuf {
        ehai_core::paths::adapters_dir(&ctx.home)
            .join("claude-desktop")
            .join(format!("{}-skills.zip", ctx.brand.name))
    }

    /// Build the skills ZIP bytes for the 5 PM flows.
    fn build_zip(ctx: &Ctx) -> Result<Vec<u8>> {
        let mut buf = Vec::new();
        let mut zip = ZipWriter::new(std::io::Cursor::new(&mut buf));
        let opts = SimpleFileOptions::default();

        // manifest.json
        let manifest = json!({
            "name": ctx.brand.name,
            "description": format!("AI-SDLC PM skills for {}", ctx.brand.name),
            "version": ehai_core::VERSION,
            "skills": PM_SKILLS,
        });
        zip.start_file("manifest.json", opts)
            .map_err(|e| Error::Command {
                cmd: "zip manifest".into(),
                stderr: e.to_string(),
            })?;
        zip.write_all(serde_json::to_string_pretty(&manifest).unwrap().as_bytes())
            .map_err(|e| Error::Command {
                cmd: "zip write manifest".into(),
                stderr: e.to_string(),
            })?;

        // one .md per PM skill
        for skill_name in PM_SKILLS {
            if let Some((_, tpl)) = crate::claude_code::SKILLS
                .iter()
                .find(|(n, _)| *n == *skill_name)
            {
                let rendered = crate::render(tpl, ctx);
                zip.start_file(format!("{skill_name}.md"), opts)
                    .map_err(|e| Error::Command {
                        cmd: format!("zip {skill_name}.md"),
                        stderr: e.to_string(),
                    })?;
                zip.write_all(rendered.as_bytes())
                    .map_err(|e| Error::Command {
                        cmd: format!("zip write {skill_name}.md"),
                        stderr: e.to_string(),
                    })?;
            }
        }

        zip.finish().map_err(|e| Error::Command {
            cmd: "zip finish".into(),
            stderr: e.to_string(),
        })?;
        Ok(buf)
    }

    fn write_zip(ctx: &Ctx) -> Result<PathBuf> {
        let dest = Self::zip_path(ctx);
        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir).map_err(ehai_core::Error::io(dir))?;
        }
        let bytes = Self::build_zip(ctx)?;
        ehai_core::migrate::write_atomic(&dest, &bytes)?;
        Ok(dest)
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
        let already_registered = servers.get(&ctx.brand.name) == Some(&entry);
        if !already_registered {
            servers.insert(ctx.brand.name.clone(), entry);
            Self::write(path, &map)?;
        }

        // always (re)generate the skills ZIP so it stays fresh after updates
        let zip_dest = Self::write_zip(ctx)?;

        if already_registered {
            Ok(format!("`{}` already registered", ctx.brand.name))
        } else {
            Ok(format!(
                "registered `{}` in {} — quit and reopen Claude desktop to load it\n\
                 Skills ZIP → {}\n\
                 Upload it via Claude Desktop → Settings → Skills to enable /{}:plan etc.",
                ctx.brand.name,
                path.display(),
                zip_dest.display(),
                ctx.brand.name,
            ))
        }
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
        }
        // remove the skills ZIP regardless
        let zip = Self::zip_path(ctx);
        let _ = std::fs::remove_file(&zip);

        if removed {
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
#[allow(deprecated)]
mod tests {
    use super::*;
    use ehai_core::Brand;

    fn ctx(brand: &str) -> Ctx {
        Ctx {
            brand: Brand::new(brand).unwrap(),
            role: None,
            home: tempfile::tempdir().unwrap().into_path(),
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

    #[test]
    fn skills_zip_contains_all_pm_skills() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("claude_desktop_config.json");
        let c = ctx("ehai");
        ClaudeDesktop::at(path).install(&c).unwrap();

        let zip_path = ClaudeDesktop::zip_path(&c);
        assert!(zip_path.exists(), "skills ZIP written");

        let data = std::fs::read(&zip_path).unwrap();
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(data)).expect("valid ZIP");
        let names: Vec<String> = (0..archive.len())
            .map(|i| {
                archive
                    .by_index(i)
                    .unwrap()
                    .name()
                    .unwrap_or_default()
                    .to_string()
            })
            .collect();
        for skill in PM_SKILLS {
            assert!(
                names.contains(&format!("{skill}.md")),
                "missing {skill}.md in ZIP; got {names:?}"
            );
        }
        assert!(names.contains(&"manifest.json".to_string()));
    }

    #[test]
    fn remove_deletes_zip() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("claude_desktop_config.json");
        let c = ctx("ehai");
        ClaudeDesktop::at(path.clone()).install(&c).unwrap();
        let zip_path = ClaudeDesktop::zip_path(&c);
        assert!(zip_path.exists());

        ClaudeDesktop::at(path).remove(&c).unwrap();
        assert!(!zip_path.exists(), "ZIP removed");
    }
}
