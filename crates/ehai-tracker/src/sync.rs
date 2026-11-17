//! Idempotent sync: match desired items to tracker items by `ehai-id`, then create
//! what's missing and update what drifted. Running it twice changes nothing.
//!
//! Rules:
//! - Items are matched only by `ehai-id`; items without one are never touched.
//! - Status is set on create only. After that the board owns it (developers move
//!   cards), so a re-sync never drags a card back.
//! - Remote items whose `ehai-id` is no longer planned are reported, not deleted.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{Filter, Item, ItemPatch, Kind, NewItem, Priority, Result, Size, Status, Tracker};

/// One planned item, as the planner produces it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Desired {
    pub ehai_id: String,
    pub title: String,
    pub body: String,
    pub status: Option<Status>,
    pub priority: Option<Priority>,
    pub size: Option<Size>,
    pub kind: Option<Kind>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    Create(Desired),
    Update {
        id: String,
        ehai_id: String,
        patch: ItemPatch,
    },
    Unchanged {
        id: String,
        ehai_id: String,
    },
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Report {
    pub created: Vec<String>,
    pub updated: Vec<String>,
    pub unchanged: Vec<String>,
    /// Remote items carrying an `ehai-id` that is no longer in the plan.
    pub orphaned: Vec<String>,
    /// `(ehai-id, error)` for items that failed; the rest still synced.
    pub failed: Vec<(String, String)>,
}

/// Pure diff of desired vs remote. Duplicated or invalid desired ids are a bug in
/// the caller and are skipped (first one wins).
pub fn plan(desired: &[Desired], remote: &[Item]) -> (Vec<Op>, Vec<String>) {
    let by_id: BTreeMap<&str, &Item> = remote
        .iter()
        .filter_map(|i| i.ehai_id.as_deref().map(|id| (id, i)))
        .collect();
    let mut seen = BTreeSet::new();
    let mut ops = Vec::new();
    for d in desired {
        if !crate::valid_ehai_id(&d.ehai_id) || !seen.insert(d.ehai_id.as_str()) {
            continue;
        }
        let op = match by_id.get(d.ehai_id.as_str()) {
            None => Op::Create(d.clone()),
            Some(r) => {
                let patch = diff(d, r);
                if patch.is_empty() {
                    Op::Unchanged {
                        id: r.id.clone(),
                        ehai_id: d.ehai_id.clone(),
                    }
                } else {
                    Op::Update {
                        id: r.id.clone(),
                        ehai_id: d.ehai_id.clone(),
                        patch,
                    }
                }
            }
        };
        ops.push(op);
    }
    let orphaned = by_id
        .keys()
        .filter(|id| !seen.contains(*id))
        .map(|id| id.to_string())
        .collect();
    (ops, orphaned)
}

fn diff(d: &Desired, r: &Item) -> ItemPatch {
    fn changed<T: PartialEq + Clone>(want: &Option<T>, have: &Option<T>) -> Option<T> {
        want.as_ref().filter(|w| Some(*w) != have.as_ref()).cloned()
    }
    ItemPatch {
        title: (d.title != r.title).then(|| d.title.clone()),
        body: (d.body.trim_end() != r.body.trim_end()).then(|| d.body.clone()),
        priority: changed(&d.priority, &r.priority),
        size: changed(&d.size, &r.size),
        kind: changed(&d.kind, &r.kind),
    }
}

