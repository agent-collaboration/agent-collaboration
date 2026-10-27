#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! RC-10/11 use the real closing position, causal anchors and a recorded dropped task.
use message_board::*;
use serde_json::{Value, json};
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
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove the test contract; Result propagates fixture errors."
)]
async fn close_is_terminal_and_late_facts_require_a_current_line_anchor() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let task = fixture.task(&project, &workstream).await?;
    accepted(&fixture.api.call("task_set_status",fixture.request(&project,json!({"kind":"taskSetStatus","taskId":task,"status":{"kind":"drop","reason":"caller chooses to drop this task"},"expectedRevision":0}))).await?);
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let read = store.snapshot().await?;
    let before_close = read.high_water();
    read.finish().await?;
    let close = fixture.request(
        &project,
        json!({"kind":"projectClose","reason":"scope completed","expectedRevision":0}),
    );
    let closed = fixture.api.call("project_close", close.clone()).await?;
    accepted(&closed);
    assert_eq!(
        fixture.api.call("project_close", close).await?["result"]["result"],
        closed["result"]["result"]
    );
    let mut read = store.snapshot().await?;
    let Some(Record::Project(state)) = read.record(&project.clone().into()).await? else {
        return Err("project state".into());
    };
    let closing = state.closed_at.ok_or("close position")?;
    assert!(closing.position > before_close.position);
    let facts = read.records(RecordKind::Fact).await?;
    assert!(facts.iter().any(|record|matches!(record,Record::Fact(fact) if matches!(&fact.kind,FactKind::Lifecycle(LifecycleFact::Closed {position,reason}) if *position==closing&&reason=="scope completed") && fact.envelope.position.project_cursor()==Some(closing))));
    read.finish().await?;
    for (tool, operation) in [
        (
            "workstream_create",
            json!({"kind":"workstreamCreate","workstreamId":WorkstreamId::generate(),"name":"after close","goal":"refused"}),
        ),
        (
            "work_trail_add",
            json!({"kind":"workTrailAdd","rowId":WorkTrailRowId::generate(),"subject":{"kind":"project"},"text":"after close","tags":[],"links":[]}),
        ),
        (
            "role_release",
            json!({"kind":"roleRelease","subject":{"kind":"project","subject":project},"role":"lead","expectedRevision":1}),
        ),
    ] {
        let refused = fixture
            .api
            .call(tool, fixture.request(&project, operation))
            .await?;
        assert_eq!(refusal_kind(&refused), Some("closed"), "{refused}");
    }
    let produced_kind = FactKind::EvaluatorResult {
        guard: "late-evidence".to_owned(),
        subject: task.clone().into(),
        outcome: GuardOutcome::Met,
        evidence: Vec::new(),
        evaluator: EvaluatorRef {
            actor: RecordedActor::declared(serde_json::from_value(fixture.actor.clone())?),
            engine: None,
            calibration: None,
        },
        executor: None,
    };
    let fact_request = |anchor: Value, id: &FactId| {
        fixture.request(&project,json!({"kind":"factRecord","fact":{"factId":id,"kind":produced_kind,"links":[{"kind":"task","id":task}],"producerTime":"2099-01-01T00:00:00Z","anchor":anchor}}))
    };
    let late_id = FactId::generate();
    let late_request = fact_request(json!(before_close), &late_id);
    let late = fixture
        .api
        .call("fact_record", late_request.clone())
        .await?;
    accepted(&late);
    assert_eq!(
        fixture.api.call("fact_record", late_request).await?["result"]["result"],
        late["result"]["result"]
    );
    let mut read = store.snapshot().await?;
    let Some(Record::Fact(fact)) = read.record(&late_id.clone().into()).await? else {
        return Err("late fact missing".into());
    };
    assert!(fact.late);
    assert_eq!(fact.anchor, Some(before_close));
    assert_eq!(fact.links, vec![task.clone().into()]);
    assert_eq!(
        fact.producer_time,
        "2099-01-01T00:00:00Z".parse::<chrono::DateTime<chrono::Utc>>()?
    );
    assert!(read.records(RecordKind::NotifyIntent).await?.iter().all(|record|!matches!(record,Record::NotifyIntent(intent) if intent.cause==RecordRef::from(late_id.clone()))));
    let after_close = read.high_water();
    read.finish().await?;
    for anchor in [Value::Null, json!(after_close)] {
        let refused = fixture
            .api
            .call("fact_record", fact_request(anchor, &FactId::generate()))
            .await?;
        assert_eq!(refusal_kind(&refused), Some("closed"), "{refused}");
    }
    let producer_closed=fixture.api.call("fact_record",fixture.request(&project,json!({"kind":"factRecord","fact":{"factId":FactId::generate(),"kind":{"kind":"lifecycle","fact":{"kind":"closed","position":before_close,"reason":"producer cannot close"}},"links":[],"producerTime":"2026-10-12T00:00:00Z","anchor":before_close}}))).await?;
    assert_eq!(refusal_kind(&producer_closed), Some("invalidShape"));
    fixture.stop().await
}
