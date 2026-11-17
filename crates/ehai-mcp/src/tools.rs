//! MCP tools. Every tool returns a JSON object with a `status`:
//!
//! - `ok` / `preview` / `invalid` (with `problems` to fix and retry),
//! - `needs_input`: `{question, header, options[{label, description, value}],
//!   multi_select, retry_param}` — the harness asks the user through its question
//!   UI and calls the same tool again with `retry_param` set to the chosen value.
//!   This works in every client; MCP elicitation can be layered on later.
//!
//! Anything written to a tracker, a PRD or a report passes the compliance guard
//! first, and every such write is recorded in the audit log (kinds only).

use std::path::PathBuf;

use ehai_core::config::ComplianceMode;
use ehai_guard::{audit, Guard, Outcome};
use ehai_planner::{breakdown::Breakdown, prd::Prd, report, triage};
use ehai_tracker::github::{self, Client, GithubProjects};
use ehai_tracker::{sync, Filter, Kind, NewItem, Priority, Size, Status, Tracker};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::{tool, tool_router, Json};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::{guard, EhaiServer, Github};

type Out = Result<Json<Map<String, Value>>, String>;

fn out(v: Value) -> Out {
    match v {
        Value::Object(m) => Ok(Json(m)),
        other => Ok(Json(Map::from_iter([("value".to_string(), other)]))),
    }
}

/// How to handle possible patient data the guard found (warn mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PhiChoice {
    Redact,
    Keep,
    Cancel,
}

/// Result of running the guard over a value.
struct Checked<T> {
    value: T,
    kinds: Vec<String>,
    action: &'static str,
    /// Low-confidence findings in strict mode, worth mentioning.
    notes: Vec<String>,
}

enum Guarded<T> {
    Ready(Checked<T>),
    Ask(Value),
}

fn needs_input(header: &str, question: String, options: Vec<Value>, retry_param: &str) -> Value {
    json!({ "status": "needs_input", "header": header, "question": question,
            "options": options, "multi_select": false, "retry_param": retry_param })
}

