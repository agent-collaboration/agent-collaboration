//! Registry settlements retain their receipt beside the committed lease projection.
use super::support::{Fixture, TestResult, bundle};
use collaboration_central_core::{
    ClaimReservationOutcome, ClaimReservationState, RegistryIncarnation,
};
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use fleet_sync_lease::{DatabaseLifecycle, RegistryGrant, RegistryRevision, RegistrySnapshot};
use fleet_sync_protocol::{DatabaseId, Epoch, Origin, PeerId};
use message_board::ServiceId;
use runtime_primitives::SyncTime;
use serde_json::{Value, json};
use sqlx_core::{executor::Executor, query::query, row::Row};
use std::num::NonZeroU64;

const PROJECT: &str = "019f0000-0000-7000-8000-000000000a83";
const ATTEMPT: &str = "019f0000-0000-7000-8000-000000000b01";

pub(super) fn encoded_projection(incarnation: RegistryIncarnation) -> TestResult<Value> {
    encoded_revision(
        incarnation,
        2,
        3,
        "550e8400-e29b-41d4-a716-4466554400a9",
        "019f0000-0000-7000-8000-000000000b03",
        1_792_411_200_000,
    )
}

pub(super) fn encoded_revision(
    incarnation: RegistryIncarnation,
    epoch: u64,
    change: u64,
    service: &str,
    peer: &str,
    changed_at_ms: i64,
) -> TestResult<Value> {
    let snapshot = RegistrySnapshot {
        database: DatabaseId::new(durable_steps::Uuid::parse_str(PROJECT)?)?,
        revision: RegistryRevision {
            epoch: Epoch::new(epoch)?,
            change: NonZeroU64::new(change).ok_or("positive fixture revision")?,
        },
        changed_at: SyncTime::from_unix_millis(changed_at_ms),
        lifecycle: DatabaseLifecycle::Registered {
            origin: Origin::Empty,
            imported_from: None,
        },
        grant: Some(RegistryGrant {
            epoch: Epoch::new(epoch)?,
            holder: PeerId::new(durable_steps::Uuid::parse_str(peer)?)?,
            expires_at: SyncTime::from_unix_millis(
                changed_at_ms
                    .checked_add(14_400_000)
                    .ok_or("fixture deadline overflow")?,
            ),
        }),
    };
    let holder = Some(ServiceId::try_from(service.to_owned())?);
    // The producer uses ring for the declared snapshot/content digest; the receiver uses sha2.
    let bytes = serde_json::to_vec(&(&snapshot, &holder))?;
    let digest = ring::digest::digest(&ring::digest::SHA256, &bytes)
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    Ok(json!({"snapshot":snapshot,"incarnation":incarnation,"holder":holder,"digest":digest}))
}

pub(super) fn registry_call(
    kind: &str,
    request: Value,
    operation: durable_steps::Uuid,
) -> TestResult<Vec<u8>> {
    Ok(serde_json::to_vec(&json!({
        "source":{"kind":"registry"},"sourceFace":"registry","authority":{"kind":"object"},
        "destination":{"kind":"fleet"},"destinationFace":"fleet",
        "identity":{"operation":operation,"step":"claim_settlement"},
        "kind":kind,"target":null,"request":request
    }))?)
}

async fn stored_settlement(fixture: &Fixture) -> TestResult<(String, String, String, String, i64)> {
    Ok(fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                let row = query::<sqlx_turso::Turso>(
                    "SELECT d.lease_projection,r.state,c.receipt_id,c.result,(SELECT count(*) FROM claim_settlement_receipts WHERE attempt_id=r.attempt_id) AS receipt_count FROM project_directory d JOIN claim_reservations r ON r.project_id=d.project_id JOIN claim_settlement_receipts c ON c.attempt_id=r.attempt_id WHERE d.project_id=? AND r.attempt_id=?",
                )
                .bind(PROJECT)
                .bind(ATTEMPT)
                .fetch_one(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                Ok((
                    row.try_get("lease_projection").map_err(StoreError::backend)?,
                    row.try_get("state").map_err(StoreError::backend)?,
                    row.try_get("receipt_id").map_err(StoreError::backend)?,
                    row.try_get("result").map_err(StoreError::backend)?,
                    row.try_get("receipt_count").map_err(StoreError::backend)?,
                ))
            })
        })
        .await?)
}

