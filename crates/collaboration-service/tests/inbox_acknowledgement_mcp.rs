//! Independent acknowledgement outcomes remain visible when a later scope cannot route.
use message_board::*;
use serde_json::json;
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError};

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn routed_refusals_keep_the_ledger_and_unreadable_retry_outcomes_stay_unknown()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let read = store.snapshot().await?;
    let head = read.high_water();
    read.finish().await?;
    let rejected_id = RequestId::generate();
    let acknowledged_id = RequestId::generate();
    let request = |id: &RequestId, through: FeedCursor| json!({"protocolVersion":1,"actor":{"kind":"human","humanId":"reader"},"actingFor":null,"entries":[{"requestId":id,"scope":{"kind":"project","projectId":project},"through":through}]});
    let rejection = fixture
        .api
        .call(
            "inbox_acknowledge",
            request(
                &rejected_id,
                FeedCursor {
                    position: FeedPosition::try_from(head.position.get() + 100)?,
                    ..head
                },
            ),
        )
        .await?;
    assert_eq!(
        rejection.pointer("/result/outcomes/0/outcome/reply/result/kind"),
        Some(&json!("decided")),
        "{rejection}"
    );
    assert_eq!(
        rejection.pointer("/result/outcomes/0/outcome/reply/result/decision/kind"),
        Some(&json!("rejected"))
    );
    let mut read = store.snapshot().await?;
    assert!(
        matches!(read.record(&rejected_id.into()).await?, Some(Record::Request(ledger)) if matches!(ledger.decision, StoredDecision::Rejected { .. }))
    );
    read.finish().await?;
    let acknowledgement = request(&acknowledged_id, head);
    let accepted = fixture
        .api
        .call("inbox_acknowledge", acknowledgement.clone())
        .await?;
    assert_eq!(
        accepted.pointer("/result/outcomes/0/outcome/reply/result/decision/kind"),
        Some(&json!("accepted"))
    );
    drop(store);
    let fixture = fixture.restart_read_fixture().await?;
    std::fs::write(
        fixture
            ._directory
            .path()
            .join("projects")
            .join(project.as_str())
            .join("e1.sqlite"),
        b"unreadable acknowledgement ledger",
    )?;
    let retry = fixture
        .api
        .call("inbox_acknowledge", acknowledgement)
        .await?;
    assert_eq!(
        retry.pointer("/result/outcomes/0/outcome/kind"),
        Some(&json!("unanswered")),
        "{retry}"
    );
    assert!(retry.pointer("/result/outcomes/0/outcome/reply").is_none());
    fixture.stop().await
}
#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove the test contract; Result propagates fixture errors.
async fn batch_acknowledgements_keep_completed_outcomes_after_an_unavailable_scope()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let store = fixture
        ._registry
        .existing_project(&project)
        .await?
        .ok_or("project")?;
    let read = store.snapshot().await?;
    let head = read.high_water();
    read.finish().await?;
    let first = RequestId::generate();
    let missing = RequestId::generate();
    let unavailable = RequestId::generate();
    let response=fixture.api.call("inbox_acknowledge",json!({"protocolVersion":1,"actor":{"kind":"human","humanId":"reader"},"actingFor":null,"entries":[
        {"requestId":first,"scope":{"kind":"project","projectId":project},"through":head},
        {"requestId":missing,"scope":{"kind":"thread","thread":{"thread":ThreadId::generate()}},"through":head},
        {"requestId":unavailable,"scope":{"kind":"project","projectId":ProjectId::generate()},"through":head}
    ]})).await?;
    let mut read = store.snapshot().await?;
    assert!(
        read.record(&first.clone().into()).await?.is_some(),
        "first entry really committed"
    );
    assert!(
        read.record(&missing.clone().into()).await?.is_none(),
        "an unroutable refusal has no invented ledger decision"
    );
    read.finish().await?;
    let outcomes = response
        .pointer("/result/outcomes")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| format!("completed acknowledgement was hidden: {response}"))?;
    assert_eq!(outcomes.len(), 3);
    assert_eq!(outcomes[0]["requestId"], json!(first));
    assert_eq!(outcomes[1]["requestId"], json!(missing));
    assert_eq!(outcomes[2]["requestId"], json!(unavailable));
    assert_eq!(
        outcomes[0].pointer("/outcome/reply/result/decision/kind"),
        Some(&json!("accepted"))
    );
    assert_eq!(
        outcomes[1].pointer("/outcome/kind"),
        Some(&json!("replied"))
    );
    assert_eq!(
        outcomes[1].pointer("/outcome/reply/result/kind"),
        Some(&json!("scopeUnavailable"))
    );
    assert_eq!(
        outcomes[1].pointer("/outcome/reply/result/decision/kind"),
        Some(&json!("refused"))
    );
    assert_eq!(
        outcomes[1].pointer("/outcome/reply/result/decision/reason/kind"),
        Some(&json!("notFound"))
    );
    assert_eq!(
        outcomes[2].pointer("/outcome/reply/result/kind"),
        Some(&json!("scopeUnavailable"))
    );
    fixture.stop().await
}
