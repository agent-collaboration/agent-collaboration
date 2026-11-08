//! Orderings a multi-threaded runtime allows between a step's own commit, its report and
//! the scheduler's wake, a completion that arrives while its Run is being checked, and
//! stop requests whose outcome is unknown.
use super::{
    CollectionLimit, INTERVAL_SECONDS, RunPhase, SchedulerFixture, TIMEOUT_SECONDS, TestResult,
    advance, at, observe, settled, settled_holding, turn_completed, wait_until,
};
use runtime_primitives::Clock;
use std::{
    sync::{Arc, atomic::Ordering},
    time::Duration,
};
use tokio::sync::Mutex;

#[tokio::test(start_paused = true)]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_step_whose_commit_woke_the_loop_before_its_report_still_continues() -> TestResult<()> {
    let fixture = SchedulerFixture::with_one_schedule().await?;
    let gate = Arc::new(Mutex::new(()));
    let held = Arc::clone(&gate).lock_owned().await;
    let scheduler = fixture.start_with(|worker| worker.hold_step_reports(gate))?;
    settled(&scheduler).await?;
    let interval = Duration::from_secs(u64::from(INTERVAL_SECONDS));

    // The preparation step records its route, a commit, and is held before it reports;
    // the commit wakes a pass that finds the Run still active and goes back to sleep.
    advance(interval).await;
    settled_holding(&scheduler.observation).await?;
    let (_, phase_while_held) = fixture.run_phase().await?.ok_or("the Run was admitted")?;
    drop(held);
    let run = fixture.executing_and_asleep(&scheduler).await?;

    assert_eq!(phase_while_held, RunPhase::Preparing);
    assert_eq!(
        fixture.read_phase(&run).await?,
        RunPhase::Executing,
        "the progress report woke the loop for the Run's next step"
    );
    scheduler.stop().await;
    Ok(())
}

#[tokio::test(start_paused = true)]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_stop_with_an_unknown_outcome_before_its_intent_is_retried() -> TestResult<()> {
    let fixture = SchedulerFixture::with_one_schedule().await?;
    fixture
        .route
        .stop_records_intent
        .store(true, Ordering::Release);
    fixture
        .route
        .unknown_stops_before_intent
        .store(1, Ordering::Release);
    let scheduler = fixture.start()?;
    settled(&scheduler).await?;
    let interval = Duration::from_secs(u64::from(INTERVAL_SECONDS));
    let deadline = interval + Duration::from_secs(u64::from(TIMEOUT_SECONDS));

    advance(interval).await;
    let run = fixture.executing_and_asleep(&scheduler).await?;
    advance(Duration::from_secs(u64::from(TIMEOUT_SECONDS))).await;
    settled(&scheduler).await?;
    let phase_after_unknown = fixture.read_phase(&run).await?;
    advance(Duration::from_secs(1)).await;
    settled(&scheduler).await?;

    assert_eq!(
        phase_after_unknown,
        RunPhase::Executing,
        "no intent was stored"
    );
    assert_eq!(fixture.read_phase(&run).await?, RunPhase::Stopping);
    assert_eq!(
        *fixture.route.stop_requests.lock().unwrap(),
        [at(deadline), at(deadline + Duration::from_secs(1))],
        "the stop is requested again at the retry time"
    );
    scheduler.stop().await;
    Ok(())
}

#[tokio::test(start_paused = true)]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_stop_with_an_unknown_outcome_after_its_intent_sends_one_interrupt_and_settles()
-> TestResult<()> {
    let fixture = SchedulerFixture::with_one_schedule().await?;
    fixture
        .route
        .unknown_stops_after_intent
        .store(1, Ordering::Release);
    let scheduler = fixture.start()?;
    settled(&scheduler).await?;
    let interval = Duration::from_secs(u64::from(INTERVAL_SECONDS));
    let deadline = interval + Duration::from_secs(u64::from(TIMEOUT_SECONDS));

    advance(interval).await;
    let run = fixture.executing_and_asleep(&scheduler).await?;
    advance(Duration::from_secs(u64::from(TIMEOUT_SECONDS))).await;
    settled(&scheduler).await?;
    let phase_after_unknown = fixture.read_phase(&run).await?;
    advance(Duration::from_secs(1)).await;
    settled(&scheduler).await?;
    let reads_before_completion = fixture.route.settlement_reads.load(Ordering::Acquire);
    fixture.route.turn_completed.store(true, Ordering::Release);
    observe(&fixture, turn_completed()).await?;
    settled(&scheduler).await?;

    assert_eq!(
        phase_after_unknown,
        RunPhase::Stopping,
        "the intent was stored"
    );
    assert_eq!(
        *fixture.route.stop_requests.lock().unwrap(),
        [at(deadline)],
        "one interrupt; the retry only reads the settlement"
    );
    assert!(reads_before_completion > 0);
    assert_ne!(
        fixture.read_phase(&run).await?,
        RunPhase::Stopping,
        "the settlement read finishes the stopping Run"
    );
    scheduler.stop().await;
    Ok(())
}

#[tokio::test(start_paused = true)]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_completion_consumed_while_its_run_is_being_checked_still_settles_the_run()
-> TestResult<()> {
    let fixture = SchedulerFixture::with_one_schedule().await?;
    let first = fixture.start()?;
    settled(&first).await?;
    advance(Duration::from_secs(u64::from(INTERVAL_SECONDS))).await;
    let run = fixture.executing_and_asleep(&first).await?;
    first.stop().await;
    let reads_before = fixture.route.settlement_reads.load(Ordering::Acquire);
    let gate = Arc::new(Mutex::new(()));
    let held = Arc::clone(&gate).lock_owned().await;

    // The restarted scheduler checks the Run, finds its turn running and is held before
    // it reports; the completion arrives and is consumed by a pass that skips the Run.
    let scheduler = fixture.start_with(|worker| worker.hold_step_reports(gate))?;
    wait_until("the restarted scheduler checked the Run", || {
        fixture.route.settlement_reads.load(Ordering::Acquire) > reads_before
    })
    .await?;
    settled_holding(&scheduler.observation).await?;
    let started_at = fixture.clock.now();
    fixture.route.turn_completed.store(true, Ordering::Release);
    let sleeps_before_completion = scheduler.observation.snapshot().sleeps;
    observe(&fixture, turn_completed()).await?;
    wait_until("the completion woke a pass", || {
        scheduler.observation.snapshot().sleeps > sleeps_before_completion
    })
    .await?;
    settled_holding(&scheduler.observation).await?;
    let phase_while_held = fixture.read_phase(&run).await?;
    drop(held);
    let limit = CollectionLimit::start("the completed Run settled");
    while fixture.read_phase(&run).await? == RunPhase::Executing {
        limit.next().await?;
    }

    assert_eq!(phase_while_held, RunPhase::Executing);
    assert_eq!(
        fixture.read_phase(&run).await?,
        RunPhase::Finished,
        "the held check reports, and the completion it missed is checked again"
    );
    assert_eq!(fixture.clock.now(), started_at, "with no clock advance");
    scheduler.stop().await;
    Ok(())
}