pub(super) async fn acquire_incarnation(fixture: &Fixture) -> TestResult<RegistryIncarnation> {
    let answer = fixture
        .runtime
        .on_call(registry_call(
            "central.lease-incarnation",
            json!({"atLeast":0}),
            durable_steps::Uuid::now_v7(),
        )?)
        .await
        .map_err(|error| format!("Registry incarnation: {error:?}"))?;
    Ok(serde_json::from_slice(&answer)?)
}

async fn supply_active_reservation(fixture: &Fixture) -> TestResult {
    fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                query::<sqlx_turso::Turso>(
                    "INSERT INTO claim_reservations(attempt_id,project_id,state) VALUES(?,?,?)",
                )
                .bind(ATTEMPT)
                .bind(PROJECT)
                .bind(r#"{"kind":"active"}"#)
                .execute(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                Ok(())
            })
        })
        .await?;
    Ok(())
}

async fn reservation_state(fixture: &Fixture) -> TestResult<Value> {
    let state = fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                let row = query::<sqlx_turso::Turso>(
                    "SELECT state FROM claim_reservations WHERE attempt_id=?",
                )
                .bind(ATTEMPT)
                .fetch_one(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                row.try_get::<String, _>("state")
                    .map_err(StoreError::backend)
            })
        })
        .await?;
    Ok(serde_json::from_str(&state)?)
}

#[tokio::test]
async fn incarnation_allocation_preserves_large_integers_and_skips_a_repeated_call() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof=async {
        let call=registry_call("central.lease-incarnation",json!({"atLeast":9_007_199_254_740_993_u64}),durable_steps::Uuid::now_v7())?;
        let first=fixture.runtime.on_call(call.clone()).await.map_err(|error|format!("large incarnation: {error:?}"))?;
        let repeated=fixture.runtime.on_call(call).await.map_err(|error|format!("repeated allocation: {error:?}"))?;
        assert_eq!(serde_json::from_slice::<RegistryIncarnation>(&first)?.value(),9_007_199_254_740_994);
        assert_eq!(serde_json::from_slice::<RegistryIncarnation>(&repeated)?.value(),9_007_199_254_740_995);
        let exhausted=registry_call("central.lease-incarnation",json!({"atLeast":u64::MAX}),durable_steps::Uuid::now_v7())?;
        assert!(matches!(fixture.runtime.on_call(exhausted).await,Err(durable_steps::CallFailure::AnswerLost)));
        let stored=fixture.object.transaction(|scope|Box::pin(async move {
            let row=query::<sqlx_turso::Turso>("SELECT incarnation,(SELECT count(*) FROM lease_incarnation_fence) AS rows FROM lease_incarnation_fence WHERE name='registry'")
                .fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
            Ok((row.try_get::<String,_>("incarnation").map_err(StoreError::backend)?,row.try_get::<i64,_>("rows").map_err(StoreError::backend)?))
        })).await?;
        assert_eq!(stored,("9007199254740995".to_owned(),1));
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    fixture.objects.close().await?;
    proof
}

