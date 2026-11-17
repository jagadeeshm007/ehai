//! Claude Code adapter.
//!
//! Generates a local plugin marketplace at `~/.ehai/adapters/claude-code/` holding
//! one plugin named after the brand. The plugin bundles the MCP server (`.mcp.json`
//! → `<exe> mcp`) and the skills, so one install registers both and one removal
//! undoes both. A marketplace added from a local path is loaded in place, so
//! regenerating files takes effect on the next session or `/reload-plugins`.
//!
//! Formats verified against code.claude.com/docs (plugins reference, marketplaces,
//! skills) on 2026-11-17 and with Claude Code 2.1.179 — see DECISIONS.md ADR-0012.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde_json::{json, Value};

use crate::{render, sync_tree, Ctx, Error, File, Harness, Result, Status};

/// Skill templates: `(name, template)`. Also served as MCP prompts by `ehai-mcp`,
/// which is how the PM flows reach Claude desktop.
pub const SKILLS: &[(&str, &str)] = &[
    (
        "help",
        include_str!("../../../templates/claude-code/skills/help/SKILL.md"),
    ),
    (
        "plan",
        include_str!("../../../templates/claude-code/skills/plan/SKILL.md"),
    ),
    (
        "breakdown",
        include_str!("../../../templates/claude-code/skills/breakdown/SKILL.md"),
    ),
    (
        "sync",
        include_str!("../../../templates/claude-code/skills/sync/SKILL.md"),
    ),
    (
        "status",
        include_str!("../../../templates/claude-code/skills/status/SKILL.md"),
    ),
    (
        "triage",
        include_str!("../../../templates/claude-code/skills/triage/SKILL.md"),
    ),
    (
        "prefs",
        include_str!("../../../templates/claude-code/skills/prefs/SKILL.md"),
    ),
];

const TIMEOUT: Duration = Duration::from_secs(120);

pub struct ClaudeCode {
    /// The `claude` CLI. `EHAI_CLAUDE_BIN` overrides PATH lookup (tests use a fake).
    bin: Option<PathBuf>,
}

impl ClaudeCode {
    pub fn from_env() -> Self {
        let bin = std::env::var_os("EHAI_CLAUDE_BIN")
            .map(PathBuf::from)
            .filter(|p| p.is_file())
            .or_else(|| crate::find_on_path("claude"));
        Self { bin }
    }

    pub fn root(ctx: &Ctx) -> PathBuf {
        ehai_core::paths::adapters_dir(&ctx.home).join("claude-code")
    }

    fn plugin_id(ctx: &Ctx) -> String {
        format!("{}@{}", ctx.brand.name, ctx.brand.marketplace_name())
    }

    fn claude(&self, args: &[&str]) -> Result<String> {
        let bin = self.bin.as_ref().ok_or(Error::NotDetected("Claude Code"))?;
        let cmd = format!("claude {}", args.join(" "));
        let out =
            ehai_core::process::output(Command::new(bin).args(args), TIMEOUT).map_err(|e| {
                Error::Command {
                    cmd: cmd.clone(),
                    stderr: e.to_string(),
                }
            })?;
        if out.status.success() {
            Ok(String::from_utf8_lossy(&out.stdout).into_owned())
        } else {
            Err(Error::Command {
                cmd,
                stderr: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            })
        }
    }

    fn claude_json(&self, args: &[&str]) -> Result<Vec<Value>> {
        let text = self.claude(args)?;
        Ok(serde_json::from_str::<Vec<Value>>(&text).unwrap_or_default())
    }

    fn plugin_installed(&self, ctx: &Ctx) -> Result<bool> {
        let id = Self::plugin_id(ctx);
        Ok(self
            .claude_json(&["plugin", "list", "--json"])?
            .iter()
            .any(|p| p["id"] == id.as_str()))
    }
}

/// Does a `marketplace list --json` entry point at `root`?
fn points_to(entry: &Value, root: &Path) -> bool {
    let root = root.to_string_lossy();
    entry
        .as_object()
        .is_some_and(|o| o.values().any(|v| v.as_str() == Some(root.as_ref())))
}

