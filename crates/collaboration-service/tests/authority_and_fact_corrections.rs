#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! Fresh request authority and producer-fact boundaries use real public admission.
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
use board_fixture::{BoardFixture, TestError, accepted, refusal_kind};
use message_board::*;
use serde_json::{Value, json};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn unauthorized_same_holder_grants_reject_and_replay_before_authorized_noops()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let implementer = json!({"kind":"human","humanId":"implementer"});
    let pair = json!({"kind":"roleGrant","subject":{"kind":"workstream","subject":workstream},
        "role":"implementer","holder":implementer,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null});
    accepted(
        &fixture
            .api
            .call("role_grant", fixture.request(&project, pair.clone()))
            .await?,
    );
    for mut operation in [
        pair,
        json!({"kind":"roleGrant","subject":{"kind":"project","subject":project},
        "role":"lead","holder":fixture.actor,"model":null,"lineage":null,"expectedRevision":1,"ownerGrant":null}),
    ] {
        operation["expectedRevision"] = json!(1);
        let mut outsider = fixture.request(&project, operation.clone());
        outsider["actor"] = json!({"kind":"human","humanId":"outsider"});
        let denied = fixture.api.call("role_grant", outsider.clone()).await?;
        assert_eq!(refusal_kind(&denied), Some("roleRequired"), "{denied}");
        let replayed = fixture.api.call("role_grant", outsider).await?;
        assert_eq!(
            denied.pointer("/result/result"),
            replayed.pointer("/result/result")
        );
        let allowed = fixture
            .api
            .call("role_grant", fixture.request(&project, operation))
            .await?;
        accepted(&allowed);
        assert_eq!(
            allowed.pointer("/result/result/decision/change"),
            Some(&json!("alreadySo"))
        );
    }
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn most_recent_released_lead_can_return_only_by_self_grant_without_owner_grant()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    accepted(&fixture.api.call("role_release", fixture.request(&project, json!({
        "kind":"roleRelease","subject":{"kind":"project","subject":project},"role":"lead","expectedRevision":1
    }))).await?);
    let operation = json!({"kind":"roleGrant","subject":{"kind":"project","subject":project},
        "role":"lead","holder":fixture.actor,"model":null,"lineage":null,"expectedRevision":2,"ownerGrant":null});
    let mut other = fixture.request(&project, operation.clone());
    other["actor"] = json!({"kind":"human","humanId":"outsider"});
    assert_eq!(
        refusal_kind(&fixture.api.call("role_grant", other).await?),
        Some("roleRequired")
    );
    accepted(
        &fixture
            .api
            .call("role_grant", fixture.request(&project, operation))
            .await?,
    );
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let roles = read.records(RecordKind::RoleAssignment).await?;
    assert_eq!(roles.len(), 2);
    assert_eq!(
        roles
            .iter()
            .filter(|record| matches!(record, Record::RoleAssignment(role) if role.ended.is_none()))
            .count(),
        1
    );
    assert!(read.records(RecordKind::OwnerGrant).await?.is_empty());
    assert_eq!(
        read.assignment_revision(&RoleSubject::Project(project))
            .await?
            .get(),
        3
    );
    read.finish().await?;
    fixture.stop().await
}

fn evaluator(fixture: &BoardFixture) -> Result<EvaluatorRef, TestError> {
    Ok(EvaluatorRef {
        actor: RecordedActor::declared(serde_json::from_value(fixture.actor.clone())?),
        engine: None,
        calibration: None,
    })
}
async fn submit_fact(
    fixture: &BoardFixture,
    project: &ProjectId,
    kind: FactKind,
    links: Vec<RecordRef>,
) -> Result<Value, TestError> {
    Ok(fixture.api.call("fact_record", fixture.request(project, json!({"kind":"factRecord","fact":FactInput {
        fact_id: FactId::generate(), kind, links, producer_time: chrono::Utc::now(), anchor: None,
    }}))).await?)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn every_lifecycle_fact_is_operation_owned_and_rejected_from_producers()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let shown = fixture
        .api
        .call("project_show", json!({"projectId":project}))
        .await?;
    let directory: ProjectShowResult =
        serde_json::from_value(shown.get("result").cloned().ok_or("project result")?)?;
    let entry = directory.project;
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let original_facts = read.records(RecordKind::Fact).await?;
    read.finish().await?;
    let cursor = FeedCursor {
        epoch: EpochNumber::try_from(1)?,
        position: FeedPosition::ZERO,
    };
    let task = TaskId::generate();
    for lifecycle in [
        LifecycleFact::ProjectCreated {
            entry: Box::new(entry),
            creating_request: RequestId::generate(),
            acting_for: None,
        },
        LifecycleFact::ThreadResolved {
            thread: ThreadId::generate(),
            reason: ThreadResolutionReason::try_from("producer resolve".to_owned())?,
        },
        LifecycleFact::Closed {
            position: cursor,
            reason: "producer close".to_owned(),
        },
        LifecycleFact::WorkstreamClosed {
            workstream,
            reason: CloseReason::Done,
        },
        LifecycleFact::Dropped {
            task,
            reason: "producer drop".to_owned(),
        },
        LifecycleFact::PhaseCompleted {
            phase: "producer phase".to_owned(),
        },
        LifecycleFact::ArchiveSeen {
            board: fixture.board.clone(),
        },
    ] {
        let reply = submit_fact(
            &fixture,
            &project,
            FactKind::Lifecycle(lifecycle),
            vec![project.clone().into()],
        )
        .await?;
        assert_eq!(refusal_kind(&reply), Some("invalidShape"), "{reply}");
        assert_eq!(
            reply.pointer("/result/result/decision/reasons/0/reason/field"),
            Some(&json!("kind"))
        );
    }
    let mut read = store.snapshot().await?;
    assert_eq!(read.records(RecordKind::Fact).await?, original_facts);
    read.finish().await?;
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn producer_facts_need_at_least_one_subject_link() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let kind = FactKind::EvaluatorResult {
        guard: "proof".to_owned(),
        subject: project.clone().into(),
        outcome: GuardOutcome::Met,
        evidence: Vec::new(),
        evaluator: evaluator(&fixture)?,
        executor: None,
    };
    for links in [Vec::new(), vec![PostId::generate().into()]] {
        let refused = submit_fact(&fixture, &project, kind.clone(), links).await?;
        assert_eq!(refusal_kind(&refused), Some("invalidShape"), "{refused}");
        assert_eq!(
            refused.pointer("/result/result/decision/reasons/0/reason/field"),
            Some(&json!("links"))
        );
    }
    accepted(&submit_fact(&fixture, &project, kind, vec![project.clone().into()]).await?);
    fixture.stop().await
}
