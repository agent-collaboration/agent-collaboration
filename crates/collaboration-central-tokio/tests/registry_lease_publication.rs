//! Ordinary snapshot discovery must converge without claim debt or idle alarms.
#![cfg(feature = "conformance")]
#[path = "registry_lease_publication/notice_atomicity.rs"]
mod notice_atomicity;

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "session_publication/support.rs"]
mod notice_receiver;
#[path = "registry_lease_publication/owner_retirement_recovery.rs"]
mod owner_retirement_recovery;
#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "mail_submit_http/registry_setup.rs"]
mod registry_setup;
#[path = "registry_lease_publication/retiring_publication.rs"]
mod retiring_publication;

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "registry_lease/support.rs"]
mod support;

use collaboration_central_core::{
    AccessCaller, CoreHost, EnrollMachine, EnrollmentDecision, ObjectRuntime, OwnerIdentity,
};
use collaboration_central_tokio::TokioStorageHost;
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use fleet_sync_protocol::{
    DatabaseId, Epoch, LeaseAnswer, LeaseRequest, RenewAnswer, RenewRequest, RequestId, SessionId,
};
use fleet_sync_tokio_runner::{ObjectKey, ObjectStore};
use runtime_primitives::SyncTime;
use serde_json::{Value, json};
use sqlx_core::{query::query, row::Row};
use std::sync::{Arc, atomic::Ordering};
use support::{RegistryFixture, TestResult};

// Multiple callers may idempotently arm the same pending deadline. Keep the
// exact requested times; raw empty-after-clear checks below still prove idle.
fn distinct_wake_deadlines(io: &support::RoutingIo) -> TestResult<Vec<SyncTime>> {
    let mut deadlines = io
        .wakes
        .lock()
        .map_err(|_| "wake observation poisoned")?
        .clone();
    deadlines.sort_unstable();
    deadlines.dedup();
    Ok(deadlines)
}

