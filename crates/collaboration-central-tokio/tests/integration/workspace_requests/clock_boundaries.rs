//! One injected clock drives UUIDs, signed admission and native alarm execution.
use super::{engine_v2_support::*, support::*};
use collaboration_central_core::identity_contract::MachineConnectionRefusal;
use router_clock::{ClockInstant, DriveGuard, GatedClock, RuntimeClock};
use runtime_primitives::{Clock, SyncTime};
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};

struct DistinctUtcView(Arc<GatedClock>);

impl Clock for DistinctUtcView {
    fn now(&self) -> SyncTime {
        self.0.now()
    }
}

impl RuntimeClock for DistinctUtcView {
    fn utc_now(&self) -> Time {
        self.0.utc_now() + chrono::Duration::milliseconds(1)
    }

    fn monotonic_now(&self) -> ClockInstant {
        self.0.monotonic_now()
    }

    fn sleep_until(&self, deadline: ClockInstant) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        self.0.sleep_until(deadline)
    }

    fn drive_guard(&self) -> Pin<Box<dyn Future<Output = Option<DriveGuard>> + Send + '_>> {
        self.0.drive_guard()
    }

    fn wall_deadline(&self, deadline: SyncTime) -> ClockInstant {
        self.0.wall_deadline(deadline)
    }
}

#[tokio::test]
async fn injected_clock_drives_uuid_minting_issuer_expiry_and_the_armed_alarm() {
    let fixture = Fixture::with_host_clock(|clock| Arc::new(DistinctUtcView(clock))).await;
    let runtime = runtime(&fixture).await;
    let start = fixture.clock.utc_now();
    let issuer_deadline = start + chrono::Duration::minutes(4);
    let due = start + chrono::Duration::minutes(5);
    let token = signed::machine_token(
        fixture.caller.organization_id().as_str(),
        &fixture.caller.caller().machine_id().to_string(),
        &id::<workspace_model::WorkspaceId>(4).to_string(),
        start.timestamp(),
        issuer_deadline.timestamp(),
    )
    .unwrap();

    let first = set_wake(&fixture, 7300, due, "first injected wake").await;
    fixture.advance(Duration::from_millis(1)).await;
    let second = set_wake(&fixture, 7301, due, "second injected wake").await;
    for (wake, milliseconds) in [(&first, 0), (&second, 1)] {
        let minted = uuid::Uuid::parse_str(wake).unwrap();
        assert!(fleet_runtime_ports::uuid_v7(minted));
        let (seconds, nanos) = minted.get_timestamp().unwrap().to_unix();
        assert_eq!(
            seconds * 1_000 + u64::from(nanos / 1_000_000),
            u64::try_from(start.timestamp_millis()).unwrap() + milliseconds
        );
    }
    assert_eq!(alarm_ms(&runtime), Some(due.timestamp_millis()));
    fixture
        .clock
        .not_due_check_now(ClockInstant::ZERO.after(Duration::from_secs(300)))
        .await;

    fixture.advance(Duration::from_millis(239_998)).await;
    let admitted = runtime
        .admit_machine_connection(&token)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        admitted.effective_expires_at().0,
        issuer_deadline.timestamp()
    );
    let enrollment_sql = "SELECT machine_id,state,enrollment_generation FROM login_machines";
    let before_expiry = rows(&runtime, enrollment_sql, vec![]).await;
    fixture.advance(Duration::from_millis(1)).await;
    assert!(matches!(
        runtime.admit_machine_connection(&token).await.unwrap(),
        Err(MachineConnectionRefusal::EnrollmentNotActive {})
    ));
    assert_eq!(rows(&runtime, enrollment_sql, vec![]).await, before_expiry);

    fixture.advance(Duration::from_millis(59_999)).await;
    assert_eq!(wake_messages(&runtime, &first).await, 0);
    assert_eq!(wake_messages(&runtime, &second).await, 0);
    assert_eq!(alarm_ms(&runtime), Some(due.timestamp_millis()));

    let next_completion = runtime.runtime().alarm_completion_count() + 1;
    fixture.advance(Duration::from_millis(1)).await;
    runtime
        .runtime()
        .until_alarm_completed(next_completion)
        .await;
    assert_eq!(wake_messages(&runtime, &first).await, 1);
    assert_eq!(wake_messages(&runtime, &second).await, 1);
    assert_eq!(alarm_ms(&runtime), None);
    fixture.host.close().await;
}

