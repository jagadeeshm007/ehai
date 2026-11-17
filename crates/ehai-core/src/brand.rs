//! The configurable product name. Everything user-visible (binary alias, MCP server
//! name, slash-command prefix) derives from it; internal dirs stay `ehai`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Error, Result};

pub const DEFAULT_NAME: &str = "ehai";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct Brand {
    /// Display name, MCP server name and slash-command prefix.
    pub name: String,
}

impl Default for Brand {
    fn default() -> Self {
        Self {
            name: DEFAULT_NAME.into(),
        }
    }
}

impl Brand {
    pub fn new(name: &str) -> Result<Self> {
        let brand = Self { name: name.into() };
        brand.validate()?;
        Ok(brand)
    }

    /// 2-32 chars of `[a-z0-9-]`, starting with a letter: safe as a binary name,
    /// an MCP server key and a slash-command namespace on every harness.
    pub fn validate(&self) -> Result<()> {
        let n = &self.name;
        let ok = (2..=32).contains(&n.len())
            && n.starts_with(|c: char| c.is_ascii_lowercase())
            && n.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
        // Claude Code refuses plugin names that pass as Anthropic's own.
        let reserved = ["claude", "anthropic", "anthropics"]
            .iter()
            .any(|r| n == r || n.starts_with(&format!("{r}-")))
            || n.starts_with("cc-plugin-");
        if ok && !reserved {
            Ok(())
        } else {
            Err(Error::InvalidBrand(n.clone()))
        }
    }

    pub fn mcp_server_name(&self) -> &str {
        &self.name
    }

    /// Name of the generated local plugin marketplace, e.g. `ehai-local`.
    pub fn marketplace_name(&self) -> String {
        format!("{}-local", self.name)
    }

    /// e.g. `ehai:` as in `/ehai:plan`.
    pub fn command_prefix(&self) -> String {
        format!("{}:", self.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_names() {
        for good in ["ehai", "acme-ops", "a1"] {
            assert!(Brand::new(good).is_ok(), "{good}");
        }
        for bad in [
            "",
            "e",
            "Ehai",
            "1ehai",
            "-x",
            "ehai ops",
            "e/h",
            &"a".repeat(33),
        ] {
            assert!(Brand::new(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn derived_names() {
        let b = Brand::new("acme").unwrap();
        assert_eq!(b.mcp_server_name(), "acme");
        assert_eq!(b.command_prefix(), "acme:");
    }
}
