//! Local JSON tracker — offline fallback when GitHub is unavailable.
//!
//! All items are stored in `~/.ehai/tracker.json` (or `EHAI_LOCAL_TRACKER`
//! for tests). The file is a JSON object `{"items": [...]}`. Each create/update
//! locks the file, reads, mutates, and rewrites atomically via `write_atomic`.
//!
//! Supports all PM flows (plan → breakdown → sync → status → triage).
//! PR linking is a no-op (returns `Ok(())`); the PR URL is recorded in the
//! item body so it is not lost. `ensure_schema` is always a no-op (the JSON
//! format is schemaless).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::{Filter, Item, ItemPatch, NewItem, Result, SchemaReport, Status, Tracker};

const EHAI_ID_PREFIX: &str = "\n\n<!-- ehai-id:";
const EHAI_ID_SUFFIX: &str = " -->";

#[derive(Debug, Serialize, Deserialize, Default)]
struct Store {
    items: Vec<Item>,
    #[serde(default)]
    next_number: u64,
}

pub struct LocalTracker {
    path: PathBuf,
    // mutex so multiple MCP tool calls in the same process don't race
    lock: Mutex<()>,
}

impl LocalTracker {
    pub fn new(home: &Path) -> Self {
        let path = std::env::var_os("EHAI_LOCAL_TRACKER")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("tracker.json"));
        Self {
            path,
            lock: Mutex::new(()),
        }
    }

    /// For tests: point at a specific file.
    pub fn at(path: PathBuf) -> Self {
        Self {
            path,
            lock: Mutex::new(()),
        }
    }

    fn read(&self) -> Result<Store> {
        match std::fs::read_to_string(&self.path) {
            Ok(s) => serde_json::from_str(&s).map_err(|e| {
                crate::Error::Api(format!(
                    "local tracker corrupted at {}: {e}",
                    self.path.display()
                ))
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Store::default()),
            Err(e) => Err(crate::Error::Core(ehai_core::Error::io(&self.path)(e))),
        }
    }

    fn write(&self, store: &Store) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir).map_err(ehai_core::Error::io(dir))?;
        }
        let bytes = serde_json::to_vec_pretty(store)
            .map_err(|e| crate::Error::Api(format!("local tracker serialization: {e}")))?;
        ehai_core::migrate::write_atomic(&self.path, &bytes)?;
        Ok(())
    }

    fn with_store<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut Store) -> Result<T>,
    {
        let _guard = self.lock.lock().unwrap();
        let mut store = self.read()?;
        let result = f(&mut store)?;
        self.write(&store)?;
        Ok(result)
    }
}

fn new_id() -> String {
    // Simple time-based ID without Date::now() (forbidden in workflow scripts,
    // but this is production Rust code — std::time is fine)
    use std::time::{SystemTime, UNIX_EPOCH};
    let ns = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    let seq = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("local-{seq:x}{ns:04x}")
}

fn encode_ehai_id(id: &str) -> String {
    format!("{EHAI_ID_PREFIX}{id}{EHAI_ID_SUFFIX}")
}

#[cfg(test)]
fn decode_ehai_id(body: &str) -> Option<String> {
    let start = body.find(EHAI_ID_PREFIX)? + EHAI_ID_PREFIX.len();
    let rest = &body[start..];
    let end = rest.find(EHAI_ID_SUFFIX)?;
    Some(rest[..end].trim().to_string())
}

fn strip_ehai_footer(body: &str) -> String {
    if let Some(pos) = body.find(EHAI_ID_PREFIX) {
        body[..pos].to_string()
    } else {
        body.to_string()
    }
}

impl Tracker for LocalTracker {
    async fn ensure_schema(&self) -> Result<SchemaReport> {
        Ok(SchemaReport::default()) // JSON store is schemaless
    }

