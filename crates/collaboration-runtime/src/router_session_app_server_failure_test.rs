//! Native RPC replies keep their established error codes and messages.

use super::*;
use collaboration_contracts::CommandFailure;
use collaboration_protocol::{
    ConversationOperationFailure, ConversationOperationFailureKind as FailureKind,
    ConversationOperationFailureStage, ProviderOperationEffect,
};

#[tokio::test]
async fn native_app_server_keeps_its_existing_restore_and_steer_errors() {
    let backend = ScriptedSessionBackend::new().expect("session backend");
    backend
        .durable_inventory
        .lock()
        .expect("inventory")
        .push(HubSessionSummary {
            session: backend.session.clone(),
            approver: backend.approver.clone(),
            working_directory: PathBuf::from("/tmp"),
            updated_at_seconds: 1,
            preview: String::new(),
            name: None,
            model: None,
            mode: None,
            state: SessionState::Unloaded,
        });
    let context = Arc::new(RouterSessionAppServerContext::new(
        backend.endpoint.clone(),
        backend.approver.clone(),
        backend.clone(),
        backend.clone(),
        super::test_support::model_catalog(tokio::sync::watch::channel(Vec::new()).1),
    ));
    let failure = |kind| {
        Box::new(ConversationOperationFailure {
            kind,
            stage: ConversationOperationFailureStage::Admission,
            effect: ProviderOperationEffect::None,
            message: "original provider guidance"
                .to_owned()
                .try_into()
                .expect("message"),
            operation_id: None,
            invalid_setting: None,
            provider_code: None,
            target: None,
            endpoint: None,
            availability: None,
        })
    };
    let thread = thread_alias(&backend.session);

    for (kind, code, message) in [
        (
            FailureKind::ProviderSessionNotFound,
            -32002,
            "thread not found",
        ),
        (
            FailureKind::OutcomeUnknown,
            -32000,
            "provider session unavailable",
        ),
    ] {
        *backend.load_failure.lock().expect("load failure") =
            Some(CommandFailure::ProviderOperation(failure(kind)));
        let reply = handle_app_server_rpc(
            context.clone(),
            json!({"id":1,"method":"thread/resume",
            "params":{"threadId":thread}}),
        )
        .await
        .expect("resume RPC");
        assert_eq!(
            reply.response,
            json!({"id":1,"error":{"code":code,"message":message}})
        );
    }

    *backend.steer_failure.lock().expect("steer failure") = Some(
        CommandFailure::ProviderSteerOperation(failure(FailureKind::CannotSteer)),
    );
    let reply = handle_app_server_rpc(
        context,
        json!({"id":2,"method":"turn/steer","params":{
            "threadId":thread,"expectedTurnId":"turn-1","input":[{"type":"text","text":"steer"}]
        }}),
    )
    .await
    .expect("steer RPC");
    assert_eq!(
        reply.response,
        json!({"id":2,"error":{
            "code":-32000,"message":"provider operation is unsupported"
        }})
    );
}
