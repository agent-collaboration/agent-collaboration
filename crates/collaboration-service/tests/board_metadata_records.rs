//! M2.5b preserves project_records' metadata, corruption and bounded hierarchy claims.
#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError, accepted};
use message_board::*;
use serde_json::{Value, json};
use sqlx::{ConnectOptions, Connection};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Corrupt identity must be attributed without echoing stored raw values."
)]
async fn malformed_stored_primary_id_is_reported_without_echoing_the_raw_value()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let mut raw = sqlx_turso::TursoConnectOptions::new()
        .filename(fixture._directory.path().join("fleet-cache.sqlite"))
        .connect()
        .await?;
    sqlx::query("INSERT INTO cached_fleet_records(kind,id,payload) SELECT kind,'malformed',payload FROM cached_fleet_records WHERE kind='workspace' AND id=?").bind(fixture.workspace.as_str()).execute(&mut raw).await?;
    raw.close().await?;
    let failure = fixture
        .api
        .call(
            "workspace_list",
            json!({"repository":null,"page":{"limit":100,"cursor":null}}),
        )
        .await?;
    assert_eq!(
        failure.pointer("/error/data/kind"),
        Some(&json!("invalidRecord")),
        "{failure}"
    );
    let message = failure
        .pointer("/error/data/message")
        .and_then(Value::as_str)
        .ok_or("message")?;
    assert!(!message.contains("malformed"));
    assert_eq!(
        failure.pointer("/error/data/details/field"),
        Some(&json!("workspaceId")),
        "{failure}"
    );
    fixture.stop().await
}
#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn corrupt_metadata_listing_names_the_resource_and_canonical_inspection_action()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let path = fixture._directory.path().join("fleet-cache.sqlite");
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(&path)
        .connect()
        .await?;
    let payload: String = sqlx::query_scalar(
        "SELECT payload FROM cached_fleet_records WHERE kind='workspace' AND id=?",
    )
    .bind(fixture.workspace.as_str())
    .fetch_one(&mut connection)
    .await?;
    let mut stored: Value = serde_json::from_str(&payload)?;
    *stored
        .pointer_mut("/record/name")
        .ok_or("stored name missing")? = json!("");
    sqlx::query("UPDATE cached_fleet_records SET payload=? WHERE kind='workspace' AND id=?")
        .bind(stored.to_string())
        .bind(fixture.workspace.as_str())
        .execute(&mut connection)
        .await?;
    connection.close().await?;
    let failure = fixture
        .api
        .call(
            "workspace_list",
            json!({"repository":null,"page":{"limit":100,"cursor":null}}),
        )
        .await?;
    assert_eq!(
        failure.pointer("/error/data/kind"),
        Some(&json!("invalidRecord")),
        "{failure}"
    );
    assert_eq!(
        failure.pointer("/error/data/stage"),
        Some(&json!("inspection"))
    );
    assert_eq!(
        failure.pointer("/error/data/nextAction"),
        Some(&json!("inspectResource"))
    );
    assert_eq!(
        failure.pointer("/error/data/details"),
        Some(
            &json!({"kind":"resource","resource":{"kind":"workspace","workspaceId":fixture.workspace}})
        )
    );
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn large_metadata_lists_continue_before_the_control_frame_limit() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let full_description = "x".repeat(MAX_DESCRIPTION_BYTES);
    let mut update = fixture.header();
    update.as_object_mut().ok_or("update object")?.extend(json!({"workspaceId":fixture.workspace,"name":"test workspace","description":full_description}).as_object().ok_or("fields")?.clone());
    accepted(&fixture.api.call("workspace_update", update).await?);
    let mut update = fixture.header();
    update.as_object_mut().ok_or("update object")?.extend(
        json!({"boardId":fixture.board,"name":"test board","description":full_description})
            .as_object()
            .ok_or("fields")?
            .clone(),
    );
    accepted(&fixture.api.call("board_update", update).await?);
    for index in 0..60 {
        for (tool, fields) in [
            (
                "workspace_create",
                json!({"workspaceId":WorkspaceId::generate(),"name":format!("Large workspace {index:02}"),"description":full_description}),
            ),
            (
                "board_create",
                json!({"workspaceId":fixture.workspace,"boardId":BoardId::generate(),"name":format!("Large board {index:02}"),"description":full_description,"kind":{"kind":"area"}}),
            ),
            (
                "project_create",
                json!({"boardId":fixture.board,"projectId":ProjectId::generate(),"title":format!("Large project {index:02}")}),
            ),
        ] {
            let mut request = fixture.header();
            request
                .as_object_mut()
                .ok_or("request object")?
                .extend(fields.as_object().ok_or("fields")?.clone());
            if tool == "project_create" {
                fixture
                    .prepare_registered_creation(&serde_json::from_value(request.clone())?)
                    .await?;
            }
            accepted(&fixture.api.call(tool, request).await?);
        }
    }
    for (tool, mut request, expected_count) in [
        (
            "workspace_list",
            json!({"repository":null,"page":{"limit":100,"cursor":null}}),
            61,
        ),
        (
            "board_list",
            json!({"workspaceId":fixture.workspace,"includeArchived":true,"page":{"limit":100,"cursor":null}}),
            61,
        ),
        // Project directory rows no longer hold descriptions: ordinary bounded paging still traverses all rows.
        (
            "project_list",
            json!({"boardId":fixture.board,"page":{"limit":1,"cursor":null}}),
            60,
        ),
    ] {
        let mut count = 0;
        let mut pages = 0;
        loop {
            let response = fixture.api.call(tool, request.clone()).await?;
            assert!(
                serde_json::to_vec(&response)?.len()
                    < collaboration_protocol::MAX_CONTROL_FRAME_BYTES
            );
            let records = response
                .pointer("/result/page/records")
                .and_then(Value::as_array)
                .ok_or("metadata records missing")?;
            count += records.len();
            pages += 1;
            let cursor = response
                .pointer("/result/page/nextCursor")
                .ok_or("cursor missing")?;
            if cursor.is_null() {
                break;
            }
            *request
                .pointer_mut("/page/cursor")
                .ok_or("request cursor missing")? = cursor.clone();
        }
        assert_eq!(count, expected_count, "{tool}");
        assert!(pages > 1, "{tool} did not continue");
    }
    fixture.stop().await
}
