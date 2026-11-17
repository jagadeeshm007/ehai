//! `ehai migration export` / `ehai migration import` — single-file ZIP migration.
//!
//! Export bundles `config.toml`, `prefs/user.toml`, and `tracker.json` (the local
//! tracker, if present) into a single ZIP. Import restores them on a new machine.
//! Harness adapter files are NOT included — run `ehai adapters regenerate` after
//! importing to rewire everything on the new machine.

use std::io::{Cursor, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Subcommand;
use serde_json::{json, Value};
use zip::write::SimpleFileOptions;
use zip::ZipWriter;

use ehai_core::VERSION;

#[derive(Subcommand)]
pub enum MigrationCmd {
    /// Bundle config, prefs and local tracker into a single ZIP for device migration.
    Export {
        /// Output path (default: `ehai-backup-YYYYMMDD.zip` in the current directory).
        #[arg(value_name = "FILE")]
        output: Option<PathBuf>,
    },
    /// Restore config, prefs and local tracker from an export ZIP.
    Import {
        #[arg(value_name = "FILE")]
        input: PathBuf,
        /// Overwrite existing config without asking.
        #[arg(long)]
        yes: bool,
    },
}

pub fn run(cmd: &MigrationCmd, home: &Path) -> Result<()> {
    match cmd {
        MigrationCmd::Export { output } => do_export(home, output.as_deref()),
        MigrationCmd::Import { input, yes } => do_import(home, input, *yes),
    }
}

// ── export ───────────────────────────────────────────────────────────────────

fn do_export(home: &Path, output: Option<&Path>) -> Result<()> {
    let now = ehai_core::date::today_utc();
    let default_name = format!("ehai-backup-{}.zip", now.replace('-', ""));
    let dest = output
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(&default_name));

    let mut buf = Vec::new();
    {
        let mut zip = ZipWriter::new(Cursor::new(&mut buf));
        let opts = SimpleFileOptions::default();

        // metadata.json
        let meta = json!({
            "schema_version": 1,
            "ehai_version": VERSION,
            "exported_at": now,
        });
        zip.start_file("metadata.json", opts)
            .context("zip: metadata.json")?;
        zip.write_all(serde_json::to_string_pretty(&meta).unwrap().as_bytes())
            .context("zip: write metadata")?;

        // config.toml
        let config_path = ehai_core::paths::global_config(home);
        if let Ok(text) = std::fs::read_to_string(&config_path) {
            zip.start_file("config.toml", opts)
                .context("zip: config.toml")?;
            zip.write_all(text.as_bytes())
                .context("zip: write config")?;
        }

        // prefs/user.toml
        let prefs_path = home.join("prefs/user.toml");
        if let Ok(text) = std::fs::read_to_string(&prefs_path) {
            zip.start_file("prefs/user.toml", opts)
                .context("zip: prefs/user.toml")?;
            zip.write_all(text.as_bytes()).context("zip: write prefs")?;
        }

        // tracker.json (local tracker, if present)
        let tracker_path = std::env::var_os("EHAI_LOCAL_TRACKER")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("tracker.json"));
        if let Ok(text) = std::fs::read_to_string(&tracker_path) {
            zip.start_file("tracker.json", opts)
                .context("zip: tracker.json")?;
            zip.write_all(text.as_bytes())
                .context("zip: write tracker")?;
        }

        zip.finish().context("zip: finish")?;
    }

    std::fs::write(&dest, &buf).with_context(|| format!("write {}", dest.display()))?;
    println!(
        "Exported to {} — copy this file to your new machine and run `ehai migration import <file>`",
        dest.display()
    );
    Ok(())
}

// ── import ───────────────────────────────────────────────────────────────────

