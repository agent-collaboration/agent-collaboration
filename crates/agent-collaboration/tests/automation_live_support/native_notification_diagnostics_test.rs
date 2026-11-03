use super::*;

pub(crate) fn retains_retry_class_without_provider_body_or_unrelated_thread_data() {
    let message = json!({"method":"error","params":{"threadId":"owned",
        "turnId":"turn","willRetry":true,"error":{"message":"SECRET_CANARY",
        "additionalDetails":"PRIVATE_BODY","codexErrorInfo":{"httpConnectionFailed":{"httpStatusCode":503}}}}});
    assert_eq!(
        for_target(&message, "owned"),
        Some(json!({"threadId":"owned",
        "errorKind":"httpConnectionFailed","httpStatusCode":503,"willRetry":true,"detailHints":[]}))
    );
    assert!(for_target(&message, "another-thread").is_none());
}

pub(crate) fn unknown_provider_payload_is_not_copied_into_diagnostics() {
    let message = json!({"method":"error","params":{"threadId":"owned",
        "error":{"codexErrorInfo":{"PRIVATE_KEY":"PRIVATE_VALUE"}},"willRetry":false}});
    assert_eq!(
        for_target(&message, "owned"),
        Some(json!({"threadId":"owned",
        "errorKind":"unclassified","httpStatusCode":null,"willRetry":false,"detailHints":[]}))
    );
}

pub(crate) fn underlying_transport_hints_do_not_retain_urls_or_error_bodies() {
    let message = json!({"method":"error","params":{"threadId":"owned","willRetry":true,
        "error":{"codexErrorInfo":"other","additionalDetails":"websocket closed by server before response.completed: PRIVATE_URL PRIVATE_TOKEN"}}});
    assert_eq!(
        for_target(&message, "owned"),
        Some(json!({"threadId":"owned",
        "errorKind":"other","httpStatusCode":null,"willRetry":true,
        "detailHints":["serverClosed","missingCompletionEvent","websocket"]}))
    );
}
