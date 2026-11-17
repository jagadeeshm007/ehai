//! Typed configuration and layered loading.
//!
//! Merge order (later wins): defaults < global `~/.ehai/config.toml` < nearest repo
//! `.ehai.toml` < `--config <file>` < `EHAI_*` env vars < CLI overrides.
//! Layers are deep-merged as raw TOML tables and deserialized once, so unknown keys
//! (e.g. from a newer ehai) are ignored rather than rejected.

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use toml::{Table, Value};

use crate::{migrate, paths, Brand, Error, Result};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Config {
    pub schema_version: u32,
    pub brand: Brand,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<Role>,
    pub compliance: Compliance,
    pub sandbox: Sandbox,
    pub prefs: Prefs,
    pub update: Update,
    pub tracker: Tracker,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: migrate::CURRENT_SCHEMA_VERSION,
            brand: Brand::default(),
            role: None,
            compliance: Compliance::default(),
            sandbox: Sandbox::default(),
            prefs: Prefs::default(),
            update: Update::default(),
            tracker: Tracker::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Pm,
    Dev,
    Both,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Compliance {
    pub mode: ComplianceMode,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ComplianceMode {
    Off,
    #[default]
    Warn,
    Strict,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Sandbox {
    pub mode: SandboxMode,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SandboxMode {
    Off,
    #[default]
    Restricted,
    Container,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Prefs {
    pub learning: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { learning: true }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Update {
    pub channel: Channel,
    /// Install updates automatically (otherwise only notify).
    pub auto: bool,
    /// Background update check, at most once a day.
    pub check: bool,
}

impl Default for Update {
    fn default() -> Self {
        Self {
            channel: Channel::Alpha,
            auto: false,
            check: true,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    #[default]
    Alpha,
    Beta,
    Stable,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Tracker {
    pub kind: TrackerKind,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TrackerKind {
    #[default]
    GithubProjects,
    Linear,
    Jira,
}

/// Inputs for [`Config::load`]. Kept explicit (no hidden env/cwd reads) so tests are hermetic.
#[derive(Debug, Default, Clone)]
pub struct LoadOptions {
    pub global: Option<PathBuf>,
    pub cwd: Option<PathBuf>,
    pub explicit: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub overrides: Table,
}

impl LoadOptions {
    /// Real process environment: `~/.ehai/config.toml`, current dir, `EHAI_*` vars.
    pub fn from_process(home: &Path) -> Self {
        Self {
            global: Some(paths::global_config(home)),
            cwd: std::env::current_dir().ok(),
            env: std::env::vars().collect(),
            ..Self::default()
        }
    }
}

impl Config {
    pub fn load(opts: &LoadOptions) -> Result<Self> {
        let mut merged = Table::new();
        let repo = opts.cwd.as_deref().and_then(paths::find_repo_config);
        for path in [opts.global.as_ref(), repo.as_ref(), opts.explicit.as_ref()]
            .into_iter()
            .flatten()
        {
            if path.is_file() {
                deep_merge(&mut merged, migrate::migrate(read_table(path)?));
            }
        }
        deep_merge(&mut merged, env_layer(&opts.env));
        deep_merge(&mut merged, opts.overrides.clone());

        let config: Config = Value::Table(merged)
            .try_into()
            .map_err(|e: toml::de::Error| Error::Invalid(e.message().to_string()))?;
        config.brand.validate()?;
        Ok(config)
    }
}

pub fn read_table(path: &Path) -> Result<Table> {
    let text = std::fs::read_to_string(path).map_err(Error::io(path))?;
    text.parse::<Table>().map_err(|e| Error::Parse {
        path: path.into(),
        source: Box::new(e),
    })
}

/// Recursively merge `top` into `base`; tables merge, everything else replaces.
pub fn deep_merge(base: &mut Table, top: Table) {
    for (key, value) in top {
        match (base.get_mut(&key), value) {
            (Some(Value::Table(b)), Value::Table(t)) => deep_merge(b, t),
            (_, v) => {
                base.insert(key, v);
            }
        }
    }
}

/// `EHAI_COMPLIANCE__MODE=strict` → `compliance.mode = "strict"`.
/// Only keys whose first segment is a known config section are mapped, so
/// unrelated vars like `EHAI_HOME` or `EHAI_NAME` are left alone.
fn env_layer(env: &[(String, String)]) -> Table {
    const SECTIONS: &[&str] = &[
        "brand",
        "role",
        "compliance",
        "sandbox",
        "prefs",
        "update",
        "tracker",
    ];
    let mut out = Table::new();
    for (key, raw) in env {
        let Some(rest) = key.strip_prefix("EHAI_") else {
            continue;
        };
        let path: Vec<String> = rest.split("__").map(str::to_ascii_lowercase).collect();
        if !SECTIONS.contains(&path[0].as_str()) {
            continue;
        }
        let mut layer = Table::new();
        let (leaf, parents) = path.split_last().expect("split yields at least one item");
        layer.insert(leaf.clone(), parse_env_value(raw));
        for p in parents.iter().rev() {
            let mut t = Table::new();
            t.insert(p.clone(), Value::Table(layer));
            layer = t;
        }
        deep_merge(&mut out, layer);
    }
    out
}

/// `true`, `42` and `["a"]` become typed TOML values; anything else is a plain string.
fn parse_env_value(raw: &str) -> Value {
    format!("v = {raw}")
        .parse::<Table>()
        .ok()
        .and_then(|mut t| t.remove("v"))
        .unwrap_or_else(|| Value::String(raw.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn defaults_when_nothing_configured() {
        let c = Config::load(&LoadOptions::default()).unwrap();
        assert_eq!(c, Config::default());
        assert_eq!(c.brand.name, "ehai");
        assert_eq!(c.compliance.mode, ComplianceMode::Warn);
        assert_eq!(c.sandbox.mode, SandboxMode::Restricted);
    }

    #[test]
    fn each_layer_overrides_the_one_below() {
        let d = tempfile::tempdir().unwrap();
        let global = write(
            d.path(),
            "home/config.toml",
            "role = \"pm\"\n[compliance]\nmode = \"off\"\n[sandbox]\nmode = \"off\"\n[update]\nchannel = \"beta\"\nauto = true",
        );
        write(
            d.path(),
            "repo/.ehai.toml",
            "[compliance]\nmode = \"strict\"\n[sandbox]\nmode = \"container\"\n[update]\nchannel = \"stable\"",
        );
        let explicit = write(
            d.path(),
            "ci.toml",
            "[sandbox]\nmode = \"restricted\"\n[update]\nchannel = \"alpha\"",
        );
        let mut overrides = Table::new();
        overrides.insert("role".into(), Value::String("both".into()));

        let c = Config::load(&LoadOptions {
            global: Some(global),
            cwd: Some(d.path().join("repo")),
            explicit: Some(explicit),
            env: vec![("EHAI_UPDATE__CHANNEL".into(), "beta".into())],
            overrides,
        })
        .unwrap();

        assert!(c.update.auto, "global survives where nobody overrides");
        assert_eq!(c.compliance.mode, ComplianceMode::Strict, "repo > global");
        assert_eq!(c.sandbox.mode, SandboxMode::Restricted, "explicit > repo");
        assert_eq!(c.update.channel, Channel::Beta, "env > explicit");
        assert_eq!(c.role, Some(Role::Both), "cli > everything");
    }

    #[test]
    fn env_mapping_types_and_ignores_unrelated_vars() {
        let env = vec![
            ("EHAI_PREFS__LEARNING".into(), "false".into()),
            ("EHAI_BRAND__NAME".into(), "acme".into()),
            ("EHAI_HOME".into(), "/tmp/x".into()),
            ("EHAI_NAME".into(), "zzz".into()),
            ("PATH".into(), "/bin".into()),
        ];
        let c = Config::load(&LoadOptions {
            env,
            ..Default::default()
        })
        .unwrap();
        assert!(!c.prefs.learning);
        assert_eq!(c.brand.name, "acme");
    }

    #[test]
    fn unknown_keys_are_tolerated() {
        let d = tempfile::tempdir().unwrap();
        let g = write(
            d.path(),
            "config.toml",
            "future_key = 1\n[compliance]\nmode = \"warn\"\nnew_field = true\n[shiny_new_section]\nx = 1",
        );
        let c = Config::load(&LoadOptions {
            global: Some(g),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(c.compliance.mode, ComplianceMode::Warn);
    }

    #[test]
    fn bad_values_are_reported() {
        let mut overrides = Table::new();
        overrides.insert("brand".into(), toml::toml! { name = "Not Valid" }.into());
        assert!(matches!(
            Config::load(&LoadOptions {
                overrides,
                ..Default::default()
            }),
            Err(Error::InvalidBrand(_))
        ));

        let env = vec![("EHAI_COMPLIANCE__MODE".into(), "loose".into())];
        assert!(matches!(
            Config::load(&LoadOptions {
                env,
                ..Default::default()
            }),
            Err(Error::Invalid(_))
        ));
    }

    #[test]
    fn parse_error_names_the_file() {
        let d = tempfile::tempdir().unwrap();
        let g = write(d.path(), "config.toml", "this is = = not toml");
        let err = Config::load(&LoadOptions {
            global: Some(g.clone()),
            ..Default::default()
        })
        .unwrap_err();
        assert!(err.to_string().contains(&g.display().to_string()));
    }
}