#[tokio::test]
async fn receipt_write_failure_rolls_back_the_projection_and_keeps_the_reservation_active()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof=async {
        fixture.import(&bundle()?).await?;
        let incarnation=acquire_incarnation(&fixture).await?;
        // The active reservation is a supplied component input; reserve admission is separate.
        supply_active_reservation(&fixture).await?;
        fixture.object.transaction(|scope|Box::pin(async move {
            scope.executor().execute("CREATE TRIGGER fail_claim_receipt BEFORE INSERT ON claim_settlement_receipts BEGIN SELECT RAISE(ABORT,'claim receipt fixture fault'); END")
                .await.map_err(StoreError::backend)?;
            Ok(())
        })).await?;
        let call=registry_call("central.claim-granted",json!({"attempt":ATTEMPT,"projection":encoded_projection(incarnation)?}),durable_steps::Uuid::now_v7())?;

        assert!(matches!(fixture.runtime.on_call(call.clone()).await,Err(durable_steps::CallFailure::AnswerLost)));
        assert_eq!(reservation_state(&fixture).await?,json!({"kind":"active"}));
        let unchanged=fixture.object.transaction(|scope|Box::pin(async move {
            let row=query::<sqlx_turso::Turso>("SELECT lease_projection,(SELECT count(*) FROM claim_settlement_receipts) AS receipts FROM project_directory WHERE project_id=?")
                .bind(PROJECT).fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
            Ok((row.try_get::<Option<String>,_>("lease_projection").map_err(StoreError::backend)?,row.try_get::<i64,_>("receipts").map_err(StoreError::backend)?))
        })).await?;
        assert_eq!(unchanged,(None,0));
        fixture.object.transaction(|scope|Box::pin(async move {
            query::<sqlx_turso::Turso>("DROP TRIGGER fail_claim_receipt").execute(scope.executor()).await.map_err(StoreError::backend)?;
            Ok(())
        })).await?;
        let answer=fixture.runtime.on_call(call).await.map_err(|error|format!("retry after receipt fault: {error:?}"))?;
        assert_eq!(serde_json::from_slice::<Value>(&answer)?,json!({"kind":"applied"}));
        assert_eq!(reservation_state(&fixture).await?,json!({"kind":"settled","value":{"outcome":"granted"}}));
        assert_eq!(stored_settlement(&fixture).await?.4,1);
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    fixture.objects.close().await?;
    proof
}

#[tokio::test]
async fn an_earlier_claim_settles_without_overwriting_a_later_published_revision() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof = async {
        fixture.import(&bundle()?).await?;
        let incarnation = acquire_incarnation(&fixture).await?;
        supply_active_reservation(&fixture).await?;
        let later = encoded_revision(
            incarnation,
            3,
            1,
            "550e8400-e29b-41d4-a716-4466554400b0",
            "019f0000-0000-7000-8000-000000000b04",
            1_792_411_201_000,
        )?;
        let answer = fixture
            .runtime
            .on_call(registry_call(
                "central.claim-granted",
                json!({"attempt":"019f0000-0000-7000-8000-000000000b02","projection":later}),
                durable_steps::Uuid::now_v7(),
            )?)
            .await
            .map_err(|error| format!("later grant: {error:?}"))?;
        assert_eq!(
            serde_json::from_slice::<Value>(&answer)?,
            json!({"kind":"applied"})
        );
        assert_eq!(reservation_state(&fixture).await?, json!({"kind":"active"}));

        let answer = fixture
            .runtime
            .on_call(registry_call(
                "central.claim-granted",
                json!({"attempt":ATTEMPT,"projection":encoded_projection(incarnation)?}),
                durable_steps::Uuid::now_v7(),
            )?)
            .await
            .map_err(|error| format!("earlier claim: {error:?}"))?;
        assert_eq!(
            serde_json::from_slice::<Value>(&answer)?,
            json!({"kind":"older","value":{"stored":{"epoch":3,"incarnation":1,"change":1}}})
        );
        let stored = stored_settlement(&fixture).await?;
        assert_eq!(serde_json::from_str::<Value>(&stored.0)?, later);
        assert_eq!(
            reservation_state(&fixture).await?,
            json!({"kind":"settled","value":{"outcome":"granted"}})
        );
        assert_eq!(stored.4, 1);
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    fixture.objects.close().await?;
    proof
}

