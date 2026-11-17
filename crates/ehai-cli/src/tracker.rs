//! `ehai tracker set|status|schema`, plus the GitHub plumbing `init` and `doctor`
//! share. GitHub is reached at `EHAI_GITHUB_API` when set (tests point it at a fake).

use std::future::Future;
use std::path::Path;

use anyhow::{bail, Context, Result};
use ehai_core::config::{self, Config};
use ehai_tracker::github::{Client, GithubProjects, ProjectSummary};
use ehai_tracker::{auth, SchemaReport, Tracker};

use crate::ui::{Choice, Prompter, StepError};

/// Run async tracker code from the synchronous CLI.
pub fn block_on<F: Future>(f: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime")
        .block_on(f)
}

/// A client for the user's token. `EHAI_NO_GH=1` skips the `gh` CLI (tests).
pub fn client() -> Result<(Client, auth::Token)> {
    Ok(ehai_tracker::github::client_from_env()?)
}

/// The configured project, or a hint on how to configure one.
pub fn configured(config: &Config, client: Client) -> Result<GithubProjects> {
    Ok(ehai_tracker::github::from_config(config, client)?)
}

/// What the user picked for the project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProjectChoice {
    Existing(u64),
    Create(String),
}

/// One select listing projects + "create new" (+ "skip" when `allow_skip`).
/// Returns `None` for skip.
///
/// Default: `current` when it's listed; otherwise "skip" when offered (so a
/// non-interactive run never touches a GitHub project nobody chose); otherwise
/// the most recently updated project, or "create new" when there is none.
pub fn ask_project(
    p: &mut dyn Prompter,
    projects: &[ProjectSummary],
    default_title: &str,
    allow_skip: bool,
    current: Option<u64>,
) -> std::io::Result<Option<ProjectChoice>> {
    let mut choices: Vec<Choice> = projects
        .iter()
        .filter(|pr| !pr.closed)
        .map(|pr| {
            Choice::new(
                &pr.number.to_string(),
                &format!("{} (#{})", pr.title, pr.number),
            )
            .hint(&pr.owner)
        })
        .collect();
    let has_open = !choices.is_empty();
    choices.push(Choice::new("new", "Create a new project"));
    if allow_skip {
        choices.push(Choice::new("skip", "Skip for now").hint("connect later with `tracker set`"));
    }
    let default = match current.map(|n| n.to_string()) {
        Some(n) if choices.iter().any(|c| c.value == n) => n,
        _ if allow_skip => "skip".into(),
        _ if has_open => choices[0].value.clone(),
        _ => "new".into(),
    };
    if let Some(c) = choices.iter_mut().find(|c| c.value == default) {
        c.default = true;
    }
    Ok(
        match p
            .select("Where should work be tracked?", &choices)?
            .as_str()
        {
            "skip" => None,
            "new" => Some(ProjectChoice::Create(
                p.input("Name for the new project", default_title)?,
            )),
            n => Some(ProjectChoice::Existing(
                n.parse().expect("choice values are project numbers"),
            )),
        },
    )
}

/// Create the project if needed, save it to config, and ensure its fields.
pub fn connect_project(
    p: &mut dyn Prompter,
    home: &Path,
    client: &Client,
    owner: &str,
    choice: &ProjectChoice,
    repo: Option<&str>,
) -> std::io::Result<bool> {
    let mut number = None;
    let created = p.step("GitHub project", &mut || match choice {
        ProjectChoice::Existing(n) => {
            number = Some(*n);
            Ok(format!("{owner} #{n}"))
        }
        ProjectChoice::Create(title) => block_on(client.create_project(owner, title))
            .map(|pr| {
                number = Some(pr.number);
                format!("created {} ({})", pr.title, pr.url)
            })
            .map_err(fail),
    })?;
    let Some(number) = number.filter(|_| created) else {
        return Ok(false);
    };
    let tracker = GithubProjects::new(client.clone(), owner, number, repo.map(String::from));
    let schema = p.step(
        "Project fields (Status, Priority, Size, Work type, Iteration)",
        &mut || {
            block_on(tracker.ensure_schema())
                .map(describe)
                .map_err(fail)
        },
    )?;
    // Save only once the project is known to exist and is set up, so a typo in
    // the owner or number never leaves a broken tracker in the config.
    if !schema {
        return Ok(false);
    }
    let global = ehai_core::paths::global_config(home);
    let saved = p.step("Save tracker settings", &mut || {
        config::edit_file(&global, |t| {
            config::set(t, "tracker", "kind", "github_projects");
            let mut gh = toml::Table::new();
            gh.insert("owner".into(), owner.into());
            gh.insert("project".into(), toml::Value::Integer(number as i64));
            if let Some(r) = repo {
                gh.insert("repo".into(), r.into());
            }
            config::set(t, "tracker", "github", toml::Value::Table(gh));
        })
        .map(|()| global.display().to_string())
        .map_err(|e| StepError {
            message: e.to_string(),
            fix: None,
        })
    })?;
    Ok(saved && schema)
}