fn opt(label: &str, value: &str, description: &str) -> Value {
    json!({ "label": label, "value": value, "description": description })
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SavePrdArgs {
    pub prd: Prd,
    /// Repository to also write `docs/prd/<slug>.md` into. Defaults to the
    /// repository the server was started in, if any.
    #[serde(default)]
    pub repo_path: Option<String>,
    /// Answer to a patient-data question from an earlier call.
    #[serde(default)]
    pub phi: Option<PhiChoice>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PlanRef {
    /// PRD slug (see `list_plans`).
    pub prd: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SaveBreakdownArgs {
    pub breakdown: Breakdown,
    #[serde(default)]
    pub phi: Option<PhiChoice>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SyncArgs {
    /// PRD slug whose breakdown to sync.
    pub prd: String,
    /// `false` (default): only show what would change. `true`: write to the tracker.
    #[serde(default)]
    pub apply: bool,
    #[serde(default)]
    pub phi: Option<PhiChoice>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct StatusArgs {
    /// Also write a self-contained HTML report and return its path.
    #[serde(default)]
    pub html: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TriageArgs {
    pub title: String,
    pub description: String,
    pub kind: Kind,
    pub priority: Priority,
    #[serde(default)]
    pub size: Option<Size>,
    /// Answer to the duplicate question: `new`, or `dup:<item id>`.
    #[serde(default)]
    pub decision: Option<String>,
    #[serde(default)]
    pub phi: Option<PhiChoice>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct GuardArgs {
    pub text: String,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct TeamArgs {
    /// Re-check now instead of using the (at most a day old) cache.
    #[serde(default)]
    pub refresh: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum AssignMode {
    #[default]
    Add,
    Replace,
    Remove,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AssignArgs {
    /// `#18`, a URL, an ehai id, or part of the title.
    pub item: String,
    /// Logins, or `"me"` for the caller. Matched against the team (case-insensitive).
    pub assignees: Vec<String>,
    #[serde(default)]
    pub mode: AssignMode,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CommentArgs {
    /// `#18`, a URL, an ehai id, or part of the title.
    pub item: String,
    pub body: String,
    /// Logins, or `"me"`, added as `@login` mentions.
    #[serde(default)]
    pub mentions: Vec<String>,
    #[serde(default)]
    pub phi: Option<PhiChoice>,
}

#[tool_router(router = tool_router, vis = "pub(crate)")]
impl EhaiServer {
    #[tool(
        name = "ping",
        description = "Check that the engine is reachable. Returns its name and version."
    )]
    pub async fn ping(&self) -> Out {
        out(json!({ "ok": true, "name": self.ctx.brand(), "version": ehai_core::VERSION }))
    }

    #[tool(
        name = "save_prd",
        description = "Validate and save a PRD (problem, users, goals, scope, flows as Mermaid, screens, metrics with numbers, risks). Writes it under ~/.ehai/plans and as docs/prd/<slug>.md in the repository. Returns `invalid` with problems to fix, or `needs_input` if possible patient data was found."
    )]
    pub async fn save_prd(&self, Parameters(a): Parameters<SavePrdArgs>) -> Out {
        let this = self.clone();
        guard(async move { this.do_save_prd(a) }).await
    }

    #[tool(
        name = "get_plan",
        description = "Read a saved PRD (as Markdown and structured) and its breakdown, if any."
    )]
    pub async fn get_plan(&self, Parameters(a): Parameters<PlanRef>) -> Out {
        let home = self.ctx.home.clone();
        guard(async move {
            let prd = Prd::load(&home, &a.prd).map_err(|e| e.to_string())?;
            let breakdown = Breakdown::load(&home, &a.prd).ok();
            out(json!({ "status": "ok", "slug": a.prd, "markdown": prd.to_markdown(), "prd": prd,
                        "breakdown": breakdown, "preview": breakdown.as_ref().map(Breakdown::preview) }))
        })
        .await
    }

    #[tool(
        name = "list_plans",
        description = "List saved plans (PRD slug and title), newest first."
    )]
    pub async fn list_plans(&self) -> Out {
        let home = self.ctx.home.clone();
        guard(async move {
            let plans: Vec<Value> = ehai_planner::list_plans(&home)
                .into_iter()
                .filter_map(|slug| {
                    let prd = Prd::load(&home, &slug).ok()?;
                    Some(json!({ "slug": slug, "title": prd.title,
                                 "has_breakdown": Breakdown::load(&home, &slug).is_ok() }))
                })
                .collect();
            out(json!({ "status": "ok", "plans": plans }))
        })
        .await
    }

    #[tool(
        name = "save_breakdown",
        description = "Validate and save a breakdown of a PRD into epics, stories (with Given/When/Then acceptance criteria) and tasks. Returns a board preview (table + Mermaid dependency graph), or `invalid` with problems such as unknown dependencies or cycles."
    )]
    pub async fn save_breakdown(&self, Parameters(a): Parameters<SaveBreakdownArgs>) -> Out {
        let this = self.clone();
        guard(async move { this.do_save_breakdown(a) }).await
    }

    #[tool(
        name = "sync_plan",
        description = "Sync a saved breakdown to the tracker (GitHub Projects) without duplicates. With `apply: false` (default) it only previews what would be created or updated; call again with `apply: true` after the user confirms. Never deletes items and never moves cards people moved."
    )]
    pub async fn sync_plan(&self, Parameters(a): Parameters<SyncArgs>) -> Out {
        let this = self.clone();
        guard(async move { this.do_sync(a).await }).await
    }

    #[tool(
        name = "status_report",
        description = "Progress report from the tracker: percent done per epic, items by status (Mermaid pie), blocked items, stale work and risks. Optionally writes a self-contained HTML report and returns its path."
    )]
    pub async fn status_report(&self, Parameters(a): Parameters<StatusArgs>) -> Out {
        let this = self.clone();
        guard(async move { this.do_status(a).await }).await
    }

    #[tool(
        name = "triage",
        description = "File a classified, prioritised bug or request on the tracker. Checks for similar open items first and returns `needs_input` when it finds some (answer with `decision`: `new` or `dup:<id>`)."
    )]
    pub async fn triage(&self, Parameters(a): Parameters<TriageArgs>) -> Out {
        let this = self.clone();
        guard(async move { this.do_triage(a).await }).await
    }

    #[tool(
        name = "guard_check",
        description = "Check text for patient data (PHI/PII) under the configured compliance mode. Reports the kinds found, never the values."
    )]
    pub async fn guard_check(&self, Parameters(a): Parameters<GuardArgs>) -> Out {
        let this = self.clone();
        guard(async move {
            let g = this.guard()?;
            let findings = g.scan(&a.text);
            out(json!({ "status": "ok", "mode": this.ctx.config.compliance.mode, "clean": findings.is_empty(),
                        "found": ehai_guard::summarize(&findings) }))
        })
        .await
    }

    #[tool(
        name = "team",
        description = "Who can be assigned work and @mentioned on the connected project. Cached up to a day; set `refresh: true` to recheck now."
    )]
    pub async fn team(&self, Parameters(a): Parameters<TeamArgs>) -> Out {
        let this = self.clone();
        guard(async move { this.do_team(a).await }).await
    }

    #[tool(
        name = "assign_task",
        description = "Add, replace or remove assignees on a tracker item. `item` accepts #18, a URL, an ehai id, or part of the title; `assignees` accepts logins or \"me\". Returns `needs_input` if the item or an assignee can't be resolved unambiguously."
    )]
    pub async fn assign_task(&self, Parameters(a): Parameters<AssignArgs>) -> Out {
        let this = self.clone();
        guard(async move { this.do_assign(a).await }).await
    }

    #[tool(
        name = "comment_task",
        description = "Post a comment on a tracker item, optionally @mentioning people. `item` accepts #18, a URL, an ehai id, or part of the title."
    )]
    pub async fn comment_task(&self, Parameters(a): Parameters<CommentArgs>) -> Out {
        let this = self.clone();
        guard(async move { this.do_comment(a).await }).await
    }
}

