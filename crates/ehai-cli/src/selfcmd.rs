//! `ehai self install` (put the binary on PATH) and `ehai uninstall` (remove every
//! trace). The M8 one-line installer downloads a release and runs `self install`.

use std::path::{Path, PathBuf};

use ehai_core::{blocks, paths, Config};

/// Where the installed binary lives: `~/.ehai/bin/ehai[.exe]`.
pub fn installed_exe(home: &Path) -> PathBuf {
    paths::bin_dir(home).join(format!("ehai{}", std::env::consts::EXE_SUFFIX))
}

/// Shell startup files that may carry our PATH block.
fn rc_candidates(user_home: &Path) -> Vec<PathBuf> {
    [
        ".bashrc",
        ".zshrc",
        ".profile",
        ".config/fish/conf.d/ehai.fish",
    ]
    .iter()
    .map(|f| user_home.join(f))
    .collect()
}

/// The rc file for the user's login shell, and the PATH line for it.
fn rc_for_shell(user_home: &Path, bin: &Path) -> (PathBuf, String) {
    let shell = std::env::var("SHELL").unwrap_or_default();
    let bin = bin.display();
    if shell.ends_with("fish") {
        (
            user_home.join(".config/fish/conf.d/ehai.fish"),
            format!("fish_add_path -g \"{bin}\""),
        )
    } else {
        let file = if shell.ends_with("zsh") {
            ".zshrc"
        } else if shell.ends_with("bash") {
            ".bashrc"
        } else {
            ".profile"
        };
        (user_home.join(file), format!("export PATH=\"{bin}:$PATH\""))
    }
}

/// Dirs that are often already on PATH and writable without sudo. A link in one
/// of them makes the command work in the *current* terminal, which an rc-file
/// edit can't do (only new shells read it). `EHAI_LINK_DIRS` overrides the list
/// (the CLI tests point it at a sandbox); unit tests never touch system dirs.
fn link_dirs(user_home: &Path) -> Vec<PathBuf> {
    if let Some(v) = std::env::var_os("EHAI_LINK_DIRS") {
        return std::env::split_paths(&v).collect();
    }
    let mut dirs = vec![user_home.join(".local/bin"), user_home.join("bin")];
    if !cfg!(test) {
        dirs.push("/opt/homebrew/bin".into());
        dirs.push("/usr/local/bin".into());
    }
    dirs
}

fn on_path(dir: &Path, path_var: &std::ffi::OsStr) -> bool {
    std::env::split_paths(path_var).any(|p| p == dir)
}

/// Link `<dir>/<name>` → `target` in the first of `dirs` that is on `path_var`
/// and writable. Never replaces anything that isn't already our link.
#[cfg(unix)]
fn link_into_path(
    dirs: &[PathBuf],
    target: &Path,
    name: &str,
    path_var: &std::ffi::OsStr,
) -> Option<PathBuf> {
    for dir in dirs.iter().filter(|d| on_path(d, path_var)) {
        let link = dir.join(name);
        match std::fs::read_link(&link) {
            Ok(t) if t == target => return Some(link),
            Ok(_) => continue,                   // someone else's link
            Err(_) if link.exists() => continue, // a real file, not ours
            Err(_) => {}
        }
        if std::os::unix::fs::symlink(target, &link).is_ok() {
            return Some(link);
        }
    }
    None
}

/// `None` when `name` already runs in this terminal; otherwise the one line to
/// paste so it does (new terminals pick it up from the rc file anyway).
pub fn activate_hint(home: &Path, name: &str) -> Option<String> {
    if ehai_env::which(name).is_some() {
        return None;
    }
    let bin = paths::bin_dir(home);
    Some(if cfg!(windows) {
        format!("$env:Path += \";{}\"", bin.display())
    } else {
        let user_home = std::env::home_dir().unwrap_or_default();
        rc_for_shell(&user_home, &bin).1
    })
}

/// Copy the running binary into `~/.ehai/bin`, add it to PATH, add the brand alias.
/// Returns human-readable lines describing what happened.
pub fn install(home: &Path, config: &Config) -> anyhow::Result<Vec<String>> {
    install_for(home, config, std::env::home_dir().as_deref())
}

