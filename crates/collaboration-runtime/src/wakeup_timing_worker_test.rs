//! The wake-up worker and the push hand-off on Tokio's paused clock, with a real automation
//! store, a real interaction store and a delivery route that records when it is called.
use super::WakeTimingWorker;
use crate::{
    AutomationConfigurationHandle, PushHandoffWorker, WorkerObservation,
    paused_wall_clock::PausedWallClock,
    paused_worker_test_support::{NoAutoAdvance, TestResult, WAIT_LIMIT, advance, settled},
};
use agent_automation::{ExpiryRule, OperationId, TimingRule, WakeState};
use automation_storage::{AutomationStore, WakeAction, WakeCreate, WakeMutation};
use collaboration_contracts::{
    AttemptEvidenceSink, AttemptReconciliation, AttemptReconciliationContext, DeliveryFuture,
    SessionMessageDelivery,
};
use collaboration_protocol::{DeliveryOutcome, DeliveryReceipt, SavedMessage, SessionReachability};
use runtime_primitives::{Clock, SyncTime};
use serde_json::json;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

const BASE_MS: i64 = 1_800_000_000_000;

fn at(offset: Duration) -> SyncTime {
    SyncTime::from_unix_millis(BASE_MS).after(offset)
}

/// Records each delivery's time; the first `retryable_rejections` are refused as retryable.
struct RecordingDelivery {
    clock: Arc<PausedWallClock>,
    delivered: std::sync::Mutex<Vec<SyncTime>>,
    retryable_rejections: AtomicUsize,
}

impl SessionMessageDelivery for RecordingDelivery {
    fn deliver<'a>(
        &'a self,
        _request: collaboration_contracts::layer_zero::DeliveryRequest,
        _evidence: &'a dyn AttemptEvidenceSink,
    ) -> DeliveryFuture<'a, collaboration_contracts::NativeDeliveryResult> {
        self.delivered.lock().unwrap().push(self.clock.now());
        let rejected = self
            .retryable_rejections
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |left| {
                left.checked_sub(1)
            })
            .is_ok();
        Box::pin(async move {
            let outcome = if rejected {
                DeliveryOutcome::NotSubmitted {
                    retryable: true,
                    reason: "fixture busy".into(),
                }
            } else {
                DeliveryOutcome::Started
            };
            Ok(collaboration_contracts::scripted_delivery_result(
                DeliveryReceipt {
                    outcome,
                    reachability: Some(SessionReachability::ProviderAcp),
                    client: None,
                },
            ))
        })
    }

    fn reconcile_attempt(
        &self,
        _context: AttemptReconciliationContext,
    ) -> DeliveryFuture<'_, AttemptReconciliation> {
        Box::pin(async { Ok(AttemptReconciliation::KnownNotSubmitted) })
    }
}

struct WakeFixture {
    _no_auto_advance: NoAutoAdvance,
    root: tempfile::TempDir,
    store: Arc<Mutex<AutomationStore>>,
    interactions: Arc<interaction_storage::InteractionStore>,
    delivery: Arc<RecordingDelivery>,
    clock: Arc<PausedWallClock>,
}

struct RunningWakes {
    observation: WorkerObservation,
    shutdown: CancellationToken,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl RunningWakes {
    async fn stop(self) {
        self.shutdown.cancel();
        for task in self.tasks {
            task.await.unwrap();
        }
    }
}

impl WakeFixture {
    async fn new() -> TestResult<Self> {
        let no_auto_advance = NoAutoAdvance::hold();
        let root = tempfile::tempdir()?;
        let store = Arc::new(Mutex::new(
            AutomationStore::open(&root.path().join("automation.sqlite")).await?,
        ));
        let interactions = Arc::new(
            interaction_storage::InteractionStore::open(root.path().join("interaction.sqlite"))
                .await?,
        );
        let clock = Arc::new(PausedWallClock::starting_at(at(Duration::ZERO)));
        let delivery = Arc::new(RecordingDelivery {
            clock: Arc::clone(&clock),
            delivered: std::sync::Mutex::default(),
            retryable_rejections: AtomicUsize::new(0),
        });
        Ok(Self {
            _no_auto_advance: no_auto_advance,
            root,
            store,
            interactions,
            delivery,
            clock,
        })
    }

