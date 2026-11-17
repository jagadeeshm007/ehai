use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use ehai_core::{paths, Config, LoadOptions};

mod doctor;
mod logging;
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
    /// Inspect configuration.
    #[command(subcommand)]
    Config(ConfigCmd),
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
    let _log_guard = logging::init(&paths::logs_dir(&home), cli.verbose);
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
