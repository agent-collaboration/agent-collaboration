//! Retained maximum escaped locator paging and corrupt stored-locator rejection.
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
use sqlx::{ConnectOptions, Connection};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn max_bound_escaped_locators_page_fully_below_the_frame_limit() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let mut expected = Vec::new();
    for marker in ['a', 'b'] {
        let common_directory = format!("/{marker}{}", "\\\"".repeat(2047));
        assert_eq!(common_directory.len(), 4096);
        let repository = RepositoryRef::Local {
            service_id: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())?,
            common_directory: CommonDirectory::try_from(common_directory)?,
        };
        let mut request = fixture.header();
        request.as_object_mut().ok_or("request")?.extend(
            json!({"workspaceId":fixture.workspace,"repository":repository})
                .as_object()
                .ok_or("fields")?
                .clone(),
        );
        accepted(&fixture.api.call("repository_attach", request).await?);
        expected.push(repository);
    }
    let mut request = json!({"workspaceId":fixture.workspace,"page":{"limit":1,"cursor":null}});
    let mut records = Vec::new();
    loop {
        let reply = fixture.api.call("repository_list", request.clone()).await?;
        assert!(
            serde_json::to_vec(&reply)?.len() < collaboration_protocol::MAX_CONTROL_FRAME_BYTES
        );
        for value in reply
            .pointer("/result/page/records")
            .and_then(Value::as_array)
            .ok_or("records missing")?
        {
            records.push(serde_json::from_value::<RepositoryRef>(value.clone())?);
        }
        let cursor = reply
            .pointer("/result/page/nextCursor")
            .ok_or("cursor missing")?;
        if cursor.is_null() {
            break;
        }
        *request
            .pointer_mut("/page/cursor")
            .ok_or("request cursor missing")? = cursor.clone();
    }
    assert_eq!(records, expected);
    fixture.stop().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn oversized_stored_locator_is_rejected_by_domain_decoding() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let path = fixture._directory.path().join("fleet-cache.sqlite");
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(path)
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
        .pointer_mut("/record/repositories")
        .ok_or("repositories missing")? = json!([{"kind":"local","serviceId":"00000000-0000-4000-8000-000000000001","commonDirectory":format!("/{}","x".repeat(4096))}]);
    sqlx::query("UPDATE cached_fleet_records SET payload=? WHERE kind='workspace' AND id=?")
        .bind(stored.to_string())
        .bind(fixture.workspace.as_str())
        .execute(&mut connection)
        .await?;
    connection.close().await?;
    let failure = fixture
        .api
        .call(
            "repository_list",
            json!({"workspaceId":fixture.workspace,"page":{"limit":10,"cursor":null}}),
        )
        .await?;
    assert_eq!(
        failure.pointer("/error/data/kind"),
        Some(&json!("invalidRecord")),
        "{failure}"
    );
    assert_eq!(
        failure.pointer("/error/data/details/resource"),
        Some(&json!({"kind":"workspace","workspaceId":fixture.workspace}))
    );
    fixture.stop().await
}
