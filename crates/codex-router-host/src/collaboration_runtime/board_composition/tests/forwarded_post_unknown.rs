//! Uncertain holder completion keeps the original request as the retry identity.
use super::*;
use codex_router_test_support::FixtureResult;
#[path = "forwarded_post_deadline.rs"]
mod forwarded_post_deadline;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn non_holder_sender_unknown_answer_retries_the_original_post_identity() -> TestResult {
    let holder = Box::pin(claim_fixture::ClaimFixture::start()).await?;
    let project = claimed_project(&holder).await?;
    let sender = Box::pin(SenderFixture::start(&holder)).await?;
    let proof = Box::pin(async {
        let create = post_request(&project)?;
        grant_posting_role(&holder, &create).await?;
        let root = sender
            .application
            .board()
            .project_write(create.clone())
            .await?;
        assert!(matches!(
            root.result,
            message_board::public_replies::RequestResult::Decided(
                message_board::public_replies::RequestDecision::Accepted { .. }
            )
        ));
        let BoardOperation::ThreadCreate { post_id, .. } = &create.operation else {
            return Err("root fixture operation".into());
        };
        let reply_id = PostId::generate();
        let request = ProjectRequest {
            request_id: RequestId::generate(),
            operation: BoardOperation::ThreadPost {
                post_id: reply_id.clone(),
                thread_id: post_id.clone().into(),
                text: PostText::try_from("synthetic unanswered reply".to_owned())?,
                references: Vec::new(),
                addressees: Vec::new(),
            },
            ..create
        };
        let store = holder
            .registry
            .existing_project(&project)
            .await?
            .ok_or("holder store")?;
        let read = store.snapshot().await?;
        let before = read.high_water();
        read.finish().await?;
        let (subscriber, mut received) =
            forwarded_post_deadline::observe_request(&request.request_id);
        let _subscriber = tracing::subscriber::set_default(subscriber);
        // Hold the same native write gate and drive the existing caller clock
        // only after the holder receives this exact forwarded request.
        let held = store.begin_admission(origin()?).await?;
        let outcome = async {
            let board = sender.application.board();
            let submitting = board.project_write(request.clone());
            tokio::pin!(submitting);
            timeout(std::time::Duration::from_secs(35), async {
                tokio::select! {
                    result = &mut submitting => {
                        result?;
                        FixtureResult::Err("forward completed before its real holder admission".into())
                    }
                    observed = received.wait_for(|received| *received) => {
                        observed?;
                        Ok(())
                    }
                }
            })
            .await??;
            let pending_before_expiry = futures_util::poll!(submitting.as_mut()).is_pending();
            tokio::time::pause();
            tokio::time::advance(std::time::Duration::from_millis(30_001)).await;
            tokio::time::resume();
            let outcome = timeout(std::time::Duration::from_secs(35), submitting).await??;
            if !pending_before_expiry {
                return Err("forward did not remain pending before controlled expiry".into());
            }
            FixtureResult::Ok(outcome)
        }
        .await;
        let released = held.rollback().await;
        let outcome = outcome?;
        released?;
        assert!(
            matches!(
                outcome.result,
                message_board::public_replies::RequestResult::Unanswered
            ),
            "a lost answer must not imply a refusal or acceptance"
        );
        let retried = timeout(BOUND, sender.application.board().project_write(request)).await??;
        assert!(matches!(
            retried.result,
            message_board::public_replies::RequestResult::Decided(
                message_board::public_replies::RequestDecision::Accepted { .. }
            )
        ));
        let mut read = store.snapshot().await?;
        let record = read
            .record(&reply_id.clone().into())
            .await?
            .ok_or("retried post record")?;
        assert_eq!(record.envelope().position.origin(), Some(&sender.origin));
        let FeedRead::Entries { entries, .. } = read.feed_after(before, 100, false).await? else {
            return Err("current-line reply feed".into());
        };
        assert_eq!(
            entries
                .iter()
                .filter(|entry| entry.record == reply_id.clone().into())
                .count(),
            1,
            "unknown-answer retry repeated the post effect"
        );
        read.finish().await?;
        #[cfg(feature = "runtime-test-support")]
        assert!(
            sender
                .admission
                .runtime_test_context()
                .fleet
                .pending_requests()
                .await?
                .is_empty()
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    })
    .await;
    let sender_closed = sender.shutdown().await;
    let holder_closed = holder.shutdown().await;
    proof?;
    sender_closed?;
    holder_closed?;
    Ok(())
}
