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
use ehai_planner::{breakdown::Breakdown, explore, prd::Prd, report, triage};
use ehai_tracker::any::AnyTracker;
use ehai_tracker::github::{self, Client};
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

/// How to handle possible sensitive data the guard found (warn mode).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SensitiveDataChoice {
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
    /// Answer to a sensitive-data question from an earlier call.
    #[serde(default)]
    pub policy: Option<SensitiveDataChoice>,
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
    pub policy: Option<SensitiveDataChoice>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct SyncArgs {
    /// PRD slug whose breakdown to sync.
    pub prd: String,
    /// `false` (default): only show what would change. `true`: write to the tracker.
    #[serde(default)]
    pub apply: bool,
    #[serde(default)]
    pub policy: Option<SensitiveDataChoice>,
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
    pub policy: Option<SensitiveDataChoice>,
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
    pub policy: Option<SensitiveDataChoice>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct EnvSyncArgs {
    /// Worktree to provision; defaults to the current directory.
    #[serde(default)]
    pub repo_path: Option<String>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct EnvCheckArgs {
    #[serde(default)]
    pub repo_path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PocNewArgs {
    /// Short feature name, e.g. "login-screen". Slugified automatically.
    pub feature: String,
    /// What to build. Passed verbatim into the POC context for Claude to use.
    pub prompt: String,
    /// Repo root; defaults to current directory.
    #[serde(default)]
    pub repo_path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PocIterateArgs {
    pub feature: String,
    /// The poc number to iterate on (e.g. 1 → produces poc-2).
    pub parent_n: u32,
    /// Feedback / change request for this iteration.
    pub feedback: String,
    #[serde(default)]
    pub repo_path: Option<String>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct PocListArgs {
    /// Filter by feature; omit to list all.
    #[serde(default)]
    pub feature: Option<String>,
    #[serde(default)]
    pub repo_path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PocDiscardArgs {
    pub feature: String,
    pub n: u32,
    #[serde(default)]
    pub repo_path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PocCompareArgs {
    pub feature: String,
    /// POC numbers to compare side-by-side.
    pub ns: Vec<u32>,
    #[serde(default)]
    pub repo_path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PocOpenArgs {
    pub feature: String,
    pub n: u32,
    #[serde(default)]
    pub repo_path: Option<String>,
}

/// One source for a compose operation: POC number + what to take from it.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct PocSourcePart {
    pub n: u32,
    pub description: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PocComposeArgs {
    pub feature: String,
    /// What to synthesise (passed to Claude as the design brief).
    pub description: String,
    /// Source POCs and what to take from each.
    pub sources: Vec<PocSourcePart>,
    #[serde(default)]
    pub repo_path: Option<String>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct PocPromoteArgs {
    pub feature: String,
    pub n: u32,
    /// Pass `"yes"` after the user confirms the confirmation question.
    #[serde(default)]
    pub confirmed: Option<String>,
    #[serde(default)]
    pub repo_path: Option<String>,
}

/// One answer supplied by the AI during the plan_explore refinement loop.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExploreAnswer {
    /// Which PRD section this answer addresses: `"flows"` | `"metrics"` | `"risks"` | `"scope"` | `"users"`.
    pub section: String,
    /// 0-based index of the item within the section's array (ignored for sections
    /// that don't address individual array items, e.g. `scope` or `users`).
    #[serde(default)]
    pub index: usize,
    /// Field name within the item to update: `"target"` | `"mitigation"` | `"mermaid"` etc.
    /// For whole-list additions (`scope`, `users`, `flows`) use `"add"`.
    pub field: String,
    /// New value.
    pub value: String,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct PlanExploreArgs {
    /// PRD slug — the same slug returned by `save_prd`.
    pub prd: String,
    /// Answers from the user. Leave empty to retrieve gaps without updating.
    #[serde(default)]
    pub answers: Vec<ExploreAnswer>,
    /// How to handle sensitive data if found in an answer.
    #[serde(default)]
    pub policy: Option<SensitiveDataChoice>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct PrefsListArgs {
    #[serde(default)]
    pub repo_path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PrefsSetArgs {
    pub key: String,
    pub value: String,
    /// `"global"` (default) or `"repo"`.
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub repo_path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PrefsDeleteArgs {
    pub key: String,
    /// `"global"` (default) or `"repo"`.
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub repo_path: Option<String>,
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
        description = "Validate and save a PRD (problem, users, goals, scope, flows as Mermaid, screens, metrics with numbers, risks). Writes it under ~/.ehai/plans and as docs/prd/<slug>.md in the repository. Returns `invalid` with problems to fix, or `needs_input` if possible sensitive data was found."
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
        description = "Check text for sensitive data (PII) under the configured compliance profile and mode. Reports the kinds found, never the values."
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

    #[tool(
        name = "env_sync",
        description = "Make a worktree runnable: copy its gitignored config (.env*, local config, etc.), allocate free ports, install dependencies, and run the configured health check. Defaults to the current directory."
    )]
    pub async fn env_sync(&self, Parameters(a): Parameters<EnvSyncArgs>) -> Out {
        let this = self.clone();
        guard(async move { this.do_env_sync(a) }).await
    }

    #[tool(
        name = "env_check",
        description = "Compare the worktree's .env* files against .env.example/.env.sample and report which keys are missing. Never returns values, only key names."
    )]
    pub async fn env_check(&self, Parameters(a): Parameters<EnvCheckArgs>) -> Out {
        let this = self.clone();
        guard(async move { this.do_env_check(a) }).await
    }

    // ── POC tools (dev + both only) ──────────────────────────────────────

    #[tool(
        name = "poc_new",
        description = "Create a new UI/UX proof-of-concept in an isolated git worktree. Returns the design profile (colors, spacing, typography, component inventory, nav patterns) extracted from the source codebase, the worktree path, the branch name, and the allocated port. Use the returned design_context to write the POC code inside `worktree_path`."
    )]
    pub async fn poc_new(&self, Parameters(a): Parameters<PocNewArgs>) -> Out {
        let this = self.clone();
        guard(async move { this.do_poc_new(a) }).await
    }

    #[tool(
        name = "poc_iterate",
        description = "Iterate on an existing POC. Creates a new worktree branched from the same base, records parent lineage, and returns the same design context as poc_new. Write updated code into the new `worktree_path`."
    )]
    pub async fn poc_iterate(&self, Parameters(a): Parameters<PocIterateArgs>) -> Out {
        let this = self.clone();
        guard(async move { this.do_poc_iterate(a) }).await
    }

    #[tool(
        name = "poc_list",
        description = "List POCs for a feature or all features, with status, lineage, and screenshots."
    )]
    pub async fn poc_list(&self, Parameters(a): Parameters<PocListArgs>) -> Out {
        let this = self.clone();
        guard(async move { this.do_poc_list(a) }).await
    }

    #[tool(
        name = "poc_discard",
        description = "Archive a POC (creates ehai-archive/<feature>/poc-<n> branch, removes worktree). Nothing is permanently deleted."
    )]
    pub async fn poc_discard(&self, Parameters(a): Parameters<PocDiscardArgs>) -> Out {
        let this = self.clone();
        guard(async move { this.do_poc_discard(a) }).await
    }

    #[tool(
        name = "poc_compare",
        description = "Generate a side-by-side HTML comparison report for the listed POC numbers of a feature. Returns the report path."
    )]
    pub async fn poc_compare(&self, Parameters(a): Parameters<PocCompareArgs>) -> Out {
        let this = self.clone();
        guard(async move { this.do_poc_compare(a) }).await
    }

    #[tool(
        name = "poc_open",
        description = "Return the worktree path, dev-server command, port, and URL for a POC so the dev server can be started. Does NOT start it automatically — run the returned `command` inside `worktree_path`."
    )]
    pub async fn poc_open(&self, Parameters(a): Parameters<PocOpenArgs>) -> Out {
        let this = self.clone();
        guard(async move { this.do_poc_open(a) }).await
    }

    #[tool(
        name = "poc_compose",
        description = "Synthesise a new POC by taking described parts from two or more existing POCs. Returns the same design context as poc_new. Claude reads the referenced POCs' branches and writes a synthesised version; ehai records lineage."
    )]
    pub async fn poc_compose(&self, Parameters(a): Parameters<PocComposeArgs>) -> Out {
        let this = self.clone();
        guard(async move { this.do_poc_compose(a) }).await
    }

    #[tool(
        name = "poc_promote",
        description = "Promote a POC to the default branch by opening a GitHub pull request. Returns `needs_input` with a confirmation question on the first call; pass `confirmed: \"yes\"` to proceed."
    )]
    pub async fn poc_promote(&self, Parameters(a): Parameters<PocPromoteArgs>) -> Out {
        let this = self.clone();
        guard(async move { this.do_poc_promote(a).await }).await
    }

    #[tool(
        name = "plan_explore",
        description = "Lite planning loop: find gaps in a saved PRD (missing flows, unmeasured metrics, unmitigated risks, empty out-of-scope, single persona) and optionally apply answers from the user. Always auto-run after save_prd; call with empty `answers` to get gaps, then call again with answers to apply them. Returns gaps_remaining; stop when it is 0 or after 3 rounds."
    )]
    pub async fn plan_explore(&self, Parameters(a): Parameters<PlanExploreArgs>) -> Out {
        let this = self.clone();
        guard(async move { this.do_plan_explore(a) }).await
    }

    #[tool(
        name = "prefs_list",
        description = "List all user preferences (global merged with repo). Returns scope and source for each entry."
    )]
    pub async fn prefs_list(&self, Parameters(a): Parameters<PrefsListArgs>) -> Out {
        let this = self.clone();
        guard(async move { this.do_prefs_list(a) }).await
    }

    #[tool(
        name = "prefs_set",
        description = "Set a user preference. `scope` is `global` (default) or `repo` (requires a repo). Source is always `explicit`."
    )]
    pub async fn prefs_set(&self, Parameters(a): Parameters<PrefsSetArgs>) -> Out {
        let this = self.clone();
        guard(async move { this.do_prefs_set(a) }).await
    }

    #[tool(
        name = "prefs_delete",
        description = "Delete a user preference by key. `scope` controls which file is updated."
    )]
    pub async fn prefs_delete(&self, Parameters(a): Parameters<PrefsDeleteArgs>) -> Out {
        let this = self.clone();
        guard(async move { this.do_prefs_delete(a) }).await
    }
}

