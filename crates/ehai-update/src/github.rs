//! GitHub Releases: list, pick the newest one a channel accepts, download
//! assets. REST, not GraphQL — releases don't need it.

use std::time::Duration;

use ehai_core::config::Channel;
use serde::Deserialize;

use crate::{Error, Result};

const TIMEOUT: Duration = Duration::from_secs(30);

pub struct Client {
    http: reqwest::Client,
    base: String,
    token: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Asset {
    pub name: String,
    /// GitHub API URL for this asset — use with `Accept: application/octet-stream`.
    /// Works for both public and private repos; the API responds with a redirect to
    /// a pre-signed CDN URL that does not require an auth header on the final hop.
    pub url: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub draft: bool,
    pub assets: Vec<Asset>,
}

impl Client {
    /// `base` is the API root (`https://api.github.com/repos/<owner>/<repo>`,
    /// or a mock server). `token` is optional: public-repo release listings
    /// work unauthenticated, just more rate-limited.
    pub fn new(base: &str, token: Option<String>) -> Result<Self> {
        Self::with_timeout(base, token, TIMEOUT)
    }

    /// Like [`Client::new`], with a caller-chosen timeout — the background
    /// daily check uses a short one so an unreachable network never stalls an
    /// ordinary command.
    pub fn with_timeout(
        base: &str,
        token: Option<String>,
        timeout: std::time::Duration,
    ) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .user_agent(concat!("ehai/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| Error::Http(e.to_string()))?;
        Ok(Self {
            http,
            base: base.trim_end_matches('/').to_string(),
            token,
        })
    }

    /// Token lookup: `EHAI_TOKEN` env → `GITHUB_TOKEN`/`GH_TOKEN` env → `gh auth token`.
    /// Callers can pass an extra `config_token` (from `config.update.token`) as a final
    /// fallback for private repos where none of the env vars are set.
    pub fn from_env(base: &str) -> Result<Self> {
        Self::with_timeout(base, env_token(None), TIMEOUT)
    }

    /// Like [`Client::from_env`] but also falls back to the stored config token.
    pub fn from_env_or_config(base: &str, config_token: Option<&str>) -> Result<Self> {
        Self::with_timeout(base, env_token(config_token), TIMEOUT)
    }

    /// Like [`Client::from_env`], with a short timeout for the background check.
    pub fn from_env_quick(base: &str, timeout: std::time::Duration) -> Result<Self> {
        Self::with_timeout(base, env_token(None), timeout)
    }

    pub async fn releases(&self) -> Result<Vec<Release>> {
        let mut req = self.http.get(format!("{}/releases", self.base));
        if let Some(t) = &self.token {
            req = req.bearer_auth(t);
        }
        let resp = req.send().await.map_err(|e| Error::Http(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Api {
                status: status.as_u16(),
                body,
            });
        }
        let releases: Vec<Release> = resp.json().await.map_err(|e| Error::Http(e.to_string()))?;
        Ok(releases.into_iter().filter(|r| !r.draft).collect())
    }

    pub async fn download(&self, url: &str) -> Result<Vec<u8>> {
        // `Accept: application/octet-stream` is required for the GitHub asset API
        // endpoint. GitHub responds with a redirect to a pre-signed CDN URL; reqwest
        // strips the auth header on the cross-domain hop (correct RFC behaviour), and
        // the pre-signed URL works without auth. This path works for both public and
        // private repos.
        let mut req = self
            .http
            .get(url)
            .header("Accept", "application/octet-stream");
        if let Some(t) = &self.token {
            req = req.bearer_auth(t);
        }
        let resp = req.send().await.map_err(|e| Error::Http(e.to_string()))?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::Api {
                status: status.as_u16(),
                body,
            });
        }
        Ok(resp
            .bytes()
            .await
            .map_err(|e| Error::Http(e.to_string()))?
            .to_vec())
    }
}

/// Token resolution order: EHAI_TOKEN → GITHUB_TOKEN/GH_TOKEN → gh auth token →
/// config_token (from config.update.token, the stored PAT for private repos).
fn env_token(config_token: Option<&str>) -> Option<String> {
    if let Ok(t) = std::env::var("EHAI_TOKEN") {
        if !t.is_empty() {
            return Some(t);
        }
    }
    if let Ok(t) = std::env::var("GITHUB_TOKEN").or_else(|_| std::env::var("GH_TOKEN")) {
        if !t.is_empty() {
            return Some(t);
        }
    }
    if let Some(t) = gh_token() {
        return Some(t);
    }
    config_token.filter(|s| !s.is_empty()).map(str::to_owned)
}

