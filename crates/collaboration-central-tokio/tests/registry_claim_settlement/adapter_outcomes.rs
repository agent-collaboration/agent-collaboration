//! These cases exercise the adapter's answers, beyond receiver-only ordering.
//! The grant arranger's Fleet/release stand-ins retain their existing limits.
use super::*;
use durable_steps::{
    OperationId, StepName,
    storage::{CallReach, JournalStorage, StepState},
};
use fleet_sync_protocol::{
    LeaseAnswer, LeaseRequest, PeerId, RenewAnswer, RenewRequest, RequestId, SessionId,
};

async fn arrange() -> TestResult<(RegistryFixture, PeerId, DatabaseId)> {
    let fixture = RegistryFixture::open(false).await?;
    let request: EnrollMachine = serde_json::from_value(json!({
        "installation":SERVICE,"peer":"019f0000-0000-7000-8000-000000001f11",
        "label":"adapter outcome writer","access_client_id":"adapter-outcome-writer",
        "credential_hash":"a".repeat(64),"expires_at":"2026-10-20T00:00:00Z"
    }))?;
    let owner = OwnerIdentity::from_access(AccessCaller::Owner {
        email: "fixture@example.test".into(),
    })?;
    let EnrollmentDecision::MachineEnrolled { peer, .. } =
        fixture.runtime.enroll_machine(&owner, request).await?
    else {
        return Err("adapter fixture enrollment did not activate".into());
    };
    let database = DatabaseId::new(durable_steps::Uuid::parse_str(DATABASE)?)?;
    registry_setup::register_grant_history(
        fixture.root.path(),
        fixture.config,
        database,
        vec![(Epoch::new(1)?, peer)],
    )
    .await?;
    // The actual ordinary CoreHost publisher primes Fleet, without a claim receipt.
    fixture.runtime.on_alarm().await?;
    Ok((fixture, peer, database))
}

async fn admit(
    fixture: &RegistryFixture,
    peer: PeerId,
    database: DatabaseId,
    granted_epoch: u64,
) -> TestResult<OperationId> {
    let settlement = ClaimSettlement {
        change: HolderChangeRequest::Claim {
            attempt: TransferId::new(durable_steps::Uuid::parse_str(ATTEMPT)?)?,
            database,
            claimant: peer,
        },
        outcome: SettleOutcome::Granted {
            epoch: Epoch::new(granted_epoch)?,
        },
    };
    Ok(fixture
        .registry
        .transaction(move |scope| {
            Box::pin(async move {
                scope
                    .admit::<SettleReservationTask<RegistryStore>>(
                        &RequestKey::new("adapter-outcome-settlement")
                            .map_err(StoreError::backend)?,
                        &settlement,
                    )
                    .await
                    .map(|record| record.id())
                    .map_err(StoreError::backend)
            })
        })
        .await?)
}

async fn projection(fixture: &RegistryFixture) -> TestResult<(Value, i64, i64)> {
    Ok(fixture.fleet.object.transaction(|scope|Box::pin(async move {
        let row = query::<sqlx_turso::Turso>("SELECT lease_projection,(SELECT count(*) FROM claim_reservations) AS claims,(SELECT count(*) FROM claim_settlement_receipts) AS receipts FROM project_directory WHERE project_id=?")
            .bind(DATABASE).fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
        let projection: String = row.try_get("lease_projection").map_err(StoreError::backend)?;
        Ok((serde_json::from_str(&projection).map_err(StoreError::backend)?,row.try_get("claims").map_err(StoreError::backend)?,row.try_get("receipts").map_err(StoreError::backend)?))
    })).await?)
}

async fn receipt(fixture: &RegistryFixture, operation: OperationId) -> TestResult<Value> {
    Ok(fixture
        .fleet
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                let row = query::<sqlx_turso::Turso>(
                    "SELECT step_name,result FROM claim_settlement_receipts WHERE operation_id=?",
                )
                .bind(operation.as_uuid().to_string())
                .fetch_one(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                let step: String = row.try_get("step_name").map_err(StoreError::backend)?;
                if step != "settle" {
                    return Err(StoreError::backend(
                        "claim receipt changed the durable step",
                    ));
                }
                let result: String = row.try_get("result").map_err(StoreError::backend)?;
                serde_json::from_str(&result).map_err(StoreError::backend)
            })
        })
        .await?)
}

