//! Fleet projection catch-up reads its checkpoint, independent of unrelated cached bodies.
#[path = "support/board_fixture.rs"]
pub mod board_fixture;
#[path = "support/board_read_setup.rs"]
mod board_read_setup;
#[path = "support/read_cost_counter.rs"]
mod read_cost_counter;
use board_fixture::{BoardFixture, TestError, accepted};
use board_read_setup::keyed_call;
use message_board::*;
use read_cost_counter::{ReadCost, current_cost};
use serde_json::json;
use sqlx::{ConnectOptions, Connection};

#[allow(clippy::panic_in_result_fn)]
async fn update_cost(size: usize) -> Result<ReadCost, TestError> {
    let fixture = BoardFixture::start().await?;
    for index in 0..size {
        accepted(&keyed_call(&fixture, "workspace_create", json!({"workspaceId":WorkspaceId::generate(),"name":format!("unrelated {index}"),"description":""})).await?);
    }
    let before = current_cost();
    accepted(&keyed_call(&fixture, "workspace_update", json!({"workspaceId":fixture.workspace,"name":"updated workspace","description":"scoped projection"})).await?);
    let cost = current_cost().since(before);
    let shown = fixture
        .api
        .call("workspace_show", json!({"workspaceId":fixture.workspace}))
        .await?;
    assert_eq!(
        shown.pointer("/result/workspace/name"),
        Some(&json!("updated workspace"))
    );
    fixture.stop().await?;
    Ok(cost)
}

#[allow(clippy::panic_in_result_fn)]
async fn corruption_isolation() -> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let unrelated = WorkspaceId::generate();
    accepted(
        &keyed_call(
            &fixture,
            "workspace_create",
            json!({"workspaceId":unrelated,"name":"unrelated corrupt workspace","description":""}),
        )
        .await?,
    );
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(fixture._directory.path().join("fleet-cache.sqlite"))
        .connect()
        .await?;
    let changed = sqlx::query("UPDATE cached_fleet_records SET payload = json_set(payload, '$.record.name', '') WHERE kind = 'workspace' AND id = ?")
        .bind(unrelated.as_str()).execute(&mut connection).await?;
    assert_eq!(changed.rows_affected(), 1);
    connection.close().await?;
    accepted(&keyed_call(&fixture, "workspace_update", json!({"workspaceId":fixture.workspace,"name":"healthy after corruption","description":"checkpoint only"})).await?);
    let healthy = fixture
        .api
        .call("workspace_show", json!({"workspaceId":fixture.workspace}))
        .await?;
    assert_eq!(
        healthy.pointer("/result/workspace/name"),
        Some(&json!("healthy after corruption"))
    );
    let corrupt = fixture
        .api
        .call("workspace_show", json!({"workspaceId":unrelated}))
        .await?;
    assert_eq!(
        corrupt.pointer("/error/data/kind"),
        Some(&json!("invalidRecord")),
        "{corrupt}"
    );
    fixture.stop().await
}

#[tokio::test]
#[allow(clippy::panic_in_result_fn)]
async fn fleet_write_reads_only_its_operands_and_survives_an_unrelated_corrupt_cache_record()
-> Result<(), TestError> {
    read_cost_counter::install();
    let small = update_cost(8).await?;
    let large = update_cost(80).await?;
    eprintln!("fleet write costs: small={small:?}, large={large:?}");
    assert_eq!(
        small, large,
        "fleet write decoded unrelated cached directory bodies"
    );
    assert_eq!(
        large.records, 3,
        "workspace operand and two incoming fleet feed records"
    );
    assert!(large.statements > 0);
    corruption_isolation().await
}