/// FNV-1a over paths and contents: stable across Rust versions and platforms
/// (unlike `DefaultHasher`), so the same content always gets the same version.
fn content_hash(files: &[File]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for f in files {
        for b in f
            .path
            .bytes()
            .chain([0])
            .chain(f.contents.bytes())
            .chain([0])
        {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    h
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).expect("json values always serialize") + "\n"
}

impl Harness for ClaudeCode {
    fn id(&self) -> &'static str {
        "claude-code"
    }

    fn display_name(&self) -> &'static str {
        "Claude Code"
    }

    fn detect(&self) -> Option<String> {
        self.bin.as_ref().map(|b| b.display().to_string())
    }

    fn generate(&self, ctx: &Ctx) -> Vec<File> {
        let brand = ctx.brand.name.as_str();
        let description =
            format!("{brand}: plan, track and prototype from Claude Code (skills + MCP server)");
        let plugin_dir = format!("plugins/{brand}");
        let mut files = vec![
            File {
                path: ".claude-plugin/marketplace.json".into(),
                contents: pretty(&json!({
                    "name": ctx.brand.marketplace_name(),
                    "description": format!("Local marketplace generated by {brand}. Do not edit; run `{brand} adapters regenerate`."),
                    "owner": { "name": brand },
                    "plugins": [{
                        "name": brand,
                        "source": format!("./{plugin_dir}"),
                        "description": description,
                    }],
                })),
            },
            File {
                path: format!("{plugin_dir}/.mcp.json"),
                contents: pretty(&json!({
                    "mcpServers": {
                        brand: {
                            "command": ctx.exe.to_string_lossy(),
                            "args": ["mcp"],
                        }
                    }
                })),
            },
        ];
        files.extend(SKILLS.iter().map(|(name, tpl)| File {
            path: format!("{plugin_dir}/skills/{name}/SKILL.md"),
            contents: render(tpl, ctx),
        }));
        // Claude Code caches an installed plugin per `version` and `plugin update`
        // only refreshes when it changes, so the version carries a content hash:
        // regenerated content ⇒ new version ⇒ the update is picked up.
        let version = format!("{}+{:08x}", ehai_core::VERSION, content_hash(&files) as u32);
        files.push(File {
            path: format!("{plugin_dir}/.claude-plugin/plugin.json"),
            contents: pretty(&json!({
                "name": brand,
                "version": version,
                "description": description,
                "author": { "name": brand },
            })),
        });
        files
    }

    fn install(&self, ctx: &Ctx) -> Result<String> {
        if self.bin.is_none() {
            return Err(Error::NotDetected("Claude Code"));
        }
        let root = Self::root(ctx);
        let changed = sync_tree(&root, &self.generate(ctx))?;
        let mp = ctx.brand.marketplace_name();
        let root_str = root.to_string_lossy().into_owned();

        let marketplaces = self.claude_json(&["plugin", "marketplace", "list", "--json"])?;
        // A renamed brand leaves the old marketplace pointing at our root: drop it.
        for stale in marketplaces
            .iter()
            .filter(|m| m["name"] != mp.as_str() && points_to(m, &root))
        {
            if let Some(name) = stale["name"].as_str() {
                self.claude(&["plugin", "marketplace", "remove", name])?;
            }
        }
        if !marketplaces.iter().any(|m| m["name"] == mp.as_str()) {
            self.claude(&["plugin", "marketplace", "add", &root_str])?;
        }
        let id = Self::plugin_id(ctx);
        if !self.plugin_installed(ctx)? {
            self.claude(&["plugin", "install", &id, "--scope", "user"])?;
        } else if changed > 0 {
            self.claude(&["plugin", "marketplace", "update", &mp])?;
            self.claude(&["plugin", "update", &id, "--scope", "user"])?;
        }
        Ok(if changed > 0 {
            format!("{id} installed ({changed} files written)")
        } else {
            format!("{id} up to date")
        })
    }

    fn remove(&self, ctx: &Ctx) -> Result<String> {
        let mp = ctx.brand.marketplace_name();
        if self.bin.is_some() {
            let listed = self
                .claude_json(&["plugin", "marketplace", "list", "--json"])?
                .iter()
                .any(|m| m["name"] == mp.as_str());
            if listed {
                // Removing the marketplace also uninstalls its plugins.
                self.claude(&["plugin", "marketplace", "remove", &mp])?;
            }
        }
        let root = Self::root(ctx);
        if root.exists() {
            std::fs::remove_dir_all(&root).map_err(ehai_core::Error::io(&root))?;
        }
        Ok(format!("removed {}", Self::plugin_id(ctx)))
    }

    fn status(&self, ctx: &Ctx) -> Status {
        if self.bin.is_none() {
            return Status::Unavailable;
        }
        match self.plugin_installed(ctx) {
            Ok(true) => Status::Installed,
            _ => Status::NotInstalled,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ehai_core::Brand;

    fn ctx(home: &Path, brand: &str, role: Option<&str>) -> Ctx {
        Ctx {
            brand: Brand::new(brand).unwrap(),
            role: role.map(Into::into),
            home: home.into(),
            exe: "/home/u/.ehai/bin/ehai".into(),
        }
    }

    #[test]
    fn generated_files_snapshot_per_brand() {
        for (brand, role) in [("ehai", Some("pm")), ("acme", None)] {
            let files = ClaudeCode { bin: None }.generate(&ctx(Path::new("/h"), brand, role));
            for f in &files {
                assert!(!f.contents.contains("{{"), "{} has a placeholder", f.path);
                if f.path.ends_with(".json") {
                    serde_json::from_str::<Value>(&f.contents).expect("valid json");
                }
            }
            let dump: String = files
                .iter()
                .map(|f| format!("=== {} ===\n{}", f.path, f.contents))
                .collect();
            insta::assert_snapshot!(format!("claude_code_{brand}"), dump);
        }
    }

    #[test]
    fn version_tracks_content() {
        let c = ClaudeCode { bin: None };
        let v = |role: Option<&str>| {
            let files = c.generate(&ctx(Path::new("/h"), "ehai", role));
            let pj = files
                .iter()
                .find(|f| f.path.ends_with("plugin.json"))
                .unwrap();
            serde_json::from_str::<Value>(&pj.contents).unwrap()["version"]
                .as_str()
                .unwrap()
                .to_string()
        };
        assert_eq!(v(Some("pm")), v(Some("pm")));
        assert_ne!(v(Some("pm")), v(Some("dev")));
        assert!(v(None).starts_with(ehai_core::VERSION));
    }

    #[test]
    fn unavailable_without_cli() {
        let d = tempfile::tempdir().unwrap();
        let c = ClaudeCode { bin: None };
        let cx = ctx(d.path(), "ehai", None);
        assert_eq!(c.status(&cx), Status::Unavailable);
        assert!(matches!(c.install(&cx), Err(Error::NotDetected(_))));
        // remove still cleans generated files without the CLI
        sync_tree(&ClaudeCode::root(&cx), &c.generate(&cx)).unwrap();
        c.remove(&cx).unwrap();
        assert!(!ClaudeCode::root(&cx).exists());
    }

    /// A fake `claude` that keeps marketplace/plugin state in files and logs calls.
    #[cfg(unix)]
    fn fake_claude(dir: &Path) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let bin = dir.join("claude");
        let script = r#"#!/bin/sh
S="$(dirname "$0")"
echo "$*" >> "$S/calls.log"
case "$*" in
  "plugin marketplace list --json") cat "$S/mps.json" 2>/dev/null || echo "[]" ;;
  "plugin list --json") cat "$S/plugins.json" 2>/dev/null || echo "[]" ;;
  "plugin marketplace add "*) printf '[{"name":"ehai-local","source":"directory","path":"%s"}]' "$4" > "$S/mps.json" ;;
  "plugin install "*) printf '[{"id":"%s","enabled":true}]' "$3" > "$S/plugins.json" ;;
  "plugin marketplace update "*|"plugin update "*) : ;;
  "plugin marketplace remove "*) echo "[]" > "$S/mps.json"; echo "[]" > "$S/plugins.json" ;;
  *) echo "unexpected: $*" >&2; exit 1 ;;