impl EhaiServer {
    fn guard(&self) -> Result<Guard, String> {
        Guard::new(&self.ctx.config.compliance.patterns).map_err(|e| e.to_string())
    }

    /// Run the guard over every string inside `value`.
    fn check<T: Serialize + DeserializeOwned>(
        &self,
        value: &T,
        phi: Option<PhiChoice>,
        what: &str,
    ) -> Result<Guarded<T>, String> {
        let mode = self.ctx.config.compliance.mode;
        let original = serde_json::to_value(value).map_err(|e| e.to_string())?;
        if mode == ComplianceMode::Off {
            let value = serde_json::from_value(original).map_err(|e| e.to_string())?;
            return Ok(Guarded::Ready(Checked {
                value,
                kinds: vec![],
                action: "unchecked",
                notes: vec![],
            }));
        }
        let g = self.guard()?;
        let mut redacted = original.clone();
        let (mut kinds, mut notes) = (Vec::new(), Vec::new());
        walk(
            &mut redacted,
            &mut |s: &mut String| match g.check(mode, s) {
                Outcome::Clean => {}
                Outcome::Ask { findings, redacted } => {
                    kinds.extend(findings.iter().map(|f| f.kind.clone()));
                    *s = redacted;
                }
                Outcome::Redacted {
                    text,
                    findings,
                    remaining,
                } => {
                    kinds.extend(findings.iter().map(|f| f.kind.clone()));
                    notes.extend(remaining.iter().map(|f| f.kind.clone()));
                    *s = text;
                }
            },
        );
        let (chosen, action) = match (mode, phi) {
            _ if kinds.is_empty() => (original, "clean"),
            (ComplianceMode::Strict, _) => (redacted, "redacted"),
            (_, None) => {
                return Ok(Guarded::Ask(needs_input(
                    "Patient data",
                    format!(
                    "This {what} looks like it contains patient data ({}). How should I handle it?",
                    ehai_guard::summarize_kinds(kinds.iter().map(String::as_str))
                ),
                    vec![
                        opt(
                            "Redact it (Recommended)",
                            "redact",
                            "Replace it with [REDACTED] and continue",
                        ),
                        opt(
                            "Keep it",
                            "keep",
                            "Write it as is (recorded in the audit log)",
                        ),
                        opt("Cancel", "cancel", "Write nothing"),
                    ],
                    "phi",
                )))
            }
            (_, Some(PhiChoice::Redact)) => (redacted, "redacted"),
            (_, Some(PhiChoice::Keep)) => (original, "kept"),
            (_, Some(PhiChoice::Cancel)) => {
                return Err(format!("cancelled: the {what} was not written"))
            }
        };
        let value = serde_json::from_value(chosen).map_err(|e| e.to_string())?;
        Ok(Guarded::Ready(Checked {
            value,
            kinds,
            action,
            notes,
        }))
    }

