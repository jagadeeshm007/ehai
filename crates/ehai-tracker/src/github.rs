//! GitHub Projects v2 adapter over hand-written GraphQL (reqwest + rustls).
//!
//! - Work items are issues in the configured repo, added to the project, so they
//!   get comments and PR links. Without a repo, items are project draft issues.
//! - Every request has a 30 s timeout and up to 3 retries with backoff on network
//!   errors, 5xx, 429 and secondary rate limits (honouring `Retry-After`).
//! - Input shapes were checked against GitHub's live schema by introspection
//!   on 2026-11-17 (DECISIONS ADR-0017).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;

use serde_json::{json, Value};

use crate::{
    split_footer, with_footer, Error, Filter, Item, ItemPatch, Kind, NewItem, Priority, Result,
    SchemaReport, Size, Status, Tracker,
};

pub const API: &str = "https://api.github.com";
const TIMEOUT: Duration = Duration::from_secs(30);
const ATTEMPTS: u32 = 3;

/// Low-level GraphQL/REST client.
#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base: String,
    token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectSummary {
    pub owner: String,
    pub number: u64,
    pub title: String,
    pub url: String,
    pub closed: bool,
    /// `viewerCanClose`: GitHub reserves closing a project to its admins, so
    /// this is the signal M3.5 uses to decide who gets asked for a role.
    pub viewer_is_admin: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Viewer {
    pub login: String,
    pub orgs: Vec<String>,
}

impl Client {
    /// `base` is the API root (`https://api.github.com`, or a mock server).
    pub fn new(base: &str, token: &str) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(TIMEOUT)
            .user_agent(concat!("ehai/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|e| Error::Http(e.to_string()))?;
        Ok(Self {
            http,
            base: base.trim_end_matches('/').to_string(),
            token: token.to_string(),
        })
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> Result<reqwest::Response> {
        let mut last = String::new();
        for attempt in 0..ATTEMPTS {
            let r = req
                .try_clone()
                .expect("requests have in-memory bodies")
                .bearer_auth(&self.token)
                .send()
                .await;
            let wait = match r {
                Ok(resp) => {
                    let status = resp.status();
                    if status == 401 {
                        return Err(Error::Auth(
                            "GitHub rejected the token (401): run `gh auth login` or refresh GITHUB_TOKEN".into(),
                        ));
                    }
                    let retry_after = resp
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<u64>().ok());
                    let rate_limited = status == 429
                        || (status == 403
                            && (retry_after.is_some()
                                || resp
                                    .headers()
                                    .get("x-ratelimit-remaining")
                                    .is_some_and(|v| v == "0")));
                    if !(status.is_server_error() || rate_limited) {
                        return Ok(resp);
                    }
                    last = format!("HTTP {status}");
                    retry_after.map(|s| Duration::from_secs(s.min(30)))
                }
                Err(e) => {
                    last = e.to_string();
                    None
                }
            };
            if attempt + 1 < ATTEMPTS {
                let backoff = wait.unwrap_or(Duration::from_millis(500 << attempt));
                tracing::warn!(attempt, ?backoff, error = %last, "retrying GitHub request");
                tokio::time::sleep(backoff).await;
            }
        }
        Err(Error::Http(format!(
            "GitHub unreachable after {ATTEMPTS} attempts: {last}"
        )))
    }

    /// Run a GraphQL operation; GraphQL-level errors become typed [`Error`]s.
    pub async fn graphql(&self, op: &str, query: &str, vars: Value) -> Result<Value> {
        let req = self
            .http
            .post(format!("{}/graphql", self.base))
            .json(&json!({ "operationName": op, "query": query, "variables": vars }));
        let resp = self.send(req).await?;
        let status = resp.status();
        let body: Value = resp
            .json()
            .await
            .map_err(|e| Error::Api(format!("{op}: unreadable response ({status}): {e}")))?;
        if let Some(errors) = body["errors"].as_array().filter(|e| !e.is_empty()) {
            let msg = errors
                .iter()
                .filter_map(|e| e["message"].as_str())
                .collect::<Vec<_>>()
                .join("; ");
            let kinds: Vec<&str> = errors.iter().filter_map(|e| e["type"].as_str()).collect();
            return Err(if kinds.contains(&"INSUFFICIENT_SCOPES") {
                Error::Auth(format!(
                    "{msg} — add the scope with `{}`",
                    crate::auth::SCOPE_FIX
                ))
            } else if kinds.contains(&"NOT_FOUND") {
                Error::NotFound(msg)
            } else {
                Error::Api(format!("{op}: {msg}"))
            });
        }
        if !status.is_success() {
            return Err(Error::Api(format!("{op}: HTTP {status}")));
        }
        Ok(body["data"].clone())
    }

    /// Token scopes from `GET /user`. `None` for fine-grained tokens.
    pub async fn scopes(&self) -> Result<Option<Vec<String>>> {
        let resp = self
            .send(self.http.get(format!("{}/user", self.base)))
            .await?;
        Ok(resp
            .headers()
            .get("x-oauth-scopes")
            .and_then(|v| v.to_str().ok())
            .map(|s| {
                s.split(',')
                    .map(|x| x.trim().to_string())
                    .filter(|x| !x.is_empty())
                    .collect()
            }))
    }

    pub async fn viewer(&self) -> Result<Viewer> {
        let d = self.graphql("Viewer", q::VIEWER, json!({})).await?;
        Ok(Viewer {
            login: str_of(&d["viewer"]["login"]),
            orgs: d["viewer"]["organizations"]["nodes"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|o| str_of(&o["login"]))
                .collect(),
        })
    }

    /// Projects of a user or org, most recently updated first.
    pub async fn projects(&self, owner: &str) -> Result<Vec<ProjectSummary>> {
        let d = self
            .graphql("Projects", q::PROJECTS, json!({ "owner": owner }))
            .await?;
        let o = &d["repositoryOwner"];
        if o.is_null() {
            return Err(Error::NotFound(format!("GitHub user or org `{owner}`")));
        }
        Ok(o["projectsV2"]["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| project_summary(owner, p))
            .collect())
    }

    pub async fn create_project(&self, owner: &str, title: &str) -> Result<ProjectSummary> {
        let d = self
            .graphql("Projects", q::PROJECTS, json!({ "owner": owner }))
            .await?;
        let owner_id = d["repositoryOwner"]["id"]
            .as_str()
            .ok_or_else(|| Error::NotFound(format!("GitHub user or org `{owner}`")))?;
        let d = self
            .graphql(
                "CreateProject",
                q::CREATE_PROJECT,
                json!({ "ownerId": owner_id, "title": title }),
            )
            .await?;
        Ok(project_summary(owner, &d["createProjectV2"]["projectV2"]))
    }

    /// Close a project (used by the live test to clean up after itself).
    pub async fn delete_project(&self, owner: &str, number: u64) -> Result<()> {
        let meta = fetch_meta(self, owner, number).await?;
        self.graphql(
            "DeleteProject",
            q::DELETE_PROJECT,
            json!({ "id": meta.project_id }),
        )
        .await
        .map(|_| ())
    }

    /// Is the viewer an admin of `owner #number`? (`viewerCanClose`; cheaper
    /// than [`Client::projects`] when only this one project matters.)
    pub async fn project_admin(&self, owner: &str, number: u64) -> Result<bool> {
        let d = self
            .graphql(
                "ProjectAdmin",
                q::PROJECT_ADMIN,
                json!({ "owner": owner, "number": number }),
            )
            .await?;
        d["repositoryOwner"]["projectV2"]["viewerCanClose"]
            .as_bool()
            .ok_or_else(|| Error::NotFound(format!("project #{number} of `{owner}`")))
    }

    /// Node id for a login, needed by the assignee mutations.
    async fn resolve_user_id(&self, login: &str) -> Result<String> {
        let d = self
            .graphql("ResolveUser", q::RESOLVE_USER, json!({ "login": login }))
            .await?;
        d["user"]["id"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| Error::NotFound(format!("GitHub user `{login}`")))
    }
}

fn str_of(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_string()
}

fn project_summary(owner: &str, p: &Value) -> ProjectSummary {
    ProjectSummary {
        owner: owner.to_string(),
        number: p["number"].as_u64().unwrap_or_default(),
        title: str_of(&p["title"]),
        url: str_of(&p["url"]),
        closed: p["closed"].as_bool().unwrap_or(false),
        viewer_is_admin: p["viewerCanClose"].as_bool().unwrap_or(false),
    }
}

#[derive(Debug, Clone)]
struct SelectOption {
    id: String,
    name: String,
    color: String,
    description: String,
}

#[derive(Debug, Clone)]
struct Field {
    id: String,
    data_type: String,
    options: Vec<SelectOption>,
}

#[derive(Debug, Clone)]
struct Meta {
    project_id: String,
    /// keyed by lowercase field name
    fields: HashMap<String, Field>,
}

async fn fetch_meta(c: &Client, owner: &str, number: u64) -> Result<Meta> {
    let d = c
        .graphql(
            "ProjectMeta",
            &q::PROJECT_META,
            json!({ "owner": owner, "number": number }),
        )
        .await?;
    let p = &d["repositoryOwner"]["projectV2"];
    let project_id = p["id"]
        .as_str()
        .ok_or_else(|| Error::NotFound(format!("project #{number} of `{owner}`")))?
        .to_string();
    let fields = p["fields"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| {
            let name = f["name"].as_str()?;
            Some((
                name.to_lowercase(),
                Field {
                    id: str_of(&f["id"]),
                    data_type: str_of(&f["dataType"]),
                    options: f["options"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .map(|o| SelectOption {
                            id: str_of(&o["id"]),
                            name: str_of(&o["name"]),
                            color: o["color"].as_str().unwrap_or("GRAY").to_string(),
                            description: str_of(&o["description"]),
                        })
                        .collect(),
                },
            ))
        })
        .collect();
    Ok(Meta { project_id, fields })
}

/// ehai's item-type field. Not `Type`: GitHub reserves that name for its built-in
/// issue types. An existing single-select `Type` field is still used if present.
pub const KIND_FIELD: &str = "Work type";

/// Look a field up by name; for the item type, fall back to a legacy single-select `Type`.
fn resolve_field<'a>(meta: &'a Meta, name: &str) -> Option<&'a Field> {
    meta.fields.get(&name.to_lowercase()).or_else(|| {
        (name == KIND_FIELD)
            .then(|| meta.fields.get("type"))
            .flatten()
            .filter(|f| f.data_type == "SINGLE_SELECT")
    })
}

/// Fields ehai manages: (name, option labels, option colours).
fn managed_fields() -> [(
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
); 4] {
    [
        (
            "Status",
            Status::LABELS,
            &["GRAY", "BLUE", "YELLOW", "PURPLE", "GREEN"],
        ),
        (
            "Priority",
            Priority::LABELS,
            &["RED", "ORANGE", "YELLOW", "GRAY"],
        ),
        (
            "Size",
            Size::LABELS,
            &["GRAY", "BLUE", "GREEN", "YELLOW", "ORANGE"],
        ),
        (KIND_FIELD, Kind::LABELS, &["PURPLE", "BLUE", "GRAY", "RED"]),
    ]
}

/// The GitHub Projects v2 tracker for one project.
pub struct GithubProjects {
    client: Client,
    owner: String,
    number: u64,
    /// `owner/name` of the repo issues are created in; `None` → draft issues.
    repo: Option<String>,
    meta: Mutex<Option<Meta>>,
    repo_id: Mutex<Option<String>>,
}

impl GithubProjects {
    pub fn new(client: Client, owner: &str, number: u64, repo: Option<String>) -> Self {
        Self {
            client,
            owner: owner.into(),
            number,
            repo,
            meta: Mutex::new(None),
            repo_id: Mutex::new(None),
        }
    }

    async fn meta(&self) -> Result<Meta> {
        if let Some(m) = self.meta.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            return Ok(m);
        }
        let m = fetch_meta(&self.client, &self.owner, self.number).await?;
        *self.meta.lock().unwrap_or_else(|e| e.into_inner()) = Some(m.clone());
        Ok(m)
    }

    fn forget_meta(&self) {
        *self.meta.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    async fn repo_id(&self) -> Result<Option<String>> {
        let Some(repo) = &self.repo else {
            return Ok(None);
        };
        if let Some(id) = self
            .repo_id
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            return Ok(Some(id));
        }
        let (owner, name) = repo
            .split_once('/')
            .ok_or_else(|| Error::Config(format!("repo `{repo}` must be owner/name")))?;
        let d = self
            .client
            .graphql("Repo", q::REPO, json!({ "owner": owner, "name": name }))
            .await?;
        let id = d["repository"]["id"]
            .as_str()
            .ok_or_else(|| Error::NotFound(format!("repository {repo}")))?
            .to_string();
        *self.repo_id.lock().unwrap_or_else(|e| e.into_inner()) = Some(id.clone());
        Ok(Some(id))
    }

    /// Set a single-select field by label.
    async fn set_select(&self, meta: &Meta, item: &str, field: &str, label: &str) -> Result<()> {
        let f = resolve_field(meta, field).ok_or_else(|| {
            Error::Config(format!(
                "project has no `{field}` field; run `ehai tracker schema`"
            ))
        })?;
        let opt = f
            .options
            .iter()
            .find(|o| o.name.eq_ignore_ascii_case(label))
            .ok_or_else(|| {
                Error::Config(format!(
                    "`{field}` has no `{label}` option; run `ehai tracker schema`"
                ))
            })?;
        self.client
            .graphql(
                "SetField",
                q::SET_FIELD,
                json!({ "project": meta.project_id, "item": item, "field": f.id, "option": opt.id }),
            )
            .await
            .map(|_| ())
    }

    /// Best effort: an unusable field must not fail creating the item itself.
    async fn set_fields(&self, meta: &Meta, item: &str, fields: &[(&str, Option<&str>)]) {
        for (field, label) in fields {
            if let Some(label) = label {
                if let Err(e) = self.set_select(meta, item, field, label).await {
                    tracing::warn!(%field, %label, error = %e, "could not set field");
                }
            }
        }
    }

    /// (project item, content node id, content typename, stored body) for an item.
    async fn raw_item(&self, id: &str) -> Result<Value> {
        let d = self
            .client
            .graphql("Item", &q::GET_ITEM, json!({ "id": id }))
            .await?;
        if d["node"]["id"].is_null() {
            return Err(Error::NotFound(format!("item {id}")));
        }
        Ok(d["node"].clone())
    }
}

fn parse_item(n: &Value) -> Option<Item> {
    let c = &n["content"];
    let typename = c["__typename"].as_str()?;
    if !matches!(typename, "Issue" | "DraftIssue") || n["isArchived"].as_bool() == Some(true) {
        return None; // pull requests and archived cards aren't work items
    }
    let (body, ehai_id) = split_footer(c["body"].as_str().unwrap_or_default());
    let mut values: HashMap<String, String> = HashMap::new();
    for v in n["fieldValues"]["nodes"].as_array().into_iter().flatten() {
        if let (Some(field), Some(name)) = (v["field"]["name"].as_str(), v["name"].as_str()) {
            values.insert(field.to_lowercase(), name.to_string());
        }
    }
    let get = |k: &str| values.get(k).map(String::as_str);
    Some(Item {
        id: str_of(&n["id"]),
        number: c["number"].as_u64(),
        url: c["url"].as_str().map(String::from),
        title: str_of(&c["title"]),
        body,
        ehai_id,
        status: get("status").and_then(Status::from_label),
        priority: get("priority").and_then(Priority::from_label),
        size: get("size").and_then(Size::from_label),
        kind: get("work type").or(get("type")).and_then(Kind::from_label),
        closed: c["closed"].as_bool().unwrap_or(false),
        updated_at: n["updatedAt"].as_str().map(String::from),
        assignees: c["assignees"]["nodes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|a| a["login"].as_str().map(String::from))
            .collect(),
    })
}

impl Tracker for GithubProjects {
    async fn ensure_schema(&self) -> Result<SchemaReport> {
        let meta = self.meta().await?;
        let mut report = SchemaReport::default();
        for (name, labels, colors) in managed_fields() {
            let wanted: Vec<(&str, &str)> =
                labels.iter().copied().zip(colors.iter().copied()).collect();
            match resolve_field(&meta, name) {
                None => {
                    let options: Vec<Value> = wanted
                        .iter()
                        .map(|(n, c)| json!({ "name": n, "color": c, "description": "" }))
                        .collect();
                    let r = self
                        .client
                        .graphql(
                            "CreateField",
                            q::CREATE_FIELD,
                            json!({ "project": meta.project_id, "name": name,
                                    "type": "SINGLE_SELECT", "options": options }),
                        )
                        .await;
                    match r {
                        Ok(_) => report.created_fields.push(name.to_string()),
                        Err(e @ Error::Auth(_)) => return Err(e),
                        // one bad field must not stop the others
                        Err(e) => report.warnings.push(format!("`{name}` not created: {e}")),
                    }
                }
                Some(f) if f.data_type != "SINGLE_SELECT" => report.warnings.push(format!(
                    "`{name}` exists as a {} field, so ehai can't manage it; rename it to let ehai create its own",
                    f.data_type
                )),
                Some(f) => {
                    let missing: Vec<_> = wanted
                        .iter()
                        .filter(|(n, _)| !f.options.iter().any(|o| o.name.eq_ignore_ascii_case(n)))
                        .collect();
                    if missing.is_empty() {
                        continue;
                    }
                    // Existing options are sent back with their ids so GitHub keeps
                    // them (and every card's current value); new ones are appended.
                    let mut options: Vec<Value> = f
                        .options
                        .iter()
                        .map(|o| json!({ "id": o.id, "name": o.name, "color": o.color, "description": o.description }))
                        .collect();
                    options.extend(missing.iter().map(|(n, c)| json!({ "name": n, "color": c, "description": "" })));
                    let r = self
                        .client
                        .graphql("UpdateField", q::UPDATE_FIELD, json!({ "field": f.id, "options": options }))
                        .await;
                    match r {
                        Ok(_) => report
                            .added_options
                            .extend(missing.iter().map(|(n, _)| format!("{name}: {n}"))),
                        Err(e @ Error::Auth(_)) => return Err(e),
                        Err(e) => report.warnings.push(format!("`{name}` options not added: {e}")),
                    }
                }
            }
        }
        if !meta.fields.values().any(|f| f.data_type == "ITERATION") {
            let r = self
                .client
                .graphql(
                    "CreateField",
                    q::CREATE_FIELD,
                    json!({ "project": meta.project_id, "name": "Iteration", "type": "ITERATION",
                            "iteration": { "startDate": ehai_core::date::today_utc(), "duration": 14, "iterations": [] } }),
                )
                .await;
            match r {
                Ok(_) => report.created_fields.push("Iteration".into()),
                Err(e) => report
                    .warnings
                    .push(format!("Iteration field not created: {e}")),
            }
        }
        self.forget_meta();
        Ok(report)
    }

    async fn create_item(&self, n: NewItem) -> Result<Item> {
        let meta = self.meta().await?;
        let body = with_footer(&n.body, n.ehai_id.as_deref());
        let item_id = match self.repo_id().await? {
            Some(repo) => {
                let d = self
                    .client
                    .graphql(
                        "CreateIssue",
                        q::CREATE_ISSUE,
                        json!({ "repo": repo, "title": n.title, "body": body }),
                    )
                    .await?;
                let issue = str_of(&d["createIssue"]["issue"]["id"]);
                let d = self
                    .client
                    .graphql(
                        "AddItem",
                        q::ADD_ITEM,
                        json!({ "project": meta.project_id, "content": issue }),
                    )
                    .await?;
                str_of(&d["addProjectV2ItemById"]["item"]["id"])
            }
            None => {
                let d = self
                    .client
                    .graphql(
                        "AddDraft",
                        q::ADD_DRAFT,
                        json!({ "project": meta.project_id, "title": n.title, "body": body }),
                    )
                    .await?;
                str_of(&d["addProjectV2DraftIssue"]["projectItem"]["id"])
            }
        };
        let status = n.status.unwrap_or(Status::Todo);
        self.set_fields(
            &meta,
            &item_id,
            &[
                ("Status", Some(status.label())),
                ("Priority", n.priority.map(Priority::label)),
                ("Size", n.size.map(Size::label)),
                (KIND_FIELD, n.kind.map(Kind::label)),
            ],
        )
        .await;
        self.get_item(&item_id).await
    }

    async fn update_item(&self, id: &str, p: ItemPatch) -> Result<Item> {
        let meta = self.meta().await?;
        if p.title.is_some() || p.body.is_some() {
            let raw = self.raw_item(id).await?;
            let c = &raw["content"];
            // keep the ehai-id footer when the body changes
            let (_, ehai_id) = split_footer(c["body"].as_str().unwrap_or_default());
            let body = p
                .body
                .as_deref()
                .map(|b| with_footer(b, ehai_id.as_deref()));
            let (op, query) = match c["__typename"].as_str() {
                Some("Issue") => ("UpdateIssue", q::UPDATE_ISSUE),
                Some("DraftIssue") => ("UpdateDraft", q::UPDATE_DRAFT),
                other => return Err(Error::Unsupported(format!("editing {other:?} items"))),
            };
            self.client
                .graphql(
                    op,
                    query,
                    // Omit unchanged fields: GitHub treats an explicit `null`
                    // as "clear it" (found by the live test: a rename wiped the body).
                    {
                        let mut vars = json!({ "id": c["id"] });
                        if let Some(t) = &p.title {
                            vars["title"] = json!(t);
                        }
                        if let Some(b) = &body {
                            vars["body"] = json!(b);
                        }
                        vars
                    },
                )
                .await?;
        }
        for (field, label) in [
            ("Priority", p.priority.map(Priority::label)),
            ("Size", p.size.map(Size::label)),
            (KIND_FIELD, p.kind.map(Kind::label)),
        ] {
            if let Some(label) = label {
                self.set_select(&meta, id, field, label).await?;
            }
        }
        self.get_item(id).await
    }

    async fn set_status(&self, id: &str, status: Status) -> Result<()> {
        let meta = self.meta().await?;
        self.set_select(&meta, id, "Status", status.label()).await
    }

    async fn list_items(&self, filter: &Filter) -> Result<Vec<Item>> {
        let meta = self.meta().await?;
        let mut items = Vec::new();
        let mut after: Option<String> = None;
        loop {
            let d = self
                .client
                .graphql(
                    "Items",
                    &q::LIST_ITEMS,
                    json!({ "project": meta.project_id, "after": after }),
                )
                .await?;
            let page = &d["node"]["items"];
            items.extend(
                page["nodes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(parse_item)
                    .filter(|i| filter.matches(i)),
            );
            if page["pageInfo"]["hasNextPage"].as_bool() != Some(true) {
                break;
            }
            after = page["pageInfo"]["endCursor"].as_str().map(String::from);
        }
        Ok(items)
    }

    async fn get_item(&self, id: &str) -> Result<Item> {
        parse_item(&self.raw_item(id).await?)
            .ok_or_else(|| Error::NotFound(format!("work item {id}")))
    }

    async fn comment(&self, id: &str, body: &str) -> Result<()> {
        let raw = self.raw_item(id).await?;
        if raw["content"]["__typename"] != "Issue" {
            return Err(Error::Unsupported(
                "comments on draft items; set a repo with `ehai tracker set --repo owner/name`"
                    .into(),
            ));
        }
        self.client
            .graphql(
                "Comment",
                q::COMMENT,
                json!({ "subject": raw["content"]["id"], "body": body }),
            )
            .await
            .map(|_| ())
    }

    async fn link_pr(&self, id: &str, pr_url: &str) -> Result<()> {
        self.comment(id, &format!("Linked pull request: {pr_url}"))
            .await
    }

    async fn assign(&self, id: &str, logins: &[String]) -> Result<()> {
        if logins.is_empty() {
            return Ok(());
        }
        let raw = self.raw_item(id).await?;
        let c = &raw["content"];
        let mut ids = Vec::with_capacity(logins.len());
        for login in logins {
            ids.push(self.client.resolve_user_id(login).await?);
        }
        match c["__typename"].as_str() {
            Some("Issue") => {
                self.client
                    .graphql(
                        "AddAssignees",
                        q::ADD_ASSIGNEES,
                        json!({ "id": c["id"], "ids": ids }),
                    )
                    .await?;
            }
            Some("DraftIssue") => {
                let current = draft_assignee_ids(c);
                let merged: Vec<String> = current
                    .into_iter()
                    .chain(ids)
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect();
                self.client
                    .graphql(
                        "SetDraftAssignees",
                        q::SET_DRAFT_ASSIGNEES,
                        json!({ "id": c["id"], "ids": merged }),
                    )
                    .await?;
            }
            other => return Err(Error::Unsupported(format!("assigning {other:?} items"))),
        }
        Ok(())
    }

    async fn unassign(&self, id: &str, logins: &[String]) -> Result<()> {
        if logins.is_empty() {
            return Ok(());
        }
        let raw = self.raw_item(id).await?;
        let c = &raw["content"];
        let mut ids = Vec::with_capacity(logins.len());
        for login in logins {
            ids.push(self.client.resolve_user_id(login).await?);
        }
        match c["__typename"].as_str() {
            Some("Issue") => {
                self.client
                    .graphql(
                        "RemoveAssignees",
                        q::REMOVE_ASSIGNEES,
                        json!({ "id": c["id"], "ids": ids }),
                    )
                    .await?;
            }
            Some("DraftIssue") => {
                let remaining: Vec<String> = draft_assignee_ids(c)
                    .into_iter()
                    .filter(|a| !ids.contains(a))
                    .collect();
                self.client
                    .graphql(
                        "SetDraftAssignees",
                        q::SET_DRAFT_ASSIGNEES,
                        json!({ "id": c["id"], "ids": remaining }),
                    )
                    .await?;
            }
            other => return Err(Error::Unsupported(format!("unassigning {other:?} items"))),
        }
        Ok(())
    }
}

/// Current assignee node ids of a `content` value from the item fragment.
fn draft_assignee_ids(c: &Value) -> Vec<String> {
    c["assignees"]["nodes"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| a["id"].as_str().map(String::from))
        .collect()
}

/// API root: `EHAI_GITHUB_API` when set (tests point it at a fake), else GitHub.
pub fn api_base() -> String {
    std::env::var("EHAI_GITHUB_API").unwrap_or_else(|_| API.to_string())
}

/// A client for the user's token (`gh`, then env). `EHAI_NO_GH=1` skips `gh`.
pub fn client_from_env() -> Result<(Client, crate::auth::Token)> {
    let env: Vec<(String, String)> = std::env::vars().collect();
    let token = crate::auth::resolve(&env, std::env::var_os("EHAI_NO_GH").is_none())?;
    Ok((Client::new(&api_base(), &token.value)?, token))
}

/// The project configured in `[tracker]`, or a hint on how to configure one.
pub fn from_config(config: &ehai_core::Config, client: Client) -> Result<GithubProjects> {
    use ehai_core::config::TrackerKind;
    if config.tracker.kind != TrackerKind::GithubProjects {
        return Err(Error::Unsupported(
            "only GitHub Projects is available so far".into(),
        ));
    }
    let g = &config.tracker.github;
    match (&g.owner, g.project) {
        (Some(owner), Some(number)) => {
            Ok(GithubProjects::new(client, owner, number, g.repo.clone()))
        }
        _ => Err(Error::Config(format!(
            "no tracker connected yet: run `{} tracker set`",
            config.brand.name
        ))),
    }
}

/// GraphQL documents. Operation names match the `op` argument, which the mock
/// server in the contract tests dispatches on.
mod q {
    const FIELDS: &str = "fields(first: 50) { nodes {
        ... on ProjectV2FieldCommon { id name dataType }
        ... on ProjectV2SingleSelectField { options { id name color description } } } }";

    const ITEM: &str = "fragment ItemF on ProjectV2Item {
        id isArchived updatedAt
        content { __typename
          ... on Issue { id number url title body closed assignees(first: 20) { nodes { id login } } }
          ... on DraftIssue { id title body assignees(first: 20) { nodes { id login } } } }
        fieldValues(first: 30) { nodes {
          ... on ProjectV2ItemFieldSingleSelectValue { name field { ... on ProjectV2FieldCommon { name } } } } } }";

    pub const VIEWER: &str =
        "query Viewer { viewer { login organizations(first: 50) { nodes { login } } } }";

    pub const PROJECTS: &str = "query Projects($owner: String!) { repositoryOwner(login: $owner) { id login
        ... on ProjectV2Owner { projectsV2(first: 50, orderBy: {field: UPDATED_AT, direction: DESC}) {
          nodes { number title url closed viewerCanClose } } } } }";

    pub const PROJECT_ADMIN: &str = "query ProjectAdmin($owner: String!, $number: Int!) {
        repositoryOwner(login: $owner) { ... on ProjectV2Owner { projectV2(number: $number) { viewerCanClose } } } }";

    pub const RESOLVE_USER: &str =
        "query ResolveUser($login: String!) { user(login: $login) { id } }";

    pub const ADD_ASSIGNEES: &str = "mutation AddAssignees($id: ID!, $ids: [ID!]) {
        addAssigneesToAssignable(input: {assignableId: $id, assigneeIds: $ids}) { clientMutationId } }";

    pub const REMOVE_ASSIGNEES: &str = "mutation RemoveAssignees($id: ID!, $ids: [ID!]!) {
        removeAssigneesFromAssignable(input: {assignableId: $id, assigneeIds: $ids}) { clientMutationId } }";

    pub const SET_DRAFT_ASSIGNEES: &str = "mutation SetDraftAssignees($id: ID!, $ids: [ID!]) {
        updateProjectV2DraftIssue(input: {draftIssueId: $id, assigneeIds: $ids}) { clientMutationId } }";

    pub const CREATE_PROJECT: &str = "mutation CreateProject($ownerId: ID!, $title: String!) {
        createProjectV2(input: {ownerId: $ownerId, title: $title}) { projectV2 { number title url closed } } }";

    pub const DELETE_PROJECT: &str = "mutation DeleteProject($id: ID!) {
        deleteProjectV2(input: {projectId: $id}) { clientMutationId } }";

    pub const REPO: &str =
        "query Repo($owner: String!, $name: String!) { repository(owner: $owner, name: $name) { id } }";

    pub const CREATE_FIELD: &str = "mutation CreateField($project: ID!, $name: String!, $type: ProjectV2CustomFieldType!,
        $options: [ProjectV2SingleSelectFieldOptionInput!], $iteration: ProjectV2IterationFieldConfigurationInput) {
        createProjectV2Field(input: {projectId: $project, dataType: $type, name: $name,
          singleSelectOptions: $options, iterationConfiguration: $iteration}) {
          projectV2Field { ... on ProjectV2FieldCommon { id } } } }";

    pub const UPDATE_FIELD: &str =
        "mutation UpdateField($field: ID!, $options: [ProjectV2SingleSelectFieldOptionInput!]) {
        updateProjectV2Field(input: {fieldId: $field, singleSelectOptions: $options}) {
          projectV2Field { ... on ProjectV2FieldCommon { id } } } }";

    pub const CREATE_ISSUE: &str =
        "mutation CreateIssue($repo: ID!, $title: String!, $body: String!) {
        createIssue(input: {repositoryId: $repo, title: $title, body: $body}) { issue { id } } }";

    pub const ADD_ITEM: &str = "mutation AddItem($project: ID!, $content: ID!) {
        addProjectV2ItemById(input: {projectId: $project, contentId: $content}) { item { id } } }";

    pub const ADD_DRAFT: &str = "mutation AddDraft($project: ID!, $title: String!, $body: String!) {
        addProjectV2DraftIssue(input: {projectId: $project, title: $title, body: $body}) { projectItem { id } } }";

    pub const SET_FIELD: &str =
        "mutation SetField($project: ID!, $item: ID!, $field: ID!, $option: String!) {
        updateProjectV2ItemFieldValue(input: {projectId: $project, itemId: $item, fieldId: $field,
          value: {singleSelectOptionId: $option}}) { projectV2Item { id } } }";

    pub const UPDATE_ISSUE: &str = "mutation UpdateIssue($id: ID!, $title: String, $body: String) {
        updateIssue(input: {id: $id, title: $title, body: $body}) { issue { id } } }";

    pub const UPDATE_DRAFT: &str = "mutation UpdateDraft($id: ID!, $title: String, $body: String) {
        updateProjectV2DraftIssue(input: {draftIssueId: $id, title: $title, body: $body}) { draftIssue { id } } }";

    pub const COMMENT: &str = "mutation Comment($subject: ID!, $body: String!) {
        addComment(input: {subjectId: $subject, body: $body}) { clientMutationId } }";

    pub fn project_meta() -> String {
        format!(
            "query ProjectMeta($owner: String!, $number: Int!) {{ repositoryOwner(login: $owner) {{
              ... on ProjectV2Owner {{ projectV2(number: $number) {{ id title url {FIELDS} }} }} }} }}"
        )
    }

    pub fn list_items() -> String {
        format!(
            "query Items($project: ID!, $after: String) {{ node(id: $project) {{ ... on ProjectV2 {{
              items(first: 100, after: $after) {{ pageInfo {{ hasNextPage endCursor }} nodes {{ ...ItemF }} }} }} }} }} {ITEM}"
        )
    }

    pub fn get_item() -> String {
        format!("query Item($id: ID!) {{ node(id: $id) {{ ...ItemF }} }} {ITEM}")
    }

    pub static PROJECT_META: std::sync::LazyLock<String> = std::sync::LazyLock::new(project_meta);
    pub static LIST_ITEMS: std::sync::LazyLock<String> = std::sync::LazyLock::new(list_items);
    pub static GET_ITEM: std::sync::LazyLock<String> = std::sync::LazyLock::new(get_item);
}
