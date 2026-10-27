#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! Fleet pages decode the returned records and shared parents, independent of directory size.
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
use serde_json::{Value, json};
use std::collections::BTreeMap;

async fn create_rows(fixture: &BoardFixture, size: usize) -> Result<(), TestError> {
    for index in 0..size {
        for (tool, fields) in [
            (
                "workspace_create",
                json!({"workspaceId":WorkspaceId::generate(),"name":format!("workspace {index}"),"description":""}),
            ),
            (
                "board_create",
                json!({"workspaceId":fixture.workspace,"boardId":BoardId::generate(),"name":format!("board {index}"),"description":"","kind":{"kind":"area"}}),
            ),
            (
                "project_create",
                json!({"boardId":fixture.board,"projectId":ProjectId::generate(),"title":format!("needle project {index}")}),
            ),
            (
                "adr_create",
                json!({"workspaceId":fixture.workspace,"adrId":AdrId::generate(),"title":format!("decision {index}"),"text":"decision","supersedes":null,"madeIn":null,"repositoryFiles":[]}),
            ),
        ] {
            let mut request = fixture.header();
            request
                .as_object_mut()
                .ok_or("request")?
                .extend(fields.as_object().ok_or("fields")?.clone());
            if tool == "project_create" {
                fixture
                    .prepare_registered_creation(&serde_json::from_value(request.clone())?)
                    .await?;
            }
            accepted(&fixture.api.call(tool, request).await?);
        }
    }
    Ok(())
}

async fn fleet_page_costs(size: usize) -> Result<BTreeMap<String, ReadCost>, TestError> {
    let fixture = BoardFixture::start().await?;
    create_rows(&fixture, size).await?;
    let mut costs = BTreeMap::new();
    for (tool, request, records_path, cursor_path) in [
        (
            "workspace_list",
            json!({"repository":null,"page":{"limit":5,"cursor":null}}),
            "/result/page/records",
            "/result/page/nextCursor",
        ),
        (
            "board_list",
            json!({"workspaceId":fixture.workspace,"includeArchived":false,"page":{"limit":5,"cursor":null}}),
            "/result/page/records",
            "/result/page/nextCursor",
        ),
        (
            "project_list",
            json!({"boardId":fixture.board,"page":{"limit":5,"cursor":null}}),
            "/result/page/records",
            "/result/page/nextCursor",
        ),
        (
            "adr_list",
            json!({"workspaceId":fixture.workspace,"page":{"limit":5,"cursor":null}}),
            "/result/records",
            "/result/nextCursor",
        ),
        (
            "discovery_search",
            json!({"query":"needle","scope":{"kind":"workspace","workspaceId":fixture.workspace},"kind":"projectDirectoryEntry","includeArchived":false,"page":{"limit":5,"cursor":null}}),
            "/result/page/records",
            "/result/page/nextCursor",
        ),
    ] {
        fixture.api.call(tool, request.clone()).await?;
        let before = current_cost();
        let response = fixture.api.call(tool, request).await?;
        costs.insert(tool.to_owned(), current_cost().since(before));
        assert_eq!(
            response
                .pointer(records_path)
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(5),
            "{response}"
        );
        assert!(
            response.pointer(cursor_path).is_some_and(Value::is_string),
            "{response}"
        );
    }
    fixture.stop().await?;
    Ok(costs)
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn fleet_pages_and_discovery_parent_batches_do_not_grow_with_directory_size()
-> Result<(), TestError> {
    read_cost_counter::install();
    let small = fleet_page_costs(8).await?;
    let large = fleet_page_costs(80).await?;
    eprintln!("fleet page costs: small={small:?}, large={large:?}");
    assert_eq!(
        small, large,
        "fleet page loaded unrelated directory records"
    );
    for (tool, cost) in large {
        assert_eq!(
            cost.records,
            if tool == "discovery_search" { 7 } else { 5 },
            "{tool}"
        );
        assert_eq!(cost.feed_entries, 0);
    }
    Ok(())
}
