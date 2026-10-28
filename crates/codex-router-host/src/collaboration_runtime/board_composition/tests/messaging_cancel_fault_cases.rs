//! Competing real mailbox calls cannot consume the selected lost-receipt fault.
use super::*;
use fleet_sync_protocol::{CancelBody, DeliveryCancel};

type NativeAnswer = OperationAnswer<board_sync_protocol::BoardConsumerProtocol>;
type ProofResult<Value> = Result<Value, Box<dyn std::error::Error + Send + Sync>>;

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn lost_receipt_hold_targets_submit_while_sender_query_completes() -> TestResult {
    let mut fixture = RuleFixture::open(true).await?;
    let mut competing_calls = tokio::task::JoinSet::new();
    let proof = async {
        let seed = arrange_sent_follow(&mut fixture).await?;
        let mailbox = ObjectAddress::Mailbox(rule_fixture::RECIPIENT.to_owned().try_into()?);
        hold_lost_receipt_submit(
            &fixture,
            mailbox,
            fixture.holder.projects.peer,
            seed.item.id(),
        )
        .await?;
        let query = sender_query_for_item(&fixture, seed.item.id())?;
        let (query_finished, mut query_ready) = tokio::sync::watch::channel(false);
        let query_requests = fixture.holder.projects.clone();
        competing_calls.spawn(async move {
            let result = query_requests.requests.request(query).await;
            query_finished.send_replace(true);
            ("query", result)
        });
        // Either the unrelated query finishes, or the old broad fault holds it.
        // Do not wait for a hold that a correctly selected fault must not create.
        timeout(BOUND, async {
            tokio::select! {
                held = fixture.holder.central.runtime_control(
                    ObjectAddress::Registry, RuntimeControl::AwaitHeldDispatch,
                ) => { held?; }
                completed = query_ready.wait_for(|ready| *ready) => { completed?; }
            }
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
        })
        .await??;
        let expected_item = seed.item.id();
        let envelope = DeliveryEnvelope::Item(seed.item);
        let recipient = envelope.recipient().clone();
        let submit = OperationRequest::Consumer(BoardConsumerRequest::mail_submit(
            envelope,
            Some(Epoch::new(2)?),
            &recipient.address.endpoint.service_id,
            recipient.peer,
        )?);
        let (submit_finished, mut submit_ready) = tokio::sync::watch::channel(false);
        let submit_requests = fixture.holder.projects.clone();
        competing_calls.spawn(async move {
            let result = submit_requests.requests.request(submit).await;
            submit_finished.send_replace(true);
            ("submit", result)
        });
        let held = fixture
            .holder
            .central
            .runtime_control(ObjectAddress::Registry, RuntimeControl::AwaitHeldDispatch)
            .await?;
        let board_central_test_support::DispatchFaultObservation::Held { request, .. } =
            held.dispatch
        else {
            return Err("lost-receipt fixture did not hold a dispatch".into());
        };
        let request: serde_json::Value = serde_json::from_slice(&request)?;
        let selected_submit = request.get("kind") == Some(&json!("central.mailbox-submit"))
            && request.pointer("/request/origin") == Some(&json!(fixture.holder.projects.peer))
            && request.pointer("/request/item/value/id") == Some(&json!(expected_item));
        if !selected_submit {
            // With the broad hold, the actual submit completes past the held
            // query. Observe that bypass before releasing either request.
            timeout(BOUND, submit_ready.wait_for(|ready| *ready)).await??;
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>((selected_submit, request))
    }
    .await;
    let _ = fixture
        .holder
        .central
        .runtime_control(ObjectAddress::Registry, RuntimeControl::ResumeDispatch)
        .await;
    if proof.is_err() {
        competing_calls.shutdown().await;
    }
    let answers = timeout(BOUND, async {
        let mut answers = Vec::new();
        while let Some(answer) = competing_calls.join_next().await {
            let (kind, answer) = answer?;
            answers.push((kind, answer?));
        }
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(answers)
    })
    .await;
    if !matches!(&answers, Ok(Ok(_))) {
        competing_calls.shutdown().await;
    }
    let closed = fixture.close().await;
    closed?;
    let (selected_submit, held_request) = proof?;
    let answers = answers??;
    assert_competing_answers(answers)?;
    assert!(
        selected_submit,
        "unrelated request was held while the intended submit completed: {held_request}"
    );
    Ok(())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
async fn lost_receipt_reply_stays_bound_when_a_cancel_submit_finishes_first() -> TestResult {
    let mut fixture = RuleFixture::open(true).await?;
    let mut held_calls = tokio::task::JoinSet::new();
    let proof = async {
        let (cancel, mailbox, held_request) =
            hold_original_with_cancel(&mut fixture, &mut held_calls).await?;
        let recipient = cancel.recipient().clone();
        fixture
            .holder
            .central
            .runtime_control(
                ObjectAddress::Registry,
                RuntimeControl::DropHeldDispatchReply,
            )
            .await?;

        // This is a real second Registry -> Mailbox submit on the same native
        // connection. It must finish while the original remains held.
        let cancel = BoardConsumerRequest::mail_submit(
            cancel,
            Some(Epoch::new(2)?),
            &recipient.address.endpoint.service_id,
            recipient.peer,
        )?;
        let answer = timeout(
            BOUND,
            fixture
                .holder
                .projects
                .requests
                .request(OperationRequest::Consumer(cancel)),
        )
        .await??;
        assert!(matches!(
            answer,
            OperationAnswer::Consumer(BoardConsumerAnswer::MailSubmit(
                MailSubmitAnswer::Submitted(_)
            ))
        ));
        assert!(
            held_calls.try_join_next().is_none(),
            "held original answered before release"
        );
        let state = fixture
            .holder
            .central
            .runtime_control(ObjectAddress::Registry, RuntimeControl::Observe)
            .await?;
        assert!(
            matches!(
                state.object_reply,
                collaboration_central_conformance::ObjectReplyFaultObservation::Armed { .. }
            ),
            "competing cancel consumed the original reply fault"
        );
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
        let collaboration_central_conformance::ObjectReplyFaultObservation::Dropped {
            destination,
            request,
        } = lost.object_reply
        else {
            return Err("original reply was not dropped".into());
        };
        assert_eq!(destination, mailbox);
        assert_eq!(
            request, held_request,
            "drop must match the exact held original"
        );
        let answer = timeout(BOUND, held_calls.join_next())
            .await?
            .ok_or("held submit task disappeared")??;
        assert!(
            matches!(answer, Err(durable_steps::CallFailure::AnswerLost)),
            "original submit should lose its actual reply"
        );
        Ok::<_, Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    let _ = fixture
        .holder
        .central
        .runtime_control(ObjectAddress::Registry, RuntimeControl::ResumeDispatch)
        .await;
    held_calls.shutdown().await;
    let closed = fixture.close().await;
    closed?;
    proof
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract asserts; setup may fail."
)]
fn assert_competing_answers(answers: Vec<(&str, NativeAnswer)>) -> TestResult {
    assert_eq!(answers.len(), 2);
    for (kind, answer) in answers {
        match kind {
            "query" => assert!(matches!(
                answer,
                OperationAnswer::Delivery(fleet_sync_protocol::DeliveryAnswer::Query(_))
            )),
            "submit" => assert!(matches!(
                answer,
                OperationAnswer::Consumer(BoardConsumerAnswer::MailSubmit(
                    MailSubmitAnswer::Submitted(SubmitResult::Replayed {
                        status: fleet_sync_protocol::DeliveryStatus::Held,
                        ..
                    })
                ))
            )),
            _ => return Err("unexpected competing call".into()),
        }
    }
    Ok(())
}

async fn hold_original_with_cancel(
    fixture: &mut RuleFixture,
    held_calls: &mut tokio::task::JoinSet<Result<NativeAnswer, durable_steps::CallFailure>>,
) -> ProofResult<(DeliveryEnvelope<BoardDeliveryTypes>, ObjectAddress, Vec<u8>)> {
    let seed = arrange_sent_follow(fixture).await?;
    let item = seed.item.id();
    let recipient = seed.item.recipient().clone();
    let cancel = DeliveryEnvelope::Cancel(DeliveryCancel::<BoardDeliveryTypes>::new(
        ItemId::new(durable_steps::Uuid::now_v7())?,
        seed.item.sender().clone(),
        recipient.clone(),
        None,
        CancelBody { target: item },
    )?);
    let mailbox = ObjectAddress::Mailbox(rule_fixture::RECIPIENT.to_owned().try_into()?);
    hold_lost_receipt_submit(fixture, mailbox.clone(), fixture.holder.projects.peer, item).await?;
    let submit = BoardConsumerRequest::mail_submit(
        DeliveryEnvelope::Item(seed.item),
        Some(Epoch::new(2)?),
        &recipient.address.endpoint.service_id,
        recipient.peer,
    )?;
    let requests = fixture.holder.projects.clone();
    held_calls.spawn(async move {
        requests
            .requests
            .request(OperationRequest::Consumer(submit))
            .await
    });
    let held = fixture
        .holder
        .central
        .runtime_control(ObjectAddress::Registry, RuntimeControl::AwaitHeldDispatch)
        .await?;
    let board_central_test_support::DispatchFaultObservation::Held {
        destination,
        request: held_request,
    } = held.dispatch
    else {
        return Err("original submit was not held".into());
    };
    assert_eq!(destination, mailbox);
    let request: serde_json::Value = serde_json::from_slice(&held_request)?;
    assert_eq!(request.get("kind"), Some(&json!("central.mailbox-submit")));
    assert_eq!(
        request.pointer("/request/origin"),
        Some(&json!(fixture.holder.projects.peer))
    );
    assert_eq!(
        request.pointer("/request/item/value/id"),
        Some(&json!(item))
    );
    Ok((cancel, mailbox, held_request))
}

fn sender_query_for_item(
    fixture: &RuleFixture,
    item: ItemId,
) -> ProofResult<OperationRequest<board_sync_protocol::BoardConsumerProtocol>> {
    Ok(OperationRequest::Delivery(
        fleet_sync_protocol::DeliveryRequest::Query {
            call: runtime_primitives::CallId {
                operation: runtime_primitives::OperationId::new(durable_steps::Uuid::now_v7())?,
                step: runtime_primitives::StepName::new("lost-receipt-competing-query")?,
            },
            mailbox: fleet_sync_protocol::PeerId::new(fixture.recipient.peer)?,
            items: vec![item],
        },
    ))
}
