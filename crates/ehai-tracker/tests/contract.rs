//! Shared tracker contract (spec §5.1): every adapter must pass `contract()`.
//! The GitHub adapter runs against `FakeGithub`, a small stateful GraphQL server
//! on wiremock that models the parts of Projects v2 ehai uses — including the
//! rule that a card loses its value when its single-select option id disappears.

use ehai_tracker::fake::FakeGithub;
use ehai_tracker::github::{Client, GithubProjects};
use ehai_tracker::memory::MemoryTracker;
use ehai_tracker::{Error, Filter, ItemPatch, Kind, NewItem, Priority, Size, Status, Tracker};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn contract<T: Tracker>(t: &T) {
    // schema: first run creates, second is a no-op
    let first = t.ensure_schema().await.unwrap();
    assert!(!first.is_noop(), "{first:?}");
    assert!(t.ensure_schema().await.unwrap().is_noop());

    // create round-trips every field and the ehai-id
    let a = t
        .create_item(NewItem {
            ehai_id: Some("story-1".into()),
            title: "Book an appointment".into(),
            body: "As a patient I can book.\n".into(),
            status: Some(Status::Backlog),
            priority: Some(Priority::P1),
            size: Some(Size::S),
            kind: Some(Kind::Story),
        })
        .await
        .unwrap();
    assert_eq!(a.ehai_id.as_deref(), Some("story-1"));
    assert_eq!(
        a.body, "As a patient I can book.",
        "footer hidden from callers"
    );
    assert_eq!(
        (a.status, a.priority, a.size, a.kind),
        (
            Some(Status::Backlog),
            Some(Priority::P1),
            Some(Size::S),
            Some(Kind::Story)
        )
    );
    assert_eq!(t.get_item(&a.id).await.unwrap(), a);

    // defaults: no status → Todo; no ehai-id is fine
    let b = t
        .create_item(NewItem {
            title: "Fix login bug".into(),
            kind: Some(Kind::Bug),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(b.status, Some(Status::Todo));
    assert_eq!(b.ehai_id, None);
    let c = t
        .create_item(NewItem {
            ehai_id: Some("task-9".into()),
            title: "Third".into(),
            ..Default::default()
        })
        .await
        .unwrap();

    // update keeps the ehai-id and only touches what the patch names
    let a2 = t
        .update_item(
            &a.id,
            ItemPatch {
                title: Some("Book a visit".into()),
                body: Some("New body".into()),
                priority: Some(Priority::P0),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        (a2.title.as_str(), a2.body.as_str()),
        ("Book a visit", "New body")
    );
    assert_eq!(a2.ehai_id.as_deref(), Some("story-1"));
    assert_eq!((a2.priority, a2.size), (Some(Priority::P0), Some(Size::S)));

    t.set_status(&a.id, Status::InReview).await.unwrap();
    assert_eq!(
        t.get_item(&a.id).await.unwrap().status,
        Some(Status::InReview)
    );

    // list + filters (GitHub fake pages 2 at a time, so this covers pagination)
    assert_eq!(t.list_items(&Filter::default()).await.unwrap().len(), 3);
    let bugs = t
        .list_items(&Filter {
            kind: Some(Kind::Bug),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(bugs.iter().map(|i| &i.id).collect::<Vec<_>>(), vec![&b.id]);
    let by_id = t
        .list_items(&Filter {
            ehai_id: Some("task-9".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(by_id[0].id, c.id);

    t.comment(&a.id, "Looks good").await.unwrap();
    t.link_pr(&a.id, "https://github.com/octo/app/pull/7")
        .await
        .unwrap();

    assert!(matches!(
        t.get_item("does-not-exist").await,
        Err(Error::NotFound(_))
    ));
}

#[tokio::test]
async fn memory_tracker_passes_contract() {
    contract(&MemoryTracker::default()).await;
}

#[tokio::test]
async fn github_projects_passes_contract() {
    let fake = FakeGithub::default();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(fake.clone())
        .mount(&server)
        .await;
    let t = GithubProjects::new(
        Client::new(&server.uri(), "t0ken").unwrap(),
        "octo",
        1,
        Some("octo/app".into()),
    );
    contract(&t).await;
    assert_eq!(
        fake.comments(),
        vec![
            "Looks good",
            "Linked pull request: https://github.com/octo/app/pull/7"
        ]
    );
}

#[tokio::test]
async fn schema_update_keeps_existing_card_values() {
    let fake = FakeGithub::default();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(fake.clone())
        .mount(&server)
        .await;
    let client = Client::new(&server.uri(), "t").unwrap();

    // a card that already sits in GitHub's default "In Progress" column
    let t = GithubProjects::new(client.clone(), "octo", 1, None);
    let item = t
        .create_item(NewItem {
            title: "Existing card".into(),
            status: None,
            ..Default::default()
        })
        .await
        .unwrap();
    t.set_status(&item.id, Status::InProgress).await.unwrap();

    // ensure_schema adds Backlog / In Review to Status...
    let r = GithubProjects::new(client, "octo", 1, None)
        .ensure_schema()
        .await
        .unwrap();
    assert!(
        r.added_options.iter().any(|o| o == "Status: In Review"),
        "{r:?}"
    );
    // ...without wiping the card's column
    assert_eq!(
        t.get_item(&item.id).await.unwrap().status,
        Some(Status::InProgress)
    );
}

#[tokio::test]
async fn draft_items_without_repo_reject_comments() {
    let fake = FakeGithub::default();
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/graphql"))
        .respond_with(fake)
        .mount(&server)
        .await;
    let t = GithubProjects::new(Client::new(&server.uri(), "t").unwrap(), "octo", 1, None);
    t.ensure_schema().await.unwrap();
    let d = t
        .create_item(NewItem {
            ehai_id: Some("d-1".into()),
            title: "Draft".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(d.ehai_id.as_deref(), Some("d-1"));
    assert!(matches!(
        t.comment(&d.id, "hi").await,
        Err(Error::Unsupported(_))
    ));
}

#[tokio::test]
async fn retries_transient_errors_then_succeeds() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(502))
        .up_to_n_times(2)
        .with_priority(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": { "viewer": { "login": "octo", "organizations": { "nodes": [{ "login": "acme" }] } } } })))
        .with_priority(2)
        .mount(&server)
        .await;
    let v = Client::new(&server.uri(), "t")
        .unwrap()
        .viewer()
        .await
        .unwrap();
    assert_eq!(
        (v.login.as_str(), v.orgs),
        ("octo", vec!["acme".to_string()])
    );
}

#[tokio::test]
async fn auth_and_scope_errors_are_actionable() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "errors": [{ "type": "INSUFFICIENT_SCOPES", "message": "Your token has not been granted the required scopes" }]
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/user"))
        .respond_with(ResponseTemplate::new(200).insert_header("x-oauth-scopes", "repo, read:org"))
        .mount(&server)
        .await;
    let c = Client::new(&server.uri(), "t").unwrap();
    let e = c.projects("octo").await.unwrap_err().to_string();
    assert!(e.contains("gh auth refresh -s project"), "{e}");
    let scopes = c.scopes().await.unwrap();
    assert_eq!(
        ehai_tracker::auth::has_project_scope(scopes.as_deref()),
        Some(false)
    );

    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let e = Client::new(&server.uri(), "bad")
        .unwrap()
        .viewer()
        .await
        .unwrap_err();
    assert!(matches!(e, Error::Auth(_)), "{e}");
}
