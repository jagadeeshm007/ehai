//! In-memory tracker: the reference implementation the contract suite is checked
//! against, and a test double for the sync engine and (later) dry runs.

use std::sync::Mutex;

use crate::{Error, Filter, Item, ItemPatch, NewItem, Result, SchemaReport, Status, Tracker};

#[derive(Debug, Default)]
pub struct MemoryTracker {
    state: Mutex<State>,
}

#[derive(Debug, Default)]
struct State {
    items: Vec<Item>,
    comments: Vec<(String, String)>,
    schema_done: bool,
}

impl MemoryTracker {
    pub fn comments(&self) -> Vec<(String, String)> {
        self.lock().comments.clone()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn with_item<T>(&self, id: &str, f: impl FnOnce(&mut Item) -> T) -> Result<T> {
        let mut s = self.lock();
        let item = s
            .items
            .iter_mut()
            .find(|i| i.id == id)
            .ok_or_else(|| Error::NotFound(format!("item {id}")))?;
        Ok(f(item))
    }
}

impl Tracker for MemoryTracker {
    async fn ensure_schema(&self) -> Result<SchemaReport> {
        let mut s = self.lock();
        if s.schema_done {
            return Ok(SchemaReport::default());
        }
        s.schema_done = true;
        Ok(SchemaReport {
            created_fields: vec!["Priority".into(), "Size".into(), "Work type".into()],
            ..Default::default()
        })
    }

    async fn create_item(&self, n: NewItem) -> Result<Item> {
        let mut s = self.lock();
        let number = s.items.len() as u64 + 1;
        let item = Item {
            id: format!("mem-{number}"),
            number: Some(number),
            url: Some(format!("memory://items/{number}")),
            title: n.title,
            body: n.body.trim_end().to_string(),
            ehai_id: n.ehai_id.filter(|id| crate::valid_ehai_id(id)),
            status: n.status.or(Some(Status::Todo)),
            priority: n.priority,
            size: n.size,
            kind: n.kind,
            closed: false,
        };
        s.items.push(item.clone());
        Ok(item)
    }

    async fn update_item(&self, id: &str, p: ItemPatch) -> Result<Item> {
        self.with_item(id, |i| {
            if let Some(t) = p.title {
                i.title = t;
            }
            if let Some(b) = p.body {
                i.body = b.trim_end().to_string();
            }
            i.priority = p.priority.or(i.priority);
            i.size = p.size.or(i.size);
            i.kind = p.kind.or(i.kind);
            i.clone()
        })
    }

    async fn set_status(&self, id: &str, status: Status) -> Result<()> {
        self.with_item(id, |i| i.status = Some(status))
    }

    async fn list_items(&self, filter: &Filter) -> Result<Vec<Item>> {
        Ok(self
            .lock()
            .items
            .iter()
            .filter(|i| filter.matches(i))
            .cloned()
            .collect())
    }

    async fn get_item(&self, id: &str) -> Result<Item> {
        self.with_item(id, |i| i.clone())
    }

    async fn comment(&self, id: &str, body: &str) -> Result<()> {
        self.with_item(id, |_| ())?;
        self.lock().comments.push((id.into(), body.into()));
        Ok(())
    }

    async fn link_pr(&self, id: &str, pr_url: &str) -> Result<()> {
        self.comment(id, &format!("Linked pull request: {pr_url}"))
            .await
    }
}
