//! Typed settlement debt is a component input; the Fleet engine, hook and receiver are real.
use super::composed_fleet_fixture::*;
use collaboration_central_conformance::{DispatchFaultObservation, DispatchKey, RuntimeControl};
use collaboration_central_core::{CoreHost, ObjectAddress};
use durable_steps::{
    AtomicStore, JournalScope, OperationId, RequestKey, StoreError, storage::TransactionScope,
};
use fleet_project_lease::SettleReservationTask;
use fleet_protocol::{ClaimSettlement, Epoch, HolderChangeRequest, SettleOutcome, TransferId};
use fleet_tokio_objects::TokioObject;
use serde_json::{Value, json};
use sqlx_core::{query::query, row::Row};
use std::{path::Path, time::Duration};

use super::typed_claim_observer as claim_observer;
type SettlementStore = TokioObject<CoreHost<Io>>;
pub const WAIT_BOUND: Duration = Duration::from_secs(5);

#[derive(Debug, PartialEq)]
pub struct ReceiptSnapshot {
    pub id: String,
    pub operation: String,
    pub step: String,
    pub result: Value,
    pub reservation: Value,
    pub created_at: i64,
    pub updated_at: i64,
    pub count: i64,
}

pub async fn prepare_fleet(root: &Path, successor: bool) -> TestResult<Fixture> {
    let fixture = Fixture::open(root, NOW).await?;
    fixture.arrange_registered().await?;
    if successor {
        let preview = fixture.owner(json!({"kind":"preview"})).await?;
        let preview = preview
            .pointer("/value/value/preview")
            .cloned()
            .ok_or("real takeover preview absent")?;
        let changed = fixture
            .owner(json!({"kind":"takeover","value":{
                "transferId":durable_steps::Uuid::now_v7(),"toInstallation":SERVICE_B,
                "reason":"claim settlement observes the genuine successor","preview":preview
            }}))
            .await?;
        if changed.pointer("/value/kind") != Some(&json!("granted")) {
            return Err(format!("successor was not genuinely granted: {changed}").into());
        }
    }
    // Drain only setup's causally admitted work, without advancing the clock.
    for _ in 0..8 {
        let report = fixture.runtime.conformance_drive_operations().await?;
        if !report.failed.is_empty() || !report.retried.is_empty() {
            return Err(format!("setup did not finish: {report:?}").into());
        }
        if report.completed.is_empty() && report.yielded.is_empty() {
            return Ok(fixture);
        }
    }
    Err("setup failed to reach a causal idle boundary".into())
}

pub async fn admit_settlement(fixture: &Fixture, granted_epoch: u64) -> TestResult<OperationId> {
    admit_settlement_outcome(
        fixture,
        SettleOutcome::Granted {
            epoch: Epoch::new(granted_epoch)?,
        },
    )
    .await
}

pub async fn admit_settlement_outcome(
    fixture: &Fixture,
    outcome: SettleOutcome,
) -> TestResult<OperationId> {
    let input = ClaimSettlement {
        change: HolderChangeRequest::Claim {
            attempt: TransferId::new(durable_steps::Uuid::now_v7())?,
            database: PROJECT.to_owned().try_into()?,
            claimant: A.to_owned().try_into()?,
        },
        outcome,
    };
    let key = RequestKey::new(format!("claim-adapter:{}", durable_steps::Uuid::now_v7()))?;
    Ok(fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                scope
                    .admit::<SettleReservationTask<SettlementStore>>(&key, &input)
                    .await
                    .map(|record| record.id())
                    .map_err(StoreError::backend)
            })
        })
        .await?)
}

pub async fn read_receipt(
    fixture: &Fixture,
    operation: OperationId,
) -> TestResult<ReceiptSnapshot> {
    Ok(fixture.store.transaction(move |scope| Box::pin(async move {
        let row = query::<sqlx_turso::Turso>("SELECT c.receipt_id,c.operation_id,c.step_name,c.result_kind,c.stored_epoch,c.stored_incarnation,c.stored_change,c.created_at,c.updated_at,r.state_kind,r.outcome,(SELECT count(*) FROM lease_claim_settlement_receipts) AS count FROM lease_claim_settlement_receipts c JOIN lease_claim_reservations r ON r.attempt_id=c.attempt_id WHERE c.operation_id=?")
            .bind(operation.as_uuid().to_string()).fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
        Ok(ReceiptSnapshot {
            id: row.try_get("receipt_id").map_err(StoreError::backend)?,
            operation: row.try_get("operation_id").map_err(StoreError::backend)?,
            step: row.try_get("step_name").map_err(StoreError::backend)?,
            result: claim_observer::receipt_result(&row)?,
            reservation: claim_observer::reservation_state(&row)?.ok_or_else(||StoreError::backend("settled reservation absent"))?,
            created_at: row.try_get("created_at").map_err(StoreError::backend)?,
            updated_at: row.try_get("updated_at").map_err(StoreError::backend)?,
            count: row.try_get("count").map_err(StoreError::backend)?,
        })
    })).await?)
}

pub async fn claim_counts(fixture: &Fixture) -> TestResult<(i64, i64)> {
    Ok(fixture.store.transaction(|scope| Box::pin(async move {
        let row=query::<sqlx_turso::Turso>("SELECT (SELECT count(*) FROM lease_claim_reservations) AS reservations,(SELECT count(*) FROM lease_claim_settlement_receipts) AS receipts")
            .fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
        Ok((row.try_get("reservations").map_err(StoreError::backend)?,row.try_get("receipts").map_err(StoreError::backend)?))
    })).await?)
}

pub fn settlement_key() -> DispatchKey {
    DispatchKey::any(ObjectAddress::Fleet)
}

pub async fn hold_next_settlement(fixture: &Fixture) -> TestResult {
    fixture
        .io
        .local_call_faults
        .control(RuntimeControl::HoldNextDispatch {
            key: settlement_key(),
        })
        .await?;
    Ok(())
}

pub async fn held_settlement(fixture: &Fixture) -> TestResult<Value> {
    held_settlement_kind(fixture, "central.claim-granted").await
}

pub async fn held_settlement_kind(fixture: &Fixture, kind: &str) -> TestResult<Value> {
    let key = settlement_key();
    let observed = tokio::time::timeout(
        WAIT_BOUND,
        fixture
            .io
            .local_call_faults
            .control(RuntimeControl::AwaitHeldDispatch { key: key.clone() }),
    )
    .await??;
    let DispatchFaultObservation::Held { request, .. } = observed.dispatch(&key) else {
        return Err("settlement never reached its actual local dispatch barrier".into());
    };
    let call: Value = serde_json::from_slice(&request)?;
    if call.get("kind") != Some(&json!(kind)) {
        return Err(format!("settlement barrier captured another call: {call}").into());
    }
    Ok(call)
}

pub async fn resume_settlement(fixture: &Fixture) -> TestResult {
    fixture
        .io
        .local_call_faults
        .control(RuntimeControl::ResumeDispatch {
            key: settlement_key(),
        })
        .await?;
    Ok(())
}

pub async fn drive_fleet(fixture: &Fixture) -> TestResult<durable_steps::DriveReport> {
    Ok(fixture.runtime.conformance_drive_operations().await?)
}
