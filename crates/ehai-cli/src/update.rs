//! `ehai update [--channel] [--rollback]`: check the configured channel,
//! download and verify the right asset, swap it in, then run
//! `doctor --post-update` and roll back automatically if that fails.

use std::path::Path;

use anyhow::{Context, Result};
use ehai_core::config::Channel;
use ehai_core::Config;

use crate::tracker::block_on;

/// Background daily check (spec §11): notify-only, silent on any failure
/// (offline, rate-limited, timed out) and bounded to a couple of seconds so
/// it never meaningfully delays an ordinary command.
pub fn maybe_notify(home: &Path, channel: Channel) {
    if !ehai_update::due_for_check(home) {
        return;
    }
    let _ = ehai_update::record_checked(home);
    let Ok(client) = ehai_update::Client::from_env_quick(
        &ehai_core::release::releases_api(),
        std::time::Duration::from_secs(3),
    ) else {
        return;
    };
    let Ok(result) = block_on(ehai_update::check(&client, channel, ehai_core::VERSION)) else {
        return;
    };
    if let ehai_update::CheckResult::Available(release) = result {
        eprintln!(
            "\nA new version is available: {} -> {} ({channel:?} channel). Run `ehai update` to install it.",
            ehai_core::VERSION,
            release.tag_name
        );
    }
}

pub fn run(home: &Path, config: &Config, channel: Option<Channel>, rollback: bool) -> Result<bool> {
    let exe = crate::selfcmd::installed_exe(home);
    if rollback {
        ehai_update::rollback(&exe).context("rollback")?;
        println!("Rolled back to the previous binary.");
        return Ok(true);
    }

    let channel = channel.unwrap_or(config.update.channel);
    let client = ehai_update::Client::from_env(&ehai_core::release::releases_api())?;
    let result = block_on(ehai_update::check(&client, channel, ehai_core::VERSION))?;
    let release = match result {
        ehai_update::CheckResult::UpToDate => {
            println!("Already up to date ({}).", ehai_core::VERSION);
            return Ok(true);
        }
        ehai_update::CheckResult::Available(r) => r,
    };
    println!(
        "Updating {} -> {} ({channel:?} channel)...",
        ehai_core::VERSION,
        release.tag_name
    );
    block_on(ehai_update::apply(
        &client,
        &release,
        &exe,
        ehai_core::release::MINISIGN_PUBLIC_KEY,
    ))
    .context("download, verify or swap failed — nothing was changed")?;

    // Run the *new* binary's own doctor as a fresh process — the swap only
    // changed the file on disk, this process is still running the old code.
    // Bounded to 30s: a hung doctor (network deadlock, panic-recovery loop)
    // must not block the update forever.
    println!("Verifying...");
    let mut child = std::process::Command::new(&exe)
        .args(["doctor", "--post-update"])
        .spawn()
        .context("running the updated binary's doctor")?;
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(child.wait());
    });
    let status = match rx.recv_timeout(std::time::Duration::from_secs(30)) {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return Err(e).context("running the updated binary's doctor"),
        Err(_) => {
            ehai_update::rollback(&exe).context("doctor timed out, rollback also failed")?;
            anyhow::bail!(
                "update verification timed out after 30s — rolled back to {}",
                ehai_core::VERSION
            );
        }
    };
    if status.success() {
        println!("Updated to {}.", release.tag_name);
        Ok(true)
    } else {
        ehai_update::rollback(&exe)
            .context("update verification failed, and rollback also failed")?;
        println!(
            "Update verification failed — rolled back to {}.",
            ehai_core::VERSION
        );
        Ok(false)
    }
}
