//! A genuine public Registry snapshot crosses CoreHost into the real Fleet store.
//! The reused grant-history arranger keeps its declared registration-gate stand-in.
//! Settlement debt is a typed component input, not a normal Claim admission proof.
#![cfg(feature = "conformance")]

#[path = "registry_claim_settlement/adapter_outcomes.rs"]
mod adapter_outcomes;
#[path = "mail_submit_http/registry_setup.rs"]
mod registry_setup;
#[allow(dead_code)]
#[path = "registry_lease/support.rs"]
mod support;
use collaboration_central_core::{
    AccessCaller, EnrollMachine, EnrollmentDecision, ObjectRuntime, OwnerIdentity,
};
use collaboration_central_tokio::TokioStorageHost;
use durable_steps::{AtomicStore, JournalScope, RequestKey, StoreError, storage::TransactionScope};
use fleet_sync_lease::SettleReservationTask;
use fleet_sync_protocol::{
    ClaimSettlement, DatabaseId, Epoch, HolderChangeRequest, SettleOutcome, TransferId,
};
use serde_json::{Value, json};
use sqlx_core::{query::query, row::Row};
use std::{sync::Arc, sync::atomic::Ordering};
use support::{FleetFixture, RegistryFixture, RegistryStore, TestResult};
const DATABASE: &str = "019f0000-0000-7000-8000-000000000a83";
const SERVICE: &str = "550e8400-e29b-41d4-a716-4466554400a9";
const SUCCESSOR: &str = "550e8400-e29b-41d4-a716-4466554400b0";
const ATTEMPT: &str = "019f0000-0000-7000-8000-000000000b01";

