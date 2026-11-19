use central_link_client::{ClientAction, ClientEvent, LinkClient};
use central_link_protocol::{
    DataFrame, FrameDirection, LinkFrame, ReplyPayload, RequestPayload, decode_text, encode_text,
};
use router_clock::ManualClock;
use std::sync::Arc;
use workspace_model::{
    Caller, WorkspaceOperation, WorkspaceReply, WorkspaceRequest, WorkspaceResult,
};

#[test]
fn protocol_stub_correlates_out_of_order_replies_and_preserves_durable_retry_id() {
    // Arrange: the stub owns only serialized server frames, never client policy.
    let machine = "01890f2e-7b4c-7cc0-98c4-000000000002".parse().unwrap();
    let clock = Arc::new(ManualClock::new("2030-01-01T00:00:00Z".parse().unwrap()));
    let mut client = LinkClient::new(machine, clock);
    let Some(ClientAction::Connect { attempt }) = client.poll_time() else {
        panic!("connect")
    };
    let welcome = r#"{"kind":"welcome","machineId":"01890f2e-7b4c-7cc0-98c4-000000000002","centralTime":"2030-01-01T00:00:00Z"}"#;
    client
        .receive(
            &attempt,
            decode_text(welcome, FrameDirection::ToRouter).unwrap(),
        )
        .unwrap();
    let caller: Caller = serde_json::from_str(r#"{"kind":"session","value":{"machineId":"01890f2e-7b4c-7cc0-98c4-000000000002","sessionId":"s"}}"#).unwrap();
    let payload = RequestPayload::Workspace(WorkspaceRequest {
        request_id: Some("01890f2e-7b4c-7cc0-98c4-000000000004".parse().unwrap()),
        workspace_id: Some("01890f2e-7b4c-7cc0-98c4-000000000003".parse().unwrap()),
        operation: WorkspaceOperation::CreateArea {
            name: "Work".to_owned().try_into().unwrap(),
            description: "Where the work happens".to_owned().try_into().unwrap(),
        },
    });
    let mut requests = Vec::new();
    // Act: retry has the same write id but gets another transport correlation.
    for _ in 0..2 {
        let frame = client
            .begin_request(caller.clone(), payload.clone())
            .unwrap();
        let text = encode_text(&frame, FrameDirection::ToCentral).unwrap();
        let LinkFrame::Data(DataFrame::Request { request }) =
            decode_text(&text, FrameDirection::ToCentral).unwrap()
        else {
            panic!("request")
        };
        requests.push(request);
    }
    let mut replies = Vec::new();
    for request in requests.iter().rev() {
        let frame = LinkFrame::Data(DataFrame::Reply {
            correlation_id: request.correlation_id().clone(),
            result: Box::new(ReplyPayload::Workspace(WorkspaceReply {
                result: WorkspaceResult::CentralUnavailable,
                actors: vec![],
            })),
        });
        let text = encode_text(&frame, FrameDirection::ToRouter).unwrap();
        replies.push(
            client
                .receive(
                    &attempt,
                    decode_text(&text, FrameDirection::ToRouter).unwrap(),
                )
                .unwrap(),
        );
    }
    // Assert: each reply names its own correlation, with the original durable id intact.
    let [first, second] = requests.as_slice() else {
        panic!("two requests")
    };
    assert_ne!(first.correlation_id(), second.correlation_id());
    assert_eq!(
        first.payload().write_request_id(),
        second.payload().write_request_id()
    );
    assert!(
        matches!(replies.first(), Some(ClientEvent::Reply { correlation_id, .. }) if correlation_id == second.correlation_id())
    );
    assert!(
        matches!(replies.last(), Some(ClientEvent::Reply { correlation_id, .. }) if correlation_id == first.correlation_id())
    );
    assert!(client.closed(&attempt).unwrap().is_empty());
}
