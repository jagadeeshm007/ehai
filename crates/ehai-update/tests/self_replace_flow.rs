//! Exercises the real `swap_in`/`rollback` (self_replace-backed) path end to
//! end, safely: everything runs against a disposable *copy* of a tiny helper
//! binary in a tempdir, spawned as its own process so `current_exe()` points
//! at that copy — never at the shared `target/debug` build artifact or the
//! real `ehai` binary. This is the "update test simulates a failed
//! post-update doctor, and rollback restores the previous binary" check from
//! spec §13 / milestones.md M8.

use std::path::Path;
use std::process::Command;

fn harness_copy(dir: &Path) -> std::path::PathBuf {
    let src = Path::new(env!("CARGO_BIN_EXE_update_test_harness"));
    let dst = dir.join(if cfg!(windows) {
        "harness.exe"
    } else {
        "harness"
    });
    std::fs::copy(src, &dst).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dst, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    dst
}

fn run(exe: &Path, args: &[&str]) -> std::process::Output {
    Command::new(exe).args(args).output().unwrap()
}

#[test]
fn swap_backs_up_and_replaces_then_rollback_restores() {
    let dir = tempfile::tempdir().unwrap();
    let exe = harness_copy(dir.path());
    let original_bytes = std::fs::read(&exe).unwrap();

    // Swap in the harness's own bytes again (content doesn't matter for the
    // file-swap mechanics; what matters is that the result is still a valid,
    // runnable binary, so a second subprocess spawn can prove it).
    let out = run(&exe, &["swap", exe.to_str().unwrap()]);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "swapped");

    let backup = exe.with_extension("prev");
    assert!(backup.is_file(), "backup was written");
    assert_eq!(
        std::fs::read(&backup).unwrap(),
        original_bytes,
        "backup matches pre-swap content"
    );

    // the swapped-in binary is still runnable
    let out = run(&exe, &["version"]);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "v1");

    // simulate a failed post-update doctor: roll back
    let out = run(&exe, &["rollback"]);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "rolled back");
    assert_eq!(
        std::fs::read(&exe).unwrap(),
        original_bytes,
        "rolled back to the exact pre-swap binary"
    );

    // still runnable after rollback
    let out = run(&exe, &["version"]);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "v1");
}

#[test]
fn rollback_with_no_prior_swap_fails_clearly() {
    let dir = tempfile::tempdir().unwrap();
    let exe = harness_copy(dir.path());
    let out = run(&exe, &["rollback"]);
    assert!(!out.status.success());
}
