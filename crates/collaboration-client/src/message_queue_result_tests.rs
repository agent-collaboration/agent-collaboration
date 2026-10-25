//! Real local API transport with scripted custody answers; no central admission claim.
use super::*;
use collaboration_protocol::{MessageSendResult, SessionMessageReplyParams};

const REMOTE_SERVICE: &str = "00000000-0000-4000-8000-000000000099";

fn remote_request() -> MessageSendRequest {
    let mut request = fixture_request(None);
    request.target.endpoint.service_id = REMOTE_SERVICE.to_owned().try_into().unwrap();
    request.item_id = Some(PUSH_ID.to_owned().try_into().unwrap());
    request
}

fn queued_answer(request: &MessageSendRequest) -> Value {
    json!({"queued":{"item":PUSH_ID,"target":request.target,"targetIdentity":"remote recipient"}})
}

#[tokio::test]
async fn remote_custody_answer_keeps_the_item_and_target_without_native_evidence_or_replay() {
    // Arrange: the API response names the item supplied before submission.
    let request = remote_request();
    let response = queued_answer(&request);
    // Act: one real API call, protected by the existing no-replay completion barrier.
    let result = single_send_result(request.clone(), response.clone())
        .await
        .unwrap();
    // Assert: custody is a separate variant and cannot invent a push, link or receipt.
    let MessageSendResult::Queued { queued } = &result else {
        panic!("remote custody became local delivery")
    };
    assert_eq!(Some(&queued.item), request.item_id.as_ref());
    assert_eq!(queued.target, request.target);
    assert_eq!(serde_json::to_value(result).unwrap(), response);
}

#[tokio::test]
async fn queued_answer_mismatches_and_malformed_fields_keep_acceptance_unknown_without_replay() {
    #[derive(Clone, Copy, Debug)]
    enum QueuedAnswerDefect {
        Item,
        Target,
        EmptyLabel,
        LocalTarget,
        InventedReceipt,
        InventedPush,
    }
    for defect in [
        QueuedAnswerDefect::Item,
        QueuedAnswerDefect::Target,
        QueuedAnswerDefect::EmptyLabel,
        QueuedAnswerDefect::LocalTarget,
        QueuedAnswerDefect::InventedReceipt,
        QueuedAnswerDefect::InventedPush,
    ] {
        let mut request = remote_request();
        let mut response = queued_answer(&request);
        match defect {
            QueuedAnswerDefect::Item => response["queued"]["item"] = json!(OTHER_PUSH_ID),
            QueuedAnswerDefect::Target => {
                response["queued"]["target"]["sessionId"] = json!("other")
            }
            QueuedAnswerDefect::EmptyLabel => response["queued"]["targetIdentity"] = json!(" "),
            QueuedAnswerDefect::LocalTarget => {
                request.target.endpoint.service_id = SERVICE_ID.to_owned().try_into().unwrap();
                response["queued"]["target"] = json!(request.target);
            }
            QueuedAnswerDefect::InventedReceipt => {
                response["queued"]["receipt"] = json!({"outcome":{"kind":"queued"}})
            }
            QueuedAnswerDefect::InventedPush => response["pushId"] = json!(PUSH_ID),
        }
        let error = single_send_result(request.clone(), response)
            .await
            .expect_err(&format!("{defect:?}"));
        let (failure, target) = error.into_operation_failure_and_target();
        assert_eq!(
            failure.effect,
            crate::OperationEffect::Unknown,
            "{defect:?}"
        );
        assert_eq!(target, Some(request.target), "{defect:?}");
    }
}

#[tokio::test]
async fn remote_target_cannot_be_reported_as_a_local_native_receipt() {
    let request = remote_request();
    let response = push_result(&request.target, PUSH_ID, PUSH_ID);
    assert_inconsistent_response(request, response).await;
}

#[tokio::test]
async fn reply_accepts_the_same_queued_custody_shape() {
    let (_api, client, mut calls) = connected_client().await;
    let response = queued_answer(&remote_request());
    let expected = response.clone();
    let peer = tokio::spawn(async move {
        let call = calls.next().await.unwrap();
        assert_eq!(call.tool, "message_reply");
        assert_eq!(call.arguments["reference"], PUSH_ID);
        call.succeed(response);
    });
    let result = client
        .message_reply(SessionMessageReplyParams {
            caller: fixture_request(None).target,
            reference: PUSH_ID.to_owned(),
            text: "reply".to_owned().try_into().unwrap(),
        })
        .await
        .unwrap();
    assert!(matches!(&result, MessageSendResult::Queued { .. }));
    assert_eq!(serde_json::to_value(result).unwrap(), expected);
    peer.await.unwrap();
}