struct FailedDriveClock {
    clock: Arc<GatedClock>,
    first_alarm_ms: i64,
    port_samples: std::sync::atomic::AtomicUsize,
    retry_mark: tokio::sync::watch::Sender<Option<SyncTime>>,
}

impl Clock for FailedDriveClock {
    fn now(&self) -> SyncTime {
        use std::sync::atomic::Ordering;
        let current = self.clock.now();
        if current.unix_millis() != self.first_alarm_ms {
            return current;
        }
        let offset = if self.port_samples.fetch_add(1, Ordering::SeqCst) == 0 {
            2
        } else {
            1
        };
        SyncTime::from_unix_millis(self.first_alarm_ms - offset)
    }
}

impl RuntimeClock for FailedDriveClock {
    fn utc_now(&self) -> Time {
        self.clock.utc_now()
    }

    fn monotonic_now(&self) -> ClockInstant {
        self.clock.monotonic_now()
    }

    fn sleep_until(&self, deadline: ClockInstant) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        self.clock.sleep_until(deadline)
    }

    fn drive_guard(&self) -> Pin<Box<dyn Future<Output = Option<DriveGuard>> + Send + '_>> {
        self.clock.drive_guard()
    }

    fn wall_deadline(&self, deadline: SyncTime) -> ClockInstant {
        if self.clock.now().unix_millis() == self.first_alarm_ms {
            self.retry_mark.send_if_modified(|observed| {
                if observed.is_some() {
                    return false;
                }
                *observed = Some(deadline);
                true
            });
        }
        self.clock.wall_deadline(deadline)
    }
}

#[tokio::test]
async fn a_failed_alarm_drive_uses_the_fresh_utc_sample_for_its_real_retry() {
    use central_storage::{AlarmUpdate, BatchOutcome, Statement, StatementBatch, StoreFailure};

    let (retry_mark, mut observed_retry) = tokio::sync::watch::channel(None);
    let fixture = Fixture::with_host_clock(|clock| {
        Arc::new(FailedDriveClock {
            first_alarm_ms: clock.now().unix_millis() + 300_000,
            clock,
            port_samples: Default::default(),
            retry_mark,
        })
    })
    .await;
    let runtime = runtime(&fixture).await;
    let start = fixture.clock.utc_now();
    let first_alarm = start + chrono::Duration::minutes(5);
    let wake_due = start + chrono::Duration::minutes(10);
    let wake = set_wake(&fixture, 7304, wake_due, "retry after failed drive").await;
    let store = runtime.runtime().statement_host();
    assert!(matches!(
        store
            .run_batch(StatementBatch {
                statements: vec![],
                alarm: AlarmUpdate::SetFromQuery(Statement {
                    sql: "SELECT ?",
                    args: vec![integer(first_alarm.timestamp_millis())],
                }),
            })
            .await
            .unwrap(),
        BatchOutcome::Applied { .. }
    ));
    fixture
        .clock
        .not_due_check_now(ClockInstant::ZERO.after(Duration::from_secs(300)))
        .await;
    store.fail_next_alarm_batch(StoreFailure::Unavailable);

    let completed = runtime.runtime().alarm_completion_count();
    fixture.advance(Duration::from_secs(300)).await;
    let scheduled = *observed_retry.wait_for(Option::is_some).await.unwrap();

    assert_eq!(
        scheduled,
        Some(SyncTime::from_unix_millis(wake_due.timestamp_millis()))
    );
    assert_eq!(
        runtime
            .runtime()
            .due_batch_snapshot()
            .last_start_unix_millis,
        Some(first_alarm.timestamp_millis() - 2),
    );
    assert_eq!(
        store.failed_alarm_publication_at(),
        Some(SyncTime::from_unix_millis(
            first_alarm.timestamp_millis() - 1
        )),
    );
    assert_eq!(runtime.runtime().alarm_failures().len(), 1);
    assert_eq!(runtime.runtime().alarm_completion_count(), completed);
    fixture.advance(Duration::from_millis(299_999)).await;
    assert_eq!(wake_messages(&runtime, &wake).await, 0);
    assert_eq!(runtime.runtime().alarm_completion_count(), completed);

    fixture.advance(Duration::from_millis(1)).await;
    runtime.runtime().until_alarm_completed(completed + 1).await;
    assert_eq!(runtime.runtime().alarm_completion_count(), completed + 1);
    assert_eq!(wake_messages(&runtime, &wake).await, 1);
    fixture.host.close().await;
}
