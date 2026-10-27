#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! Session context and cards retain assignment and scope fact projections.
#![deny(clippy::too_many_lines)]
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError, accepted};
use message_board::*;
use serde_json::json;

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn context_covers_assignments_scope_facts_and_card_session_presence() -> Result<(), TestError>
{
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
        .call("view_session_context", json!({"session":session}))
        .await?;
    assert_eq!(
        context.pointer("/result/kind"),
        Some(&json!("context")),
        "{context}"
    );
    let payload = context
        .pointer("/result/payload")
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
    let card=fixture.api.call("view_card",json!({"projectId":project,"actor":holder,"subject":{"kind":"workstream","subject":workstream}})).await?;
    assert_eq!(
        card.pointer("/result/assignment/kind"),
        Some(&json!("implementer"))
    );
    assert_eq!(
        card.pointer("/result/sessionState/kind"),
        Some(&json!("unavailable"))
    );
    // Having an assignment/intent is not evidence of a running session.
    assert!(card.pointer("/result/notifyIntents").is_some());
    assert_eq!(
        card.pointer("/result/notifyIntents/0/state/kind"),
        Some(&json!("emitted")),
        "an assignment's notification state must be shown independently: {card}"
    );
    assert!(card.pointer("/result/blockers").is_some());
    fixture.stop().await
}
