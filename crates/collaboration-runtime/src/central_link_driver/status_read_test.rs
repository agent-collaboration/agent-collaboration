use super::super::driver_test_support::*;
use central_link_client::{LinkCalls, RequestStatus};
use central_link_protocol::{DataFrame, LinkFrame, ReplyPayload, RequestStatusAnswer};
use workspace_model::{
    ListQuery, WorkspaceAnswer, WorkspaceOperation, WorkspaceReply, WorkspaceResult,
    central_mail::MessageStatus,
};

#[tokio::test]
async fn a_local_write_does_not_answer_a_query_for_the_wrong_scope() {
    let mut harness = Harness::start(tempfile::tempdir().unwrap(), clock_at(MARK - 20_000)).await;
    let mut central = harness.connected().await;
    let request = uuid_v7(90);
    harness.calls.submit(caller(), thread_post(&request)).await;
    let mut query = status_query(&request);
    assert!(matches!(
        harness.calls.request_status(caller(), query.clone()).await,
        RequestStatus::InOutbox { .. }
    ));
    query.scope = central_link_protocol::RequestScope::Organization;

    let calls = harness.calls.clone();
    let issued = query.clone();
    let mut lookup = tokio::spawn(async move { calls.request_status(caller(), issued).await });
    let frame = tokio::select! {
        frame = central.frame() => frame,
        early = &mut lookup => panic!("returned the local answer for the wrong scope: {early:?}"),
    };
    let LinkFrame::Data(DataFrame::RequestStatus {
        correlation_id,
        query: sent,
        ..
    }) = frame
    else {
        panic!("expected the organization replay read: {frame:?}")
    };
    assert_eq!(sent, query);
    central.send(LinkFrame::Data(DataFrame::RequestStatusReply {
        correlation_id,
        result: Box::new(RequestStatusAnswer::NotFound),
    }));
    assert_eq!(lookup.await.unwrap(), RequestStatus::NotFound);
    harness.stop().await;
}

#[tokio::test]
async fn an_absent_local_request_uses_the_caller_scoped_central_replay_read() {
    let mut harness = Harness::start(tempfile::tempdir().unwrap(), clock_at(MARK - 20_000)).await;
    let mut central = harness.connected().await;
    let request = uuid_v7(91);
    let query = status_query(&request);
    assert_eq!(
        harness.local_status(&request).await,
        RequestStatus::NotFound
    );

    let calls = harness.calls.clone();
    let issued = query.clone();
    let mut lookup = tokio::spawn(async move { calls.request_status(caller(), issued).await });
    let frame = tokio::select! {
        frame = central.frame() => frame,
        early = &mut lookup => panic!("returned before consulting central: {early:?}"),
    };
    let LinkFrame::Data(DataFrame::RequestStatus {
        correlation_id,
        caller: sent_caller,
        query: sent,
    }) = frame
    else {
        panic!("expected the scoped replay read: {frame:?}")
    };
    assert_eq!(sent_caller, caller());
    assert_eq!(sent, query);
    central.send(LinkFrame::Data(DataFrame::RequestStatusReply {
        correlation_id,
        result: Box::new(RequestStatusAnswer::Answered(reply())),
    }));
    assert_eq!(lookup.await.unwrap(), RequestStatus::Answered(reply()));

    let calls = harness.calls.clone();
    let mut lookup = tokio::spawn(async move { calls.request_status(caller(), query).await });
    let frame = tokio::select! {
        frame = central.frame() => frame,
        early = &mut lookup => panic!("returned before consulting central: {early:?}"),
    };
    assert!(matches!(
        frame,
        LinkFrame::Data(DataFrame::RequestStatus { .. })
    ));
    drop(central);
    assert_eq!(lookup.await.unwrap(), RequestStatus::StatusUnavailable);
    harness.stop().await;
}

#[tokio::test]
async fn message_status_makes_one_sender_scoped_list_read() {
    let mut harness = Harness::start(tempfile::tempdir().unwrap(), clock_at(MARK - 20_000)).await;
    let mut central = harness.connected().await;
    let message_id: workspace_model::MessageId = uuid_v7(92).parse().unwrap();
    let issued = message_id.clone();
    let calls = harness.calls.clone();
    let mut lookup = tokio::spawn(async move { calls.message_status(caller(), issued).await });
    let frame = tokio::select! {
        frame = central.frame() => frame,
        early = &mut lookup => panic!("returned before the sender-scoped list read: {early:?}"),
    };
    let LinkFrame::Data(DataFrame::Request { request }) = frame else {
        panic!("expected one List request, got {frame:?}");
    };
    assert_eq!(request.caller(), &caller());
    let central_link_protocol::RequestPayload::Workspace(payload) = request.payload() else {
        panic!("expected a workspace read");
    };
    assert!(payload.request_id.is_none());
    assert!(payload.workspace_id.is_none());
    assert!(matches!(&payload.operation, WorkspaceOperation::List {
        query: ListQuery::MessageStatus { message_id: sent }, ..
    } if sent == &message_id));
    central.send(LinkFrame::Data(DataFrame::Reply {
        correlation_id: request.correlation_id().clone(),
        result: Box::new(ReplyPayload::Workspace(WorkspaceReply {
            result: WorkspaceResult::Done(WorkspaceAnswer::MessageStatus {
                status: MessageStatus::Delivered,
            }),
            actors: Vec::new(),
        })),
    }));
    assert_eq!(lookup.await.unwrap(), Ok(Some(MessageStatus::Delivered)));
    assert!(central.from_router.try_recv().is_err());
    harness.stop().await;
}
