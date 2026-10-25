//! BR01: cold rejected ledgers stay outside real aggregate reads and session context.
#![deny(clippy::too_many_lines)]
use super::{
    fixture::{BoardFixture, accepted, at, fields, header, human, identity},
    notification_peer::NotificationPeer,
    registration_orphan,
    repository_fixture::attach_repository,
    role_checks::Roles,
};
use codex_router_test_support::{FixtureResult, scenario_finalization::run_scenario};
use collaboration_client::protocol::{EndpointId, EndpointRef, SessionRef};
use serde_json::{Value, json};

const SENTINEL: &str = "BR01 independent healthy scope sentinel";

struct ScopeCase {
    reader: Value,
    session: SessionRef,
    fact: String,
    post: String,
    cursor: Value,
    context: Value,
    searches: Vec<Value>,
}

pub async fn rejected_scope_and_context(fixture: &mut BoardFixture) -> FixtureResult {
    // Wrong builds: orphan selection opens damaged storage, clears healthy scopes, or drops context.
    fixture.create().await?;
    let repository = attach_repository(fixture).await?;
    let installation = fixture.installation.as_ref().ok_or("installation absent")?;
    let endpoint = EndpointRef {
        service_id: installation.service_id.clone(),
        endpoint_id: EndpointId::try_from("claude-local".to_owned())?,
    };
    let peer = NotificationPeer::start_in_repository(
        &fixture.directory,
        endpoint,
        identity().try_into()?,
        &repository,
    )
    .await?;
    run_scenario(
        peer,
        async |peer: &mut NotificationPeer| {
            let case = prepare(fixture, peer.target.clone()).await?;
            observe(fixture, &case).await?;
            registration_orphan::directory_collision(fixture).await?;
            let rejected = registration_orphan::arrange(fixture).await?;
            observe(fixture, &case).await?;
            fixture.stop_host().await?;
            fixture.reopen_host().await?;
            observe(fixture, &case).await?;
            fixture.stop_host().await?;
            registration_orphan::damage_after_stop(fixture, &rejected).await?;
            fixture.reopen_host().await?;
            observe(fixture, &case).await?;
            let replay = fixture
                .host()?
                .mcp("project_create", rejected.clone())
                .await?;
            assert_eq!(
                replay.pointer("/result/kind"),
                Some(&json!("projectCreationPending"))
            );
            assert_eq!(
                replay.pointer("/result/decision/project"),
                rejected.get("projectId")
            );
            assert_eq!(
                std::fs::read(registration_orphan::orphan_path(fixture, &rejected)?)?,
                vec![b'X'; 8192]
            );
            acknowledge(fixture, &case).await
        },
        NotificationPeer::shutdown,
        || Ok(()),
    )
    .await
}

async fn summary(fixture: &BoardFixture, reader: &Value) -> FixtureResult<Value> {
    let reply = fixture
        .host()?
        .mcp(
            "inbox_workspaces",
            json!({"reader":reader,"unreadOnly":false,"page":{"limit":100,"cursor":null}}),
        )
        .await?;
    assert_eq!(reply.pointer("/page/nextCursor"), Some(&Value::Null));
    at(&reply, "/page/records").cloned()
}

fn expected_summary(fixture: &BoardFixture, initialized: bool, unread: bool) -> Value {
    json!([{"workspaceId":fixture.workspace,"mainTrackingInitialized":initialized,"hasUnread":unread}])
}

async fn prepare(fixture: &BoardFixture, session: SessionRef) -> FixtureResult<ScopeCase> {
    let reader = human("rejected-scope-reader");
    assert_eq!(summary(fixture, &reader).await?, json!([]));
    fixture
        .accept(
            "project_watch",
            fields(header(&reader), json!({"projectId":fixture.project}))?,
        )
        .await?;
    assert_eq!(
        summary(fixture, &reader).await?,
        expected_summary(fixture, false, false)
    );
    fixture.initialize(&reader, &fixture.project).await?;
    assert_eq!(
        summary(fixture, &reader).await?,
        expected_summary(fixture, true, false)
    );
    let mut roles = Roles::new();
    roles
        .grant(
            fixture,
            &fixture.owner,
            "workstream",
            "implementer",
            &json!({"kind":"session","session":session}),
            json!("changed"),
        )
        .await?;
    let fact = identity();
    fixture
        .accept(
            "fact_record",
            fixture.request(
                &fixture.owner,
                json!({"kind":"factRecord","fact":{"factId":fact,
        "kind":{"kind":"scopeAcknowledgement","fact":{"session":session,
            "subject":{"kind":"workstream","subject":fixture.workstream},
            "paths":["src/verification.rs"],"reason":"owned scope input"}},
        "links":[{"kind":"workstream","id":fixture.workstream}],"producerTime":chrono::Utc::now(),"anchor":null}}),
            ),
        )
        .await?;
    let post = identity();
    fixture.accept("thread_create", fields(header(&fixture.owner), json!({"postId":post,
        "projectId":fixture.project,"about":{"kind":"project","subject":{"line":"healthy scope sentinel"}},
        "text":SENTINEL,"references":[],"addressees":[],"role":null,"watch":false}))?).await?;
    let shown = fixture
        .host()?
        .mcp("post_show", json!({"postId":post}))
        .await?;
    assert_eq!(shown.get("text"), Some(&json!(SENTINEL)));
    assert_eq!(shown.get("projectId"), Some(&json!(fixture.project)));
    let placement: message_board::RecordPlacement =
        serde_json::from_value(at(&shown, "/position")?.clone())?;
    let cursor = serde_json::to_value(placement.project_cursor().ok_or("sentinel cursor absent")?)?;
    let context = context(fixture, &session, &fact).await?;
    let mut case = ScopeCase {
        reader,
        session,
        fact,
        post,
        cursor,
        context,
        searches: Vec::new(),
    };
    case.searches = searches(fixture, &case).await?;
    Ok(case)
}

