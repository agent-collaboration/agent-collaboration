use super::*;
use collaboration_protocol::ConversationOperationFailure;
use provider_session_machines::session_lifecycle_machine::RestoreVia;

async fn restore_failure(
    adapter: Arc<dyn acp_client_runtime::ProviderBehavior>,
    reply: &str,
    via: RestoreVia,
) -> ConversationOperationFailure {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host_with_adapter(&[reply], adapter).await;
    let operation = OperationId::generate();
    let method = match via {
        RestoreVia::Load => {
            host.load(rig.load_request(operation.clone()))
                .await
                .expect("load admitted");
            "session/load"
        }
        RestoreVia::Resume => {
            host.resume(rig.resume_request(operation.clone()))
                .await
                .expect("resume admitted");
            "session/resume"
        }
    };
    let failure = host
        .wait(wait_request(operation))
        .await
        .expect_err("restore rejected by fixture");
    assert_eq!(rig.wire_receipt()["restores"][0]["method"], method);
    assert_eq!(failure.provider_code, Some(-32602));
    assert_eq!(failure.effect, ProviderOperationEffect::None);
    host.shutdown().await.expect("fixture shutdown");
    failure
}

#[tokio::test]
async fn cursor_missing_session_invalid_params_reaches_owner_as_lost_on_load_and_resume() {
    for via in [RestoreVia::Load, RestoreVia::Resume] {
        let failure = restore_failure(
            Arc::new(acp_provider_adapters::CursorAcpBehavior),
            "restore-session-not-found",
            via,
        )
        .await;
        let message = String::from(failure.message);
        assert_eq!(
            failure.kind,
            ConversationOperationFailureKind::ProviderSessionNotFound,
            "{message}"
        );
        let reference = message
            .strip_prefix("the provider no longer knows this session, so start a new session with the same setup (provider code -32602; reference ")
            .and_then(|rest| rest.strip_suffix(')'))
            .expect("lost-session answer with diagnostic reference");
        assert_eq!(
            uuid::Uuid::parse_str(reference)
                .expect("diagnostic UUID")
                .get_version_num(),
            7
        );
    }
}

#[tokio::test]
async fn cursor_other_invalid_params_restore_replies_stay_invalid_parameters() {
    for reply in ["restore-other-session-not-found", "restore-invalid-cwd"] {
        let failure = restore_failure(
            Arc::new(acp_provider_adapters::CursorAcpBehavior),
            reply,
            RestoreVia::Load,
        )
        .await;
        assert_invalid_params(failure);
    }
}

#[tokio::test]
async fn missing_session_detail_does_not_reclassify_non_cursor_invalid_params() {
    let failure = restore_failure(
        Arc::new(acp_provider_adapters::StandardAcpBehavior),
        "restore-session-not-found",
        RestoreVia::Load,
    )
    .await;
    assert_invalid_params(failure);
}

fn assert_invalid_params(failure: ConversationOperationFailure) {
    let message = String::from(failure.message);
    assert_eq!(
        failure.kind,
        ConversationOperationFailureKind::InvalidRequest,
        "{message}"
    );
    assert!(message.starts_with("provider ACP parameters are invalid, so check the session setup before retrying (ACP code -32602; reference "), "{message}");
}
