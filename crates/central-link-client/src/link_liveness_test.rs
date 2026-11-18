//! Lead-approved liveness requirements, first exercised against f4 production code.
use super::*;

#[test]
fn call_deadline_preempts_a_healthy_auto_response_heartbeat() {
    let (mut client, clock) = fixture();
    let attempt = connect(&mut client);
    let correlation = request(&mut client);
    clock.advance(Duration::from_secs(20)).unwrap();
    assert_eq!(client.poll_time(), Some(ClientAction::SendPing));
    client.receive(&attempt, LinkFrame::Pong).unwrap();
    assert_eq!(
        client.next_deadline(),
        Some(ClockInstant::ZERO.after(Duration::from_secs(30)))
    );
    clock.advance(Duration::from_secs(9)).unwrap();
    assert_eq!(client.poll_time(), None);
    clock.advance(Duration::from_secs(1)).unwrap();
    let Some(ClientAction::CallsExpired { failed }) = client.poll_time() else {
        panic!("expiry action")
    };
    assert!(
        matches!(failed.as_slice(), [crate::FailedCall { correlation_id, failure: CallFailure::OutcomeUnknown { request_id } }] if correlation_id == &correlation && request_id.as_str() == "01890f2e-7b4c-7cc0-98c4-000000000004")
    );
    assert!(
        client.closed(&attempt).unwrap().is_empty(),
        "expiry retires the call"
    );
}

#[test]
fn missing_welcome_closes_at_ten_seconds_and_retries_after_backoff() {
    let (mut client, clock) = fixture();
    assert!(matches!(
        client.poll_time(),
        Some(ClientAction::Connect { .. })
    ));
    assert_eq!(
        client.next_deadline(),
        Some(ClockInstant::ZERO.after(Duration::from_secs(10)))
    );
    clock.advance(Duration::from_secs(9)).unwrap();
    assert_eq!(client.poll_time(), None);
    clock.advance(Duration::from_secs(1)).unwrap();
    assert!(matches!(
        client.poll_time(),
        Some(ClientAction::Reconnect { .. })
    ));
    clock.advance(Duration::from_secs(1)).unwrap();
    assert!(matches!(
        client.poll_time(),
        Some(ClientAction::Connect { .. })
    ));
}

#[test]
fn matching_reply_is_proof_of_receipt_without_a_send_mark() {
    let (mut client, _) = fixture();
    let attempt = connect(&mut client);
    let correlation = request(&mut client);
    assert!(matches!(
        client.receive(&attempt, reply(correlation)),
        Ok(ClientEvent::Reply { .. })
    ));
    assert!(client.closed(&attempt).unwrap().is_empty());
}

#[test]
fn unmarked_write_defaults_to_unknown_when_the_connection_closes() {
    let (mut client, _) = fixture();
    let attempt = connect(&mut client);
    let correlation = request(&mut client);
    let failures = client.closed(&attempt).unwrap();
    assert!(
        matches!(failures.as_slice(), [crate::FailedCall { correlation_id, failure: CallFailure::OutcomeUnknown { .. } }] if correlation_id == &correlation)
    );
}

#[test]
fn late_reply_after_ambiguous_send_is_an_event_and_does_not_disrupt_another_call() {
    let (mut client, _) = fixture();
    let attempt = connect(&mut client);
    let retired = request(&mut client);
    let live = request(&mut client);
    client
        .send_failed(&attempt, &retired, crate::SendFailure::MayHaveBeenSent)
        .unwrap();
    assert_eq!(
        client.receive(&attempt, reply(retired.clone())),
        Ok(ClientEvent::LateReply {
            correlation_id: retired
        })
    );
    assert!(matches!(
        client.receive(&attempt, reply(live)),
        Ok(ClientEvent::Reply { .. })
    ));
    assert!(matches!(client.status(), LinkStatus::Connected { .. }));
}