    fn audit(&self, command: &str, target: &str, location: &str, kinds: &[String], action: &str) {
        let event = audit::Event::with_kinds(command, target, location, kinds.to_vec(), action);
        if let Err(e) = audit::record(&self.ctx.home, &event) {
            tracing::error!(error = %e, "audit log write failed");
        }
    }

    fn repo(&self, explicit: Option<&str>) -> Result<Option<PathBuf>, String> {
        match explicit {
            Some(p) => {
                let p = PathBuf::from(p);
                if !p.is_dir() {
                    return Err(format!("repo_path {} is not a directory", p.display()));
                }
                Ok(Some(p))
            }
            None => Ok(self.ctx.cwd.as_deref().and_then(ehai_core::git::toplevel)),
        }
    }

    fn github_client(&self) -> Result<Client, String> {
        match &self.ctx.github {
            Github::FromEnv => github::client_from_env().map(|(c, _)| c),
            Github::Fixed { api, token } => Client::new(api, token),
        }
        .map_err(|e| e.to_string())
    }

    fn tracker(&self) -> Result<GithubProjects, String> {
        let client = self.github_client()?;
        github::from_config(&self.ctx.config, client).map_err(|e| e.to_string())
    }

    /// The team for the connected project (cached; `refresh` forces a recheck).
    async fn fetch_team(&self, refresh: bool) -> Result<ehai_tracker::team::Team, String> {
        let g = &self.ctx.config.tracker.github;
        let (owner, number) = match (&g.owner, g.project) {
            (Some(o), Some(n)) => (o.clone(), n),
            _ => {
                return Err(format!(
                    "no tracker connected yet: run `{} tracker set`",
                    self.ctx.brand()
                ))
            }
        };
        let client = self.github_client()?;
        ehai_tracker::team::get(
            &client,
            &self.ctx.home,
            &owner,
            number,
            g.repo.as_deref(),
            refresh,
        )
        .await
        .map_err(|e| e.to_string())
    }

    fn do_save_prd(&self, a: SavePrdArgs) -> Out {
        let problems = a.prd.problems();
        if !problems.is_empty() {
            return out(json!({ "status": "invalid", "problems": problems }));
        }
        let checked = match self.check(&a.prd, a.phi, "PRD")? {
            Guarded::Ask(q) => return out(q),
            Guarded::Ready(c) => c,
        };
        let repo = self.repo(a.repo_path.as_deref())?;
        let files = checked
            .value
            .save(&self.ctx.home, repo.as_deref())
            .map_err(|e| e.to_string())?;
        let location = files
            .last()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        self.audit("save_prd", "prd", &location, &checked.kinds, checked.action);
        out(
            json!({ "status": "ok", "slug": checked.value.slug(), "files": files,
                    "markdown": checked.value.to_markdown(), "patient_data": checked.action,
                    "low_confidence_flags": checked.notes }),
        )
    }

    fn do_save_breakdown(&self, a: SaveBreakdownArgs) -> Out {
        let problems = a.breakdown.problems();
        if !problems.is_empty() {
            return out(json!({ "status": "invalid", "problems": problems }));
        }
        let checked = match self.check(&a.breakdown, a.phi, "breakdown")? {
            Guarded::Ask(q) => return out(q),
            Guarded::Ready(c) => c,
        };
        let b = checked.value;
        let path = b.save(&self.ctx.home).map_err(|e| e.to_string())?;
        self.audit(
            "save_breakdown",
            "breakdown",
            &path.display().to_string(),
            &checked.kinds,
            checked.action,
        );
        let prd_saved = Prd::load(&self.ctx.home, &b.prd).is_ok();
        out(
            json!({ "status": "ok", "prd": b.prd, "items": b.rows().len(), "preview": b.preview(),
                    "note": (!prd_saved).then_some("no saved PRD with this slug; save the PRD too so the plan has context"),
                    "patient_data": checked.action }),
        )
    }

