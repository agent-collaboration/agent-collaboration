//! V12 races on the native host: a publication built before a newer claim uses
//! the newer floor and backlog, a claim waits out a decision's publication, two
//! simultaneous events run one due batch, notice units get no look-ahead, and a
//! changed organization stamp refuses every event.
use super::super::support::*;
use super::engine_event_support::*;
use super::engine_probe_support::*;
use central_storage::{AlarmPublication, CorruptReason, StoreCall, StoreFailure, StoreReply};
use collaboration_central_core::{
    AlarmNotPublished, ClockIds, PausePoint, WorkspaceRuntime, WorkspaceStoreCalls,
    engine_probe::ProbeEvent,
};
use collaboration_central_tokio::ObjectStorage;
use router_clock::RuntimeClock;
use runtime_primitives::{Clock, SyncTime};
use std::sync::{Arc, Mutex, PoisonError};

fn set(time: Time) -> AlarmPublication {
    AlarmPublication::Set {
        unix_millis: time.timestamp_millis(),
    }
}

/// Moves the observer's open window to `due`, keeping its items.
async fn window_due_at(runtime: &Runtime, due: Time) {
    raw(
        runtime,
        "UPDATE notification_windows SET due_at=?",
        vec![stamp(due)],
    )
    .await;
}

#[tokio::test]
async fn a_publication_held_before_a_newer_claim_publishes_the_newer_floor() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = fixture.clock.utc_now();
    open_notice_window(&fixture, 32000).await;
    due_batch_at(&fixture, &runtime, at(start, 4, 0, 0)).await;
    window_due_at(&runtime, at(start, 5, 0, 0)).await;
    driver_settled(&fixture, &runtime).await;
    fixture.set_utc(at(start, 4, 20, 0)).await;
    // Event A, inside the spacing, holds its only batch after commit and
    // before its alarm decision, with the 10:04:00 start still current.
    let mut held = runtime.statement_host().hold_alarm_publication();
    let event_a = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.after_event().await }
    });
    held.until_held().await;
    fixture.set_utc(at(start, 4, 58, 0)).await;
    let event_b = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.after_event().await }
    });
    // B claimed before its first await and now queues behind A's batch.
    runtime.statement_host().until_batch_waiting().await;
    assert_eq!(
        runtime.due_batch_snapshot().last_start_unix_millis,
        ms(at(start, 4, 58, 0))
    );
    fixture.set_utc(at(start, 4, 59, 0)).await;

    held.release();
    let published_a = event_a.await.unwrap().unwrap().publication;
    let completion_b = event_b.await.unwrap().unwrap();

    assert_eq!(
        published_a,
        set(at(start, 5, 28, 0)),
        "A used B's floor, never its own 10:05:00"
    );
    assert_eq!(completion_b.publication, set(at(start, 5, 28, 0)));
    assert_eq!(
        completion_b.work.windows_fired, 0,
        "the window is not run early"
    );
    assert_eq!(alarm_ms(&runtime), ms(at(start, 5, 28, 0)));
    assert_eq!(
        window_due(&runtime).await,
        vec![vec![stamp(at(start, 5, 0, 0))]]
    );
    assert_eq!(notices(&runtime).await, 0);
    fixture.host.close().await;
}