const DATABASE: &str = "019f0000-0000-7000-8000-000000000a83";
const SERVICE: &str = "550e8400-e29b-41d4-a716-4466554400a9";

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)] // Result carries setup errors; assertions preserve the obligations.
async fn an_unacknowledged_unheld_snapshot_repairs_after_store_reopen_then_leaves_no_idle_alarm()
-> TestResult {
    let RegistryFixture {
        root,
        fleet,
        io,
        config,
        objects,
        registry,
        runtime,
    } = RegistryFixture::open(false).await?;
    let database = DatabaseId::new(durable_steps::Uuid::parse_str(DATABASE)?)?;
    registry_setup::import_unheld(root.path(), config, database).await?;
    let expected = registry
        .transaction(move |scope| {
            Box::pin(async move {
                fleet_sync_lease::registry_snapshot(scope, database)
                    .await
                    .map_err(StoreError::backend)
            })
        })
        .await?
        .ok_or("public Unheld snapshot absent")?;
    assert!(matches!(
        expected.lifecycle,
        fleet_sync_lease::DatabaseLifecycle::Unheld { .. }
    ));
    assert!(expected.grant.is_none());
    // No alarm or bookkeeping runs between the library commit and the owning store close.
    drop(runtime);
    objects.close().await?;
    drop(registry);
    drop(objects);
    io.lose_publication_answer.store(true, Ordering::SeqCst);

    let reopened = ObjectStore::open(root.path()).await?;
    let registry = reopened
        .object(
            ObjectKey::new("registry")?,
            Default::default(),
            Arc::new(CoreHost::for_store(
                io.clone(),
                config.authoritative_receiver,
            )),
        )
        .await?;
    let runtime = ObjectRuntime::registry(
        Arc::new(TokioStorageHost::new(registry.clone())),
        io.clone(),
        config,
        collaboration_central_tokio::DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
    )?;
    let proof = async {
        assert_eq!(runtime.initialize().await?, 0);
        assert!(
            io.calls
                .lock()
                .map_err(|_| "call observation poisoned")?
                .is_empty(),
            "object readiness must not await a Fleet RPC"
        );
        assert_eq!(
            distinct_wake_deadlines(&io)?,
            vec![SyncTime::from_unix_millis(1_792_281_600_000)]
        );
        runtime.on_alarm().await?;
        let first = io
            .calls
            .lock()
            .map_err(|_| "call observation poisoned")?
            .iter()
            .find(|call| {
                call.get("kind").and_then(Value::as_str) == Some("central.lease-projection")
            })
            .cloned()
            .ok_or("startup missed the committed Unheld snapshot")?;
        assert_eq!(
            first.pointer("/request/snapshot"),
            Some(&serde_json::to_value(&expected)?)
        );
        assert_eq!(first.pointer("/request/holder"), Some(&Value::Null));
        let acknowledged = registry
            .transaction(|scope| {
                Box::pin(async move {
                    let row = query::<sqlx_turso::Turso>(
                        "SELECT count(*) AS acknowledgements FROM registry_lease_acknowledgements",
                    )
                    .fetch_one(scope.executor())
                    .await
                    .map_err(StoreError::backend)?;
                    row.try_get::<i64, _>("acknowledgements")
                        .map_err(StoreError::backend)
                })
            })
            .await?;
        assert_eq!(
            acknowledged, 0,
            "a lost answer must not be acknowledged locally"
        );
        // Cloudflare calls initialize again on warm entries; that must not bypass retry cadence.
        assert_eq!(runtime.initialize().await?, 0);
        assert_eq!(
            distinct_wake_deadlines(&io)?,
            vec![
                SyncTime::from_unix_millis(1_792_281_600_000),
                SyncTime::from_unix_millis(1_792_281_605_000)
            ]
        );

        io.now.fetch_add(5_000, Ordering::SeqCst);
        runtime.on_alarm().await?;

        let publication_count = io
            .calls
            .lock()
            .map_err(|_| "call observation poisoned")?
            .iter()
            .filter(|call| {
                call.get("kind").and_then(Value::as_str) == Some("central.lease-projection")
            })
            .count();
        assert_eq!(publication_count, 2);
        let projection = fleet
            .object
            .transaction(|scope| {
                Box::pin(async move {
                    let row = query::<sqlx_turso::Turso>(
                        "SELECT lease_projection FROM project_directory WHERE project_id=?",
                    )
                    .bind(DATABASE)
                    .fetch_one(scope.executor())
                    .await
                    .map_err(StoreError::backend)?;
                    row.try_get::<String, _>("lease_projection")
                        .map_err(StoreError::backend)
                })
            })
            .await?;
        let projection: Value = serde_json::from_str(&projection)?;
        assert_eq!(
            projection.get("snapshot"),
            Some(&serde_json::to_value(expected)?)
        );
        assert_eq!(projection.get("holder"), Some(&Value::Null));
        io.wakes
            .lock()
            .map_err(|_| "wake observation poisoned")?
            .clear();
        io.now.fetch_add(120_000, Ordering::SeqCst);
        runtime.on_alarm().await?;
        assert_eq!(
            io.calls
                .lock()
                .map_err(|_| "call observation poisoned")?
                .iter()
                .filter(|call| call.get("kind").and_then(Value::as_str)
                    == Some("central.lease-projection"))
                .count(),
            2
        );
        assert!(
            io.wakes
                .lock()
                .map_err(|_| "wake observation poisoned")?
                .is_empty(),
            "settled repair must not arm another idle alarm"
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    reopened.close().await?;
    fleet.objects.close().await?;
    proof
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)] // Assertions are the proof; Result propagates fixture failures.
async fn alarm_discovers_a_public_snapshot_and_an_acknowledged_idle_registry_sets_no_alarm()
-> TestResult {
    let delivery_root = tempfile::tempdir()?;
    let delivery = Arc::new(
        notice_receiver::Fixture::open_for_installation(
            delivery_root.path(),
            SERVICE.to_owned().try_into()?,
        )
        .await?,
    );
    let RegistryFixture {
        root,
        fleet,
        io,
        config,
        objects,
        registry,
        runtime,
    } = RegistryFixture::open(false).await?;
    let receiving = delivery.clone();
    *io.notice_delivery
        .lock()
        .map_err(|_| "notice route poisoned")? = Some(Arc::new(move |address, bytes| {
        let receiving = receiving.clone();
        Box::pin(async move {
            if address
                != collaboration_central_core::ObjectAddress::Connection(
                    SERVICE
                        .to_owned()
                        .try_into()
                        .map_err(|_| durable_steps::CallFailure::AnswerLost)?,
                )
            {
                return Err(durable_steps::CallFailure::AnswerLost);
            }
            receiving
                .runtime
                .delivery()
                .map_err(|_| durable_steps::CallFailure::AnswerLost)?
                .on_call(bytes)
                .await
        })
    }));
    let proof=async {
        assert!(io.wakes.lock().map_err(|_|"wake observation poisoned")?.is_empty());
        let owner=OwnerIdentity::from_access(AccessCaller::Owner {email:"fixture@example.test".into()})?;
        let request:EnrollMachine=serde_json::from_value(json!({
            "installation":SERVICE,"peer":"019f0000-0000-7000-8000-000000001f13","label":"publication writer","access_client_id":"publication-writer",
            "credential_hash":"c".repeat(64),"expires_at":"2026-10-20T00:00:00Z"
        }))?;
        let EnrollmentDecision::MachineEnrolled {peer,..}=runtime.enroll_machine(&owner,request).await? else {
            return Err("publication fixture enrollment did not activate".into());
        };
        let database=DatabaseId::new(durable_steps::Uuid::parse_str(DATABASE)?)?;
        registry_setup::register_grant_history(root.path(),config,database,vec![(Epoch::new(1)?,peer)]).await?;
        let expected=registry.transaction(move |scope|Box::pin(async move {
            fleet_sync_lease::registry_snapshot(scope,database).await.map_err(StoreError::backend)
        })).await?.ok_or("public snapshot absent")?;

        // No central dirty bookkeeping was performed when the separate library task committed.
        runtime.on_alarm().await?;

        let publications=io.calls.lock().map_err(|_|"call observation poisoned")?.iter()
            .filter(|call|call.get("kind").and_then(Value::as_str)==Some("central.lease-projection"))
            .cloned().collect::<Vec<_>>();
        assert_eq!(publications.len(),1,"Registry wake did not discover its committed public snapshot");
        let publication=publications.first().ok_or("lease projection absent")?;
        assert_eq!(publication.pointer("/request/snapshot"),Some(&serde_json::to_value(&expected)?));
        assert_eq!(publication.pointer("/request/holder"),Some(&json!(SERVICE)));
        let (projection,claims,receipts)=fleet.object.transaction(|scope|Box::pin(async move {
            let row=query::<sqlx_turso::Turso>("SELECT lease_projection,(SELECT count(*) FROM claim_reservations) AS claims,(SELECT count(*) FROM claim_settlement_receipts) AS receipts FROM project_directory WHERE project_id=?")
                .bind(DATABASE).fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
            Ok((row.try_get::<String,_>("lease_projection").map_err(StoreError::backend)?,row.try_get::<i64,_>("claims").map_err(StoreError::backend)?,row.try_get::<i64,_>("receipts").map_err(StoreError::backend)?))
        })).await?;
        assert_eq!(serde_json::from_str::<Value>(&projection)?,publication.get("request").cloned().ok_or("request absent")?);
        assert_eq!((claims,receipts),(0,0));
        assert_eq!(distinct_wake_deadlines(&io)?,vec![SyncTime::from_unix_millis(1_792_281_600_000)],"the new notice debt must wake Registry after its admission commits");
        // This fixture now routes the actual Delivery receiver. Dispatch, append
        // on its owning alarm, then retry the same call to finish Registry debt.
        runtime.on_alarm().await?;
        delivery.runtime.on_alarm().await?;
        io.now.fetch_add(5_000,Ordering::SeqCst);
        runtime.on_alarm().await?;
        let completed = registry.transaction(|scope|Box::pin(async move {
            let row=query::<sqlx_turso::Turso>("SELECT COUNT(*) AS count FROM _fleet_sync_operations WHERE kind='central.registry-notice' AND state='completed'")
                .fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
            row.try_get::<i64,_>("count").map_err(StoreError::backend)
        })).await?;
        assert_eq!(completed,1,"idle assertion requires the actual notice append to finish");
        io.wakes.lock().map_err(|_|"wake observation poisoned")?.clear();
        runtime.on_alarm().await?;
        assert!(io.wakes.lock().map_err(|_|"wake observation poisoned")?.is_empty(),"a settled publication and notice must not keep Registry awake");

        io.now.fetch_add(120_000,Ordering::SeqCst);
        runtime.on_alarm().await?;
        let repeated=io.calls.lock().map_err(|_|"call observation poisoned")?.iter()
            .filter(|call|call.get("kind").and_then(Value::as_str)==Some("central.lease-projection")).count();
        assert_eq!(repeated,1,"read time must not change a committed snapshot's content or version");
        assert!(io.wakes.lock().map_err(|_|"wake observation poisoned")?.is_empty());

        // The Session-owned invocation is supplied; Registry renewal and Fleet publication are real.
        let renewal=LeaseRequest::Renew(RenewRequest {
            attempt:RequestId::new(durable_steps::Uuid::now_v7())?,database,epoch:Epoch::new(1)?,
        });
        let call=serde_json::to_vec(&json!({
            "source":{"kind":"connection","value":SERVICE},"sourceFace":"session","authority":{"kind":"object"},
            "destination":{"kind":"registry"},"destinationFace":"registry",
            "identity":{"operation":durable_steps::Uuid::now_v7(),"step":"lease_dispatch"},
            "kind":"central.session-lease","target":null,"request":{
                "session":fleet_sync_session::SessionKey {peer,session_id:SessionId::new(durable_steps::Uuid::now_v7())?},
                "request":renewal
            }
        }))?;
        io.lose_publication_answer.store(true,Ordering::SeqCst);
        let reply=runtime.on_call(call.clone()).await.map_err(|error|format!("Registry renewal: {error:?}"))?;
        assert!(matches!(serde_json::from_slice::<LeaseAnswer>(&reply)?,LeaseAnswer::Renew(RenewAnswer::Renewed {replayed:false,..})),"metadata answer loss changed a committed renewal reply");
        let after_renewal=io.calls.lock().map_err(|_|"call observation poisoned")?.iter()
            .filter(|call|call.get("kind").and_then(Value::as_str)==Some("central.lease-projection")).count();
        assert_eq!(after_renewal,1,"the committed lease answer must precede publication");
        let replay=runtime.on_call(call).await.map_err(|error|format!("Registry renewal replay: {error:?}"))?;
        assert!(matches!(serde_json::from_slice::<LeaseAnswer>(&replay)?,LeaseAnswer::Renew(RenewAnswer::Renewed {replayed:true,..})));
        assert_eq!(io.calls.lock().map_err(|_|"call observation poisoned")?.iter()
            .filter(|call|call.get("kind").and_then(Value::as_str)==Some("central.lease-projection")).count(),1);
        assert_eq!(distinct_wake_deadlines(&io)?,vec![SyncTime::from_unix_millis(1_792_281_725_000)]);
        runtime.on_alarm().await?;
        assert_eq!(io.calls.lock().map_err(|_|"call observation poisoned")?.iter()
            .filter(|call|call.get("kind").and_then(Value::as_str)==Some("central.lease-projection")).count(),2);
        assert_eq!(distinct_wake_deadlines(&io)?,vec![SyncTime::from_unix_millis(1_792_281_725_000),SyncTime::from_unix_millis(1_792_281_730_000)]);
        io.now.fetch_add(5_000,Ordering::SeqCst);
        runtime.on_alarm().await?;
        assert_eq!(io.calls.lock().map_err(|_|"call observation poisoned")?.iter()
            .filter(|call|call.get("kind").and_then(Value::as_str)==Some("central.lease-projection")).count(),3);
        assert_eq!(distinct_wake_deadlines(&io)?.len(),2);
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }.await;
    objects.close().await?;
    fleet.objects.close().await?;
    delivery.objects.close().await?;
    proof
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)] // Result carries setup errors; assertions decide the availability contract.
async fn renewal_and_replay_answer_while_the_separate_publication_rpc_is_blocked() -> TestResult {
    let fixture = RegistryFixture::open(false).await?;
    let proof=Box::pin(async {
        let owner=OwnerIdentity::from_access(AccessCaller::Owner {email:"fixture@example.test".into()})?;
        let request:EnrollMachine=serde_json::from_value(json!({
            "installation":SERVICE,"peer":"019f0000-0000-7000-8000-000000001f13","label":"publication writer","access_client_id":"publication-writer",
            "credential_hash":"c".repeat(64),"expires_at":"2026-10-20T00:00:00Z"
        }))?;
        let EnrollmentDecision::MachineEnrolled {peer,..}=fixture.runtime.enroll_machine(&owner,request).await? else {
            return Err("paused-publication fixture enrollment did not activate".into());
        };
        let database=DatabaseId::new(durable_steps::Uuid::parse_str(DATABASE)?)?;
        registry_setup::register_grant_history(fixture.root.path(),fixture.config,database,vec![(Epoch::new(1)?,peer)]).await?;
        fixture.runtime.on_alarm().await?;
        fixture.io.now.fetch_add(10_000,Ordering::SeqCst);
        fixture.io.pause_publication.store(true,Ordering::SeqCst);
        let request=LeaseRequest::Renew(RenewRequest {attempt:RequestId::new(durable_steps::Uuid::now_v7())?,database,epoch:Epoch::new(1)?});
        let call=serde_json::to_vec(&json!({
            "source":{"kind":"connection","value":SERVICE},"sourceFace":"session","authority":{"kind":"object"},
            "destination":{"kind":"registry"},"destinationFace":"registry",
            "identity":{"operation":durable_steps::Uuid::now_v7(),"step":"lease_dispatch"},
            "kind":"central.session-lease","target":null,"request":{
                "session":fleet_sync_session::SessionKey {peer,session_id:SessionId::new(durable_steps::Uuid::now_v7())?},
                "request":request
            }
        }))?;
        let reply=tokio::select! {
            result=fixture.runtime.on_call(call.clone())=>result.map_err(|error|format!("paused publication renewal: {error:?}"))?,
            _=fixture.io.publication_entered.notified()=>return Err("committed renewal waited on the Fleet publication RPC".into()),
        };
        assert!(matches!(serde_json::from_slice::<LeaseAnswer>(&reply)?,LeaseAnswer::Renew(RenewAnswer::Renewed {replayed:false,..})));
        let alarm=fixture.runtime.on_alarm();
        tokio::pin!(alarm);
        tokio::select! {
            result=&mut alarm=>return Err(format!("publication did not remain paused: {result:?}").into()),
            _=fixture.io.publication_entered.notified()=>{},
        }
        let replay=tokio::time::timeout(std::time::Duration::from_secs(5),fixture.runtime.on_call(call)).await?
            .map_err(|error|format!("replay while publication awaits: {error:?}"))?;
        assert!(matches!(serde_json::from_slice::<LeaseAnswer>(&replay)?,LeaseAnswer::Renew(RenewAnswer::Renewed {replayed:true,..})));
        fixture.io.publication_release.notify_one();
        alarm.await?;
        Ok::<_,Box<dyn std::error::Error+Send+Sync>>(())
    }).await;
    fixture.objects.close().await?;
    fixture.fleet.objects.close().await?;
    proof
}
