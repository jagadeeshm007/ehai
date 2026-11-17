//! GitHub token resolution (spec §5.1): `gh auth token` → `GITHUB_TOKEN` /
//! `GH_TOKEN`. The token is read fresh each run and never written anywhere by
//! ehai, so there's nothing to leak or rotate. OAuth device flow (and keychain
//! storage for its token) is deferred until ehai has its own OAuth app; see
//! DECISIONS ADR-0017.

use std::process::Command;
use std::time::Duration;

use crate::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    GhCli,
    Env(&'static str),
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::GhCli => f.write_str("gh CLI"),
            Source::Env(v) => write!(f, "${v}"),
        }
    }
}

#[derive(Clone)]
pub struct Token {
    pub value: String,
    pub source: Source,
}

/// Never print the secret, even in debug logs.
impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Token({}, ***)", self.source)
    }
}

/// The command that adds the scope GitHub Projects needs to a `gh` login.
pub const SCOPE_FIX: &str = "gh auth refresh -s project";

/// Resolve a token. `gh` is tried first when present (reuses its login);
/// `env` is the process environment, passed in so tests stay hermetic.
pub fn resolve(env: &[(String, String)], use_gh: bool) -> Result<Token> {
    if use_gh {
        if let Some(t) = gh_token() {
            return Ok(Token {
                value: t,
                source: Source::GhCli,
            });
        }
    }
    for var in ["GITHUB_TOKEN", "GH_TOKEN"] {
        if let Some((_, v)) = env.iter().find(|(k, v)| k == var && !v.trim().is_empty()) {
            return Ok(Token {
                value: v.trim().to_string(),
                source: Source::Env(var),
            });
        }
    }
    Err(Error::Auth(
        "no GitHub token found: run `gh auth login`, or set GITHUB_TOKEN".into(),
    ))
}

fn gh_token() -> Option<String> {
    let out = ehai_core::process::output(
        Command::new("gh").args(["auth", "token"]),
        Duration::from_secs(10),
    )
    .ok()?;
    let t = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !t.is_empty()).then_some(t)
}

/// Classic/OAuth tokens list scopes in `X-OAuth-Scopes`. Fine-grained tokens
/// don't, so `None` means "can't tell" and callers should just try.
pub fn has_project_scope(scopes: Option<&[String]>) -> Option<bool> {
    scopes.map(|s| s.iter().any(|x| x == "project"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn env_fallback_order_and_redaction() {
        let t = resolve(&env(&[("GH_TOKEN", "b"), ("GITHUB_TOKEN", "a")]), false).unwrap();
        assert_eq!(
            (t.value.as_str(), t.source),
            ("a", Source::Env("GITHUB_TOKEN"))
        );
        let t = resolve(&env(&[("GITHUB_TOKEN", "  "), ("GH_TOKEN", "b")]), false).unwrap();
        assert_eq!(t.source, Source::Env("GH_TOKEN"));
        assert!(
            !format!("{t:?}").contains('b'),
            "token never in Debug output"
        );
        let e = resolve(&[], false).unwrap_err().to_string();
        assert!(e.contains("gh auth login"), "{e}");
    }

    #[test]
    fn scope_check() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(
            has_project_scope(Some(&s(&["repo", "project"]))),
            Some(true)
        );
        assert_eq!(
            has_project_scope(Some(&s(&["repo", "read:project"]))),
            Some(false)
        );
        assert_eq!(has_project_scope(None), None);
    }
}
