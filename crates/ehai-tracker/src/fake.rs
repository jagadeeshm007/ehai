//! `FakeGithub`: a small stateful GraphQL server (on wiremock) modelling the parts
//! of GitHub Projects v2 ehai uses, including the rule that a card loses its value
//! when its single-select option id disappears. Behind the `fake` feature; used by
//! the tracker contract suite and the CLI's end-to-end tests.

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::{json, Value};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

/// Mount the fake on `server`: `POST /graphql` plus `GET /user` reporting
/// classic-token scopes `repo, project`.
pub async fn mount(server: &MockServer) -> FakeGithub {
    let fake = FakeGithub::default();
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(fake.clone())
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path("/user"))
        .respond_with(ResponseTemplate::new(200).insert_header("x-oauth-scopes", "repo, project"))
        .mount(server)
        .await;
    fake
}

#[derive(Clone, Default)]
pub struct FakeGithub(std::sync::Arc<Mutex<State>>);

#[derive(Default)]
struct State {
    next: u64,
    fields: Vec<FakeField>,
    items: Vec<FakeItem>,
    comments: Vec<String>,
    seeded: bool,
    fail_field: Option<String>,
}

struct FakeField {
    id: String,
    name: String,
    data_type: String,
    options: Vec<(String, String, String)>, // id, name, color
}

struct FakeItem {
    id: String,
    content_id: String,
    typename: &'static str,
    number: Option<u64>,
    title: String,
    body: String,
    values: HashMap<String, String>, // field id → option id
}

impl FakeGithub {
    /// Titles of every item on the board, in creation order.
    pub fn item_titles(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap()
            .items
            .iter()
            .map(|i| i.title.clone())
            .collect()
    }

    /// Make creating the field `name` fail (simulates a GitHub-side error).
    pub fn fail_field(&self, name: &str) {
        self.0.lock().unwrap().fail_field = Some(name.into());
    }

    /// Add a field before ehai sees the project (e.g. a team's own `Type`).
    pub fn add_select_field(&self, name: &str, options: &[&str]) {
        let mut s = self.0.lock().unwrap();
        s.seed();
        let id = s.id("F");
        let opts = options
            .iter()
            .map(|o| (s.id("O"), o.to_string(), "GRAY".to_string()))
            .collect();
        s.fields.push(FakeField {
            id,
            name: name.into(),
            data_type: "SINGLE_SELECT".into(),
            options: opts,
        });
    }

    /// Names of the project's fields.
    pub fn field_names(&self) -> Vec<String> {
        let mut s = self.0.lock().unwrap();
        s.seed();
        s.fields.iter().map(|f| f.name.clone()).collect()
    }

    pub fn comments(&self) -> Vec<String> {
        self.0.lock().unwrap().comments.clone()
    }
}

impl State {
    fn id(&mut self, prefix: &str) -> String {
        self.next += 1;
        format!("{prefix}_{}", self.next)
    }

    /// New GitHub projects come with Title + a Status field (Todo/In Progress/Done).
    fn seed(&mut self) {
        if self.seeded {
            return;
        }
        self.seeded = true;
        let title = self.id("F");
        self.fields.push(FakeField {
            id: title,
            name: "Title".into(),
            data_type: "TITLE".into(),
            options: vec![],
        });
        let status = self.id("F");
        let opts = ["Todo", "In Progress", "Done"]
            .iter()
            .map(|n| (self.id("O"), n.to_string(), "GRAY".to_string()))
            .collect();
        self.fields.push(FakeField {
            id: status,
            name: "Status".into(),
            data_type: "SINGLE_SELECT".into(),
            options: opts,
        });
    }

    fn field_json(f: &FakeField) -> Value {
        json!({ "id": f.id, "name": f.name, "dataType": f.data_type,
                "options": f.options.iter().map(|(id, n, c)| json!({ "id": id, "name": n, "color": c, "description": "" })).collect::<Vec<_>>() })
    }

