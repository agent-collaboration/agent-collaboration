//! Real takeover keeps precisely the confirmed membership-end consequences.
use super::*;
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerRequest, BoardDeliveryTypes, MailSubmitAnswer,
};
use fleet_sync_protocol::{
    DeliveryEnvelope, Epoch, OperationAnswer, OperationRequest, SubmitResult,
};
use message_board_storage::MailCancelGroup;
use relay_transport::{operation, relay_request};

#[tokio::test]
async fn unreplicated_subscription_end_never_sends_its_cancel_after_takeover() -> TestResult {
    Box::pin(prove_remote_cancel(CancelPrefix::Discarded)).await
}
#[tokio::test]
async fn replicated_subscription_end_publishes_the_same_frozen_cancel_after_takeover() -> TestResult
{
    Box::pin(prove_remote_cancel(CancelPrefix::Retained)).await
}
#[tokio::test]
async fn lost_custody_receipt_then_takeover_replays_the_batch_and_cancel_without_new_effects()
-> TestResult {
    Box::pin(prove_remote_cancel(CancelPrefix::LostReceipt)).await
}
#[derive(Clone, Copy)]
enum CancelPrefix {
    Discarded,
    Retained,
    LostReceipt,
}

async fn prove_remote_cancel(case: CancelPrefix) -> TestResult {
    let mut fixture = Box::pin(RuleFixture::open(true)).await?;
    let proof = Box::pin(async {
        let seed = Box::pin(arrange_sent_follow(&mut fixture)).await?;
        let connection = ObjectAddress::Connection(origin()?.installation);
        let (group, cancel) = arrange_remote_end(&fixture, &seed, case).await?;
        let mailbox = ObjectAddress::Mailbox(rule_fixture::RECIPIENT.to_owned().try_into()?);
        if matches!(case, CancelPrefix::LostReceipt) {
            fixture.holder.central.runtime_control(ObjectAddress::Registry, RuntimeControl::HoldNextDispatch { destination: mailbox.clone() }).await?;
        }
        fixture.holder.disconnect_holder().await?;
        let moved = fixture.holder.central.take_over_project(DatabaseId::try_from(fixture.project.as_str().to_owned())?,
            &fixture.successor_origin.installation, TransferId::new(durable_steps::Uuid::now_v7())?).await?;
        assert_eq!(moved.pointer("/value/newEpoch"), Some(&json!(3)));
        if matches!(case, CancelPrefix::Discarded) {
            fixture.holder.central.runtime_control(connection, RuntimeControl::ResumeDispatch).await?;
        }
        received_grants::wait_published_project(&fixture.successor.registry, &fixture.project, 3).await?;
        if matches!(case, CancelPrefix::LostReceipt) {
            lose_actual_submit_answer(&fixture, mailbox, seed.item.id()).await?;
        }
        let items = wait_fetch(&mut fixture, "items").await?;
        assert_eq!(items.pointer("/value/value/through"), Some(&json!(1)));
        assert_eq!(items.pointer("/value/value/entries"), Some(&json!([{"kind":"item","value":{"seq":1,"item":{"kind":"item","value":seed.item}}}])));
        if matches!(case, CancelPrefix::Discarded) {
            assert_eq!(operation(&mut fixture.recipient.socket, relay_request("fetch", json!({"stream":"controls","after":0,"maxItems":10,"maxBytes":1_000_000}))).await?,
                json!({"kind":"delivery","value":{"kind":"fetch","value":{"stream":"controls","entries":[],"through":0}}}));
        } else {
            let controls = wait_fetch(&mut fixture, "controls").await?;
            assert_eq!(controls.pointer("/value/value/entries"), Some(&json!([{"kind":"item","value":{"seq":1,"item":cancel}}])));
            assert_eq!(controls.pointer("/value/value/through"), Some(&json!(1)));
            decide_and_replay(&mut fixture, &seed, &cancel).await?;
        }
        let store = fixture.successor.registry.existing_project(&fixture.project).await?.ok_or("successor store")?;
        let mut read = store.snapshot().await?;
        assert_eq!(read.mail_handoff(&group.id).await?.is_some(), !matches!(case, CancelPrefix::Discarded));
        assert_eq!(read.mail_cancel_expansion_for_target(&group.id, &seed.item.id().as_uuid().to_string().try_into()?).await?.is_some(), !matches!(case, CancelPrefix::Discarded));
        read.finish().await?;
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }).await;
    let _ = fixture
        .holder
        .central
        .runtime_control(ObjectAddress::Registry, RuntimeControl::ResumeDispatch)
        .await;
    let closed = fixture.close().await;
    closed?;
    proof
}

