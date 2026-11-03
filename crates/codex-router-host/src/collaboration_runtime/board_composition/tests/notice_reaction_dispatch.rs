//! One movable reaction owner serializes real SDK notice catch-up while requests continue.
use super::super::super::{finish_reaction, notice_dispatch::NoticeReactionQueue};
use super::*;
use fleet_sync_protocol::{OperationAnswer, OperationRequest};

fn event_owners(harness: &MailHarness) -> MachineSessionEventOwners {
    MachineSessionEventOwners {
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
) -> Result<(NoticePosition, Notice<BoardConsumerProtocol>), TestError> {
    harness.transport.publish(position).await?;
    let ClientEvent::Notice {
        position: actual,
        body,
    } = harness.next_manual_event().await?
    else {
        return Err("SDK did not yield the published notice".into());
    };
    assert!(
        matches!(&body, Notice::Consumer(notice) if notice.position == NoticePosition::new(position))
    );
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
async fn one_reaction_owner_defers_y_while_x_waits_on_notice_catch_up_and_requests_continue()
-> Result<(), TestError> {
    let (_directory, mut harness) = welcomed_manual().await?;
    let owners = event_owners(&harness);
    let mut reactions =
        NoticeReactionQueue::new(MachineNoticeReactions::open(&harness.root).await?);
    let mut receipts = NoticeReceipts::default();
    let mut handlers = JoinSet::new();
    // Hold the real notice-read reply needed to fill the first notice's gap.
    // The pump must keep serving independent requests while one reaction owns catch-up.
    let (entered, release) = harness.transport.observer.hold_next_notice_read();
    harness
        .central
        .script_notice_answers(vec![board_sync_protocol::NoticeRead::Page {
            notices: vec![ledger::positioned_notice(1)?],
            as_of: chrono::Utc::now(),
        }])
        .await;
    let (first, first_body) = actual_notice(&mut harness, 2).await?;
    queue_notice(&mut reactions, &mut receipts, 1, first, first_body);
    reactions.dispatch_next(&mut handlers, &owners, &harness.requests);
    tokio::time::timeout(BOUND, entered).await??;
    let (later, later_body) = actual_notice(&mut harness, 3).await?;
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
    release
        .send(())
        .map_err(|_| "notice-read hold ended early")?;
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
    assert!(harness.mail.state().await?.is_none());
    assert_eq!(
        board_fleet_sync::MachineNoticeProgressStore::open(&harness.root)
            .await?
            .load()
            .await?,
        NoticePosition::new(3)
    );
    println!("NOTICE_DISPATCH_CASE one_owner_x_then_y_real_catch_up_hold_requests_continue");
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
    let (position, body) = actual_notice(&mut harness, 1).await?;
    let replayed = serde_json::from_slice(&serde_json::to_vec(&body)?)?;
    queue_notice(&mut reactions, &mut receipts, 1, position, body);
    reactions.dispatch_next(&mut handlers, &owners, &harness.requests);
    let completed = completion(&mut handlers).await?;
    let state = harness.mail.state().await?;
    assert!(state.is_none());
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
        state,
        "replayed legacy notice admitted obsolete Fetch work"
    );
    println!(
        "NOTICE_DISPATCH_CASE stale_completion_restores_owner_without_ack_new_generation_replay_idempotent"
    );
    drop(owners);
    drop(reactions);
    harness.close().await
}
