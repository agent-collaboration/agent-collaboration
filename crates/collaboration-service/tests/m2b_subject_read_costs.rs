//! Subject projection is page-bounded and leaves byte-truncated bodies opaque.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
#[path = "support/read_cost_counter.rs"]
mod read_cost_counter;
use board_fixture::{BoardFixture, TestError, accepted};
use message_board::*;
use read_cost_counter::{ReadCost, current_cost};
use serde_json::{Value, json};

async fn distinct_subject_cost(size: usize) -> Result<ReadCost, TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    for _ in 0..size {
        let about = fixture.create_discussion_subject(&project).await?;
        let mut request = fixture.header();
        request.as_object_mut().ok_or("header")?.extend(json!({
            "projectId":project,"postId":PostId::generate(),"about":about,"text":"distinct subject",
            "actor":{"kind":"human","humanId":PostId::generate()},
            "references":[],"role":"orchestrator","watch":true,"addressees":[]
        }).as_object().ok_or("fields")?.clone());
        accepted(&fixture.api.call("thread_create", request).await?);
    }
    let request = json!({"workspaceId":fixture.workspace,"reader":fixture.actor,"watchedOnly":false,"page":{"limit":5,"cursor":null}});
    fixture.api.call("thread_list", request.clone()).await?;
    let before = current_cost();
    let page = fixture.api.call("thread_list", request).await?;
    let cost = current_cost().since(before);
    assert_eq!(
        page.pointer("/result/page/records")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(5),
        "{page}"
    );
    assert!(
        page.pointer("/result/page/nextCursor")
            .and_then(Value::as_str)
            .is_some()
    );
    assert_eq!(
        cost.records, 10,
        "five returned threads plus five distinct current subjects"
    );
    fixture.stop().await?;
    Ok(cost)
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn distinct_subject_projection_has_size_independent_statements_and_decodes()
-> Result<(), TestError> {
    read_cost_counter::install();
    let small = distinct_subject_cost(8).await?;
    let large = distinct_subject_cost(80).await?;
    eprintln!("distinct subject cost: small={small:?} large={large:?}");
    assert_eq!(small, large, "unreturned subjects must not add work");
    assert!(large.statements > 0);
    Ok(())
}