#[test]
fn late_poll_waits_for_two_actual_unanswered_probes() {
    let (mut client, clock) = fixture();
    connect(&mut client);
    clock.advance(Duration::from_secs(20)).unwrap();
    assert_eq!(client.poll_time(), Some(ClientAction::SendPing));
    clock.advance(Duration::from_secs(40)).unwrap();
    assert_eq!(
        client.poll_time(),
        Some(ClientAction::SendPing),
        "only one probe was actually emitted"
    );
    clock.advance(Duration::from_secs(20)).unwrap();
    assert!(matches!(
        client.poll_time(),
        Some(ClientAction::Reconnect { .. })
    ));
}

#[test]
fn frames_before_welcome_are_protocol_errors() {
    let (mut client, _) = fixture();
    let Some(ClientAction::Connect { attempt }) = client.poll_time() else {
        panic!("connect")
    };
    let delivery = LinkFrame::Data(DataFrame::Deliver { message: Box::new(serde_json::from_value(serde_json::json!({"messageId":"01890f2e-7b4c-7cc0-98c4-000000000009","sender":{"kind":"central"},"target":{"machineId":"01890f2e-7b4c-7cc0-98c4-000000000002","endpoint":"codex","sessionId":"s"},"body":{"kind":"direct","body":"hello","delivery":"auto","replyTo":null},"state":{"kind":"queued"},"source":null,"createdAt":"2030-01-01T00:00:00Z","updatedAt":"2030-01-01T00:00:00Z"})).unwrap())});
    for frame in [
        LinkFrame::Pong,
        reply("01890f2e-7b4c-7cc0-98c4-000000000009".parse().unwrap()),
        delivery,
    ] {
        let text = encode_text(&frame, FrameDirection::ToRouter).unwrap();
        let parsed = decode_text(&text, FrameDirection::ToRouter).unwrap();
        assert_eq!(
            client.receive(&attempt, parsed),
            Err(ClientError::UnexpectedFrame)
        );
    }
}

#[test]
fn ack_is_a_connected_id_keyed_frame_without_a_pending_call() {
    use workspace_model::central_mail::{DeliveryAnswer, MessageRefusal};

    let (mut client, _) = fixture();
    let message_id: workspace_model::MessageId =
        "01890f2e-7b4c-7cc0-98c4-000000000009".parse().unwrap();
    let answer = DeliveryAnswer::Refused {
        reason: MessageRefusal::RecipientRefused,
    };
    assert_eq!(
        client.acknowledge_delivery(message_id.clone(), answer),
        Err(ClientError::CentralUnavailable)
    );
    let attempt = connect(&mut client);
    let deadline = client.next_deadline();
    let frame = client
        .acknowledge_delivery(message_id.clone(), answer)
        .unwrap();
    let text = encode_text(&frame, FrameDirection::ToCentral).unwrap();
    assert_eq!(
        decode_text(&text, FrameDirection::ToCentral).unwrap(),
        LinkFrame::Data(DataFrame::Ack { message_id, answer })
    );
    assert_eq!(client.next_deadline(), deadline);
    assert!(client.closed(&attempt).unwrap().is_empty());
}

#[test]
fn close_then_refuse_keeps_the_most_recent_attempt_refused() {
    let (mut client, clock) = fixture();
    let attempt = connect(&mut client);
    client.closed(&attempt).unwrap();
    assert!(
        client
            .refused(&attempt, RefusalReason::MachineRevoked)
            .is_ok()
    );
    clock.advance(Duration::from_secs(100)).unwrap();
    assert_eq!(client.poll_time(), None);
    assert_eq!(
        client.status(),
        LinkStatus::Refused {
            reason: RefusalReason::MachineRevoked
        }
    );
}

