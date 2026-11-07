//! Approved typed claim rows preserve state, receipts and their injected timestamps.
use super::{
    composed_fleet_fixture::*,
    fleet_claim_settlement::{receipts, settle},
};
use durable_steps::{AtomicStore, CallTarget, StoreError, storage::TransactionScope};
use fleet_protocol::{ClaimReservation, ClaimSettlement, HolderChangeRequest, SettleOutcome};
use serde_json::{Value, json};
use sqlx_core::{query::query, row::Row};
type ReceiptObservation = (
    String,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    i64,
    i64,
);

async fn call_claim_gate(
    fixture: &Fixture,
    attempt: durable_steps::Uuid,
    refuse: bool,
) -> TestResult<Value> {
    let change = HolderChangeRequest::Claim {
        attempt: fleet_protocol::TransferId::new(attempt)?,
        database: PROJECT.to_owned().try_into()?,
        claimant: B.to_owned().try_into()?,
    };
    let (kind, request) = if refuse {
        (
            fleet_project_lease::ClaimSettlement::KIND,
            serde_json::to_value(ClaimSettlement {
                change,
                outcome: SettleOutcome::Cancelled,
            })?,
        )
    } else {
        (
            fleet_project_lease::ClaimReservation::KIND,
            serde_json::to_value(ClaimReservation { change })?,
        )
    };
    let answer=fixture.runtime.on_call(serde_json::to_vec(&json!({
        "source":{"kind":"fleet"},"sourceFace":"registry","authority":{"kind":"object"},"destination":{"kind":"fleet"},"destinationFace":"fleet",
        "identity":{"operation":durable_steps::Uuid::now_v7(),"step":"claim_columns"},"kind":kind,"target":null,"request":request
    }))?).await.map_err(|error|format!("claim gate: {error:?}"))?;
    Ok(serde_json::from_slice(&answer)?)
}
async fn read_reservation(
    fixture: &Fixture,
    attempt: durable_steps::Uuid,
) -> TestResult<(String, Option<String>, i64, i64)> {
    Ok(fixture.store.transaction(move |scope|Box::pin(async move {
        let row=query::<sqlx_turso::Turso>("SELECT state_kind,outcome,created_at,updated_at FROM lease_claim_reservations WHERE attempt_id=?").bind(attempt.to_string()).fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
        Ok((row.try_get("state_kind").map_err(StoreError::backend)?,row.try_get("outcome").map_err(StoreError::backend)?,row.try_get("created_at").map_err(StoreError::backend)?,row.try_get("updated_at").map_err(StoreError::backend)?))
    })).await?)
}
async fn read_receipt(fixture: &Fixture) -> TestResult<ReceiptObservation> {
    Ok(fixture.store.transaction(|scope|Box::pin(async move {
        let row=query::<sqlx_turso::Turso>("SELECT receipt_id,result_kind,stored_epoch,stored_incarnation,stored_change,created_at,updated_at FROM lease_claim_settlement_receipts").fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
        Ok((row.try_get("receipt_id").map_err(StoreError::backend)?,row.try_get("result_kind").map_err(StoreError::backend)?,row.try_get("stored_epoch").map_err(StoreError::backend)?,row.try_get("stored_incarnation").map_err(StoreError::backend)?,row.try_get("stored_change").map_err(StoreError::backend)?,row.try_get("created_at").map_err(StoreError::backend)?,row.try_get("updated_at").map_err(StoreError::backend)?))
    })).await?)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Literal typed state and timestamps must survive settlement and replay without JSON coercion."
)]
#[tokio::test]
async fn reservations_preserve_creation_time_on_settlement_and_late_reserve_is_read_only()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    fixture.arrange_registered().await?;
    let attempt = durable_steps::Uuid::now_v7();
    assert_eq!(
        call_claim_gate(&fixture, attempt, false).await?,
        json!({"kind":"reserved"})
    );
    assert_eq!(
        read_reservation(&fixture, attempt).await?,
        ("active".into(), None, NOW, NOW)
    );
    fixture.io.advance(10_000);
    call_claim_gate(&fixture, attempt, true).await?;
    let settled = ("settled".into(), Some("refused".into()), NOW, NOW + 10_000);
    assert_eq!(read_reservation(&fixture, attempt).await?, settled);
    fixture.io.advance(10_000);
    assert!(
        matches!(serde_json::from_value::<fleet_protocol::GateAnswer>(call_claim_gate(&fixture,attempt,false).await?)?, fleet_protocol::GateAnswer::Refused {reason} if reason.code.as_str()=="claim_already_settled")
    );
    assert_eq!(read_reservation(&fixture, attempt).await?, settled);
    fixture.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Receipts bind their call and typed result, preserve exact times on replay, and reject incoherent columns."
)]
#[tokio::test]
async fn receipt_replay_keeps_timestamps_and_refuses_an_extraneous_stored_version() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    fixture.arrange_registered().await?;
    let projection: Value = serde_json::from_str(
        fixture
            .lease_and_notices()
            .await?
            .1
            .as_deref()
            .ok_or("hook view absent")?,
    )?;
    let attempt = durable_steps::Uuid::now_v7();
    let operation = durable_steps::Uuid::now_v7();
    assert_eq!(
        settle(&fixture, operation, attempt, projection.clone()).await?,
        json!({"kind":"duplicate"})
    );
    let saved = read_receipt(&fixture).await?;
    assert!(runtime_primitives::uuid_v7(durable_steps::Uuid::parse_str(
        &saved.0
    )?));
    assert_eq!(
        (
            saved.1.as_str(),
            &saved.2,
            &saved.3,
            &saved.4,
            saved.5,
            saved.6
        ),
        ("duplicate", &None, &None, &None, NOW, NOW)
    );
    fixture.io.advance(10_000);
    assert_eq!(
        settle(&fixture, operation, attempt, projection.clone()).await?,
        json!({"kind":"duplicate"})
    );
    assert_eq!(read_receipt(&fixture).await?, saved);
    fixture
        .sql("UPDATE lease_claim_settlement_receipts SET stored_epoch='1'")
        .await?;
    assert!(
        settle(&fixture, operation, attempt, projection.clone())
            .await
            .is_err()
    );
    fixture.sql("UPDATE lease_claim_settlement_receipts SET stored_epoch=NULL,result_kind='unrecognized'").await?;
    assert!(
        settle(&fixture, operation, attempt, projection)
            .await
            .is_err()
    );
    fixture.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The SQL schema must permit a malformed variant so the real Rust gate demonstrates rejection without normalization."
)]
#[tokio::test]
async fn an_active_reservation_with_a_settlement_outcome_is_rejected() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    fixture.arrange_registered().await?;
    let attempt = durable_steps::Uuid::now_v7();
    call_claim_gate(&fixture, attempt, false).await?;
    fixture
        .sql("UPDATE lease_claim_reservations SET outcome='granted'")
        .await?;
    assert!(call_claim_gate(&fixture, attempt, false).await.is_err());
    assert_eq!(
        read_reservation(&fixture, attempt).await?,
        ("active".into(), Some("granted".into()), NOW, NOW)
    );
    fixture.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "An older claim must store the actual committed lease version in typed columns and reject a partial version."
)]
#[tokio::test]
async fn older_receipt_stores_the_renewed_hook_version_and_refuses_partial_version_columns()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    fixture.arrange_registered().await?;
    let older: Value = serde_json::from_str(
        fixture
            .lease_and_notices()
            .await?
            .1
            .as_deref()
            .ok_or("hook view absent")?,
    )?;
    fixture.io.advance(30_000);
    let answer=fixture.call("session","central.session-lease",Value::Null,json!({
        "session":{"peer":A,"sessionId":durable_steps::Uuid::now_v7()},
        "request":{"kind":"renew","value":{"attempt":durable_steps::Uuid::now_v7(),"database":PROJECT,"epoch":1}}
    })).await?;
    assert!(matches!(
        serde_json::from_value::<fleet_protocol::LeaseAnswer>(answer)?,
        fleet_protocol::LeaseAnswer::Renew(fleet_protocol::RenewAnswer::Renewed {
            replayed: false,
            ..
        })
    ));
    let attempt = durable_steps::Uuid::now_v7();
    let operation = durable_steps::Uuid::now_v7();
    assert_eq!(
        settle(&fixture, operation, attempt, older.clone()).await?,
        json!({"kind":"older","value":{"stored":{"epoch":1,"incarnation":2,"change":2}}})
    );
    let saved = read_receipt(&fixture).await?;
    assert_eq!(
        (saved.1, saved.2, saved.3, saved.4, saved.5, saved.6),
        (
            "older".into(),
            Some("1".into()),
            Some("2".into()),
            Some("2".into()),
            NOW + 30_000,
            NOW + 30_000
        )
    );
    fixture
        .sql("UPDATE lease_claim_settlement_receipts SET stored_change=NULL")
        .await?;
    assert!(settle(&fixture, operation, attempt, older).await.is_err());
    fixture.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "An actual receipt INSERT failure must roll back reservation state and timestamps while retaining the hook's view."
)]
#[tokio::test]
async fn receipt_failure_rolls_back_settlement_and_retry_uses_the_same_call() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    fixture.arrange_registered().await?;
    let before = fixture.lease_and_notices().await?;
    let projection: Value = serde_json::from_str(before.1.as_deref().ok_or("hook view absent")?)?;
    let attempt = durable_steps::Uuid::now_v7();
    let operation = durable_steps::Uuid::now_v7();
    call_claim_gate(&fixture, attempt, false).await?;
    fixture.io.advance(10_000);
    fixture.sql("CREATE TRIGGER reject_typed_claim_receipt BEFORE INSERT ON lease_claim_settlement_receipts BEGIN SELECT RAISE(ABORT,'injected typed receipt failure'); END").await?;
    assert!(
        settle(&fixture, operation, attempt, projection.clone())
            .await
            .is_err()
    );
    assert_eq!(
        read_reservation(&fixture, attempt).await?,
        ("active".into(), None, NOW, NOW)
    );
    assert_eq!(receipts(&fixture).await?, 0);
    assert_eq!(fixture.lease_and_notices().await?, before);
    fixture
        .sql("DROP TRIGGER reject_typed_claim_receipt")
        .await?;
    assert_eq!(
        settle(&fixture, operation, attempt, projection).await?,
        json!({"kind":"duplicate"})
    );
    assert_eq!(
        read_reservation(&fixture, attempt).await?,
        ("settled".into(), Some("granted".into()), NOW, NOW + 10_000)
    );
    assert_eq!(receipts(&fixture).await?, 1);
    assert_eq!(fixture.lease_and_notices().await?, before);
    fixture.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Untrusted provenance and an invalid content digest must leave the real hook view, reservation and receipt unchanged."
)]
#[tokio::test]
async fn untrusted_source_and_invalid_digest_create_no_typed_claim_rows() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    fixture.arrange_registered().await?;
    let before = fixture.lease_and_notices().await?;
    let projection: Value = serde_json::from_str(before.1.as_deref().ok_or("hook view absent")?)?;
    let valid = json!({"source":{"kind":"fleet"},"sourceFace":"registry","authority":{"kind":"object"},"destination":{"kind":"fleet"},"destinationFace":"fleet",
        "identity":{"operation":durable_steps::Uuid::now_v7(),"step":"claim_columns"},"kind":"central.claim-granted","target":null,"request":{"attempt":durable_steps::Uuid::now_v7(),"projection":projection}});
    let mut untrusted = valid.clone();
    untrusted["source"] = json!({"kind":"copy","value":PROJECT});
    untrusted["sourceFace"] = json!("copy");
    let mut malformed = valid;
    malformed["request"]["projection"]["digest"] = json!("0".repeat(64));
    for call in [untrusted, malformed] {
        assert!(
            fixture
                .runtime
                .on_call(serde_json::to_vec(&call)?)
                .await
                .is_err()
        );
    }
    assert_eq!(receipts(&fixture).await?, 0);
    assert_eq!(fixture.lease_and_notices().await?, before);
    let reservations = fixture
        .store
        .transaction(|scope| {
            Box::pin(async move {
                let row = query::<sqlx_turso::Turso>(
                    "SELECT count(*) AS count FROM lease_claim_reservations",
                )
                .fetch_one(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                row.try_get::<i64, _>("count").map_err(StoreError::backend)
            })
        })
        .await?;
    assert_eq!(reservations, 0);
    fixture.close().await
}