    async fn create_item(&self, item: NewItem) -> Result<Item> {
        self.with_store(|store| {
            store.next_number += 1;
            let number = store.next_number;
            let id = new_id();
            let ehai_id = item.ehai_id.clone();
            let body_with_footer = match &ehai_id {
                Some(eid) => format!("{}{}", item.body, encode_ehai_id(eid)),
                None => item.body.clone(),
            };
            let entry = Item {
                id: id.clone(),
                number: Some(number),
                url: None,
                title: item.title,
                body: body_with_footer,
                ehai_id,
                status: item.status,
                priority: item.priority,
                size: item.size,
                kind: item.kind,
                closed: false,
                updated_at: Some(ehai_core::date::today_utc()),
                assignees: vec![],
            };
            store.items.push(entry.clone());
            Ok(entry)
        })
    }

    async fn update_item(&self, id: &str, patch: ItemPatch) -> Result<Item> {
        self.with_store(|store| {
            let entry = store
                .items
                .iter_mut()
                .find(|i| i.id == id)
                .ok_or_else(|| crate::Error::NotFound(id.to_string()))?;
            if let Some(t) = patch.title {
                entry.title = t;
            }
            if let Some(b) = patch.body {
                // preserve ehai-id footer
                let footer = entry
                    .ehai_id
                    .as_deref()
                    .map(encode_ehai_id)
                    .unwrap_or_default();
                entry.body = format!("{b}{footer}");
            }
            if let Some(p) = patch.priority {
                entry.priority = Some(p);
            }
            if let Some(s) = patch.size {
                entry.size = Some(s);
            }
            if let Some(k) = patch.kind {
                entry.kind = Some(k);
            }
            entry.updated_at = Some(ehai_core::date::today_utc());
            Ok(entry.clone())
        })
    }

    async fn set_status(&self, id: &str, status: Status) -> Result<()> {
        self.with_store(|store| {
            let entry = store
                .items
                .iter_mut()
                .find(|i| i.id == id)
                .ok_or_else(|| crate::Error::NotFound(id.to_string()))?;
            entry.status = Some(status);
            entry.closed = matches!(status, Status::Done);
            entry.updated_at = Some(ehai_core::date::today_utc());
            Ok(())
        })
    }

    async fn list_items(&self, filter: &Filter) -> Result<Vec<Item>> {
        let store = self.read()?;
        Ok(store
            .items
            .into_iter()
            .filter(|i| filter.matches(i))
            .map(|mut i| {
                i.body = strip_ehai_footer(&i.body);
                i
            })
            .collect())
    }

    async fn get_item(&self, id: &str) -> Result<Item> {
        let store = self.read()?;
        store
            .items
            .into_iter()
            .find(|i| i.id == id)
            .map(|mut i| {
                i.body = strip_ehai_footer(&i.body);
                i
            })
            .ok_or_else(|| crate::Error::NotFound(id.to_string()))
    }

    async fn comment(&self, id: &str, body: &str) -> Result<()> {
        // Append comment to item body (local tracker has no separate comment thread)
        self.with_store(|store| {
            let entry = store
                .items
                .iter_mut()
                .find(|i| i.id == id)
                .ok_or_else(|| crate::Error::NotFound(id.to_string()))?;
            let footer = entry
                .ehai_id
                .as_deref()
                .map(encode_ehai_id)
                .unwrap_or_default();
            let clean = strip_ehai_footer(&entry.body);
            entry.body = format!("{clean}\n\n---\n**Comment:** {body}{footer}");
            entry.updated_at = Some(ehai_core::date::today_utc());
            Ok(())
        })
    }

    async fn link_pr(&self, id: &str, pr_url: &str) -> Result<()> {
        // Record the PR URL in the body so it isn't lost, but no actual GitHub link
        self.with_store(|store| {
            let entry = store
                .items
                .iter_mut()
                .find(|i| i.id == id)
                .ok_or_else(|| crate::Error::NotFound(id.to_string()))?;
            let footer = entry
                .ehai_id
                .as_deref()
                .map(encode_ehai_id)
                .unwrap_or_default();
            let clean = strip_ehai_footer(&entry.body);
            entry.body = format!("{clean}\n\n**PR:** {pr_url}{footer}");
            entry.updated_at = Some(ehai_core::date::today_utc());
            Ok(())
        })
    }