    async fn create_wake(
        &self,
        session: &str,
        after_seconds: u32,
        expiry: ExpiryRule,
    ) -> TestResult<agent_automation::WakeupId> {
        let message: SavedMessage = serde_json::from_value(json!({
            "target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},"sessionId":session},
            "content":{"kind":"humanUser","text":"wake fixture"},
            "delivery":"auto"
        }))?;
        let wake = self
            .store
            .lock()
            .await
            .create_wakeup(&WakeCreate {
                requested_by: collaboration_protocol::WakeCreator::Owner {},
                replace: false,
                operation_id: OperationId::generate(),
                message,
                timing: TimingRule::After {
                    seconds: after_seconds,
                },
                expiry,
                now_ms: self.clock.now().unix_millis(),
            })
            .await?;
        Ok(wake.definition.wakeup_id)
    }

    /// Starts the wake-up worker and the push hand-off, both on the loop helper.
    fn start(&self) -> TestResult<RunningWakes> {
        let mut wakes = self.start_wakes_only(|worker| worker)?;
        let handoff =
            PushHandoffWorker::new(Arc::clone(&self.store), Arc::clone(&self.interactions));
        wakes.tasks.push(tokio::spawn(handoff.run(
            Arc::clone(&self.clock) as Arc<dyn Clock>,
            wakes.shutdown.clone(),
        )));
        Ok(wakes)
    }

    /// Starts only the wake-up worker; the test applies the hand-off itself.
    fn start_wakes_only(
        &self,
        configure: impl FnOnce(WakeTimingWorker) -> WakeTimingWorker,
    ) -> TestResult<RunningWakes> {
        let worker = WakeTimingWorker::new(
            Arc::clone(&self.store),
            Arc::clone(&self.delivery) as Arc<dyn SessionMessageDelivery>,
            AutomationConfigurationHandle::default(),
            collaboration_contracts::MachineIdentity::new(
                "00000000-0000-4000-8000-000000000001"
                    .to_owned()
                    .try_into()?,
                None,
            )?,
            Arc::clone(&self.interactions),
            Arc::clone(&self.clock) as Arc<dyn Clock>,
        );
        let worker = configure(worker);
        let observation = worker.observation();
        let shutdown = CancellationToken::new();
        let tasks = vec![tokio::spawn(worker.run(shutdown.clone()))];
        Ok(RunningWakes {
            observation,
            shutdown,
            tasks,
        })
    }

    fn deliveries(&self) -> Vec<SyncTime> {
        self.delivery.delivered.lock().unwrap().clone()
    }

    /// Waits, without moving the clock, until `count` deliveries happened and the worker
    /// is asleep again. It fails once no delivery happened for a whole wait limit.
    async fn delivered(&self, wakes: &RunningWakes, count: usize) -> TestResult {
        let mut seen = self.deliveries().len();
        let mut idle = 0;
        while idle < WAIT_LIMIT {
            let now_seen = self.deliveries().len();
            if now_seen >= count {
                return settled(&wakes.observation).await;
            }
            if now_seen == seen {
                idle += 1;
            } else {
                seen = now_seen;
                idle = 0;
            }
            tokio::task::yield_now().await;
        }
        Err(format!("deliveries stalled at {seen} of {count}").into())
    }
}

#[tokio::test(start_paused = true)]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn an_idle_wake_worker_holds_no_timer_and_reads_nothing_for_an_hour() -> TestResult {
    let fixture = WakeFixture::new().await?;
    let wakes = fixture.start()?;
    settled(&wakes.observation).await?;
    let asleep = wakes.observation.snapshot();

    advance(Duration::from_secs(3_600)).await;

    assert_eq!(
        asleep.reads,
        [
            "interrupted deliveries",
            "due wake-ups",
            "eligible deliveries",
            "wake-up next due"
        ]
    );
    assert_eq!(asleep.armed_deadline, None);
    assert_eq!(
        wakes.observation.snapshot(),
        asleep,
        "an idle hour reads nothing"
    );
    wakes.stop().await;
    Ok(())
}

