#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    clippy::large_futures,
    reason = "lint pass follows the crate split"
)]
//! Non-lead cards select their own pending operands before metadata decoding.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
#[path = "support/read_cost_counter.rs"]
mod read_cost_counter;
use board_fixture::{BoardFixture, TestError, accepted};
use message_board::*;
use read_cost_counter::{ReadCost, current_cost};
use serde_json::{Value, json};
use sqlx::{ConnectOptions, Connection};

async fn add_trail(
    fixture: &BoardFixture,
    project: &ProjectId,
    fields: Value,
) -> Result<WorkTrailRowId, TestError> {
    let row = WorkTrailRowId::generate();
    let mut request = fixture.request(project,json!({"kind":"workTrailAdd","rowId":row,"subject":fields.get("subject"),"text":"recorded pending proof","tags":fields.get("tags"),"links":fields.get("links")}));
    request.as_object_mut().ok_or("request")?.insert(
        "actor".to_owned(),
        fields.get("actor").ok_or("actor")?.clone(),
    );
    accepted(&fixture.api.call("work_trail_add", request).await?);
    Ok(row)
}
struct CardExpectation {
    task: TaskId,
    workstream: WorkstreamId,
    handback: WorkTrailRowId,
    question: WorkTrailRowId,
    recent: Vec<WorkTrailRow>,
    count: usize,
    as_of: FeedCursor,
}
#[allow(clippy::panic_in_result_fn)]
fn assert_card(card: &Value, expected: &CardExpectation) -> Result<(), TestError> {
    assert_eq!(
        card.get("subject"),
        Some(&json!(RecordRef::from(expected.task.clone())))
    );
    assert_eq!(
        card.get("assignment"),
        Some(&json!({"kind":"implementer","subject":expected.workstream}))
    );
    assert_eq!(
        card.get("pendingHandbacks"),
        Some(&json!([RecordRef::from(expected.handback.clone())]))
    );
    assert_eq!(
        card.get("pendingDecisions"),
        Some(&json!([RecordRef::from(expected.question.clone())]))
    );
    assert_eq!(card.get("nextObligation"), Some(&Value::Null));
    assert_eq!(card.get("blockers"), Some(&json!([])));
    assert_eq!(card.get("recentTrail"), Some(&json!(expected.recent)));
    assert_eq!(
        card.pointer("/counts/workTrailRow"),
        Some(&json!(expected.count))
    );
    assert_eq!(card.get("asOf"), Some(&json!(expected.as_of)));
    Ok(())
}

#[allow(clippy::panic_in_result_fn)]
async fn card_cost(size: usize) -> Result<ReadCost, TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let task = fixture.task(&project, &workstream).await?;
    let other_task = fixture.task(&project, &workstream).await?;
    let actor = json!({"kind":"human","humanId":"scoped implementer"});
    let foreign_actor = json!({"kind":"human","humanId":"foreign author"});
    accepted(&fixture.api.call("role_grant",fixture.request(&project,json!({"kind":"roleGrant","subject":{"kind":"workstream","subject":workstream},"role":"implementer","holder":actor,"model":null,"lineage":null,"expectedRevision":0,"ownerGrant":null}))).await?);
    accepted(&fixture.api.call("role_grant",fixture.request(&project,json!({"kind":"roleGrant","subject":{"kind":"workstream","subject":workstream},"role":"verifier","holder":foreign_actor,"model":null,"lineage":null,"expectedRevision":1,"ownerGrant":null}))).await?);
    let subject = json!({"kind":"task","subject":task});
    let handback = add_trail(
        &fixture,
        &project,
        json!({"actor":actor,"subject":subject,"tags":["handback"],"links":[]}),
    )
    .await?;
    let question = add_trail(&fixture,&project,json!({"actor":fixture.actor,"subject":subject,"tags":["owner-question"],"links":[{"kind":"record","record":RecordRef::from(task.clone())}]})).await?;
    let settled = add_trail(
        &fixture,
        &project,
        json!({"actor":actor,"subject":subject,"tags":["handback"],"links":[]}),
    )
    .await?;
    let decision = add_trail(&fixture,&project,json!({"actor":fixture.actor,"subject":subject,"tags":["decision"],"links":[{"kind":"record","record":RecordRef::from(settled.clone())}]})).await?;
    let mut foreign = None;
    for _ in 0..size {
        // Each tag crosses the real 100-row candidate batch in the large fixture.
        let row = add_trail(&fixture,&project,json!({"actor":foreign_actor,"subject":{"kind":"task","subject":other_task},"tags":["handback","owner-question"],"links":[{"kind":"record","record":RecordRef::from(other_task.clone())}]})).await?;
        add_trail(&fixture,&project,json!({"actor":fixture.actor,"subject":{"kind":"task","subject":other_task},"tags":["decision"],"links":[{"kind":"record","record":RecordRef::from(row.clone())}]})).await?;
        foreign.get_or_insert(row);
    }
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let mut read = store.snapshot().await?;
    let as_of = read.high_water();
    let mut recent = Vec::new();
    for id in [decision, settled, question.clone(), handback.clone()] {
        let Some(Record::WorkTrailRow(row)) = read.record(&id.into()).await? else {
            return Err("expected trail body".into());
        };
        recent.push(row);
    }
    read.finish().await?;
    let expected = CardExpectation {
        task,
        workstream,
        handback,
        question,
        recent,
        count: 4 + size * 2,
        as_of,
    };
    let request = json!({"projectId":project,"actor":actor,"subject":subject});
    let before = current_cost();
    let answer = fixture.api.call("view_card", request.clone()).await?;
    let cost = current_cost().since(before);
    assert_card(
        answer
            .get("result")
            .ok_or_else(|| format!("card: {answer}"))?,
        &expected,
    )?;
    let foreign = foreign.ok_or("foreign candidate")?;
    let mut raw = sqlx_turso::TursoConnectOptions::new()
        .filename(store.path())
        .connect()
        .await?;
    assert_eq!(
        sqlx::query(
            "UPDATE work_trail_links SET target='{\"kind\":\"unsupportedEvidence\"}' WHERE row_id=?"
        )
        .bind(foreign.as_str())
        .execute(&mut raw)
        .await?
        .rows_affected(),
        1
    );
    raw.close().await?;
    let answer = fixture.api.call("view_card", request.clone()).await?;
    assert_card(answer.get("result").ok_or("card")?, &expected)?;
    let mut relevant = request;
    relevant.as_object_mut().ok_or("card")?.insert(
        "subject".to_owned(),
        json!({"kind":"task","subject":other_task}),
    );
    let answer = fixture.api.call("view_card", relevant.clone()).await?;
    assert!(
        answer.get("error").is_some(),
        "relevant pending corruption accepted: {answer}"
    );
    let mut read = store.snapshot().await?;
    assert!(
        read.record(&foreign.into()).await.is_err(),
        "direct body corruption must remain strict"
    );
    read.finish().await?;
    fixture.stop().await?;
    Ok(cost)
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn non_lead_pending_metadata_costs_ignore_foreign_tagged_batches() -> Result<(), TestError> {
    read_cost_counter::install();
    let small = card_cost(8).await?;
    let large = card_cost(120).await?;
    eprintln!("scoped pending card: small={small:?}; large={large:?}");
    assert_eq!(small, large, "foreign tagged batches grew card work");
    assert_eq!(
        large.trail_metadata, 4,
        "two own handbacks, one subject question and their one settlement"
    );
    assert_eq!(large.feed_entries, 0);
    Ok(())
}
