//! Scheduler deadlines and failed passes, driven by the injected manual clock.
use super::*;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions report violated deadline contracts."
)]
async fn a_blocked_run_step_times_out_on_the_injected_clock_and_retries() -> TestResult<()> {
    use router_clock::ClockInstant;

    let fixture = SchedulerFixture::with_one_schedule().await?;
    let scheduler = fixture.start()?;
    settled(&scheduler).await?;
    let interval = Duration::from_secs(u64::from(INTERVAL_SECONDS));
    advance(&fixture.clock, interval).await;
    let run = fixture.executing_and_asleep(&scheduler).await?;
    *fixture.route.held_settlement.lock().unwrap() = Some(Arc::new(tokio::sync::Notify::new()));
    let prior_reads = fixture.route.settlement_reads.load(Ordering::Acquire);
    observe(
        &fixture,
        json!({"kind":"turnTerminal","turnId":"fixture-turn","status":"completed"}),
    )
    .await?;
    wait_until("the running step reached its held observation", || {
        fixture.route.settlement_reads.load(Ordering::Acquire) > prior_reads
    })
    .await?;
    let deadline = ClockInstant::ZERO.after(interval + Duration::from_secs(35));
    assert!(
        fixture.clock.has_not_due_check_now(deadline),
        "step must register its injected execution bound"
    );

    advance(&fixture.clock, Duration::from_secs(34)).await;
    fixture.clock.not_due_check_now(deadline).await;
    assert_eq!(fixture.read_phase(&run).await?, RunPhase::Executing);
    advance(&fixture.clock, Duration::from_secs(1)).await;
    settled(&scheduler).await?;

    assert_eq!(
        scheduler.observation.snapshot().armed_deadline,
        Some(at(interval + Duration::from_secs(36)))
    );
    assert_eq!(fixture.read_phase(&run).await?, RunPhase::Executing);
    scheduler.stop().await;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_failed_step_retries_on_the_backoff_and_recovers_without_a_new_write() -> TestResult<()> {
    let fixture = SchedulerFixture::with_one_schedule().await?;
    fixture
        .route
        .initial_evidence_fails
        .store(true, Ordering::Release);
    let scheduler = fixture.start()?;
    settled(&scheduler).await?;
    let interval = Duration::from_secs(u64::from(INTERVAL_SECONDS));

    advance(&fixture.clock, interval).await;
    settled(&scheduler).await?;
    let first_retry = scheduler.observation.snapshot().armed_deadline;
    advance(&fixture.clock, Duration::from_secs(1)).await;
    settled(&scheduler).await?;
    let second_retry = scheduler.observation.snapshot().armed_deadline;
    fixture
        .route
        .initial_evidence_fails
        .store(false, Ordering::Release);
    advance(&fixture.clock, Duration::from_secs(2)).await;
    let run = fixture.executing_and_asleep(&scheduler).await?;

    assert_eq!(first_retry, Some(at(interval + Duration::from_secs(1))));
    assert_eq!(second_retry, Some(at(interval + Duration::from_secs(3))));
    assert_eq!(
        fixture.read_phase(&run).await?,
        RunPhase::Executing,
        "the retry recovered"
    );
    assert_eq!(
        scheduler.observation.snapshot().armed_deadline,
        Some(at(
            interval + Duration::from_secs(3 + u64::from(TIMEOUT_SECONDS))
        )),
        "a good pass clears the retry; the run deadline arms next"
    );
    scheduler.stop().await;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_failed_read_retries_on_the_backoff_and_recovers_when_the_fault_is_fixed()
-> TestResult<()> {
    let fixture = SchedulerFixture::with_one_schedule().await?;
    let mut fault = <sqlx::SqliteConnection as sqlx::Connection>::connect(&format!(
        "sqlite://{}",
        fixture.root.path().join("automation.sqlite").display()
    ))
    .await?;
    sqlx::query("ALTER TABLE schedule_timing_state RENAME TO schedule_timing_state_hidden")
        .execute(&mut fault)
        .await?;
    let scheduler = fixture.start()?;
    settled(&scheduler).await?;
    let first_retry = scheduler.observation.snapshot().armed_deadline;
    advance(&fixture.clock, Duration::from_secs(1)).await;
    settled(&scheduler).await?;
    let second_retry = scheduler.observation.snapshot().armed_deadline;
    sqlx::query("ALTER TABLE schedule_timing_state_hidden RENAME TO schedule_timing_state")
        .execute(&mut fault)
        .await?;
    advance(&fixture.clock, Duration::from_secs(2)).await;
    settled(&scheduler).await?;

    assert_eq!(first_retry, Some(at(Duration::from_secs(1))));
    assert_eq!(second_retry, Some(at(Duration::from_secs(3))));
    assert_eq!(
        scheduler.observation.snapshot().armed_deadline,
        Some(at(Duration::from_secs(u64::from(INTERVAL_SECONDS)))),
        "the recovered pass arms the schedule again"
    );
    scheduler.stop().await;
    Ok(())
}

/// Appends a native observation for the fixture thread, as the observer would.

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn an_empty_scheduler_holds_no_timer_and_reads_nothing_for_an_hour() -> TestResult<()> {
    let fixture = SchedulerFixture::empty().await?;
    let scheduler = fixture.start()?;
    settled(&scheduler).await?;
    let asleep = scheduler.observation.snapshot();

    advance(&fixture.clock, Duration::from_secs(3_600)).await;

    assert_eq!(asleep.passes, 1, "only the start-up pass ran");
    assert_eq!(
        asleep.reads,
        [
            "due schedules",
            "waiting schedules",
            "observable runs",
            "scheduler next due"
        ]
    );
    assert_eq!(asleep.armed_deadline, None, "nothing due, so no timer");
    assert_eq!(
        scheduler.observation.snapshot(),
        asleep,
        "an idle hour reads nothing"
    );
    scheduler.stop().await;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_run_deadline_stores_the_stop_at_its_time_and_does_not_act_again() -> TestResult<()> {
    let fixture = SchedulerFixture::with_one_schedule().await?;
    fixture
        .route
        .stop_records_intent
        .store(true, Ordering::Release);
    let scheduler = fixture.start()?;
    settled(&scheduler).await?;
    let interval = Duration::from_secs(u64::from(INTERVAL_SECONDS));
    let deadline = interval + Duration::from_secs(u64::from(TIMEOUT_SECONDS));

    advance(&fixture.clock, interval).await;
    let run = fixture.executing_and_asleep(&scheduler).await?;
    advance(
        &fixture.clock,
        Duration::from_secs(u64::from(TIMEOUT_SECONDS) - 1),
    )
    .await;
    settled(&scheduler).await?;
    let phase_before_deadline = fixture.read_phase(&run).await?;
    advance(&fixture.clock, Duration::from_secs(1)).await;
    settled(&scheduler).await?;
    let phase_at_deadline = fixture.read_phase(&run).await?;
    advance(&fixture.clock, Duration::from_secs(600)).await;
    settled(&scheduler).await?;

    assert_eq!(phase_before_deadline, RunPhase::Executing);
    assert_eq!(
        phase_at_deadline,
        RunPhase::Stopping,
        "the stop intent is stored at the deadline"
    );
    assert_eq!(
        *fixture.route.stop_requests.lock().unwrap(),
        [at(deadline)],
        "one stop, at its time"
    );
    assert_eq!(fixture.read_phase(&run).await?, RunPhase::Stopping);
    scheduler.stop().await;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn an_unreachable_client_is_retried_on_the_backoff_and_the_run_settles() -> TestResult<()> {
    let fixture = SchedulerFixture::with_one_schedule().await?;
    let scheduler = fixture.start()?;
    settled(&scheduler).await?;
    advance(
        &fixture.clock,
        Duration::from_secs(u64::from(INTERVAL_SECONDS)),
    )
    .await;
    let run = fixture.executing_and_asleep(&scheduler).await?;
    let completed_at = fixture.clock.now();

    // The turn completes, but the first two reads cannot reach the client.
    fixture.route.turn_completed.store(true, Ordering::Release);
    fixture
        .route
        .unreachable_settlements
        .store(2, Ordering::Release);
    observe(&fixture, turn_completed()).await?;
    settled(&scheduler).await?;
    let first_retry = scheduler.observation.snapshot().armed_deadline;
    advance(&fixture.clock, Duration::from_secs(1)).await;
    settled(&scheduler).await?;
    let second_retry = scheduler.observation.snapshot().armed_deadline;
    let phase_before_recovery = fixture.read_phase(&run).await?;
    advance(&fixture.clock, Duration::from_secs(2)).await;
    settled(&scheduler).await?;

    assert_eq!(
        first_retry,
        Some(completed_at.after(Duration::from_secs(1)))
    );
    assert_eq!(
        second_retry,
        Some(completed_at.after(Duration::from_secs(3)))
    );
    assert_eq!(phase_before_recovery, RunPhase::Executing);
    assert_eq!(
        fixture.read_phase(&run).await?,
        RunPhase::Finished,
        "the retry reached the client and settled the Run"
    );
    scheduler.stop().await;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_stop_that_cannot_reach_its_client_is_retried_after_the_deadline() -> TestResult<()> {
    let fixture = SchedulerFixture::with_one_schedule().await?;
    fixture
        .route
        .stop_records_intent
        .store(true, Ordering::Release);
    fixture.route.unreachable_stops.store(1, Ordering::Release);
    let scheduler = fixture.start()?;
    settled(&scheduler).await?;
    let interval = Duration::from_secs(u64::from(INTERVAL_SECONDS));
    let deadline = interval + Duration::from_secs(u64::from(TIMEOUT_SECONDS));

    advance(&fixture.clock, interval).await;
    let run = fixture.executing_and_asleep(&scheduler).await?;
    advance(
        &fixture.clock,
        Duration::from_secs(u64::from(TIMEOUT_SECONDS)),
    )
    .await;
    settled(&scheduler).await?;
    let retry = scheduler.observation.snapshot().armed_deadline;
    let phase_after_failed_stop = fixture.read_phase(&run).await?;
    advance(&fixture.clock, Duration::from_secs(1)).await;
    settled(&scheduler).await?;

    assert_eq!(retry, Some(at(deadline + Duration::from_secs(1))));
    assert_eq!(phase_after_failed_stop, RunPhase::Executing);
    assert_eq!(
        fixture.read_phase(&run).await?,
        RunPhase::Stopping,
        "the retried stop reached the client"
    );
    assert_eq!(
        *fixture.route.stop_requests.lock().unwrap(),
        [at(deadline), at(deadline + Duration::from_secs(1))]
    );
    scheduler.stop().await;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn each_run_of_failed_steps_logs_one_warning() -> TestResult<()> {
    let (warnings, _subscriber) =
        crate::captured_warnings_test_support::CapturedWarnings::install();
    let fixture = SchedulerFixture::with_one_schedule().await?;
    fixture
        .route
        .initial_evidence_fails
        .store(true, Ordering::Release);
    let scheduler = fixture.start()?;
    settled(&scheduler).await?;

    advance(
        &fixture.clock,
        Duration::from_secs(u64::from(INTERVAL_SECONDS)),
    )
    .await;
    settled(&scheduler).await?;
    advance(&fixture.clock, Duration::from_secs(1)).await;
    settled(&scheduler).await?;
    let warnings_in_first_run = step_warnings(&warnings);
    fixture
        .route
        .initial_evidence_fails
        .store(false, Ordering::Release);
    advance(&fixture.clock, Duration::from_secs(2)).await;
    fixture.executing_and_asleep(&scheduler).await?;
    let warnings_after_recovery = step_warnings(&warnings);
    fixture
        .route
        .unreachable_settlements
        .store(2, Ordering::Release);
    observe(&fixture, turn_completed()).await?;
    settled(&scheduler).await?;
    advance(&fixture.clock, Duration::from_secs(1)).await;
    settled(&scheduler).await?;
    advance(&fixture.clock, Duration::from_secs(2)).await;
    settled(&scheduler).await?;

    assert_eq!(warnings_in_first_run, 1, "two failures, one warning");
    assert_eq!(warnings_after_recovery, 1, "recovery logs nothing");
    assert_eq!(
        step_warnings(&warnings),
        2,
        "a second run of failures logs once more"
    );
    scheduler.stop().await;
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Test assertions must fail the test while fixture errors propagate."
)]
async fn a_step_that_reports_before_its_task_ends_still_arms_its_retry() -> TestResult<()> {
    let fixture = SchedulerFixture::with_one_schedule().await?;
    fixture
        .route
        .initial_evidence_fails
        .store(true, Ordering::Release);
    let gate = Arc::new(Mutex::new(()));
    let held = Arc::clone(&gate).lock_owned().await;
    let scheduler = fixture.start_holding_step_exits(gate)?;
    settled(&scheduler).await?;
    let interval = Duration::from_secs(u64::from(INTERVAL_SECONDS));

    // The failed step reports and wakes the loop, then its task stays unfinished while
    // the scheduler runs its pass and goes back to sleep.
    advance(&fixture.clock, interval).await;
    settled(&scheduler).await?;
    let armed_while_task_held = scheduler.observation.snapshot().armed_deadline;
    drop(held);
    fixture
        .route
        .initial_evidence_fails
        .store(false, Ordering::Release);
    advance(&fixture.clock, Duration::from_secs(1)).await;
    let run = fixture.executing_and_asleep(&scheduler).await?;

    assert_eq!(
        armed_while_task_held,
        Some(at(interval + Duration::from_secs(1))),
        "the retry is armed from the report, not from the task's completion"
    );
    assert_eq!(fixture.read_phase(&run).await?, RunPhase::Executing);
    scheduler.stop().await;
    Ok(())
}