async fn close(fixture: &RegistryFixture) -> TestResult {
    fixture.objects.close().await?;
    fixture.fleet.objects.close().await?;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn duplicate_projection_is_recorded_by_a_new_claim_call_and_the_adapter_completes()
-> TestResult {
    let (fixture, peer, database) = arrange().await?;
    let proof = async {
        // Arrange: ordinary publication has the identical genuine snapshot, but no claim receipt.
        let before = projection(&fixture).await?;
        assert_eq!((before.1, before.2), (0, 0));
        let operation = admit(&fixture, peer, database, 1).await?;

        // Act: the real CoreHost adapts the durable settlement into real Fleet storage.
        let drive = fixture.runtime.on_alarm().await?;

        // Assert: this is a fresh Duplicate receipt, not a replay of an Applied receipt.
        assert_eq!(drive.completed, vec![operation]);
        assert_eq!(
            receipt(&fixture, operation).await?,
            json!({"kind":"duplicate"})
        );
        let after = projection(&fixture).await?;
        assert_eq!(after.0, before.0);
        assert_eq!((after.1, after.2), (1, 1));
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    close(&fixture).await?;
    proof
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_newer_real_publication_overtakes_the_claim_call_and_older_still_completes() -> TestResult
{
    let (fixture, peer, database) = arrange().await?;
    let proof = Box::pin(async {
        // Arrange: stop only transport, after CoreHost has captured the old public snapshot.
        let operation = admit(&fixture,peer,database,1).await?;
        fixture.io.pause_grant.store(true,Ordering::SeqCst);
        let claim = fixture.runtime.on_alarm();
        tokio::pin!(claim);
        tokio::select! {
            answer = &mut claim => return Err(format!("claim did not pause: {answer:?}").into()),
            entered = tokio::time::timeout(std::time::Duration::from_secs(5),fixture.io.grant_entered.notified()) => entered?,
        }
        fixture.io.now.fetch_add(10_000,Ordering::SeqCst);
        let renewal = LeaseRequest::Renew(RenewRequest {attempt:RequestId::new(durable_steps::Uuid::now_v7())?,database,epoch:Epoch::new(1)?});
        let call = serde_json::to_vec(&json!({
            "source":{"kind":"connection","value":SERVICE},"sourceFace":"session","authority":{"kind":"object"},
            "destination":{"kind":"registry"},"destinationFace":"registry",
            "identity":{"operation":durable_steps::Uuid::now_v7(),"step":"lease_dispatch"},
            "kind":"central.session-lease","target":null,"request":{
                "session":fleet_sync_session::SessionKey {peer,session_id:SessionId::new(durable_steps::Uuid::now_v7())?},"request":renewal
            }
        }))?;
        let renewed = fixture.runtime.on_call(call).await.map_err(|error|format!("real renewal: {error:?}"))?;
        assert!(matches!(serde_json::from_slice::<LeaseAnswer>(&renewed)?,LeaseAnswer::Renew(RenewAnswer::Renewed {replayed:false,..})));
        tokio::time::timeout(std::time::Duration::from_secs(5),fixture.runtime.on_alarm()).await??;
        let newer = projection(&fixture).await?;
        assert_eq!(newer.0.pointer("/snapshot/revision/change"),Some(&json!(2)));
        assert_eq!((newer.1,newer.2),(0,0));

        // Act: the older captured call now reaches Fleet, with the original CallId.
        fixture.io.grant_release.notify_one();
        let completed = claim.await?;

        // Assert: the adapter accepts Older, and the newer projection survives exactly.
        assert_eq!(completed.completed,vec![operation]);
        assert_eq!(receipt(&fixture,operation).await?,json!({"kind":"older","value":{"stored":{"epoch":1,"incarnation":1,"change":2}}}));
        let after = projection(&fixture).await?;
        assert_eq!(after.0,newer.0);
        assert_eq!((after.1,after.2),(1,1));
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }).await;
    fixture.io.grant_release.notify_one();
    close(&fixture).await?;
    proof
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_snapshot_behind_the_grant_keeps_the_same_call_pending_without_settling_fleet()
-> TestResult {
    let (fixture, peer, database) = arrange().await?;
    let proof = async {
        // Granted epoch 2 is a typed settlement-debt precondition; the genuine
        // Registry is still at epoch 1. This is not normal Claim admission proof.
        let before = projection(&fixture).await?;
        let operation = admit(&fixture,peer,database,2).await?;
        let mut pending = None;
        for _ in 0..2 {
            // Act: a real Engine drive reaches CoreHost's behind-snapshot guard.
            let drive = fixture.runtime.on_alarm().await?;
            let mut claim_retries = Vec::new();
            for id in &drive.retried {
                if *id == operation {
                    claim_retries.push(*id);
                } else {
                    // The genuine grant also owes its holder notice. This fixture
                    // deliberately has no Delivery route; that independent debt
                    // may retry beside the claim, never as another claim call.
                    let id = *id;
                    let status = fixture.registry.transaction(move |scope| Box::pin(async move {
                        scope.operation(id).await.map(|record| record.map(|record| record.status()))
                    })).await?.ok_or("retried notice operation is absent")?;
                    assert_eq!(status.kind,"central.registry-notice");
                }
            }
            assert_eq!(claim_retries,vec![operation]);
            let step = fixture.registry.transaction(move |scope|Box::pin(async move {
                scope.step(operation,0).await?.ok_or_else(||StoreError::backend("settlement step missing"))
            })).await?;
            assert_eq!(step.operation,operation);
            assert_eq!(step.name,StepName::new("settle")?);
            assert!(matches!(&step.state,StepState::CallPending {attempt,..} if attempt.reach==CallReach::MayHaveReached && attempt.retry_reason.is_none()));
            if let Some(previous) = &pending {assert_eq!(&step,previous);}
            pending = Some(step);
            assert_eq!(projection(&fixture).await?,before);
            assert!(!fixture.io.calls.lock().map_err(|_|"call observation poisoned")?.iter().any(|call|call.get("kind").and_then(Value::as_str)==Some("central.claim-granted")));
            fixture.io.now.fetch_add(60_000,Ordering::SeqCst);
        }
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    close(&fixture).await?;
    proof
}
