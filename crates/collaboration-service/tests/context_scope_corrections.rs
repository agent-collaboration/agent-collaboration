//! Hook context uses repository membership and its two fact kinds, not whole boards.
#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
pub mod board_fixture;
#[path = "support/board_read_setup.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
mod board_read_setup;
use board_fixture::{BoardFixture, TestError, accepted};
use board_read_setup::*;
use message_board::*;
use serde_json::{Value, json};
use sqlx::{ConnectOptions, Connection};

fn session() -> Result<SessionRef, TestError> {
    Ok(serde_json::from_value(
        json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"context-reader"}),
    )?)
}
async fn grant(
    fixture: &BoardFixture,
    project: &ProjectId,
    workstream: &WorkstreamId,
) -> Result<(), TestError> {
    accepted(&fixture.api.call("role_grant",fixture.request(project,json!({"kind":"roleGrant","subject":{"kind":"workstream","subject":workstream},"role":"implementer","holder":{"kind":"session","session":session()?},"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}))).await?);
    Ok(())
}
async fn assignments(fixture: &BoardFixture) -> Result<Value, TestError> {
    Ok(fixture
        .api
        .call("view_session_context", json!({"session":session()?}))
        .await?)
}
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn context_excludes_projects_outside_the_sessions_repository() -> Result<(), TestError> {
    let fixture = BoardFixture::start_context().await?;
    let (project, lane) = fixture.project().await?;
    grant(&fixture, &project, &lane).await?;
    let (_, _, foreign) = create_location(&fixture, "unrelated repo").await?;
    accepted(&fixture.api.call("role_grant",fixture.request(&foreign,json!({"kind":"roleGrant","subject":{"kind":"project","subject":foreign},"role":"lead","holder":fixture.actor,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}))).await?);
    let workstream = WorkstreamId::generate();
    accepted(&fixture.api.call("workstream_create",fixture.request(&foreign,json!({"kind":"workstreamCreate","workstreamId":workstream,"name":"foreign lane","goal":"other repository"}))).await?);
    grant(&fixture, &foreign, &workstream).await?;
    let result = assignments(&fixture).await?;
    assert_eq!(
        result.pointer("/result/kind"),
        Some(&json!("context")),
        "{result}"
    );
    assert_eq!(
        result
            .pointer("/result/payload/assignments")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1),
        "{result}"
    );
    assert!(result.pointer("/result/payload/readMarks").is_none());
    fixture.stop().await
}
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn corrupt_unrelated_project_does_not_poison_a_healthy_session_context()
-> Result<(), TestError> {
    let fixture = BoardFixture::start_context().await?;
    let (project, lane) = fixture.project().await?;
    grant(&fixture, &project, &lane).await?;
    let (_, _, foreign) = create_location(&fixture, "corrupt unrelated repo").await?;
    let store = fixture
        ._registry
        .existing_project(&foreign)
        .await?
        .ok_or("foreign")?;
    store.close_connections().await;
    std::fs::write(
        fixture
            ._directory
            .path()
            .join(format!("projects/{}.sqlite", foreign.as_str())),
        b"corrupt sibling",
    )?;
    let result = assignments(&fixture).await?;
    assert_eq!(
        result.pointer("/result/kind"),
        Some(&json!("context")),
        "{result}"
    );
    assert_eq!(
        result
            .pointer("/result/payload/assignments")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1)
    );
    fixture.stop().await
}
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn context_reads_roles_and_scope_facts_without_decoding_posts_or_feed()
-> Result<(), TestError> {
    let fixture = BoardFixture::start_context().await?;
    let (project, lane) = fixture.project().await?;
    grant(&fixture, &project, &lane).await?;
    let fact = FactId::generate();
    let input = FactInput {
        fact_id: fact.clone(),
        kind: FactKind::ScopeAcknowledgement {
            session: session()?,
            subject: TrailSubject::Workstream(lane.clone()),
            paths: vec!["src".to_owned()],
            reason: "scope read".to_owned(),
        },
        links: vec![lane.into()],
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
        json!({"text":"unrelated post"}),
    )
    .await?;
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(
            fixture
                ._directory
                .path()
                .join("projects")
                .join(project.as_str())
                .join("e1.sqlite"),
        )
        .connect()
        .await?;
    sqlx::query("UPDATE posts SET text='' WHERE post_id=?")
        .bind(post.as_str())
        .execute(&mut connection)
        .await?;
    sqlx::query("UPDATE project_feed SET content='{}' WHERE position=(SELECT MAX(position) FROM project_feed)").execute(&mut connection).await?;
    connection.close().await?;
    let result = assignments(&fixture).await?;
    assert_eq!(
        result.pointer("/result/kind"),
        Some(&json!("context")),
        "{result}"
    );
    assert_eq!(
        result
            .pointer("/result/payload/assignments")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        result.pointer("/result/payload/acknowledgedScopes"),
        Some(&json!([fact]))
    );
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    assert!(read.records(RecordKind::ReadMark).await?.is_empty());
    read.finish().await?;
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn provider_context_uses_existing_durable_session_worktrees() -> Result<(), TestError> {
    let fixture = BoardFixture::start_context().await?;
    let (project, lane) = fixture.project().await?;
    let mut providers = collaboration_runtime::ProviderOperationStore::open(
        &fixture._directory.path().join("context-providers.sqlite"),
    )
    .await?;
    for (revision, endpoint) in [(0, "claude-local"), (1, "cursor-local")] {
        let target: collaboration_protocol::SessionRef = serde_json::from_value(
            json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":endpoint},"sessionId":endpoint}),
        )?;
        providers
            .record_session(&collaboration_runtime::ProviderSessionRecord {
                target: target.clone(),
                working_directory: fixture
                    ._directory
                    .path()
                    .join("context-repository")
                    .to_str()
                    .ok_or("worktree")?
                    .to_owned()
                    .try_into()?,
                requested_policy: collaboration_protocol::ProviderRequestedPolicy {
                    access: collaboration_protocol::RouterAccess::WorkspaceWrite,
                },
                created_by: target.clone().into(),
                approver: target.clone().into(),
                updated_at_ms: 1,
            })
            .await?;
        accepted(&fixture.api.call("role_grant",fixture.request(&project,json!({"kind":"roleGrant","subject":{"kind":"workstream","subject":lane},"role":if revision==0 {"implementer"} else {"verifier"},"holder":{"kind":"session","session":target},"model":null,"lineage":null,"expectedRevision":revision,"ownerGrant":null}))).await?);
        let result = fixture
            .api
            .call("view_session_context", json!({"session":target}))
            .await?;
        assert_eq!(
            result.pointer("/result/kind"),
            Some(&json!("context")),
            "{result}"
        );
        assert_eq!(
            result
                .pointer("/result/payload/assignments")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(1)
        );
    }
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn unresolved_session_metadata_never_invents_an_allowed_context() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    fixture.project().await?;
    let result = assignments(&fixture).await?;
    assert_eq!(
        result.pointer("/error/data/kind"),
        Some(&json!("boardUnavailable")),
        "{result}"
    );
    fixture.stop().await
}