async fn context(fixture: &BoardFixture, session: &SessionRef, fact: &str) -> FixtureResult<Value> {
    let context = fixture
        .host()?
        .mcp("view_session_context", json!({"session":session}))
        .await?;
    let _: message_board::SessionContext = serde_json::from_value(context.clone())?;
    assert_eq!(context.get("kind"), Some(&json!("context")));
    assert_eq!(context.pointer("/payload/session"), Some(&json!(session)));
    assert_eq!(
        context.pointer("/payload/assignments"),
        Some(&json!([{"kind":"implementer","subject":fixture.workstream}]))
    );
    assert_eq!(
        context.pointer("/payload/acknowledgedScopes"),
        Some(&json!([fact]))
    );
    Ok(context)
}

async fn searches(fixture: &BoardFixture, case: &ScopeCase) -> FixtureResult<Vec<Value>> {
    let scopes = [
        json!({"kind":"board","boardId":fixture.board}),
        json!({"kind":"workspace","workspaceId":fixture.workspace}),
        json!({"kind":"allWorkspaces"}),
    ];
    let mut pages = Vec::new();
    for scope in scopes {
        for archived in [false, true] {
            let page = fixture.host()?.mcp("post_search", json!({"query":SENTINEL,
                "scope":scope,"kind":"both","includeArchived":archived,"page":{"limit":1,"cursor":null}})).await?;
            assert_eq!(page.pointer("/page/nextCursor"), Some(&Value::Null));
            let records = at(&page, "/page/records")?
                .as_array()
                .ok_or("search rows absent")?;
            assert_eq!(records.len(), 1);
            let hit = records.first().ok_or("healthy sentinel absent")?;
            assert_eq!(hit.get("workspaceId"), Some(&json!(fixture.workspace)));
            assert_eq!(hit.pointer("/post/postId"), Some(&json!(case.post)));
            assert_eq!(hit.pointer("/post/text"), Some(&json!(SENTINEL)));
            let listed = fixture
                .host()?
                .mcp(
                    "post_list",
                    json!({"scope":scope,
                "selection":{"kind":"latest"},"page":{"limit":100,"cursor":null}}),
                )
                .await?;
            assert_eq!(listed.pointer("/page/unavailable"), Some(&json!([])));
            assert_eq!(listed.pointer("/page/nextCursor"), Some(&Value::Null));
            let listed = at(&listed, "/page/records")?
                .as_array()
                .ok_or("listed posts absent")?;
            assert_eq!(listed.len(), 1);
            assert_eq!(
                listed.first().and_then(|row| row.get("postId")),
                Some(&json!(case.post))
            );
            pages.push(page);
        }
    }
    Ok(pages)
}

async fn observe(fixture: &BoardFixture, case: &ScopeCase) -> FixtureResult {
    assert_eq!(searches(fixture, case).await?, case.searches);
    assert_eq!(
        context(fixture, &case.session, &case.fact).await?,
        case.context
    );
    assert_eq!(
        summary(fixture, &case.reader).await?,
        expected_summary(fixture, true, true)
    );
    let inbox = fixture
        .inbox(&case.reader, json!({"readMode":"latest"}))
        .await?;
    assert_eq!(inbox.pointer("/page/unavailable"), Some(&json!([])));
    let threads = fixture
        .host()?
        .mcp(
            "thread_list",
            json!({"workspaceId":fixture.workspace,
        "reader":case.reader,"watchedOnly":false,"page":{"limit":100,"cursor":null}}),
        )
        .await?;
    let threads: message_board::ThreadListResult = serde_json::from_value(threads)?;
    assert_eq!(threads.page.next_cursor, None);
    assert_eq!(threads.page.records.len(), 1);
    let thread = threads
        .page
        .records
        .first()
        .ok_or("healthy thread absent")?;
    assert_eq!(thread.thread.thread_id.as_str(), case.post);
    assert_eq!(thread.project_id.as_str(), fixture.project);
    Ok(())
}

async fn acknowledge(fixture: &BoardFixture, case: &ScopeCase) -> FixtureResult {
    let entry = json!({"requestId":identity(),"scope":{"kind":"project","projectId":fixture.project},
        "through":case.cursor});
    let reply = fixture
        .cli(&[
            "inbox",
            "acknowledge",
            "--actor",
            &case.reader.to_string(),
            "--entry",
            &entry.to_string(),
        ])
        .await?;
    let outcomes = at(&reply, "/record/outcomes")?
        .as_array()
        .ok_or("ACK outcomes absent")?;
    assert_eq!(outcomes.len(), 1);
    let outcome = outcomes.first().ok_or("ACK outcome absent")?;
    accepted(at(outcome, "/outcome/reply")?)?;
    assert_eq!(
        summary(fixture, &case.reader).await?,
        expected_summary(fixture, true, false)
    );
    Ok(())
}
