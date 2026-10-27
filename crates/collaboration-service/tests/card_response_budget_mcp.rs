//! A card's latest trail must fit the same real response bound as other board reads.
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
use board_fixture::{BoardFixture, TestError, accepted};
use message_board::*;
use serde_json::{Value, json};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn escaped_latest_trail_is_a_bounded_prefix_without_losing_the_recorded_count()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let mut rows = Vec::new();
    for _ in 0..20 {
        let row = WorkTrailRowId::generate();
        rows.push(row.clone());
        accepted(&fixture.api.call("work_trail_add",fixture.request(&project,json!({"kind":"workTrailAdd","rowId":row,"subject":{"kind":"project"},"text":"\\".repeat(MAX_MESSAGE_TEXT_BYTES),"tags":[],"links":[]}))).await?);
    }
    let card = fixture
        .api
        .call(
            "view_card",
            json!({"projectId":project,"actor":fixture.actor,"subject":{"kind":"project"}}),
        )
        .await?;
    let body = card.get("result").ok_or("card result missing")?;
    assert!(
        serde_json::to_vec(&card)?.len() <= collaboration_protocol::MAX_CONTROL_FRAME_BYTES,
        "actual card response exceeds the frame bound"
    );
    assert_eq!(body.pointer("/counts/workTrailRow"), Some(&json!(20)));
    let returned = body
        .get("recentTrail")
        .and_then(Value::as_array)
        .ok_or("recent trail missing")?;
    assert!(!returned.is_empty());
    assert!(
        returned.len() < 20,
        "escaped text must exercise the byte cap"
    );
    assert_eq!(
        returned
            .iter()
            .filter_map(|row| row.get("rowId"))
            .cloned()
            .collect::<Vec<_>>(),
        rows.into_iter()
            .rev()
            .take(returned.len())
            .map(|id| json!(id))
            .collect::<Vec<_>>()
    );
    fixture.stop().await
}
