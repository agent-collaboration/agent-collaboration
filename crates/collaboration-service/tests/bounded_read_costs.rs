//! Increasing project size must not increase one page's statements or body decodes.
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
use message_board::*;
use read_cost_counter::{ReadCost, current_cost};
use serde_json::json;

async fn thread_page_cost(size: usize) -> Result<ReadCost, TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    for index in 0..size {
        let actor = json!({"kind":"human","humanId":PostId::generate()});
        accepted(&fixture.api.call("role_grant", fixture.request(&project, json!({"kind":"roleGrant","subject":{"kind":"project","subject":project},"role":"advisor","holder":actor,"model":null,"lineage":null,"expectedRevision":index+1,"ownerGrant":null}))).await?);
        let mut request = fixture.header();
        request.as_object_mut().ok_or("header")?.extend(
            json!({
            "projectId":project,"postId":PostId::generate(),"about":{"kind":"project","subject":{"line":"Cost fixture"}},"text":"cost fixture",
            "actor":actor,
                "references":[],"role":"orchestrator","watch":true,"addressees":[]
            })
            .as_object()
            .ok_or("fields")?
            .clone(),
        );
        accepted(&fixture.api.call("thread_create", request).await?);
    }
    let request = json!({"workspaceId":fixture.workspace,"reader":fixture.actor,"watchedOnly":false,
        "page":{"limit":5,"cursor":null}});
    // Warm schema/cursor setup; count the public operation itself on the next call.
    fixture.api.call("thread_list", request.clone()).await?;
    let before = current_cost();
    let page = fixture.api.call("thread_list", request).await?;
    let cost = current_cost().since(before);
    assert_eq!(
        page.pointer("/result/page/records")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(5),
        "{page}"
    );
    assert!(
        page.pointer("/result/page/nextCursor")
            .and_then(serde_json::Value::as_str)
            .is_some()
    );
    fixture.stop().await?;
    Ok(cost)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn one_thread_page_has_size_independent_statement_and_decode_counts() -> Result<(), TestError>
{
    read_cost_counter::install();
    let small = thread_page_cost(8).await?;
    let large = thread_page_cost(80).await?;
    eprintln!("thread page costs: small={small:?} large={large:?}");
    assert_eq!(
        small, large,
        "page work grew with unreturned project records"
    );
    assert_eq!(large.records, 5, "decode only the returned threads");
    assert_eq!(
        large.participants, 5,
        "validate the returned threads' participants"
    );
    assert_eq!(
        large.feed_entries, 5,
        "validate the returned participants' anchors"
    );
    assert!(
        large.statements > 0,
        "the counter must observe the real SQL path"
    );
    Ok(())
}
