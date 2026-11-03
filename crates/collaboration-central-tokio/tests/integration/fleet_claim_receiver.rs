//! Claim callbacks receipt committed hook facts; they never write a competing view.
use super::{
    composed_fleet_fixture::*,
    fleet_claim_adapter_fixture::{claim_counts, prepare_fleet, read_receipt},
    fleet_claim_settlement::{settle, settlement_call},
};
use durable_steps::{AtomicStore, CallTarget, OperationId, StoreError, storage::TransactionScope};
use fleet_sync_protocol::{
    ClaimReservation, ClaimSettlement, GateAnswer, HolderChangeRequest, SettleOutcome,
};
use serde_json::{Value, json};
use sqlx_core::{query::query, row::Row};

async fn claim_gate(
    fixture: &Fixture,
    operation: durable_steps::Uuid,
    attempt: durable_steps::Uuid,
    cancelled: bool,
) -> TestResult<Value> {
    let change = HolderChangeRequest::Claim {
        attempt: fleet_sync_protocol::TransferId::new(attempt)?,
        database: PROJECT.to_owned().try_into()?,
        claimant: A.to_owned().try_into()?,
    };
    let (kind, request) = if cancelled {
        (
            fleet_sync_lease::ClaimSettlement::KIND,
            serde_json::to_value(ClaimSettlement {
                change,
                outcome: SettleOutcome::Cancelled,
            })?,
        )
    } else {
        (
            fleet_sync_lease::ClaimReservation::KIND,
            serde_json::to_value(ClaimReservation { change })?,
        )
    };
    let reply = fixture.runtime.on_call(serde_json::to_vec(&json!({
        "source":{"kind":"fleet"},"sourceFace":"registry","authority":{"kind":"object"},
        "destination":{"kind":"fleet"},"destinationFace":"fleet",
        "identity":{"operation":operation,"step":"claim_gate"},"kind":kind,"target":null,"request":request
    }))?).await.map_err(|failure|format!("real claim gate: {failure:?}"))?;
    Ok(serde_json::from_slice(&reply)?)
}

async fn reservation(
    fixture: &Fixture,
    attempt: durable_steps::Uuid,
) -> TestResult<(String, Option<String>)> {
    Ok(fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                let row = query::<sqlx_turso::Turso>(
                    "SELECT state_kind,outcome FROM lease_claim_reservations WHERE attempt_id=?",
                )
                .bind(attempt.to_string())
                .fetch_one(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                Ok((
                    row.try_get("state_kind").map_err(StoreError::backend)?,
                    row.try_get("outcome").map_err(StoreError::backend)?,
                ))
            })
        })
        .await?)
}

fn projection(view: &Option<String>) -> TestResult<Value> {
    Ok(serde_json::from_str(
        view.as_deref().ok_or("genuine hook view absent")?,
    )?)
}

