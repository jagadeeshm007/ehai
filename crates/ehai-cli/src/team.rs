//! `ehai team [refresh]`: who can be assigned work and @mentioned on the
//! connected project. Cached for a day; see docs/plans/roles-and-team.md and
//! `ehai_tracker::team`.

use anyhow::{Context, Result};
use ehai_core::Config;
use ehai_tracker::team::Team;

use crate::tracker::block_on;

/// Fetch (or re-check) the team for the connected project, re-checking admin
/// status at the same time and persisting it to `access.*` like `tracker set`.
pub fn run(home: &std::path::Path, config: &Config, refresh: bool) -> Result<String> {
    let g = &config.tracker.github;
    let (owner, number) = match (&g.owner, g.project) {
        (Some(o), Some(n)) => (o.clone(), n),
        _ => anyhow::bail!(
            "no tracker connected yet: run `{} tracker set`",
            config.brand.name
        ),
    };
    let (client, _) = crate::tracker::client()?;
    let team = block_on(ehai_tracker::team::get(
        &client,
        home,
        &owner,
        number,
        g.repo.as_deref(),
        refresh,
    ))
    .context("fetching the team")?;

    if refresh {
        let global = ehai_core::paths::global_config(home);
        ehai_core::config::edit_file(&global, |t| {
            ehai_core::config::set(t, "access", "project_admin", team.viewer_is_admin);
            ehai_core::config::set(t, "access", "checked", ehai_core::date::today_utc());
        })?;
    }
    Ok(describe(&team, refresh))
}

fn describe(team: &Team, refreshed: bool) -> String {
    let mut out = format!(
        "{} members for {} ({}) ({})\n",
        team.members.len(),
        team.viewer,
        if team.viewer_is_admin {
            "admin"
        } else {
            "developer"
        },
        if refreshed {
            "just refreshed".to_string()
        } else {
            format!("cached {}s ago", team.age_secs())
        }
    );
    for m in &team.members {
        out += &format!(
            "  {}{}{}\n",
            m.login,
            m.name
                .as_deref()
                .map(|n| format!(" ({n})"))
                .unwrap_or_default(),
            if m.assignable {
                ""
            } else {
                " — not assignable on this repo"
            }
        );
    }
    out.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ehai_tracker::team::TeamMember;

    #[test]
    fn describe_lists_members_and_flags_non_assignable() {
        let team = Team {
            fetched_at: 0,
            viewer: "jane".into(),
            viewer_is_admin: true,
            members: vec![
                TeamMember {
                    login: "jane".into(),
                    name: Some("Jane".into()),
                    assignable: true,
                },
                TeamMember {
                    login: "bob".into(),
                    name: None,
                    assignable: false,
                },
            ],
        };
        let out = describe(&team, true);
        assert!(
            out.starts_with("2 members for jane (admin) (just refreshed)"),
            "{out}"
        );
        assert!(out.contains("jane (Jane)"));
        assert!(out.contains("bob — not assignable"));
    }
}
