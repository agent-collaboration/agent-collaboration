//! A retryable refusal keeps its window slot (owed-work design, "How an answer
//! finds its row": Retry stays in flight on the 30 s backoff).
use super::composed_fleet_fixture::{B, Fixture, NOW, SERVICE_B, TestResult};
use super::fleet_mail_dispatch::{Socket, custody, envelope, submit};
use durable_steps::{AtomicStore, CallId, StoreError, storage::TransactionScope};
use fleet_protocol::{
    DeliveryRefusal, FrameId, ItemId, PushResult, ServerCall, ServerFrame, ServerRequest,
};
use runtime_primitives::Clock;
use sqlx_core::{query::query, row::Row};

/// The next push frame on `socket`: its frame id, call identity and item.
async fn next_push(socket: &mut Socket) -> TestResult<(FrameId, CallId, ItemId)> {
    loop {
        if let ServerFrame::Request {
            frame,
            body: ServerRequest::Crate(ServerCall::Push { call, item, .. }),
        } = socket.next().await?
        {
            return Ok((frame, call, item.id()));
        }
    }
}

/// Push rows in flight on the recipient's connection: the window it holds.
async fn in_flight(fixture: &Fixture, socket: &Socket) -> TestResult<i64> {
    let connection = socket.binding.connection.as_uuid().to_string();
    Ok(fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                query::<sqlx_turso::Turso>(
                    "SELECT count(*) AS held FROM central_work_attempts WHERE state='in_flight' AND kind='push' AND connection_id=?1",
                )
                .bind(connection)
                .fetch_one(scope.executor())
                .await
                .map_err(StoreError::backend)?
                .try_get("held")
                .map_err(StoreError::backend)
            })
        })
        .await?)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Each assertion names the window state the Router's own ledger would see."
)]
#[tokio::test]
async fn a_refused_push_keeps_its_window_slot_until_its_resend_is_answered() -> TestResult {
    // Arrange: 33 pushes owed to B; its window takes the first 32.
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    let mut recipient = Socket::open(&fixture, B, SERVICE_B).await?;
    let mut items = Vec::new();
    for _ in 0..33 {
        let item = envelope(B, SERVICE_B)?;
        submit(&fixture, &item).await?;
        items.push(item.id());
    }
    let mut sent = Vec::new();
    for _ in 0..32 {
        sent.push(next_push(&mut recipient).await?);
    }
    assert!(recipient.frames_is_empty(), "the window sent a 33rd push");
    assert_eq!(in_flight(&fixture, &recipient).await?, 32);
    let (refused_frame, refused_call, refused_item) = sent[0].clone();

    // Act 1: B's mailbox is full for the first push.
    recipient
        .answer(
            &fixture,
            refused_frame,
            PushResult::Refused {
                reason: DeliveryRefusal::MailboxFull,
            },
        )
        .await?;

    // Assert: the refused push still holds its slot, so no 33rd is sent.
    assert!(
        recipient.frames_is_empty(),
        "a refusal freed a window slot central's peer still counts"
    );
    assert_eq!(in_flight(&fixture, &recipient).await?, 32);

    // Act 2: past the backoff the refused push is resent under its own call.
    // Every push went out at once, so the other 31 are due too and resent as
    // unanswered; none of the frames is the 33rd item.
    fixture.io.advance(31_000);
    crate::kernel_alarm::deliver_alarm(&fixture.runtime).await?;
    let mut resent = Vec::new();
    while recipient.queued_commands() > 0 {
        resent.push(next_push(&mut recipient).await?);
    }
    assert!(
        resent.iter().all(|(_, _, item)| Some(item) != items.last()),
        "a resend came with the 33rd push"
    );
    let (resent_frame, resent_call, _) = resent
        .iter()
        .find(|(_, _, item)| *item == refused_item)
        .cloned()
        .ok_or("the refused push was not resent after its backoff")?;
    assert_eq!(resent_call, refused_call);
    assert_ne!(resent_frame, refused_frame, "a resend mints a new send id");
    assert_eq!(in_flight(&fixture, &recipient).await?, 32);

    // A late answer to the old send is stale: it changes nothing.
    recipient
        .answer(
            &fixture,
            refused_frame,
            PushResult::Accepted {
                accepted_at: fixture.io.now(),
            },
        )
        .await?;
    assert_eq!(
        custody(&fixture, refused_item).await?,
        ("sent".into(), true)
    );
    assert!(recipient.frames_is_empty());

    // Act 3: the resend is accepted; its slot frees and the 33rd goes out.
    recipient
        .answer(
            &fixture,
            resent_frame,
            PushResult::Accepted {
                accepted_at: fixture.io.now(),
            },
        )
        .await?;
    assert_eq!(
        custody(&fixture, refused_item).await?,
        ("atRecipient".into(), false)
    );
    let (_, _, last) = next_push(&mut recipient).await?;
    assert_eq!(Some(&last), items.last());
    assert_eq!(in_flight(&fixture, &recipient).await?, 32);
    fixture.close().await
}

