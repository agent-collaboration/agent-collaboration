//! A later unavailable project cannot hide earlier independent initialization writes.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError};
use message_board::*;
use serde_json::{Value, json};
use sqlx::{ConnectOptions, Connection};
#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove the test contract; Result propagates fixture errors.
async fn aggregate_initializations_expose_each_result_when_a_project_file_is_moving()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (first, _) = fixture.project().await?;
    let (moving, _) = fixture.project().await?;
    let (third, _) = fixture.project().await?;
    let moving_store = fixture
        ._registry
        .existing_project(&moving)
        .await?
        .ok_or("moving store")?;
    let mut external = sqlx_turso::TursoConnectOptions::new()
        .filename(moving_store.path())
        .create_if_missing(false)
        .connect()
        .await?;
    let mut held = external.begin().await?;
    sqlx::query::<sqlx_turso::Turso>("SELECT singleton FROM feed_checkpoint WHERE singleton = 1")
        .fetch_one(&mut *held)
        .await?;
    board_fixture::accepted(&fixture.api.call("work_trail_add", fixture.request(&moving, json!({"kind":"workTrailAdd","rowId":WorkTrailRowId::generate(),"subject":{"kind":"project"},"text":"make newer WAL frames","tags":[],"links":[]}))).await?);
    let write = moving_store.begin_admission().await?;
    let committed = write.commit_retaining_writer().await?;
    assert!(matches!(
        moving_store
            .move_aside(&committed, &fixture._directory.path().join("moving.sqlite"))
            .await,
        Err(message_board_storage::ProjectStorageError::MoveAsidePending { .. })
    ));
    assert!(moving_store.path().exists());
    drop(committed);
    let keys = [
        RequestId::generate(),
        RequestId::generate(),
        RequestId::generate(),
    ];
    let request = json!({"protocolVersion":1,"actingFor":null,"reader":{"kind":"human","humanId":"reader"},"scope":{"kind":"board","boardId":fixture.board},"readMode":"unread","page":{},"initializations":[
        {"projectId":first,"requestId":keys[0],"observedEpoch":1,"resubmits":null},
        {"projectId":moving,"requestId":keys[1],"observedEpoch":1,"resubmits":null},
        {"projectId":third,"requestId":keys[2],"observedEpoch":1,"resubmits":null}
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
