//! PRD → epics → stories → tasks, with Given/When/Then acceptance criteria, and
//! the conversion into tracker items (`sync::Desired`).

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use ehai_tracker::sync::Desired;
use ehai_tracker::{Kind, Priority, Size, Status};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::prd::cell;
use crate::{plan_dir, read_json, write_json, Error, Result};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    /// React Native (Expo or bare)
    Rn,
    /// React web (Vite / Next.js / CRA)
    Web,
    /// Go backend
    Go,
    /// Python backend
    Py,
}

impl Platform {
    pub fn tag(self) -> &'static str {
        match self {
            Platform::Rn => "rn",
            Platform::Web => "web",
            Platform::Go => "go",
            Platform::Py => "py",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Criterion {
    pub given: String,
    pub when: String,
    pub then: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Breakdown {
    /// Slug of the PRD this breaks down.
    pub prd: String,
    pub epics: Vec<Epic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Epic {
    /// Short stable id, e.g. `e1`. Letters, digits, `.`, `_`, `-`.
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub priority: Priority,
    pub stories: Vec<Story>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Story {
    pub id: String,
    pub title: String,
    /// "As a …, I want …, so that …".
    pub description: String,
    pub acceptance: Vec<Criterion>,
    pub priority: Priority,
    pub size: Size,
    #[serde(default)]
    pub platforms: Vec<Platform>,
    /// Ids of stories/tasks this depends on.
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Logins to assign when this story is first created.
    #[serde(default)]
    pub assignees: Vec<String>,
    #[serde(default)]
    pub tasks: Vec<Task>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Task {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub size: Size,
    #[serde(default)]
    pub priority: Option<Priority>,
    #[serde(default)]
    pub platforms: Vec<Platform>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Logins to assign when this task is first created. Inherits the story's
    /// assignees when empty, like `platforms` inherits.
    #[serde(default)]
    pub assignees: Vec<String>,
}

/// A flattened row: every epic, story and task with its parent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub kind: Kind,
    pub title: String,
    pub parent: Option<String>,
    pub priority: Priority,
    pub size: Option<Size>,
    pub platforms: Vec<Platform>,
    pub depends_on: Vec<String>,
    pub assignees: Vec<String>,
}

impl Breakdown {
    pub fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for e in &self.epics {
            rows.push(Row {
                id: e.id.clone(),
                kind: Kind::Epic,
                title: e.title.clone(),
                parent: None,
                priority: e.priority,
                size: None,
                platforms: vec![],
                depends_on: vec![],
                assignees: vec![],
            });
            for s in &e.stories {
                rows.push(Row {
                    id: s.id.clone(),
                    kind: Kind::Story,
                    title: s.title.clone(),
                    parent: Some(e.id.clone()),
                    priority: s.priority,
                    size: Some(s.size),
                    platforms: s.platforms.clone(),
                    depends_on: s.depends_on.clone(),
                    assignees: s.assignees.clone(),
                });
                for t in &s.tasks {
                    rows.push(Row {
                        id: t.id.clone(),
                        kind: Kind::Task,
                        title: t.title.clone(),
                        parent: Some(s.id.clone()),
                        priority: t.priority.unwrap_or(s.priority),
                        size: Some(t.size),
                        platforms: if t.platforms.is_empty() {
                            s.platforms.clone()
                        } else {
                            t.platforms.clone()
                        },
                        depends_on: t.depends_on.clone(),
                        assignees: if t.assignees.is_empty() {
                            s.assignees.clone()
                        } else {
                            t.assignees.clone()
                        },
                    });
                }
            }
        }
        rows
    }

    /// Unique valid ids, known dependencies, no cycles, every story testable.
    pub fn problems(&self) -> Vec<String> {
        let mut p = Vec::new();
        let rows = self.rows();
        if rows.is_empty() {
            p.push("the breakdown has no epics".into());
        }
        let mut seen = BTreeSet::new();
        for r in &rows {
            let full = item_id(&self.prd, &r.id);
            if !ehai_tracker::valid_ehai_id(&full) {
                p.push(format!(
                    "id `{}` must be short and use only letters, digits, `.`, `_`, `-` (no `--`)",
                    r.id
                ));
            }
            if !seen.insert(r.id.as_str()) {
                p.push(format!("id `{}` is used twice", r.id));
            }
        }
        for r in &rows {
            for d in &r.depends_on {
                if !seen.contains(d.as_str()) {
                    p.push(format!("`{}` depends on unknown `{d}`", r.id));
                } else if d == &r.id {
                    p.push(format!("`{}` depends on itself", r.id));
                }
            }
        }
        for e in &self.epics {
            if e.stories.is_empty() {
                p.push(format!("epic `{}` has no stories", e.id));
            }
            for s in &e.stories {
                if s.acceptance.is_empty() {
                    p.push(format!(
                        "story `{}` needs acceptance criteria (Given/When/Then)",
                        s.id
                    ));
                }
            }
        }
        if let Some(cycle) = find_cycle(&rows) {
            p.push(format!("dependency cycle: {}", cycle.join(" → ")));
        }
        p
    }

    /// Board preview: a table plus a Mermaid graph of hierarchy and dependencies.
    pub fn preview(&self) -> String {
        let rows = self.rows();
        let mut md = String::from("| ID | Type | Title | Priority | Size | Platforms | Assignees | Depends on |\n|---|---|---|---|---|---|---|---|\n");
        for r in &rows {
            md += &format!(
                "| {} | {} | {} | {} | {} | {} | {} | {} |\n",
                r.id,
                r.kind.label(),
                cell(&r.title),
                r.priority.label(),
                r.size.map_or("", Size::label),
                r.platforms
                    .iter()
                    .map(|p| p.tag())
                    .collect::<Vec<_>>()
                    .join(", "),
                r.assignees.join(", "),
                r.depends_on.join(", "),
            );
        }
        let (epics, stories, tasks) = rows.iter().fold((0, 0, 0), |(e, s, t), r| match r.kind {
            Kind::Epic => (e + 1, s, t),
            Kind::Story => (e, s + 1, t),
            _ => (e, s, t + 1),
        });
        md += &format!(
            "\n**{} · {} · {}**\n\n```mermaid\nflowchart LR\n",
            crate::count(epics, "epic", "epics"),
            crate::count(stories, "story", "stories"),
            crate::count(tasks, "task", "tasks")
        );
        for r in &rows {
            md += &format!(
                "  {}[\"{}\"]\n",
                node(&r.id),
                mermaid_label(&format!("{}: {}", r.kind.label(), r.title))
            );
        }
        for r in &rows {
            if let Some(parent) = &r.parent {
                md += &format!("  {} --- {}\n", node(parent), node(&r.id));
            }
            for d in &r.depends_on {
                md += &format!("  {} -.->|blocks| {}\n", node(d), node(&r.id));
            }
        }
        md += "```\n";
        md
    }

    /// Tracker items, one per epic/story/task, keyed `<prd>.<id>`.
    pub fn to_desired(&self) -> Vec<Desired> {
        let titles: BTreeMap<String, String> =
            self.rows().into_iter().map(|r| (r.id, r.title)).collect();
        let mut out = Vec::new();
        let push = |out: &mut Vec<Desired>, r: Row, body: String| {
            out.push(Desired {
                ehai_id: item_id(&self.prd, &r.id),
                title: r.title,
                body,
                status: Some(Status::Backlog),
                priority: Some(r.priority),
                size: r.size,
                kind: Some(r.kind),
                assignees: r.assignees,
            });
        };
        let rows = self.rows();
        for r in rows {
            let mut body = String::new();
            match r.kind {
                Kind::Epic => {
                    let e = self
                        .epics
                        .iter()
                        .find(|e| e.id == r.id)
                        .expect("row from epic");
                    body += e.description.trim();
                    body += "\n\n**Stories**\n";
                    for s in &e.stories {
                        body += &format!("- [ ] {}\n", self.reference(&s.id, &titles));
                    }
                }
                Kind::Story => {
                    let s = self
                        .stories()
                        .find(|s| s.id == r.id)
                        .expect("row from story");
                    body += s.description.trim();
                    body += "\n\n**Acceptance criteria**\n";
                    for c in &s.acceptance {
                        body += &format!(
                            "- **Given** {} **When** {} **Then** {}\n",
                            c.given.trim(),
                            c.when.trim(),
                            c.then.trim()
                        );
                    }
                    if !s.tasks.is_empty() {
                        body += "\n**Tasks**\n";
                        for t in &s.tasks {
                            body += &format!("- [ ] {}\n", self.reference(&t.id, &titles));
                        }
                    }
                }
                _ => {
                    let t = self
                        .stories()
                        .flat_map(|s| &s.tasks)
                        .find(|t| t.id == r.id)
                        .expect("row from task");
                    body += t.description.trim();
                    body += "\n";
                }
            }
            let mut meta = Vec::new();
            if let Some(p) = &r.parent {
                meta.push(format!("Part of: {}", self.reference(p, &titles)));
            }
            if !r.depends_on.is_empty() {
                let deps: Vec<String> = r
                    .depends_on
                    .iter()
                    .map(|d| self.reference(d, &titles))
                    .collect();
                meta.push(format!("Depends on: {}", deps.join("; ")));
            }
            if !r.platforms.is_empty() {
                meta.push(format!(
                    "Platforms: {}",
                    r.platforms
                        .iter()
                        .map(|p| format!("`{}`", p.tag()))
                        .collect::<Vec<_>>()
                        .join(" ")
                ));
            }
            if !r.assignees.is_empty() {
                meta.push(format!(
                    "Assignees: {}",
                    r.assignees
                        .iter()
                        .map(|a| format!("@{a}"))
                        .collect::<Vec<_>>()
                        .join(" ")
                ));
            }
            if !meta.is_empty() {
                body += &format!("\n{}\n", meta.join("  \n"));
            }
            push(&mut out, r, body.trim_start().to_string());
        }
        out
    }

    /// Tracker items with cross-references resolved: `#<number>` for items that
    /// already exist as issues (GitHub shows their live title, so a rename touches
    /// only the renamed issue), the title otherwise.
    pub fn to_desired_with(&self, numbers: &BTreeMap<String, u64>) -> Vec<Desired> {
        let mut out = self.to_desired();
        for d in &mut out {
            d.body = resolve_refs(&d.body, numbers);
        }
        out
    }

    /// `[[ehai-ref:<ehai-id>|<title>]]`, resolved later by [`resolve_refs`].
    fn reference(&self, id: &str, titles: &BTreeMap<String, String>) -> String {
        let title = titles
            .get(id)
            .map_or(id, String::as_str)
            .replace("]]", "] ]");
        format!("{REF_OPEN}{}|{title}]]", item_id(&self.prd, id))
    }

    fn stories(&self) -> impl Iterator<Item = &Story> {
        self.epics.iter().flat_map(|e| &e.stories)
    }

    pub fn save(&self, home: &Path) -> Result<std::path::PathBuf> {
        let problems = self.problems();
        if !problems.is_empty() {
            return Err(Error::Invalid(problems));
        }
        let path = plan_dir(home, &self.prd).join("breakdown.json");
        write_json(&path, self)?;
        Ok(path)
    }

    pub fn load(home: &Path, prd: &str) -> Result<Self> {
        read_json(&plan_dir(home, prd).join("breakdown.json"), prd)
    }
}

const REF_OPEN: &str = "[[ehai-ref:";

/// Replace `[[ehai-ref:<ehai-id>|<title>]]` tokens with `#<number>` when the
/// item's issue number is known, else with its title.
pub fn resolve_refs(body: &str, numbers: &BTreeMap<String, u64>) -> String {
    let mut out = String::with_capacity(body.len());
    let mut rest = body;
    while let Some(start) = rest.find(REF_OPEN) {
        out.push_str(&rest[..start]);
        let after = &rest[start + REF_OPEN.len()..];
        let Some(end) = after.find("]]") else {
            out.push_str(&rest[start..]);
            return out;
        };
        let (id, title) = after[..end].split_once('|').unwrap_or((&after[..end], ""));
        match numbers.get(id) {
            Some(n) => out.push_str(&format!("#{n}")),
            None => out.push_str(title),
        }
        rest = &after[end + 2..];
    }
    out.push_str(rest);
    out
}

/// Tracker `ehai-id` for an item: `<prd-slug>.<id>`, slug cut so it stays ≤ 64.
pub fn item_id(prd: &str, id: &str) -> String {
    let room = 63usize.saturating_sub(id.len()).min(prd.len());
    let prefix = prd[..room].trim_end_matches('-');
    format!("{prefix}.{id}")
}

fn node(id: &str) -> String {
    format!("n_{}", id.replace(['.', '-'], "_"))
}

fn mermaid_label(s: &str) -> String {
    s.replace('"', "'").replace(['[', ']'], "")
}

/// A dependency cycle among rows, if any (iterative DFS, colours).
fn find_cycle(rows: &[Row]) -> Option<Vec<String>> {
    let deps: BTreeMap<&str, Vec<&str>> = rows
        .iter()
        .map(|r| {
            (
                r.id.as_str(),
                r.depends_on.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    let mut state: BTreeMap<&str, u8> = BTreeMap::new(); // 1 = on stack, 2 = done
    for start in deps.keys() {
        if state.contains_key(start) {
            continue;
        }
        let mut stack: Vec<(&str, usize)> = vec![(start, 0)];
        state.insert(start, 1);
        while let Some((node, i)) = stack.pop() {
            let next = deps.get(node).and_then(|d| d.get(i)).copied();
            match next {
                Some(n) => {
                    stack.push((node, i + 1));
                    match state.get(n) {
                        Some(1) => {
                            let mut cycle: Vec<String> = stack
                                .iter()
                                .map(|(x, _)| x.to_string())
                                .skip_while(|x| x != n)
                                .collect();
                            cycle.push(n.to_string());
                            return Some(cycle);
                        }
                        Some(_) => {}
                        None if deps.contains_key(n) => {
                            state.insert(n, 1);
                            stack.push((n, 0));
                        }
                        None => {}
                    }
                }
                None => {
                    state.insert(node, 2);
                }
            }
        }
    }
    None
}

#[cfg(test)]
pub(crate) fn sample() -> Breakdown {
    let ac = |g: &str, w: &str, t: &str| Criterion {
        given: g.into(),
        when: w.into(),
        then: t.into(),
    };
    Breakdown {
        prd: "online-appointment-booking".into(),
        epics: vec![Epic {
            id: "e1".into(),
            title: "Self-service booking".into(),
            description: "Patients manage visits in the app.".into(),
            priority: Priority::P1,
            stories: vec![
                Story {
                    id: "s1".into(),
                    title: "See free slots".into(),
                    description:
                        "As a patient, I want to see free slots, so that I can pick a time.".into(),
                    acceptance: vec![ac(
                        "I am signed in",
                        "I open Book",
                        "I see this week's free slots",
                    )],
                    priority: Priority::P1,
                    size: Size::M,
                    platforms: vec![Platform::Rn, Platform::Go],
                    depends_on: vec![],
                    assignees: vec!["jane".into()],
                    tasks: vec![
                        Task {
                            id: "t1".into(),
                            title: "Slots API".into(),
                            description: "GET /slots".into(),
                            size: Size::S,
                            priority: None,
                            platforms: vec![Platform::Go],
                            depends_on: vec![],
                            assignees: vec![],
                        },
                        Task {
                            id: "t2".into(),
                            title: "Slot picker screen".into(),
                            description: String::new(),
                            size: Size::M,
                            priority: None,
                            platforms: vec![Platform::Rn],
                            depends_on: vec!["t1".into()],
                            assignees: vec![],
                        },
                    ],
                },
                Story {
                    id: "s2".into(),
                    title: "Confirm a booking".into(),
                    description: "As a patient, I want to confirm a slot.".into(),
                    acceptance: vec![ac(
                        "I picked a slot",
                        "I tap Confirm",
                        "the visit shows in My visits",
                    )],
                    priority: Priority::P0,
                    size: Size::S,
                    platforms: vec![Platform::Rn],
                    depends_on: vec!["s1".into()],
                    assignees: vec![],
                    tasks: vec![],
                },
            ],
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_sample_and_preview_snapshot() {
        let b = sample();
        assert!(b.problems().is_empty(), "{:?}", b.problems());
        insta::assert_snapshot!(b.preview());
    }

    #[test]
    fn catches_bad_plans() {
        let mut b = sample();
        b.epics[0].stories[0].depends_on = vec!["s2".into()]; // s1 ↔ s2
        b.epics[0].stories[1].acceptance.clear();
        b.epics[0].stories[1].tasks.push(Task {
            id: "t1".into(),
            title: "dup".into(),
            description: String::new(),
            size: Size::S,
            priority: None,
            platforms: vec![],
            assignees: vec![],
            depends_on: vec!["ghost".into()],
        });
        let p = b.problems();
        assert!(p.iter().any(|x| x.contains("cycle")), "{p:?}");
        assert!(p.iter().any(|x| x.contains("used twice")), "{p:?}");
        assert!(p.iter().any(|x| x.contains("unknown `ghost`")), "{p:?}");
        assert!(p.iter().any(|x| x.contains("acceptance criteria")), "{p:?}");
        let mut bad_id = sample();
        bad_id.epics[0].id = "e 1".into();
        assert!(bad_id
            .problems()
            .iter()
            .any(|x| x.contains("letters, digits")));
    }

    #[test]
    fn desired_items() {
        let d = sample().to_desired_with(&BTreeMap::new());
        assert_eq!(d.len(), 5);
        let story = d
            .iter()
            .find(|x| x.ehai_id == "online-appointment-booking.s2")
            .unwrap();
        assert_eq!(
            (story.kind, story.priority, story.status),
            (Some(Kind::Story), Some(Priority::P0), Some(Status::Backlog))
        );
        assert!(story
            .body
            .contains("**Given** I picked a slot **When** I tap Confirm"));
        assert!(story.body.contains("Depends on: See free slots"));
        assert!(story.body.contains("Part of: Self-service booking"));
        let task = d.iter().find(|x| x.ehai_id.ends_with(".t1")).unwrap();
        assert_eq!(
            task.priority,
            Some(Priority::P1),
            "inherits the story's priority"
        );
        assert!(d.iter().all(|x| ehai_tracker::valid_ehai_id(&x.ehai_id)));
    }

    #[test]
    fn references_use_issue_numbers_once_known() {
        let mut numbers = BTreeMap::new();
        numbers.insert("online-appointment-booking.s1".to_string(), 18);
        let d = sample().to_desired_with(&numbers);
        let s2 = d.iter().find(|x| x.ehai_id.ends_with(".s2")).unwrap();
        assert!(s2.body.contains("Depends on: #18"), "{}", s2.body);
        let epic = d.iter().find(|x| x.ehai_id.ends_with(".e1")).unwrap();
        assert!(
            epic.body.contains("- [ ] #18") && epic.body.contains("- [ ] Confirm a booking"),
            "{}",
            epic.body
        );

        // renaming s1 changes only s1 once everything has a number
        let all: BTreeMap<String, u64> = sample()
            .to_desired()
            .iter()
            .enumerate()
            .map(|(i, x)| (x.ehai_id.clone(), i as u64 + 10))
            .collect();
        let before = sample().to_desired_with(&all);
        let mut renamed = sample();
        renamed.epics[0].stories[0].title = "See open slots".into();
        let after = renamed.to_desired_with(&all);
        let changed: Vec<_> = before
            .iter()
            .zip(&after)
            .filter(|(a, b)| a != b)
            .map(|(a, _)| a.ehai_id.clone())
            .collect();
        assert_eq!(changed, ["online-appointment-booking.s1"]);
    }

    #[test]
    fn resolve_refs_edge_cases() {
        let none = BTreeMap::new();
        assert_eq!(resolve_refs("a [[ehai-ref:x.y|T]] b", &none), "a T b");
        assert_eq!(
            resolve_refs("broken [[ehai-ref:x", &none),
            "broken [[ehai-ref:x"
        );
        assert_eq!(resolve_refs("plain", &none), "plain");
    }

    #[test]
    fn item_ids_stay_within_limit() {
        let long = "x".repeat(80);
        let id = item_id(&long, "story-12");
        assert!(id.len() <= 64 && id.ends_with(".story-12"));
        assert!(ehai_tracker::valid_ehai_id(&id));
    }

    #[test]
    fn save_and_load() {
        let home = tempfile::tempdir().unwrap();
        sample().save(home.path()).unwrap();
        assert_eq!(
            Breakdown::load(home.path(), "online-appointment-booking").unwrap(),
            sample()
        );
    }
}
