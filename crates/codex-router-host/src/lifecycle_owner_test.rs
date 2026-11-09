use super::*;
use crate::OperatorFrame;

#[test]
fn retirement_answers_every_coalesced_status_request_exactly_once() {
    // Arrange: distinct requesters share one unfinished observation.
    let requests = [
        OperatorRequest::Status,
        OperatorRequest::AwaitHostStart,
        OperatorRequest::Status,
    ];
    let mut responses = Vec::new();
    let mut receivers = Vec::new();
    for request in requests {
        let (sender, receiver) = mpsc::channel(2);
        responses.push((request.clone(), sender));
        receivers.push((request, receiver));
    }
    let mut active = Some(request_admission::ActiveStatusObservation {
        future: Box::pin(std::future::pending()),
        responses,
    });
    let state = RuntimeState::ready(
        RouterCondition::ExternalReachable,
        AppServerReadiness::Ready {
            running_version: "fixture".to_owned(),
            remote_control_server_name: None,
        },
    );

    // Act: repeated retirement cannot answer an already cancelled observation.
    cancel_status_observation(&mut active, state.snapshot());
    cancel_status_observation(&mut active, state.snapshot());

    // Assert: no requester is forgotten, misidentified or answered twice.
    assert!(active.is_none());
    for (request, mut receiver) in receivers {
        let OperatorFrame::Terminal(answer) = receiver.try_recv().unwrap() else {
            panic!("retirement did not answer the request with a terminal");
        };
        assert_eq!(answer.request(), &request);
        assert_eq!(answer.classification(), TerminalClassification::Unavailable);
        assert_eq!(answer.snapshot(), &state.snapshot());
        assert!(matches!(
            receiver.try_recv(),
            Err(mpsc::error::TryRecvError::Disconnected)
        ));
    }
}
