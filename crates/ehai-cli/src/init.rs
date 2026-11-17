//! `ehai init`: detect everything first, show one summary card, ask only for the
//! gaps, then apply with a live checklist (spec §5.0).
//!
//! Nothing is written until every question is answered, so cancelling at any
//! question leaves no half-applied state. Apply steps are idempotent, so rerunning
//! `init` after an interrupted apply simply resumes.

use std::io;
use std::path::{Path, PathBuf};

use ehai_core::config::{self, ComplianceMode, Role, SandboxMode};
use ehai_core::Config;
use ehai_stack::RepoScan;

use ehai_tracker::github::ProjectSummary;

use crate::tracker::{block_on, ProjectChoice};
use crate::ui::{Choice, Prompter, StepError};

/// Answers that can be given as flags (CI / scripting). A flag skips its question.
#[derive(Debug, Default, Clone, clap::Args)]
pub struct InitArgs {
    /// Your role: pm, dev or both.
    #[arg(long, value_parser = ["pm", "dev", "both"])]
    pub role: Option<String>,
    /// Harness to wire up (repeatable), or `none`.
    #[arg(long = "harness", value_name = "ID")]
    pub harnesses: Vec<String>,
    /// Compliance guard: off, warn or strict.
    #[arg(long, value_parser = ["off", "warn", "strict"])]
    pub compliance: Option<String>,
    /// Sandbox for agent-driven work: off, restricted or container.
    #[arg(long, value_parser = ["off", "restricted", "container"])]
    pub sandbox: Option<String>,
    /// Don't create a profile for the current repo.
    #[arg(long)]
    pub no_repo: bool,
    /// Connect this existing GitHub project number.
    #[arg(long, conflicts_with_all = ["tracker_create", "no_tracker"])]
    pub tracker_project: Option<u64>,
    /// Create a GitHub project with this title and connect it.
    #[arg(long, value_name = "TITLE", conflicts_with = "no_tracker")]
    pub tracker_create: Option<String>,
    /// Don't ask about the tracker.
    #[arg(long)]
    pub no_tracker: bool,
}

/// What init found out about GitHub Projects access.
#[derive(Debug, Clone, Default)]
pub struct GithubDetect {
    pub login: String,
    /// Owner whose projects are offered: the repo's org when you belong to it.
    pub owner: String,
    /// `None` = can't tell (fine-grained token).
    pub scope_ok: Option<bool>,
    pub projects: Vec<ProjectSummary>,
    /// `owner/name` of the current repo, used for issues.
    pub repo: Option<String>,
}