async fn arrange_sent_follow(
    fixture: &mut RuleFixture,
) -> Result<FollowSeed, Box<dyn std::error::Error + Send + Sync>> {
    let (release, task) = fixture
        .holder
        .notifications
        .as_ref()
        .ok_or("old publisher")?
        .hold_progress_writer()
        .await?;
    let seed = Box::pin(fixture.seed_follow(false)).await?;
    fixture.confirm_prefix(fixture.head().await?).await?;
    release.send(()).map_err(|_| "old publication hold ended")?;
    task.await??;
    queue_old_publisher(fixture).await?;
    let sent = wait_fetch(fixture, "items").await?;
    assert_eq!(
        sent.pointer("/value/value/entries"),
        Some(&json!([{"kind":"item","value":{"seq":1,"item":{"kind":"item","value":seed.item}}}]))
    );
    let held = operation(
        &mut fixture.recipient.socket,
        relay_request(
            "report",
            json!({"report":{"item":seed.item.id(),"reportSeq":1,"outcome":{"kind":"held"}}}),
        ),
    )
    .await?;
    assert_eq!(
        held.pointer("/value/value/value/status"),
        Some(&json!({"kind":"held"}))
    );
    Ok(seed)
}

async fn queue_old_publisher(fixture: &RuleFixture) -> TestResult {
    let owner = fixture
        .holder
        .notifications
        .as_ref()
        .ok_or("old publisher")?;
    owner
        .publisher
        .queue_project(fixture.project.clone())
        .await?;
    timeout(
        BOUND,
        owner.publisher.wait_project_idle_for_test(&fixture.project),
    )
    .await?;
    Ok(())
}

async fn arrange_remote_end(
    fixture: &RuleFixture,
    seed: &FollowSeed,
    case: CancelPrefix,
) -> Result<
    (MailCancelGroup, DeliveryEnvelope<BoardDeliveryTypes>),
    Box<dyn std::error::Error + Send + Sync>,
