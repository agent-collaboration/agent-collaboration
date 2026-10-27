#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! Entity pages select keys before decoding the returned records.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
#[path = "support/read_cost_counter.rs"]
mod read_cost_counter;
use board_fixture::{BoardFixture, TestError};
use read_cost_counter::{ReadCost, current_cost};
use serde_json::{Value, json};

#[allow(clippy::panic_in_result_fn)]
async fn task_page_cost(size: usize) -> Result<ReadCost, TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, workstream) = fixture.project().await?;
    let mut expected = Vec::new();
    for _ in 0..size {
        expected.push(fixture.task(&project, &workstream).await?);
    }
    expected.sort();
    let request = json!({"projectId":project,"page":{"limit":5,"cursor":null}});
    fixture.api.call("task_list", request.clone()).await?;
    let before = current_cost();
    let page = fixture.api.call("task_list", request).await?;
    let cost = current_cost().since(before);
    let records = page
        .pointer("/result/records")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("{page}"))?;
    assert_eq!(records.len(), 5);
    assert_eq!(
        records
            .iter()
            .filter_map(|record| record.pointer("/record/taskId"))
            .cloned()
            .collect::<Vec<_>>(),
        expected
            .into_iter()
            .take(5)
            .map(|id| json!(id))
            .collect::<Vec<_>>()
    );
    assert!(
        page.pointer("/result/nextCursor")
            .is_some_and(Value::is_string)
    );
    fixture.stop().await?;
    Ok(cost)
}
#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn entity_page_cost_does_not_grow_with_unreturned_tasks() -> Result<(), TestError> {
    read_cost_counter::install();
    let small = task_page_cost(8).await?;
    let large = task_page_cost(80).await?;
    eprintln!("entity page costs: small={small:?}, large={large:?}");
    assert_eq!(small, large, "entity page decoded the whole task kind");
    assert_eq!(large.records, 5);
    assert_eq!(large.feed_entries, 0);
    Ok(())
}