#[tokio::test]
async fn untrusted_source_and_invalid_snapshot_digest_create_no_projection_or_receipt() -> TestResult
{
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof=async {
        fixture.import(&bundle()?).await?;
        let incarnation=acquire_incarnation(&fixture).await?;
        let valid=serde_json::from_slice::<Value>(&registry_call("central.claim-granted",json!({"attempt":ATTEMPT,"projection":encoded_projection(incarnation)?}),durable_steps::Uuid::now_v7())?)?;
        let mut foreign=valid.clone();
        foreign.as_object_mut().ok_or("call object")?.insert("source".into(),json!({"kind":"connection","value":"550e8400-e29b-41d4-a716-4466554400a9"}));
        foreign.as_object_mut().ok_or("call object")?.insert("sourceFace".into(),json!("session"));
        let mut malformed=valid;
        *malformed.pointer_mut("/request/projection/digest").ok_or("projection digest")?=json!("0".repeat(64));
        for call in [foreign,malformed] {
            assert!(matches!(fixture.runtime.on_call(serde_json::to_vec(&call)?).await,Err(durable_steps::CallFailure::NotSent { .. })));
        }
        let unchanged=fixture.object.transaction(|scope|Box::pin(async move {
            let row=query::<sqlx_turso::Turso>("SELECT lease_projection,(SELECT count(*) FROM claim_reservations) AS claims,(SELECT count(*) FROM claim_settlement_receipts) AS receipts FROM project_directory WHERE project_id=?")
                .bind(PROJECT).fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
            Ok((row.try_get::<Option<String>,_>("lease_projection").map_err(StoreError::backend)?,row.try_get::<i64,_>("claims").map_err(StoreError::backend)?,row.try_get::<i64,_>("receipts").map_err(StoreError::backend)?))
        })).await?;
        assert_eq!(unchanged,(None,0,0));
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    fixture.objects.close().await?;
    proof
}

#[tokio::test]
async fn claim_granted_commits_the_projection_settlement_and_replay_receipt() -> TestResult {
    // Arrange: imported directory data is real; the Registry-owned call is the component input.
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let proof = async {
        fixture.import(&bundle()?).await?;
        let allocated = fixture
            .runtime
            .on_call(registry_call(
                "central.lease-incarnation",
                json!({"atLeast":0}),
                durable_steps::Uuid::now_v7(),
            )?)
            .await
            .map_err(|error| format!("Registry incarnation call did not answer: {error:?}"))?;
        let incarnation: RegistryIncarnation = serde_json::from_slice(&allocated)?;
        assert_eq!(incarnation.value(), 1);
        let projection = encoded_projection(incarnation)?;
        let operation = durable_steps::Uuid::now_v7();
        let call = registry_call(
            "central.claim-granted",
            json!({"attempt":ATTEMPT,"projection":projection}),
            operation,
        )?;

        // Act: replay the same settlement CallId at the actual receiver.
        let first = fixture
            .runtime
            .on_call(call.clone())
            .await
            .map_err(|error| format!("ClaimGranted call did not answer: {error:?}"))?;
        assert_eq!(
            serde_json::from_slice::<Value>(&first)?,
            json!({"kind":"applied"})
        );
        let committed = stored_settlement(&fixture).await?;
        assert_eq!(committed.4, 1);
        let repeated = fixture
            .runtime
            .on_call(call)
            .await
            .map_err(|error| format!("ClaimGranted replay did not answer: {error:?}"))?;

        // Assert: the retry returns the first receipt, including Applied rather than Duplicate.
        assert_eq!(repeated, first);
        assert_eq!(stored_settlement(&fixture).await?, committed);
        let refreshed = encoded_revision(
            incarnation,
            3,
            1,
            "550e8400-e29b-41d4-a716-4466554400b0",
            "019f0000-0000-7000-8000-000000000b04",
            1_792_411_201_000,
        )?;
        let replay_with_new_snapshot = fixture
            .runtime
            .on_call(registry_call(
                "central.claim-granted",
                json!({"attempt":ATTEMPT,"projection":refreshed}),
                operation,
            )?)
            .await
            .map_err(|error| format!("refreshed settlement replay: {error:?}"))?;
        assert_eq!(replay_with_new_snapshot, first);
        assert_eq!(stored_settlement(&fixture).await?, committed);
        assert_eq!(serde_json::from_str::<Value>(&committed.0)?, projection);
        assert_eq!(
            projection.pointer("/holder"),
            Some(&json!("550e8400-e29b-41d4-a716-4466554400a9"))
        );
        assert_eq!(
            projection.pointer("/snapshot/grant/holder"),
            Some(&json!("019f0000-0000-7000-8000-000000000b03"))
        );
        assert_eq!(
            projection.pointer("/snapshot/changedAt"),
            Some(&json!(1_792_411_200_000_i64))
        );
        assert_eq!(
            serde_json::from_str::<ClaimReservationState>(&committed.1)?,
            ClaimReservationState::Settled {
                outcome: ClaimReservationOutcome::Granted
            }
        );
        assert_eq!(
            durable_steps::Uuid::parse_str(&committed.2)?.get_version_num(),
            7
        );
        assert_eq!(
            serde_json::from_str::<Value>(&committed.3)?,
            json!({"kind":"applied"})
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    fixture.objects.close().await?;
    proof
}
