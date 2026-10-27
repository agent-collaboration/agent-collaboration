//! A later unavailable project cannot hide earlier independent initialization writes.
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
use board_fixture::{BoardFixture, TestError};
use message_board::*;
use serde_json::{Value, json};
use sqlx::{ConnectOptions, Connection};
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove the test contract; Result propagates fixture errors."
)]
async fn aggregate_initializations_expose_each_result_when_a_project_file_is_moving()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (first, _) = fixture.project().await?;
    let moving = ProjectId::generate();
    let input: ProjectCreateRequest = serde_json::from_value(json!({
        "requestId":RequestId::generate(),"protocolVersion":1,"actor":fixture.actor,
        "projectId":moving,"boardId":fixture.board,"title":moving.as_str()
    }))?;
    board_fixture::settle_accepted_creation(&fixture._fleet, &input, &fixture.record_origin()?)
        .await?;
    let (third, _) = fixture.project().await?;
    let moving_store = fixture._registry.open_project(&moving).await?;
    let mut empty = moving_store.snapshot().await?;
    assert!(empty.records(RecordKind::Project).await?.is_empty());
    assert!(empty.records(RecordKind::Request).await?.is_empty());
    empty.finish().await?;
    let mut external = sqlx_turso::TursoConnectOptions::new()
        .filename(moving_store.path())
        .create_if_missing(false)
        .connect()
        .await?;
    let mut held = external.begin().await?;
    sqlx::query::<sqlx_turso::Turso>("SELECT version FROM _sqlx_migrations LIMIT 1")
        .fetch_one(&mut *held)
        .await?;
    let database = fixture._registry.database(&moving).await?;
    // This native write creates newer WAL frames while the reader pins the
    // empty Pending database. Ready registration cannot subsequently reject.
    database
        .reject_registration(fleet_sync_protocol::GateReason {
            code: fleet_sync_protocol::GateReasonCode::new("fixture.rejected")?,
            detail: "partial initialization during a busy native move".to_owned(),
        })
        .await?;
    assert!(matches!(
        fixture._registry.orphan_project(&moving).await,
        Err(collaboration_service::ProjectRegistryError::Move(
            fleet_sync_writer::MoveAsideError::Publish(
                fleet_sync_writer::native_file::PublishError::CheckpointBusy { .. },
            )
        ))
    ));
    assert!(moving_store.path().exists());
    let keys = [
        RequestId::generate(),
        RequestId::generate(),
        RequestId::generate(),
    ];
    let request = json!({"protocolVersion":1,"actingFor":null,"reader":{"kind":"human","humanId":"reader"},"scope":{"kind":"board","boardId":fixture.board},"readMode":"unread","page":{},"initializations":[
        {"projectId":first,"requestId":keys[0]},
        {"projectId":moving,"requestId":keys[1]},
        {"projectId":third,"requestId":keys[2]}
    ]});
    let response = fixture.api.call("inbox_fetch", request.clone()).await?;
    let first_store = fixture
        ._registry
        .existing_project(&first)
        .await?
        .ok_or("first store")?;
    let mut read = first_store.snapshot().await?;
    assert!(
        read.record(&keys[0].clone().into()).await?.is_some(),
        "first initialization really committed"
    );
    let first_head = read.high_water();
    read.finish().await?;
    let results = response
        .pointer("/result/page/initializations")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("committed prefix is hidden: {response}"))?;
    assert_eq!(results.len(), 3);
    for (result, key) in results.iter().zip(&keys) {
        assert_eq!(result["requestId"], json!(key));
    }
    assert_eq!(
        results[0].pointer("/outcome/reply/result/decision/kind"),
        Some(&json!("accepted"))
    );
    assert_eq!(
        results[1].pointer("/outcome/kind"),
        Some(&json!("unanswered"))
    );
    assert_eq!(
        results[2].pointer("/outcome/reply/result/decision/kind"),
        Some(&json!("accepted"))
    );
    assert_eq!(
        response.pointer("/result/page/unavailable"),
        Some(&json!([moving]))
    );
    let replay = fixture.api.call("inbox_fetch", request).await?;
    assert_eq!(
        replay.pointer("/result/page/initializations/0/outcome/reply/result"),
        results[0].pointer("/outcome/reply/result")
    );
    let read = first_store.snapshot().await?;
    assert_eq!(read.high_water(), first_head);
    read.finish().await?;
    let third_store = fixture
        ._registry
        .existing_project(&third)
        .await?
        .ok_or("third store")?;
    let mut read = third_store.snapshot().await?;
    assert!(read.record(&keys[2].clone().into()).await?.is_some());
    read.finish().await?;
    held.rollback().await?;
    external.close().await?;
    fixture.stop().await
}
