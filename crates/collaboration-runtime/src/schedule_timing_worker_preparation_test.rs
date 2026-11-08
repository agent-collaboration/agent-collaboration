//! A Run whose preparation cannot reach its client writes nothing, so no commit would
//! step it again: the scheduler retries it on the failed-step backoff.
use super::{INTERVAL_SECONDS, RunPhase, SchedulerFixture, TestResult, advance, at, settled};
use std::{sync::atomic::Ordering, time::Duration};

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
