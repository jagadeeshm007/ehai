//! The whole PM flow through the MCP tools, against a fake GitHub:
//! PRD → breakdown → sync preview → sync → re-sync (no-op) → status → triage,
//! plus the sensitive-data guard in warn and strict modes.

#![allow(clippy::field_reassign_with_default)] // test setup reads better field by field

use std::path::Path;

use ehai_core::config::{ComplianceMode, Role};
use ehai_core::Config;
use ehai_mcp::{Context, EhaiServer, Github, SensitiveDataChoice};
use rmcp::handler::server::wrapper::Parameters;
use serde_json::{json, Map, Value};

fn prd() -> Value {
    json!({
        "title": "Online appointment booking",
        "problem": "Patients can only book by phone.",
        "users": ["Patients", "Front-desk staff"],
        "goals": ["Book without calling"],
        "scope": ["Book and cancel in the app"],
        "flows": [{ "name": "Book", "mermaid": "flowchart TD\n A[Open] --> B[Pick slot]" }],
        "metrics": [{ "name": "Online bookings", "target": "60% in 3 months" }]
    })
}

fn breakdown() -> Value {
    let ac =
        json!([{ "given": "I am signed in", "when": "I open Book", "then": "I see free slots" }]);
    json!({
        "prd": "online-appointment-booking",
        "epics": [{
            "id": "e1", "title": "Self-service booking", "priority": "P1",
            "stories": [
                { "id": "s1", "title": "See free slots", "description": "As a patient, I want to see slots.",
                  "acceptance": ac, "priority": "P1", "size": "M", "platforms": ["rn", "go"],
                  "tasks": [{ "id": "t1", "title": "Slots API", "size": "S", "platforms": ["go"] }] },
                { "id": "s2", "title": "Confirm a booking", "description": "As a patient, I want to confirm.",
                  "acceptance": ac, "priority": "P0", "size": "S", "depends_on": ["s1"] }
            ]
        }]
    })
}

fn server(home: &Path, api: &str, mode: ComplianceMode) -> EhaiServer {
    let mut config = Config::default();
    config.role = Some(Role::Pm);
    config.access.project_admin = Some(true);
    config.compliance.mode = mode;
    config.tracker.github.owner = Some("octo".into());
    config.tracker.github.project = Some(1);
    config.tracker.github.repo = Some("octo/app".into());
    EhaiServer::new(Context {
        config,
        home: home.into(),
        cwd: None,
        github: Github::Fixed {
            api: api.into(),
            token: "t".into(),
        },
    })
}

fn p<T: serde::de::DeserializeOwned>(v: Value) -> Parameters<T> {
    Parameters(serde_json::from_value(v).unwrap())
}

fn val(r: Result<rmcp::Json<Map<String, Value>>, String>) -> Value {
    Value::Object(r.expect("tool ok").0)
}