#[tokio::test]
async fn a_request_held_across_a_capped_batch_publishes_the_continuation() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = fixture.clock.utc_now();
    due_batch_at(&fixture, &runtime, at(start, 4, 0, 0)).await;
    let group = probe_id(32100);
    for index in 0..251 {
        seed_probe(
            &runtime,
            probe_id(32200 + index),
            group,
            Some(at(start, 5, 0, 0)),
            &format!("capped{index}"),
        )
        .await;
    }
    let (traffic, traffic_group) = (probe_id(32101), probe_id(32102));
    seed_probe(&runtime, traffic, traffic_group, None, "traffic").await;
    let mut held = runtime
        .pause_points()
        .hold_engine(PausePoint::AfterEngineCommit, &traffic.to_string());
    driver_settled(&fixture, &runtime).await;
    fixture.set_utc(at(start, 4, 20, 0)).await;
    let request_a = tokio::spawn({
        let runtime = runtime.clone();
        let caller = fixture.caller.clone();
        async move {
            runtime
                .request_probe(&caller, probe_request(32103, traffic, ProbeEvent::Run))
                .await
        }
    });
    held.until_held().await;
    fixture.set_utc(at(start, 4, 58, 0)).await;

    let completion_b = runtime.after_event().await.unwrap();

    assert_eq!(completion_b.publication, set(at(start, 5, 28, 0)));
    assert!(runtime.due_batch_snapshot().leftovers_pending);
    assert_eq!(
        count(
            &runtime,
            "SELECT COUNT(*) FROM engine_probe_items WHERE group_id=? AND state='fired'",
            vec![text(group)]
        )
        .await,
        250
    );
    driver_settled(&fixture, &runtime).await;
    fixture.set_utc(at(start, 4, 59, 0)).await;
    held.release();
    assert!(matches!(
        request_a.await.unwrap().unwrap(),
        collaboration_central_core::engine_probe::EngineRequestOutcome::Answer { .. }
    ));
    assert_eq!(
        alarm_ms(&runtime),
        ms(at(start, 5, 28, 0)),
        "A, published last, kept the continuation, not the 10:05:00 due"
    );
    fixture.host.close().await;
}