#[tokio::test(start_paused = true)]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_wake_fires_at_its_time_on_one_timer_and_is_delivered_once() -> TestResult {
    let fixture = WakeFixture::new().await?;
    fixture
        .create_wake("woken-session", 60, ExpiryRule::None)
        .await?;
    let wakes = fixture.start()?;
    settled(&wakes.observation).await?;
    let armed = wakes.observation.snapshot().armed_deadline;

    advance(Duration::from_secs(59)).await;
    settled(&wakes.observation).await?;
    let before_due = fixture.deliveries();
    advance(Duration::from_secs(1)).await;
    fixture.delivered(&wakes, 1).await?;
    advance(Duration::from_secs(3_600)).await;

    assert_eq!(armed, Some(at(Duration::from_secs(60))));
    assert!(before_due.is_empty(), "nothing fires before its time");
    assert_eq!(
        fixture.deliveries(),
        [at(Duration::from_secs(60))],
        "fired once, at its time"
    );
    wakes.stop().await;
    Ok(())
}

#[tokio::test(start_paused = true)]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_paused_wake_expires_at_its_time() -> TestResult {
    let fixture = WakeFixture::new().await?;
    let wake = fixture
        .create_wake("woken-session", 3_600, ExpiryRule::After { seconds: 120 })
        .await?;
    fixture
        .store
        .lock()
        .await
        .mutate_wakeup::<SavedMessage>(&WakeMutation {
            operation_id: OperationId::generate(),
            wakeup_id: wake.clone(),
            action: WakeAction::Pause,
            now_ms: BASE_MS,
        })
        .await?;
    let wakes = fixture.start()?;
    settled(&wakes.observation).await?;
    let armed = wakes.observation.snapshot().armed_deadline;

    advance(Duration::from_secs(120)).await;
    settled(&wakes.observation).await?;
    let state = fixture
        .store
        .lock()
        .await
        .read_wakeup::<SavedMessage>(&wake)
        .await?
        .state;

    assert_eq!(
        armed,
        Some(at(Duration::from_secs(120))),
        "a paused wake's expiry arms the timer"
    );
    assert_eq!(state, WakeState::Expired);
    assert!(fixture.deliveries().is_empty());
    wakes.stop().await;
    Ok(())
}

#[tokio::test(start_paused = true)]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_retryable_delivery_is_dispatched_again_when_it_becomes_eligible() -> TestResult {
    let fixture = WakeFixture::new().await?;
    fixture
        .delivery
        .retryable_rejections
        .store(1, Ordering::Release);
    fixture
        .create_wake("woken-session", 60, ExpiryRule::None)
        .await?;
    let wakes = fixture.start()?;
    settled(&wakes.observation).await?;

    advance(Duration::from_secs(60)).await;
    fixture.delivered(&wakes, 1).await?;
    let retry = wakes
        .observation
        .snapshot()
        .armed_deadline
        .ok_or("the retryable delivery arms the timer")?;
    advance(Duration::from_millis(u64::try_from(
        retry.unix_millis() - fixture.clock.now().unix_millis(),
    )?))
    .await;
    fixture.delivered(&wakes, 2).await?;

    assert!(retry > at(Duration::from_secs(60)));
    assert_eq!(
        fixture.deliveries(),
        [at(Duration::from_secs(60)), retry],
        "redispatched exactly when it became eligible"
    );
    wakes.stop().await;
    Ok(())
}

#[tokio::test(start_paused = true)]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn more_than_a_batch_of_wakes_and_slots_is_drained_without_a_clock_advance() -> TestResult {
    let fixture = WakeFixture::new().await?;
    for session in 0..120 {
        fixture
            .create_wake(&format!("woken-session-{session}"), 60, ExpiryRule::None)
            .await?;
    }
    let wakes = fixture.start()?;
    settled(&wakes.observation).await?;

    advance(Duration::from_secs(60)).await;
    fixture.delivered(&wakes, 120).await?;

    assert_eq!(fixture.deliveries().len(), 120);
    assert!(
        fixture
            .deliveries()
            .iter()
            .all(|delivered| *delivered == at(Duration::from_secs(60))),
        "120 wakes (more than one batch of 100, more than 16 dispatch slots) drain at their due time"
    );
    wakes.stop().await;
    Ok(())
}

