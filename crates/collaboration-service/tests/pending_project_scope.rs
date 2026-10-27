//! Empty refused registrations leave aggregate scope without discarding healthy context.

#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
#[expect(
    clippy::indexing_slicing,
    reason = "This test consumes a subset of the shared fixture and retains its fixed JSON construction."
)]
pub mod board_fixture;
#[path = "support/board_read_setup.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
mod board_read_setup;
#[path = "support/read_cost_counter.rs"]
#[expect(
    unused_imports,
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
mod read_cost_counter;
use board_fixture::{BoardFixture, TestError, accepted};
use board_read_setup::{initialized_request, keyed_call, post_at_placement};
use message_board::*;
use serde_json::{Value, json};

async fn reject_and_move_creation(
    fixture: &BoardFixture,
    project: &ProjectId,
    request: Value,
) -> Result<Value, TestError> {
    let creation: ProjectCreateRequest = serde_json::from_value(request.clone())?;
    board_fixture::settle_accepted_creation(&fixture._fleet, &creation, &fixture.record_origin()?)
        .await?;
    // The directory policy and this native gate refusal are controlled inputs.
    // No application row is written before the empty database is moved aside.
    let store = fixture._registry.open_project(project).await?;
    let mut read = store.snapshot().await?;
    assert!(read.records(RecordKind::Project).await?.is_empty());
    assert!(read.records(RecordKind::Request).await?.is_empty());
    read.finish().await?;
    drop(store);
    let database = fixture._registry.database(project).await?;
    database
        .reject_registration(fleet_sync_protocol::GateReason {
            code: fleet_sync_protocol::GateReasonCode::new("consumer.refused")?,
            detail: "projection fixture registration refused".to_owned(),
        })
        .await?;
    drop(database);
    fixture._registry.orphan_project(project).await?;
    Ok(fixture.api.call("project_create", request).await?)
}

