//! One-shot failures occur at real timer-consumed store boundaries, without HTTP nudges.

use super::super::*;
use collaboration_central_core::ObjectPlatform;
use durable_steps::{
    AtomicStore, JournalScope, OperationContext, OperationFailure, RequestKey, Task,
    storage::{JournalStorage, OperationExecution},
};
use fleet_protocol::{CommitSequence, CopyStatusAnswer, DatabaseId, Epoch, ReplicationPosition};
use serde_json::{Value, json};
use std::time::Duration;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
#[path = "../../tests/native_http/support.rs"]
mod configuration;
async fn open(root: &Path) -> TestResult<Arc<NativeObjects>> {
    open_with_clock(root, Arc::new(runtime_primitives::SystemClock)).await
}
async fn open_with_clock(root: &Path, clock: Arc<dyn Clock>) -> TestResult<Arc<NativeObjects>> {
    Ok(NativeObjects::open_with_controls(
        root,
        clock,
        configuration::registry_config()?,
        crate::DeliveryConfig {
            relay_peer: "019f0000-0000-7000-8000-000000005240"
                .to_owned()
                .try_into()?,
        },
        Duration::from_secs(2),
        Duration::from_millis(500),
        vec![],
    )
    .await?)
}
struct TokioClock(tokio::time::Instant);
impl Clock for TokioClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(
            1_792_281_600_000_i64
                .saturating_add(i64::try_from(self.0.elapsed().as_millis()).unwrap_or(i64::MAX)),
        )
    }
}

