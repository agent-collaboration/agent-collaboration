use super::*;
use crate::CallFailure;
use central_link_protocol::{FrameDirection, decode_text, encode_text};
use router_clock::ManualClock;
use workspace_model::{WorkspaceOperation, WorkspaceReply, WorkspaceRequest, WorkspaceResult};

#[path = "link_liveness_test.rs"]
mod liveness;

fn fixture() -> (LinkClient, Arc<ManualClock>) {
    let clock = Arc::new(ManualClock::new("2030-01-01T00:00:00Z".parse().unwrap()));
    (
        LinkClient::new(
            "01890f2e-7b4c-7cc0-98c4-000000000002".parse().unwrap(),
            clock.clone(),
        ),
        clock,
    )
}
fn connect(client: &mut LinkClient) -> ConnectionAttempt {
    let Some(ClientAction::Connect { attempt }) = client.poll_time() else {
        panic!("connect action required")
    };
    let welcome = LinkFrame::Data(DataFrame::Welcome {
        machine_id: client.machine.clone(),
        central_time: client.clock.utc_now(),
    });
    assert_eq!(
        client.receive(&attempt, welcome).unwrap(),
        ClientEvent::Welcomed
    );
    attempt
}
fn caller() -> Caller {
    serde_json::from_str(r#"{"kind":"session","value":{"machineId":"01890f2e-7b4c-7cc0-98c4-000000000002","endpoint":"codex","sessionId":"s"}}"#).unwrap()
}
fn payload() -> RequestPayload {
    RequestPayload::Workspace(WorkspaceRequest {
        request_id: Some("01890f2e-7b4c-7cc0-98c4-000000000004".parse().unwrap()),
        workspace_id: Some("01890f2e-7b4c-7cc0-98c4-000000000003".parse().unwrap()),
        operation: WorkspaceOperation::CreateArea {
            name: "Work".to_owned().try_into().unwrap(),
        },
    })
}
fn request(client: &mut LinkClient) -> CorrelationId {
    let frame = client.begin_request(caller(), payload()).unwrap();
    let wire = encode_text(&frame, FrameDirection::ToCentral).unwrap();
    let LinkFrame::Data(DataFrame::Request { request }) =
        decode_text(&wire, FrameDirection::ToCentral).unwrap()
    else {
        panic!("request")
    };
    request.correlation_id().clone()
}
fn reply(correlation_id: CorrelationId) -> LinkFrame {
    LinkFrame::Data(DataFrame::Reply {
        correlation_id,
        result: Box::new(ReplyPayload::Workspace(WorkspaceReply {
            result: WorkspaceResult::CentralUnavailable,
            actors: vec![],
        })),
    })
}

#[test]
fn down_pre_welcome_and_refused_calls_fail_without_queueing() {
    let (mut client, clock) = fixture();
    assert_eq!(
        client.begin_request(caller(), payload()),
        Err(ClientError::CentralUnavailable)
    );
    let Some(ClientAction::Connect { attempt }) = client.poll_time() else {
        panic!("connect")
    };
    assert_eq!(
        client.begin_request(caller(), payload()),
        Err(ClientError::CentralUnavailable)
    );
    assert!(
        client
            .refused(&attempt, RefusalReason::MachineRevoked)
            .unwrap()
            .is_empty()
    );
    clock.advance(Duration::from_secs(1000)).unwrap();
    assert_eq!(client.poll_time(), None);
    assert_eq!(client.next_deadline(), None);
    assert_eq!(
        client.begin_request(caller(), payload()),
        Err(ClientError::CentralUnavailable)
    );
    client.reenrolled();
    connect(&mut client);
    assert!(matches!(client.status(), LinkStatus::Connected { .. }));
}

#[test]
fn concurrent_out_of_order_replies_do_not_conflate_write_and_correlation_ids() {
    let (mut client, _) = fixture();
    let attempt = connect(&mut client);
    let first = request(&mut client);
    let second = request(&mut client);
    assert_ne!(first, second);
    assert!(
        matches!(client.receive(&attempt, reply(second.clone())), Ok(ClientEvent::Reply { correlation_id, .. }) if correlation_id == second)
    );
    assert_eq!(
        client.receive(&attempt, reply(second.clone())),
        Ok(ClientEvent::LateReply {
            correlation_id: second
        })
    );
    assert!(matches!(
        client.receive(&attempt, reply(first)),
        Ok(ClientEvent::Reply { .. })
    ));
    assert!(client.closed(&attempt).unwrap().is_empty());
}

#[test]
fn transport_frontier_preserves_unknown_writes_and_old_events_cannot_complete_new_calls() {
    let (mut client, clock) = fixture();
    let old = connect(&mut client);
    let unsent = request(&mut client);
    let sent = request(&mut client);
    let known_unsent = client
        .send_failed(&old, &unsent, crate::SendFailure::DefinitelyNotSent)
        .unwrap();
    assert_eq!(known_unsent.failure, CallFailure::CentralUnavailable);
    let failed = client.closed(&old).unwrap();
    assert!(!failed.iter().any(|call| call.correlation_id == unsent));
    assert!(failed.iter().any(|call| call.correlation_id == sent && matches!(&call.failure, CallFailure::OutcomeUnknown { request_id } if request_id.as_str() == "01890f2e-7b4c-7cc0-98c4-000000000004")));
    clock.advance(Duration::from_secs(1)).unwrap();
    let current = connect(&mut client);
    let fresh = request(&mut client);
    assert_ne!(sent, fresh);
    assert_eq!(
        client.receive(&old, reply(fresh.clone())),
        Err(ClientError::StaleConnection)
    );
    assert_eq!(client.closed(&old), Err(ClientError::StaleConnection));
    assert_eq!(
        client.receive(&current, reply(sent)),
        Err(ClientError::UnknownCorrelation)
    );
    assert!(matches!(
        client.receive(&current, reply(fresh)),
        Ok(ClientEvent::Reply { .. })
    ));
}

#[test]
fn injected_clock_drives_exact_pings_pong_recovery_and_two_missed_answers() {
    let (mut client, clock) = fixture();
    let attempt = connect(&mut client);
    clock.advance(Duration::from_secs(19)).unwrap();
    assert_eq!(client.poll_time(), None);
    clock.advance(Duration::from_secs(1)).unwrap();
    assert_eq!(client.poll_time(), Some(ClientAction::SendPing));
    assert_eq!(
        client.receive(&attempt, LinkFrame::Pong),
        Ok(ClientEvent::Pong)
    );
    clock.advance(Duration::from_secs(20)).unwrap();
    assert_eq!(client.poll_time(), Some(ClientAction::SendPing));
    clock.advance(Duration::from_secs(20)).unwrap();
    assert_eq!(client.poll_time(), Some(ClientAction::SendPing));
    clock.advance(Duration::from_secs(20)).unwrap();
    assert!(matches!(
        client.poll_time(),
        Some(ClientAction::Reconnect { .. })
    ));
    assert_eq!(client.status(), LinkStatus::Connecting);
}

#[test]
fn reconnect_backoff_caps_at_thirty_and_welcome_resets_it() {
    let (mut client, clock) = fixture();
    for seconds in [1, 2, 4, 8, 16, 30, 30] {
        let Some(ClientAction::Connect { attempt }) = client.poll_time() else {
            panic!("connect")
        };
        assert!(client.closed(&attempt).unwrap().is_empty());
        clock
            .advance(Duration::from_millis(seconds * 1000 - 1))
            .unwrap();
        assert_eq!(client.poll_time(), None);
        clock.advance(Duration::from_millis(1)).unwrap();
    }
    let attempt = connect(&mut client);
    client.closed(&attempt).unwrap();
    clock.advance(Duration::from_millis(999)).unwrap();
    assert_eq!(client.poll_time(), None);
    clock.advance(Duration::from_millis(1)).unwrap();
    assert!(matches!(
        client.poll_time(),
        Some(ClientAction::Connect { .. })
    ));
}

#[test]
fn duplicate_wrong_welcome_reply_family_and_direction_never_consume_a_pending_call() {
    let (mut client, _) = fixture();
    let attempt = connect(&mut client);
    let wrong_welcome = LinkFrame::Data(DataFrame::Welcome {
        machine_id: client.machine.clone(),
        central_time: client.clock.utc_now(),
    });
    assert_eq!(
        client.receive(&attempt, wrong_welcome),
        Err(ClientError::InvalidWelcome)
    );
    let id = request(&mut client);
    let wrong = LinkFrame::Data(DataFrame::Reply {
        correlation_id: id.clone(),
        result: Box::new(ReplyPayload::Mail(
            workspace_model::central_mail::CentralMailResult::CentralUnavailable,
        )),
    });
    assert_eq!(
        client.receive(&attempt, wrong),
        Err(ClientError::WrongReplyFamily)
    );
    assert_eq!(
        client.receive(&attempt, LinkFrame::Ping),
        Err(ClientError::UnexpectedFrame)
    );
    assert!(matches!(
        client.receive(&attempt, reply(id)),
        Ok(ClientEvent::Reply { .. })
    ));
}

#[test]
fn wrong_machine_welcome_is_refused_and_read_disconnect_has_no_durable_retry_id() {
    let (mut client, _) = fixture();
    let Some(ClientAction::Connect { attempt }) = client.poll_time() else {
        panic!("connect")
    };
    let wrong = LinkFrame::Data(DataFrame::Welcome {
        machine_id: "01890f2e-7b4c-7cc0-98c4-000000000099".parse().unwrap(),
        central_time: client.clock.utc_now(),
    });
    assert_eq!(
        client.receive(&attempt, wrong),
        Err(ClientError::InvalidWelcome)
    );
    assert_eq!(client.status(), LinkStatus::Connecting);
    let welcome = LinkFrame::Data(DataFrame::Welcome {
        machine_id: client.machine.clone(),
        central_time: client.clock.utc_now(),
    });
    client.receive(&attempt, welcome).unwrap();
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
    assert!(request.payload().write_request_id().is_none());
    let failed = client.closed(&attempt).unwrap();
    assert!(matches!(
        failed.as_slice(),
        [crate::FailedCall {
            failure: CallFailure::CentralUnavailable,
            ..
        }]
    ));
}

#[test]
fn late_clock_poll_counts_actual_unanswered_probes_and_ids_follow_the_same_clock() {
    let (mut client, clock) = fixture();
    let attempt = connect(&mut client);
    clock.advance(Duration::from_secs(100)).unwrap();
    assert_eq!(client.poll_time(), Some(ClientAction::SendPing));
    assert_eq!(
        client.receive(&attempt, LinkFrame::Pong),
        Ok(ClientEvent::Pong)
    );
    let correlation = request(&mut client);
    client
        .receive(&attempt, reply(correlation.clone()))
        .unwrap();
    let id: uuid::Uuid = String::from(correlation).parse().unwrap();
    let (seconds, _) = id.get_timestamp().unwrap().to_unix();
    assert_eq!(seconds, 1_893_456_100);
    clock.advance(Duration::from_secs(20)).unwrap();
    assert_eq!(client.poll_time(), Some(ClientAction::SendPing));
    clock.advance(Duration::from_secs(40)).unwrap();
    assert_eq!(client.poll_time(), Some(ClientAction::SendPing));
    clock.advance(Duration::from_secs(20)).unwrap();
    assert!(matches!(
        client.poll_time(),
        Some(ClientAction::Reconnect { .. })
    ));
}

#[test]
fn transport_failure_receipts_distinguish_unsent_from_ambiguous_write() {
    let (mut client, _) = fixture();
    let attempt = connect(&mut client);
    let known = request(&mut client);
    let ambiguous = request(&mut client);
    let known = client
        .send_failed(&attempt, &known, crate::SendFailure::DefinitelyNotSent)
        .unwrap();
    let ambiguous = client
        .send_failed(&attempt, &ambiguous, crate::SendFailure::MayHaveBeenSent)
        .unwrap();
    assert_eq!(known.failure, CallFailure::CentralUnavailable);
    assert!(matches!(
        ambiguous.failure,
        CallFailure::OutcomeUnknown { .. }
    ));
    assert!(client.closed(&attempt).unwrap().is_empty());
}