fn do_import(home: &Path, input: &Path, yes: bool) -> Result<()> {
    let data = std::fs::read(input).with_context(|| format!("read {}", input.display()))?;
    let mut zip = zip::ZipArchive::new(Cursor::new(data)).context("not a valid ZIP file")?;

    // validate metadata
    let meta: Value = {
        let mut f = zip
            .by_name("metadata.json")
            .context("missing metadata.json — was this exported by ehai?")?;
        let mut s = String::new();
        f.read_to_string(&mut s).context("read metadata")?;
        serde_json::from_str(&s).context("parse metadata")?
    };
    let schema = meta
        .get("schema_version")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    if schema != 1 {
        anyhow::bail!("unknown schema_version {schema}; this export was made by a newer ehai");
    }
    let exported_at = meta
        .get("exported_at")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let from_version = meta
        .get("ehai_version")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    println!("Importing export from {exported_at} (ehai {from_version})");

    // config.toml
    if let Ok(mut f) = zip.by_name("config.toml") {
        let mut text = String::new();
        f.read_to_string(&mut text).context("read config")?;
        let dest = ehai_core::paths::global_config(home);
        if dest.exists() && !yes {
            eprintln!(
                "  {} already exists. Pass --yes to overwrite.",
                dest.display()
            );
        } else {
            if let Some(dir) = dest.parent() {
                std::fs::create_dir_all(dir)?;
            }
            ehai_core::migrate::write_atomic(&dest, text.as_bytes())?;
            println!("  ✔ config.toml");
        }
    }

    // prefs/user.toml
    if let Ok(mut f) = zip.by_name("prefs/user.toml") {
        let mut text = String::new();
        f.read_to_string(&mut text).context("read prefs")?;
        let dest = home.join("prefs/user.toml");
        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir)?;
        }
        ehai_core::migrate::write_atomic(&dest, text.as_bytes())?;
        println!("  ✔ prefs/user.toml");
    }

    // tracker.json
    if let Ok(mut f) = zip.by_name("tracker.json") {
        let mut text = String::new();
        f.read_to_string(&mut text).context("read tracker")?;
        let dest = home.join("tracker.json");
        // back up existing tracker before overwriting
        if dest.exists() {
            let bak = dest.with_extension("json.bak");
            let _ = std::fs::copy(&dest, &bak);
        }
        ehai_core::migrate::write_atomic(&dest, text.as_bytes())?;
        println!("  ✔ tracker.json");
    }

    println!("\nDone. Run `ehai adapters regenerate` to rewire harnesses on this machine.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn export_creates_valid_zip_with_metadata() {
        let home = tempdir().unwrap();
        // write a minimal config so the export finds something
        let config_path = ehai_core::paths::global_config(home.path());
        std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        std::fs::write(&config_path, "[brand]\nname = \"ehai\"\n").unwrap();

        let out_path = home.path().join("backup.zip");
        do_export(home.path(), Some(&out_path)).unwrap();

        // validate ZIP
        let data = std::fs::read(&out_path).unwrap();
        let mut zip = zip::ZipArchive::new(Cursor::new(data)).unwrap();
        assert!(
            zip.by_name("metadata.json").is_ok(),
            "metadata.json present"
        );
        assert!(zip.by_name("config.toml").is_ok(), "config.toml present");
    }

    #[test]
    fn import_restores_config_and_prefs() {
        let src = tempdir().unwrap();
        // set up source home
        let config_path = ehai_core::paths::global_config(src.path());
        std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        std::fs::write(&config_path, "[brand]\nname = \"ehai\"\n").unwrap();
        let prefs_path = src.path().join("prefs/user.toml");
        std::fs::create_dir_all(prefs_path.parent().unwrap()).unwrap();
        std::fs::write(&prefs_path, "[[entries]]\nkey = \"detail\"\nvalue = \"short\"\nscope = \"global\"\nsource = \"explicit\"\n").unwrap();

        let zip_path = src.path().join("backup.zip");
        do_export(src.path(), Some(&zip_path)).unwrap();

        // import into a fresh home
        let dst = tempdir().unwrap();
        do_import(dst.path(), &zip_path, true).unwrap();

        let restored_config = ehai_core::paths::global_config(dst.path());
        assert!(restored_config.exists(), "config restored");
        let restored_prefs = dst.path().join("prefs/user.toml");
        assert!(restored_prefs.exists(), "prefs restored");
        let prefs_text = std::fs::read_to_string(&restored_prefs).unwrap();
        assert!(prefs_text.contains("detail"), "prefs content intact");
    }
}
