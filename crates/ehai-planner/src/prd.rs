//! Product requirements document.

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{plan_dir, read_json, write_json, Error, Result};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Prd {
    pub title: String,
    /// The problem in the user's words.
    pub problem: String,
    /// Who it's for (personas / roles).
    pub users: Vec<String>,
    pub goals: Vec<String>,
    pub scope: Vec<String>,
    #[serde(default)]
    pub out_of_scope: Vec<String>,
    /// User flows as Mermaid diagrams.
    #[serde(default)]
    pub flows: Vec<Flow>,
    /// Plain-language screen sketches.
    #[serde(default)]
    pub wireframes: Vec<Wireframe>,
    pub metrics: Vec<Metric>,
    #[serde(default)]
    pub risks: Vec<Risk>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Flow {
    pub name: String,
    /// Mermaid source, e.g. `flowchart TD ...`.
    pub mermaid: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Wireframe {
    pub screen: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Metric {
    pub name: String,
    /// Measurable target, e.g. `≥ 60% of bookings online within 3 months`.
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Risk {
    pub risk: String,
    pub mitigation: String,
}

const MERMAID_STARTS: &[&str] = &[
    "flowchart",
    "graph",
    "sequenceDiagram",
    "stateDiagram",
    "journey",
    "classDiagram",
    "erDiagram",
];

impl Prd {
    pub fn slug(&self) -> String {
        crate::slug(&self.title)
    }

    /// Problems a reviewer would bounce the PRD for.
    pub fn problems(&self) -> Vec<String> {
        let mut p = Vec::new();
        if self.title.trim().is_empty() {
            p.push("title is empty".into());
        }
        if self.problem.trim().is_empty() {
            p.push("problem statement is empty".into());
        }
        for (field, list) in [
            ("users", &self.users),
            ("goals", &self.goals),
            ("scope", &self.scope),
        ] {
            if list.iter().all(|s| s.trim().is_empty()) {
                p.push(format!("{field} is empty"));
            }
        }
        if self.metrics.is_empty() {
            p.push("add at least one success metric".into());
        }
        for m in &self.metrics {
            if !m.target.chars().any(|c| c.is_ascii_digit()) {
                p.push(format!(
                    "metric `{}` needs a measurable target (a number)",
                    m.name
                ));
            }
        }
        for f in &self.flows {
            let first = f.mermaid.split_whitespace().next().unwrap_or_default();
            if !MERMAID_STARTS.contains(&first) {
                p.push(format!(
                    "flow `{}` is not a Mermaid diagram (starts with `{first}`)",
                    f.name
                ));
            }
        }
        p
    }

    pub fn to_markdown(&self) -> String {
        let list = |items: &[String]| items.iter().map(|i| format!("- {i}\n")).collect::<String>();
        let mut md = format!(
            "# {}\n\n## Problem\n\n{}\n\n",
            self.title,
            self.problem.trim()
        );
        md += &format!(
            "## Users\n\n{}\n## Goals\n\n{}\n",
            list(&self.users),
            list(&self.goals)
        );
        md += &format!("## Scope\n\n{}\n", list(&self.scope));
        if !self.out_of_scope.is_empty() {
            md += &format!("## Out of scope\n\n{}\n", list(&self.out_of_scope));
        }
        if !self.flows.is_empty() {
            md += "## User flows\n\n";
            for f in &self.flows {
                md += &format!(
                    "### {}\n\n```mermaid\n{}\n```\n\n",
                    f.name,
                    f.mermaid.trim()
                );
            }
        }
        if !self.wireframes.is_empty() {
            md += "## Screens\n\n";
            for w in &self.wireframes {
                md += &format!("### {}\n\n{}\n\n", w.screen, w.description.trim());
            }
        }
        md += "## Success metrics\n\n| Metric | Target |\n|---|---|\n";
        for m in &self.metrics {
            md += &format!("| {} | {} |\n", cell(&m.name), cell(&m.target));
        }
        if !self.risks.is_empty() {
            md += "\n## Risks\n\n| Risk | Mitigation |\n|---|---|\n";
            for r in &self.risks {
                md += &format!("| {} | {} |\n", cell(&r.risk), cell(&r.mitigation));
            }
        }
        md
    }

    /// Validate, then save as `prd.json` + `prd.md` in the plan dir, and as
    /// `docs/prd/<slug>.md` in `repo` when given. Returns the files written.
    pub fn save(&self, home: &Path, repo: Option<&Path>) -> Result<Vec<PathBuf>> {
        let problems = self.problems();
        if !problems.is_empty() {
            return Err(Error::Invalid(problems));
        }
        let slug = self.slug();
        let dir = plan_dir(home, &slug);
        let md = self.to_markdown();
        write_json(&dir.join("prd.json"), self)?;
        ehai_core::migrate::write_atomic(&dir.join("prd.md"), md.as_bytes())?;
        let mut written = vec![dir.join("prd.md")];
        if let Some(repo) = repo {
            let p = repo.join("docs/prd").join(format!("{slug}.md"));
            std::fs::create_dir_all(p.parent().expect("has parent"))
                .map_err(ehai_core::Error::io(&p))?;
            ehai_core::migrate::write_atomic(&p, md.as_bytes())?;
            written.push(p);
        }
        Ok(written)
    }

    pub fn load(home: &Path, slug: &str) -> Result<Self> {
        read_json(&plan_dir(home, slug).join("prd.json"), slug)
    }
}

/// Keep table cells on one line.
pub(crate) fn cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

#[cfg(test)]
pub(crate) fn sample() -> Prd {
    Prd {
        title: "Online appointment booking".into(),
        problem: "Patients can only book by phone, so lines are busy and slots go unused.".into(),
        users: vec!["Patients".into(), "Front-desk staff".into()],
        goals: vec!["Patients book without calling".into()],
        scope: vec!["Book, reschedule and cancel in the app".into()],
        out_of_scope: vec!["Payments".into()],
        flows: vec![Flow {
            name: "Book a visit".into(),
            mermaid: "flowchart TD\n  A[Open app] --> B[Pick slot] --> C[Confirm]".into(),
        }],
        wireframes: vec![Wireframe {
            screen: "Slot picker".into(),
            description: "A week view with free slots.".into(),
        }],
        metrics: vec![Metric {
            name: "Online bookings".into(),
            target: "≥ 60% within 3 months".into(),
        }],
        risks: vec![Risk {
            risk: "No-shows rise".into(),
            mitigation: "Reminders 24 h before".into(),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_snapshot() {
        insta::assert_snapshot!(sample().to_markdown());
    }

    #[test]
    fn problems_are_specific() {
        let mut p = sample();
        p.metrics[0].target = "more".into();
        p.flows[0].mermaid = "A -> B".into();
        p.goals.clear();
        let probs = p.problems();
        assert_eq!(probs.len(), 3, "{probs:?}");
        assert!(probs.iter().any(|x| x.contains("measurable")));
        assert!(probs.iter().any(|x| x.contains("Mermaid")));
    }

    #[test]
    fn save_and_load() {
        let home = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let files = sample().save(home.path(), Some(repo.path())).unwrap();
        assert_eq!(files.len(), 2);
        assert!(repo
            .path()
            .join("docs/prd/online-appointment-booking.md")
            .is_file());
        assert_eq!(
            Prd::load(home.path(), "online-appointment-booking").unwrap(),
            sample()
        );
        assert!(matches!(
            Prd::load(home.path(), "nope"),
            Err(Error::NotFound(_))
        ));
        let bad = Prd {
            title: "x".into(),
            ..Default::default()
        };
        assert!(matches!(
            bad.save(home.path(), None),
            Err(Error::Invalid(_))
        ));
    }
}
