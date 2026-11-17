//! `ehai role [set]`: who gets to choose a role. Whoever administers the
//! connected GitHub project (`viewerCanClose`) can pick `pm`/`dev`/`both`;
//! everyone else is `dev`, automatically, with no question — see
//! docs/plans/roles-and-team.md. The admin check itself happens where the
//! tracker connects (`crate::tracker::connect_project`), not here.

use std::io;
use std::path::Path;

use anyhow::{bail, Result};
use ehai_core::config::{self, Config, Role};

use crate::ui::{Choice, Prompter};

/// The role question, shared by `ehai init` and `ehai tracker set`.
pub fn ask_role(p: &mut dyn Prompter, current: &str) -> io::Result<String> {
    let mark = |c: Choice, v: &str| if c.value == v { c.default() } else { c };
    p.select(
        "What's your role?",
        &[
            mark(
                Choice::new("pm", "Product manager").hint("plain language, visuals first"),
                current,
            ),
            mark(
                Choice::new("dev", "Developer").hint("concise, technical"),
                current,
            ),
            mark(
                Choice::new("both", "Both").hint("plain first, technical details after"),
                current,
            ),
        ],
    )
}

/// Explain the effective role and why it applies: `ehai role`.
pub fn show(config: &Config) -> String {
    let effective = role_str(config.effective_role());
    match (config.access.project_admin, &config.access.checked) {
        (Some(true), checked) => format!(
            "{effective} — you administer the connected project{}",
            checked.as_deref().map(|d| format!(" (checked {d})")).unwrap_or_default()
        ),
        (Some(false), checked) => format!(
            "dev — you're not an admin of the connected project{}; only project admins choose a role",
            checked.as_deref().map(|d| format!(" (checked {d})")).unwrap_or_default()
        ),
        (None, _) => "dev — no project connected yet; connect one with `ehai tracker set`, and if you administer it you'll be asked".into(),
    }
}

/// `ehai role set <role>`: refused unless the last check found the viewer an admin.
pub fn set(home: &Path, config: &Config, role: &str) -> Result<String> {
    if config.access.project_admin != Some(true) {
        bail!(
            "only an admin of the connected project can choose a role; you're a developer here. \
             Run `ehai tracker set` to connect a project you administer, or `ehai team refresh` to recheck."
        );
    }
    let global = ehai_core::paths::global_config(home);
    config::edit_file(&global, |t| config::set(t, "", "role", role))?;
    Ok(format!("role set to {role}"))
}

pub fn role_str(r: Role) -> &'static str {
    match r {
        Role::Pm => "pm",
        Role::Dev => "dev",
        Role::Both => "both",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::PlainPrompter;
    use ehai_core::config::Access;

    fn cfg(admin: Option<bool>, role: Option<Role>) -> Config {
        Config {
            role,
            access: Access {
                project_admin: admin,
                checked: Some("2026-11-17".into()),
            },
            ..Default::default()
        }
    }

    #[test]
    fn ask_role_defaults_to_current() {
        let mut p = PlainPrompter::new(Vec::new());
        assert_eq!(ask_role(&mut p, "pm").unwrap(), "pm");
    }

    #[test]
    fn show_explains_each_state() {
        assert!(show(&cfg(Some(true), Some(Role::Pm))).starts_with("pm — you administer"));
        assert!(show(&cfg(Some(false), Some(Role::Pm))).starts_with("dev — you're not an admin"));
        assert!(show(&Config::default()).starts_with("dev — no project connected"));
    }

    #[test]
    fn set_is_refused_for_non_admins() {
        let d = tempfile::tempdir().unwrap();
        assert!(set(d.path(), &cfg(Some(false), None), "pm").is_err());
        assert!(set(d.path(), &Config::default(), "pm").is_err());
        let r = set(d.path(), &cfg(Some(true), None), "pm").unwrap();
        assert_eq!(r, "role set to pm");
        let cfg = Config::load(&ehai_core::LoadOptions {
            global: Some(ehai_core::paths::global_config(d.path())),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(cfg.role, Some(Role::Pm));
    }
}
