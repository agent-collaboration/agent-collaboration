//! A Run whose preparation cannot reach its client writes nothing, so no commit would
//! step it again: the scheduler retries it on the failed-step backoff. Each place a
//! preparation can fail to reach its client is covered: the route evidence, the target
//! and a fresh session's submission.
use super::{
    BASE_MS, INTERVAL_SECONDS, RunPhase, SchedulerFixture, TIMEOUT_SECONDS, TestResult, advance,
    at, settled,
};
use agent_automation::{
    ContinuityInput, ExecutionDestination, InstructionText, OperationId, ScheduleDefinition,
    TimingRule,
};
use automation_storage::ScheduleCreate;
use collaboration_contracts::{DeliveryContractError, DeliveryFuture, RunSubmission};
use collaboration_protocol::{EndpointRef, SessionRef};
use runtime_primitives::SyncTime;
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

/// Fresh-session submissions: the first `unreachable` cannot reach the client; later ones
/// are refused as busy, a written outcome.
#[derive(Default)]
pub(super) struct FreshSubmissions {
    pub(super) unreachable: AtomicUsize,
    pub(super) attempts: std::sync::Mutex<Vec<SyncTime>>,
}

impl FreshSubmissions {
    pub(super) fn submit(&self, now: SyncTime) -> DeliveryFuture<'_, RunSubmission> {
        self.attempts.lock().unwrap().push(now);
        if super::take_unreachable(&self.unreachable) {
            return Box::pin(async { Err(DeliveryContractError::ClientOperation) });
        }
        Box::pin(async {
            Ok(RunSubmission::Rejected(
                collaboration_protocol::DeliveryRejection {
                    reason: collaboration_protocol::DeliveryRejectionReason::Busy,
                    next_action: collaboration_protocol::DeliveryNextAction::RetryLater,
                    client_code: None,
                    detail: Some("fixture rejection".into()),
                    claims: None,
                    native_failure: None,
                },
            ))
        })
    }
}

impl SchedulerFixture {
    /// One enabled schedule that starts a fresh session each Run, first due at base + one
    /// interval.
    async fn create_fresh_session_schedule(&self) -> TestResult<()> {
        let instruction = self
            .store
            .lock()
            .await
            .create_instruction(
                &OperationId::generate(),
                &InstructionText::try_from("Check job".to_owned())?,
                BASE_MS,
            )
            .await?;
        self.store
            .lock()
            .await
            .create_schedule(&ScheduleCreate::<SessionRef, EndpointRef> {
                operation_id: OperationId::generate(),
                definition: ScheduleDefinition {
                    instruction_id: instruction.instruction_id,
                    timing: TimingRule::Interval {
                        seconds: INTERVAL_SECONDS,
                    },
                    enabled: true,
                    destination: ExecutionDestination::FreshEachRun {
                        endpoint: self.route.inner.target.endpoint.clone(),
                        cwd: self.root.path().to_string_lossy().into_owned(),
                    },
                    execution_timeout_seconds: Some(TIMEOUT_SECONDS),
                    model: Some("fixture-model".into()),
                    effort: Some("medium".into()),
                },
                imported_continuity: ContinuityInput::None,
                now_ms: BASE_MS,
            })
            .await?;
        Ok(())
    }
}

#[tokio::test(start_paused = true)]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_preparation_that_cannot_reach_its_client_is_retried_on_the_backoff() -> TestResult<()> {
    let fixture = SchedulerFixture::with_one_schedule().await?;
    fixture
        .route
        .initial_evidence_unreachable
        .store(true, Ordering::Release);
    let scheduler = fixture.start()?;
    settled(&scheduler).await?;
    let interval = Duration::from_secs(u64::from(INTERVAL_SECONDS));

    advance(interval).await;
    settled(&scheduler).await?;
    let armed = scheduler.observation.snapshot().armed_deadline;
    advance(Duration::from_secs(1)).await;
    settled(&scheduler).await?;
    fixture
        .route
        .initial_evidence_unreachable
        .store(false, Ordering::Release);
    advance(Duration::from_secs(2)).await;
    let run = fixture.executing_and_asleep(&scheduler).await?;

    assert_eq!(armed, Some(at(interval + Duration::from_secs(1))));
    assert_eq!(
        fixture.route.preparations.lock().unwrap()[..3],
        [
            at(interval),
            at(interval + Duration::from_secs(1)),
            at(interval + Duration::from_secs(3)),
        ],
        "retried at 1 s, then 2 s later, when the client is reachable again"
    );
    assert_eq!(fixture.read_phase(&run).await?, RunPhase::Executing);
    scheduler.stop().await;
    Ok(())
}

#[tokio::test(start_paused = true)]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_target_preparation_that_cannot_reach_its_client_is_retried_on_the_backoff()
-> TestResult<()> {
    let fixture = SchedulerFixture::with_one_schedule().await?;
    fixture
        .route
        .unreachable_targets
        .store(1, Ordering::Release);
    let scheduler = fixture.start()?;
    settled(&scheduler).await?;
    let interval = Duration::from_secs(u64::from(INTERVAL_SECONDS));

    advance(interval).await;
    settled(&scheduler).await?;
    let armed = scheduler.observation.snapshot().armed_deadline;
    let (_, phase_after_failure) = fixture.run_phase().await?.ok_or("the Run was admitted")?;
    advance(Duration::from_secs(1)).await;
    let run = fixture.executing_and_asleep(&scheduler).await?;

    assert_eq!(phase_after_failure, RunPhase::Preparing);
    assert_eq!(armed, Some(at(interval + Duration::from_secs(1))));
    assert_eq!(
        fixture.route.preparations.lock().unwrap()[..2],
        [at(interval), at(interval + Duration::from_secs(1))],
        "the target is prepared again at the retry"
    );
    assert_eq!(fixture.read_phase(&run).await?, RunPhase::Executing);
    scheduler.stop().await;
    Ok(())
}

#[tokio::test(start_paused = true)]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_fresh_session_submission_that_cannot_reach_its_client_is_retried_on_the_backoff()
-> TestResult<()> {
    let fixture = SchedulerFixture::empty().await?;
    fixture.create_fresh_session_schedule().await?;
    fixture.route.fresh.unreachable.store(1, Ordering::Release);
    let scheduler = fixture.start()?;
    settled(&scheduler).await?;
    let interval = Duration::from_secs(u64::from(INTERVAL_SECONDS));

    advance(interval).await;
    settled(&scheduler).await?;
    let armed = scheduler.observation.snapshot().armed_deadline;
    let (run, phase_after_failure) = fixture.run_phase().await?.ok_or("the Run was admitted")?;
    advance(Duration::from_secs(1)).await;
    settled(&scheduler).await?;
    let phase_after_retry = fixture.read_phase(&run).await?;

    assert_eq!(phase_after_failure, RunPhase::Preparing);
    assert_eq!(armed, Some(at(interval + Duration::from_secs(1))));
    assert_eq!(
        *fixture.route.fresh.attempts.lock().unwrap(),
        [at(interval), at(interval + Duration::from_secs(1))],
        "the fresh session's run is submitted again at the retry, and only then"
    );
    assert_eq!(
        phase_after_retry,
        RunPhase::Preparing,
        "the retried submission's busy refusal leaves the Run preparing, as any busy answer does"
    );
    scheduler.stop().await;
    Ok(())
}