fn gh_token() -> Option<String> {
    let out = std::process::Command::new("gh")
        .args(["auth", "token"])
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8(out.stdout).ok())?
        .map(|s| s.trim().to_string())
}

/// Which channel a tag belongs to, from its semver pre-release identifier.
/// `v0.1.0-alpha.1` → Alpha, `v0.1.0-beta.1`/`-rc.1` → Beta, `v0.1.0` → Stable.
fn tag_channel(tag: &str) -> Option<Channel> {
    let version = semver::Version::parse(tag.trim_start_matches('v')).ok()?;
    Some(if version.pre.as_str().starts_with("alpha") {
        Channel::Alpha
    } else if version.pre.as_str().starts_with("beta") || version.pre.as_str().starts_with("rc") {
        Channel::Beta
    } else {
        Channel::Stable
    })
}

/// Lower is more bleeding-edge; a channel accepts tags at its own rank or above.
fn channel_rank(c: Channel) -> u8 {
    match c {
        Channel::Alpha => 0,
        Channel::Beta => 1,
        Channel::Stable => 2,
    }
}

/// The newest release `channel` accepts that's strictly newer than
/// `current_version`, or `None` if already up to date.
pub fn latest_for_channel(
    releases: &[Release],
    channel: Channel,
    current_version: &str,
) -> Option<Release> {
    let current = semver::Version::parse(current_version).ok();
    releases
        .iter()
        .filter_map(|r| {
            let v = semver::Version::parse(r.tag_name.trim_start_matches('v')).ok()?;
            let c = tag_channel(&r.tag_name)?;
            (channel_rank(c) >= channel_rank(channel)).then_some((v, r))
        })
        .filter(|(v, _)| current.as_ref().is_none_or(|c| v > c))
        .max_by(|(a, _), (b, _)| a.cmp(b))
        .map(|(_, r)| r.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str) -> Release {
        Release {
            tag_name: tag.into(),
            draft: false,
            assets: vec![],
        }
    }

    #[test]
    fn stable_channel_skips_prereleases() {
        let releases = vec![release("v0.2.0-alpha.1"), release("v0.1.0")];
        let r = latest_for_channel(&releases, Channel::Stable, "0.0.1").unwrap();
        assert_eq!(r.tag_name, "v0.1.0");
    }

    #[test]
    fn alpha_channel_accepts_everything_and_picks_the_newest() {
        let releases = vec![
            release("v0.1.0"),
            release("v0.2.0-alpha.1"),
            release("v0.1.5-beta.1"),
        ];
        let r = latest_for_channel(&releases, Channel::Alpha, "0.0.1").unwrap();
        assert_eq!(r.tag_name, "v0.2.0-alpha.1");
    }

    #[test]
    fn patch_prerelease_is_offered_to_its_base_and_older() {
        // alpha.2.1 is a patch of alpha.2: newer than alpha.2, older than alpha.3.
        let releases = vec![release("v0.1.0-alpha.2.1"), release("v0.1.0-alpha.2")];
        let r = latest_for_channel(&releases, Channel::Alpha, "0.1.0-alpha.2").unwrap();
        assert_eq!(r.tag_name, "v0.1.0-alpha.2.1");
        assert!(latest_for_channel(&releases, Channel::Alpha, "0.1.0-alpha.2.1").is_none());
        let with_3 = vec![release("v0.1.0-alpha.3"), release("v0.1.0-alpha.2.1")];
        let r = latest_for_channel(&with_3, Channel::Alpha, "0.1.0-alpha.2.1").unwrap();
        assert_eq!(r.tag_name, "v0.1.0-alpha.3");
    }

    #[test]
    fn beta_channel_skips_alpha_but_takes_beta_and_stable() {
        let releases = vec![
            release("v0.3.0-alpha.1"),
            release("v0.2.0-beta.1"),
            release("v0.1.0"),
        ];
        let r = latest_for_channel(&releases, Channel::Beta, "0.0.1").unwrap();
        assert_eq!(r.tag_name, "v0.2.0-beta.1");
    }

    #[test]
    fn already_on_latest_is_up_to_date() {
        let releases = vec![release("v0.1.0")];
        assert!(latest_for_channel(&releases, Channel::Stable, "0.1.0").is_none());
    }
}