async fn ready(slot: &ObjectSlot) -> TestResult {
    let mut ready = slot.alarm_owner_generation.subscribe();
    tokio::time::timeout(
        Duration::from_secs(2),
        ready.wait_for(|generation| *generation > 0),
    )
    .await??;
    Ok(())
}
fn envelope(database: DatabaseId, kind: &str, target: Value, request: Value) -> Value {
    json!({"source":{"kind":"fleet"},"sourceFace":"registry","authority":{"kind":"object"},"destination":{"kind":"copy","value":database},"destinationFace":"copy","identity":{"operation":durable_steps::Uuid::now_v7(),"step":"alarm_test"},"kind":kind,"target":target,"request":request})
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn copy_ack_wait_ends_at_its_event_deadline_and_arms_no_alarm() -> TestResult {
    let root = tempfile::tempdir()?;
    let objects = open(root.path()).await?;
    let database = DatabaseId::new(durable_steps::Uuid::now_v7())?;
    let receiver = objects.config.authoritative_receiver;
    let slot = objects.slot(ObjectAddress::Copy(database)).await?;
    ready(&slot).await?;
    let runtime = slot.runtime.get().ok_or("copy runtime missing")?.clone();
    let RuntimeFace::Copy(copy_kernel) = runtime.as_ref() else {
        return Err("wrong Copy face".into());
    };
    let copy = copy_kernel.face();
    copy.on_call(serde_json::to_vec(&envelope(database,"fleet-sync.copy-create",json!({"copy":receiver}),json!({"database":database,"epoch":1,"holder":durable_steps::Uuid::now_v7(),"origin":{"kind":"empty"}})))?).await.map_err(|failure|format!("create: {failure:?}"))?;
    let waiting = runtime.clone();
    let task = tokio::spawn(async move {
        let RuntimeFace::Copy(copy_kernel) = waiting.as_ref() else {
            return Err(CallFailure::AnswerLost);
        };
        let copy = copy_kernel.face();
        copy.await_copy_acked(ReplicationPosition::new(
            Epoch::new(1).map_err(|_| CallFailure::AnswerLost)?,
            CommitSequence::new(1),
        ))
        .await
    });
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let observed = copy
                .on_call(serde_json::to_vec(&envelope(
                    database,
                    "central.conformance-copy-wait",
                    json!({"copy":receiver,"database":database}),
                    json!({"kind":"observe"}),
                ))?)
                .await
                .map_err(|failure| format!("observe: {failure:?}"))?;
            if serde_json::from_slice::<Value>(&observed)?.get("waiting") == Some(&json!(1)) {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    // No call, alarm or mutation follows. Only the wait's own event deadline runs (H2).
    let outcome = tokio::time::timeout(Duration::from_secs(2), task).await??;
    assert!(
        matches!(outcome, Err(CallFailure::AnswerLost)),
        "expiry invented progress: {outcome:?}"
    );
    objects.shutdown().await?;
    Ok(())
}

// Admission writes the registered task's exact kind/version/input through the
// public journal API; this local type never executes the task or replaces its rules.
struct SavedCopyProbe;
impl Task for SavedCopyProbe {
    const KIND: &'static str = "central.conformance-copy-task";
    const VERSION: u32 = 1;
    type Store = <NativeHost as ObjectPlatform>::Store;
    type Input = Value;
    type Output = Value;
    async fn run<Context: OperationContext<Store = Self::Store>>(
        &self,
        _: &mut Context,
        _: &Value,
    ) -> Result<Value, OperationFailure> {
        Err(OperationFailure::InvalidPayload {
            message: "admission type must never execute".into(),
        })
    }
}
#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn registry_consumed_wake_recovers_its_saved_operation_after_two_begin_failures() -> TestResult
{
    let root = tempfile::tempdir()?;
    let objects = open_with_clock(
        root.path(),
        Arc::new(TokioClock(tokio::time::Instant::now())),
    )
    .await?;
    let slot = objects.slot(ObjectAddress::Fleet).await?;
    ready(&slot).await?;
    let host = slot.host.get().ok_or("Registry host missing")?;
    let database = DatabaseId::new(durable_steps::Uuid::now_v7())?;
    let key = RequestKey::new("retained-original-alarm-probe")?;
    let input = json!({"attempt":durable_steps::Uuid::now_v7(),"copy":objects.config.authoritative_receiver,"request":{"kind":"create","value":{"database":database,"epoch":1,"holder":durable_steps::Uuid::now_v7(),"origin":{"kind":"empty"}}}});
    let saved_key = key.clone();
    let operation = host
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                Ok(scope
                    .admit::<SavedCopyProbe>(&saved_key, &input)
                    .await
                    .map_err(StoreError::backend)?
                    .id())
            })
        })
        .await?;
    host.observed.inner().fail_next_begins(2);
    let mut generation = slot.alarm_owner_generation.subscribe();
    let mut previous = *generation.borrow_and_update();
    tokio::time::pause();
    slot.io
        .wake_at(slot.io.now().after(Duration::from_millis(50)))
        .await?;
    for elapsed in [Duration::from_millis(50), Duration::from_secs(1)] {
        tokio::time::advance(elapsed).await;
        let current = *tokio::time::timeout(
            Duration::from_secs(3),
            generation.wait_for(|current| *current > previous),
        )
        .await??;
        assert_eq!(
            current,
            previous + 1,
            "one virtual deadline must finish one owner drive"
        );
        previous = current;
    }
    assert_eq!(host.observed.inner().begin_failures(), 2);
    tokio::time::advance(Duration::from_secs(2)).await;
    // Independent phases can consume both injected failures in one drive and
    // still complete the operation. Do not demand another alarm after the
    // journal is already settled; observe the original outcome directly.
    tokio::time::resume();
    let observed = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            // Read-only public journal observation bypasses the injected wrapper;
            // it cannot consume the failure or drive/rearm the Registry.
            let record = host
                .object
                .transaction(move |scope| Box::pin(async move { scope.operation(operation).await }))
                .await?
                .ok_or("saved operation disappeared")?;
            match record.execution {
                OperationExecution::Completed { .. } => {
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(record);
                }
                OperationExecution::Failed { .. } => {
                    return Err(format!("saved copy probe failed: {record:?}").into());
                }
                OperationExecution::Running { .. } => {}
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    assert_eq!(host.observed.inner().begin_failures(), 2);
    println!("ALARM_FAILURES registry begin_failures=2");
    let observed = match observed {
        Ok(result) => result?,
        Err(error) => {
            let retained = host
                .object
                .transaction(move |scope| Box::pin(async move { scope.operation(operation).await }))
                .await?;
            return Err(
                format!("saved probe recovery timed out: {error}; retained={retained:?}").into(),
            );
        }
    };
    assert_eq!(observed.id, operation);
    assert_eq!(observed.request_key, key);
    assert_eq!(host.observed.inner().begin_failures(), 2);
    let copy_slot = objects.slot(ObjectAddress::Copy(database)).await?;
    let RuntimeFace::Copy(copy_kernel) = copy_slot.runtime.get().ok_or("copy missing")?.as_ref()
    else {
        return Err("wrong face".into());
    };
    let copy = copy_kernel.face();
    assert!(
        matches!(copy.read_copy_status().await.map_err(|failure|format!("{failure:?}"))?,CopyStatusAnswer::Status(status) if status.database==database && status.acked.sequence.value()==0)
    );
    objects.shutdown().await?;
    Ok(())
}

