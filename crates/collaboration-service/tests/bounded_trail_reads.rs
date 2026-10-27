#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! A decision continuation reads backward from its cursor, not from feed position zero.
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
use board_read_setup::post_at_placement;
use message_board::*;
use read_cost_counter::{ReadCost, current_cost};
use serde_json::{Value, json};

async fn second_page_cost(size: usize) -> Result<ReadCost, TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    for _ in 0..size {
        post_at_placement(&fixture, Placement::Project { project_id: project.clone() }, json!({"actor":{"kind":"human","humanId":PostId::generate()},"text":"old non-trail history"})).await?;
    }
    let mut decisions = Vec::new();
    for _ in 0..7 {
        let id = WorkTrailRowId::generate();
        decisions.push(id.clone());
        accepted(&fixture.api.call("work_trail_add", fixture.request(&project, json!({"kind":"workTrailAdd","rowId":id,"subject":{"kind":"project"},"text":"decision","tags":["decision"],"links":[]}))).await?);
    }
    let mut request = json!({"projectId":project,"selection":{"kind":"decisions","after":null,"before":null,"rangeTop":null},"limit":2,"subject":null,"tags":[],"includeRedacted":false,"reader":{"kind":"anonymous"}});
    let first = fixture.api.call("work_trail_show", request.clone()).await?;
    request
        .pointer_mut("/selection/before")
        .ok_or("before")?
        .clone_from(
            first
                .pointer("/result/value/page/olderThan")
                .ok_or("older cursor")?,
        );
    request
        .pointer_mut("/selection/rangeTop")
        .ok_or("range top")?
        .clone_from(
            first
                .pointer("/result/value/page/rangeTop")
                .ok_or("frozen range")?,
        );
    let before = current_cost();
    let second = fixture.api.call("work_trail_show", request).await?;
    let cost = current_cost().since(before);
    let ids: Vec<_> = second
        .pointer("/result/value/page/rows")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{second}"))?
        .iter()
        .filter_map(|row| row.get("rowId"))
        .cloned()
        .collect();
    assert_eq!(
        ids,
        decisions
            .iter()
            .rev()
            .skip(2)
            .take(2)
            .map(|id| json!(id))
            .collect::<Vec<_>>()
    );
    assert!(
        second
            .pointer("/result/value/page/coveredThrough")
            .is_some_and(Value::is_null)
    );
    assert!(
        second
            .pointer("/result/value/page/olderThan")
            .is_some_and(Value::is_object)
    );
    fixture.stop().await?;
    Ok(cost)
}
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn a_backward_decision_page_has_size_independent_cost() -> Result<(), TestError> {
    read_cost_counter::install();
    let small = second_page_cost(8).await?;
    let large = second_page_cost(80).await?;
    eprintln!("decision continuation: small={small:?}, large={large:?}");
    assert_eq!(small, large, "decision page reread old unrelated history");
    assert_eq!(
        large.feed_entries, 2,
        "decode only returned decision bodies"
    );
    Ok(())
}