/// Like [`install`], with the user's shell-profile home (where `.bashrc` etc.
/// live — distinct from `home`, which is `EHAI_HOME`) passed explicitly
/// rather than read from the real process environment. Tests that call
/// `apply()`/`install()` in-process (not through a sandboxed subprocess, the
/// way the CLI integration tests do) must pass a disposable tempdir here —
/// `std::env::home_dir()` would otherwise resolve to the real machine's
/// actual home directory and edit its real `.bashrc`.
pub fn install_for(
    home: &Path,
    config: &Config,
    user_home: Option<&Path>,
) -> anyhow::Result<Vec<String>> {
    let mut done = Vec::new();
    let target = installed_exe(home);
    let bin_dir = paths::bin_dir(home);
    std::fs::create_dir_all(&bin_dir)?;

    let current = std::env::current_exe()?.canonicalize()?;
    if target.canonicalize().ok().as_ref() != Some(&current) {
        // copy beside, then rename: atomic, and fine while the old one is running
        let tmp = bin_dir.join(".ehai.new");
        std::fs::copy(&current, &tmp)?;
        std::fs::rename(&tmp, &target)?;
        done.push(format!("installed {}", target.display()));
    } else {
        done.push(format!(
            "{} is already the running binary",
            target.display()
        ));
    }

    // spec §2.4: install under the brand name too
    let brand = &config.brand.name;
    if brand != "ehai" {
        let alias = bin_dir.join(format!("{brand}{}", std::env::consts::EXE_SUFFIX));
        let _ = std::fs::remove_file(&alias);
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &alias)?;
        #[cfg(not(unix))]
        std::fs::copy(&target, &alias)?;
        done.push(format!("alias `{brand}` → ehai"));
    }

    if cfg!(windows) {
        done.push(format!(
            "add {} to your user PATH (the Windows installer does this automatically)",
            bin_dir.display()
        ));
    } else {
        let user_home = user_home.ok_or(ehai_core::Error::NoHome)?;
        let (rc, line) = rc_for_shell(user_home, &bin_dir);
        if blocks::upsert(&rc, &line)? {
            done.push(format!(
                "added {} to PATH in {}",
                bin_dir.display(),
                rc.display()
            ));
        } else {
            done.push(format!("PATH already set in {}", rc.display()));
        }
        #[cfg(unix)]
        {
            let path_var = std::env::var_os("PATH").unwrap_or_default();
            if !on_path(&bin_dir, &path_var) {
                if let Some(link) = link_into_path(&link_dirs(user_home), &target, brand, &path_var)
                {
                    done.push(format!(
                        "linked {} so `{brand}` works in this terminal now",
                        link.display()
                    ));
                }
            }
        }
    }
    Ok(done)
}

/// Everything ehai creates under its home dir. Uninstall deletes only these, so a
/// mistaken `EHAI_HOME` can never wipe unrelated files.
const OWNED: &[&str] = &[
    "config.toml",
    "config.lock",
    "logs",
    "prefs",
    "adapters",
    "bin",
    "audit",
    "reports",
    "worktrees",
    "cache",
    "ports.json",
    "update-check.json",
    "tracker.json",
    "tracker.json.bak",
    ".doctor-probe",
];

fn owned(name: &str) -> bool {
    OWNED.contains(&name)
        || name.starts_with("config.toml.")
        || name.ends_with(".tmp")
        || name.ends_with(".lock")
}