    async fn assign(&self, id: &str, logins: &[String]) -> Result<()> {
        self.with_store(|store| {
            let entry = store
                .items
                .iter_mut()
                .find(|i| i.id == id)
                .ok_or_else(|| crate::Error::NotFound(id.to_string()))?;
            for login in logins {
                if !entry.assignees.contains(login) {
                    entry.assignees.push(login.clone());
                }
            }
            entry.updated_at = Some(ehai_core::date::today_utc());
            Ok(())
        })
    }

    async fn unassign(&self, id: &str, logins: &[String]) -> Result<()> {
        self.with_store(|store| {
            let entry = store
                .items
                .iter_mut()
                .find(|i| i.id == id)
                .ok_or_else(|| crate::Error::NotFound(id.to_string()))?;
            entry.assignees.retain(|l| !logins.contains(l));
            entry.updated_at = Some(ehai_core::date::today_utc());
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Kind, Priority, Size};

    fn tracker() -> (LocalTracker, tempfile::TempDir) {
        let d = tempfile::tempdir().unwrap();
        let t = LocalTracker::at(d.path().join("tracker.json"));
        (t, d)
    }

    fn new_item(title: &str, ehai_id: &str) -> NewItem {
        NewItem {
            ehai_id: Some(ehai_id.to_string()),
            title: title.to_string(),
            body: "body text".to_string(),
            status: Some(Status::Todo),
            priority: Some(Priority::P2),
            size: Some(Size::M),
            kind: Some(Kind::Task),
        }
    }

    #[tokio::test]
    async fn create_list_get_roundtrip() {
        let (t, _d) = tracker();
        let item = t
            .create_item(new_item("My task", "my-plan.T1"))
            .await
            .unwrap();
        assert!(item.id.starts_with("local-"));
        assert_eq!(item.number, Some(1));
        assert_eq!(item.ehai_id.as_deref(), Some("my-plan.T1"));

        let list = t.list_items(&Filter::default()).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "My task");
        assert!(
            !list[0].body.contains(EHAI_ID_PREFIX),
            "footer stripped from list"
        );

        let got = t.get_item(&item.id).await.unwrap();
        assert_eq!(got.ehai_id.as_deref(), Some("my-plan.T1"));
    }

    #[tokio::test]
    async fn update_and_set_status() {
        let (t, _d) = tracker();
        let item = t.create_item(new_item("T", "p.T2")).await.unwrap();
        t.set_status(&item.id, Status::InProgress).await.unwrap();
        let got = t.get_item(&item.id).await.unwrap();
        assert_eq!(got.status, Some(Status::InProgress));
        assert!(!got.closed);

        t.set_status(&item.id, Status::Done).await.unwrap();
        let done = t.get_item(&item.id).await.unwrap();
        assert!(done.closed);
    }

    #[tokio::test]
    async fn filter_by_ehai_id() {
        let (t, _d) = tracker();
        t.create_item(new_item("A", "plan.A")).await.unwrap();
        t.create_item(new_item("B", "plan.B")).await.unwrap();
        let filter = Filter {
            ehai_id: Some("plan.A".to_string()),
            ..Default::default()
        };
        let list = t.list_items(&filter).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "A");
    }

    #[tokio::test]
    async fn link_pr_recorded_in_body() {
        let (t, _d) = tracker();
        let item = t.create_item(new_item("T", "p.T3")).await.unwrap();
        t.link_pr(&item.id, "https://github.com/owner/repo/pull/42")
            .await
            .unwrap();
        let got = t.get_item(&item.id).await.unwrap();
        assert!(got.body.contains("pull/42"));
    }

    #[tokio::test]
    async fn ensure_schema_is_noop() {
        let (t, _d) = tracker();
        let r = t.ensure_schema().await.unwrap();
        assert!(r.is_noop());
    }

    #[test]
    fn ehai_id_encode_decode_roundtrip() {
        let body = format!("some text{}", encode_ehai_id("plan.T1"));
        assert_eq!(decode_ehai_id(&body).as_deref(), Some("plan.T1"));
        assert_eq!(strip_ehai_footer(&body), "some text");
    }
}
