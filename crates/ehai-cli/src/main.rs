use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use ehai_core::{paths, Config, LoadOptions};

mod doctor;
mod init;
mod logging;
mod selfcmd;
mod sys;
mod tracker;
mod ui;

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
    /// Run the MCP server on stdio (started by your AI harness, not by hand).
    Mcp,
    /// Inspect configuration.
    #[command(subcommand)]
    Config(ConfigCmd),
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
    /// Create any missing project fields (Status, Priority, Size, Type, Iteration).
    Schema,
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
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e:#}");
            ExitCode::FAILURE
        }
    }
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

    match cli.command {
        Command::Mcp => {
            let config = config?;
            let ctx = ehai_mcp::Context {
                brand: config.brand.name.clone(),
                role: config.role.map(role_str),
                home: home.clone(),
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
                    println!("global: {}", global.display());
                    let repo = opts.cwd.as_deref().and_then(paths::find_repo_config);
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
