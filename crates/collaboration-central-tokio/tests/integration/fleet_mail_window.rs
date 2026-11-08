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