    async fn do_sync(&self, a: SyncArgs) -> Out {
        let b = Breakdown::load(&self.ctx.home, &a.prd).map_err(|e| e.to_string())?;
        let t = self.tracker()?;
        let remote = t
            .list_items(&Filter::default())
            .await
            .map_err(|e| e.to_string())?;
        let desired = match self.check(&b.to_desired_with(&numbers(&remote)), a.phi, "plan")? {
            Guarded::Ask(q) => return out(q),
            Guarded::Ready(c) => c,
        };
        let (ops, orphaned) = sync::plan(&desired.value, &remote);
        let title_of = |id: &str| {
            desired
                .value
                .iter()
                .find(|d| d.ehai_id == id)
                .map_or(id.to_string(), |d| d.title.clone())
        };
        if !a.apply {
            let (mut create, mut update, mut unchanged) = (vec![], vec![], 0);
            for op in &ops {
                match op {
                    sync::Op::Create(d) => create.push(d.title.clone()),
                    sync::Op::Update { ehai_id, .. } => update.push(title_of(ehai_id)),
                    sync::Op::Unchanged { .. } => unchanged += 1,
                }
            }
            return out(
                json!({ "status": "preview", "create": create, "update": update,
                               "unchanged": unchanged, "orphaned": orphaned,
                               "nothing_to_do": create.is_empty() && update.is_empty() }),
            );
        }
        // fields must exist before items are written, or every later sync sees
        // "drift" in the fields that couldn't be set (idempotent, cheap)
        let schema = t.ensure_schema().await.map_err(|e| e.to_string())?;
        let mut report = sync::sync(&t, &desired.value)
            .await
            .map_err(|e| e.to_string())?;
        let mut after = t.list_items(&Filter::default()).await.unwrap_or_default();
        if !report.created.is_empty() {
            // second pass: new items now have issue numbers, so cross-references
            // become `#n` links (and a re-sync stays a no-op)
            let resolved: Vec<_> = self
                .check(
                    &b.to_desired_with(&numbers(&after)),
                    Some(a.phi.unwrap_or(PhiChoice::Redact)),
                    "plan",
                )
                .ok()
                .and_then(|g| match g {
                    Guarded::Ready(c) => Some(c.value),
                    Guarded::Ask(_) => None,
                })
                .unwrap_or_default();
            if let Ok(second) = sync::sync(&t, &resolved).await {
                // links filled into just-created items are part of creating them
                let extra: Vec<String> = second
                    .updated
                    .into_iter()
                    .filter(|id| !report.created.contains(id) && !report.updated.contains(id))
                    .collect();
                report.updated.extend(extra);
                report.failed.extend(second.failed);
                after = t.list_items(&Filter::default()).await.unwrap_or_default();
            }
        }
        let location = format!(
            "{} #{}",
            self.ctx
                .config
                .tracker
                .github
                .owner
                .as_deref()
                .unwrap_or("?"),
            self.ctx.config.tracker.github.project.unwrap_or_default()
        );
        self.audit(
            "sync_plan",
            "tracker-items",
            &location,
            &desired.kinds,
            desired.action,
        );
        let link = |id: &String| {
            let item = after.iter().find(|i| i.ehai_id.as_ref() == Some(id));
            json!({ "title": item.map_or_else(|| title_of(id), |i| i.title.clone()), "url": item.and_then(|i| i.url.clone()) })
        };
        out(json!({ "status": "ok",
                    "created": report.created.iter().map(link).collect::<Vec<_>>(),
                    "updated": report.updated.iter().map(link).collect::<Vec<_>>(),
                    "unchanged": report.unchanged.len(),
                    "orphaned": report.orphaned,
                    "schema_notes": schema.warnings,
                    "failed": report.failed.iter().map(|(id, e)| json!({ "title": title_of(id), "error": e })).collect::<Vec<_>>() }))
    }