struct SavedFleetNotice;
impl Task for SavedFleetNotice {
    const KIND: &'static str = "central.fleet-notice";
    const VERSION: u32 = 1;
    type Store = <NativeHost as ObjectPlatform>::Store;
    type Input = Value;
    type Output = ();
    async fn run<Context: OperationContext<Store = Self::Store>>(
        &self,
        _: &mut Context,
        _: &Value,
    ) -> Result<(), OperationFailure> {
        Err(OperationFailure::InvalidPayload {
            message: "admission type must never execute".into(),
        })
    }
}

async fn completed(
    host: &NativeHost,
    operation: durable_steps::OperationId,
    key: RequestKey,
) -> TestResult {
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            // Read the real public journal outside the fault wrapper. This
            // observes completion without consuming a fault or nudging an owner.
            let record = host
                .object
                .transaction(move |scope| Box::pin(async move { scope.operation(operation).await }))
                .await?
                .ok_or("saved operation disappeared")?;
            match record.execution {
                OperationExecution::Completed { .. } => {
                    assert_eq!(record.id, operation);
                    assert_eq!(record.request_key, key);
                    return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
                }
                OperationExecution::Failed { .. } => {
                    return Err(format!("saved operation failed: {record:?}").into());
                }
                OperationExecution::Running { .. } => tokio::task::yield_now().await,
            }
        }
    })
    .await?
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn fleet_consumed_wake_recovers_its_saved_notice_after_prune_begin_failure() -> TestResult {
    let root = tempfile::tempdir()?;
    let objects = open(root.path()).await?;
    let proof = async {
        let installation = message_board::ServiceId::try_from(durable_steps::Uuid::now_v7().to_string())?;
        // Initialize the real destination before fault injection, without a
        // connected transport. The same notice must survive missing admission.
        let target = objects.slot(ObjectAddress::Fleet).await?;
        ready(&target).await?;
        let slot = objects.slot(ObjectAddress::Fleet).await?;
        ready(&slot).await?;
        let host = slot.host.get().ok_or("Fleet host missing")?;
        let peer = fleet_protocol::PeerId::new(durable_steps::Uuid::now_v7())?;
        let enrolled = installation.clone();
        host.object.transaction(move |scope|Box::pin(async move {
            use durable_steps::storage::TransactionScope;
            sqlx_core::query::query::<sqlx_turso::Turso>("INSERT INTO machine_enrollments(machine_id,service_id,machine_label,access_client_id,credential_hash,state,expires_at,created_at,updated_at) VALUES(?,?,'alarm fixture','alarm-client','alarm-credential','enrolled','2033-05-18T03:33:20+00:00','2026-10-18T00:00:00+00:00','2026-10-18T00:00:00+00:00')")
                .bind(peer.to_string()).bind(enrolled.as_str()).execute(scope.executor()).await.map_err(StoreError::backend)?;
            Ok(())
        })).await?;
        let key = RequestKey::new("retained-fleet-notice-alarm")?;
        let saved_key = key.clone();
        let input = json!({"installation":installation,"notice":{"kind":"projectListChanged","value":{}}});
        let operation = host.object.transaction(move |scope| Box::pin(async move {
            Ok(scope.admit::<SavedFleetNotice>(&saved_key, &input).await.map_err(StoreError::backend)?.id())
        })).await?;
        host.observed.inner().fail_next_begin();
        slot.io.wake_at(slot.io.now().after(Duration::from_millis(50))).await?;
        // From this single wake onward, observers only read the owning journal.
        let completion = completed(host, operation, key).await;
        assert_eq!(host.observed.inner().begin_failures(), 1);
        println!("ALARM_FAILURES maintenance begin_failures=1");
        completion?;
        let target_host = target.host.get().ok_or("notice destination missing")?;
        let rows = target_host.object.transaction(|scope| Box::pin(async move {
            use durable_steps::storage::TransactionScope;
            use sqlx_core::{query::query, row::Row};
            let rows = query::<sqlx_turso::Turso>("SELECT position,kind FROM machine_control_project_notices ORDER BY position").fetch_all(scope.executor()).await.map_err(StoreError::backend)?;
            rows.into_iter().map(|row| Ok((row.try_get::<i64, _>("position").map_err(StoreError::backend)?, row.try_get::<String, _>("kind").map_err(StoreError::backend)?))).collect::<Result<Vec<_>, _>>()
        })).await?;
        assert_eq!(rows.len(), 1);
        assert_eq!(rows, vec![(1, "project_list_changed".to_owned())]);
        Ok(())
    }.await;
    objects.shutdown().await?;
    proof
}