fn read_request(client: &mut LinkClient) -> CorrelationId {
    let payload = RequestPayload::Workspace(WorkspaceRequest {
        request_id: None,
        workspace_id: Some("01890f2e-7b4c-7cc0-98c4-000000000003".parse().unwrap()),
        operation: WorkspaceOperation::List {
            query: workspace_model::ListQuery::Areas {
                include_archived: false,
            },
            cursor: workspace_model::ListCursor::start(workspace_model::ListOrder::OldestFirst),
            limit: 10_u16.try_into().unwrap(),
        },
    });
    let LinkFrame::Data(DataFrame::Request { request }) =
        client.begin_request(caller(), payload).unwrap()
    else {
        panic!("request")
    };
    request.correlation_id().clone()
}

#[test]
fn staggered_and_simultaneous_call_deadlines_retire_only_due_calls() {
    let (mut client, clock) = fixture();
    let attempt = connect(&mut client);
    let first = request(&mut client);
    clock.advance(Duration::from_secs(5)).unwrap();
    let second = request(&mut client);
    let read = read_request(&mut client);
    let mail_payload = RequestPayload::Mail(workspace_model::central_mail::CentralMailRequest {
        request_id: "01890f2e-7b4c-7cc0-98c4-000000000005".parse().unwrap(),
        operation: workspace_model::central_mail::CentralMailOperation::SetNoticeCadence {
            cadence: workspace_model::NoticeCadence::M25,
        },
    });
    let LinkFrame::Data(DataFrame::Request {
        request: mail_request,
    }) = client.begin_request(caller(), mail_payload).unwrap()
    else {
        panic!("mail request")
    };
    let mail = mail_request.correlation_id().clone();
    clock.advance(Duration::from_secs(15)).unwrap();
    assert_eq!(client.poll_time(), Some(ClientAction::SendPing));
    client.receive(&attempt, LinkFrame::Pong).unwrap();
    clock.advance(Duration::from_secs(10)).unwrap();
    let Some(ClientAction::CallsExpired { failed }) = client.poll_time() else {
        panic!("first expiry")
    };
    assert!(
        matches!(failed.as_slice(), [crate::FailedCall { correlation_id, failure: CallFailure::OutcomeUnknown { .. } }] if correlation_id == &first)
    );
    assert_eq!(
        client.next_deadline(),
        Some(ClockInstant::ZERO.after(Duration::from_secs(35)))
    );
    assert_eq!(
        client.receive(&attempt, reply(first.clone())),
        Ok(ClientEvent::LateReply {
            correlation_id: first
        })
    );
    clock.advance(Duration::from_secs(5)).unwrap();
    let Some(ClientAction::CallsExpired { failed }) = client.poll_time() else {
        panic!("second expiry")
    };
    assert_eq!(failed.len(), 3);
    assert!(failed.iter().any(|call| call.correlation_id == second
        && matches!(call.failure, CallFailure::OutcomeUnknown { .. })));
    assert!(
        failed
            .iter()
            .any(|call| call.correlation_id == read
                && call.failure == CallFailure::CentralUnavailable)
    );
    assert!(failed.iter().any(|call| call.correlation_id == mail && matches!(&call.failure, CallFailure::OutcomeUnknown { request_id } if request_id.as_str() == "01890f2e-7b4c-7cc0-98c4-000000000005")));
    assert_eq!(client.poll_time(), None);
    assert!(client.closed(&attempt).unwrap().is_empty());
}

#[test]
fn a_call_expiry_does_not_consume_a_heartbeat_due_at_the_same_instant() {
    let (mut client, clock) = fixture();
    let attempt = connect(&mut client);
    clock.advance(Duration::from_secs(10)).unwrap();
    request(&mut client);
    clock.advance(Duration::from_secs(10)).unwrap();
    assert_eq!(client.poll_time(), Some(ClientAction::SendPing));
    client.receive(&attempt, LinkFrame::Pong).unwrap();
    clock.advance(Duration::from_secs(20)).unwrap();
    assert!(matches!(
        client.poll_time(),
        Some(ClientAction::CallsExpired { .. })
    ));
    assert_eq!(client.next_deadline(), Some(clock.monotonic_now()));
    assert_eq!(client.poll_time(), Some(ClientAction::SendPing));
    assert_eq!(client.poll_time(), None);
}

