#![expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "lint pass follows the crate split"
)]
//! Foreign local locators are refused at every public boundary before metadata changes.
use serde_json::json;
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

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions prove the test contract; Result propagates fixture errors."
)]
async fn foreign_local_repositories_are_refused_on_every_request_that_names_one()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let foreign = json!({"kind":"local","serviceId":"00000000-0000-4000-8000-000000000099","commonDirectory":"/work/repo"});
    let before = fixture
        .api
        .call("workspace_show", json!({"workspaceId":fixture.workspace}))
        .await?;
    let message = "Correct repository.serviceId: must match the selected board service.";
    let expected = json!({"code":-32050,"message":message,"data":{"kind":"invalidField","stage":"validation","message":message,"nextAction":"correctRequest","details":{"kind":"fieldConstraint","field":"repository.serviceId","requirement":"must match the selected board service"}}});
    for tool in ["workspace_list", "repository_attach", "repository_detach"] {
        let request = if tool == "workspace_list" {
            json!({"repository":foreign,"page":{"limit":10,"cursor":null}})
        } else {
            let mut header = fixture.header();
            header["workspaceId"] = json!(fixture.workspace);
            header["repository"] = foreign.clone();
            header
        };
        let response = fixture.api.call(tool, request).await?;
        assert_eq!(response.get("error"), Some(&expected), "{tool}: {response}");
        assert!(
            response
                .get("result")
                .is_none_or(serde_json::Value::is_null),
            "foreign repository request succeeded"
        );
    }
    assert_eq!(
        fixture
            .api
            .call("workspace_show", json!({"workspaceId":fixture.workspace}))
            .await?,
        before
    );
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn workspace_repository_filter_matches_typed_locator_fields() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let repositories = [
        json!({"normalizedOrigin":"github.com/example/project","kind":"origin"}),
        json!({"commonDirectory":"/work/project/.git","kind":"local","serviceId":"00000000-0000-4000-8000-000000000001"}),
    ];
    for repository in &repositories {
        let mut request = fixture.header();
        request.as_object_mut().ok_or("header")?.extend(
            json!({"workspaceId":fixture.workspace,"repository":repository})
                .as_object()
                .ok_or("fields")?
                .clone(),
        );
        accepted(&fixture.api.call("repository_attach", request).await?);
    }
    for repository in repositories {
        let response = fixture
            .api
            .call(
                "workspace_list",
                json!({"repository":repository,"page":{"limit":1,"cursor":null}}),
            )
            .await?;
        assert_eq!(
            response.pointer("/result/page/records/0/workspaceId"),
            Some(&json!(fixture.workspace)),
            "{response}"
        );
        assert!(
            response
                .pointer("/result/page/nextCursor")
                .is_some_and(serde_json::Value::is_null)
        );
    }
    let response = fixture.api.call("workspace_list", json!({"repository":{"kind":"origin","normalizedOrigin":"github.com/example/another"},"page":{"limit":1,"cursor":null}})).await?;
    assert_eq!(
        response
            .pointer("/result/page/records")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(0),
        "{response}"
    );
    fixture.stop().await
}