#[tokio::test(flavor = "multi_thread")]
async fn plan_breakdown_sync_status_triage() {
    let gh = wiremock::MockServer::start().await;
    let fake = ehai_tracker::fake::mount(&gh).await;
    let home = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    let s = server(home.path(), &gh.uri(), ComplianceMode::Warn);

    // PRD: invalid first, then saved (also into the repo)
    let mut bad = prd();
    bad["metrics"][0]["target"] = json!("more");
    let r = val(s.save_prd(p(json!({ "prd": bad }))).await);
    assert_eq!(r["status"], "invalid");
    let r = val(s
        .save_prd(p(json!({ "prd": prd(), "repo_path": repo.path() })))
        .await);
    assert_eq!(r["status"], "ok", "{r}");
    assert!(repo
        .path()
        .join("docs/prd/online-appointment-booking.md")
        .is_file());

    // breakdown: a cycle is reported, the fixed one returns a preview
    let mut cyclic = breakdown();
    cyclic["epics"][0]["stories"][0]["depends_on"] = json!(["s2"]);
    let r = val(s.save_breakdown(p(json!({ "breakdown": cyclic }))).await);
    assert!(r["problems"].to_string().contains("cycle"), "{r}");
    let r = val(s
        .save_breakdown(p(json!({ "breakdown": breakdown() })))
        .await);
    assert_eq!(r["items"], 4);
    assert!(r["preview"].as_str().unwrap().contains("```mermaid"));

    let r = val(s.list_plans().await);
    assert_eq!(r["plans"][0]["slug"], "online-appointment-booking");
    assert_eq!(r["plans"][0]["has_breakdown"], true);

    // sync: preview writes nothing; apply creates; re-apply is a no-op
    let r = val(s
        .sync_plan(p(json!({ "prd": "online-appointment-booking" })))
        .await);
    assert_eq!(r["status"], "preview");
    assert_eq!(r["create"].as_array().unwrap().len(), 4);
    assert!(fake.item_titles().is_empty(), "preview wrote nothing");
    let r = val(s
        .sync_plan(p(
            json!({ "prd": "online-appointment-booking", "apply": true }),
        ))
        .await);
    assert_eq!(r["created"].as_array().unwrap().len(), 4, "{r}");
    assert!(r["created"][0]["url"]
        .as_str()
        .unwrap()
        .starts_with("https://github.com/"));
    let r = val(s
        .sync_plan(p(json!({ "prd": "online-appointment-booking" })))
        .await);
    assert_eq!(r["nothing_to_do"], true, "{r}");
    assert_eq!(fake.item_titles().len(), 4);

    // renaming one story updates exactly that issue (others link it by #number)
    let mut renamed = breakdown();
    renamed["epics"][0]["stories"][0]["title"] = json!("See open slots");
    val(s.save_breakdown(p(json!({ "breakdown": renamed }))).await);
    let r = val(s
        .sync_plan(p(json!({ "prd": "online-appointment-booking" })))
        .await);
    assert_eq!(r["update"], json!(["See open slots"]), "{r}");
    val(s
        .sync_plan(p(
            json!({ "prd": "online-appointment-booking", "apply": true }),
        ))
        .await);

    // status: markdown + HTML file
    let r = val(s.status_report(p(json!({ "html": true }))).await);
    assert!(r["markdown"]
        .as_str()
        .unwrap()
        .contains("Self-service booking"));
    let html = r["html_path"].as_str().unwrap();
    assert!(Path::new(html).is_file() && html.starts_with(home.path().to_str().unwrap()));

    // triage: similar item → question; "new" → created; dup → comment
    let triage = |decision: Option<&str>| {
        let mut a = json!({ "title": "Slots API returns no slots", "description": "Empty list on Mondays",
                            "kind": "Bug", "priority": "P1" });
        if let Some(d) = decision {
            a["decision"] = json!(d);
        }
        a
    };
    let r = val(s.triage(p(triage(None))).await);
    assert_eq!(r["status"], "needs_input", "{r}");
    assert_eq!(r["retry_param"], "decision");
    let dup = r["options"][0]["value"].as_str().unwrap().to_string();
    assert!(dup.starts_with("dup:"));
    let r = val(s.triage(p(triage(Some(&dup)))).await);
    assert_eq!(r["action"], "added to existing item");
    assert!(fake.comments()[0].contains("Empty list on Mondays"));
    let r = val(s.triage(p(triage(Some("new")))).await);
    assert_eq!(r["action"], "created");
    assert_eq!(fake.item_titles().len(), 5);

    // every write is in the audit log, without values
    let audit = std::fs::read_dir(home.path().join("audit"))
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
        .map(|e| std::fs::read_to_string(e.path()).unwrap())
        .collect::<String>();
    for cmd in [
        "save_prd",
        "save_breakdown",
        "sync_plan",
        "status_report",
        "triage",
    ] {
        assert!(audit.contains(cmd), "{cmd} audited");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn sensitive_data_is_caught_in_warn_and_strict() {
    let gh = wiremock::MockServer::start().await;
    ehai_tracker::fake::mount(&gh).await;
    let home = tempfile::tempdir().unwrap();
    let mut leaky = prd();
    leaky["problem"] = json!("SSN 123-45-6789 (jane@example.org) on file.");

    // warn: ask first; cancel writes nothing; redact writes a clean PRD
    let s = server(home.path(), &gh.uri(), ComplianceMode::Warn);
    let r = val(s.save_prd(p(json!({ "prd": leaky }))).await);
    assert_eq!(r["status"], "needs_input");
    assert_eq!(r["retry_param"], "policy");
    assert!(
        r["question"].as_str().unwrap().contains("ssn ×1, email ×1"),
        "{r}"
    );
    assert!(!r.to_string().contains("123-45-6789"), "values never echoed");
    let e = s
        .save_prd(p(json!({ "prd": leaky, "policy": SensitiveDataChoice::Cancel })))
        .await
        .err()
        .expect("tool error");
    assert!(e.contains("cancelled"));
    assert!(ehai_planner::list_plans(home.path()).is_empty());
    let r = val(s
        .save_prd(p(json!({ "prd": leaky, "policy": "redact" })))
        .await);
    assert_eq!(r["sensitive_data"], "redacted");
    let md = std::fs::read_to_string(home.path().join("plans/online-appointment-booking/prd.md"))
        .unwrap();
    assert!(
        !md.contains("123-45-6789")
            && !md.contains("jane@example.org")
            && md.contains("[REDACTED:ssn]")
    );

    // strict: redacted without asking
    let home2 = tempfile::tempdir().unwrap();
    let s = server(home2.path(), &gh.uri(), ComplianceMode::Strict);
    let r = val(s.save_prd(p(json!({ "prd": leaky }))).await);
    assert_eq!(
        (r["status"].as_str(), r["sensitive_data"].as_str()),
        (Some("ok"), Some("redacted"))
    );

    // guard_check reports kinds only
    let r = val(s.guard_check(p(json!({ "text": "SSN 123-45-6789" }))).await);
    assert_eq!(
        (r["clean"].as_bool(), r["found"].as_str()),
        (Some(false), Some("ssn ×1"))
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn tracker_not_connected_is_explained() {
    let home = tempfile::tempdir().unwrap();
    let mut s = server(home.path(), "http://127.0.0.1:9", ComplianceMode::Off);
    val(s
        .save_breakdown(p(json!({ "breakdown": breakdown() })))
        .await);
    let mut config = Config::default();
    config.tracker.github.owner = None;
    s = EhaiServer::new(Context {
        config,
        home: home.path().into(),
        cwd: None,
        github: Github::Fixed {
            api: "http://127.0.0.1:9".into(),
            token: "t".into(),
        },
    });
    let e = s
        .sync_plan(p(json!({ "prd": "online-appointment-booking" })))
        .await
        .err()
        .expect("tool error");
    assert!(e.contains("tracker set"), "{e}");
}

#[tokio::test(flavor = "multi_thread")]
async fn team_assign_and_comment_tools() {
    let gh = wiremock::MockServer::start().await;
    let fake = ehai_tracker::fake::mount(&gh).await;
    fake.set_assignable_users(&[("octo", Some("Octo Cat")), ("priya", Some("Priya P"))]);
    fake.set_org_members(&[("priya", Some("Priya P")), ("asha", Some("Asha A"))]);
    let home = tempfile::tempdir().unwrap();
    let s = server(home.path(), &gh.uri(), ComplianceMode::Warn);

    // team: lists the merged roster, with the viewer as admin
    let r = val(s.team(p(json!({}))).await);
    assert_eq!(r["status"], "ok");
    assert_eq!(r["viewer_is_admin"], true);
    let logins: Vec<&str> = r["members"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["login"].as_str().unwrap())
        .collect();
    assert_eq!(logins, vec!["asha", "octo", "priya"], "{r}");

    // a real item to assign/comment on
    let item = val(
        s.triage(p(json!({ "title": "Export visit history", "description": "As a PDF", "kind": "Story", "priority": "P2" })))
            .await,
    );
    assert_eq!(item["action"], "created", "{item}");
    let number: u64 = item["url"]
        .as_str()
        .unwrap()
        .rsplit('/')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let item_ref = format!("#{number}");
    // a second item, so a loose title search can be genuinely ambiguous
    val(s
        .triage(p(json!({ "title": "Export lab results", "description": "As a CSV", "kind": "Story", "priority": "P2" })))
        .await);

    // assign "me" and a teammate; unknown name asks, "priya" by fuzzy title also works
    let r = val(s
        .assign_task(p(json!({ "item": item_ref, "assignees": ["me", "priya"] })))
        .await);
    assert_eq!(r["status"], "ok", "{r}");
    let mut assignees: Vec<&str> = r["assignees"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assignees.sort();
    assert_eq!(assignees, vec!["octo", "priya"]);

    let r = val(s
        .assign_task(p(
            json!({ "item": "Export visit", "assignees": ["nobody-here"] }),
        ))
        .await);
    assert_eq!(r["status"], "needs_input", "{r}");
    assert_eq!(r["retry_param"], "assignees");

    // replace drops octo, keeps asha only
    let r = val(s
        .assign_task(p(
            json!({ "item": item_ref, "assignees": ["asha"], "mode": "replace" }),
        ))
        .await);
    assert_eq!(r["assignees"], json!(["asha"]), "{r}");

    // remove
    let r = val(s
        .assign_task(p(
            json!({ "item": item_ref, "assignees": ["asha"], "mode": "remove" }),
        ))
        .await);
    assert_eq!(r["assignees"], json!([]), "{r}");

    // ambiguous item reference
    let r = val(s
        .assign_task(p(json!({ "item": "e", "assignees": ["me"] })))
        .await);
    assert_eq!(r["status"], "needs_input");
    assert_eq!(r["retry_param"], "item");

    // comment with a mention and a PII-bearing body (warn mode asks first)
    let r = val(
        s.comment_task(p(json!({ "item": item_ref, "body": "SSN 123-45-6789 on file per priya.", "mentions": ["priya"] })))
            .await,
    );
    assert_eq!(r["status"], "needs_input");
    let r = val(
        s.comment_task(p(json!({ "item": item_ref, "body": "SSN 123-45-6789 on file per priya.", "mentions": ["priya"], "policy": "redact" })))
            .await,
    );
    assert_eq!(r["status"], "ok", "{r}");
    let last = fake.comments().last().unwrap().clone();
    assert!(last.starts_with("@priya"), "{last}");
    assert!(!last.contains("123-45-6789"), "{last}");
}