    async fn do_status(&self, a: StatusArgs) -> Out {
        let t = self.tracker()?;
        let items = t
            .list_items(&Filter::default())
            .await
            .map_err(|e| e.to_string())?;
        // reports leave the machine (shared HTML): titles pass the guard, never asking
        let mode = self.ctx.config.compliance.mode;
        let g = self.guard()?;
        let mut flagged = Vec::new();
        let items: Vec<_> = items
            .into_iter()
            .map(|mut i| {
                match g.check(mode, &i.title) {
                    Outcome::Redacted { text, findings, .. } => {
                        flagged.extend(findings.into_iter().map(|f| f.kind));
                        i.title = text;
                    }
                    Outcome::Ask { findings, .. } => {
                        flagged.extend(findings.into_iter().map(|f| f.kind))
                    }
                    Outcome::Clean => {}
                }
                i
            })
            .collect();
        let plans: Vec<Breakdown> = ehai_planner::list_plans(&self.ctx.home)
            .iter()
            .filter_map(|s| Breakdown::load(&self.ctx.home, s).ok())
            .collect();
        let r = report::build(
            &items,
            &plans,
            &ehai_core::date::today_utc(),
            &ehai_core::date::days_ago(report::STALE_DAYS),
        );
        let mut html_path = None;
        if a.html {
            let g = &self.ctx.config.tracker.github;
            let project = format!(
                "{} #{}",
                g.owner.as_deref().unwrap_or(""),
                g.project.unwrap_or_default()
            );
            let dir = self.ctx.home.join("reports");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let path = dir.join(format!("status-{}.html", r.date));
            ehai_core::migrate::write_atomic(&path, r.to_html(&project).as_bytes())
                .map_err(|e| e.to_string())?;
            self.audit(
                "status_report",
                "report",
                &path.display().to_string(),
                &flagged,
                if mode == ComplianceMode::Strict && !flagged.is_empty() {
                    "redacted"
                } else if flagged.is_empty() {
                    "clean"
                } else {
                    "flagged"
                },
            );
            html_path = Some(path);
        }
        out(
            json!({ "status": "ok", "markdown": r.to_markdown(), "percent_done": r.percent(),
                    "html_path": html_path,
                    "patient_data_in_titles": (!flagged.is_empty()).then(|| format!("{} item title(s) look like they contain patient data; consider editing them", flagged.len())) }),
        )
    }

    async fn do_triage(&self, a: TriageArgs) -> Out {
        #[derive(Serialize, Deserialize)]
        struct Text {
            title: String,
            description: String,
        }
        let checked = match self.check(
            &Text {
                title: a.title.clone(),
                description: a.description.clone(),
            },
            a.phi,
            "report",
        )? {
            Guarded::Ask(q) => return out(q),
            Guarded::Ready(c) => c,
        };
        let Text { title, description } = checked.value;
        let t = self.tracker()?;
        match a.decision.as_deref() {
            None => {
                let items = t
                    .list_items(&Filter::default())
                    .await
                    .map_err(|e| e.to_string())?;
                let dups =
                    triage::duplicates(&title, &description, &items, triage::DEFAULT_THRESHOLD);
                if !dups.is_empty() {
                    let mut options: Vec<Value> = dups
                        .iter()
                        .take(3)
                        .map(|d| {
                            opt(
                                &format!("Same as: {}", d.title),
                                &format!("dup:{}", d.id),
                                d.url.as_deref().unwrap_or("existing item"),
                            )
                        })
                        .collect();
                    options.push(opt("It's new — create it", "new", "File a new item anyway"));
                    return out(needs_input(
                        "Duplicate?",
                        format!(
                            "This looks similar to {} existing item(s). Is it one of them?",
                            dups.len()
                        ),
                        options,
                        "decision",
                    ));
                }
            }
            Some(d) if d.starts_with("dup:") => {
                let id = &d[4..];
                t.comment(
                    id,
                    &format!("Also reported:\n\n**{title}**\n\n{description}"),
                )
                .await
                .map_err(|e| e.to_string())?;
                let item = t.get_item(id).await.map_err(|e| e.to_string())?;
                self.audit(
                    "triage",
                    "tracker-comment",
                    item.url.as_deref().unwrap_or(id),
                    &checked.kinds,
                    checked.action,
                );
                return out(
                    json!({ "status": "ok", "action": "added to existing item", "title": item.title, "url": item.url }),
                );
            }
            Some("new") => {}
            Some(other) => {
                return Err(format!(
                    "unknown decision `{other}`: use `new` or `dup:<id>`"
                ))
            }
        }
        let item = t
            .create_item(NewItem {
                ehai_id: None,
                title,
                body: description,
                status: Some(Status::Todo),
                priority: Some(a.priority),
                size: a.size,
                kind: Some(a.kind),
            })
            .await
            .map_err(|e| e.to_string())?;
        self.audit(
            "triage",
            "tracker-item",
            item.url.as_deref().unwrap_or(&item.id),
            &checked.kinds,
            checked.action,
        );
        out(
            json!({ "status": "ok", "action": "created", "title": item.title, "url": item.url,
                    "kind": a.kind, "priority": a.priority }),
        )
    }