esac
"#;
        std::fs::write(&bin, script).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        bin
    }

    #[cfg(unix)]
    #[test]
    fn install_is_idempotent_and_remove_reverses_it() {
        let d = tempfile::tempdir().unwrap();
        let bin = fake_claude(d.path());
        let c = ClaudeCode { bin: Some(bin) };
        let cx = ctx(&d.path().join("home"), "ehai", Some("dev"));

        assert_eq!(c.status(&cx), Status::NotInstalled);
        let first = c.install(&cx).unwrap();
        assert!(first.contains("installed"), "{first}");
        assert_eq!(c.status(&cx), Status::Installed);
        let second = c.install(&cx).unwrap();
        assert!(second.contains("up to date"), "{second}");

        let log = std::fs::read_to_string(d.path().join("calls.log")).unwrap();
        assert_eq!(log.matches("marketplace add").count(), 1, "{log}");
        assert_eq!(log.matches("plugin install").count(), 1, "{log}");
        assert!(log.contains("--scope user"));

        // changed content (role) ⇒ new version ⇒ refresh the installed copy
        let cx2 = Ctx {
            role: Some("pm".into()),
            ..cx.clone()
        };
        assert!(c.install(&cx2).unwrap().contains("installed"));
        let log = std::fs::read_to_string(d.path().join("calls.log")).unwrap();
        assert!(log.contains("plugin update ehai@ehai-local"), "{log}");
        assert_eq!(log.matches("plugin install").count(), 1, "{log}");

        c.remove(&cx).unwrap();
        assert_eq!(c.status(&cx), Status::NotInstalled);
        assert!(!ClaudeCode::root(&cx).exists());
    }
}