    fn item_json(&self, i: &FakeItem) -> Value {
        let values: Vec<Value> = i
            .values
            .iter()
            .filter_map(|(fid, oid)| {
                let f = self.fields.iter().find(|f| &f.id == fid)?;
                let (_, name, _) = f.options.iter().find(|(id, _, _)| id == oid)?; // vanished option ⇒ value gone
                Some(json!({ "name": name, "field": { "name": f.name } }))
            })
            .collect();
        let mut content = json!({ "__typename": i.typename, "id": i.content_id, "title": i.title, "body": i.body });
        if let Some(n) = i.number {
            content["number"] = json!(n);
            content["url"] = json!(format!("https://github.com/octo/app/issues/{n}"));
            content["closed"] = json!(false);
        }
        json!({ "id": i.id, "isArchived": false, "updatedAt": "2026-11-17T00:00:00Z", "content": content, "fieldValues": { "nodes": values } })
    }

    fn handle(&mut self, op: &str, v: &Value) -> Value {
        self.seed();
        match op {
            // only project #1 exists
            "ProjectMeta" if v["number"] != 1 => {
                json!({ "repositoryOwner": { "projectV2": null } })
            }
            "ProjectMeta" => json!({ "repositoryOwner": { "projectV2": {
                "id": "PVT_1", "title": "Fake", "url": "https://github.com/orgs/octo/projects/1",
                "fields": { "nodes": self.fields.iter().map(Self::field_json).collect::<Vec<_>>() } } } }),
            "CreateField" => {
                let name = v["name"].as_str().unwrap();
                // GitHub reserves these names for built-in fields
                if ["type", "title", "status", "assignees", "labels"]
                    .contains(&name.to_lowercase().as_str())
                    && !self
                        .fields
                        .iter()
                        .any(|f| f.name.eq_ignore_ascii_case(name))
                    || self.fail_field.as_deref() == Some(name)
                {
                    return json!({ "__error": "Name cannot have a reserved value" });
                }
                let id = self.id("F");
                let options = v["options"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .map(|o| {
                        (
                            self.id("O"),
                            o["name"].as_str().unwrap().to_string(),
                            o["color"].as_str().unwrap().to_string(),
                        )
                    })
                    .collect();
                self.fields.push(FakeField {
                    id: id.clone(),
                    name: v["name"].as_str().unwrap().into(),
                    data_type: v["type"].as_str().unwrap().into(),
                    options,
                });
                json!({ "createProjectV2Field": { "projectV2Field": { "id": id } } })
            }
            "UpdateField" => {
                // GitHub semantics: the list *replaces* options; ids passed back are kept.
                let fid = v["field"].as_str().unwrap().to_string();
                let new: Vec<_> = v["options"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|o| {
                        (
                            o["id"]
                                .as_str()
                                .map(String::from)
                                .unwrap_or_else(|| self.id("O")),
                            o["name"].as_str().unwrap().into(),
                            o["color"].as_str().unwrap().into(),
                        )
                    })
                    .collect();
                self.fields
                    .iter_mut()
                    .find(|f| f.id == fid)
                    .unwrap()
                    .options = new;
                json!({ "updateProjectV2Field": { "projectV2Field": { "id": fid } } })
            }
            "Repo" => json!({ "repository": { "id": "R_1" } }),
            "Viewer" => {
                json!({ "viewer": { "login": "octo", "organizations": { "nodes": [{ "login": "acme" }] } } })
            }
            "Projects" => json!({ "repositoryOwner": { "id": "U_1", "login": v["owner"],
                "projectsV2": { "nodes": [{ "number": 1, "title": "Fake", "url": "https://github.com/users/octo/projects/1", "closed": false }] } } }),
            "CreateProject" => json!({ "createProjectV2": { "projectV2": {
                "number": 1, "title": v["title"], "url": "https://github.com/users/octo/projects/1", "closed": false } } }),
            "CreateIssue" => {
                let content_id = self.id("I");
                let number = self.next;
                self.items.push(FakeItem {
                    id: String::new(),
                    content_id: content_id.clone(),
                    typename: "Issue",
                    number: Some(number),
                    title: v["title"].as_str().unwrap().into(),
                    body: v["body"].as_str().unwrap().into(),
                    values: HashMap::new(),
                });
                json!({ "createIssue": { "issue": { "id": content_id } } })
            }
            "AddItem" => {
                let id = self.id("PVTI");
                let content = v["content"].as_str().unwrap();
                self.items
                    .iter_mut()
                    .find(|i| i.content_id == content)
                    .unwrap()
                    .id = id.clone();
                json!({ "addProjectV2ItemById": { "item": { "id": id } } })
            }
            "AddDraft" => {
                let id = self.id("PVTI");
                let content_id = self.id("DI");
                self.items.push(FakeItem {
                    id: id.clone(),
                    content_id,
                    typename: "DraftIssue",
                    number: None,
                    title: v["title"].as_str().unwrap().into(),
                    body: v["body"].as_str().unwrap().into(),
                    values: HashMap::new(),
                });
                json!({ "addProjectV2DraftIssue": { "projectItem": { "id": id } } })
            }
            "SetField" => {
                let (fid, oid) = (v["field"].as_str().unwrap(), v["option"].as_str().unwrap());
                let valid = self
                    .fields
                    .iter()
                    .any(|f| f.id == fid && f.options.iter().any(|(id, _, _)| id == oid));
                if !valid {
                    return json!({ "__error": "bad option" });
                }
                let item = self
                    .items
                    .iter_mut()
                    .find(|i| i.id == v["item"].as_str().unwrap())
                    .unwrap();
                item.values.insert(fid.into(), oid.into());
                json!({ "updateProjectV2ItemFieldValue": { "projectV2Item": { "id": item.id } } })
            }
            "Item" => match self
                .items
                .iter()
                .find(|i| i.id == v["id"].as_str().unwrap())
            {
                Some(i) => json!({ "node": self.item_json(i) }),
                None => json!({ "node": null }),
            },
            "Items" => {
                let start: usize = v["after"].as_str().map_or(0, |c| c.parse().unwrap());
                let page: Vec<Value> = self
                    .items
                    .iter()
                    .skip(start)
                    .take(2)
                    .map(|i| self.item_json(i))
                    .collect();
                let end = start + page.len();
                json!({ "node": { "items": { "nodes": page,
                    "pageInfo": { "hasNextPage": end < self.items.len(), "endCursor": end.to_string() } } } })
            }
            "UpdateIssue" | "UpdateDraft" => {
                let item = self
                    .items
                    .iter_mut()
                    .find(|i| i.content_id == v["id"].as_str().unwrap())
                    .unwrap();
                if let Some(t) = v["title"].as_str() {
                    item.title = t.into();
                }
                // GitHub semantics: an omitted variable leaves the field alone,
                // an explicit `null` clears it.
                if let Some(b) = v.get("body") {
                    item.body = b.as_str().unwrap_or_default().into();
                }
                json!({})
            }
            "Comment" => {
                self.comments.push(v["body"].as_str().unwrap().into());
                json!({ "addComment": { "clientMutationId": null } })
            }
            other => json!({ "__error": format!("unknown operation {other}") }),
        }
    }
}

impl Respond for FakeGithub {
    fn respond(&self, req: &Request) -> ResponseTemplate {
        let body: Value = serde_json::from_slice(&req.body).unwrap();
        assert!(
            req.headers
                .get("authorization")
                .and_then(|h| h.to_str().ok())
                .is_some_and(|h| h.starts_with("Bearer ") && h.len() > 7),
            "every request carries a bearer token"
        );
        let op = body["operationName"].as_str().unwrap_or_default();
        let data = self.0.lock().unwrap().handle(op, &body["variables"]);
        match data.get("__error") {
            Some(e) => {
                ResponseTemplate::new(200).set_body_json(json!({ "errors": [{ "message": e }] }))
            }
            None => ResponseTemplate::new(200).set_body_json(json!({ "data": data })),
        }
    }
}
