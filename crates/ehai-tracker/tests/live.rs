//! Opt-in live test against real GitHub (spec §13): `EHAI_LIVE_GH=1 cargo test -p
//! ehai-tracker --test live -- --nocapture`. Uses the `gh` login (or GITHUB_TOKEN).
//!
//! - Without the `project` scope: read-only checks, then a note on how to enable
//!   the full run.
//! - With it: creates a throwaway project, runs the tracker flow on draft items
//!   (no issues land in any repo), and deletes the project even if a step fails.

use ehai_tracker::github::{Client, GithubProjects, API};
use ehai_tracker::{auth, Error, Filter, ItemPatch, Kind, NewItem, Priority, Status, Tracker};

#[tokio::test]
async fn live_github_projects() {
    if std::env::var("EHAI_LIVE_GH").as_deref() != Ok("1") {
        eprintln!("skipped: set EHAI_LIVE_GH=1 to run against real GitHub");
        return;
    }
    let env: Vec<(String, String)> = std::env::vars().collect();
    let token = auth::resolve(&env, true).expect("a GitHub token (gh auth login or GITHUB_TOKEN)");
    let c = Client::new(API, &token.value).unwrap();

    let me = c.viewer().await.expect("viewer query");
    eprintln!("signed in as {} via {}", me.login, token.source);

    let scopes = c.scopes().await.unwrap();
    if auth::has_project_scope(scopes.as_deref()) == Some(false) {
        let e = c.projects(&me.login).await.unwrap_err();
        assert!(
            matches!(e, Error::Auth(_)),
            "missing scope must be an actionable auth error: {e}"
        );
        assert!(e.to_string().contains(auth::SCOPE_FIX));
        eprintln!(
            "read-only checks passed. For the full run: `{}`",
            auth::SCOPE_FIX
        );
        return;
    }

    let title = format!("ehai-live-test-{}", std::process::id());
    let p = c
        .create_project(&me.login, &title)
        .await
        .expect("create project");
    eprintln!("created {} ({})", p.title, p.url);
    let result = tokio::spawn(flow(c.clone(), me.login.clone(), p.number)).await;
    c.delete_project(&me.login, p.number)
        .await
        .expect("cleanup: delete project");
    eprintln!("deleted test project #{}", p.number);
    if let Err(e) = result {
        std::panic::resume_unwind(e.into_panic());
    }
}

async fn flow(c: Client, owner: String, number: u64) {
    let listed = c.projects(&owner).await.unwrap();
    assert!(
        listed.iter().any(|p| p.number == number),
        "new project is listed"
    );

    let t = GithubProjects::new(c.clone(), &owner, number, None);
    // a card in GitHub's default In Progress column survives ensure_schema
    let pre = t
        .create_item(NewItem {
            title: "pre-existing".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    t.set_status(&pre.id, Status::InProgress).await.unwrap();

    let r = t.ensure_schema().await.unwrap();
    eprintln!("schema: {r:?}");
    let fresh = GithubProjects::new(c, &owner, number, None);
    assert!(
        fresh.ensure_schema().await.unwrap().is_noop(),
        "second ensure_schema is a no-op"
    );
    assert_eq!(
        fresh.get_item(&pre.id).await.unwrap().status,
        Some(Status::InProgress),
        "existing card kept its column"
    );

    let a = fresh
        .create_item(NewItem {
            ehai_id: Some("live-1".into()),
            title: "Live item".into(),
            body: "Created by ehai's live test".into(),
            status: Some(Status::Backlog),
            priority: Some(Priority::P2),
            kind: Some(Kind::Task),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(
        (a.ehai_id.as_deref(), a.status, a.priority),
        (Some("live-1"), Some(Status::Backlog), Some(Priority::P2))
    );
    let a = fresh
        .update_item(
            &a.id,
            ItemPatch {
                title: Some("Live item, renamed".into()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        a.ehai_id.as_deref(),
        Some("live-1"),
        "footer survives a rename"
    );
    fresh.set_status(&a.id, Status::Done).await.unwrap();
    let found = fresh
        .list_items(&Filter {
            ehai_id: Some("live-1".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(found[0].status, Some(Status::Done));
}