/// Everything init detects up front.
#[derive(Debug, Clone, Default)]
pub struct Detected {
    pub os: String,
    pub git: Option<String>,
    pub gh: Option<String>,
    pub gh_auth: bool,
    /// Supported harnesses found on this machine: (id, display name).
    pub harnesses: Vec<(String, String)>,
    /// Harnesses found without an adapter yet.
    pub other_harnesses: Vec<String>,
    pub claude_desktop: bool,
    pub chrome: Option<PathBuf>,
    pub repo: Option<(PathBuf, RepoScan)>,
    pub github: Option<GithubDetect>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answers {
    pub role: String,
    pub harnesses: Vec<String>,
    pub compliance: String,
    pub sandbox: String,
    pub onboard_repo: bool,
    pub tracker: Option<ProjectChoice>,
    /// `true` when the user chose local storage because gh is absent.
    pub use_local_tracker: bool,
}

pub fn run(
    p: &mut dyn Prompter,
    home: &Path,
    cwd: &Path,
    config: &Config,
    args: &InitArgs,
) -> io::Result<bool> {
    let brand = &config.brand.name;
    p.intro(&format!("{brand} {}", ehai_core::VERSION))?;
    let detected = detect(p, cwd, !args.no_tracker)?;
    p.note("Here's what I found", &summary(&detected))?;
    let answers = ask(p, &detected, config, args)?;
    let ok = apply(p, home, config, &detected, &answers)?;
    p.note("Try next", &next_commands(brand, &answers.role))?;
    p.outro(if ok {
        "You're ready."
    } else {
        "Done, with problems above — fix them and rerun init (it picks up where it left off)."
    })?;
    Ok(ok)
}

fn detect(p: &mut dyn Prompter, cwd: &Path, github: bool) -> io::Result<Detected> {
    use crate::sys;
    let mut d = Detected {
        os: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        ..Default::default()
    };
    let os = d.os.clone();
    p.step("System", &mut || Ok(os.clone()))?;
    p.step("Git", &mut || {
        d.git = sys::tool_version("git", &["--version"]);
        d.git.clone().ok_or_else(|| StepError {
            message: "not found".into(),
            fix: Some("install git 2.20+".into()),
        })
    })?;
    p.step("GitHub CLI", &mut || {
        d.gh = sys::tool_version("gh", &["--version"]);
        d.gh_auth = d.gh.is_some() && sys::gh_authenticated();
        Ok(match (&d.gh, d.gh_auth) {
            (Some(_), true) => "found, signed in".into(),
            (Some(_), false) => "found, not signed in (run `gh auth login`)".into(),
            (None, _) => "not found (optional)".into(),
        })
    })?;
    p.step("AI harnesses", &mut || {
        d.harnesses = ehai_adapters::all()
            .iter()
            .filter(|h| h.detect().is_some())
            .map(|h| (h.id().to_string(), h.display_name().to_string()))
            .collect();
        d.other_harnesses = sys::other_harnesses()
            .into_iter()
            .map(String::from)
            .collect();
        let all: Vec<&str> = d
            .harnesses
            .iter()
            .map(|(_, n)| n.as_str())
            .chain(d.other_harnesses.iter().map(String::as_str))
            .collect();
        Ok(if all.is_empty() {
            "none found".into()
        } else {
            all.join(", ")
        })
    })?;
    p.step("Claude desktop & Chrome", &mut || {
        d.claude_desktop = sys::claude_desktop_installed();
        d.chrome = sys::chrome();
        Ok(format!(
            "desktop {}, chrome {}",
            if d.claude_desktop {
                "found"
            } else {
                "not found"
            },
            if d.chrome.is_some() {
                "found"
            } else {
                "not found"
            }
        ))
    })?;
    p.step("Current repo", &mut || {
        d.repo = ehai_core::git::toplevel(cwd).map(|root| {
            let scan = RepoScan::scan(&root);
            (root, scan)
        });
        Ok(match &d.repo {
            Some((root, scan)) => format!("{} ({} apps)", root.display(), scan.apps.len()),
            None => "not in a git repo".into(),
        })
    })?;
    if github {
        p.step("GitHub Projects", &mut || {
            let (client, _) = crate::tracker::client().map_err(|e| StepError {
                message: e.to_string(),
                fix: None,
            })?;
            let viewer = block_on(client.viewer()).map_err(crate::tracker::fail)?;
            let repo = crate::tracker::detected_repo(cwd);
            let owner = repo
                .as_deref()
                .and_then(|r| r.split('/').next())
                .filter(|o| *o == viewer.login || viewer.orgs.iter().any(|x| x == o))
                .unwrap_or(&viewer.login)
                .to_string();
            let scope_ok = block_on(client.scopes())
                .ok()
                .and_then(|s| ehai_tracker::auth::has_project_scope(s.as_deref()));
            let projects = if scope_ok == Some(false) {
                vec![]
            } else {
                block_on(client.projects(&owner)).unwrap_or_default()
            };
            let msg = match scope_ok {
                Some(false) => format!(
                    "signed in as {}, but Projects needs `{}`",
                    viewer.login,
                    ehai_tracker::auth::SCOPE_FIX
                ),
                _ => format!("{} projects for {owner}", projects.len()),
            };
            d.github = Some(GithubDetect {
                login: viewer.login,
                owner,
                scope_ok,
                projects,
                repo,
            });
            Ok(msg)
        })?;
    }
    Ok(d)
}

pub fn summary(d: &Detected) -> String {
    let yes_no = |b: bool| if b { "yes" } else { "no" };
    let mut lines = vec![
        format!("System        {}", d.os),
        format!("Git           {}", d.git.as_deref().unwrap_or("missing")),
        format!(
            "GitHub CLI    {}",
            match (&d.gh, d.gh_auth) {
                (Some(_), true) => "signed in",
                (Some(_), false) => "installed, not signed in",
                (None, _) => "not installed (optional)",
            }
        ),
    ];
    let hs: Vec<&str> = d.harnesses.iter().map(|(_, n)| n.as_str()).collect();
    lines.push(format!(
        "Harnesses     {}",
        if hs.is_empty() {
            "none supported found".into()
        } else {
            hs.join(", ")
        }
    ));
    if !d.other_harnesses.is_empty() {
        lines.push(format!(
            "              also found: {} (support coming soon)",
            d.other_harnesses.join(", ")
        ));
    }
    lines.push(format!(
        "Tracker       {}",
        match &d.github {
            None => "GitHub not reachable (connect later with `tracker set`)".to_string(),
            Some(g) if g.scope_ok == Some(false) => format!(
                "GitHub {} — run `{}` to enable Projects",
                g.login,
                ehai_tracker::auth::SCOPE_FIX
            ),
            Some(g) => format!(
                "GitHub Projects ({} found for {})",
                g.projects.len(),
                g.owner
            ),
        }
    ));
    lines.push(format!("Claude app    {}", yes_no(d.claude_desktop)));
    lines.push(format!(
        "Chrome        {} (POC screenshots)",
        yes_no(d.chrome.is_some())
    ));
    match &d.repo {
        Some((root, scan)) => {
            lines.push(format!("Repo          {}", root.display()));
            for a in &scan.apps {
                lines.push(format!("              {} → {:?}", a.path, a.stack));
            }
        }
        None => lines.push("Repo          none (run init inside a repo to onboard it)".into()),
    }
    lines.join("\n")
}

/// The question round. A flag answers its question; Esc goes back one question.
pub fn ask(
    p: &mut dyn Prompter,
    d: &Detected,
    config: &Config,
    args: &InitArgs,
) -> io::Result<Answers> {
    let mut a = Answers {
        role: args.role.clone().unwrap_or_else(|| {
            config
                .role
                .map_or("both", |r| match r {
                    Role::Pm => "pm",
                    Role::Dev => "dev",
                    Role::Both => "both",
                })
                .into()
        }),
        harnesses: if args.harnesses.is_empty() {
            preselect_harnesses(d, config)
        } else {
            args.harnesses
                .iter()
                .filter(|h| *h != "none")
                .cloned()
                .collect()
        },
        compliance: args
            .compliance
            .clone()
            .unwrap_or_else(|| mode_str(config.compliance.mode)),
        sandbox: args
            .sandbox
            .clone()
            .unwrap_or_else(|| sandbox_str(config.sandbox.mode)),
        onboard_repo: d.repo.is_some() && !args.no_repo,
        tracker: match (&args.tracker_project, &args.tracker_create) {
            (Some(n), _) => Some(ProjectChoice::Existing(*n)),
            (_, Some(t)) => Some(ProjectChoice::Create(t.clone())),
            _ => None,
        },
        use_local_tracker: false,
    };
    let ask_tracker = !args.no_tracker
        && args.tracker_project.is_none()
        && args.tracker_create.is_none()
        && d.github.as_ref().is_some_and(|g| g.scope_ok != Some(false));
    // When gh is absent (and no GITHUB_TOKEN supplied), offer "install gh or
    // use local storage?" rather than silently skipping the tracker question.
    let ask_gh_missing = !args.no_tracker
        && d.gh.is_none()
        && d.github.is_none()
        && std::env::var_os("GITHUB_TOKEN").is_none()
        && std::env::var_os("GH_TOKEN").is_none();

    // Questions in order; each is skipped when a flag (or detection) answered it.
    #[derive(Clone, Copy)]
    enum Q {
        Role,
        Harnesses,
        GhMissing,
        Tracker,
        Compliance,
        Sandbox,
        Repo,
    }
    let mut todo = Vec::new();
    if args.harnesses.is_empty() && !d.harnesses.is_empty() {
        todo.push(Q::Harnesses);
    }
    if ask_gh_missing {
        todo.push(Q::GhMissing);
    } else if ask_tracker {
        todo.push(Q::Tracker);
    } else if args.role.is_none() && admin_of_configured(d, config) == Some(true) {
        // tracker already connected and known to be administered by the
        // viewer: ask the role question now, in the same place it would run
        // right after Q::Tracker below.
        todo.push(Q::Role);
    }
    if args.compliance.is_none() {
        todo.push(Q::Compliance);
    }
    if args.sandbox.is_none() {
        todo.push(Q::Sandbox);
    }
    if a.onboard_repo {
        todo.push(Q::Repo);
    }

    let mut i = 0;
    while i < todo.len() {
        let result = match todo[i] {
            Q::Role => crate::role::ask_role(p, &a.role).map(|v| a.role = v),
            Q::Harnesses => {
                let choices: Vec<Choice> = d
                    .harnesses
                    .iter()
                    .map(|(id, name)| {
                        let c = Choice::new(id, name);
                        if a.harnesses.contains(id) {
                            c.default()
                        } else {
                            c
                        }
                    })
                    .collect();
                p.multiselect("Which tools should I set up?", &choices)
                    .map(|v| a.harnesses = v)
            }
            Q::GhMissing => p
                .select(
                    "GitHub CLI (gh) not found — it is needed for GitHub Projects sync.",
                    &[
                        Choice::new("local", "Use local storage (all PM flows work offline)")
                            .hint("stores items in ~/.ehai/tracker.json; add GitHub later with `ehai tracker set github`")
                            .default(),
                        Choice::new(
                            "install",
                            "Install gh first (https://cli.github.com) and re-run init",
                        )
                        .hint("required for GitHub Projects, PR linking, and team sync"),
                    ],
                )
                .map(|v| {
                    if v == "local" {
                        a.use_local_tracker = true;
                    }
                }),
            Q::Tracker => {
                let g = d
                    .github
                    .as_ref()
                    .expect("asked only when GitHub was detected");
                let current = (config.tracker.github.owner.as_deref() == Some(g.owner.as_str()))
                    .then_some(config.tracker.github.project)
                    .flatten();
                crate::tracker::ask_project(
                    p,
                    &g.projects,
                    &crate::tracker::default_title(g.repo.as_deref()),
                    true,
                    current,
                )
                .map(|v| a.tracker = v)
            }
            Q::Compliance => p
                .select(
                    "How strict should the patient-data guard be?",
                    &[
                        mark(
                            Choice::new("warn", "Warn")
                                .hint("flag possible PHI and ask before writing"),
                            &a.compliance,
                        ),
                        mark(
                            Choice::new("strict", "Strict")
                                .hint("block and redact PHI, synthetic data only"),
                            &a.compliance,
                        ),
                        mark(Choice::new("off", "Off").hint("no checks"), &a.compliance),
                    ],
                )
                .map(|v| a.compliance = v),
            Q::Sandbox => p
                .select(
                    "How should agent-run commands be sandboxed?",
                    &[
                        mark(
                            Choice::new("restricted", "Restricted")
                                .hint("writes only inside the POC worktree"),
                            &a.sandbox,
                        ),
                        mark(
                            Choice::new("container", "Container")
                                .hint("Docker/Podman, if installed"),
                            &a.sandbox,
                        ),
                        mark(Choice::new("off", "Off").hint("no sandbox"), &a.sandbox),
                    ],
                )
                .map(|v| a.sandbox = v),
            Q::Repo => {
                let (_, scan) = d
                    .repo
                    .as_ref()
                    .expect("Repo question only when a repo exists");
                p.note("This repo", &scan.summary())
                    .and_then(|()| {
                        p.confirm("Save this as the repo profile (.ehai/profile.toml)?", true)
                    })
                    .map(|v| a.onboard_repo = v)
            }
        };
        match result {
            Ok(()) => {
                // The tracker question just resolved who (if anyone) is being
                // connected; only an admin of that project gets asked a role.
                if matches!(todo[i], Q::Tracker) && args.role.is_none() {
                    let is_admin = match &a.tracker {
                        None => false,                          // connecting was skipped
                        Some(ProjectChoice::Create(_)) => true, // the creator administers it
                        Some(ProjectChoice::Existing(n)) => d
                            .github
                            .as_ref()
                            .is_some_and(|g| admin_of(&g.projects, &g.owner, *n) == Some(true)),
                    };
                    if is_admin {
                        todo.insert(i + 1, Q::Role);
                    }
                }
                i += 1;
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {
                // cliclack reports Esc and Ctrl-C alike; offer back, quit on a second cancel.
                let back = p.select(
                    "Cancelled",
                    &[
                        Choice::new("back", "Go back").default(),
                        Choice::new("quit", "Quit without changing anything"),
                    ],
                );
                match back.as_deref() {
                    Ok("back") => i = i.saturating_sub(1),
                    _ => return Err(io::ErrorKind::Interrupted.into()),
                }
            }
            Err(e) => return Err(e),
        }
    }
    Ok(a)
}

/// Looks up `viewerCanClose` for `owner #number` from the already-detected
/// (open) project list — no extra network call. `None` when it isn't there
/// (closed, or a different owner), in which case the caller treats it as
/// "can't tell" and defaults to not asking.
fn admin_of(projects: &[ProjectSummary], owner: &str, number: u64) -> Option<bool> {
    projects
        .iter()
        .find(|p| p.owner == owner && p.number == number)
        .map(|p| p.viewer_is_admin)
}

/// Is the project already connected in `config` one the viewer administers?
/// Only answerable when that project happens to be in the detected list; see
/// [`admin_of`].
fn admin_of_configured(d: &Detected, config: &Config) -> Option<bool> {
    let g = d.github.as_ref()?;
    let owner = config.tracker.github.owner.as_deref()?;
    let number = config.tracker.github.project?;
    admin_of(&g.projects, owner, number)
}

/// Mark the choice matching the current value as the default.
fn mark(c: Choice, current: &str) -> Choice {
    if c.value == current {
        c.default()
    } else {
        c
    }
}

/// Which detected harnesses start pre-checked in the "set up which tools?" question.
fn preselect_harnesses(d: &Detected, config: &Config) -> Vec<String> {
    // ponytail: every detected harness, minus ones the user previously left out
    // when a prior init recorded choices. Smarter ranking can come from prefs (M6).
    let previously_chosen = &config.adapters.enabled;
    d.harnesses
        .iter()
        .map(|(id, _)| id.clone())
        .filter(|id| previously_chosen.is_empty() || previously_chosen.contains(id))
        .collect()
}

fn mode_str(m: ComplianceMode) -> String {
    match m {
        ComplianceMode::Off => "off",
        ComplianceMode::Warn => "warn",
        ComplianceMode::Strict => "strict",
    }
    .into()
}

fn sandbox_str(m: SandboxMode) -> String {
    match m {
        SandboxMode::Off => "off",
        SandboxMode::Restricted => "restricted",
        SandboxMode::Container => "container",
    }
    .into()
}

fn apply(
    p: &mut dyn Prompter,
    home: &Path,
    config: &Config,
    d: &Detected,
    a: &Answers,
) -> io::Result<bool> {
    let mut ok = true;
    let global = ehai_core::paths::global_config(home);

    // So harnesses get wired to a stable location (not wherever this binary
    // happened to be run from, e.g. a build dir or a download folder) and the
    // command is on PATH without a separate step — "do it for the user"
    // (spec §2.10). Already-installed is a fast, idempotent no-op.
    ok &= retrying(p, "Install ehai", &mut || {
        crate::selfcmd::install(home, config)
            .map(|lines| lines.join("; "))
            .map_err(|e| StepError {
                message: e.to_string(),
                fix: Some(format!("check that {} is writable", home.display())),
            })
    })?;

    ok &= retrying(p, "Save settings", &mut || {
        config::edit_file(&global, |t| {
            config::set(t, "", "role", a.role.as_str());
            config::set(t, "compliance", "mode", a.compliance.as_str());
            config::set(t, "sandbox", "mode", a.sandbox.as_str());
            config::set(
                t,
                "adapters",
                "enabled",
                toml::Value::Array(a.harnesses.iter().map(|h| h.as_str().into()).collect()),
            );
        })
        .map(|()| global.display().to_string())
        .map_err(|e| StepError {
            message: e.to_string(),
            fix: Some(format!("check that {} is writable", global.display())),
        })
    })?;

    let mut updated = config.clone();
    updated.role = match a.role.as_str() {
        "pm" => Some(Role::Pm),
        "dev" => Some(Role::Dev),
        _ => Some(Role::Both),
    };
    let ctx = crate::adapter_ctx(home, &updated);
    for id in &a.harnesses {
        let Some(h) = ehai_adapters::by_id(id) else {
            p.warn(&format!("unknown harness `{id}` skipped"))?;
            continue;
        };
        ok &= retrying(p, &format!("Set up {}", h.display_name()), &mut || {
            h.install(&ctx).map_err(|e| StepError {
                fix: Some(match &e {
                    ehai_adapters::Error::NotDetected(name) => {
                        format!("install {name}, then rerun init")
                    }
                    _ => format!(
                        "rerun `{} adapters install {id}` for details",
                        updated.brand.name
                    ),
                }),
                message: e.to_string(),
            })
        })?;
    }

    if a.use_local_tracker {
        ok &= retrying(p, "Set up local tracker", &mut || {
            config::edit_file(&global, |t| {
                config::set(t, "tracker", "kind", "local");
            })
            .map(|()| "~/.ehai/tracker.json".to_string())
            .map_err(|e| StepError {
                message: e.to_string(),
                fix: Some(format!("check that {} is writable", global.display())),
            })
        })?;
    }

    if let Some(choice) = &a.tracker {
        match crate::tracker::client() {
            Ok((client, _)) => {
                let (owner, repo) = d.github.as_ref().map_or_else(
                    || {
                        (
                            config.tracker.github.owner.clone().unwrap_or_default(),
                            None,
                        )
                    },
                    |g| (g.owner.clone(), g.repo.clone()),
                );
                let (connected, _is_admin) = crate::tracker::connect_project(
                    p,
                    home,
                    &client,
                    &owner,
                    choice,
                    repo.as_deref(),
                )?;
                ok &= connected;
            }
            Err(e) => {
                ok &= p.step("GitHub project", &mut || {
                    Err(StepError {
                        message: e.to_string(),
                        fix: Some("run `gh auth login`, or set GITHUB_TOKEN".into()),
                    })
                })?;
            }
        }
    }

    if let (true, Some((root, scan))) = (a.onboard_repo, &d.repo) {
        ok &= retrying(p, "Save repo profile", &mut || {
            scan.write_profile(root)
                .map(|path| path.display().to_string())
                .map_err(|e| StepError {
                    message: e.to_string(),
                    fix: None,
                })
        })?;
    }
    Ok(ok)
}

/// A checklist step that offers a retry on failure (default no, so `--yes` and
/// non-interactive runs never loop).
fn retrying(
    p: &mut dyn Prompter,
    label: &str,
    work: &mut dyn FnMut() -> crate::ui::StepResult,
) -> io::Result<bool> {
    for _ in 0..3 {
        if p.step(label, work)? {
            return Ok(true);
        }
        if !p.confirm("Retry this step?", false)? {
            break;
        }
    }
    Ok(false)
}

fn next_commands(brand: &str, role: &str) -> String {
    let lines = match role {
        "pm" => [
            format!("/{brand}:help      in Claude: what I can do for you"),
            format!("/{brand}:prefs     in Claude: your role and preferences"),
            format!("{brand} doctor     if anything looks off"),
        ],
        "dev" => [
            format!("{brand} repo init  profile another repo"),
            format!("/{brand}:help      in your harness: commands and status"),
            format!("{brand} doctor     health check"),
        ],
        _ => [
            format!("/{brand}:help      in Claude: what I can do"),
            format!("{brand} repo init  profile a repo"),
            format!("{brand} doctor     health check"),
        ],
    };
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::PlainPrompter;

    fn detected(repo: Option<&Path>) -> Detected {
        Detected {
            os: "linux x86_64".into(),
            git: Some("git version 2.53.0".into()),
            gh: Some("gh version 2.74.0".into()),
            gh_auth: true,
            harnesses: vec![("claude-code".into(), "Claude Code".into())],
            other_harnesses: vec!["Cursor".into()],
            claude_desktop: true,
            chrome: Some("/usr/bin/google-chrome".into()),
            repo: repo.map(|r| (r.to_path_buf(), RepoScan::default())),
            github: None,
        }
    }

    #[test]
    fn defaults_prefill_every_answer() {
        let mut p = PlainPrompter::new(Vec::new());
        let a = ask(
            &mut p,
            &detected(Some(Path::new("/r"))),
            &Config::default(),
            &InitArgs::default(),
        )
        .unwrap();
        assert_eq!(
            a,
            Answers {
                role: "both".into(),
                harnesses: vec!["claude-code".into()],
                compliance: "warn".into(),
                sandbox: "restricted".into(),
                onboard_repo: true,
                tracker: None,
                use_local_tracker: false,
            }
        );
        let out = String::from_utf8(p.into_inner()).unwrap();
        assert_eq!(
            out.lines().filter(|l| l.starts_with("? ")).count(),
            4,
            "at most 5 questions, and Role is skipped entirely with no GitHub \
             detected (nobody to confirm as admin):\n{out}"
        );
    }

    #[test]
    fn scripted_answers_and_flags_skip_questions() {
        let args = InitArgs {
            sandbox: Some("off".into()),
            ..Default::default()
        };
        // Harnesses, then Compliance: Role is skipped (no GitHub detected) and
        // Sandbox is skipped (answered by the flag).
        let mut p = PlainPrompter::scripted(Vec::new(), &["", "strict"]);
        let a = ask(&mut p, &detected(None), &Config::default(), &args).unwrap();
        assert_eq!(a.role, "both", "not asked, so it keeps its default");
        assert!(a.harnesses.is_empty(), "user unchecked every harness");
        assert_eq!(a.compliance, "strict");
        assert_eq!(a.sandbox, "off");
        assert!(!a.onboard_repo);
        let out = String::from_utf8(p.into_inner()).unwrap();
        assert!(
            !out.contains("sandboxed"),
            "flag skipped the sandbox question"
        );
    }

    #[test]
    fn existing_config_becomes_the_default() {
        let cfg = Config {
            role: Some(Role::Dev),
            compliance: config::Compliance {
                mode: ComplianceMode::Strict,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut p = PlainPrompter::new(Vec::new());
        let a = ask(&mut p, &detected(None), &cfg, &InitArgs::default()).unwrap();
        assert_eq!((a.role.as_str(), a.compliance.as_str()), ("dev", "strict"));
    }

    #[test]
    fn tracker_question_defaults_to_skip_and_respects_scope() {
        let mut d = detected(None);
        let gh = GithubDetect {
            login: "octo".into(),
            owner: "acme".into(),
            scope_ok: Some(true),
            projects: vec![ProjectSummary {
                owner: "acme".into(),
                number: 4,
                title: "Clinic".into(),
                url: String::new(),
                closed: false,
                viewer_is_admin: true,
            }],
            repo: Some("acme/clinic".into()),
        };
        d.github = Some(gh.clone());
        let mut p = PlainPrompter::new(Vec::new());
        assert_eq!(
            ask(&mut p, &d, &Config::default(), &InitArgs::default())
                .unwrap()
                .tracker,
            None
        );
        // Harnesses, then Tracker; the viewer administers #4, so Role is
        // inserted right after Tracker is answered.
        let mut p = PlainPrompter::scripted(Vec::new(), &["claude-code", "4", "both"]);
        let a = ask(&mut p, &d, &Config::default(), &InitArgs::default()).unwrap();
        assert_eq!(a.tracker, Some(ProjectChoice::Existing(4)));
        assert_eq!(a.role, "both");

        // a project the viewer does NOT administer: no Role question at all
        let mut non_admin_gh = gh.clone();
        non_admin_gh.projects[0].viewer_is_admin = false;
        d.github = Some(non_admin_gh);
        let mut p = PlainPrompter::scripted(Vec::new(), &["claude-code", "4"]);
        let a = ask(&mut p, &d, &Config::default(), &InitArgs::default()).unwrap();
        assert_eq!(
            (a.tracker, a.role.as_str()),
            (Some(ProjectChoice::Existing(4)), "both"),
            "role stays at its default"
        );
        let out = String::from_utf8(p.into_inner()).unwrap();
        assert!(!out.contains("What's your role?"), "{out}");

        // missing scope: no question, the summary explains the fix
        d.github = Some(GithubDetect {
            scope_ok: Some(false),
            projects: vec![],
            ..gh
        });
        let mut p = PlainPrompter::new(Vec::new());
        ask(&mut p, &d, &Config::default(), &InitArgs::default()).unwrap();
        assert!(!String::from_utf8(p.into_inner())
            .unwrap()
            .contains("tracked"));
        assert!(summary(&d).contains("gh auth refresh -s project"));
    }

    #[test]
    fn summary_card_snapshot() {
        insta::assert_snapshot!(summary(&detected(None)));
    }

    #[test]
    fn apply_writes_config_and_profile() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let mut d = detected(Some(repo.path()));
        d.harnesses.clear();
        let a = Answers {
            role: "pm".into(),
            harnesses: vec![],
            compliance: "strict".into(),
            sandbox: "off".into(),
            onboard_repo: true,
            tracker: None,
            use_local_tracker: false,
        };
        let mut p = PlainPrompter::new(Vec::new());
        assert!(apply(&mut p, home.path(), &Config::default(), &d, &a).unwrap());
        let cfg = Config::load(&ehai_core::LoadOptions {
            global: Some(ehai_core::paths::global_config(home.path())),
            ..Default::default()
        })
        .unwrap();
        assert_eq!(cfg.role, Some(Role::Pm));
        assert_eq!(cfg.compliance.mode, ComplianceMode::Strict);
        assert!(ehai_stack::profile_exists(repo.path()));
    }

    #[test]
    fn next_commands_differ_by_role() {
        assert_ne!(next_commands("ehai", "pm"), next_commands("ehai", "dev"));
        assert!(next_commands("acme", "pm").contains("/acme:help"));
    }
}