impl EhaiServer {
    /// Record one observed signal into the global prefs store. Fire-and-forget:
    /// errors are silently ignored so a prefs write failure never blocks a tool.
    /// Never write PHI — only behavior patterns like `"requirements_completeness"`.
    fn observe_interaction(&self, key: &str, value: &str) {
        let mut prefs = ehai_prefs::Prefs::load_global(&self.ctx.home);
        prefs.set(
            key,
            value,
            ehai_prefs::Scope::Global,
            ehai_prefs::Source::Observed,
        );
        // compact when too many observed signals accumulate
        if prefs.entries.len() > 50 {
            let (mut explicit, mut observed): (Vec<_>, Vec<_>) = prefs
                .entries
                .into_iter()
                .partition(|p| p.source == ehai_prefs::Source::Explicit);
            observed.sort_by(|a, b| a.key.cmp(&b.key));
            observed.truncate(30);
            explicit.extend(observed);
            prefs.entries = explicit;
        }
        let _ = prefs.save_global(&self.ctx.home);
    }

    fn guard(&self) -> Result<Guard, String> {
        Guard::new(
            self.ctx.config.compliance.profile,
            &self.ctx.config.compliance.patterns,
        )
        .map_err(|e| e.to_string())
    }

    /// Run the guard over every string inside `value`.
    fn check<T: Serialize + DeserializeOwned>(
        &self,
        value: &T,
        policy: Option<SensitiveDataChoice>,
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
        let (chosen, action) = match (mode, policy) {
            _ if kinds.is_empty() => (original, "clean"),
            (ComplianceMode::Strict, _) => (redacted, "redacted"),
            (_, None) => {
                return Ok(Guarded::Ask(needs_input(
                    "Sensitive data",
                    format!(
                    "This {what} looks like it contains sensitive data ({}). How should I handle it?",
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
                    "policy",
                )))
            }
            (_, Some(SensitiveDataChoice::Redact)) => (redacted, "redacted"),
            (_, Some(SensitiveDataChoice::Keep)) => (original, "kept"),
            (_, Some(SensitiveDataChoice::Cancel)) => {
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

    fn tracker(&self) -> Result<AnyTracker, String> {
        use ehai_core::config::TrackerKind;
        match self.ctx.config.tracker.kind {
            TrackerKind::Local => {
                let t = ehai_tracker::local::LocalTracker::new(&self.ctx.home);
                Ok(AnyTracker::Local(t))
            }
            _ => {
                let client = self.github_client()?;
                github::from_config(&self.ctx.config, client)
                    .map(AnyTracker::Github)
                    .map_err(|e| e.to_string())
            }
        }
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

    fn do_env_sync(&self, a: EnvSyncArgs) -> Out {
        let target = self
            .repo(a.repo_path.as_deref())?
            .ok_or_else(|| "repo_path is required when not running inside a repo".to_string())?;
        let source = ehai_env::source_root(&target);
        let id = ehai_env::worktree_id(&target);
        let report = ehai_env::sync(&source, &target, &id, &self.ctx.home, &self.ctx.config.env)
            .map_err(|e| e.to_string())?;
        out(json!({ "status": "ok", "report": report, "ready": report.is_ready() }))
    }

    fn do_env_check(&self, a: EnvCheckArgs) -> Out {
        let target = self
            .repo(a.repo_path.as_deref())?
            .ok_or_else(|| "repo_path is required when not running inside a repo".to_string())?;
        let missing = ehai_env::check(&target);
        out(json!({ "status": "ok", "missing_keys": missing }))
    }

    fn do_save_prd(&self, a: SavePrdArgs) -> Out {
        let problems = a.prd.problems();
        if !problems.is_empty() {
            return out(json!({ "status": "invalid", "problems": problems }));
        }
        let checked = match self.check(&a.prd, a.policy,"PRD")? {
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
        let complexity = checked.value.risks.len() + checked.value.flows.len();
        self.observe_interaction(
            "last_plan_complexity",
            if complexity > 4 {
                "high"
            } else if complexity > 1 {
                "medium"
            } else {
                "low"
            },
        );
        out(
            json!({ "status": "ok", "slug": checked.value.slug(), "files": files,
                    "markdown": checked.value.to_markdown(), "sensitive_data": checked.action,
                    "low_confidence_flags": checked.notes,
                    "role": self.ctx.role(),
                    "presentation_hint": if self.ctx.role() == "pm" { "visual" } else { "technical" } }),
        )
    }

    fn do_plan_explore(&self, a: PlanExploreArgs) -> Out {
        let mut prd = Prd::load(&self.ctx.home, &a.prd).map_err(|e| e.to_string())?;
        let prd_updated = !a.answers.is_empty();
        if prd_updated {
            // apply each answer to the PRD struct
            for ans in &a.answers {
                match ans.field.as_str() {
                    "add" => match ans.section.as_str() {
                        "scope" => prd.out_of_scope.push(ans.value.clone()),
                        "users" => prd.users.push(ans.value.clone()),
                        "flows" => prd.flows.push(ehai_planner::prd::Flow {
                            name: format!("flow-{}", prd.flows.len() + 1),
                            mermaid: ans.value.clone(),
                        }),
                        _ => {}
                    },
                    "target" => {
                        if let Some(m) = prd.metrics.get_mut(ans.index) {
                            m.target = ans.value.clone();
                        }
                    }
                    "mitigation" => {
                        if let Some(r) = prd.risks.get_mut(ans.index) {
                            r.mitigation = ans.value.clone();
                        }
                    }
                    "mermaid" => {
                        if let Some(f) = prd.flows.get_mut(ans.index) {
                            f.mermaid = ans.value.clone();
                        }
                    }
                    _ => {}
                }
            }
            // data policy check on the updated PRD before saving
            let checked = match self.check(&prd, a.policy, "PRD")? {
                Guarded::Ask(q) => return out(q),
                Guarded::Ready(c) => c,
            };
            prd = checked.value;
            let problems = prd.problems();
            if !problems.is_empty() {
                return out(json!({ "status": "invalid", "problems": problems }));
            }
            prd.save(&self.ctx.home, None).map_err(|e| e.to_string())?;
        }
        let gaps = explore::find_gaps(&prd);
        let gaps_json: Vec<serde_json::Value> = gaps
            .iter()
            .map(|g| json!({ "section": g.section, "question": g.question, "hint": g.hint }))
            .collect();
        self.observe_interaction(
            "requirements_completeness",
            if gaps.is_empty() {
                "thorough"
            } else {
                "gaps_found"
            },
        );
        out(json!({
            "status": "ok",
            "prd": a.prd,
            "gaps_remaining": gaps.len(),
            "gaps": gaps_json,
            "prd_updated": prd_updated,
            "prd_markdown": if prd_updated { prd.to_markdown() } else { String::new() },
        }))
    }

    fn do_save_breakdown(&self, a: SaveBreakdownArgs) -> Out {
        let problems = a.breakdown.problems();
        if !problems.is_empty() {
            return out(json!({ "status": "invalid", "problems": problems }));
        }
        let checked = match self.check(&a.breakdown, a.policy,"breakdown")? {
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
                    "sensitive_data": checked.action }),
        )
    }

    async fn do_sync(&self, a: SyncArgs) -> Out {
        let b = Breakdown::load(&self.ctx.home, &a.prd).map_err(|e| e.to_string())?;
        let t = self.tracker()?;
        let remote = t
            .list_items(&Filter::default())
            .await
            .map_err(|e| e.to_string())?;
        let desired = match self.check(&b.to_desired_with(&numbers(&remote)), a.policy,"plan")? {
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
                    Some(a.policy.unwrap_or(SensitiveDataChoice::Redact)),
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
                    "sensitive_data_in_titles": (!flagged.is_empty()).then(|| format!("{} item title(s) look like they contain sensitive data; consider editing them", flagged.len())),
                    "role": self.ctx.role(),
                    "presentation_hint": if self.ctx.role() == "pm" { "visual" } else { "technical" } }),
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
            a.policy,
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
        let checked = match self.check(&a.body, a.policy,"comment")? {
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

    // ── POC handlers ──────────────────────────────────────────────────────

    fn do_poc_new(&self, a: PocNewArgs) -> Out {
        let root = self
            .repo(a.repo_path.as_deref())?
            .ok_or_else(|| "repo_path required".to_string())?;
        let created = ehai_poc::poc_new(
            &root,
            &self.ctx.home,
            &a.feature,
            &a.prompt,
            &self.ctx.config.env,
            self.ctx.config.sandbox.mode,
        )
        .map_err(|e| e.to_string())?;
        self.observe_interaction("poc_created", "true");
        out(json!({
            "status": "ok",
            "entry": created.entry,
            "worktree_path": created.entry.worktree,
            "branch": created.entry.branch,
            "port": created.entry.port,
            "url": format!("http://localhost:{}", created.entry.port),
            "design_context": created.design_context,
            "instructions": "Write the POC code inside `worktree_path`. Do not modify any files outside it. Run the dev server on the allocated `port`."
        }))
    }

    fn do_poc_iterate(&self, a: PocIterateArgs) -> Out {
        let root = self
            .repo(a.repo_path.as_deref())?
            .ok_or_else(|| "repo_path required".to_string())?;
        let created = ehai_poc::poc_iterate(
            &root,
            &self.ctx.home,
            &a.feature,
            a.parent_n,
            &a.feedback,
            &self.ctx.config.env,
            self.ctx.config.sandbox.mode,
        )
        .map_err(|e| e.to_string())?;
        out(json!({
            "status": "ok",
            "entry": created.entry,
            "worktree_path": created.entry.worktree,
            "branch": created.entry.branch,
            "port": created.entry.port,
            "url": format!("http://localhost:{}", created.entry.port),
            "design_context": created.design_context,
            "parent_n": a.parent_n,
            "instructions": "Write the iterated POC code inside `worktree_path`."
        }))
    }

    fn do_poc_list(&self, a: PocListArgs) -> Out {
        let root = self
            .repo(a.repo_path.as_deref())?
            .ok_or_else(|| "repo_path required".to_string())?;
        let entries = ehai_poc::poc_list(&root, a.feature.as_deref()).map_err(|e| e.to_string())?;
        out(json!({ "status": "ok", "count": entries.len(), "pocs": entries }))
    }

    fn do_poc_discard(&self, a: PocDiscardArgs) -> Out {
        let root = self
            .repo(a.repo_path.as_deref())?
            .ok_or_else(|| "repo_path required".to_string())?;
        ehai_poc::poc_discard(&root, &self.ctx.home, &a.feature, a.n).map_err(|e| e.to_string())?;
        let archive = format!(
            "ehai-archive/{}/poc-{}",
            ehai_poc::feature_slug(&a.feature),
            a.n
        );
        out(json!({ "status": "ok", "archived_as": archive }))
    }

    fn do_poc_compare(&self, a: PocCompareArgs) -> Out {
        let root = self
            .repo(a.repo_path.as_deref())?
            .ok_or_else(|| "repo_path required".to_string())?;
        let path = ehai_poc::poc_compare(&root, &self.ctx.home, &a.feature, &a.ns)
            .map_err(|e| e.to_string())?;
        out(json!({ "status": "ok", "report": path }))
    }

    fn do_poc_open(&self, a: PocOpenArgs) -> Out {
        let root = self
            .repo(a.repo_path.as_deref())?
            .ok_or_else(|| "repo_path required".to_string())?;
        let info = ehai_poc::poc_open_info(&root, &a.feature, a.n).map_err(|e| e.to_string())?;
        out(json!({
            "status": "ok",
            "worktree_path": info.worktree,
            "port": info.port,
            "branch": info.branch,
            "command": info.command,
            "url": info.url
        }))
    }

    fn do_poc_compose(&self, a: PocComposeArgs) -> Out {
        let root = self
            .repo(a.repo_path.as_deref())?
            .ok_or_else(|| "repo_path required".to_string())?;
        let sources: Vec<(u32, String)> = a
            .sources
            .into_iter()
            .map(|p| (p.n, p.description))
            .collect();
        let created = ehai_poc::poc_compose(
            &root,
            &self.ctx.home,
            &a.feature,
            &a.description,
            sources,
            &self.ctx.config.env,
            self.ctx.config.sandbox.mode,
        )
        .map_err(|e| e.to_string())?;
        let parent_ns: Vec<u32> = created.entry.parts.iter().map(|(n, _)| *n).collect();
        out(json!({
            "status": "ok",
            "entry": created.entry,
            "worktree_path": created.entry.worktree,
            "branch": created.entry.branch,
            "port": created.entry.port,
            "url": format!("http://localhost:{}", created.entry.port),
            "design_context": created.design_context,
            "composed_from": parent_ns,
            "instructions": "Write the composed POC code inside `worktree_path`, synthesising the described parts."
        }))
    }

    async fn do_poc_promote(&self, a: PocPromoteArgs) -> Out {
        // Confirm before creating a PR
        if a.confirmed.as_deref() != Some("yes") {
            return out(needs_input(
                "Promote POC?",
                format!(
                    "Promote poc-{n} of '{feature}' to `main`? This opens a GitHub pull request.",
                    n = a.n,
                    feature = a.feature
                ),
                vec![
                    opt(
                        "Yes, create PR",
                        "yes",
                        "Open a pull request to the default branch",
                    ),
                    opt("Cancel", "no", "Do nothing"),
                ],
                "confirmed",
            ));
        }
        let root = self
            .repo(a.repo_path.as_deref())?
            .ok_or_else(|| "repo_path required".to_string())?;
        let repo_slug = self
            .ctx
            .config
            .tracker
            .github
            .repo
            .as_deref()
            .ok_or_else(|| {
                format!(
                    "no repo configured; run `{} tracker set` first",
                    self.ctx.brand()
                )
            })?;
        let client = self.github_client()?;
        let pr_url = ehai_poc::poc_promote(
            &root,
            &self.ctx.home,
            &a.feature,
            a.n,
            &client,
            repo_slug,
            "main",
            self.ctx.config.sandbox.mode,
        )
        .await
        .map_err(|e| e.to_string())?;
        out(json!({ "status": "ok", "pr_url": pr_url,
                    "message": format!("poc-{n} promoted — review and merge the PR", n = a.n) }))
    }

    fn do_prefs_list(&self, a: PrefsListArgs) -> Out {
        let root = self
            .repo(a.repo_path.as_deref())
            .ok()
            .flatten()
            .and_then(|p| p.canonicalize().ok());
        let prefs = ehai_prefs::Prefs::merged(&self.ctx.home, root.as_deref());
        let entries: Vec<serde_json::Value> = prefs
            .entries
            .iter()
            .map(|p| {
                json!({ "key": p.key, "value": p.value,
                         "scope": format!("{:?}", p.scope).to_lowercase(),
                         "source": format!("{:?}", p.source).to_lowercase() })
            })
            .collect();
        out(
            json!({ "status": "ok", "count": entries.len(), "prefs": entries,
                    "context": prefs.to_context_text() }),
        )
    }

    fn do_prefs_set(&self, a: PrefsSetArgs) -> Out {
        use ehai_prefs::{Scope, Source};
        let use_repo = a.scope.as_deref() == Some("repo");
        if use_repo {
            let root = self
                .repo(a.repo_path.as_deref())?
                .ok_or_else(|| "repo_path required for repo-scoped prefs".to_string())?;
            let mut prefs = ehai_prefs::Prefs::load_repo(&root);
            prefs.set(&a.key, &a.value, Scope::Repo, Source::Explicit);
            prefs.save_repo(&root).map_err(|e| e.to_string())?;
        } else {
            let mut prefs = ehai_prefs::Prefs::load_global(&self.ctx.home);
            prefs.set(&a.key, &a.value, Scope::Global, Source::Explicit);
            prefs
                .save_global(&self.ctx.home)
                .map_err(|e| e.to_string())?;
        }
        out(json!({ "status": "ok", "key": a.key, "value": a.value,
                    "scope": a.scope.as_deref().unwrap_or("global") }))
    }

    fn do_prefs_delete(&self, a: PrefsDeleteArgs) -> Out {
        let use_repo = a.scope.as_deref() == Some("repo");
        let deleted = if use_repo {
            let root = self
                .repo(a.repo_path.as_deref())?
                .ok_or_else(|| "repo_path required for repo-scoped prefs".to_string())?;
            let mut prefs = ehai_prefs::Prefs::load_repo(&root);
            let d = prefs.delete(&a.key);
            if d {
                prefs.save_repo(&root).map_err(|e| e.to_string())?;
            }
            d
        } else {
            let mut prefs = ehai_prefs::Prefs::load_global(&self.ctx.home);
            let d = prefs.delete(&a.key);
            if d {
                prefs
                    .save_global(&self.ctx.home)
                    .map_err(|e| e.to_string())?;
            }
            d
        };
        out(json!({ "status": "ok", "key": a.key, "deleted": deleted }))
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