/// Pass-through workspace calls for a hand-built runtime.
#[derive(Clone)]
struct WorkspaceStore(Arc<ObjectStorage>);
impl WorkspaceStoreCalls for WorkspaceStore {
    async fn call_workspace(
        &self,
        _workspace: &workspace_model::WorkspaceId,
        call: StoreCall,
    ) -> Result<StoreReply, StoreFailure> {
        match call {
            StoreCall::Read(statement) => self.0.read(statement).await.map(StoreReply::Read),
            StoreCall::Batch(batch) => self.0.run_batch(batch).await.map(StoreReply::Batch),
        }
    }
}
#[derive(Clone)]
struct SharedClock(Arc<HeldClock>);
impl Clock for SharedClock {
    fn now(&self) -> SyncTime {
        self.0.now()
    }
}
/// One publication stops between its decision and its alarm write.
struct DecisionGate {
    decided: tokio::sync::oneshot::Sender<Option<i64>>,
    release: std::sync::mpsc::Receiver<()>,
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_claim_waits_for_a_publication_between_its_decision_and_its_write() {
    let fixture = Fixture::new().await;
    let start = fixture.clock.utc_now();
    {
        let runtime = runtime(&fixture).await;
        open_notice_window(&fixture, 32300).await;
        window_due_at(&runtime, at(start, 5, 0, 0)).await;
    }
    fixture.host.close().await;
    let due_batches = Arc::new(collaboration_central_core::DueBatchState::default());
    let gate: Arc<Mutex<Option<DecisionGate>>> = Arc::default();
    let decide = collaboration_central_core::engine_alarm_decider(due_batches.clone());
    let gated: central_storage::EngineAlarmDecider = Arc::new({
        let gate = gate.clone();
        move |times, now, write| {
            decide(times, now, &mut |time| {
                let armed = gate.lock().unwrap_or_else(PoisonError::into_inner).take();
                if let Some(armed) = armed {
                    armed.decided.send(time).unwrap();
                    armed.release.recv().unwrap();
                }
                write(time);
            })
        }
    });
    let central = Arc::new(
        ObjectStorage::new(
            fixture.root.path().join("central").join(format!(
                "{}.sqlite",
                fixture.caller.organization_id.as_str()
            )),
            Arc::new(collaboration_central_core::workspace_migrations::central_migrator()),
        )
        .with_engine_publication(fixture.clock.shared_for_runtime(), gated),
    );
    let workspace = Arc::new(ObjectStorage::new(
        fixture
            .root
            .path()
            .join("workspace-stores")
            .join(format!("{}.sqlite", fixture.workspace)),
        Arc::new(collaboration_central_core::workspace_migrations::workspace_migrator()),
    ));
    let runtime = Arc::new(WorkspaceRuntime::new(
        fixture.caller.organization_id.clone(),
        central.clone(),
        WorkspaceStore(workspace.clone()),
        fixture.clock.shared_for_runtime(),
        Arc::new(ClockIds::new(SharedClock(
            fixture.clock.shared_for_runtime(),
        ))),
        due_batches.clone(),
    ));
    fixture.set_utc(at(start, 4, 0, 0)).await;
    assert_eq!(
        runtime.after_event().await.unwrap().publication,
        set(at(start, 5, 0, 0))
    );
    fixture.set_utc(at(start, 4, 20, 0)).await;
    let (decided_send, decided) = tokio::sync::oneshot::channel();
    let (release, release_receiver) = std::sync::mpsc::channel();
    *gate.lock().unwrap_or_else(PoisonError::into_inner) = Some(DecisionGate {
        decided: decided_send,
        release: release_receiver,
    });
    let event_a = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.after_event().await }
    });
    assert_eq!(
        decided.await.unwrap(),
        ms(at(start, 5, 0, 0)),
        "A decided with the 10:04:00 start"
    );
    fixture.set_utc(at(start, 4, 58, 0)).await;
    let event_b = tokio::spawn({
        let runtime = runtime.clone();
        async move { runtime.after_event().await }
    });

    // Either B's claim waits on the critical section, or it lands at 10:04:58
    // between A's decision and A's write; whichever is observed first decides.
    let claim_landed = async {
        while due_batches.snapshot().last_start_unix_millis != ms(at(start, 4, 58, 0)) {
            tokio::task::yield_now().await;
        }
    };
    let waited = tokio::select! {
        () = due_batches.until_claim_waiting() => true,
        () = claim_landed => false,
    };
    let start_while_a_held = due_batches.snapshot().last_start_unix_millis;
    release.send(()).unwrap();

    assert!(waited, "B's claim crossed A's decision and publication");
    assert_eq!(start_while_a_held, ms(at(start, 4, 0, 0)));
    assert_eq!(
        event_a.await.unwrap().unwrap().publication,
        set(at(start, 5, 0, 0))
    );
    assert_eq!(
        event_b.await.unwrap().unwrap().publication,
        set(at(start, 5, 28, 0)),
        "B claimed after A published and published its own floor"
    );
    assert_eq!(
        due_batches.snapshot().last_start_unix_millis,
        ms(at(start, 4, 58, 0))
    );
    assert_eq!(central.alarm(), Some(sync(at(start, 5, 28, 0))));
    central.close().await;
    workspace.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_events_released_together_run_exactly_one_due_batch() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = fixture.clock.utc_now();
    due_batch_at(&fixture, &runtime, at(start, 4, 0, 0)).await;
    let before = runtime.legacy_unit_invocations();
    fixture.set_utc(at(start, 4, 31, 0)).await;
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let events: Vec<_> = (0..2)
        .map(|_| {
            let runtime = runtime.clone();
            let barrier = barrier.clone();
            tokio::spawn(async move {
                barrier.wait().await;
                runtime.after_event().await
            })
        })
        .collect();

    for event in events {
        event.await.unwrap().unwrap();
    }

    let after = runtime.legacy_unit_invocations();
    assert_eq!(
        (after.windows - before.windows, after.drains - before.drains),
        (1, 1),
        "exactly one due batch ran its notice units"
    );
    assert_eq!(
        runtime.due_batch_snapshot().last_start_unix_millis,
        ms(at(start, 4, 31, 0))
    );
    fixture.host.close().await;
}

