//! Hooks share the real view/check paths and never turn unavailable storage into permission.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError, accepted};
use collaboration_mcp::test_support::ServedCollaborationApi;
use collaboration_service::{CollaborationApplication, ServiceIdentity};
use message_board::*;
use serde_json::json;
use std::os::unix::fs::PermissionsExt;

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn hooks_return_one_typed_result_and_context_covers_assignments_and_scope_facts()
-> Result<(), TestError> {
    let fixture = BoardFixture::start_context().await?;
    let (project, workstream) = fixture.project().await?;
    let (second, second_work) = fixture.project().await?;
    let session: SessionRef = serde_json::from_value(
        json!({"endpoint":{"endpointId":"codex-local","serviceId":"00000000-0000-4000-8000-000000000001"},"sessionId":"00000000-0000-4000-8000-000000000003"}),
    )?;
    let holder = Identity::Session {
        session: session.clone(),
    };
    for (project, workstream) in [(&project, &workstream), (&second, &second_work)] {
        accepted(&fixture.api.call("role_grant",fixture.request(project,json!({"kind":"roleGrant","subject":{"kind":"workstream","subject":workstream},"role":"implementer","holder":holder,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}))).await?);
    }
    let fact = FactInput {
        fact_id: FactId::generate(),
        kind: FactKind::ScopeAcknowledgement {
            session: session.clone(),
            subject: TrailSubject::Workstream(workstream.clone()),
            paths: vec!["src/view.rs".to_owned()],
            reason: "checked scope".to_owned(),
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
                fixture.request(&project, json!({"kind":"factRecord","fact":fact})),
            )
            .await?,
    );
    let context = fixture
        .api
        .call("hook_context", json!({"session":session}))
        .await?;
    assert_eq!(
        context.pointer("/result/kind"),
        Some(&json!("ok")),
        "{context}"
    );
    assert_eq!(
        context.pointer("/result/payload/kind"),
        Some(&json!("context"))
    );
    let payload = context
        .pointer("/result/payload/payload")
        .ok_or("context payload missing")?;
    let assignments: Vec<Assignment> = serde_json::from_value(
        payload
            .get("assignments")
            .cloned()
            .ok_or("assignments missing")?,
    )?;
    assert!(assignments.contains(&Assignment::Implementer(workstream.clone())));
    assert!(assignments.contains(&Assignment::Implementer(second_work)));
    assert_eq!(assignments.len(), 2);
    assert_eq!(
        payload.get("acknowledgedScopes"),
        Some(&json!([fact.fact_id]))
    );
    let card=fixture.api.call("hook_card",json!({"projectId":project,"actor":holder,"subject":{"kind":"workstream","subject":workstream}})).await?;
    assert_eq!(card.pointer("/result/payload/kind"), Some(&json!("card")));
    assert_eq!(
        card.pointer("/result/payload/payload/assignment/kind"),
        Some(&json!("implementer"))
    );
    assert_eq!(
        card.pointer("/result/payload/payload/sessionState/kind"),
        Some(&json!("unavailable"))
    );
    // Having an assignment/intent is not evidence of a running session.
    assert!(
        card.pointer("/result/payload/payload/notifyIntents")
            .is_some()
    );
    assert_eq!(
        card.pointer("/result/payload/payload/notifyIntents/0/state/kind"),
        Some(&json!("emitted")),
        "an assignment's notification state must be shown independently: {card}"
    );
    assert!(card.pointer("/result/payload/payload/blockers").is_some());
    let check_request = json!({"projectId":project,"actor":fixture.actor,"moveName":"unregistered","expectedRevision":0});
    let pure = fixture
        .api
        .call("move_check", check_request.clone())
        .await?;
    let started = std::time::Instant::now();
    let check = fixture
        .api
        .call("hook_check", check_request.clone())
        .await?;
    eprintln!(
        "G6 native MCP hook_check elapsed_us={}",
        started.elapsed().as_micros()
    );
    assert_eq!(check.pointer("/result/kind"), Some(&json!("ok")));
    assert_eq!(check.pointer("/result/payload/kind"), Some(&json!("check")));
    assert_eq!(check.pointer("/result/payload/payload"), pure.get("result"));
    assert_eq!(
        check.pointer("/result/payload/payload/kind"),
        Some(&json!("rejected"))
    );
    let absent=fixture.api.call("hook_card",json!({"projectId":ProjectId::generate(),"actor":fixture.actor,"subject":{"kind":"project"}})).await?;
    assert_eq!(
        absent.pointer("/result/kind"),
        Some(&json!("unavailable")),
        "{absent}"
    );
    assert_eq!(
        absent.pointer("/result/payload/reason/kind"),
        Some(&json!("storeUnavailable"))
    );
    let directory = tempfile::tempdir()?;
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    let api = ServedCollaborationApi::start(
        directory.path(),
        CollaborationApplication::new(ServiceIdentity::new(
            "00000000-0000-4000-8000-000000000001",
            "00000000-0000-4000-8000-000000000002",
        )?),
    )
    .await?;
    let unavailable = api.call("hook_check", check_request).await?;
    assert_eq!(
        unavailable.pointer("/result/kind"),
        Some(&json!("unavailable")),
        "{unavailable}"
    );
    assert_eq!(
        unavailable.pointer("/result/payload/reason/kind"),
        Some(&json!("boardUnreachable"))
    );
    api.stop().await?;
    fixture.stop().await
}
