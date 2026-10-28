#![cfg(feature = "conformance")]

use collaboration_central_core::{CoreHost, ObjectAddress, ObjectRuntime, PlatformIo};
use collaboration_central_tokio::TokioStorageHost;
use durable_steps::{
    AtomicStore, CallFailure, JournalScope, RequestKey, StoreError,
    storage::{JournalStorage, OperationExecution},
};
use fleet_sync_lease::{LeasePolicy, RegistryConfig, SettleReservationTask};
use fleet_sync_protocol::{
    ClaimSettlement, DatabaseId, HolderChangeRequest, PeerId, ReceiverId, SettleOutcome,
    SettlementRecorded, TransferId,
};
use fleet_sync_tokio_runner::{ObjectKey, ObjectOptions, ObjectStore, TokioObject};
use runtime_primitives::{Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

type TestResult = Result<(), Box<dyn std::error::Error + Send + Sync>>;
type CapturedCall = (ObjectAddress, Vec<u8>);

#[derive(Clone, Default)]
struct RecordingIo {
    calls: Arc<Mutex<Vec<CapturedCall>>>,
    wakes: Arc<Mutex<Vec<SyncTime>>>,
}
impl Clock for RecordingIo {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(1_792_281_600_000)
    }
}
impl IdSource for RecordingIo {
    fn new_id(&self) -> durable_steps::Uuid {
        durable_steps::Uuid::now_v7()
    }
}
impl WakeScheduler for RecordingIo {
    async fn wake_at(&self, at: SyncTime) -> Result<(), WakeFailure> {
        self.wakes
            .lock()
            .map_err(|_| WakeFailure::Unavailable {
                message: "wake observation lock poisoned".into(),
            })?
            .push(at);
        Ok(())
    }
}
impl PlatformIo for RecordingIo {
    async fn object_call(
        &self,
        destination: ObjectAddress,
        body: Vec<u8>,
    ) -> Result<Vec<u8>, CallFailure> {
        self.calls
            .lock()
            .map_err(|_| CallFailure::AnswerLost)?
            .push((destination, body));
        serde_json::to_vec(&SettlementRecorded).map_err(|_| CallFailure::AnswerLost)
    }
}

fn config() -> Result<RegistryConfig, Box<dyn std::error::Error + Send + Sync>> {
    Ok(RegistryConfig {
        lease: LeasePolicy {
            lease_duration: Duration::from_secs(60),
            renew_interval: Duration::from_secs(20),
        },
        authoritative_receiver: ReceiverId::try_from(
            "019f0000-0000-7000-8000-000000000010".to_owned(),
        )?,
        release_unsent_for: Duration::from_secs(10),
        move_needs_renewal_within: None,
    })
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn restart_drives_a_committed_registry_task_and_keeps_its_result() -> TestResult {
    let directory = tempfile::tempdir()?;
    let before_restart = RecordingIo::default();
    let configuration = config()?;
    let objects = ObjectStore::open(directory.path()).await?;
    let object = objects
        .object(
            ObjectKey::new("registry")?,
            ObjectOptions::default(),
            Arc::new(CoreHost::conformance_registry(
                before_restart.clone(),
                configuration.authoritative_receiver,
            )),
        )
        .await?;
    let settlement = ClaimSettlement {
        change: HolderChangeRequest::Claim {
            attempt: TransferId::try_from("019f0000-0000-7000-8000-000000000011".to_owned())?,
            database: DatabaseId::try_from("019f0000-0000-7000-8000-000000000012".to_owned())?,
            claimant: PeerId::try_from("019f0000-0000-7000-8000-000000000013".to_owned())?,
        },
        outcome: SettleOutcome::Cancelled,
    };
    let expected_request = serde_json::to_value(&settlement)?;
    let operation = object
        .transaction(move |scope| {
            Box::pin(async move {
                scope
                    .admit::<SettleReservationTask<TokioObject<CoreHost<RecordingIo>>>>(
                        &RequestKey::new("settlement-before-restart")
                            .map_err(StoreError::backend)?,
                        &settlement,
                    )
                    .await
                    .map(|admitted| admitted.id())
                    .map_err(StoreError::backend)
            })
        })
        .await?;
    assert!(
        before_restart
            .calls
            .lock()
            .map_err(|_| "call lock poisoned")?
            .is_empty()
    );
    objects.close().await?;
    drop(object);
    drop(objects);

    let after_restart = RecordingIo::default();
    let reopened = ObjectStore::open(directory.path()).await?;
    let object = reopened
        .object(
            ObjectKey::new("registry")?,
            ObjectOptions::default(),
            Arc::new(CoreHost::conformance_registry(
                after_restart.clone(),
                configuration.authoritative_receiver,
            )),
        )
        .await?;
    let runtime = ObjectRuntime::registry(
        Arc::new(TokioStorageHost::new(object.clone())),
        after_restart.clone(),
        configuration,
        collaboration_central_tokio::DeliveryConfig {
            relay_peer: fleet_sync_protocol::PeerId::try_from(
                "019f0000-0000-7000-8000-000000005240".to_owned(),
            )?,
        },
    )?;

    runtime.initialize().await?;
    let report = runtime.on_alarm().await?;

    assert_eq!(report.completed, vec![operation]);
    assert!(report.failed.is_empty());
    assert!(report.retried.is_empty());
    assert!(report.yielded.is_empty());
    assert!(report.wake_failure.is_none());
    {
        let calls = after_restart
            .calls
            .lock()
            .map_err(|_| "call lock poisoned")?;
        let [(destination, bytes)] = calls.as_slice() else {
            return Err("expected one settlement call after restart".into());
        };
        assert_eq!(*destination, ObjectAddress::Fleet);
        let call: serde_json::Value = serde_json::from_slice(bytes)?;
        assert_eq!(call["sourceFace"], "registry");
        assert_eq!(call["destinationFace"], "fleet");
        assert_eq!(call["request"], expected_request);
        assert_eq!(
            call["identity"]["operation"],
            operation.as_uuid().to_string()
        );
        assert_eq!(call["identity"]["step"], "settle");
    }
    assert!(
        !after_restart
            .wakes
            .lock()
            .map_err(|_| "wake lock poisoned")?
            .is_empty()
    );
    assert_eq!(
        runtime.on_alarm().await?,
        durable_steps::DriveReport::default()
    );
    assert_eq!(
        after_restart
            .calls
            .lock()
            .map_err(|_| "call lock poisoned")?
            .len(),
        1
    );
    let record = object
        .transaction(move |scope| Box::pin(async move { scope.operation(operation).await }))
        .await?
        .ok_or("completed operation missing")?;
    assert!(
        matches!(record.execution, OperationExecution::Completed { result, .. }
        if result == serde_json::to_vec(&SettlementRecorded)?)
    );
    reopened.close().await?;
    Ok(())
}
