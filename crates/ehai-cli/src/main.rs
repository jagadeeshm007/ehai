use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use ehai_core::{paths, Config, LoadOptions};

mod doctor;
mod env;
mod init;
mod logging;
mod migration;
mod poc;
mod prefs;
mod role;
mod selfcmd;
mod sys;
mod team;
mod tracker;
mod ui;
mod update;

pub const VERSION_LINE: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    " (",
    env!("EHAI_GIT_SHA"),
    " ",
    env!("EHAI_BUILD_DATE"),
    " ",
    env!("EHAI_TARGET"),
    ")"
);

/// AI-SDLC ops engine: plan, track and prototype from any AI coding harness.
#[derive(Parser)]
#[command(name = "ehai", version = VERSION_LINE)]
struct Cli {
    /// Accept defaults for every question (non-interactive).
    #[arg(long, short = 'y', global = true)]
    yes: bool,
    /// Extra config file, applied above global and repo config.
    #[arg(long, global = true, value_name = "FILE")]
    config: Option<PathBuf>,
    /// More logging on stderr (-v info, -vv debug, -vvv trace).
    #[arg(long, short, global = true, action = clap::ArgAction::Count)]
    verbose: u8,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Check that ehai and its environment are healthy.
    Doctor {
        /// Run as the post-update gate (non-zero exit triggers rollback).
        #[arg(long)]
        post_update: bool,
    },
    /// Guided setup: detect your tools, ask a few questions, wire everything up.
    Init(init::InitArgs),
    /// Repo onboarding.
    #[command(subcommand)]
    Repo(RepoCmd),
    /// Connect ehai to an AI harness (e.g. `ehai connect claude-code`).
    Connect {
        /// Harness id; see `ehai adapters list`.
        #[arg(required = true)]
        ids: Vec<String>,
    },
    /// Disconnect ehai from an AI harness, or from all of them with `--all`.
    Disconnect {
        /// Harness id; see `ehai adapters list`.
        ids: Vec<String>,
        /// Disconnect every harness.
        #[arg(long, conflicts_with = "ids")]
        all: bool,
    },
    /// Manage harness integrations (Claude Code, ...).
    #[command(subcommand)]
    Adapters(AdaptersCmd),
    /// Manage the ehai installation itself.
    #[command(name = "self", subcommand)]
    SelfCmd(SelfCmd),
    /// Remove ehai completely: harness integrations, PATH entry, ~/.ehai.
    Uninstall,
    /// Connect and inspect the work tracker (GitHub Projects).
    #[command(subcommand)]
    Tracker(TrackerCmd),
    /// Show or change your role (only a project admin can set one).
    Role {
        #[command(subcommand)]
        cmd: Option<RoleCmd>,
    },
    /// Who can be assigned work and @mentioned on the connected project.
    Team {
        /// Re-check now instead of using the (at most a day old) cache.
        #[arg(long)]
        refresh: bool,
    },
    /// Make a worktree runnable: gitignored config, ports, dependencies, health check.
    #[command(subcommand)]
    Env(EnvCmd),
    /// Create and manage UI/UX proof-of-concept worktrees.
    #[command(subcommand)]
    Poc(PocCmd),
    /// View and edit output preferences (role, style, planning granularity).
    #[command(subcommand)]
    Prefs(PrefsCmd),
    /// Run the MCP server on stdio (started by your AI harness, not by hand).
    Mcp,
    /// Inspect configuration.
    #[command(subcommand)]
    Config(ConfigCmd),
    /// Export or import config, prefs, and local tracker for device migration.
    #[command(subcommand)]
    Migration(migration::MigrationCmd),
    /// Check for and install a newer release, with automatic rollback if it fails.
    Update {
        /// Override the configured update channel.
        #[arg(long, value_parser = ["alpha", "beta", "stable"])]
        channel: Option<String>,
        /// Restore the previous binary instead of checking for an update.
        #[arg(long)]
        rollback: bool,
    },
}

#[derive(Subcommand)]
enum RepoCmd {
    /// Detect this repo's stack and conventions and save `.ehai/profile.toml`.
    Init,
}

#[derive(Subcommand)]
enum AdaptersCmd {
    /// Show every adapter and whether it is installed.
    List,
    /// Generate and register one adapter (e.g. `claude-code`).
    Install { id: String },
    /// Unregister one adapter and delete its generated files.
    Remove { id: String },
    /// Re-render every enabled adapter (after an update or a brand change).
    Regenerate,
}