> {
    let connection = ObjectAddress::Connection(origin()?.installation);
    if matches!(case, CancelPrefix::Discarded) {
        fixture
            .holder
            .central
            .runtime_control(
                connection.clone(),
                RuntimeControl::HoldNextDispatch {
                    destination: ObjectAddress::Copy(DatabaseId::try_from(
                        fixture.project.as_str().to_owned(),
                    )?),
                },
            )
            .await?;
    }
    let group = fixture.end_follow(seed).await?;
    if matches!(case, CancelPrefix::Discarded) {
        fixture
            .holder
            .central
            .runtime_control(connection, RuntimeControl::AwaitHeldDispatch)
            .await?;
        queue_old_publisher(fixture).await?;
        assert_no_outgoing_cancel(fixture, seed.item.id()).await?;
        let cancel = fixture.freeze_cancel(seed, &group).await?;
        queue_old_publisher(fixture).await?;
        assert_no_outgoing_cancel(fixture, seed.item.id()).await?;
        return Ok((group, cancel));
    }
    // The old holder freezes this child. Its owning readback is the input to
    // takeover, independent of the successor or recipient's eventual answer.
    let store = fixture
        .holder
        .registry
        .existing_project(&fixture.project)
        .await?
        .ok_or("old store")?;
    let cancel = timeout(BOUND, async {
        loop {
            queue_old_publisher(fixture).await?;
            let mut read = store.snapshot().await?;
            let child = read
                .mail_cancel_expansion_for_target(
                    &group.id,
                    &seed.item.id().as_uuid().to_string().try_into()?,
                )
                .await?;
            read.finish().await?;
            if let Some(child) = child {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(
                    *child.expansion.envelope,
                );
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    timeout(BOUND, async {
        loop {
            let marks = fixture
                .holder
                .projects
                .acknowledged_mail_changes(&fixture.project)
                .await?;
            if marks.max_handoff_seq == Some(2) && marks.max_expansion_seq == Some(1) {
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
            tokio::task::yield_now().await;
        }
    })
    .await??;
    queue_old_publisher(fixture).await?;
    Ok((group, cancel))
}

async fn assert_no_outgoing_cancel(fixture: &RuleFixture, original: ItemId) -> TestResult {
    let owner = fixture
        .holder
        .notifications
        .as_ref()
        .ok_or("old publisher")?;
    assert!(
        owner
            .rows()
            .await?
            .iter()
            .filter(|row| row.category == "outbox")
            .all(|row| row.identity == original.as_uuid().to_string()),
        "unreplicated cancel entered outgoing custody"
    );
    Ok(())
}

async fn wait_fetch(
    fixture: &mut RuleFixture,
    stream: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    timeout(BOUND, async {
        loop {
            let fetched = operation(
                &mut fixture.recipient.socket,
                relay_request(
                    "fetch",
                    json!({"stream":stream,"after":0,"maxItems":10,"maxBytes":1_000_000}),
                ),
            )
            .await?;
            if fetched
                .pointer("/value/value/entries")
                .and_then(serde_json::Value::as_array)
                .is_some_and(|entries| !entries.is_empty())
            {
                return Ok(fetched);
            }
            tokio::task::yield_now().await;
        }
    })
    .await?
}

async fn lose_actual_submit_answer(
    fixture: &RuleFixture,
    mailbox: ObjectAddress,
    item: ItemId,
) -> TestResult {
    timeout(BOUND, async {
        loop {
            let held = fixture
                .holder
                .central
                .runtime_control(ObjectAddress::Registry, RuntimeControl::AwaitHeldDispatch)
                .await?;
            let board_central_test_support::DispatchFaultObservation::Held { request, .. } =
                held.dispatch
            else {
                return Err("registry dispatch was not held".into());
            };
            let request: serde_json::Value = serde_json::from_slice(&request)?;
            if request.get("kind") == Some(&json!("central.mailbox-submit"))
                && request.pointer("/request/origin")
                    == Some(&json!(fixture.successor.projects.peer))
            {
                assert_eq!(
                    request.pointer("/request/item/value/id"),
                    Some(&json!(item))
                );
                fixture
                    .holder
                    .central
                    .runtime_control(
                        ObjectAddress::Registry,
                        RuntimeControl::DropNextObjectReply {
                            destination: mailbox,
                        },
                    )
                    .await?;
                fixture
                    .holder
                    .central
                    .runtime_control(ObjectAddress::Registry, RuntimeControl::ResumeDispatch)
                    .await?;
                let lost = fixture
                    .holder
                    .central
                    .runtime_control(
                        ObjectAddress::Registry,
                        RuntimeControl::AwaitDroppedObjectReply,
                    )
                    .await?;
                assert!(matches!(
                    lost.object_reply,
                    collaboration_central_conformance::ObjectReplyFaultObservation::Dropped { .. }
                ));
                return Ok::<_, Box<dyn std::error::Error + Send + Sync>>(());
            }
            fixture
                .holder
                .central
                .runtime_control(
                    ObjectAddress::Registry,
                    RuntimeControl::ResumeDispatchAndHoldNext {
                        destination: mailbox.clone(),
                    },
                )
                .await?;
        }
    })
    .await?
}

async fn decide_and_replay(
    fixture: &mut RuleFixture,
    seed: &FollowSeed,
    cancel: &DeliveryEnvelope<BoardDeliveryTypes>,
) -> TestResult {
    for (item, report_seq) in [(seed.item.id(), 2), (cancel.id(), 1)] {
        let answer = operation(&mut fixture.recipient.socket, relay_request("report", json!({"report":{"item":item,"reportSeq":report_seq,"outcome":{"kind":"prevented","value":{"cancel":cancel.id()}}}}))).await?;
        assert_eq!(
            answer.pointer("/value/value/value/status"),
            Some(&json!({"kind":"cancelled"}))
        );
    }
    for value in [
        json!({"kind":"item","value":seed.item}),
        serde_json::to_value(cancel)?,
    ] {
        let answer = replay_after_reconnect(fixture, value).await?;
        assert!(matches!(
            answer,
            OperationAnswer::Consumer(BoardConsumerAnswer::MailSubmit(
                MailSubmitAnswer::Submitted(SubmitResult::Replayed {
                    status: fleet_sync_protocol::DeliveryStatus::Cancelled,
                    ..
                })
            ))
        ));
    }
    let statuses = operation(
        &mut fixture.recipient.socket,
        relay_request(
            "query",
            json!({"mailbox":fixture.recipient.peer,"items":[seed.item.id(),cancel.id()]}),
        ),
    )
    .await?;
    assert_eq!(
        statuses.pointer("/value/value/statuses/0/state/value/status"),
        Some(&json!({"kind":"cancelled"}))
    );
    assert_eq!(
        statuses.pointer("/value/value/statuses/1/state/value/cancel"),
        Some(&json!({"kind":"prevented"}))
    );
    assert_eq!(
        operation(
            &mut fixture.recipient.socket,
            relay_request(
                "fetch",
                json!({"stream":"controls","after":1,"maxItems":10,"maxBytes":1_000_000})
            )
        )
        .await?
        .pointer("/value/value/through"),
        Some(&json!(1))
    );
    Ok(())
}

async fn replay_after_reconnect(
    fixture: &RuleFixture,
    value: serde_json::Value,
) -> Result<
    OperationAnswer<board_sync_protocol::BoardConsumerProtocol>,
    Box<dyn std::error::Error + Send + Sync>,
> {
    // The lost successful reply can close the native connection. Repeat the
    // identical immutable item only after known NotSent, until normal reconnect.
    timeout(BOUND, async {
        loop {
            let item: DeliveryEnvelope<BoardDeliveryTypes> = serde_json::from_value(value.clone())?;
            let recipient = item.recipient().clone();
            let request = BoardConsumerRequest::mail_submit(
                item,
                Some(Epoch::new(3)?),
                &recipient.address.endpoint.service_id,
                recipient.peer,
            )?;
            match fixture
                .successor
                .projects
                .requests
                .request(OperationRequest::Consumer(request))
                .await
            {
                Ok(answer) => return Ok(answer),
                Err(durable_steps::CallFailure::NotSent { .. }) => tokio::task::yield_now().await,
                Err(error) => return Err(format!("replay call failed: {error:?}").into()),
            }
        }
    })
    .await?
}
