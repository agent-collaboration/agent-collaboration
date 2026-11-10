//! The push hand-off alone on the injected manual clock, with real automation and interaction
//! stores: it sleeps until an automation commit, applies what is owed, and retries a
//! failed read on the backoff with one warning.
use super::PushHandoffWorker;
use crate::{
    WorkerObservation,
    captured_warnings_test_support::CapturedWarnings,
    worker_test_support::{TestResult, advance, settled},
};
use agent_automation::{ExpiryRule, OperationId, TimingRule};
use automation_storage::{AutomationStore, MAX_OWED_PUSH_OPERATION_BATCH, WakeCreate};
use collaboration_protocol::SavedMessage;
use router_clock::{ClockInstant, ManualClock};
use runtime_primitives::{Clock, SyncTime};
use serde_json::json;
use std::{sync::Arc, time::Duration};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

const BASE_MS: i64 = 1_800_000_000_000;

fn at(offset: Duration) -> SyncTime {
    SyncTime::from_unix_millis(BASE_MS).after(offset)
}

struct HandoffFixture {
    root: tempfile::TempDir,
    store: Arc<Mutex<AutomationStore>>,
    clock: Arc<ManualClock>,
    observation: WorkerObservation,
    shutdown: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}

impl HandoffFixture {
    async fn start() -> TestResult<Self> {
        let root = tempfile::tempdir()?;
        let store = Arc::new(Mutex::new(
            AutomationStore::open(&root.path().join("automation.sqlite")).await?,
        ));
        let interactions = Arc::new(
            interaction_storage::InteractionStore::open(root.path().join("interaction.sqlite"))
                .await?,
        );
        let clock = Arc::new(ManualClock::starting_at(at(Duration::ZERO)).unwrap());
        let worker = PushHandoffWorker::new(Arc::clone(&store), interactions);
        let observation = worker.observation();
        let shutdown = CancellationToken::new();
        let task = tokio::spawn(worker.run(
            Arc::clone(&clock) as Arc<dyn router_clock::RuntimeClock>,
            shutdown.clone(),
        ));
        Ok(Self {
            root,
            store,
            clock,
            observation,
            shutdown,
            task,
        })
    }

    /// Commits one owed push admission: a wake-up due now, evaluated.
    async fn owe_one_push(&self) -> TestResult {
        let message: SavedMessage = serde_json::from_value(json!({
            "target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},"sessionId":"woken-session"},
            "content":{"kind":"humanUser","text":"wake fixture"},
            "delivery":"auto"
        }))?;
        let now_ms = self.clock.now().unix_millis();
        let wake = self
            .store
            .lock()
            .await
            .create_wakeup(&WakeCreate {
                requested_by: collaboration_protocol::WakeCreator::Owner {},
                replace: false,
                operation_id: OperationId::generate(),
                message,
                timing: TimingRule::After { seconds: 1 },
                expiry: ExpiryRule::None,
                now_ms,
            })
            .await?;
        self.store
            .lock()
            .await
            .evaluate_wakeup::<SavedMessage>(
                &wake.definition.wakeup_id,
                now_ms + 1_000,
                crate::wakeup_delivery_sender::build_wake_push_draft,
            )
            .await?;
        Ok(())
    }

    async fn owed(&self) -> TestResult<usize> {
        Ok(self
            .store
            .lock()
            .await
            .list_owed_push_operations(MAX_OWED_PUSH_OPERATION_BATCH)
            .await?
            .len())
    }

    async fn fault_connection(&self) -> TestResult<sqlx::SqliteConnection> {
        Ok(
            <sqlx::SqliteConnection as sqlx::Connection>::connect(&format!(
                "sqlite://{}",
                self.root.path().join("automation.sqlite").display()
            ))
            .await?,
        )
    }

    async fn stop(self) {
        self.shutdown.cancel();
        self.task.await.unwrap();
    }
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn an_idle_handoff_holds_no_timer_and_reads_nothing_for_an_hour() -> TestResult {
    let fixture = HandoffFixture::start().await?;
    settled(&fixture.observation).await?;
    let asleep = fixture.observation.snapshot();

    advance(&fixture.clock, Duration::from_secs(3_600)).await;

    assert_eq!(asleep.reads, ["owed push operations"]);
    assert_eq!(asleep.armed_deadline, None);
    assert_eq!(
        fixture.observation.snapshot(),
        asleep,
        "an idle hour reads nothing"
    );
    fixture.stop().await;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_committed_push_admission_wakes_the_handoff_with_no_clock_advance() -> TestResult {
    let fixture = HandoffFixture::start().await?;
    settled(&fixture.observation).await?;
    let started_at = fixture.clock.now();

    fixture.owe_one_push().await?;
    let mut owed = fixture.owed().await?;
    while owed > 0 {
        tokio::task::yield_now().await;
        owed = fixture.owed().await?;
    }
    settled(&fixture.observation).await?;

    assert_eq!(owed, 0, "the commit woke the hand-off, which applied it");
    assert_eq!(fixture.clock.now(), started_at, "with no clock advance");
    assert_eq!(fixture.observation.snapshot().armed_deadline, None);
    fixture.stop().await;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_failed_handoff_read_retries_on_the_backoff_with_one_warning() -> TestResult {
    let (warnings, _guard) = CapturedWarnings::install();
    let fixture = HandoffFixture::start().await?;
    settled(&fixture.observation).await?;
    let mut fault = fixture.fault_connection().await?;
    sqlx::query("ALTER TABLE owed_push_operations RENAME TO owed_push_operations_hidden")
        .execute(&mut fault)
        .await?;

    // Creating the wake-up commits and wakes the hand-off into the failing read; its
    // evaluation cannot record the owed admission and rolls back.
    let evaluation = fixture.owe_one_push().await;
    settled(&fixture.observation).await?;
    let first_retry = fixture.observation.snapshot().armed_deadline;
    assert!(
        fixture
            .clock
            .has_not_due_check_now(ClockInstant::ZERO + Duration::from_secs(1)),
        "handoff worker must register its injected retry timer"
    );
    advance(&fixture.clock, Duration::from_secs(1)).await;
    settled(&fixture.observation).await?;
    let second_retry = fixture.observation.snapshot().armed_deadline;
    sqlx::query("ALTER TABLE owed_push_operations_hidden RENAME TO owed_push_operations")
        .execute(&mut fault)
        .await?;
    advance(&fixture.clock, Duration::from_secs(2)).await;
    settled(&fixture.observation).await?;
    let handoff_warnings = warnings
        .events()
        .iter()
        .filter(|warning| warning.worker == "push hand-off")
        .count();

    assert!(
        evaluation.is_err(),
        "the hidden table refuses the owed admission"
    );
    assert_eq!(first_retry, Some(at(Duration::from_secs(1))));
    assert_eq!(second_retry, Some(at(Duration::from_secs(3))));
    assert_eq!(
        fixture.observation.snapshot().armed_deadline,
        None,
        "recovered, and nothing more is owed"
    );
    assert_eq!(handoff_warnings, 1, "one warning for the run of failures");
    fixture.stop().await;
    Ok(())
}