/// One push row's attempt state: (state, send id present, attempts, next retry).
async fn push_attempt(
    fixture: &Fixture,
    item: ItemId,
) -> TestResult<(String, bool, i64, Option<i64>)> {
    let subject = item.as_uuid().to_string();
    Ok(fixture
        .store
        .transaction(move |scope| {
            Box::pin(async move {
                let row = query::<sqlx_turso::Turso>(
                    "SELECT state, send_id IS NOT NULL AS sent, attempts, next_attempt_at FROM central_work_attempts WHERE subject=?1 AND kind='push'",
                )
                .bind(subject)
                .fetch_one(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                Ok((
                    row.try_get::<String, _>("state").map_err(StoreError::backend)?,
                    row.try_get::<i64, _>("sent").map_err(StoreError::backend)? == 1,
                    row.try_get::<i64, _>("attempts").map_err(StoreError::backend)?,
                    row.try_get::<Option<i64>, _>("next_attempt_at")
                        .map_err(StoreError::backend)?,
                ))
            })
        })
        .await?)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Each assertion names one step of the counting rule."
)]
#[tokio::test]
async fn a_refusal_followed_by_silence_keeps_doubling_the_backoff() -> TestResult {
    // Arrange: one push to B, refused for now.
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path(), NOW).await?;
    let mut recipient = Socket::open(&fixture, B, SERVICE_B).await?;
    let item = envelope(B, SERVICE_B)?;
    submit(&fixture, &item).await?;
    let (refused_frame, call, _) = next_push(&mut recipient).await?;
    recipient
        .answer(
            &fixture,
            refused_frame,
            PushResult::Refused {
                reason: DeliveryRefusal::MailboxFull,
            },
        )
        .await?;

    // The refusal is counted once, at its answer: in flight with no send,
    // retried backoff(1) = 30 s later.
    let refused_at = fixture.io.now().unix_millis();
    let (state, sent, attempts, next) = push_attempt(&fixture, item.id()).await?;
    assert_eq!((state.as_str(), sent, attempts), ("in_flight", false, 1));
    assert_eq!(next, Some(refused_at + 30_000));
    assert_eq!(
        in_flight(&fixture, &recipient).await?,
        1,
        "the refusal freed its slot"
    );

    // Act and assert: each silence is counted at its retry time; after the
    // k-th failure the next retry is backoff(k) later: 30, then 60, then 120 s.
    let mut previous = refused_frame;
    for (expected_attempts, wait) in [(1_i64, 30_000_i64), (2, 60_000), (3, 120_000)] {
        let due = push_attempt(&fixture, item.id())
            .await?
            .3
            .ok_or("no retry time")?;
        fixture.io.advance(due - fixture.io.now().unix_millis() + 1);
        crate::kernel_alarm::deliver_alarm(&fixture.runtime).await?;
        let (frame, resent_call, resent_item) = next_push(&mut recipient).await?;
        assert_eq!(resent_item, item.id());
        assert_eq!(resent_call, call, "a resend keeps its call identity");
        assert_ne!(frame, previous, "a resend mints a new send id");
        let resent_at = fixture.io.now().unix_millis();
        let (state, sent, attempts, next) = push_attempt(&fixture, item.id()).await?;
        assert_eq!((state.as_str(), sent), ("in_flight", true));
        assert_eq!(
            attempts, expected_attempts,
            "a failure was not counted once"
        );
        assert_eq!(next, Some(resent_at + wait), "the backoff did not double");
        assert_eq!(in_flight(&fixture, &recipient).await?, 1);
        // A late answer to the previous send is stale: nothing changes.
        recipient
            .answer(
                &fixture,
                previous,
                PushResult::Accepted {
                    accepted_at: fixture.io.now(),
                },
            )
            .await?;
        assert_eq!(custody(&fixture, item.id()).await?, ("sent".into(), true));
        previous = frame;
    }
    fixture.close().await
}
