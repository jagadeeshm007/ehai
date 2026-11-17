//! Self-update: check the right channel, download the right asset, verify its
//! checksum and minisign signature, then swap the running binary in place with
//! automatic rollback if the post-update `doctor` fails (spec §11).
//!
//! [`stage`] (network + verify + extract) is fully unit/integration tested.
//! [`swap_in`] and [`rollback`] are not: they call `self_replace`, which
//! operates on `std::env::current_exe()` — the actual test binary — so a test
//! that exercised them would replace itself mid-run. They're deliberately thin
//! wrappers around a well-tested crate, verified manually instead (see
//! DECISIONS.md).

mod archive;
mod error;
mod github;
mod target;
mod verify;

pub use error::{Error, Result};
pub use github::{Asset, Client, Release};
pub use target::current_target;

use std::path::Path;

use ehai_core::config::Channel;

/// What [`check`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckResult {
    UpToDate,
    Available(Release),
}

/// List releases and pick the newest one the channel accepts that's newer
/// than `current_version`. Alpha accepts alpha/beta/stable tags; beta accepts
/// beta/stable; stable accepts only stable (spec §11A).
pub async fn check(
    client: &Client,
    channel: Channel,
    current_version: &str,
) -> Result<CheckResult> {
    let releases = client.releases().await?;
    match github::latest_for_channel(&releases, channel, current_version) {
        Some(r) => Ok(CheckResult::Available(r)),
        None => Ok(CheckResult::UpToDate),
    }
}

/// Download this platform's asset for `release`, verify its signature against
/// `public_key` (a minisign base64 public key — callers pass
/// [`ehai_core::release::MINISIGN_PUBLIC_KEY`]) and its checksum, and return
/// the extracted binary bytes. Writes nothing and never touches the running
/// executable — safe to call from a test with a throwaway test key.
pub async fn stage(client: &Client, release: &Release, public_key: &str) -> Result<Vec<u8>> {
    let target = target::current_target();
    let asset = release
        .assets
        .iter()
        .find(|a| a.name.contains(target))
        .ok_or_else(|| Error::NoAsset {
            target: target.into(),
        })?;
    let sums_asset = release
        .assets
        .iter()
        .find(|a| a.name == "SHA256SUMS")
        .ok_or_else(|| Error::NoAsset {
            target: "SHA256SUMS".into(),
        })?;
    let sig_asset = release
        .assets
        .iter()
        .find(|a| a.name == "SHA256SUMS.minisig")
        .ok_or_else(|| Error::NoAsset {
            target: "SHA256SUMS.minisig".into(),
        })?;

    let archive = client.download(&asset.url).await?;
    let sums = client.download(&sums_asset.url).await?;
    let sig = client.download(&sig_asset.url).await?;

    verify::verify_signature(&sums, &sig, public_key)?;
    verify::verify_checksum(&sums, &asset.name, &archive)?;

    archive::extract_binary(&archive, &asset.name)
}

/// Back up the running binary to `<exe>.prev`, then swap `binary` into its
/// place. Not unit-tested — see the module doc.
pub fn swap_in(binary: &[u8], exe_path: &Path) -> Result<()> {
    let backup = exe_path.with_extension("prev");
    std::fs::copy(exe_path, &backup).map_err(Error::io(&backup))?;

    let staged = exe_path.with_extension("new");
    std::fs::write(&staged, binary).map_err(Error::io(&staged))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&staged, std::fs::Permissions::from_mode(0o755))
            .map_err(Error::io(&staged))?;
    }

    self_replace::self_replace(&staged).map_err(Error::io(&staged))?;
    let _ = std::fs::remove_file(&staged);
    Ok(())
}

/// Fetch, verify and apply `release` over `exe_path` in one step.
pub async fn apply(
    client: &Client,
    release: &Release,
    exe_path: &Path,
    public_key: &str,
) -> Result<()> {
    let binary = stage(client, release, public_key).await?;
    swap_in(&binary, exe_path)
}

/// Restore `<exe>.prev` (written by [`swap_in`]) back over `exe_path`. Not
/// unit-tested — see the module doc.
pub fn rollback(exe_path: &Path) -> Result<()> {
    let backup = exe_path.with_extension("prev");
    if !backup.is_file() {
        return Err(Error::Io {
            path: backup,
            source: std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "no previous binary to roll back to",
            ),
        });
    }
    self_replace::self_replace(&backup).map_err(Error::io(exe_path))
}

const DAY: u64 = 24 * 60 * 60;

/// `true` if the background check (spec §11, `update.check`) hasn't run in
/// the last day, or has never run.
pub fn due_for_check(home: &Path) -> bool {
    let path = ehai_core::paths::update_check_cache(home);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return true;
    };
    let Some(last) = text.trim().parse::<u64>().ok() else {
        return true;
    };
    now().saturating_sub(last) > DAY
}

/// Record that the background check ran just now, regardless of outcome —
/// a failed (e.g. offline) check shouldn't retry every single command.
pub fn record_checked(home: &Path) -> Result<()> {
    let path = ehai_core::paths::update_check_cache(home);
    ehai_core::migrate::write_atomic(&path, now().to_string().as_bytes())?;
    Ok(())
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rollback_without_a_backup_is_a_clear_error() {
        let dir = tempfile::tempdir().unwrap();
        let exe = dir.path().join("ehai");
        std::fs::write(&exe, b"current").unwrap();
        let err = rollback(&exe);
        assert!(matches!(err, Err(Error::Io { .. })), "{err:?}");
    }

    #[test]
    fn due_for_check_is_true_until_recorded_then_false() {
        let home = tempfile::tempdir().unwrap();
        assert!(due_for_check(home.path()), "never checked yet");
        record_checked(home.path()).unwrap();
        assert!(!due_for_check(home.path()), "just checked");
    }

    #[test]
    fn due_for_check_is_true_again_after_a_day() {
        let home = tempfile::tempdir().unwrap();
        let path = ehai_core::paths::update_check_cache(home.path());
        let stale = now() - DAY - 1;
        std::fs::write(&path, stale.to_string()).unwrap();
        assert!(due_for_check(home.path()));
    }
}