async fn renew(fixture: &Fixture) -> TestResult {
    fixture.io.advance(10_000);
    let answer = fixture.call("session","central.session-lease",Value::Null,json!({
        "session":{"peer":A,"sessionId":durable_steps::Uuid::now_v7()},
        "request":{"kind":"renew","value":{"attempt":durable_steps::Uuid::now_v7(),"database":PROJECT,"epoch":1}}
    })).await?;
    if !matches!(
        serde_json::from_value::<fleet_sync_protocol::LeaseAnswer>(answer)?,
        fleet_sync_protocol::LeaseAnswer::Renew(fleet_sync_protocol::RenewAnswer::Renewed {
            replayed: false,
            ..
        })
    ) {
        return Err("genuine renewal did not commit".into());
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Receipt replay must keep its original result and metadata even when the real lease view advances."
)]
async fn claim_granted_receipts_the_hook_projection_and_replays_its_original_result() -> TestResult
{
    let root = tempfile::tempdir()?;
    let fixture = prepare_fleet(root.path(), false).await?;
    let before = fixture.lease_and_notices().await?;
    let committed = projection(&before.1)?;
    let operation = durable_steps::Uuid::now_v7();
    let attempt = durable_steps::Uuid::now_v7();
    let call = settlement_call(operation, attempt, committed.clone())?;
    let original_bytes = fixture.runtime.on_call(call.clone()).await?;
    let first: Value = serde_json::from_slice(&original_bytes)?;
    assert_eq!(first, json!({"kind":"duplicate"}));
    let receipt = read_receipt(&fixture, OperationId::new(operation)?).await?;
    assert_eq!(receipt.count, 1);
    assert_eq!(receipt.step, "settle_claim");
    assert_eq!(receipt.operation, operation.to_string());
    assert_eq!(
        durable_steps::Uuid::parse_str(&receipt.id)?.get_version_num(),
        7
    );
    assert_eq!((receipt.created_at, receipt.updated_at), (NOW, NOW));
    assert_eq!(receipt.result, first);
    assert_eq!(
        receipt.reservation,
        json!({"kind":"settled","value":{"outcome":"granted"}})
    );
    assert_eq!(fixture.runtime.on_call(call).await?, original_bytes);
    assert_eq!(
        read_receipt(&fixture, OperationId::new(operation)?).await?,
        receipt
    );
    assert_eq!(fixture.lease_and_notices().await?, before);
    assert_eq!(committed.pointer("/holder"), Some(&json!(SERVICE_A)));
    assert_eq!(committed.pointer("/snapshot/grant/holder"), Some(&json!(A)));
    assert_eq!(committed.pointer("/snapshot/changedAt"), Some(&json!(NOW)));
    renew(&fixture).await?;
    let newer = fixture.lease_and_notices().await?;
    assert_eq!(
        settle(&fixture, operation, attempt, projection(&newer.1)?).await?,
        first
    );
    assert_eq!(
        read_receipt(&fixture, OperationId::new(operation)?).await?,
        receipt
    );
    assert_eq!(fixture.lease_and_notices().await?, newer);
    fixture.close().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A real late receipt write abort must retain the active reservation and every committed hook fact until retry."
)]
async fn receipt_write_failure_keeps_the_hook_view_and_reservation_active_until_retry() -> TestResult
{
    let root = tempfile::tempdir()?;
    let fixture = prepare_fleet(root.path(), false).await?;
    let attempt = durable_steps::Uuid::now_v7();
    assert_eq!(
        claim_gate(&fixture, durable_steps::Uuid::now_v7(), attempt, false).await?,
        json!({"kind":"reserved"})
    );
    let before = fixture.lease_and_notices().await?;
    let committed = projection(&before.1)?;
    let operation = durable_steps::Uuid::now_v7();
    fixture.sql("CREATE TRIGGER fail_claim_receipt BEFORE INSERT ON lease_claim_settlement_receipts BEGIN SELECT RAISE(ABORT,'claim receipt fixture fault'); END").await?;
    let call = settlement_call(operation, attempt, committed.clone())?;
    assert!(matches!(
        fixture.runtime.on_call(call).await,
        Err(durable_steps::CallFailure::AnswerLost)
    ));
    assert_eq!(
        reservation(&fixture, attempt).await?,
        ("active".into(), None)
    );
    assert_eq!(claim_counts(&fixture).await?, (1, 0));
    assert_eq!(fixture.lease_and_notices().await?, before);
    fixture.sql("DROP TRIGGER fail_claim_receipt").await?;
    assert_eq!(
        settle(&fixture, operation, attempt, committed).await?,
        json!({"kind":"duplicate"})
    );
    assert_eq!(
        reservation(&fixture, attempt).await?,
        ("settled".into(), Some("granted".into()))
    );
    assert_eq!(claim_counts(&fixture).await?, (1, 1));
    assert_eq!(fixture.lease_and_notices().await?, before);
    fixture.close().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "An untrusted caller or altered digest cannot create a claim row, receipt, or overwrite the hook view."
)]
async fn untrusted_source_and_invalid_snapshot_digest_create_no_claim_or_receipt() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = prepare_fleet(root.path(), false).await?;
    let before = fixture.lease_and_notices().await?;
    let valid = json!({
        "source":{"kind":"fleet"},"sourceFace":"registry","authority":{"kind":"object"},
        "destination":{"kind":"fleet"},"destinationFace":"fleet",
        "identity":{"operation":durable_steps::Uuid::now_v7(),"step":"negative_claim"},
        "kind":"central.claim-granted","target":null,"request":{"attempt":durable_steps::Uuid::now_v7(),"projection":projection(&before.1)?}
    });
    let mut foreign = valid.clone();
    foreign["sourceFace"] = json!("session");
    foreign["authority"] = json!({"kind":"machine","value":{"installation":SERVICE_A}});
    let mut invalid_digest = valid;
    invalid_digest["request"]["projection"]["digest"] = json!("0".repeat(64));
    for call in [foreign, invalid_digest] {
        assert!(matches!(
            fixture.runtime.on_call(serde_json::to_vec(&call)?).await,
            Err(durable_steps::CallFailure::NotSent { .. })
        ));
        assert_eq!(claim_counts(&fixture).await?, (0, 0));
        assert_eq!(fixture.lease_and_notices().await?, before);
    }
    fixture.close().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "An earlier genuine snapshot settles Older once while retaining the newer hook view."
)]
async fn an_earlier_claim_settles_without_overwriting_a_later_committed_revision() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = prepare_fleet(root.path(), false).await?;
    let old = projection(&fixture.lease_and_notices().await?.1)?;
    let attempt = durable_steps::Uuid::now_v7();
    assert_eq!(
        claim_gate(&fixture, durable_steps::Uuid::now_v7(), attempt, false).await?,
        json!({"kind":"reserved"})
    );
    renew(&fixture).await?;
    let newer = fixture.lease_and_notices().await?;
    assert_eq!(
        reservation(&fixture, attempt).await?,
        ("active".into(), None)
    );
    let operation = durable_steps::Uuid::now_v7();
    assert_eq!(
        settle(&fixture, operation, attempt, old).await?,
        json!({"kind":"older","value":{"stored":{"epoch":1,"incarnation":2,"change":2}}})
    );
    assert_eq!(
        reservation(&fixture, attempt).await?,
        ("settled".into(), Some("granted".into()))
    );
    assert_eq!(
        read_receipt(&fixture, OperationId::new(operation)?)
            .await?
            .count,
        1
    );
    assert_eq!(fixture.lease_and_notices().await?, newer);
    fixture.close().await
}

