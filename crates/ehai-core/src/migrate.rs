//! Config schema migrations. Forward-only, pure `Table -> Table` steps.
//! Files are only rewritten by [`migrate_file`], which backs up first, writes
//! atomically and holds a lock so parallel harness sessions can't race.

use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use toml::{Table, Value};

use crate::config::read_table;
use crate::{Error, Result};

pub const CURRENT_SCHEMA_VERSION: u32 = 1;

type Step = fn(Table) -> Table;

/// `STEPS[i]` migrates schema `i + 1` to `i + 2`. Empty at v1.
const STEPS: &[Step] = &[];

const _: () = assert!(STEPS.len() as u32 + 1 == CURRENT_SCHEMA_VERSION);

/// Missing `schema_version` means v1. Configs from a *newer* ehai are returned
/// untouched: unknown keys are ignored on load, so an older binary keeps running.
pub fn schema_version(t: &Table) -> u32 {
    t.get("schema_version")
        .and_then(Value::as_integer)
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(1)
}

pub fn migrate(mut t: Table) -> Table {
    let from = schema_version(&t);
    if from >= CURRENT_SCHEMA_VERSION {
        return t;
    }
    for step in &STEPS[(from.max(1) - 1) as usize..] {
        t = step(t);
    }
    t.insert(
        "schema_version".into(),
        Value::Integer(CURRENT_SCHEMA_VERSION.into()),
    );
    t
}

/// Migrate the file at `path` in place if it is behind. Returns the backup path when
/// a rewrite happened.
pub fn migrate_file(path: &Path) -> Result<Option<PathBuf>> {
    if !path.is_file() {
        return Ok(None);
    }
    let _lock = lock(path)?;
    let table = read_table(path)?;
    let from = schema_version(&table);
    if from >= CURRENT_SCHEMA_VERSION {
        return Ok(None);
    }
    let backup = path.with_extension(format!("toml.bak-v{from}"));
    std::fs::copy(path, &backup).map_err(Error::io(&backup))?;
    let body =
        toml::to_string_pretty(&migrate(table)).map_err(|e| Error::Invalid(e.to_string()))?;
    write_atomic(path, body.as_bytes())?;
    Ok(Some(backup))
}

/// Exclusive advisory lock on `<path>.lock`, released on drop.
pub fn lock(path: &Path) -> Result<File> {
    let lock_path = path.with_extension("lock");
    let f = File::create(&lock_path).map_err(Error::io(&lock_path))?;
    f.lock().map_err(Error::io(&lock_path))?;
    Ok(f)
}

/// Write via a temp file in the same directory + rename, so readers never see a torn file.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("tmp");
    let mut f = File::create(&tmp).map_err(Error::io(&tmp))?;
    f.write_all(bytes).map_err(Error::io(&tmp))?;
    f.sync_all().map_err(Error::io(&tmp))?;
    std::fs::rename(&tmp, path).map_err(Error::io(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Config, LoadOptions};
    use proptest::prelude::*;

    fn arb_v1_config() -> impl Strategy<Value = Config> {
        (
            "[a-z][a-z0-9-]{1,31}",
            prop::option::of(prop_oneof!["pm", "dev", "both"]),
            prop_oneof!["off", "warn", "strict"],
            prop_oneof!["off", "restricted", "container"],
            any::<bool>(),
            prop_oneof!["alpha", "beta", "stable"],
            any::<bool>(),
            prop_oneof!["github_projects", "linear", "jira"],
        )
            .prop_map(
                |(name, role, comp, sandbox, learning, channel, auto, tracker)| {
                    let mut t = Table::new();
                    t.insert("brand".into(), toml::toml! { name = name }.into());
                    if let Some(r) = role {
                        t.insert("role".into(), Value::String(r));
                    }
                    t.insert("compliance".into(), toml::toml! { mode = comp }.into());
                    t.insert("sandbox".into(), toml::toml! { mode = sandbox }.into());
                    t.insert("prefs".into(), toml::toml! { learning = learning }.into());
                    t.insert(
                        "update".into(),
                        toml::toml! { channel = channel auto = auto }.into(),
                    );
                    t.insert("tracker".into(), toml::toml! { kind = tracker }.into());
                    Value::Table(t).try_into().unwrap()
                },
            )
    }

    proptest! {
        /// Any v1 config (with or without an explicit version) migrates to a valid
        /// latest config that keeps every user value.
        #[test]
        fn migration_preserves_user_values(cfg in arb_v1_config(), explicit_version in any::<bool>()) {
            let mut t = Table::try_from(&cfg).unwrap();
            if !explicit_version {
                t.remove("schema_version");
            }
            let out: Config = Value::Table(migrate(t)).try_into().unwrap();
            prop_assert_eq!(out, cfg);
        }
    }

    #[test]
    fn future_versions_are_left_alone() {
        let t: Table = "schema_version = 99\nnew_thing = 1".parse().unwrap();
        assert_eq!(migrate(t.clone()), t);
    }

    #[test]
    fn migrate_file_is_noop_when_current() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.toml");
        std::fs::write(&p, "role = \"pm\"\n").unwrap();
        assert_eq!(migrate_file(&p).unwrap(), None);
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "role = \"pm\"\n");
        assert_eq!(migrate_file(&d.path().join("missing.toml")).unwrap(), None);
    }

    #[test]
    fn write_atomic_replaces_content() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.toml");
        write_atomic(&p, b"a = 1").unwrap();
        write_atomic(&p, b"a = 2").unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "a = 2");
        assert!(!p.with_extension("tmp").exists());
        let c = Config::load(&LoadOptions {
            global: Some(p),
            ..Default::default()
        });
        assert!(c.is_ok());
    }
}
