//! `AnyTracker` — an enum that dispatches over all concrete tracker variants so
//! callers don't need a `dyn Tracker` (the trait uses `impl Future`, which is
//! object-unsafe without async_trait). Add a new variant here when a new
//! adapter lands. Implements `Tracker` via `async fn` delegation so it works
//! with `sync::sync` and other generic callers.

use crate::{
    github::GithubProjects, local::LocalTracker, Filter, Item, ItemPatch, NewItem, Result,
    SchemaReport, Status, Tracker,
};

pub enum AnyTracker {
    Github(GithubProjects),
    Local(LocalTracker),
}

impl Tracker for AnyTracker {
    async fn ensure_schema(&self) -> Result<SchemaReport> {
        match self {
            AnyTracker::Github(t) => t.ensure_schema().await,
            AnyTracker::Local(t) => t.ensure_schema().await,
        }
    }
    async fn create_item(&self, item: NewItem) -> Result<Item> {
        match self {
            AnyTracker::Github(t) => t.create_item(item).await,
            AnyTracker::Local(t) => t.create_item(item).await,
        }
    }
    async fn update_item(&self, id: &str, patch: ItemPatch) -> Result<Item> {
        match self {
            AnyTracker::Github(t) => t.update_item(id, patch).await,
            AnyTracker::Local(t) => t.update_item(id, patch).await,
        }
    }
    async fn set_status(&self, id: &str, status: Status) -> Result<()> {
        match self {
            AnyTracker::Github(t) => t.set_status(id, status).await,
            AnyTracker::Local(t) => t.set_status(id, status).await,
        }
    }
    async fn list_items(&self, filter: &Filter) -> Result<Vec<Item>> {
        match self {
            AnyTracker::Github(t) => t.list_items(filter).await,
            AnyTracker::Local(t) => t.list_items(filter).await,
        }
    }
    async fn get_item(&self, id: &str) -> Result<Item> {
        match self {
            AnyTracker::Github(t) => t.get_item(id).await,
            AnyTracker::Local(t) => t.get_item(id).await,
        }
    }
    async fn comment(&self, id: &str, body: &str) -> Result<()> {
        match self {
            AnyTracker::Github(t) => t.comment(id, body).await,
            AnyTracker::Local(t) => t.comment(id, body).await,
        }
    }
    async fn link_pr(&self, id: &str, pr_url: &str) -> Result<()> {
        match self {
            AnyTracker::Github(t) => t.link_pr(id, pr_url).await,
            AnyTracker::Local(t) => t.link_pr(id, pr_url).await,
        }
    }
    async fn assign(&self, id: &str, logins: &[String]) -> Result<()> {
        match self {
            AnyTracker::Github(t) => t.assign(id, logins).await,
            AnyTracker::Local(t) => t.assign(id, logins).await,
        }
    }
    async fn unassign(&self, id: &str, logins: &[String]) -> Result<()> {
        match self {
            AnyTracker::Github(t) => t.unassign(id, logins).await,
            AnyTracker::Local(t) => t.unassign(id, logins).await,
        }
    }
}