    async fn do_team(&self, a: TeamArgs) -> Out {
        let team = self.fetch_team(a.refresh).await?;
        out(
            json!({ "status": "ok", "viewer": team.viewer, "viewer_is_admin": team.viewer_is_admin,
                    "cache_age_secs": team.age_secs(),
                    "members": team.members.iter().map(|m| json!({ "login": m.login, "name": m.name, "assignable": m.assignable })).collect::<Vec<_>>() }),
        )
    }

    async fn do_assign(&self, a: AssignArgs) -> Out {
        let t = self.tracker()?;
        let items = t
            .list_items(&Filter::default())
            .await
            .map_err(|e| e.to_string())?;
        let item = match resolve_item(&items, &a.item) {
            Ok(i) => i,
            Err(e) => return e,
        };
        let team = self.fetch_team(false).await?;
        let logins = match resolve_assignees(&team, &a.assignees) {
            Ok(v) => v,
            Err(e) => return out(e),
        };
        let unassignable: Vec<&str> = logins
            .iter()
            .filter(|l| team.find(l).is_some_and(|m| !m.assignable))
            .map(String::as_str)
            .collect();
        match a.mode {
            AssignMode::Add => t
                .assign(&item.id, &logins)
                .await
                .map_err(|e| e.to_string())?,
            AssignMode::Remove => t
                .unassign(&item.id, &logins)
                .await
                .map_err(|e| e.to_string())?,
            AssignMode::Replace => {
                t.unassign(&item.id, &item.assignees)
                    .await
                    .map_err(|e| e.to_string())?;
                t.assign(&item.id, &logins)
                    .await
                    .map_err(|e| e.to_string())?;
            }
        }
        let updated = t.get_item(&item.id).await.map_err(|e| e.to_string())?;
        self.audit(
            "assign_task",
            "tracker-item",
            updated.url.as_deref().unwrap_or(&updated.id),
            &[],
            "clean",
        );
        out(
            json!({ "status": "ok", "title": updated.title, "url": updated.url, "assignees": updated.assignees,
                    "warning": (!unassignable.is_empty()).then(|| format!("{} may not be assignable on this repo", unassignable.join(", "))) }),
        )
    }

