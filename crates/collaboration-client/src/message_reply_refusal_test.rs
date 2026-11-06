use super::{MessageReplyError, MessageReplyRequest};
use crate::{CollaborationClient, OperationEffect, scripted_api::ScriptedApi};
use serde_json::json;

#[tokio::test]
async fn too_old_reply_is_a_decided_refusal_with_no_effect_or_caller() {
    let (api, mut calls) = ScriptedApi::start().unwrap();
    let client = CollaborationClient::connect(api.directory(), "old-reply", "1")
        .await
        .unwrap();
    let answering = tokio::spawn(async move {
        let request = calls.next().await.unwrap();
        assert_eq!(request.tool, "message_reply");
        request.fail(json!({
            "kind":"rejected","code":-32602,
            "data":{"kind":"tooOld","stage":"discovery","message":"Item is older than the 60-day limit"}
        }));
    });
    let request: MessageReplyRequest = serde_json::from_value(json!({
        "caller":{"endpoint":{"serviceId":client.identity().service_id,"endpointId":"codex-local"},"sessionId":"old-reply"},
        "reference":"019f0000-0000-7000-8000-000000000101","text":"reply"
    })).unwrap();

    let refusal = client.reply_to_push(request).await.unwrap_err();

    assert!(
        matches!(refusal, MessageReplyError::Preparation(_)),
        "{refusal:?}"
    );
    let (failure, caller) = refusal.into_operation_failure_and_caller();
    assert_eq!(failure.service_kind.as_deref(), Some("tooOld"));
    assert_eq!(failure.effect, OperationEffect::None);
    assert!(caller.is_none());
    answering.await.unwrap();
}