async fn reject_registration_fixture(fixture: &BoardFixture) -> Result<ProjectId, TestError> {
    let rejected = ProjectId::generate();
    let mut request = fixture.header();
    request.as_object_mut().ok_or("creation header")?.extend(
        json!({"projectId":rejected,"boardId":fixture.board,"title":rejected.as_str()})
            .as_object()
            .ok_or("creation fields")?
            .clone(),
    );
    let reply = reject_and_move_creation(fixture, &rejected, request).await?;
    assert_eq!(
        reply.pointer("/result/result/decision/kind"),
        Some(&json!("projectRejected"))
    );
    assert!(
        fixture
            ._registry
            .rejected_project(&rejected)
            .await?
            .is_some()
    );
    Ok(rejected)
}
fn search(scope: PostListScope, archived: bool) -> Result<PostSearchRequest, TestError> {
    Ok(PostSearchRequest {
        query: SearchQuery::try_from("scope sentinel".to_owned())?,
        scope,
        kind: SearchPostKind::Both,
        include_archived: archived,
        page: PageRequest {
            limit: PageLimit::try_from(1)?,
            cursor: None,
        },
    })
}
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn rejected_creations_leave_every_search_scope_and_archive_mode() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let client = fixture.api.client("pending scopes").await?;
    let (project, _) = fixture.project().await?;
    let post = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: project.clone(),
        },
        json!({"text":"scope sentinel"}),
    )
    .await?;
    let scopes = [
        PostListScope::Board {
            board_id: fixture.board.clone(),
        },
        PostListScope::Workspace {
            workspace_id: fixture.workspace.clone(),
        },
        PostListScope::AllWorkspaces,
        PostListScope::Project {
            project_id: project.clone(),
        },
    ];
    let mut expected = Vec::new();
    for scope in &scopes {
        for archived in [false, true] {
            let page = client
                .post_search(search(scope.clone(), archived)?)
                .await?
                .page;
            assert_eq!(page.records.len(), 1);
            assert_eq!(page.records[0].post.post_id, post);
            assert_eq!(page.records[0].post.text.as_str(), "scope sentinel");
            assert_eq!(page.records[0].workspace_id, fixture.workspace);
            assert!(page.next_cursor.is_none());
            expected.push(page);
        }
    }
    reject_registration_fixture(&fixture).await?;
    let mut expected = expected.into_iter();
    for scope in scopes {
        for archived in [false, true] {
            assert_eq!(
                client
                    .post_search(search(scope.clone(), archived)?)
                    .await?
                    .page,
                expected.next().ok_or("expected page")?
            );
        }
    }
    fixture.stop().await
}
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn rejected_creation_keeps_initialized_workspace_summary() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let client = fixture.api.client("pending summary").await?;
    let (project, _) = fixture.project().await?;
    let reader: Identity = serde_json::from_value(fixture.actor.clone())?;
    client
        .inbox_fetch(initialized_request(project.clone(), reader.clone())?)
        .await?;
    let request = InboxWorkspacesRequest {
        reader,
        unread_only: false,
        page: PageRequest::default(),
    };
    let before = client.inbox_workspaces(request.clone()).await?;
    let own = before
        .page
        .records
        .iter()
        .find(|row| row.workspace_id == fixture.workspace)
        .ok_or("initialized workspace")?;
    assert!(own.main_tracking_initialized);
    reject_registration_fixture(&fixture).await?;
    assert_eq!(client.inbox_workspaces(request).await?, before);
    fixture.stop().await
}
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn rejected_creation_preserves_nonempty_repository_context() -> Result<(), TestError> {
    let fixture = BoardFixture::start_context().await?;
    let (project, workstream) = fixture.project().await?;
    let session: SessionRef = serde_json::from_value(
        json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"context-reader"}),
    )?;
    accepted(&fixture.api.call("role_grant",fixture.request(&project,json!({"kind":"roleGrant","subject":{"kind":"workstream","subject":workstream},"role":"implementer","holder":{"kind":"session","session":session},"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}))).await?);
    let fact = FactId::generate();
    let input = FactInput {
        fact_id: fact.clone(),
        kind: FactKind::ScopeAcknowledgement {
            session: session.clone(),
            subject: TrailSubject::Workstream(workstream.clone()),
            paths: vec!["src".to_owned()],
            reason: "scope read".to_owned(),
        },
        links: vec![workstream.clone().into()],
        producer_time: chrono::Utc::now(),
        anchor: None,
    };
    accepted(
        &fixture
            .api
            .call(
                "fact_record",
                fixture.request(&project, json!({"kind":"factRecord","fact":input})),
            )
            .await?,
    );
    let before = fixture
        .api
        .call("view_session_context", json!({"session":session}))
        .await?;
    assert_eq!(
        before.pointer("/result/kind"),
        Some(&json!("context")),
        "{before}"
    );
    let payload = before.pointer("/result/payload").ok_or("context payload")?;
    let assignments = payload
        .get("assignments")
        .and_then(Value::as_array)
        .ok_or("assignments")?;
    assert_eq!(assignments.len(), 1);
    assert!(assignments[0].to_string().contains(workstream.as_str()));
    assert_eq!(payload.get("acknowledgedScopes"), Some(&json!([fact])));
    reject_registration_fixture(&fixture).await?;
    let after = fixture
        .api
        .call("view_session_context", json!({"session":session}))
        .await?;
    assert_eq!(
        after.pointer("/result/kind"),
        Some(&json!("context")),
        "{after}"
    );
    assert_eq!(
        after.pointer("/result/payload/assignments"),
        payload.get("assignments")
    );
    assert_eq!(
        after.pointer("/result/payload/acknowledgedScopes"),
        payload.get("acknowledgedScopes")
    );
    fixture.stop().await
}
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn cold_corrupt_rejected_file_is_excluded_without_reading_it() -> Result<(), TestError> {
    read_cost_counter::install();
    let fixture = BoardFixture::start_context().await?;
    let (project, workstream) = fixture.project().await?;
    let session: SessionRef = serde_json::from_value(json!({
        "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},
        "sessionId":"context-reader"
    }))?;
    accepted(
        &fixture
            .api
            .call(
                "role_grant",
                fixture.request(
                    &project,
                    json!({
                        "kind":"roleGrant","subject":{"kind":"workstream","subject":workstream},
                        "role":"implementer","holder":{"kind":"session","session":session},
                        "model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null
                    }),
                ),
            )
            .await?,
    );
    let fact = FactId::generate();
    let input = FactInput {
        fact_id: fact.clone(),
        kind: FactKind::ScopeAcknowledgement {
            session: session.clone(),
            subject: TrailSubject::Workstream(workstream.clone()),
            paths: vec!["src".to_owned()],
            reason: "cold scope proof".to_owned(),
        },
        links: vec![workstream.clone().into()],
        producer_time: chrono::Utc::now(),
        anchor: None,
    };
    accepted(
        &fixture
            .api
            .call(
                "fact_record",
                fixture.request(&project, json!({"kind":"factRecord","fact":input})),
            )
            .await?,
    );
    let post = post_at_placement(
        &fixture,
        Placement::Project {
            project_id: project.clone(),
        },
        json!({"text":"scope sentinel"}),
    )
    .await?;
    let root = PostId::generate();
    accepted(
        &keyed_call(
            &fixture,
            "thread_create",
            json!({
                "projectId":project,"postId":root,"about":fixture.discussion_subject(&project).await?,"text":"healthy context thread",
                "references":[],"role":null,"watch":false,"addressees":[],
                "actor":{"kind":"human","humanId":"thread-author"}
            }),
        )
        .await?,
    );
    let reader: Identity = serde_json::from_value(fixture.actor.clone())?;
    let client = fixture.api.client("cold rejected scope").await?;
    client
        .inbox_fetch(initialized_request(project.clone(), reader.clone())?)
        .await?;
    let summary_request = InboxWorkspacesRequest {
        reader: reader.clone(),
        unread_only: false,
        page: PageRequest::default(),
    };
    let summary = client.inbox_workspaces(summary_request.clone()).await?;
    assert_eq!(summary.page.records.len(), 1);
    assert_eq!(summary.page.records[0].workspace_id, fixture.workspace);
    assert!(summary.page.records[0].main_tracking_initialized);
    let context = fixture
        .api
        .call("view_session_context", json!({"session":session}))
        .await?;
    assert_eq!(
        context.pointer("/result/kind"),
        Some(&json!("context")),
        "{context}"
    );
    let assignments = context
        .pointer("/result/payload/assignments")
        .ok_or("assignments")?
        .clone();
    assert_eq!(assignments.as_array().map(Vec::len), Some(1));
    assert!(assignments.to_string().contains(workstream.as_str()));
    assert_eq!(
        assignments,
        json!([Assignment::Implementer(workstream.clone())])
    );
    assert_eq!(
        context.pointer("/result/payload/acknowledgedScopes"),
        Some(&json!([fact.clone()]))
    );
    let scopes = [
        PostListScope::Board {
            board_id: fixture.board.clone(),
        },
        PostListScope::Workspace {
            workspace_id: fixture.workspace.clone(),
        },
        PostListScope::AllWorkspaces,
    ];
    let mut expected = Vec::new();
    for scope in &scopes {
        for archived in [false, true] {
            let page = client
                .post_search(search(scope.clone(), archived)?)
                .await?
                .page;
            assert_eq!(page.records.len(), 1);
            assert_eq!(page.records[0].post.post_id, post);
            assert_eq!(page.records[0].post.text.as_str(), "scope sentinel");
            expected.push(page);
        }
    }
    let thread_request = ThreadListRequest {
        workspace_id: fixture.workspace.clone(),
        reader: reader.clone(),
        watched_only: false,
        page: PageRequest::default(),
    };
    let threads = client.thread_list(thread_request.clone()).await?;
    let mut thread_ids: Vec<_> = threads
        .page
        .records
        .iter()
        .map(|row| row.thread.thread_id.clone())
        .collect();
    thread_ids.sort();
    let mut expected_threads = vec![ThreadId::from(post.clone()), ThreadId::from(root.clone())];
    expected_threads.sort();
    assert_eq!(thread_ids, expected_threads);
    let rejected = ProjectId::generate();
    let mut creation = fixture.header();
    creation.as_object_mut().ok_or("request")?.extend(
        json!({
            "projectId":rejected,"boardId":fixture.board,"title":rejected.as_str()
        })
        .as_object()
        .ok_or("creation")?
        .clone(),
    );
    let reply = reject_and_move_creation(&fixture, &rejected, creation.clone()).await?;
    assert_eq!(
        reply.pointer("/result/result/decision/kind"),
        Some(&json!("projectRejected")),
        "{reply}"
    );
    let orphan = format!("projects/{}/e1.orphaned.sqlite", rejected.as_str());
    drop(client);
    let fixture = fixture.restart_read_fixture().await?;
    let intact_opens = read_cost_counter::project_open_attempts(rejected.as_str());
    let client = fixture.api.client("intact cold rejected scope").await?;
    let mut expected_intact = expected.iter();
    for scope in &scopes {
        for archived in [false, true] {
            assert_eq!(
                &client
                    .post_search(search(scope.clone(), archived)?)
                    .await?
                    .page,
                expected_intact.next().ok_or("expected intact scope")?
            );
        }
    }
    assert_eq!(
        read_cost_counter::project_open_attempts(rejected.as_str()),
        intact_opens,
        "excluding an intact cold orphan must not open its ledger either"
    );
    drop(client);
    let fixture = fixture
        .restart_read_fixture_after_close(|directory| {
            assert!(
                directory.join(&orphan).is_file(),
                "fault must target the real moved-aside file"
            );
            std::fs::write(directory.join(&orphan), b"invalid native rejected metadata")?;
            Ok(())
        })
        .await?;
    let orphan_path = fixture._directory.path().join(&orphan);
    let before_probe = read_cost_counter::project_open_attempts(rejected.as_str());
    assert!(
        message_board_storage::ProjectLedgerReader::open(
            &orphan_path,
            rejected.clone(),
            board_fleet_sync::BoardProjectSchema::new()?
        )
        .await
        .is_err(),
        "the cold fixture must actually refuse direct native ledger opening"
    );
    assert_eq!(
        read_cost_counter::project_open_attempts(rejected.as_str()),
        before_probe + 1,
        "the open-attempt instrument observes the rejected-ledger boundary"
    );
    let before_scope = read_cost_counter::project_open_attempts(rejected.as_str());
    let before_cost = read_cost_counter::current_cost();
    let client = fixture.api.client("cold rejected scope reopened").await?;
    let mut expected = expected.into_iter();
    for scope in scopes {
        for archived in [false, true] {
            let actual = client.post_search(search(scope.clone(),archived)?).await.map_err(|error|
                format!("cold scope {scope:?}, archived={archived}: {error}; rejected ledger open attempts={}",
                    read_cost_counter::project_open_attempts(rejected.as_str()) - before_scope))?;
            assert_eq!(actual.page, expected.next().ok_or("expected search")?);
        }
        let listed = client
            .post_list(PostListRequest {
                scope,
                selection: PostSelection::Latest,
                page: PageRequest::default(),
            })
            .await?
            .page;
        assert!(
            listed.unavailable.is_empty(),
            "a rejected creation is not an unavailable project"
        );
        let mut ids: Vec<_> = listed
            .records
            .into_iter()
            .map(|post| post.post_id)
            .collect();
        ids.sort();
        let mut wanted = vec![post.clone(), root.clone()];
        wanted.sort();
        assert_eq!(ids, wanted);
    }
    assert_eq!(client.thread_list(thread_request).await?, threads);
    assert_eq!(client.inbox_workspaces(summary_request).await?, summary);
    let inbox = client
        .inbox_fetch(board_read_setup::inbox_request(
            InboxScope::Workspace {
                workspace_id: fixture.workspace.clone(),
            },
            reader,
            InboxReadMode::Latest,
        )?)
        .await?
        .page;
    assert!(inbox.unavailable.is_empty());
    let context = fixture
        .api
        .call("view_session_context", json!({"session":session}))
        .await?;
    assert_eq!(
        context.pointer("/result/kind"),
        Some(&json!("context")),
        "{context}"
    );
    assert_eq!(
        context.pointer("/result/payload/assignments"),
        Some(&assignments)
    );
    assert_eq!(
        context.pointer("/result/payload/acknowledgedScopes"),
        Some(&json!([fact]))
    );
    assert_eq!(
        read_cost_counter::project_open_attempts(rejected.as_str()),
        before_scope,
        "scope selection must never open an excluded orphan ledger"
    );
    eprintln!(
        "cold orphan aggregate read cost: {:?}; excluded ledger opens=0",
        read_cost_counter::current_cost().since(before_cost)
    );
    assert!(
        fixture._registry.rejected_project(&rejected).await.is_err(),
        "direct rejected-ledger inspection stays strict"
    );
    let replay = fixture.api.call("project_create", creation).await?;
    assert!(
        replay.pointer("/result/result/kind") == Some(&json!("projectCreationPending")),
        "durably accepted damaged creation must remain pending: {replay}"
    );
    assert_eq!(
        replay.pointer("/result/result/decision/project"),
        Some(&json!(rejected))
    );
    assert!(read_cost_counter::project_open_attempts(rejected.as_str()) >= before_scope + 2);
    assert_eq!(
        std::fs::read(orphan_path)?,
        b"invalid native rejected metadata"
    );
    drop(client);
    fixture.stop().await
}