#[tokio::test(start_paused = true)]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_failed_read_retries_on_the_backoff_and_recovers_when_the_fault_is_fixed() -> TestResult {
    let fixture = WakeFixture::new().await?;
    let mut fault = <sqlx::SqliteConnection as sqlx::Connection>::connect(&format!(
        "sqlite://{}",
        fixture.root.path().join("automation.sqlite").display()
    ))
    .await?;
    sqlx::query("ALTER TABLE wakeup_definitions RENAME TO wakeup_definitions_hidden")
        .execute(&mut fault)
        .await?;
    let wakes = fixture.start()?;
    settled(&wakes.observation).await?;
    let first_retry = wakes.observation.snapshot().armed_deadline;
    advance(Duration::from_secs(1)).await;
    settled(&wakes.observation).await?;
    let second_retry = wakes.observation.snapshot().armed_deadline;
    sqlx::query("ALTER TABLE wakeup_definitions_hidden RENAME TO wakeup_definitions")
        .execute(&mut fault)
        .await?;
    advance(Duration::from_secs(2)).await;
    settled(&wakes.observation).await?;

    assert_eq!(first_retry, Some(at(Duration::from_secs(1))));
    assert_eq!(second_retry, Some(at(Duration::from_secs(3))));
    assert_eq!(
        wakes.observation.snapshot().armed_deadline,
        None,
        "recovered, and nothing is due"
    );
    wakes.stop().await;
    Ok(())
}

#[tokio::test(start_paused = true)]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_wake_created_while_the_worker_sleeps_arms_its_timer_without_a_clock_advance()
-> TestResult {
    let fixture = WakeFixture::new().await?;
    let wakes = fixture.start()?;
    settled(&wakes.observation).await?;
    let idle = wakes.observation.snapshot().armed_deadline;

    fixture
        .create_wake("woken-session", 60, ExpiryRule::None)
        .await?;
    settled(&wakes.observation).await?;

    assert_eq!(idle, None);
    assert_eq!(
        wakes.observation.snapshot().armed_deadline,
        Some(at(Duration::from_secs(60))),
        "the creating commit woke the worker, which armed the new wake"
    );
    wakes.stop().await;
    Ok(())
}

#[tokio::test(start_paused = true)]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_push_admitted_while_its_dispatch_is_in_flight_is_still_delivered() -> TestResult {
    let fixture = WakeFixture::new().await?;
    fixture
        .create_wake("woken-session", 60, ExpiryRule::None)
        .await?;
    let gate = Arc::new(Mutex::new(()));
    let held = Arc::clone(&gate).lock_owned().await;
    let arrivals = Arc::new(AtomicUsize::new(0));
    let wakes = fixture.start_wakes_only(|worker| {
        worker.hold_dispatch_reports(Arc::clone(&gate), Arc::clone(&arrivals))
    })?;
    settled(&wakes.observation).await?;

    // At its time the wake's dispatch finds its push not yet admitted, writes nothing
    // and is held before it reports. The hand-off then admits the push; the commit wakes
    // a pass that skips the in-flight delivery.
    advance(Duration::from_secs(60)).await;
    for _ in 0..WAIT_LIMIT {
        if arrivals.load(Ordering::SeqCst) > 0 {
            break;
        }
        tokio::task::yield_now().await;
    }
    let delivered_while_unadmitted = fixture.deliveries();
    let sleeps_before_admission = wakes.observation.snapshot().sleeps;
    PushHandoffWorker::new(
        Arc::clone(&fixture.store),
        Arc::clone(&fixture.interactions),
    )
    .apply_pending()
    .await?;
    wakes
        .observation
        .asleep_at_least(sleeps_before_admission + 1)
        .await;
    settled(&wakes.observation).await?;
    drop(held);
    fixture.delivered(&wakes, 1).await?;

    assert!(arrivals.load(Ordering::SeqCst) > 0, "the dispatch was held");
    assert!(delivered_while_unadmitted.is_empty());
    assert_eq!(
        fixture.deliveries(),
        [at(Duration::from_secs(60))],
        "the dispatch owed a recheck is redispatched once admitted, with no clock advance"
    );
    wakes.stop().await;
    Ok(())
}
