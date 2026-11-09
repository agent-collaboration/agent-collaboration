//! The D4 seams on a real native object: its alarm loop, timer and kernel.
#![expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail these tests while fixture errors propagate through Result."
)]

use super::super::*;
use collaboration_central_conformance::{
    RuntimeControl, RuntimeControlAnswer, RuntimeControlRequest, RuntimeControlResponse,
};
use collaboration_central_core::event_trace::{
    BarrierPoint, EventKindRecord, EventRecord, ReplyRecord,
};
use collaboration_central_core::{ObjectAlarm, RegistryTime};
use std::time::Duration;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
use super::super::test_configuration as configuration;

/// The injected business time every test pins first (2026-10-17T00:00:00Z).
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

fn at(millis: i64) -> TestResult<RegistryTime> {
    Ok(RegistryTime(
        chrono::DateTime::from_timestamp_millis(millis).ok_or("time out of range")?,
    ))
}

async fn control(
    objects: &Arc<NativeObjects>,
    command: RuntimeControl,
) -> TestResult<RuntimeControlResponse> {
    Ok(objects
        .runtime_control(RuntimeControlRequest {
            address: ObjectAddress::Fleet,
            command,
        })
        .await?)
}

async fn seam(
    objects: &Arc<NativeObjects>,
    command: RuntimeControl,
) -> TestResult<RuntimeControlAnswer> {
    match control(objects, command).await? {
        RuntimeControlResponse::Seam(answer) => Ok(answer),
        RuntimeControlResponse::Faults(_) => {
            Err("a seam control answered the fault observation".into())
        }
    }
}

async fn records(objects: &Arc<NativeObjects>) -> TestResult<Vec<EventRecord>> {
    match seam(objects, RuntimeControl::ReadEvents { after: 0 }).await? {
        RuntimeControlAnswer::Events { records, .. } => {
            Ok(records.into_iter().map(|entry| entry.record).collect())
        }
        other => Err(format!("not events: {other:?}").into()),
    }
}

async fn stored(objects: &Arc<NativeObjects>) -> TestResult<Option<i64>> {
    match seam(objects, RuntimeControl::ReadAlarm).await? {
        RuntimeControlAnswer::Alarm { stored } => Ok(stored),
        other => Err(format!("not an alarm: {other:?}").into()),
    }
}

/// A Fleet whose alarm loop has finished its startup drive, with the clock pinned at T0.
async fn quiet_fleet(objects: &Arc<NativeObjects>) -> TestResult<Arc<ObjectSlot>> {
    let slot = objects.slot(ObjectAddress::Fleet).await?;
    let mut ready = slot.alarm_owner_generation.subscribe();
    tokio::time::timeout(
        Duration::from_secs(2),
        ready.wait_for(|generation| *generation > 0),
    )
    .await??;
    seam(objects, RuntimeControl::SettleClock { now: at(T0)? }).await?;
    Ok(slot)
}