#[derive(Subcommand)]
enum TrackerCmd {
    /// Pick (or create) the GitHub project to track work in.
    Set(tracker::SetArgs),
    /// Show the connected project and how many items it has.
    Status,
    /// Create any missing project fields (Status, Priority, Size, Work type, Iteration).
    Schema,
}

#[derive(Subcommand)]
enum RoleCmd {
    /// Choose a role. Refused unless you administer the connected project.
    Set {
        #[arg(value_parser = ["pm", "dev", "both"])]
        role: String,
    },
}

#[derive(Subcommand)]
enum EnvCmd {
    /// Discover and copy gitignored config, allocate ports, install dependencies, run the health check.
    Sync {
        /// Worktree to provision; defaults to the current directory.
        path: Option<PathBuf>,
    },
    /// Compare `.env*` against `.env.example`/`.env.sample`: which keys are missing.
    Check { path: Option<PathBuf> },
    /// Read-only: missing keys plus the configured smoke checks.
    Doctor { path: Option<PathBuf> },
}

#[derive(Subcommand)]
enum PocCmd {
    /// Create a new POC in an isolated worktree.
    New {
        feature: String,
        /// What to build (passed to Claude as the design brief).
        prompt: String,
    },
    /// Iterate on an existing POC.
    Iterate {
        feature: String,
        /// POC number to iterate on.
        parent: u32,
        /// Change request / feedback.
        feedback: String,
    },
    /// List POCs (all features, or one).
    List { feature: Option<String> },
    /// Archive a POC branch and remove its worktree.
    Discard { feature: String, n: u32 },
    /// Open a side-by-side HTML comparison report in the browser.
    Compare {
        feature: String,
        /// POC numbers to compare (space-separated).
        ns: Vec<u32>,
    },
    /// Print the worktree path, dev-server command, and URL for a POC.
    Open { feature: String, n: u32 },
    /// Synthesise a new POC from parts of two or more existing POCs.
    Compose {
        feature: String,
        description: String,
        /// Source POC numbers (repeat: --from 1 --from 2).
        #[arg(long = "from", value_name = "N")]
        sources: Vec<u32>,
    },
    /// Promote a POC by opening a GitHub pull request to the default branch.
    Promote { feature: String, n: u32 },
}

#[derive(Subcommand)]
enum PrefsCmd {
    /// List all preferences (global merged with repo).
    List,
    /// Set a preference value.
    Set {
        key: String,
        value: String,
        /// `global` (default) or `repo`.
        #[arg(long, default_value = "global")]
        scope: String,
    },
    /// Delete a preference.
    Delete {
        key: String,
        /// `global` (default) or `repo`.
        #[arg(long, default_value = "global")]
        scope: String,
    },
}

#[derive(Subcommand)]
enum SelfCmd {
    /// Copy this binary to ~/.ehai/bin and put it on your PATH.
    Install,
}

#[derive(Subcommand)]
enum ConfigCmd {
    /// Print the effective (merged) configuration.
    Show,
    /// Print the config file locations.
    Path,
    /// Print the JSON schema of the config file.
    Schema,
}

