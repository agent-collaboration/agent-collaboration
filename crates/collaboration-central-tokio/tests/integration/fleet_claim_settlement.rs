//! Claim settlement cannot become a second writer of the lease directory view.
use super::composed_fleet_fixture::{Fixture, NOW, PROJECT, TestResult};
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use serde_json::{Value, json};
use sqlx_core::{query::query, row::Row};

pub(super) async fn settle(
    fixture: &Fixture,
    operation: durable_steps::Uuid,
    attempt: durable_steps::Uuid,
    projection: Value,
) -> TestResult<Value> {
    let reply = fixture
        .runtime
        .on_call(settlement_call(operation, attempt, projection)?)
        .await
        .map_err(|error| format!("real claim settlement: {error:?}"))?;
    Ok(serde_json::from_slice(&reply)?)
}

pub(super) fn settlement_call(
    operation: durable_steps::Uuid,
    attempt: durable_steps::Uuid,
    projection: Value,
) -> TestResult<Vec<u8>> {
    Ok(serde_json::to_vec(&json!({
        "organization":"019f0000-0000-7000-8000-00000000ac01","source":{"kind":"fleet","value":{"organization":"019f0000-0000-7000-8000-00000000ac01"}},"sourceFace":"registry","authority":{"kind":"object"},
        "destination":{"kind":"fleet","value":{"organization":"019f0000-0000-7000-8000-00000000ac01"}},"destinationFace":"fleet",
        "identity":{"operation":operation,"step":"settle_claim"},
        "kind":"central.claim-granted","target":null,"request":{"attempt":attempt,"projection":projection}
    }))?)
}

pub(super) async fn receipts(fixture: &Fixture) -> TestResult<i64> {
    Ok(fixture
        .store
        .transaction(|scope| {
            Box::pin(async move {
                query::<sqlx_turso::Turso>(
                    "SELECT COUNT(*) AS count FROM lease_claim_settlement_receipts",
                )
                .fetch_one(scope.executor())
                .await
                .map_err(StoreError::backend)?
                .try_get("count")
                .map_err(StoreError::backend)
            })
        })
        .await?)
}

#[tokio::test]
async fn settlement_receipts_observe_the_hook_view_and_never_publish_a_newer_projection()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    let proof = async {
        fixture.arrange_registered().await?;
        let before = fixture.lease_and_notices().await?;
        let committed: Value = serde_json::from_str(before.1.as_ref().ok_or("hook view absent")?)?;
        let mut newer = committed.clone();
        newer["incarnation"] = json!(
            committed
                .get("incarnation")
                .and_then(Value::as_u64)
                .ok_or("hook incarnation absent")?
                .checked_add(1)
                .ok_or("fixture incarnation overflow")?
        );
        let attempt = durable_steps::Uuid::now_v7();
        let operation = durable_steps::Uuid::now_v7();

        // A valid content digest and a higher version used to let this callback publish a new view.
        let answer = settle(&fixture, operation, attempt, newer.clone()).await?;
        if answer.get("kind") != Some(&json!("conflict"))
            || fixture.lease_and_notices().await? != before
            || receipts(&fixture).await? != 0
        {
            return Err(
                "claim settlement wrote or receipted a projection the lease hook never committed"
                    .into(),
            );
        }
        if settle(&fixture, operation, attempt, committed).await? != json!({"kind":"duplicate"})
            || receipts(&fixture).await? != 1
            || fixture.lease_and_notices().await? != before
        {
            return Err(
                "exact committed claim did not settle once without changing the view".into(),
            );
        }
        // Replay is the same receipted call even if a caller refreshes its projection payload.
        if settle(&fixture, operation, attempt, newer).await? != json!({"kind":"duplicate"})
            || receipts(&fixture).await? != 1
            || fixture.lease_and_notices().await? != before
        {
            return Err("settlement replay changed its original receipt or the lease view".into());
        }
        let state = fixture
            .store
            .transaction(move |scope| {
                Box::pin(async move {
                    let row = query::<sqlx_turso::Turso>(
                        "SELECT project_id,state_kind,outcome FROM lease_claim_reservations WHERE attempt_id=?",
                    )
                    .bind(attempt.to_string())
                    .fetch_one(scope.executor())
                    .await
                    .map_err(StoreError::backend)?;
                    Ok((
                        row.try_get::<String, _>("project_id")
                            .map_err(StoreError::backend)?,
                        row.try_get::<String, _>("state_kind")
                            .map_err(StoreError::backend)?,
                        row.try_get::<Option<String>, _>("outcome").map_err(StoreError::backend)?,
                    ))
                })
            })
            .await?;
        if state
            != (
                PROJECT.to_owned(),
                "settled".to_owned(),
                Some("granted".to_owned()),
            )
        {
            return Err(format!(
                "claim reservation did not retain its exact granted outcome: {state:?}"
            )
            .into());
        }
        Ok(())
    }
    .await;
    fixture.close().await?;
    proof
}