#[tokio::test]
#[allow(clippy::panic_in_result_fn)] // Assertions prove the contract; Result propagates setup errors.
async fn granted_settlement_reads_the_public_snapshot_and_replays_its_original_call_after_answer_loss()
-> TestResult {
    // Arrange: real enrollment and a genuine library grant, with the existing setup gate.
    let RegistryFixture {
        root,
        fleet,
        io,
        config,
        objects,
        registry,
        runtime,
    } = RegistryFixture::open(true).await?;
    let proof = async {
        let owner = OwnerIdentity::from_access(AccessCaller::Owner {
            email: "fixture@example.test".into(),
        })?;
        let request: EnrollMachine = serde_json::from_value(json!({
            "installation":SERVICE,"peer":"019f0000-0000-7000-8000-000000001f11","label":"snapshot writer","access_client_id":"snapshot-writer",
            "credential_hash":"a".repeat(64),"expires_at":"2026-10-20T00:00:00Z"
        }))?;
        let EnrollmentDecision::MachineEnrolled { peer, .. } =
            runtime.enroll_machine(&owner, request).await?
        else {
            return Err("fixture enrollment did not activate".into());
        };
        let successor: EnrollMachine = serde_json::from_value(json!({
            "installation":SUCCESSOR,"peer":"019f0000-0000-7000-8000-000000001f12","label":"successor writer","access_client_id":"snapshot-successor",
            "credential_hash":"b".repeat(64),"expires_at":"2026-10-20T00:00:00Z"
        }))?;
        let EnrollmentDecision::MachineEnrolled { peer: successor, .. } =
            runtime.enroll_machine(&owner, successor).await?
        else {
            return Err("successor enrollment did not activate".into());
        };
        let database = DatabaseId::new(durable_steps::Uuid::parse_str(DATABASE)?)?;
        registry_setup::register_grant_history(
            root.path(),
            config,
            database,
            vec![(Epoch::new(1)?, peer), (Epoch::new(2)?, successor)],
        )
        .await?;
        let snapshot = registry
            .transaction(move |scope| {
                Box::pin(async move {
                    fleet_sync_lease::registry_snapshot(scope, database)
                        .await
                        .map_err(StoreError::backend)
                })
            })
            .await?
            .ok_or("genuine public Registry snapshot is absent")?;
        assert_eq!(snapshot.revision.epoch, Epoch::new(2)?);
        assert_eq!(snapshot.revision.change.get(), 1);
        assert_eq!(snapshot.changed_at.unix_millis(), 1_792_281_600_000);
        let settlement = ClaimSettlement {
            change: HolderChangeRequest::Claim {
                attempt: TransferId::new(durable_steps::Uuid::parse_str(ATTEMPT)?)?,
                database,
                claimant: peer,
            },
            outcome: SettleOutcome::Granted {
                epoch: Epoch::new(1)?,
            },
        };
        let restarted_settlement = settlement.clone();
        let operation = registry
            .transaction(move |scope| {
                Box::pin(async move {
                    scope
                        .admit::<SettleReservationTask<RegistryStore>>(
                            &RequestKey::new("public-snapshot-settlement")
                                .map_err(StoreError::backend)?,
                            &settlement,
                        )
                        .await
                        .map(|record| record.id())
                        .map_err(StoreError::backend)
                })
            })
            .await?;

        // Act: the original durable call commits at Fleet, then loses its answer.
        let first_drive = runtime.on_alarm().await?;
        let first_call = io
            .calls
            .lock()
            .map_err(|_| "call observation poisoned")?
            .iter()
            .find(|call| call.get("kind").and_then(Value::as_str) == Some("central.claim-granted"))
            .cloned()
            .ok_or("Granted did not adapt to central's snapshot-carrying call")?;
        assert_eq!(
            first_call.pointer("/identity/operation"),
            Some(&json!(operation.as_uuid().to_string()))
        );
        assert_eq!(first_call.pointer("/identity/step"), Some(&json!("settle")));
        assert_eq!(
            first_call.pointer("/request/attempt"),
            Some(&json!(ATTEMPT))
        );
        assert_eq!(
            first_call.pointer("/request/projection/snapshot"),
            Some(&serde_json::to_value(&snapshot)?)
        );
        assert_eq!(
            first_call.pointer("/request/projection/holder"),
            Some(&json!(SUCCESSOR))
        );
        assert_eq!(first_drive.retried, vec![operation]);
        let committed = stored_receipt(&fleet).await?;
        assert_eq!(committed.0, 1);
        assert_eq!(committed.1, operation.as_uuid().to_string());
        assert_eq!(committed.2, "settle");
        assert_eq!(
            serde_json::from_str::<Value>(&committed.3)?,
            json!({"kind":"applied"})
        );
        let persisted = registry.transaction(|scope|Box::pin(async move {
            let row=query::<sqlx_turso::Turso>("SELECT incarnation FROM registry_lease_incarnation WHERE name='registry'")
                .fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
            row.try_get::<String,_>("incarnation").map_err(StoreError::backend)
        })).await?;
        assert_eq!(persisted,"1");

        io.now.fetch_add(60_000, Ordering::SeqCst);
        let retried = runtime.on_alarm().await?;

        // Assert: engine retry preserves the call and the one durable Fleet receipt.
        assert_eq!(retried.completed, vec![operation]);
        assert_eq!(stored_receipt(&fleet).await?, committed);
        let grants = io
            .calls
            .lock()
            .map_err(|_| "call observation poisoned")?
            .iter()
            .filter(|call| {
                call.get("kind").and_then(Value::as_str) == Some("central.claim-granted")
            })
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(grants, vec![first_call.clone(), first_call]);

        // A fresh runtime must acquire above the last persisted run, even at one revision.
        drop(runtime);
        let restarted = ObjectRuntime::registry(
            Arc::new(TokioStorageHost::new(registry.clone())),io.clone(),config,
collaboration_central_tokio::DeliveryConfig { relay_peer: fleet_sync_protocol::PeerId::try_from("019f0000-0000-7000-8000-000000005240".to_owned())? },
)?;
        assert_eq!(restarted.initialize().await?,0);
        let resumed = registry.transaction(move |scope|Box::pin(async move {
            scope.admit::<SettleReservationTask<RegistryStore>>(
                &RequestKey::new("public-snapshot-next-run").map_err(StoreError::backend)?,
                &restarted_settlement,
            ).await.map(|record|record.id()).map_err(StoreError::backend)
        })).await?;
        assert_eq!(restarted.on_alarm().await?.completed,vec![resumed]);
        let calls=io.calls.lock().map_err(|_|"call observation poisoned")?;
        let floors=calls.iter().filter(|call|call.get("kind").and_then(Value::as_str)==Some("central.lease-incarnation"))
            .map(|call|call.pointer("/request/atLeast").cloned().ok_or("allocation floor missing"))
            .collect::<Result<Vec<_>,_>>()?;
        assert_eq!(floors,vec![json!(0),json!(1)]);
        let next=calls.iter().rev().find(|call|call.get("kind").and_then(Value::as_str)==Some("central.claim-granted"))
            .ok_or("new run did not settle")?;
        assert_eq!(next.pointer("/request/projection/incarnation"),Some(&json!(2)));
        assert_eq!(next.pointer("/request/projection/snapshot"),Some(&serde_json::to_value(snapshot)?));
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    objects.close().await?;
    fleet.objects.close().await?;
    proof
}

async fn stored_receipt(fleet: &FleetFixture) -> TestResult<(i64, String, String, String)> {
    Ok(fleet.object.transaction(|scope|Box::pin(async move {
        let row=query::<sqlx_turso::Turso>("SELECT (SELECT count(*) FROM claim_settlement_receipts) AS receipts,operation_id,step_name,result FROM claim_settlement_receipts WHERE attempt_id=?")
            .bind(ATTEMPT).fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
        Ok((row.try_get("receipts").map_err(StoreError::backend)?,row.try_get("operation_id").map_err(StoreError::backend)?,row.try_get("step_name").map_err(StoreError::backend)?,row.try_get("result").map_err(StoreError::backend)?))
    })).await?)
}