pub fn describe(r: SchemaReport) -> String {
    let mut parts = Vec::new();
    if !r.created_fields.is_empty() {
        parts.push(format!("created {}", r.created_fields.join(", ")));
    }
    if !r.added_options.is_empty() {
        parts.push(format!("added {}", r.added_options.join(", ")));
    }
    parts.extend(r.warnings.into_iter().map(|w| format!("note: {w}")));
    if parts.is_empty() {
        "already set up".into()
    } else {
        parts.join("; ")
    }
}

pub fn fail(e: ehai_tracker::Error) -> StepError {
    let fix = match &e {
        ehai_tracker::Error::Auth(_) => Some(format!(
            "run `{}` (or `gh auth login`), then try again",
            auth::SCOPE_FIX
        )),
        ehai_tracker::Error::Http(_) => Some("check your network connection".into()),
        _ => None,
    };
    StepError {
        message: e.to_string(),
        fix,
    }
}

#[derive(Debug, Default, Clone, clap::Args)]
pub struct SetArgs {
    /// Tracker type. Only `github_projects` is available so far.
    #[arg(default_value = "github_projects")]
    pub kind: String,
    /// GitHub user or org that owns the project.
    #[arg(long)]
    pub owner: Option<String>,
    /// Existing project number.
    #[arg(long, conflicts_with = "create")]
    pub project: Option<u64>,
    /// Create a new project with this title.
    #[arg(long, value_name = "TITLE")]
    pub create: Option<String>,
    /// Repo (`owner/name`) to create work items in as issues.
    #[arg(long, conflicts_with = "no_repo")]
    pub repo: Option<String>,
    /// Don't use a repo: items become project draft issues.
    #[arg(long)]
    pub no_repo: bool,
}

pub fn set(p: &mut dyn Prompter, home: &Path, cwd: &Path, args: &SetArgs) -> Result<bool> {
    if args.kind != "github_projects" {
        bail!(
            "`{}` isn't available yet; GitHub Projects (`github_projects`) is",
            args.kind
        );
    }
    p.intro("Connect a tracker")?;
    let (client, token) = client()?;
    let mut viewer = None;
    let signed_in = p.step("GitHub sign-in", &mut || {
        block_on(client.viewer())
            .map(|v| {
                let s = format!("{} (via {})", v.login, token.source);
                viewer = Some(v);
                s
            })
            .map_err(fail)
    })?;
    let Some(viewer) = viewer.filter(|_| signed_in) else {
        p.outro("Not connected.")?;
        return Ok(false);
    };
    let scope_ok = p.step(
        "Projects permission",
        &mut || match block_on(client.scopes()) {
            Ok(s) if auth::has_project_scope(s.as_deref()) == Some(false) => Err(StepError {
                message: "your GitHub login can't manage Projects yet".into(),
                fix: Some(format!(
                    "run `{}`, then rerun this command",
                    auth::SCOPE_FIX
                )),
            }),
            Ok(_) => Ok("ok".into()),
            Err(e) => Err(fail(e)),
        },
    )?;
    if !scope_ok {
        p.outro("Not connected.")?;
        return Ok(false);
    }

    let repo_slug = detected_repo(cwd);
    let owner = match &args.owner {
        Some(o) => o.clone(),
        None => {
            let mut owners = vec![viewer.login.clone()];
            owners.extend(viewer.orgs.iter().cloned());
            let preferred = repo_slug
                .as_deref()
                .and_then(|r| r.split('/').next())
                .filter(|o| owners.iter().any(|x| x == o))
                .unwrap_or(&viewer.login)
                .to_string();
            let choices: Vec<Choice> = owners
                .iter()
                .map(|o| {
                    let c = Choice::new(o, o).hint(if *o == viewer.login {
                        "you"
                    } else {
                        "organization"
                    });
                    if *o == preferred {
                        c.default()
                    } else {
                        c
                    }
                })
                .collect();
            p.select("Whose project?", &choices)?
        }
    };
    let choice = match (&args.project, &args.create) {
        (Some(n), _) => ProjectChoice::Existing(*n),
        (_, Some(t)) => ProjectChoice::Create(t.clone()),
        _ => {
            let projects = block_on(client.projects(&owner)).context("listing projects")?;
            ask_project(
                p,
                &projects,
                &default_title(repo_slug.as_deref()),
                false,
                None,
            )?
            .expect("skip is not offered here")
        }
    };
    let repo = if args.no_repo {
        None
    } else if let Some(r) = &args.repo {
        Some(r.clone())
    } else if let Some(r) = &repo_slug {
        let pick = p.select(
            "Create work items as issues in which repo?",
            &[
                Choice::new(r, r).hint("this repo").default(),
                Choice::new("none", "No repo").hint("project draft items; no comments or PR links"),
            ],
        )?;
        (pick != "none").then_some(pick)
    } else {
        None
    };
    let ok = connect_project(p, home, &client, &owner, &choice, repo.as_deref())?;
    p.outro(if ok {
        "Tracker connected."
    } else {
        "Not fully connected — see above."
    })?;
    Ok(ok)
}

