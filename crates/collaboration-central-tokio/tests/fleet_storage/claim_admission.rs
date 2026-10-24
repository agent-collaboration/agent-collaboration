//! Native gate calls meet durable tombstones without the library's task replay.
use super::{
    claim_settlement::{acquire_incarnation, encoded_projection, registry_call},
    support::{Fixture, TestResult, bundle},
};
use collaboration_central_core::{ClaimReservationOutcome, ClaimReservationState};
use durable_steps::{AtomicStore, CallTarget, StoreError, storage::TransactionScope};
use fleet_sync_protocol::{
    DatabaseId, GateAnswer, HolderChangeRequest, PeerId, SettleOutcome, TransferId,
};
use serde_json::{Value, json};
use sqlx_core::{query::query, row::Row};

const PROJECT: &str = "019f0000-0000-7000-8000-000000000a83";
const ATTEMPT: &str = "019f0000-0000-7000-8000-000000000b01";

pub(super) fn change() -> TestResult<HolderChangeRequest> {
    Ok(HolderChangeRequest::Claim {
        attempt: TransferId::new(durable_steps::Uuid::parse_str(ATTEMPT)?)?,
        database: DatabaseId::new(durable_steps::Uuid::parse_str(PROJECT)?)?,
        claimant: PeerId::new(durable_steps::Uuid::parse_str(
            "019f0000-0000-7000-8000-000000000b03",
        )?)?,
    })
}

pub(super) async fn reserve(
    fixture: &Fixture,
    operation: durable_steps::Uuid,
) -> TestResult<GateAnswer> {
    let bytes = fixture
        .runtime
        .on_call(registry_call(
            fleet_sync_lease::ClaimReservation::KIND,
            serde_json::to_value(fleet_sync_protocol::ClaimReservation { change: change()? })?,
            operation,
        )?)
        .await
        .map_err(|error| format!("native reserve did not answer: {error:?}"))?;
    Ok(serde_json::from_slice(&bytes)?)
}

async fn cancel(fixture: &Fixture) -> TestResult {
    let bytes = fixture
        .runtime
        .on_call(registry_call(
            fleet_sync_lease::ClaimSettlement::KIND,
            serde_json::to_value(fleet_sync_protocol::ClaimSettlement {
                change: change()?,
                outcome: SettleOutcome::Cancelled,
            })?,
            durable_steps::Uuid::now_v7(),
        )?)
        .await
        .map_err(|error| format!("native cancel did not answer: {error:?}"))?;
    let _: fleet_sync_protocol::SettlementRecorded = serde_json::from_slice(&bytes)?;
    Ok(())
}

async fn state(fixture: &Fixture) -> TestResult<ClaimReservationState> {
    let payload = fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                let row = query::<sqlx_turso::Turso>(
                    "SELECT state FROM claim_reservations WHERE attempt_id=? AND project_id=?",
                )
                .bind(ATTEMPT)
                .bind(PROJECT)
                .fetch_one(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                row.try_get::<String, _>("state")
                    .map_err(StoreError::backend)
            })
        })
        .await?;
    Ok(serde_json::from_str(&payload)?)
}

fn assert_settled(answer: GateAnswer, outcome: ClaimReservationOutcome) -> TestResult {
    let GateAnswer::Refused { reason } = answer else {
        return Err(format!("a settled attempt was reactivated: {answer:?}").into());
    };
    assert_eq!(reason.code.as_str(), "claim_already_settled");
    assert!(reason.detail.contains(match outcome {
        ClaimReservationOutcome::Granted => "granted",
        ClaimReservationOutcome::Refused => "refused",
    }));
    Ok(())
}

#[tokio::test]
async fn cancel_before_reserve_stays_refused_after_reopen() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof = async {
        fixture.import(&bundle()?).await?;
        cancel(&fixture).await?;
        cancel(&fixture).await?;
        assert_settled(
            reserve(&fixture, durable_steps::Uuid::now_v7()).await?,
            ClaimReservationOutcome::Refused,
        )?;
        assert_eq!(
            state(&fixture).await?,
            ClaimReservationState::Settled {
                outcome: ClaimReservationOutcome::Refused
            }
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    fixture.objects.close().await?;
    proof?;
    drop(fixture);
    let reopened = Fixture::open(root.path()).await?;
    let proof = async {
        assert_settled(
            reserve(&reopened, durable_steps::Uuid::now_v7()).await?,
            ClaimReservationOutcome::Refused,
        )?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    reopened.objects.close().await?;
    proof
}

#[tokio::test]
async fn granted_attempt_refuses_the_original_reserve_call_even_after_reopen() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let reserve_call = durable_steps::Uuid::now_v7();
    let proof = async {
        fixture.import(&bundle()?).await?;
        assert_eq!(reserve(&fixture, reserve_call).await?, GateAnswer::Reserved);
        assert_eq!(reserve(&fixture, reserve_call).await?, GateAnswer::Reserved);
        let incarnation = acquire_incarnation(&fixture).await?;
        let answer = fixture
            .runtime
            .on_call(registry_call(
                "central.claim-granted",
                json!({"attempt":ATTEMPT,"projection":encoded_projection(incarnation)?}),
                durable_steps::Uuid::now_v7(),
            )?)
            .await
            .map_err(|error| format!("grant did not answer: {error:?}"))?;
        assert_eq!(
            serde_json::from_slice::<Value>(&answer)?,
            json!({"kind":"applied"})
        );
        assert_settled(
            reserve(&fixture, reserve_call).await?,
            ClaimReservationOutcome::Granted,
        )?;
        assert_eq!(
            state(&fixture).await?,
            ClaimReservationState::Settled {
                outcome: ClaimReservationOutcome::Granted
            }
        );
        assert!(
            cancel(&fixture).await.is_err(),
            "a conflicting outcome must retain the grant"
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    fixture.objects.close().await?;
    proof?;
    drop(fixture);
    let reopened = Fixture::open(root.path()).await?;
    let proof = async {
        assert_settled(
            reserve(&reopened, reserve_call).await?,
            ClaimReservationOutcome::Granted,
        )?;
        assert_settled(
            reserve(&reopened, durable_steps::Uuid::now_v7()).await?,
            ClaimReservationOutcome::Granted,
        )?;
        assert_eq!(
            state(&reopened).await?,
            ClaimReservationState::Settled {
                outcome: ClaimReservationOutcome::Granted
            }
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    reopened.objects.close().await?;
    proof
}

#[tokio::test]
async fn cancel_closes_an_active_reservation_and_its_old_call_cannot_reactivate_it() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let reserve_call = durable_steps::Uuid::now_v7();
    let proof = async {
        fixture.import(&bundle()?).await?;
        assert_eq!(reserve(&fixture, reserve_call).await?, GateAnswer::Reserved);
        cancel(&fixture).await?;
        assert_settled(
            reserve(&fixture, reserve_call).await?,
            ClaimReservationOutcome::Refused,
        )?;
        assert_eq!(
            state(&fixture).await?,
            ClaimReservationState::Settled {
                outcome: ClaimReservationOutcome::Refused
            }
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    fixture.objects.close().await?;
    proof?;
    drop(fixture);
    let reopened = Fixture::open(root.path()).await?;
    let proof = async {
        assert_settled(
            reserve(&reopened, reserve_call).await?,
            ClaimReservationOutcome::Refused,
        )?;
        assert_eq!(
            state(&reopened).await?,
            ClaimReservationState::Settled {
                outcome: ClaimReservationOutcome::Refused
            }
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    reopened.objects.close().await?;
    proof
}