fn require_settled(answer: Value) -> TestResult {
    let GateAnswer::Refused { reason } = serde_json::from_value(answer)? else {
        return Err("settled claim reactivated".into());
    };
    if reason.code.as_str() != "claim_already_settled" || !reason.detail.contains("granted") {
        return Err(format!("grant tombstone lost its refusal: {reason:?}").into());
    }
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "A granted tombstone refuses its old reserve CallId, a fresh reserve, and conflicting cancellation across owning reopen."
)]
async fn granted_attempt_refuses_the_original_reserve_call_even_after_reopen() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = prepare_fleet(root.path(), false).await?;
    let attempt = durable_steps::Uuid::now_v7();
    let reserve_call = durable_steps::Uuid::now_v7();
    for _ in 0..2 {
        assert_eq!(
            claim_gate(&fixture, reserve_call, attempt, false).await?,
            json!({"kind":"reserved"})
        );
    }
    let before = fixture.lease_and_notices().await?;
    assert_eq!(
        settle(
            &fixture,
            durable_steps::Uuid::now_v7(),
            attempt,
            projection(&before.1)?
        )
        .await?,
        json!({"kind":"duplicate"})
    );
    require_settled(claim_gate(&fixture, reserve_call, attempt, false).await?)?;
    assert_eq!(
        reservation(&fixture, attempt).await?,
        ("settled".into(), Some("granted".into()))
    );
    assert!(
        claim_gate(&fixture, durable_steps::Uuid::now_v7(), attempt, true)
            .await
            .is_err()
    );
    assert_eq!(fixture.lease_and_notices().await?, before);
    fixture.close().await?;
    let reopened = Fixture::open(root.path(), NOW).await?;
    for call in [reserve_call, durable_steps::Uuid::now_v7()] {
        require_settled(claim_gate(&reopened, call, attempt, false).await?)?;
    }
    assert_eq!(
        reservation(&reopened, attempt).await?,
        ("settled".into(), Some("granted".into()))
    );
    assert_eq!(claim_counts(&reopened).await?, (1, 1));
    assert_eq!(reopened.lease_and_notices().await?, before);
    reopened.close().await
}
