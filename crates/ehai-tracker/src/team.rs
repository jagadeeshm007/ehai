//! The team: who can be assigned work and @mentioned on a project. Cached per
//! project at `~/.ehai/cache/team/<owner>-<number>.json`, refreshed once a day
//! or on demand (`ehai team refresh`). See docs/plans/roles-and-team.md.
//!
//! Also carries `viewer_is_admin` (`ProjectV2.viewerCanClose`, the field GitHub
//! reserves for project admins), the signal M3.5 uses to decide who gets asked
//! for a role at all.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::github::Client;
use crate::{Error, Result};

/// Refresh after this long.
pub const MAX_AGE_SECS: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamMember {
    pub login: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Can this login be put in an issue's `assignees`? (Org members who aren't
    /// repo collaborators can be @mentioned but not assigned.)
    pub assignable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Team {
    /// Unix seconds; compared against [`MAX_AGE_SECS`] for staleness.
    pub fetched_at: u64,
    pub viewer: String,
    pub viewer_is_admin: bool,
    pub members: Vec<TeamMember>,
}

impl Team {
    pub fn is_stale(&self) -> bool {
        now().saturating_sub(self.fetched_at) > MAX_AGE_SECS
    }

    pub fn age_secs(&self) -> u64 {
        now().saturating_sub(self.fetched_at)
    }

    /// Case-insensitive lookup, with `"me"` meaning the viewer.
    pub fn find(&self, who: &str) -> Option<&TeamMember> {
        if who.eq_ignore_ascii_case("me") {
            return self.members.iter().find(|m| m.login == self.viewer);
        }
        let who = who.strip_prefix('@').unwrap_or(who);
        self.members
            .iter()
            .find(|m| m.login.eq_ignore_ascii_case(who))
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn cache_path(home: &Path, owner: &str, number: u64) -> PathBuf {
    ehai_core::paths::team_cache(home, owner, number)
}

fn load_cached(home: &Path, owner: &str, number: u64) -> Option<Team> {
    let text = std::fs::read_to_string(cache_path(home, owner, number)).ok()?;
    serde_json::from_str(&text).ok()
}

/// Read the cache without a network round trip, e.g. for `doctor`'s age display.
pub fn cached(home: &Path, owner: &str, number: u64) -> Option<Team> {
    load_cached(home, owner, number)
}

fn save(home: &Path, owner: &str, number: u64, team: &Team) -> Result<()> {
    let path = cache_path(home, owner, number);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(ehai_core::Error::io(dir))?;
    }
    let _lock = ehai_core::migrate::lock(&path)?;
    let body = serde_json::to_string_pretty(team).map_err(|e| Error::Api(e.to_string()))?;
    ehai_core::migrate::write_atomic(&path, body.as_bytes())?;
    Ok(())
}

/// One round trip: viewer login, `viewerCanClose` for the project, org members
/// (when the owner is an org) and repo assignable users (when `repo` is given).
/// Members merge by login; assignable-on-the-repo is marked where known.
pub async fn fetch(client: &Client, owner: &str, number: u64, repo: Option<&str>) -> Result<Team> {
    let (repo_owner, repo_name) = match repo.and_then(|r| r.split_once('/')) {
        Some((o, n)) => (o, n),
        None => ("", ""),
    };
    let d = client
        .graphql(
            "Team",
            q::TEAM,
            serde_json::json!({
                "owner": owner, "number": number,
                "hasRepo": repo.is_some(), "repoOwner": repo_owner, "repoName": repo_name,
            }),
        )
        .await?;
    let viewer = d["viewer"]["login"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let owner_node = &d["repositoryOwner"];
    let viewer_is_admin = owner_node["projectV2"]["viewerCanClose"]
        .as_bool()
        .unwrap_or(false);

    let logins = |v: &serde_json::Value| -> Vec<(String, Option<String>)> {
        v.as_array()
            .into_iter()
            .flatten()
            .filter_map(|n| {
                Some((
                    n["login"].as_str()?.to_string(),
                    n["name"].as_str().map(String::from),
                ))
            })
            .collect()
    };
    let assignable: Vec<(String, Option<String>)> =
        logins(&d["repository"]["assignableUsers"]["nodes"]);
    let org_members: Vec<(String, Option<String>)> =
        logins(&owner_node["membersWithRole"]["nodes"]);

    let mut members: Vec<TeamMember> = Vec::new();
    for (login, name) in assignable.into_iter().chain(org_members) {
        match members.iter_mut().find(|m| m.login == login) {
            Some(m) => m.name = m.name.take().or(name),
            None => members.push(TeamMember {
                login: login.clone(),
                name,
                assignable: false, // set below, once we know which list it came from
            }),
        }
    }
    // mark assignable separately, since the merge above may have met the login
    // first via org membership
    let assignable_logins: std::collections::BTreeSet<&str> = d["repository"]["assignableUsers"]
        ["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|n| n["login"].as_str())
        .collect();
    for m in &mut members {
        m.assignable = assignable_logins.contains(m.login.as_str());
    }
    members.sort_by(|a, b| a.login.cmp(&b.login));

    Ok(Team {
        fetched_at: now(),
        viewer,
        viewer_is_admin,
        members,
    })
}

/// Cached team for `owner #number`. Fills the cache on first use, refreshes it
/// once a day, and on a failed refresh falls back to the (stale) cache rather
/// than failing outright — a team list a day old is still useful.
pub async fn get(
    client: &Client,
    home: &Path,
    owner: &str,
    number: u64,
    repo: Option<&str>,
    force_refresh: bool,
) -> Result<Team> {
    let cached = load_cached(home, owner, number);
    if !force_refresh {
        if let Some(team) = &cached {
            if !team.is_stale() {
                return Ok(team.clone());
            }
        }
    }
    match fetch(client, owner, number, repo).await {
        Ok(team) => {
            save(home, owner, number, &team)?;
            Ok(team)
        }
        Err(e) => match cached {
            Some(team) => {
                tracing::warn!(error = %e, "team refresh failed; using the cached list");
                Ok(team)
            }
            None => Err(e),
        },
    }
}

mod q {
    pub const TEAM: &str = "query Team($owner: String!, $number: Int!, $hasRepo: Boolean!, $repoOwner: String!, $repoName: String!) {
        viewer { login }
        repositoryOwner(login: $owner) {
          ... on ProjectV2Owner { projectV2(number: $number) { viewerCanClose } }
          ... on Organization { membersWithRole(first: 100) { nodes { login name } } }
        }
        repository(owner: $repoOwner, name: $repoName) @include(if: $hasRepo) {
          assignableUsers(first: 100) { nodes { login name } }
        }
      }";
}

#[cfg(test)]
mod tests {
    use super::*;

    fn member(login: &str, assignable: bool) -> TeamMember {
        TeamMember {
            login: login.into(),
            name: None,
            assignable,
        }
    }

    fn team() -> Team {
        Team {
            fetched_at: now(),
            viewer: "jane".into(),
            viewer_is_admin: true,
            members: vec![member("jane", true), member("bob", false)],
        }
    }

    #[test]
    fn find_matches_me_and_case_insensitively() {
        let t = team();
        assert_eq!(t.find("me").unwrap().login, "jane");
        assert_eq!(t.find("BOB").unwrap().login, "bob");
        assert_eq!(t.find("@bob").unwrap().login, "bob");
        assert!(t.find("nobody").is_none());
    }

    #[test]
    fn staleness() {
        let mut t = team();
        assert!(!t.is_stale());
        t.fetched_at = now() - MAX_AGE_SECS - 1;
        assert!(t.is_stale());
        assert!(t.age_secs() >= MAX_AGE_SECS);
    }

    #[test]
    fn cache_round_trips() {
        let d = tempfile::tempdir().unwrap();
        assert!(load_cached(d.path(), "octo", 1).is_none());
        save(d.path(), "octo", 1, &team()).unwrap();
        let loaded = load_cached(d.path(), "octo", 1).unwrap();
        assert_eq!(loaded, team());
    }
}
