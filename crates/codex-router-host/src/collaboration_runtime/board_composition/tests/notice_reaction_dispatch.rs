//! The movable reaction owner uses real SDK notices and actual mail transactions.
use super::super::super::{finish_reaction, notice_dispatch::NoticeReactionQueue};
use super::*;
use durable_steps::{AtomicStore, CallFailure};
use fleet_sync_protocol::{OperationAnswer, OperationRequest};
use fleet_sync_tokio_runner::{CallEnvelope, CallRouter, TokioHost};
use runtime_primitives::SystemClock;

struct NoTransactionEffects;
impl CallRouter for NoTransactionEffects {
    async fn dispatch(&self, _: CallEnvelope) -> Result<serde_json::Value, CallFailure> {
        Err(CallFailure::AnswerLost)
    }
}

fn event_owners(harness: &MailHarness) -> MachineSessionEventOwners {
    MachineSessionEventOwners {
        #[cfg(feature = "runtime-test-support")]
        progress: Default::default(),
        projects: harness.projects.clone(),
        admission: harness.admission.clone(),
        fleet: harness.fleet.clone(),
        mail: Arc::downgrade(&harness.mail.mail),
        publisher: harness.publisher.clone(),
    }
}

async fn welcomed_manual() -> Result<(tempfile::TempDir, MailHarness), TestError> {
    let directory = tempfile::tempdir()?;
    let mut harness = MailHarness::open_manual(directory.path(), None).await?;
    harness.release_welcome()?;
    assert!(matches!(
        harness.next_manual_event().await?,
        ClientEvent::Welcome { .. }
    ));
    harness.welcomed.send_replace(true);
    Ok((directory, harness))
}

async fn actual_notice(
    harness: &mut MailHarness,
    position: u64,
    stream: DeliveryStream,
) -> Result<(NoticePosition, Notice<BoardConsumerProtocol>), TestError> {
    harness.transport.publish(position, stream).await?;
    let ClientEvent::Notice {
        position: actual,
        body,
    } = harness.next_manual_event().await?
    else {
        return Err("SDK did not yield the published notice".into());
    };
    assert_eq!(actual, NoticePosition::new(position));
    Ok((actual, body))
}

