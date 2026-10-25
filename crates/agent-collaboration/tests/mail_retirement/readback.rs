//! Read-only central custody evidence, independent of sender status projections.
use crate::fixture::BoardFixture;
use crate::owned_host::BoardHost;
use codex_router_test_support::FixtureResult;
use serde_json::{Value, json};
use sqlx_turso::sqlx::{ConnectOptions, Connection, Row};
use std::time::Duration;

pub async fn terminal(host: &BoardHost, sender: &Value, item: &str) -> FixtureResult<Value> {
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let shown = host
                .mcp("router_show", json!({"caller":sender,"reference":item}))
                .await?;
            let status = shown
                .pointer("/mail/status")
                .ok_or_else(|| format!("no mail status: {shown}"))?;
            if matches!(
                status.get("kind").and_then(Value::as_str),
                Some("delivered" | "refused" | "cancelled" | "expired" | "uncertain")
            ) {
                return Ok(status.clone());
            }
            tokio::task::yield_now().await;
        }
    })
    .await?
}

async fn accounting(
    fixture: &BoardFixture,
    service: &str,
    item: &str,
) -> FixtureResult<Option<(i64, i64, i64, i64)>> {
    let path = fixture
        .root
        .path()
        .join("central")
        .join(format!("mailbox-{service}.db"));
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(path)
        .read_only(true)
        .create_if_missing(false)
        .connect()
        .await?;
    let result=async {
        let row=sqlx_turso::sqlx::query::<sqlx_turso::Turso>("SELECT live,capacity_slot,payload_bytes,settlement_bytes,record FROM _fleet_sync_mail_items WHERE item_id=?")
            .bind(item).fetch_optional(&mut connection).await?;
        let Some(row)=row else {return Ok(None)};
        let accounting=(row.try_get("live")?,row.try_get("capacity_slot")?,row.try_get("payload_bytes")?,row.try_get("settlement_bytes")?);
        if accounting == (0,0,0,0) {
            let record: Value=serde_json::from_str(&row.try_get::<String,_>("record")?)?;
            assert_eq!(record.get("kind"),Some(&json!("compacted")));
            assert_eq!(record.pointer("/value/id"),Some(&json!(item)));
            assert!(record.pointer("/value/version").and_then(Value::as_u64).is_some_and(|version|version>0));
            let status: fleet_sync_protocol::DeliveryStatus<
                <board_sync_protocol::BoardDeliveryTypes as fleet_sync_protocol::DeliveryTypes>::Acceptance
            >=serde_json::from_value(record.pointer("/value/status").ok_or("canonical status missing")?.clone())?;
            assert!(status.is_terminal(),"retirement lost its terminal replay evidence");
            let _: fleet_sync_protocol::ItemDigest=serde_json::from_value(record.pointer("/value/digest").ok_or("canonical digest missing")?.clone())?;
        }
        Ok(Some(accounting))
    }.await;
    let closed = connection.close().await;
    match (result, closed) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), _) => Err(error),
        (_, Err(error)) => Err(error.into()),
    }
}

pub async fn await_compacted(fixture: &BoardFixture, service: &str, item: &str) -> FixtureResult {
    tokio::time::timeout(Duration::from_secs(12), async {
        loop {
            if accounting(fixture, service, item).await? == Some((0, 0, 0, 0)) {
                return Ok(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .map_err(|_| "terminal mail never retired: central capacity/reservation remains occupied")?
}

pub async fn require_compacted(fixture: &BoardFixture, service: &str, item: &str) -> FixtureResult {
    assert_eq!(
        accounting(fixture, service, item).await?,
        Some((0, 0, 0, 0)),
        "retirement must release custody, capacity and reservation while retaining the row"
    );
    Ok(())
}

/// The native file has one owner: reopen only after the exact sender Host exited.
pub async fn require_cold_terminal(directory: &std::path::Path, item: &str) -> FixtureResult {
    let mut connection = sqlx_turso::TursoConnectOptions::new()
        .filename(directory.join("interaction.sqlite"))
        .read_only(true)
        .create_if_missing(false)
        .connect()
        .await?;
    let result = async {
        let record = sqlx_turso::sqlx::query_scalar::<sqlx_turso::Turso, String>(
            "SELECT record FROM _fleet_sync_outbox_items WHERE item_id=?",
        )
        .bind(item)
        .fetch_one(&mut connection)
        .await?;
        let record: Value = serde_json::from_str(&record)?;
        assert_eq!(record.pointer("/value/id"), Some(&json!(item)));
        assert_eq!(
            record.pointer("/value/status/kind"),
            Some(&json!("delivered"))
        );
        Ok(())
    }
    .await;
    let closed = connection.close().await;
    result.and(closed.map_err(Into::into))
}

/// Public requested controls must not append handoffs, group expansions, decisions or feed rows.
pub async fn project_control_counts(sender: &BoardFixture) -> FixtureResult<Value> {
    if sender.host.is_some() {
        return Err("project readback requires a joined Host".into());
    }
    let path = sender
        .directory
        .join("projects")
        .join(&sender.project)
        .join("e1.sqlite");
    let connection = sqlx_turso::TursoConnectOptions::new()
        .filename(path)
        .read_only(true)
        .create_if_missing(false)
        .connect()
        .await?;
    let mut result = None;
    codex_router_test_support::scenario_finalization::run_scenario(
        connection,
        async |connection: &mut sqlx_turso::TursoConnection| {
            let json = sqlx_turso::sqlx::query_scalar::<sqlx_turso::Turso, String>(
                "SELECT json_object('handoffs',(SELECT COUNT(*) FROM mail_handoffs),'expansions',(SELECT COUNT(*) FROM mail_cancel_expansions),'decisions',(SELECT COUNT(*) FROM request_decisions),'feed',(SELECT COUNT(*) FROM project_feed))")
                .fetch_one(connection).await?;
            result = Some(serde_json::from_str(&json)?);
            Ok(())
        },
        |connection| async { connection.close().await.map_err(Into::into) },
        || Ok(()),
    ).await?;
    result.ok_or_else(|| "project control readback result absent".into())
}
