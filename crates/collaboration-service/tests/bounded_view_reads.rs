#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! Current-state views must not replay unrelated content history.
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
#[path = "support/board_read_setup.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
mod board_read_setup;
#[path = "support/read_cost_counter.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
#[expect(
    unused_imports,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
mod read_cost_counter;
use board_fixture::{BoardFixture, TestError, accepted};
use board_read_setup::*;
use message_board::*;
use read_cost_counter::{ReadCost, current_cost};
use serde_json::{Value, json};

async fn card_cost(size: usize) -> Result<ReadCost, TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    fixture.task(&project, &workstream).await?;
    for index in 0..size {
        post_at_placement(&fixture, Placement::Project { project_id: project.clone() }, json!({"actor":{"kind":"human","humanId":PostId::generate()},"text":"unrelated history"})).await?;
        accepted(&fixture.api.call("work_trail_add", fixture.request(&project, json!({"kind":"workTrailAdd","rowId":WorkTrailRowId::generate(),"subject":{"kind":"project"},"text":format!("note {index}"),"tags":[],"links":[]}))).await?);
    }
    let request = json!({"projectId":project,"actor":fixture.actor,"subject":{"kind":"project"}});
    fixture.api.call("view_card", request.clone()).await?;
    let before = current_cost();
    let card = fixture.api.call("view_card", request).await?;
    let cost = current_cost().since(before);
    assert_eq!(
        card.pointer("/result/counts/post"),
        Some(&json!(size)),
        "{card}"
    );
    assert_eq!(
        card.pointer("/result/counts/workTrailRow"),
        Some(&json!(size))
    );
    assert_eq!(
        card.pointer("/result/recentTrail")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(20)
    );
    fixture.stop().await?;
    Ok(cost)
}
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn card_reads_current_state_and_a_bounded_recent_prefix() -> Result<(), TestError> {
    read_cost_counter::install();
    let small = card_cost(24).await?;
    let large = card_cost(80).await?;
    eprintln!("card costs: small={small:?}, large={large:?}");
    assert_eq!(
        small, large,
        "unreturned post/trail history changed card cost"
    );
    assert_eq!(
        large.feed_entries, 0,
        "current-state view cannot replay the feed"
    );
    Ok(())
}
