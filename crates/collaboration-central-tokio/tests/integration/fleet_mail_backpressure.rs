//! The configured Push deadline includes the actual native socket queue.
use super::{composed_fleet_fixture::*, fleet_mail_dispatch::*};
use durable_steps::{
    AtomicStore, StoreError,
    storage::{JournalStorage, StepState},
};
use fleet_protocol::PushResult;
use runtime_primitives::Clock;

#[expect(
    clippy::panic_in_result_fn,
    reason = "An unrelated recipient must commit while the full queue retries at the injected call deadline."
)]
#[tokio::test]
async fn full_recipient_send_queue_cannot_outlive_the_push_deadline_or_block_another_recipient()
-> TestResult {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    let blocked = Socket::open(&fixture, B, SERVICE_B).await?;
    blocked.fill_send_queue(&fixture).await?;
    let mut responsive = Socket::open(&fixture, C, SERVICE_C).await?;
    let first = envelope(B, SERVICE_B)?;
    let second = envelope(C, SERVICE_C)?;
    submit(&fixture, &first).await?;
    submit(&fixture, &second).await?;
    let (drive, ()) = Box::pin(tokio::time::timeout(CAP, async {
        tokio::try_join!(fixture.runtime.on_alarm(), async {
            let frame = responsive
                .push(second.id())
                .await
                .map_err(StoreError::backend)?;
            responsive
                .answer(
                    &fixture,
                    frame,
                    PushResult::Accepted {
                        accepted_at: fixture.io.now(),
                    },
                )
                .await
                .map_err(StoreError::backend)?;
            tokio::time::timeout(CAP, async {
                loop {
                    if custody(&fixture, second.id())
                        .await
                        .map_err(StoreError::backend)?
                        == ("atRecipient".into(), false)
                    {
                        return Ok::<_, StoreError>(());
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .map_err(StoreError::backend)??;
            assert_eq!(
                custody(&fixture, first.id())
                    .await
                    .map_err(StoreError::backend)?,
                ("sent".into(), true)
            );
            let mut deadlines = fixture.io.reply_deadlines.subscribe();
            tokio::time::timeout(CAP, deadlines.wait_for(|count| *count >= 1))
                .await
                .map_err(StoreError::backend)?
                .map_err(StoreError::backend)?;
            fixture.io.advance(2_000);
            Ok::<_, StoreError>(())
        })
    }))
    .await??;
    assert!(drive.failed.is_empty(), "{drive:?}");
    assert_eq!(drive.retried.len(), 1, "{drive:?}");
    assert_eq!(custody(&fixture, first.id()).await?, ("sent".into(), true));
    let operation = drive.retried[0];
    let step = fixture
        .store
        .transaction(move |scope| Box::pin(async move { scope.step(operation, 0).await }))
        .await?
        .ok_or("Push step absent")?;
    assert!(
        matches!(step.state,StepState::CallPending {attempt,..} if attempt.retry_reason.as_ref().is_some_and(|reason|reason.code=="peerSendDeadline"))
    );
    fixture.close().await
}