#[tokio::test]
async fn a_control_on_an_untouched_object_opens_nothing() -> TestResult {
    // Arrange
    let root = tempfile::tempdir()?;
    let objects = open(root.path()).await?;
    let copy = ObjectAddress::Copy(fleet_protocol::DatabaseId::new(
        durable_steps::Uuid::now_v7(),
    )?);

    // Act
    let answer = objects
        .runtime_control(RuntimeControlRequest {
            address: copy.clone(),
            command: RuntimeControl::ReadAlarm,
        })
        .await?;

    // Assert (S3): the slot exists in memory only; no store, kernel or alarm loop.
    assert!(matches!(
        answer,
        RuntimeControlResponse::Seam(RuntimeControlAnswer::Alarm { stored: None })
    ));
    let slot = objects.slot_entry(&copy).await?;
    assert!(slot.host.get().is_none());
    assert!(slot.kernel.get().is_none());
    assert!(slot.alarm_task.get().is_none());
    objects.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn set_time_moves_the_clock_without_writing_or_firing_the_alarm() -> TestResult {
    // Arrange: an alarm stored 60 s after T0.
    let root = tempfile::tempdir()?;
    let objects = open(root.path()).await?;
    let slot = quiet_fleet(&objects).await?;
    let alarm = SyncTime::from_unix_millis(T0 + 60_000);
    slot.io.wake.set_exact(Some(alarm)).await?;
    let before = records(&objects).await?.len();

    // Act: the legacy control, 10 s later. It used to lower the alarm to now.
    control(
        &objects,
        RuntimeControl::SetTime {
            now: at(T0 + 10_000)?,
        },
    )
    .await?;
    let settled = seam(
        &objects,
        RuntimeControl::SettleClock {
            now: at(T0 + 10_000)?,
        },
    )
    .await?;

    // Assert
    assert_eq!(stored(&objects).await?, Some(T0 + 60_000));
    assert_eq!(
        settled,
        RuntimeControlAnswer::ClockSettled { alarm_events: 0 }
    );
    assert_eq!(records(&objects).await?.len(), before);
    objects.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn settle_clock_answers_after_the_alarm_it_made_due_has_completed() -> TestResult {
    // Arrange
    let root = tempfile::tempdir()?;
    let objects = open(root.path()).await?;
    let slot = quiet_fleet(&objects).await?;
    slot.io
        .wake
        .set_exact(Some(SyncTime::from_unix_millis(T0 + 60_000)))
        .await?;
    let before = records(&objects).await?.len();

    // Act: move past the alarm.
    let settled = seam(
        &objects,
        RuntimeControl::SettleClock {
            now: at(T0 + 61_000)?,
        },
    )
    .await?;

    // Assert: one alarm event, already complete and in the ring when the answer came.
    assert_eq!(
        settled,
        RuntimeControlAnswer::ClockSettled { alarm_events: 1 }
    );
    let records = records(&objects).await?;
    assert_eq!(records.len(), before + 1);
    let last = records.last().ok_or("no record")?;
    assert_eq!(
        last.event,
        EventKindRecord::Alarm {
            redelivery: Some(0),
            delivery: collaboration_central_core::event_trace::AlarmDelivery::Platform,
        }
    );
    assert_eq!(last.reply, ReplyRecord::AlarmDone);
    assert_eq!(last.entered_at, T0 + 61_000);
    // Delivery consumed the stored alarm before the event entered, as on Cloudflare.
    assert_eq!(last.alarm.before, None);
    objects.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn an_alarm_with_delivery_off_is_counted_and_dropped_without_a_record() -> TestResult {
    // Arrange
    let root = tempfile::tempdir()?;
    let objects = open(root.path()).await?;
    let slot = quiet_fleet(&objects).await?;
    seam(&objects, RuntimeControl::AlarmDelivery { enabled: false }).await?;
    slot.io
        .wake
        .set_exact(Some(SyncTime::from_unix_millis(T0 + 60_000)))
        .await?;
    let before = records(&objects).await?.len();

    // Act
    let settled = seam(
        &objects,
        RuntimeControl::SettleClock {
            now: at(T0 + 61_000)?,
        },
    )
    .await?;

    // Assert: the platform consumed the alarm, and no event ran.
    assert_eq!(
        settled,
        RuntimeControlAnswer::ClockSettled { alarm_events: 1 }
    );
    assert_eq!(records(&objects).await?.len(), before);
    assert_eq!(stored(&objects).await?, None);
    objects.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn a_duplicate_alarm_completes_before_its_answer_and_reports_its_redelivery() -> TestResult {
    // Arrange
    let root = tempfile::tempdir()?;
    let objects = open(root.path()).await?;
    quiet_fleet(&objects).await?;

    // Act
    let answer = seam(&objects, RuntimeControl::DuplicateAlarm).await?;

    // Assert
    assert_eq!(answer, RuntimeControlAnswer::Done);
    let records = records(&objects).await?;
    let last = records.last().ok_or("no record")?;
    assert_eq!(
        last.event,
        EventKindRecord::Alarm {
            redelivery: Some(1),
            delivery: collaboration_central_core::event_trace::AlarmDelivery::Platform,
        }
    );
    objects.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn a_quiescent_rebuild_handles_the_next_event_in_a_new_construction() -> TestResult {
    // Arrange
    let root = tempfile::tempdir()?;
    let objects = open(root.path()).await?;
    quiet_fleet(&objects).await?;
    seam(&objects, RuntimeControl::DuplicateAlarm).await?;
    let first = records(&objects)
        .await?
        .last()
        .ok_or("no record")?
        .construction;

    // Act
    seam(
        &objects,
        RuntimeControl::RebuildAtQuiescence { enabled: true },
    )
    .await?;
    seam(&objects, RuntimeControl::DuplicateAlarm).await?;
    seam(
        &objects,
        RuntimeControl::RebuildAtQuiescence { enabled: false },
    )
    .await?;
    seam(&objects, RuntimeControl::DuplicateAlarm).await?;

    // Assert: the rebuilt kernel numbers its records from 0, and stays once the switch is off.
    let records = records(&objects).await?;
    let [.., rebuilt, kept] = records.as_slice() else {
        return Err("too few records".into());
    };
    assert_ne!(rebuilt.construction, first);
    assert_eq!(rebuilt.sequence, 0);
    assert_eq!(kept.construction, rebuilt.construction);
    assert_eq!(kept.sequence, 1);
    objects.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn a_barrier_holds_its_event_inside_the_kernel_until_released() -> TestResult {
    // Arrange
    let root = tempfile::tempdir()?;
    let objects = open(root.path()).await?;
    quiet_fleet(&objects).await?;
    let RuntimeControlAnswer::BarrierArmed { barrier_id } = seam(
        &objects,
        RuntimeControl::Barrier {
            point: BarrierPoint::BeforeClaim,
            unit_kind: None,
        },
    )
    .await?
    else {
        return Err("barrier not armed".into());
    };

    // Act: an alarm event reaches the barrier.
    let duplicate = tokio::spawn({
        let objects = objects.clone();
        async move { seam(&objects, RuntimeControl::DuplicateAlarm).await }
    });
    let held = seam(&objects, RuntimeControl::AwaitBarrier { barrier_id }).await?;
    let while_held = seam(&objects, RuntimeControl::ReadPending).await?;
    seam(&objects, RuntimeControl::ReleaseBarrier { barrier_id }).await?;
    duplicate.await??;
    let after = seam(&objects, RuntimeControl::ReadPending).await?;

    // Assert: the held event is the one recorded, and it counted as running only while held.
    let RuntimeControlAnswer::Barrier { event_id, .. } = held else {
        return Err("not a barrier answer".into());
    };
    assert_eq!(
        records(&objects)
            .await?
            .last()
            .map(|record| record.event_id),
        Some(event_id)
    );
    assert!(matches!(
        while_held,
        RuntimeControlAnswer::Pending { tasks: 1, .. }
    ));
    assert!(matches!(
        after,
        RuntimeControlAnswer::Pending { tasks: 0, .. }
    ));
    objects.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn a_seeded_stale_alarm_stays_stored_and_undelivered_until_an_event_replaces_it() -> TestResult
{
    // Arrange: the Nov 5 shape, an alarm stored 50 s in the past that never fired.
    let root = tempfile::tempdir()?;
    let objects = open(root.path()).await?;
    quiet_fleet(&objects).await?;
    seam(
        &objects,
        RuntimeControl::Seed {
            seed: collaboration_central_conformance::ObjectSeed::StaleAlarm { at: T0 - 50_000 },
        },
    )
    .await?;

    // Act: time passes without delivering it, then one event runs.
    let settled = seam(
        &objects,
        RuntimeControl::SettleClock {
            now: at(T0 + 1_000)?,
        },
    )
    .await?;
    let still_stale = stored(&objects).await?;
    seam(&objects, RuntimeControl::DuplicateAlarm).await?;

    // Assert
    assert_eq!(
        settled,
        RuntimeControlAnswer::ClockSettled { alarm_events: 0 }
    );
    assert_eq!(still_stale, Some(T0 - 50_000));
    assert_ne!(stored(&objects).await?, Some(T0 - 50_000));
    objects.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn an_armed_exact_write_fault_fails_one_alarm_event_and_keeps_an_alarm_write_row()
-> TestResult {
    // Arrange
    let root = tempfile::tempdir()?;
    let objects = open(root.path()).await?;
    let slot = quiet_fleet(&objects).await?;
    let RuntimeControlAnswer::FaultArmed { fault_id } = seam(
        &objects,
        RuntimeControl::ArmFault {
            fault: collaboration_central_conformance::ObjectFault::ExactAlarmWrite {
                on: collaboration_central_conformance::FaultedEvent::Alarm,
            },
            count: 1,
        },
    )
    .await?
    else {
        return Err("fault not armed".into());
    };

    // Act: two alarm events; only the first meets an armed fault.
    seam(&objects, RuntimeControl::DuplicateAlarm).await?;
    seam(&objects, RuntimeControl::DuplicateAlarm).await?;

    // Assert: the faulted event asked for redelivery; the next one wrote the alarm
    // and cleared the failure row the first one left.
    let records = records(&objects).await?;
    let [.., faulted, recovered] = records.as_slice() else {
        return Err("too few records".into());
    };
    assert_eq!(faulted.reply, ReplyRecord::AlarmRetry);
    assert_eq!(recovered.reply, ReplyRecord::AlarmDone);
    assert_eq!(
        seam(&objects, RuntimeControl::ReadFault { fault_id }).await?,
        RuntimeControlAnswer::Fault {
            armed: 1,
            reached: 2,
            consumed: 1
        }
    );
    let host = slot.host.get().ok_or("no host")?.clone();
    use durable_steps::AtomicStore as _;
    let rows: i64 = host
        .object
        .transaction(|scope| {
            Box::pin(async move {
                use durable_steps::storage::TransactionScope;
                use sqlx_core::{query::query, row::Row};
                query::<sqlx_turso::Turso>(
                    "SELECT COUNT(*) AS rows FROM central_work_attempts WHERE kind='alarm_write'",
                )
                .fetch_one(scope.executor())
                .await
                .map_err(StoreError::backend)?
                .try_get("rows")
                .map_err(StoreError::backend)
            })
        })
        .await?;
    assert_eq!(
        rows, 0,
        "the successful write must forget the alarm_write row"
    );
    // The cleared row's retry time is not read back as due work: a quiet Fleet
    // stores no alarm after the recovered write (Verifier finding on 12e5c11f9).
    assert_eq!(stored(&objects).await?, None);
    objects.shutdown().await?;
    Ok(())
}
