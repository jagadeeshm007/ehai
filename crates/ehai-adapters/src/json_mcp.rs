//! Shared JSON config editing for harnesses that register the MCP server
//! under `{section}.{brand}` in a JSON config file (Gemini, Cursor, VS Code,
//! Windsurf, OpenCode all follow this pattern).

use std::path::Path;

use serde_json::{Map, Value};

use crate::{Error, Result, Status};

/// Read a JSON object from `path` (empty object when file doesn't exist).
pub fn read(path: &Path) -> Result<Map<String, Value>> {
    match std::fs::read_to_string(path) {
        Ok(text) if text.trim().is_empty() => Ok(Map::new()),
        Ok(text) => match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(m)) => Ok(m),
            _ => Err(Error::Command {
                cmd: format!("read {}", path.display()),
                stderr: format!(
                    "{} is not a JSON object; fix or move it, then retry",
                    path.display()
                ),
            }),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Map::new()),
        Err(e) => Err(ehai_core::Error::io(path)(e).into()),
    }
}

/// Write `map` to `path`, taking a `.ehai-backup` on first write.
pub fn write(path: &Path, map: &Map<String, Value>) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(ehai_core::Error::io(dir))?;
    }
    let backup = path.with_extension(format!(
        "{}.ehai-backup",
        path.extension().and_then(|e| e.to_str()).unwrap_or("json")
    ));
    if path.is_file() && !backup.exists() {
        std::fs::copy(path, &backup).map_err(ehai_core::Error::io(&backup))?;
    }
    let body = serde_json::to_string_pretty(map).expect("json serializes") + "\n";
    ehai_core::migrate::write_atomic(path, body.as_bytes())?;
    Ok(())
}

/// Add/update `{section}.{brand}` = `entry` in the config at `path`.
/// `lock_name` is used to name the advisory lock file inside `~/.ehai/adapters/`.
pub fn install(
    path: &Path,
    section: &str,
    brand: &str,
    entry: Value,
    home: &Path,
    lock_name: &str,
) -> Result<String> {
    let _lock = lock(home, lock_name)?;
    let mut map = read(path)?;
    let servers = map
        .entry(section)
        .or_insert_with(|| Value::Object(Map::new()));
    let Value::Object(servers) = servers else {
        return Err(Error::Command {
            cmd: format!("edit {}", path.display()),
            stderr: format!("`{section}` is not an object"),
        });
    };
    if servers.get(brand) == Some(&entry) {
        return Ok(format!("`{brand}` already registered"));
    }
    servers.insert(brand.to_string(), entry);
    write(path, &map)?;
    Ok(format!("registered `{brand}` in {}", path.display()))
}

/// Remove `{section}.{brand}` from the config at `path`.
pub fn remove(
    path: &Path,
    section: &str,
    brand: &str,
    home: &Path,
    lock_name: &str,
) -> Result<String> {
    if !path.exists() {
        return Ok(format!("`{brand}` was not registered"));
    }
    let _lock = lock(home, lock_name)?;
    let mut map = read(path)?;
    let removed = map
        .get_mut(section)
        .and_then(Value::as_object_mut)
        .and_then(|s| s.shift_remove(brand))
        .is_some();
    if removed {
        write(path, &map)?;
        Ok(format!("removed `{brand}`"))
    } else {
        Ok(format!("`{brand}` was not registered"))
    }
}

/// Check if `{section}.{brand}` exists in the config at `path`.
pub fn status(path: &Path, section: &str, brand: &str) -> Status {
    let present = read(path)
        .ok()
        .and_then(|m| m.get(section)?.get(brand).cloned())
        .is_some();
    if present {
        Status::Installed
    } else {
        Status::NotInstalled
    }
}

fn lock(home: &Path, name: &str) -> Result<std::fs::File> {
    let dir = ehai_core::paths::adapters_dir(home);
    std::fs::create_dir_all(&dir).map_err(ehai_core::Error::io(&dir))?;
    Ok(ehai_core::migrate::lock(&dir.join(name))?)
}
