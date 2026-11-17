//! `/ehai:status`: progress per epic, blockers, stale work and risk flags, as
//! Markdown + Mermaid and as a self-contained HTML page (no external assets).

use std::collections::BTreeMap;

use ehai_tracker::{Item, Kind, Priority, Status};
use serde::Serialize;

use crate::breakdown::{item_id, Breakdown};

/// Work in progress untouched this long is flagged as stale.
pub const STALE_DAYS: i64 = 14;

/// More in-progress items than this for one person is flagged as a risk.
pub const OVERLOADED_THRESHOLD: usize = 5;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PersonStats {
    pub open: usize,
    pub in_progress: usize,
    pub done: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EpicProgress {
    pub title: String,
    pub done: usize,
    pub total: usize,
}

impl EpicProgress {
    pub fn percent(&self) -> usize {
        (self.done * 100).checked_div(self.total).unwrap_or(0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Flagged {
    pub title: String,
    pub url: Option<String>,
    pub why: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatusReport {
    pub date: String,
    pub total: usize,
    pub done: usize,
    pub epics: Vec<EpicProgress>,
    pub by_status: BTreeMap<String, usize>,
    pub blocked: Vec<Flagged>,
    pub stale: Vec<Flagged>,
    pub risks: Vec<Flagged>,
    pub by_person: BTreeMap<String, PersonStats>,
    /// Logins with more than [`OVERLOADED_THRESHOLD`] items in progress.
    pub overloaded: Vec<String>,
}

fn is_done(i: &Item) -> bool {
    i.closed || i.status == Some(Status::Done)
}

fn started(i: &Item) -> bool {
    matches!(
        i.status,
        Some(Status::InProgress | Status::InReview | Status::Done)
    ) || i.closed
}

/// Build the report. `plans` give epic grouping and dependencies for items ehai
/// created; other items land under "Other work". `stale_before` is a
/// `YYYY-MM-DD` cutoff (see [`STALE_DAYS`]).
pub fn build(items: &[Item], plans: &[Breakdown], today: &str, stale_before: &str) -> StatusReport {
    // ehai-id → (epic title, dependency ehai-ids)
    let mut meta: BTreeMap<String, (String, Vec<String>)> = BTreeMap::new();
    for plan in plans {
        let rows = plan.rows();
        let parent: BTreeMap<&str, &str> = rows
            .iter()
            .filter_map(|r| Some((r.id.as_str(), r.parent.as_deref()?)))
            .collect();
        let title: BTreeMap<&str, &str> = rows
            .iter()
            .map(|r| (r.id.as_str(), r.title.as_str()))
            .collect();
        for r in &rows {
            let mut epic = r.id.as_str();
            while let Some(p) = parent.get(epic) {
                epic = p;
            }
            let deps = r.depends_on.iter().map(|d| item_id(&plan.prd, d)).collect();
            meta.insert(item_id(&plan.prd, &r.id), (title[epic].to_string(), deps));
        }
    }
    let by_id: BTreeMap<&str, &Item> = items
        .iter()
        .filter_map(|i| Some((i.ehai_id.as_deref()?, i)))
        .collect();
    let work: Vec<&Item> = items
        .iter()
        .filter(|i| i.kind != Some(Kind::Epic))
        .collect();

    let mut epics: BTreeMap<String, EpicProgress> = BTreeMap::new();
    let mut by_status: BTreeMap<String, usize> = BTreeMap::new();
    let mut by_person: BTreeMap<String, PersonStats> = BTreeMap::new();
    let (mut blocked, mut stale, mut risks) = (Vec::new(), Vec::new(), Vec::new());
    for i in &work {
        for login in &i.assignees {
            let p = by_person.entry(login.clone()).or_default();
            if is_done(i) {
                p.done += 1;
            } else if started(i) {
                p.in_progress += 1;
            } else {
                p.open += 1;
            }
        }
        let m = i.ehai_id.as_ref().and_then(|id| meta.get(id));
        let epic = m.map_or("Other work".to_string(), |(e, _)| e.clone());
        let e = epics.entry(epic.clone()).or_insert(EpicProgress {
            title: epic,
            done: 0,
            total: 0,
        });
        e.total += 1;
        e.done += usize::from(is_done(i));
        let status = if i.closed {
            "Done"
        } else {
            i.status.map_or("No status", Status::label)
        };
        *by_status.entry(status.to_string()).or_default() += 1;

        let flag = |why: String| Flagged {
            title: i.title.clone(),
            url: i.url.clone(),
            why,
        };
        if !is_done(i) {
            let waiting: Vec<&str> = m
                .map(|(_, deps)| deps.as_slice())
                .unwrap_or_default()
                .iter()
                .filter_map(|d| by_id.get(d.as_str()))
                .filter(|d| !is_done(d))
                .map(|d| d.title.as_str())
                .collect();
            if !waiting.is_empty() && started(i) {
                blocked.push(flag(format!("waiting on {}", waiting.join(", "))));
            }
            if let Some(u) = i.updated_at.as_deref().filter(|u| *u < stale_before) {
                if matches!(i.status, Some(Status::InProgress | Status::InReview)) {
                    stale.push(flag(format!(
                        "no change since {}",
                        u.get(..10).unwrap_or(u)
                    )));
                }
            }
            if i.priority == Some(Priority::P0) && !started(i) {
                risks.push(flag("P0 not started".into()));
            }
            if matches!(i.status, Some(Status::InProgress | Status::InReview))
                && i.assignees.is_empty()
            {
                risks.push(flag("no one is assigned".into()));
            }
        }
    }
    let overloaded: Vec<String> = by_person
        .iter()
        .filter(|(_, p)| p.in_progress > OVERLOADED_THRESHOLD)
        .map(|(login, p)| format!("{login} ({} in progress)", p.in_progress))
        .collect();
    let done = work.iter().filter(|i| is_done(i)).count();
    StatusReport {
        date: today.to_string(),
        total: work.len(),
        done,
        epics: epics.into_values().collect(),
        by_status,
        blocked,
        stale,
        risks,
        by_person,
        overloaded,
    }
}

fn bar(pct: usize) -> String {
    let filled = pct / 10;
    format!("{}{}", "█".repeat(filled), "░".repeat(10 - filled))
}

fn link(f: &Flagged) -> String {
    match &f.url {
        Some(u) => format!("[{}]({u})", crate::prd::cell(&f.title)),
        None => crate::prd::cell(&f.title),
    }
}

impl StatusReport {
    pub fn percent(&self) -> usize {
        (self.done * 100).checked_div(self.total).unwrap_or(0)
    }

    pub fn to_markdown(&self) -> String {
        let mut md = format!(
            "# Status — {}\n\n**{}% done** · {} of {} · {} blocked · {} stale · {}\n\n",
            self.date,
            self.percent(),
            self.done,
            crate::count(self.total, "item", "items"),
            self.blocked.len(),
            self.stale.len(),
            crate::count(self.risks.len(), "risk", "risks"),
        );
        md += "| Epic | Progress | Done |\n|---|---|---|\n";
        for e in &self.epics {
            md += &format!(
                "| {} | `{}` {}% | {}/{} |\n",
                crate::prd::cell(&e.title),
                bar(e.percent()),
                e.percent(),
                e.done,
                e.total
            );
        }
        if !self.by_status.is_empty() {
            md += "\n```mermaid\npie title Items by status\n";
            for (s, n) in &self.by_status {
                md += &format!("  \"{s}\" : {n}\n");
            }
            md += "```\n";
        }
        for (title, list) in [
            ("Blocked", &self.blocked),
            ("Stale", &self.stale),
            ("Risks", &self.risks),
        ] {
            if !list.is_empty() {
                md += &format!("\n## {title}\n\n");
                for f in list {
                    md += &format!("- {} — {}\n", link(f), f.why);
                }
            }
        }
        if !self.by_person.is_empty() {
            md += "\n## By person\n\n| Person | Open | In progress | Done |\n|---|---|---|---|\n";
            for (login, p) in &self.by_person {
                md += &format!(
                    "| {login} | {} | {} | {} |\n",
                    p.open, p.in_progress, p.done
                );
            }
        }
        if !self.overloaded.is_empty() {
            md += &format!("\nOverloaded: {}\n", self.overloaded.join(", "));
        }
        md
    }

    /// Self-contained HTML (inline CSS, light and dark), safe to open offline.
    pub fn to_html(&self, project: &str) -> String {
        let mut rows = String::new();
        for e in &self.epics {
            rows += &format!(
                "<tr><td>{}</td><td><div class=bar><span style=\"width:{}%\"></span></div></td><td>{}%</td><td>{}/{}</td></tr>",
                esc(&e.title), e.percent(), e.percent(), e.done, e.total
            );
        }
        let mut lists = String::new();
        for (title, list) in [
            ("Blocked", &self.blocked),
            ("Stale", &self.stale),
            ("Risks", &self.risks),
        ] {
            if list.is_empty() {
                continue;
            }
            lists += &format!("<h2>{title}</h2><ul>");
            for f in list {
                let t = match &f.url {
                    Some(u) if u.starts_with("https://") => {
                        format!("<a href=\"{}\">{}</a>", esc(u), esc(&f.title))
                    }
                    _ => esc(&f.title),
                };
                lists += &format!("<li>{t} — {}</li>", esc(&f.why));
            }
            lists += "</ul>";
        }
        let chips: String = self
            .by_status
            .iter()
            .map(|(s, n)| format!("<span class=chip>{} · {n}</span>", esc(s)))
            .collect();
        let mut person_rows = String::new();
        for (login, p) in &self.by_person {
            person_rows += &format!(
                "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                esc(login),
                p.open,
                p.in_progress,
                p.done
            );
        }
        if !person_rows.is_empty() {
            lists += &format!(
                "<h2>By person</h2><table><tr><th>Person</th><th>Open</th><th>In progress</th><th>Done</th></tr>{person_rows}</table>"
            );
        }
        if !self.overloaded.is_empty() {
            lists += &format!(
                "<h2>Overloaded</h2><p>{}</p>",
                esc(&self.overloaded.join(", "))
            );
        }
        format!(
            r#"<!doctype html><html lang=en><head><meta charset=utf-8><meta name=viewport content="width=device-width,initial-scale=1">
<title>Status — {project}</title><style>
:root{{--bg:#fff;--fg:#1b1f24;--muted:#59636e;--line:#d8dee4;--accent:#1f6feb;--track:#eaeef2}}
@media (prefers-color-scheme:dark){{:root{{--bg:#0d1117;--fg:#e6edf3;--muted:#9198a1;--line:#30363d;--accent:#4493f8;--track:#21262d}}}}
body{{background:var(--bg);color:var(--fg);font:15px/1.5 system-ui,sans-serif;max-width:860px;margin:0 auto;padding:24px 16px}}
h1{{margin:0 0 4px;font-size:22px}}.sub{{color:var(--muted);margin-bottom:20px}}
.big{{font-size:40px;font-weight:700}}table{{width:100%;border-collapse:collapse;margin:16px 0}}
td,th{{padding:8px 6px;border-bottom:1px solid var(--line);text-align:left}}
.bar{{background:var(--track);border-radius:6px;height:10px;min-width:120px}}.bar span{{display:block;height:100%;background:var(--accent);border-radius:6px}}
.chip{{display:inline-block;border:1px solid var(--line);border-radius:999px;padding:2px 10px;margin:0 6px 6px 0;color:var(--muted)}}
a{{color:var(--accent)}}h2{{font-size:17px;margin-top:24px}}</style></head><body>
<h1>{project}</h1><div class=sub>Status on {date}</div>
<div class=big>{pct}% done</div><div class=sub>{done} of {total} · {b} blocked · {s} stale · {r}</div>
<div>{chips}</div>
<table><tr><th>Epic</th><th>Progress</th><th></th><th>Done</th></tr>{rows}</table>{lists}
</body></html>"#,
            project = esc(project),
            date = esc(&self.date),
            pct = self.percent(),
            done = self.done,
            total = crate::count(self.total, "item", "items"),
            b = self.blocked.len(),
            s = self.stale.len(),
            r = crate::count(self.risks.len(), "risk", "risks"),
        )
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::breakdown::sample;

    fn item(
        id: &str,
        title: &str,
        kind: Kind,
        status: Status,
        prio: Priority,
        updated: &str,
        assignees: &[&str],
    ) -> Item {
        Item {
            id: id.into(),
            number: None,
            url: Some(format!("https://github.com/o/r/issues/{id}")),
            title: title.into(),
            body: String::new(),
            ehai_id: Some(format!("online-appointment-booking.{id}")),
            status: Some(status),
            priority: Some(prio),
            size: None,
            kind: Some(kind),
            closed: false,
            updated_at: Some(format!("{updated}T10:00:00Z")),
            assignees: assignees.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn items() -> Vec<Item> {
        vec![
            item(
                "e1",
                "Self-service booking",
                Kind::Epic,
                Status::Backlog,
                Priority::P1,
                "2026-11-01",
                &[],
            ),
            item(
                "s1",
                "See free slots",
                Kind::Story,
                Status::InProgress,
                Priority::P1,
                "2026-10-01",
                &["jane"],
            ),
            item(
                "t1",
                "Slots API",
                Kind::Task,
                Status::Done,
                Priority::P1,
                "2026-11-10",
                &["jane"],
            ),
            item(
                "t2",
                "Slot picker screen",
                Kind::Task,
                Status::InProgress,
                Priority::P1,
                "2026-11-15",
                &["jane"],
            ),
            item(
                "s2",
                "Confirm a booking",
                Kind::Story,
                Status::Todo,
                Priority::P0,
                "2026-11-15",
                &["bob"],
            ),
            Item {
                ehai_id: None,
                title: "Hand-made bug".into(),
                ..item(
                    "x",
                    "",
                    Kind::Bug,
                    Status::Done,
                    Priority::P2,
                    "2026-11-15",
                    &[],
                )
            },
        ]
    }

    #[test]
    fn report_numbers_and_flags() {
        let r = build(&items(), &[sample()], "2026-11-17", "2026-11-03");
        assert_eq!(
            (r.total, r.done, r.percent()),
            (5, 2, 40),
            "epics themselves don't count"
        );
        let epic = r
            .epics
            .iter()
            .find(|e| e.title == "Self-service booking")
            .unwrap();
        assert_eq!((epic.done, epic.total), (1, 4));
        assert_eq!(
            r.epics
                .iter()
                .find(|e| e.title == "Other work")
                .unwrap()
                .total,
            1
        );
        assert_eq!(
            r.stale.iter().map(|f| f.title.as_str()).collect::<Vec<_>>(),
            ["See free slots"]
        );
        assert!(r.blocked.is_empty(), "t2 depends on t1, which is done");
        assert_eq!(r.risks.len(), 1, "{:?}", r.risks);
        assert_eq!(r.risks[0].why, "P0 not started");
        assert_eq!(
            r.by_person.get("jane"),
            Some(&PersonStats {
                open: 0,
                in_progress: 2,
                done: 1
            })
        );
        assert_eq!(
            r.by_person.get("bob"),
            Some(&PersonStats {
                open: 1,
                in_progress: 0,
                done: 0
            })
        );
        assert!(r.overloaded.is_empty());

        // s2 starts while s1 is unfinished → blocked
        let mut its = items();
        its[4].status = Some(Status::InProgress);
        let r = build(&its, &[sample()], "2026-11-17", "2026-11-03");
        assert_eq!(r.blocked[0].why, "waiting on See free slots");
    }

    #[test]
    fn unassigned_in_progress_is_a_risk_and_overload_is_flagged() {
        let mut its = items();
        its[1].assignees.clear(); // s1: In Progress, now nobody's on it
        let r = build(&its, &[sample()], "2026-11-17", "2026-11-03");
        assert!(
            r.risks
                .iter()
                .any(|f| f.title == "See free slots" && f.why == "no one is assigned"),
            "{:?}",
            r.risks
        );

        // jane picks up 6 more in-progress tasks: now overloaded
        let mut its = items();
        for i in 0..6 {
            its.push(item(
                &format!("extra{i}"),
                "Extra task",
                Kind::Task,
                Status::InProgress,
                Priority::P2,
                "2026-11-15",
                &["jane"],
            ));
        }
        let r = build(&its, &[sample()], "2026-11-17", "2026-11-03");
        assert_eq!(
            r.overloaded,
            vec!["jane (8 in progress)".to_string()],
            "{:?}",
            r.by_person
        );
    }

    #[test]
    fn markdown_snapshot_and_html_is_escaped() {
        let r = build(&items(), &[sample()], "2026-11-17", "2026-11-03");
        insta::assert_snapshot!(r.to_markdown());
        let mut evil = items();
        evil[1].title = "<script>alert(1)</script>".into();
        evil[1].url = Some("javascript:alert(1)".into());
        let html = build(&evil, &[sample()], "2026-11-17", "2026-11-03").to_html("Clinic <app>");
        assert!(
            !html.contains("<script>alert") && !html.contains("javascript:"),
            "escaped"
        );
        assert!(html.contains("Clinic &lt;app&gt;") && html.contains("prefers-color-scheme:dark"));
    }

    #[test]
    fn empty_board() {
        let r = build(&[], &[], "2026-11-17", "2026-11-03");
        assert_eq!(r.percent(), 0);
        assert!(r.to_markdown().contains("0% done"));
    }
}