    async fn do_comment(&self, a: CommentArgs) -> Out {
        let t = self.tracker()?;
        let items = t
            .list_items(&Filter::default())
            .await
            .map_err(|e| e.to_string())?;
        let item = match resolve_item(&items, &a.item) {
            Ok(i) => i,
            Err(e) => return e,
        };
        let mentions = if a.mentions.is_empty() {
            vec![]
        } else {
            let team = self.fetch_team(false).await?;
            match resolve_assignees(&team, &a.mentions) {
                Ok(v) => v,
                Err(e) => return out(e),
            }
        };
        let checked = match self.check(&a.body, a.phi, "comment")? {
            Guarded::Ask(q) => return out(q),
            Guarded::Ready(c) => c,
        };
        let prefix = mentions
            .iter()
            .map(|l| format!("@{l}"))
            .collect::<Vec<_>>()
            .join(" ");
        let body = if prefix.is_empty() {
            checked.value
        } else {
            format!("{prefix}\n\n{}", checked.value)
        };
        t.comment(&item.id, &body)
            .await
            .map_err(|e| e.to_string())?;
        self.audit(
            "comment_task",
            "tracker-comment",
            item.url.as_deref().unwrap_or(&item.id),
            &checked.kinds,
            checked.action,
        );
        out(json!({ "status": "ok", "title": item.title, "url": item.url }))
    }
}

/// Resolve a loose item reference — `#18`, a URL, an ehai id, or a title
/// fragment — against the tracker's current items. `Err` carries the final
/// tool result to return directly: a plain error when nothing matches, or
/// `needs_input` with up to 5 candidates when more than one does.
fn resolve_item<'a>(
    items: &'a [ehai_tracker::Item],
    reference: &str,
) -> Result<&'a ehai_tracker::Item, Out> {
    let r = reference.trim();
    if let Some(n) = r.strip_prefix('#').and_then(|n| n.parse::<u64>().ok()) {
        if let Some(i) = items.iter().find(|i| i.number == Some(n)) {
            return Ok(i);
        }
    }
    if r.contains("://") {
        if let Some(i) = items.iter().find(|i| i.url.as_deref() == Some(r)) {
            return Ok(i);
        }
    }
    if let Some(i) = items.iter().find(|i| i.ehai_id.as_deref() == Some(r)) {
        return Ok(i);
    }
    let needle = r.to_lowercase();
    let matches: Vec<&ehai_tracker::Item> = items
        .iter()
        .filter(|i| i.title.to_lowercase().contains(&needle))
        .collect();
    match matches.as_slice() {
        [one] => Ok(one),
        [] => Err(Err(format!("no tracker item matches `{reference}`"))),
        many => {
            let options = many
                .iter()
                .take(5)
                .map(|i| {
                    opt(
                        &i.title,
                        i.ehai_id.as_deref().unwrap_or(&i.id),
                        i.url.as_deref().unwrap_or(""),
                    )
                })
                .collect();
            Err(out(needs_input(
                "Which item?",
                format!(
                    "`{reference}` matches {} items. Which one did you mean?",
                    many.len()
                ),
                options,
                "item",
            )))
        }
    }
}

/// Resolve names (logins or `"me"`) against the team. `Err` is a
/// `needs_input` value listing the team as options, for names that didn't match.
fn resolve_assignees(
    team: &ehai_tracker::team::Team,
    names: &[String],
) -> Result<Vec<String>, Value> {
    let mut resolved = Vec::new();
    let mut missing = Vec::new();
    for n in names {
        match team.find(n) {
            Some(m) => resolved.push(m.login.clone()),
            None => missing.push(n.clone()),
        }
    }
    if missing.is_empty() {
        return Ok(resolved);
    }
    let options = team
        .members
        .iter()
        .map(|m| opt(&m.login, &m.login, m.name.as_deref().unwrap_or("")))
        .collect();
    Err(needs_input(
        "Who?",
        format!(
            "I don't recognise {} on this project's team. Who did you mean?",
            missing.join(", ")
        ),
        options,
        "assignees",
    ))
}

/// `ehai-id` → issue number for items that are issues.
fn numbers(items: &[ehai_tracker::Item]) -> std::collections::BTreeMap<String, u64> {
    items
        .iter()
        .filter_map(|i| Some((i.ehai_id.clone()?, i.number?)))
        .collect()
}

/// Apply `f` to every string in a JSON value.
fn walk(v: &mut Value, f: &mut dyn FnMut(&mut String)) {
    match v {
        Value::String(s) => f(s),
        Value::Array(a) => a.iter_mut().for_each(|x| walk(x, f)),
        Value::Object(o) => o.values_mut().for_each(|x| walk(x, f)),
        _ => {}
    }
}