/// Remove ehai's files from `home` and the PATH blocks from shell rc files.
/// Adapters are unregistered by the caller first (they need the config).
pub fn remove_files(home: &Path, user_home: Option<&Path>) -> anyhow::Result<Vec<String>> {
    anyhow::ensure!(
        home.parent().is_some() && Some(home) != user_home,
        "refusing to delete {}: it is not an ehai directory",
        home.display()
    );
    let mut done = Vec::new();
    if let Some(uh) = user_home {
        // links made by `install_for` (anything pointing into our home)
        for dir in link_dirs(uh) {
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let p = entry.path();
                if std::fs::read_link(&p).is_ok_and(|t| t.starts_with(home)) {
                    std::fs::remove_file(&p)?;
                    done.push(format!("removed link {}", p.display()));
                }
            }
        }
        for rc in rc_candidates(uh) {
            if blocks::remove(&rc)? {
                done.push(format!("removed PATH entry from {}", rc.display()));
                // our own fish snippet: delete once it is empty
                if rc.ends_with("conf.d/ehai.fish")
                    && std::fs::read_to_string(&rc).is_ok_and(|s| s.trim().is_empty())
                {
                    std::fs::remove_file(&rc)?;
                }
            }
        }
    }
    let mut leftovers = Vec::new();
    for entry in std::fs::read_dir(home).into_iter().flatten().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !owned(&name) {
            leftovers.push(name);
            continue;
        }
        let p = entry.path();
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            std::fs::remove_dir_all(&p)?;
        } else {
            std::fs::remove_file(&p)?;
        }
    }
    if leftovers.is_empty() {
        if home.exists() {
            std::fs::remove_dir(home)?;
        }
        done.push(format!("deleted {}", home.display()));
    } else {
        done.push(format!(
            "kept {} because it also holds files ehai didn't create: {}",
            home.display(),
            leftovers.join(", ")
        ));
    }
    Ok(done)
}

/// Unregister every adapter, whether or not it is in `adapters.enabled`.
pub fn remove_adapters(home: &Path, config: &Config) -> Vec<String> {
    let ctx = crate::adapter_ctx(home, config);
    ehai_adapters::all()
        .into_iter()
        .map(|h| match h.remove(&ctx) {
            Ok(msg) => format!("{}: {msg}", h.display_name()),
            Err(e) => format!("{}: could not unregister ({e})", h.display_name()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remove_files_only_touches_owned_entries() {
        let d = tempfile::tempdir().unwrap();
        let home = d.path().join(".ehai");
        std::fs::create_dir_all(home.join("logs")).unwrap();
        std::fs::create_dir_all(home.join("bin")).unwrap();
        std::fs::write(home.join("config.toml"), "").unwrap();
        std::fs::write(home.join("config.toml.bak-v1"), "").unwrap();

        std::fs::write(home.join("my-notes.txt"), "keep me").unwrap();
        let user = d.path().join("user");
        let out = remove_files(&home, Some(&user)).unwrap().join("\n");
        assert!(out.contains("my-notes.txt"), "{out}");
        assert!(home.join("my-notes.txt").exists());
        assert!(!home.join("logs").exists() && !home.join("config.toml").exists());

        std::fs::remove_file(home.join("my-notes.txt")).unwrap();
        remove_files(&home, Some(&user)).unwrap();
        assert!(!home.exists());
        assert!(
            remove_files(&user, Some(&user)).is_err(),
            "never deletes the user's home"
        );
    }

    #[cfg(unix)]
    #[test]
    fn links_into_first_writable_dir_on_path_and_never_clobbers() {
        let d = tempfile::tempdir().unwrap();
        let target = d.path().join(".ehai/bin/ehai");
        let (off, taken, free) = (
            d.path().join("off"),
            d.path().join("taken"),
            d.path().join("free"),
        );
        for dir in [&off, &taken, &free] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::write(taken.join("ehai"), "someone else's ehai").unwrap();
        let dirs = vec![off.clone(), taken.clone(), free.clone()];
        // `off` isn't on PATH, `taken` has a real file: only `free` qualifies
        let path_var = std::env::join_paths([&taken, &free]).unwrap();

        let link = link_into_path(&dirs, &target, "ehai", &path_var).unwrap();
        assert_eq!(link, free.join("ehai"));
        assert_eq!(std::fs::read_link(&link).unwrap(), target);
        assert_eq!(
            std::fs::read_to_string(taken.join("ehai")).unwrap(),
            "someone else's ehai"
        );
        assert!(!off.join("ehai").exists());
        // idempotent
        assert_eq!(
            link_into_path(&dirs, &target, "ehai", &path_var),
            Some(link)
        );
        // nothing on PATH → no link
        assert_eq!(
            link_into_path(&dirs, &target, "ehai", std::ffi::OsStr::new("")),
            None
        );
    }

    #[test]
    fn rc_choice_follows_shell() {
        let h = Path::new("/u");
        let bin = Path::new("/u/.ehai/bin");
        // SHELL is read from the environment; just check the mapping helper output shape
        let (rc, line) = rc_for_shell(h, bin);
        assert!(rc.starts_with(h));
        assert!(line.contains("/u/.ehai/bin"));
    }
}
