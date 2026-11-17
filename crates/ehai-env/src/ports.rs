//! Per-worktree port allocation (spec §6, step 4): a registry at
//! `~/.ehai/ports.json`, locked on every edit so parallel harness sessions
//! never hand out the same port twice.

use std::collections::{BTreeMap, BTreeSet};
use std::net::TcpListener;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::{Error, Result};

const BASE_PORT: u16 = 20000;
const MAX_PORT: u16 = 40000;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortRegistry {
    assignments: BTreeMap<String, Vec<u16>>,
}

impl PortRegistry {
    pub fn load(home: &Path) -> Result<Self> {
        let path = ehai_core::paths::ports_registry(home);
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(serde_json::from_str(&text).unwrap_or_default()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::io(&path)(e)),
        }
    }

    fn used(&self, except: &str) -> BTreeSet<u16> {
        self.assignments
            .iter()
            .filter(|(id, _)| id.as_str() != except)
            .flat_map(|(_, ports)| ports.iter().copied())
            .collect()
    }

    /// `n` ports for `worktree_id`, stable across calls (a re-sync keeps them).
    pub fn allocate(&self, home: &Path, worktree_id: &str, n: usize) -> Result<Vec<u16>> {
        let path = ehai_core::paths::ports_registry(home);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(Error::io(dir))?;
        }
        let _lock = ehai_core::migrate::lock(&path)?;
        let mut reg = Self::load(home)?;

        let mut ports = reg
            .assignments
            .get(worktree_id)
            .cloned()
            .unwrap_or_default();
        ports.truncate(n);
        let used = reg.used(worktree_id);
        while ports.len() < n {
            let taken: BTreeSet<u16> = used
                .union(&ports.iter().copied().collect())
                .copied()
                .collect();
            let port = free_port(&taken).ok_or_else(|| {
                Error::Core(ehai_core::Error::Invalid("no free port in range".into()))
            })?;
            ports.push(port);
        }
        reg.assignments
            .insert(worktree_id.to_string(), ports.clone());
        reg.save(home)?;
        Ok(ports)
    }

    pub fn release(&self, home: &Path, worktree_id: &str) -> Result<()> {
        let path = ehai_core::paths::ports_registry(home);
        let _lock = ehai_core::migrate::lock(&path)?;
        let mut reg = Self::load(home)?;
        reg.assignments.remove(worktree_id);
        reg.save(home)
    }

    fn save(&self, home: &Path) -> Result<()> {
        let path = ehai_core::paths::ports_registry(home);
        let body = serde_json::to_string_pretty(self)
            .map_err(|e| Error::Core(ehai_core::Error::Invalid(e.to_string())))?;
        Ok(ehai_core::migrate::write_atomic(&path, body.as_bytes())?)
    }
}

fn free_port(taken: &BTreeSet<u16>) -> Option<u16> {
    (BASE_PORT..MAX_PORT)
        .find(|p| !taken.contains(p) && TcpListener::bind(("127.0.0.1", *p)).is_ok())
}

/// Write `.env.ehai.local` with the allocated ports plus a unique
/// `COMPOSE_PROJECT_NAME`, overlaying whatever the app's own `.env*` sets.
pub fn write_env_overlay(
    worktree_root: &Path,
    worktree_id: &str,
    vars: &[(String, u16)],
) -> Result<()> {
    let mut body = format!("COMPOSE_PROJECT_NAME={}\n", sanitize(worktree_id));
    for (k, v) in vars {
        body += &format!("{k}={v}\n");
    }
    let path = worktree_root.join(".env.ehai.local");
    std::fs::write(&path, body).map_err(Error::io(&path))
}

fn sanitize(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocates_stable_disjoint_ports_per_worktree() {
        let home = tempfile::tempdir().unwrap();
        let reg = PortRegistry::load(home.path()).unwrap();
        let a = reg.allocate(home.path(), "wt-a", 2).unwrap();
        let b = reg.allocate(home.path(), "wt-b", 2).unwrap();
        assert_eq!(a.len(), 2);
        assert_eq!(b.len(), 2);
        assert!(a
            .iter()
            .collect::<BTreeSet<_>>()
            .is_disjoint(&b.iter().collect()));

        // re-sync (same id, same count) is a no-op: same ports come back.
        let again = reg.allocate(home.path(), "wt-a", 2).unwrap();
        assert_eq!(a, again);
    }

    #[test]
    fn release_frees_ports_for_reuse() {
        let home = tempfile::tempdir().unwrap();
        let reg = PortRegistry::load(home.path()).unwrap();
        let a = reg.allocate(home.path(), "wt-a", 1).unwrap();
        reg.release(home.path(), "wt-a").unwrap();
        let reg = PortRegistry::load(home.path()).unwrap();
        assert!(!reg.assignments.contains_key("wt-a"));
        let b = reg.allocate(home.path(), "wt-b", 1).unwrap();
        // not asserting b == a (another process could've grabbed it), just that
        // the registry no longer reserves it for wt-a.
        let _ = (a, b);
    }

    #[test]
    fn many_worktrees_never_collide() {
        let home = tempfile::tempdir().unwrap();
        let reg = PortRegistry::load(home.path()).unwrap();
        let mut all = BTreeSet::new();
        for i in 0..20 {
            let ports = reg.allocate(home.path(), &format!("wt-{i}"), 3).unwrap();
            for p in ports {
                assert!(all.insert(p), "port {p} allocated twice");
            }
        }
    }
}