fn main() -> ExitCode {
    // `ehai … | head` closes stdout early. Exit quietly like other Unix tools
    // instead of panicking. ponytail: one hook for every println!, rather than
    // threading a writer through each command.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let msg = info
            .payload()
            .downcast_ref::<String>()
            .map(String::as_str)
            .unwrap_or_default();
        if msg.contains("failed printing to stdout") && msg.contains("Broken pipe") {
            std::process::exit(0);
        }
        default_hook(info);
    }));
    match run() {
        Ok(code) => code,
        Err(e) if is_broken_pipe(&e) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn is_broken_pipe(e: &anyhow::Error) -> bool {
    e.chain()
        .filter_map(|c| c.downcast_ref::<std::io::Error>())
        .any(|io| io.kind() == std::io::ErrorKind::BrokenPipe)
}

fn run() -> Result<ExitCode> {
    let cli = Cli::parse();
    let home = paths::ehai_home()?;
    // uninstall must not recreate ~/.ehai/logs while deleting it
    let logs = (!matches!(cli.command, Command::Uninstall)).then(|| paths::logs_dir(&home));
    let _log_guard = logging::init(logs.as_deref(), cli.verbose);
    tracing::info!(version = VERSION_LINE, home = %home.display(), "start");

    let global = paths::global_config(&home);
    if let Some(backup) = ehai_core::migrate::migrate_file(&global)? {
        tracing::info!(backup = %backup.display(), "migrated config");
    }
    let opts = LoadOptions {
        explicit: cli.config.clone(),
        ..LoadOptions::from_process(&home)
    };
    let config = Config::load(&opts);

    // Background update check (spec §11): at most once a day, notify-only.
    // Skipped for `mcp` (must keep stdout clean for the protocol), `update`
    // itself, and `doctor` (already a health check). Runs after the command's
    // own output, so it never delays the thing the user actually asked for.
    let skip_notify = matches!(
        cli.command,
        Command::Mcp | Command::Update { .. } | Command::Doctor { .. } | Command::Uninstall
    );
    let notify_settings = config
        .as_ref()
        .ok()
        .map(|c| (c.update.check, c.update.channel));

    let result = run_command(cli, home.clone(), config);

    if !skip_notify {
        if let Some((check, channel)) = notify_settings {
            if check {
                update::maybe_notify(&home, channel);
            }
        }
    }
    result
}

fn run_command(cli: Cli, home: PathBuf, config: ehai_core::Result<Config>) -> Result<ExitCode> {
    match cli.command {
        Command::Mcp => {
            let config = config?;
            let ctx = ehai_mcp::Context {
                config,
                home: home.clone(),
                cwd: std::env::current_dir().ok(),
                github: ehai_mcp::Github::FromEnv,
            };
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?
                .block_on(ehai_mcp::serve_stdio(ctx))?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Init(args) => {
            let config = config?;
            let cwd = std::env::current_dir()?;
            let mut p = ui::prompter(cli.yes);
            let ok = cancellable(init::run(p.as_mut(), &home, &cwd, &config, &args))?;
            Ok(if ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        Command::Repo(RepoCmd::Init) => {
            let cwd = std::env::current_dir()?;
            let root = ehai_core::git::toplevel(&cwd).context("not inside a git repository")?;
            let mut p = ui::prompter(cli.yes);
            let scan = ehai_stack::RepoScan::scan(&root);
            cancellable((|| {
                p.intro(&format!(
                    "{} repo init",
                    config.as_ref().map_or("ehai", |c| c.brand.name.as_str())
                ))?;
                p.note(&root.display().to_string(), &scan.summary())?;
                if p.confirm("Save this as the repo profile (.ehai/profile.toml)?", true)? {
                    let path = scan.write_profile(&root).map_err(std::io::Error::other)?;
                    p.outro(&format!("Saved {}", path.display()))?;
                } else {
                    p.outro("Nothing saved")?;
                }
                Ok(())
            })())?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Role { cmd } => {
            let config = config?;
            match cmd {
                None => println!("{}", role::show(&config)),
                Some(RoleCmd::Set { role: r }) => println!("{}", role::set(&home, &config, &r)?),
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Team { refresh } => {
            let config = config?;
            println!("{}", team::run(&home, &config, refresh)?);
            Ok(ExitCode::SUCCESS)
        }
        Command::Tracker(cmd) => {
            let config = config?;
            match cmd {
                TrackerCmd::Set(args) => {
                    let cwd = std::env::current_dir()?;
                    let mut p = ui::prompter(cli.yes);
                    let ok = tracker::set(p.as_mut(), &home, &cwd, &args)
                        .inspect_err(exit_if_cancelled)?;
                    return Ok(if ok {
                        ExitCode::SUCCESS
                    } else {
                        ExitCode::FAILURE
                    });
                }
                TrackerCmd::Status => println!("{}", tracker::status(&config)?),
                TrackerCmd::Schema => println!("{}", tracker::schema(&config)?),
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Env(cmd) => {
            let config = config?;
            match cmd {
                EnvCmd::Sync { path } => {
                    println!("{}", env::sync(&home, &config, path.as_deref())?)
                }
                EnvCmd::Check { path } => println!("{}", env::check(path.as_deref())?),
                EnvCmd::Doctor { path } => println!("{}", env::doctor(&config, path.as_deref())?),
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Poc(cmd) => {
            let config = config?;
            match cmd {
                PocCmd::New { feature, prompt } => {
                    println!("{}", poc::new_poc(&home, &config, &feature, &prompt)?)
                }
                PocCmd::Iterate {
                    feature,
                    parent,
                    feedback,
                } => {
                    println!(
                        "{}",
                        poc::iterate_poc(&home, &config, &feature, parent, &feedback)?
                    )
                }
                PocCmd::List { feature } => {
                    println!("{}", poc::list_pocs(&config, feature.as_deref())?)
                }
                PocCmd::Discard { feature, n } => {
                    println!("{}", poc::discard_poc(&home, &config, &feature, n)?)
                }
                PocCmd::Compare { feature, ns } => {
                    println!("{}", poc::compare_pocs(&home, &config, &feature, &ns)?)
                }
                PocCmd::Open { feature, n } => {
                    println!("{}", poc::open_poc(&config, &feature, n)?)
                }
                PocCmd::Compose {
                    feature,
                    description,
                    sources,
                } => {
                    // Build (n, description) pairs from --from flags (description is empty; Claude synthesises)
                    let src: Vec<(u32, String)> =
                        sources.into_iter().map(|n| (n, String::new())).collect();
                    println!(
                        "{}",
                        poc::compose_poc(&home, &config, &feature, &description, src)?
                    )
                }
                PocCmd::Promote { feature, n } => {
                    // Always confirm before promoting; --yes does not bypass this
                    let mut p = ui::prompter(false);
                    let ok = cancellable(p.confirm(
                        &format!(
                            "Promote poc-{n} of '{feature}' to main? This creates a GitHub PR."
                        ),
                        false,
                    ))?;
                    if !ok {
                        println!("Cancelled.");
                        return Ok(ExitCode::FAILURE);
                    }
                    println!("{}", poc::promote_poc(&home, &config, &feature, n)?)
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Prefs(cmd) => {
            let config = config?;
            match cmd {
                PrefsCmd::List => println!("{}", prefs::list_prefs(&home, &config)?),
                PrefsCmd::Set { key, value, scope } => {
                    println!("{}", prefs::set_pref(&home, &config, &key, &value, &scope)?)
                }
                PrefsCmd::Delete { key, scope } => {
                    println!("{}", prefs::delete_pref(&home, &config, &key, &scope)?)
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Connect { ids } => {
            let config = config?;
            let ctx = adapter_ctx(&home, &config);
            for id in &ids {
                let h = find_adapter(id)?;
                println!("✔ {}: {}", h.display_name(), h.install(&ctx)?);
                set_enabled(&home, id, true)?;
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Disconnect { ids, all } => {
            let config = config?;
            let ctx = adapter_ctx(&home, &config);
            let every: Vec<String> = ehai_adapters::all()
                .iter()
                .map(|h| h.id().to_string())
                .collect();
            anyhow::ensure!(
                all || !ids.is_empty(),
                "name a harness to disconnect ({}) or pass --all",
                every.join(", ")
            );
            let targets = if all { every } else { ids };
            for id in &targets {
                let h = find_adapter(id)?;
                println!("✔ {}: {}", h.display_name(), h.remove(&ctx)?);
                set_enabled(&home, id, false)?;
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::SelfCmd(SelfCmd::Install) => {
            let config = config?;
            for line in selfcmd::install(&home, &config)? {
                println!("✔ {line}");
            }
            // re-point enabled adapters at the installed binary
            let ctx = adapter_ctx(&home, &config);
            for id in &config.adapters.enabled {
                println!("✔ {}", find_adapter(id)?.install(&ctx)?);
            }
            println!(
                "Open a new terminal (or `source` your shell rc) to use `{}`.",
                config.brand.name
            );
            Ok(ExitCode::SUCCESS)
        }
        Command::Uninstall => {
            let brand = config
                .as_ref()
                .map_or("ehai".to_string(), |c| c.brand.name.clone());
            if !cli.yes {
                let mut p = ui::prompter(false);
                let sure = cancellable(p.confirm(
                    &format!(
                        "Remove {brand} completely? This disconnects every harness, removes the PATH entry and deletes {}",
                        home.display()
                    ),
                    false,
                ))?;
                if !sure {
                    println!("Nothing removed. (Use --yes to skip this question.)");
                    return Ok(ExitCode::FAILURE);
                }
            }
            // a broken config must not block uninstall: fall back to defaults
            let config = config.unwrap_or_default();
            for line in selfcmd::remove_adapters(&home, &config) {
                println!("✔ {line}");
            }
            for line in selfcmd::remove_files(&home, std::env::home_dir().as_deref())? {
                println!("✔ {line}");
            }
            println!("{brand} is uninstalled. Repo profiles (.ehai/ inside your repos) are left in place.");
            Ok(ExitCode::SUCCESS)
        }
        Command::Adapters(cmd) => {
            let config = config?;
            let ctx = adapter_ctx(&home, &config);
            let find = find_adapter;
            match cmd {
                AdaptersCmd::List => {
                    for h in ehai_adapters::all() {
                        let enabled = config.adapters.enabled.iter().any(|e| e == h.id());
                        println!(
                            "{:<12} {:<14} {:?}{}",
                            h.id(),
                            h.display_name(),
                            h.status(&ctx),
                            if enabled { " (enabled)" } else { "" }
                        );
                    }
                }
                AdaptersCmd::Install { id } => {
                    let h = find(&id)?;
                    println!("{}", h.install(&ctx)?);
                    set_enabled(&home, &id, true)?;
                }
                AdaptersCmd::Remove { id } => {
                    let h = find(&id)?;
                    println!("{}", h.remove(&ctx)?);
                    set_enabled(&home, &id, false)?;
                }
                AdaptersCmd::Regenerate => {
                    for id in &config.adapters.enabled {
                        println!("{}", find(id)?.install(&ctx)?);
                    }
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Doctor { post_update } => {
            let mut p = ui::prompter(cli.yes);
            let ok = doctor::run(p.as_mut(), &home, &config, post_update)?;
            Ok(if ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
        Command::Config(cmd) => {
            match cmd {
                ConfigCmd::Show => print!("{}", toml::to_string_pretty(&config?)?),
                ConfigCmd::Path => {
                    println!("global: {}", paths::global_config(&home).display());
                    let repo = std::env::current_dir()
                        .ok()
                        .and_then(|d| paths::find_repo_config(&d));
                    match repo {
                        Some(r) => println!("repo:   {}", r.display()),
                        None => println!("repo:   (none; create {})", paths::REPO_CONFIG_FILE),
                    }
                }
                ConfigCmd::Schema => {
                    let schema = schemars::schema_for!(Config);
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&schema).context("schema")?
                    );
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Migration(cmd) => {
            migration::run(&cmd, &home)?;
            Ok(ExitCode::SUCCESS)
        }
        Command::Update { channel, rollback } => {
            let config = config?;
            let channel = channel.map(|c| match c.as_str() {
                "alpha" => ehai_core::config::Channel::Alpha,
                "beta" => ehai_core::config::Channel::Beta,
                _ => ehai_core::config::Channel::Stable,
            });
            let ok = update::run(&home, &config, channel, rollback)?;
            Ok(if ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            })
        }
    }
}

fn find_adapter(id: &str) -> Result<Box<dyn ehai_adapters::Harness>> {
    ehai_adapters::by_id(id).with_context(|| {
        let ids: Vec<_> = ehai_adapters::all().iter().map(|h| h.id()).collect();
        format!("unknown harness `{id}`; available: {}", ids.join(", "))
    })
}

/// Inputs for rendering harness adapters from the effective config.
pub fn adapter_ctx(home: &std::path::Path, config: &Config) -> ehai_adapters::Ctx {
    ehai_adapters::Ctx {
        brand: config.brand.clone(),
        role: config.role.map(role_str),
        home: home.to_path_buf(),
        // Prefer the installed binary so harnesses keep working after `cargo clean`
        // or when ehai was run from a download folder.
        exe: Some(selfcmd::installed_exe(home))
            .filter(|p| p.is_file())
            .or_else(|| std::env::current_exe().and_then(|p| p.canonicalize()).ok())
            .unwrap_or_else(|| PathBuf::from("ehai")),
    }
}

fn role_str(r: ehai_core::config::Role) -> String {
    use ehai_core::config::Role;
    match r {
        Role::Pm => "pm",
        Role::Dev => "dev",
        Role::Both => "both",
    }
    .into()
}

fn set_enabled(home: &std::path::Path, id: &str, on: bool) -> Result<()> {
    ehai_core::config::edit_file(&paths::global_config(home), |t| {
        let current: Vec<String> = t
            .get("adapters")
            .and_then(|a| a.get("enabled"))
            .and_then(|e| e.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let mut next: Vec<String> = current.into_iter().filter(|e| e != id).collect();
        if on {
            next.push(id.into());
        }
        ehai_core::config::set(
            t,
            "adapters",
            "enabled",
            toml::Value::Array(next.into_iter().map(Into::into).collect()),
        );
    })?;
    Ok(())
}

/// Same as [`cancellable`] for errors that went through `anyhow`.
fn exit_if_cancelled(e: &anyhow::Error) {
    if e.downcast_ref::<std::io::Error>()
        .is_some_and(|io| io.kind() == std::io::ErrorKind::Interrupted)
    {
        eprintln!("Cancelled. Nothing was changed.");
        std::process::exit(130);
    }
}

/// Esc/Ctrl-C in a prompt: say so plainly and exit 130 instead of printing an error.
fn cancellable<T>(r: std::io::Result<T>) -> Result<T> {
    match r {
        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {
            eprintln!("Cancelled. Nothing was changed.");
            std::process::exit(130);
        }
        other => Ok(other?),
    }
}