/// Apply a plan. Individual failures are collected so one bad item doesn't stop the rest.
pub async fn sync<T: Tracker>(tracker: &T, desired: &[Desired]) -> Result<Report> {
    let remote = tracker.list_items(&Filter::default()).await?;
    let (ops, orphaned) = plan(desired, &remote);
    let mut report = Report {
        orphaned,
        ..Default::default()
    };
    for op in ops {
        match op {
            Op::Create(d) => {
                let id = d.ehai_id.clone();
                let r = tracker
                    .create_item(NewItem {
                        ehai_id: Some(d.ehai_id),
                        title: d.title,
                        body: d.body,
                        status: d.status,
                        priority: d.priority,
                        size: d.size,
                        kind: d.kind,
                    })
                    .await;
                match r {
                    Ok(_) => report.created.push(id),
                    Err(e) => report.failed.push((id, e.to_string())),
                }
            }
            Op::Update { id, ehai_id, patch } => match tracker.update_item(&id, patch).await {
                Ok(_) => report.updated.push(ehai_id),
                Err(e) => report.failed.push((ehai_id, e.to_string())),
            },
            Op::Unchanged { ehai_id, .. } => report.unchanged.push(ehai_id),
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::MemoryTracker;
    use proptest::prelude::*;

    fn d(id: &str, title: &str) -> Desired {
        Desired {
            ehai_id: id.into(),
            title: title.into(),
            body: format!("body of {id}"),
            status: Some(Status::Backlog),
            priority: Some(Priority::P2),
            size: Some(Size::M),
            kind: Some(Kind::Task),
        }
    }

    #[tokio::test]
    async fn second_sync_changes_nothing() {
        let t = MemoryTracker::default();
        let plan = vec![d("t-1", "One"), d("t-2", "Two")];
        let r1 = sync(&t, &plan).await.unwrap();
        assert_eq!(r1.created, vec!["t-1", "t-2"]);
        let r2 = sync(&t, &plan).await.unwrap();
        assert!(r2.created.is_empty() && r2.updated.is_empty(), "{r2:?}");
        assert_eq!(r2.unchanged.len(), 2);
        assert_eq!(t.list_items(&Filter::default()).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn updates_drift_but_keeps_board_status() {
        let t = MemoryTracker::default();
        sync(&t, &[d("t-1", "One")]).await.unwrap();
        let id = t.list_items(&Filter::default()).await.unwrap()[0]
            .id
            .clone();
        t.set_status(&id, Status::InProgress).await.unwrap(); // a dev moved the card

        let mut changed = d("t-1", "One, renamed");
        changed.priority = Some(Priority::P0);
        let r = sync(&t, &[changed]).await.unwrap();
        assert_eq!(r.updated, vec!["t-1"]);
        let item = t.get_item(&id).await.unwrap();
        assert_eq!(item.title, "One, renamed");
        assert_eq!(item.priority, Some(Priority::P0));
        assert_eq!(
            item.status,
            Some(Status::InProgress),
            "sync never moves cards"
        );
    }

    #[tokio::test]
    async fn orphans_reported_and_foreign_items_untouched() {
        let t = MemoryTracker::default();
        sync(&t, &[d("t-1", "One"), d("t-2", "Two")]).await.unwrap();
        t.create_item(NewItem {
            title: "hand-made".into(),
            ..Default::default()
        })
        .await
        .unwrap();
        let r = sync(&t, &[d("t-1", "One")]).await.unwrap();
        assert_eq!(r.orphaned, vec!["t-2"]);
        assert_eq!(
            t.list_items(&Filter::default()).await.unwrap().len(),
            3,
            "nothing deleted"
        );
    }

    #[test]
    fn duplicate_and_invalid_ids_skipped() {
        let (ops, _) = plan(&[d("a", "1"), d("a", "2"), d("bad id", "3")], &[]);
        assert_eq!(ops.len(), 1);
    }

    proptest! {
        /// For any plan, sync followed by sync is a fixed point.
        #[test]
        fn sync_is_idempotent(titles in proptest::collection::vec("[A-Za-z ]{1,20}", 0..12)) {
            let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
            rt.block_on(async {
                let t = MemoryTracker::default();
                let plan: Vec<_> = titles.iter().enumerate().map(|(i, s)| d(&format!("x-{i}"), s)).collect();
                sync(&t, &plan).await.unwrap();
                let again = sync(&t, &plan).await.unwrap();
                assert!(again.created.is_empty() && again.updated.is_empty());
                assert_eq!(t.list_items(&Filter::default()).await.unwrap().len(), plan.len());
            });
        }
    }
}