#[tokio::test]
async fn a_wake_due_with_a_window_fires_early_and_the_window_waits_for_its_slot() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = fixture.clock.utc_now();
    open_notice_window(&fixture, 32400).await;
    assert_eq!(
        window_due(&runtime).await,
        vec![vec![stamp(at(start, 15, 0, 0))]]
    );
    let wake = set_wake(&fixture, 32410, at(start, 15, 0, 0)).await;
    let windows = count(
        &runtime,
        "SELECT COUNT(*) FROM notification_windows",
        vec![],
    )
    .await;
    assert!(windows > 0);

    due_batch_at(&fixture, &runtime, at(start, 14, 40, 0)).await;

    assert_eq!(
        wake_state(&runtime, &wake).await,
        text("done"),
        "20 s early"
    );
    assert_eq!(notices(&runtime).await, 0, "the window is not run early");
    assert_eq!(alarm_ms(&runtime), ms(at(start, 15, 10, 0)));
    driver_fires_at(&fixture, &runtime, at(start, 15, 10, 0)).await;
    assert_eq!(
        notices(&runtime).await,
        windows,
        "each window sent its notice"
    );
    due_batch_at(&fixture, &runtime, at(start, 15, 41, 0)).await;
    assert_eq!(
        notices(&runtime).await,
        windows,
        "each notice went out once"
    );
    fixture.host.close().await;
}

#[tokio::test]
async fn another_valid_stamp_refuses_every_event_and_touches_nothing() {
    let fixture = Fixture::new().await;
    let runtime = runtime(&fixture).await;
    let start = fixture.clock.utc_now();
    let (due, due_group) = (probe_id(32500), probe_id(32501));
    let (traffic, traffic_group) = (probe_id(32502), probe_id(32503));
    seed_probe(&runtime, due, due_group, Some(at(start, 5, 0, 0)), "due").await;
    seed_probe(&runtime, traffic, traffic_group, None, "traffic").await;
    due_batch_at(&fixture, &runtime, at(start, 1, 0, 0)).await;
    let previous = alarm_ms(&runtime);
    assert_eq!(previous, ms(at(start, 5, 0, 0)));
    let other: uuid::Uuid = id::<workspace_model::WorkspaceId>(32504)
        .to_string()
        .parse()
        .unwrap();
    assert_eq!(other.get_version_num(), 7);
    raw(
        &runtime,
        "UPDATE organization_identity SET id=?",
        vec![text(other)],
    )
    .await;
    let refused = StoreFailure::Corrupt {
        reason: CorruptReason::BadValue {
            column: "organization_identity.id".to_owned(),
        },
    };
    driver_settled(&fixture, &runtime).await;
    fixture.set_utc(at(start, 5, 0, 0)).await;

    let fan_out = runtime.after_event().await;
    let request = runtime
        .request_probe(
            &fixture.caller,
            probe_request(32505, traffic, ProbeEvent::Run),
        )
        .await;

    assert_eq!(
        fan_out,
        Err(AlarmNotPublished {
            failure: refused.clone()
        })
    );
    assert_eq!(request, Err(refused));
    for (item, group) in [(due, due_group), (traffic, traffic_group)] {
        assert_eq!(
            probe_facts(&runtime, item, group).await,
            vec![text("waiting"), integer(0), integer(0)],
            "no instance is touched"
        );
    }
    assert_eq!(receipts(&runtime, 32505).await, 0);
    assert_eq!(
        count(
            &runtime,
            "SELECT COUNT(*) FROM central_work_attempts",
            vec![]
        )
        .await,
        0,
        "no record is written"
    );
    assert_eq!(alarm_ms(&runtime), previous);

    raw(&runtime, "DELETE FROM organization_identity", vec![]).await;
    runtime.after_event().await.unwrap();
    assert_eq!(
        alarm_ms(&runtime),
        None,
        "unprovisioned: the alarm is cleared"
    );
    assert_eq!(
        probe_facts(&runtime, due, due_group).await,
        vec![text("waiting"), integer(0), integer(0)]
    );
    fixture.host.close().await;
}
