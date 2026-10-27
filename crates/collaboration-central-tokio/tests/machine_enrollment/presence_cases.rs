//! Presence records verified admission, independently of a lease renewal.
use super::*;
use durable_steps::CallTarget;
use fleet_sync_protocol::*;
use serde_json::json;
use std::sync::atomic::{AtomicI64, Ordering};

#[derive(Clone)]
struct PresenceIo(Arc<AtomicI64>);
impl Clock for PresenceIo {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.0.load(Ordering::SeqCst))
    }
}
impl IdSource for PresenceIo {
    fn new_id(&self) -> durable_steps::Uuid {
        durable_steps::Uuid::now_v7()
    }
}
impl WakeScheduler for PresenceIo {
    async fn wake_at(&self, _: SyncTime) -> Result<(), WakeFailure> {
        Ok(())
    }
}
impl PlatformIo for PresenceIo {
    async fn object_call(&self, _: ObjectAddress, _: Vec<u8>) -> Result<Vec<u8>, CallFailure> {
        Err(CallFailure::NotSent {
            reason: NotSentReason::Unreachable { last_seen: None },
        })
    }
}
fn config() -> TestResult<RegistryConfig> {
    Ok(RegistryConfig {
        lease: LeasePolicy {
            lease_duration: Duration::from_secs(60),
            renew_interval: Duration::from_secs(20),
        },
        authoritative_receiver: ReceiverId::try_from(
            "019f0000-0000-7000-8000-000000000801".to_owned(),
        )?,
        release_unsent_for: Duration::from_secs(10),
        move_needs_renewal_within: None,
    })
}
type PresenceRuntime = ObjectRuntime<RegistryKind, TokioStorageHost<CoreHost<Io>>, PresenceIo>;
fn runtime(fixture: &Fixture, clock: &PresenceIo) -> TestResult<PresenceRuntime> {
    Ok(ObjectRuntime::registry(
        Arc::new(TokioStorageHost::new(fixture.object.clone())),
        clock.clone(),
        config()?,
        collaboration_central_tokio::DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
    )?)
}
async fn observed(fixture: &Fixture) -> TestResult<Option<String>> {
    Ok(fixture
        .object
        .transaction(|scope| {
            Box::pin(async move {
                query::<sqlx_turso::Turso>("SELECT last_seen FROM machine_enrollments")
                    .fetch_one(scope.executor())
                    .await
                    .map_err(StoreError::backend)?
                    .try_get("last_seen")
                    .map_err(StoreError::backend)
            })
        })
        .await?)
}
async fn invoke(
    runtime: &PresenceRuntime,
    service: &ServiceId,
    kind: &str,
    request: serde_json::Value,
) -> TestResult<Vec<u8>> {
    Ok(runtime.on_call(serde_json::to_vec(&json!({
        "source":{"kind":"connection","value":service},"sourceFace":"session","authority":{"kind":"object"},
        "destination":{"kind":"registry"},"destinationFace":"registry",
        "identity":{"operation":durable_steps::Uuid::now_v7(),"step":"presence_case"},
        "kind":kind,"target":null,"request":request
    }))?).await?)
}
async fn report(
    runtime: &PresenceRuntime,
    service: &ServiceId,
    report: &ReportSchemaSupport,
) -> TestResult {
    let bytes = invoke(
        runtime,
        service,
        fleet_sync_session::SchemaSupportRegistry::KIND,
        serde_json::to_value(report)?,
    )
    .await?;
    let _: SchemaSupportRecorded = serde_json::from_slice(&bytes)?;
    Ok(())
}
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn only_a_new_verified_admission_sets_presence_and_a_renewal_does_not() -> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let request = request(
        "550e8400-e29b-41d4-a716-446655440502",
        "presence-client",
        "presence-hash",
    )?;
    peer(
        fixture
            .runtime
            .enroll_machine(&owner()?, request.clone())
            .await?,
        &request,
    )?;
    assert_eq!(observed(&fixture).await?, None);
    // Authentication alone is not a completed Hello barrier.
    assert_eq!(
        fixture
            .authenticate(&request)
            .await?
            .map_err(|error| format!("{error:?}"))?
            .peer(),
        request.peer
    );
    assert_eq!(observed(&fixture).await?, None);
    let first_time = 1_792_281_601_000;
    let clock = PresenceIo(Arc::new(AtomicI64::new(first_time)));
    let registry = runtime(&fixture, &clock)?;
    let first = ReportSchemaSupport {
        peer: request.peer,
        session_id: SessionId::new(durable_steps::Uuid::now_v7())?,
        attach_seq: AttachSequence::new(1)?,
        support: SchemaSupport { max_version: 1 },
    };
    report(&registry, &request.installation, &first).await?;
    assert_eq!(
        observed(&fixture).await?,
        Some("2026-10-18T00:00:01+00:00".to_owned())
    );
    let next_time = first_time + 1_000;
    clock.0.store(next_time, Ordering::SeqCst);
    let next = ReportSchemaSupport {
        attach_seq: AttachSequence::new(2)?,
        session_id: SessionId::new(durable_steps::Uuid::now_v7())?,
        ..first.clone()
    };
    report(&registry, &request.installation, &next).await?;
    assert_eq!(
        observed(&fixture).await?,
        Some("2026-10-18T00:00:02+00:00".to_owned())
    );
    clock.0.store(first_time + 2_000, Ordering::SeqCst);
    report(&registry, &request.installation, &first).await?;
    report(&registry, &request.installation, &next).await?;
    assert_eq!(
        observed(&fixture).await?,
        Some("2026-10-18T00:00:02+00:00".to_owned()),
        "late or repeated schema report fabricated fresh presence"
    );
    drop(registry);
    fixture.objects.close().await?;
    drop(fixture);

    let database = DatabaseId::new(durable_steps::Uuid::now_v7())?;
    // Genuine library grant/Copy; the gate used to arrange it is a setup stand-in.
    registry_setup::register_grant_history(
        root.path(),
        config()?,
        database,
        vec![(Epoch::new(1)?, request.peer)],
    )
    .await?;
    let fixture = Fixture::open(root.path()).await?;
    clock.0.store(first_time + 10_000, Ordering::SeqCst);
    let registry = runtime(&fixture, &clock)?;
    let bytes=invoke(&registry,&request.installation,"central.session-lease",json!({
        "session":fleet_sync_session::SessionKey {peer:request.peer,session_id:next.session_id},
        "request":LeaseRequest::Renew(RenewRequest {attempt:RequestId::new(durable_steps::Uuid::now_v7())?,database,epoch:Epoch::new(1)?})
    })).await?;
    assert!(
        matches!(
            serde_json::from_slice::<LeaseAnswer>(&bytes)?,
            LeaseAnswer::Renew(RenewAnswer::Renewed {
                replayed: false,
                ..
            })
        ),
        "the presence control must reach a genuine successful renewal"
    );
    assert_eq!(
        observed(&fixture).await?,
        Some("2026-10-18T00:00:02+00:00".to_owned())
    );
    fixture.objects.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn stored_presence_rejects_invalid_time_and_precision_loss_but_accepts_utc_notation()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let request = request(
        "550e8400-e29b-41d4-a716-446655440503",
        "presence-codec",
        "presence-codec-hash",
    )?;
    peer(
        fixture
            .runtime
            .enroll_machine(&owner()?, request.clone())
            .await?,
        &request,
    )?;
    for encoded in ["not-a-time", "2026-10-18T00:00:01.000000001+00:00"] {
        inject_presence(&fixture, encoded).await?;
        let refused = fixture
            .runtime
            .authenticate_machine(
                request.access_client_id.clone(),
                request.credential_hash.clone(),
            )
            .await;
        assert!(
            matches!(&refused, Err(StoreError::Corrupt {field,..}) if field=="machine_enrollments.last_seen"),
            "{refused:?}"
        );
    }
    inject_presence(&fixture, "2026-10-18T00:00:01Z").await?;
    assert_eq!(
        fixture
            .authenticate(&request)
            .await?
            .map_err(|error| format!("{error:?}"))?
            .peer(),
        request.peer
    );
    fixture.objects.close().await?;
    Ok(())
}

async fn inject_presence(fixture: &Fixture, value: &str) -> TestResult {
    let value = value.to_owned();
    fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                query::<sqlx_turso::Turso>("UPDATE machine_enrollments SET last_seen = ?")
                    .bind(value)
                    .execute(scope.executor())
                    .await
                    .map_err(StoreError::backend)?;
                Ok(())
            })
        })
        .await?;
    Ok(())
}