/// `owner/name` of the current repo's GitHub remote, if any.
pub fn detected_repo(cwd: &Path) -> Option<String> {
    let root = ehai_core::git::toplevel(cwd)?;
    ehai_core::git::try_run(&root, &["remote", "get-url", "origin"])
        .and_then(|r| ehai_core::git::github_slug(&r))
}

pub fn default_title(repo: Option<&str>) -> String {
    repo.and_then(|r| r.split('/').nth(1))
        .map_or_else(|| "Product roadmap".to_string(), |n| format!("{n} roadmap"))
}

pub fn status(config: &Config) -> Result<String> {
    let (client, token) = client()?;
    let t = configured(config, client)?;
    let items = block_on(t.list_items(&Default::default()))?;
    let g = &config.tracker.github;
    Ok(format!(
        "GitHub Projects: {} #{} ({} items){}\ntoken: {}",
        g.owner.as_deref().unwrap_or("?"),
        g.project.unwrap_or_default(),
        items.len(),
        g.repo
            .as_deref()
            .map(|r| format!(", issues in {r}"))
            .unwrap_or_default(),
        token.source
    ))
}

pub fn schema(config: &Config) -> Result<String> {
    let (client, _) = client()?;
    let t = configured(config, client)?;
    Ok(describe(block_on(t.ensure_schema())?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::PlainPrompter;

    fn proj(n: u64, title: &str, closed: bool) -> ProjectSummary {
        ProjectSummary {
            owner: "octo".into(),
            number: n,
            title: title.into(),
            url: String::new(),
            closed,
        }
    }

    #[test]
    fn project_question() {
        let ps = [proj(3, "Roadmap", false), proj(1, "Old", true)];
        let ask = |answers: &[&str], ps: &[ProjectSummary], skip: bool, current: Option<u64>| {
            let mut p = PlainPrompter::scripted(Vec::new(), answers);
            ask_project(&mut p, ps, "Roadmap", skip, current)
        };
        // explicit command: most recent open project
        assert_eq!(
            ask(&[], &ps, false, None).unwrap(),
            Some(ProjectChoice::Existing(3))
        );
        // init: skip unless something is already connected
        assert_eq!(ask(&[], &ps, true, None).unwrap(), None);
        assert_eq!(
            ask(&[], &ps, true, Some(3)).unwrap(),
            Some(ProjectChoice::Existing(3))
        );
        assert_eq!(
            ask(&["new", "Clinic app"], &ps, true, None).unwrap(),
            Some(ProjectChoice::Create("Clinic app".into()))
        );
        // only closed projects → create new
        assert_eq!(
            ask(&[], &ps[1..], false, None).unwrap(),
            Some(ProjectChoice::Create("Roadmap".into()))
        );
        // closed projects are not offered
        assert!(ask(&["1"], &ps, true, None).is_err());
    }

    #[test]
    fn titles() {
        assert_eq!(default_title(Some("octo/clinic")), "clinic roadmap");
        assert_eq!(default_title(None), "Product roadmap");
    }
}