async fn completion(
    handlers: &mut JoinSet<SessionCompletion>,
) -> Result<super::super::super::notice_dispatch::ReactionCompletion, TestError> {
    match tokio::time::timeout(BOUND, handlers.join_next())
        .await?
        .ok_or("reaction missing")??
    {
        SessionCompletion::Reaction(completed) => Ok(completed),
        SessionCompletion::Operation(_) => Err("unexpected independent operation".into()),
    }
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn one_reaction_owner_defers_y_while_x_waits_on_the_actual_store_and_requests_continue()
-> Result<(), TestError> {
    let (_directory, mut harness) = welcomed_manual().await?;
    let owners = event_owners(&harness);
    let mut reactions =
        NoticeReactionQueue::new(MachineNoticeReactions::open(&harness.root).await?);
    let mut receipts = NoticeReceipts::default();
    let mut handlers = JoinSet::new();
    let (entered, observed) = tokio::sync::oneshot::channel();
    let (release, released) = tokio::sync::oneshot::channel();
    let owner = harness.mail.owner.clone();
    // Borrow the same physical owner's write lock. No alternate file or mocked
    // successful reaction can stand in for the blocked durable admission.
    let holding = tokio::spawn(async move {
        let store = interaction_storage::InteractionAtomicStore::new(
            owner,
            Arc::new(TokioHost::new(Arc::new(NoTransactionEffects), SystemClock)),
        );
        store
            .transaction(move |_| {
                Box::pin(async move {
                    entered
                        .send(())
                        .map_err(|_| durable_steps::StoreError::backend("store observer closed"))?;
                    released.await.map_err(durable_steps::StoreError::backend)?;
                    Ok(())
                })
            })
            .await
    });
    tokio::time::timeout(BOUND, observed).await??;
    let (first, first_body) = actual_notice(&mut harness, 1, DeliveryStream::Items).await?;
    queue_notice(&mut reactions, &mut receipts, 1, first, first_body);
    reactions.dispatch_next(&mut handlers, &owners, &harness.requests);
    let (later, later_body) = actual_notice(&mut harness, 2, DeliveryStream::Controls).await?;
    queue_notice(&mut reactions, &mut receipts, 1, later, later_body);
    reactions.dispatch_next(&mut handlers, &owners, &harness.requests);
    assert_eq!(
        handlers.len(),
        1,
        "Y started a second reaction owner while X was in flight"
    );
    assert_eq!(harness.ack().await?, NoticePosition::new(0));
    let answer = tokio::time::timeout(
        BOUND,
        harness.requests.request(OperationRequest::Consumer(
            board_sync_protocol::BoardConsumerRequest::Read {
                scope: board_sync_protocol::BoardReadScope::Notices,
                cursor: None,
            },
        )),
    )
    .await??;
    assert!(matches!(
        answer,
        OperationAnswer::Consumer(board_sync_protocol::BoardConsumerAnswer::Read(_))
    ));
    release.send(()).map_err(|_| "store hold ended early")?;
    tokio::time::timeout(BOUND, holding).await???;
    let first = completion(&mut handlers).await?;
    finish_reaction(
        first,
        1,
        &mut reactions,
        &mut receipts,
        &harness.requests,
        &harness.projects,
    )
    .await?;
    assert_eq!(harness.ack().await?, NoticePosition::new(1));
    reactions.dispatch_next(&mut handlers, &owners, &harness.requests);
    let later = completion(&mut handlers).await?;
    finish_reaction(
        later,
        1,
        &mut reactions,
        &mut receipts,
        &harness.requests,
        &harness.projects,
    )
    .await?;
    assert_eq!(harness.ack().await?, NoticePosition::new(2));
    let state = harness
        .mail
        .state()
        .await?
        .ok_or("actual receive obligations missing")?;
    assert_eq!(state["items"]["active"]["cycle"], 1);
    assert_eq!(state["controls"]["active"]["cycle"], 1);
    assert_eq!(
        board_fleet_sync::MachineNoticeProgressStore::open(&harness.root)
            .await?
            .load()
            .await?,
        NoticePosition::new(2)
    );
    println!("NOTICE_DISPATCH_CASE one_owner_x_then_y_real_store_hold_requests_continue");
    drop(owners);
    drop(reactions);
    harness.close().await
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn stale_generation_completion_returns_the_owner_without_acknowledging_new_receipts()
-> Result<(), TestError> {
    let (_directory, mut harness) = welcomed_manual().await?;
    let owners = event_owners(&harness);
    let mut reactions =
        NoticeReactionQueue::new(MachineNoticeReactions::open(&harness.root).await?);
    let mut receipts = NoticeReceipts::default();
    let mut handlers = JoinSet::new();
    let (position, body) = actual_notice(&mut harness, 1, DeliveryStream::Items).await?;
    let replayed = serde_json::from_slice(&serde_json::to_vec(&body)?)?;
    queue_notice(&mut reactions, &mut receipts, 1, position, body);
    reactions.dispatch_next(&mut handlers, &owners, &harness.requests);
    let completed = completion(&mut handlers).await?;
    let state = harness
        .mail
        .state()
        .await?
        .ok_or("actual receive admission missing")?;
    // Model the exact completion boundary after connection generation changes:
    // the new generation has received this coordinate, but has not handled it.
    receipts = NoticeReceipts::default();
    assert!(receipts.begin(position));
    finish_reaction(
        completed,
        2,
        &mut reactions,
        &mut receipts,
        &harness.requests,
        &harness.projects,
    )
    .await?;
    assert_eq!(
        harness.ack().await?,
        NoticePosition::new(0),
        "old completion acknowledged the new generation"
    );
    reactions.notice(2, position, replayed);
    reactions.dispatch_next(&mut handlers, &owners, &harness.requests);
    let completed = completion(&mut handlers).await?;
    finish_reaction(
        completed,
        2,
        &mut reactions,
        &mut receipts,
        &harness.requests,
        &harness.projects,
    )
    .await?;
    assert_eq!(harness.ack().await?, position);
    assert_eq!(
        harness.mail.state().await?,
        Some(state),
        "replayed inner notice repeated durable admission"
    );
    println!(
        "NOTICE_DISPATCH_CASE stale_completion_restores_owner_without_ack_new_generation_replay_idempotent"
    );
    drop(owners);
    drop(reactions);
    harness.close().await
}
