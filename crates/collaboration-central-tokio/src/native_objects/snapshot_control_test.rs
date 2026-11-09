//! S8 on a real native object: its slot, store and socket table.
#![expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail these tests while fixture errors propagate through Result."
)]

use super::super::*;
use super::snapshot;
use collaboration_central_conformance::{
    RuntimeControl, RuntimeControlAnswer, RuntimeControlFailure, RuntimeControlRequest,
};
use collaboration_central_core::RegistryTime;
use collaboration_central_core::conformance_snapshot::AttemptRecord;
use durable_steps::{AtomicStore, StoreError, storage::TransactionScope};
use sqlx_core::query::query;
use std::time::Duration;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
use super::super::test_configuration as configuration;

/// The injected business time (2026-10-17T00:00:00Z).
const T0: i64 = 1_792_195_200_000;

async fn open(root: &Path) -> TestResult<Arc<NativeObjects>> {
    Ok(NativeObjects::open_with_controls(
        root,
        Arc::new(runtime_primitives::SystemClock),
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

/// A Fleet whose alarm loop has finished its startup drive, at T0.
async fn quiet_fleet(objects: &Arc<NativeObjects>) -> TestResult<Arc<ObjectSlot>> {
    let slot = objects.slot(ObjectAddress::Fleet).await?;
    let mut ready = slot.alarm_owner_generation.subscribe();
    tokio::time::timeout(
        Duration::from_secs(2),
        ready.wait_for(|generation| *generation > 0),
    )
    .await??;
    objects
        .runtime_control(RuntimeControlRequest {
            address: ObjectAddress::Fleet,
            command: RuntimeControl::SettleClock {
                now: RegistryTime(
                    chrono::DateTime::from_timestamp_millis(T0).ok_or("time out of range")?,
                ),
            },
        })
        .await?;
    Ok(slot)
}

/// Store `row` as the shells' writers would, through the object's store.
async fn insert_attempt(slot: &ObjectSlot, row: AttemptRecord) -> TestResult {
    let host = slot.host.get().ok_or("the Fleet has no host")?;
    host.object
        .transaction(move |scope| {
            Box::pin(async move {
                let text = |id: Option<durable_steps::Uuid>| id.map(|id| id.to_string());
                query::<sqlx_turso::Turso>(
                    "INSERT INTO central_work_attempts (id,subject,kind,state,send_id,connection_id,session_id,sent_version,next_attempt_at,attempts,first_failed_at,last_error_class,last_error,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
                )
                .bind(row.id.to_string())
                .bind(row.subject.to_string())
                .bind(row.kind)
                .bind(row.state)
                .bind(text(row.send_id))
                .bind(text(row.connection_id))
                .bind(text(row.session_id))
                .bind(row.sent_version.map(|version| i64::try_from(version).unwrap_or(i64::MAX)))
                .bind(row.next_attempt_at)
                .bind(i64::try_from(row.attempts).unwrap_or(i64::MAX))
                .bind(row.first_failed_at)
                .bind(row.last_error_class)
                .bind(row.last_error)
                .bind(row.created_at)
                .bind(row.updated_at)
                .execute(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                Ok(())
            })
        })
        .await?;
    Ok(())
}

/// Corrupt one stored subject: a value the snapshot must refuse to decode.
async fn corrupt_subject(slot: &ObjectSlot, id: durable_steps::Uuid) -> TestResult {
    let host = slot.host.get().ok_or("the Fleet has no host")?;
    host.object
        .transaction(move |scope| {
            Box::pin(async move {
                query::<sqlx_turso::Turso>(
                    "UPDATE central_work_attempts SET subject='not-a-uuid' WHERE id=?1",
                )
                .bind(id.to_string())
                .execute(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                Ok(())
            })
        })
        .await?;
    Ok(())
}

#[tokio::test]
async fn an_object_with_no_open_store_answers_not_open_and_opens_nothing() -> TestResult {
    // Arrange: a Copy nothing has run in.
    let root = tempfile::tempdir()?;
    let objects = open(root.path()).await?;
    let copy = ObjectAddress::Copy(fleet_protocol::DatabaseId::new(
        durable_steps::Uuid::now_v7(),
    )?);
    let slot = objects.slot_entry(&copy).await?;

    // Act
    let answer = snapshot(&slot).await;

    // Assert: refused, and the slot still has no store, kernel or alarm loop.
    assert!(matches!(answer, Err(RuntimeControlFailure::NotOpen)));
    assert!(slot.host.get().is_none());
    assert!(slot.kernel.get().is_none());
    assert!(slot.alarm_task.get().is_none());
    objects.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn a_fleet_snapshot_reports_stored_rows_as_stored_and_refuses_a_corrupt_one() -> TestResult {
    // Arrange: one push in flight on a connection that is not attached (so it
    // counts in no window), one waiting status publication.
    let root = tempfile::tempdir()?;
    let objects = open(root.path()).await?;
    let slot = quiet_fleet(&objects).await?;
    let push = AttemptRecord {
        id: durable_steps::Uuid::now_v7(),
        subject: durable_steps::Uuid::now_v7(),
        kind: "push".into(),
        state: "in_flight".into(),
        send_id: Some(durable_steps::Uuid::now_v7()),
        connection_id: Some(durable_steps::Uuid::now_v7()),
        session_id: Some(durable_steps::Uuid::now_v7()),
        sent_version: None,
        next_attempt_at: Some(T0 + 30_000),
        attempts: 0,
        first_failed_at: None,
        last_error_class: None,
        last_error: None,
        created_at: T0,
        updated_at: T0,
    };
    let status = AttemptRecord {
        id: durable_steps::Uuid::now_v7(),
        subject: durable_steps::Uuid::now_v7(),
        kind: "status".into(),
        state: "waiting".into(),
        send_id: None,
        connection_id: None,
        session_id: None,
        sent_version: None,
        next_attempt_at: None,
        attempts: 2,
        first_failed_at: Some(T0 - 1_000),
        last_error_class: Some("retryable_refusal".into()),
        last_error: Some("the recipient's mailbox is full".into()),
        created_at: T0 - 2_000,
        updated_at: T0,
    };
    insert_attempt(&slot, push.clone()).await?;
    insert_attempt(&slot, status.clone()).await?;

    // Act
    let answer = snapshot(&slot).await;

    // Assert: both rows exactly as stored; no window for an unattached
    // connection; the Fleet fields present.
    let Ok(RuntimeControlAnswer::Snapshot(snapshot_answer)) = answer else {
        return Err(format!("not a snapshot: {answer:?}").into());
    };
    for row in [&push, &status] {
        assert_eq!(
            snapshot_answer
                .attempts
                .iter()
                .find(|stored| stored.id == row.id),
            Some(row)
        );
    }
    assert!(snapshot_answer.windows.is_empty());
    assert!(snapshot_answer.inventory.is_empty());
    assert!(snapshot_answer.feed.is_some());

    // A stored value that does not decode refuses the snapshot.
    corrupt_subject(&slot, push.id).await?;
    assert!(matches!(
        snapshot(&slot).await,
        Err(RuntimeControlFailure::CorruptRow { ref table, ref field, .. })
            if table == "central_work_attempts" && field == "subject"
    ));
    objects.shutdown().await?;
    Ok(())
}