#[test]
fn a_reply_cancels_its_call_deadline_and_reconnect_clears_retired_correlations() {
    let (mut client, clock) = fixture();
    let old = connect(&mut client);
    let completed = request(&mut client);
    clock.advance(Duration::from_secs(20)).unwrap();
    assert_eq!(client.poll_time(), Some(ClientAction::SendPing));
    client.receive(&old, LinkFrame::Pong).unwrap();
    clock.advance(Duration::from_secs(9)).unwrap();
    assert!(matches!(
        client.receive(&old, reply(completed.clone())),
        Ok(ClientEvent::Reply { .. })
    ));
    assert_eq!(
        client.next_deadline(),
        Some(ClockInstant::ZERO.after(Duration::from_secs(40)))
    );
    clock.advance(Duration::from_secs(1)).unwrap();
    assert_eq!(client.poll_time(), None);
    client.closed(&old).unwrap();
    clock.advance(Duration::from_secs(1)).unwrap();
    let current = connect(&mut client);
    let live = request(&mut client);
    assert_eq!(
        client.receive(&current, reply(completed)),
        Err(ClientError::UnknownCorrelation)
    );
    assert_eq!(
        client.receive(&old, reply(live.clone())),
        Err(ClientError::StaleConnection)
    );
    assert!(matches!(
        client.receive(&current, reply(live)),
        Ok(ClientEvent::Reply { .. })
    ));
}

#[test]
fn refusal_order_is_idempotent_and_an_old_attempt_cannot_refuse_a_new_one() {
    let (mut client, clock) = fixture();
    let old = connect(&mut client);
    request(&mut client);
    assert_eq!(client.closed(&old).unwrap().len(), 1);
    assert!(
        client
            .refused(&old, RefusalReason::MachineRevoked)
            .unwrap()
            .is_empty()
    );
    assert!(client.closed(&old).unwrap().is_empty());
    assert_eq!(client.poll_time(), None);
    client.reenrolled();
    let current = connect(&mut client);
    assert_eq!(
        client.refused(&old, RefusalReason::MachineRevoked),
        Err(ClientError::StaleConnection)
    );
    clock.advance(Duration::from_secs(20)).unwrap();
    assert_eq!(client.poll_time(), Some(ClientAction::SendPing));
    assert_eq!(
        client.receive(&old, LinkFrame::Pong),
        Err(ClientError::StaleConnection)
    );
    client
        .refused(&current, RefusalReason::CredentialRefused)
        .unwrap();
    assert!(client.closed(&current).unwrap().is_empty());
    clock.advance(Duration::from_secs(100)).unwrap();
    assert_eq!(client.poll_time(), None);
}

#[test]
fn welcome_cancels_its_deadline_and_late_socket_close_does_not_double_backoff() {
    let (mut client, clock) = fixture();
    let Some(ClientAction::Connect { attempt }) = client.poll_time() else {
        panic!("connect")
    };
    clock.advance(Duration::from_secs(9)).unwrap();
    client
        .receive(
            &attempt,
            LinkFrame::Data(DataFrame::Welcome {
                machine_id: client.machine.clone(),
                central_time: clock.utc_now(),
            }),
        )
        .unwrap();
    assert_eq!(
        client.next_deadline(),
        Some(ClockInstant::ZERO.after(Duration::from_secs(29)))
    );
    clock.advance(Duration::from_secs(1)).unwrap();
    assert_eq!(client.poll_time(), None);
    client.closed(&attempt).unwrap();
    let deadline = client.next_deadline();
    client.closed(&attempt).unwrap();
    assert_eq!(client.next_deadline(), deadline);
}
