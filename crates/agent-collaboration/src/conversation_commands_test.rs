use super::{
    ConversationArguments, ConversationCommand, conversation_end_exit, operation_failure_exit,
    resolve_current_session_ref, settlement_record,
};
use clap::Parser;
use collaboration_client::{
    ClientError, OperationEffect, OperationFailureKind, operation_failure_from_client_error,
};

#[test]
fn conversation_exit_preserves_rejection_and_post_dispatch_unknown_precedence() {
    let rejection = operation_failure_from_client_error(
        ClientError::Rejected {
            code: -32603,
            data: Some(serde_json::json!({"kind":"nativeRejected"})),
        },
        OperationEffect::Unknown,
    );
    assert_eq!(rejection.kind, OperationFailureKind::Rejected);
    assert_eq!(operation_failure_exit(&rejection), 4);

    let malformed = operation_failure_from_client_error(
        ClientError::Protocol("malformed result"),
        OperationEffect::Unknown,
    );
    assert_eq!(operation_failure_exit(&malformed), 5);
}

#[test]
fn create_is_a_standalone_machine_composable_command() {
    let parsed = ConversationArguments::try_parse_from([
        "agent-collaboration conversation",
        "create",
        "--endpoint",
        "codex-local",
        "--cwd",
        "/tmp/project",
        "--model",
        "gpt-5.6-sol",
        "--effort",
        "low",
        "--access",
        "workspace-write",
        "--json",
    ])
    .expect("standalone create arguments");
    assert!(matches!(parsed.command, ConversationCommand::Create(_)));
}

#[test]
fn provider_subcommand_is_removed_after_common_cutover() {
    assert!(
        ConversationArguments::try_parse_from([
            "agent-collaboration conversation",
            "provider",
            "create",
        ])
        .is_err()
    );
}

#[test]
fn provider_lifecycle_commands_accept_exact_targets_and_operation_ids() {
    let resume = ConversationArguments::try_parse_from([
        "agent-collaboration conversation",
        "resume",
        "--target",
        "target-json",
        "--cwd",
        "/tmp/project",
        "--access",
        "workspace-write",
        "--operation-id",
        "019f0000-0000-7000-8000-000000000011",
    ])
    .expect("resume arguments");
    assert!(matches!(resume.command, ConversationCommand::Resume(_)));
    let close = ConversationArguments::try_parse_from([
        "agent-collaboration conversation",
        "close",
        "--target",
        "target-json",
        "--operation-id",
        "019f0000-0000-7000-8000-000000000012",
    ])
    .expect("close arguments");
    assert!(matches!(close.command, ConversationCommand::Close(_)));
}

#[test]
fn create_accepts_from_session_ref_override() {
    let parsed = ConversationArguments::try_parse_from([
        "agent-collaboration conversation",
        "create",
        "--endpoint",
        "codex-local",
        "--cwd",
        "/tmp/project",
        "--model",
        "gpt-5.6-sol",
        "--effort",
        "low",
        "--access",
        "workspace-write",
        "--from",
        r#"{"endpoint":{"serviceId":"018f47d2-24d5-7a68-b9ec-6f759c39458f","endpointId":"codex-local"},"sessionId":"cursor-conversation"}"#,
        "--json",
    ])
    .expect("create with --from");
    match parsed.command {
        ConversationCommand::Create(args) => {
            assert_eq!(
                args.from.as_deref(),
                Some(
                    r#"{"endpoint":{"serviceId":"018f47d2-24d5-7a68-b9ec-6f759c39458f","endpointId":"codex-local"},"sessionId":"cursor-conversation"}"#
                )
            );
        }
        ConversationCommand::RemoteStatus(_)
        | ConversationCommand::Prompt(_)
        | ConversationCommand::Load(_)
        | ConversationCommand::Resume(_)
        | ConversationCommand::Close(_)
        | ConversationCommand::Cancel(_)
        | ConversationCommand::Settings(_)
        | ConversationCommand::Operation { .. } => {
            panic!("create parse selected another command")
        }
    }
}

#[test]
fn from_overrides_missing_and_present_env_identities() {
    let service_id = "018f47d2-24d5-7a68-b9ec-6f759c39458f"
        .to_owned()
        .try_into()
        .expect("service id");
    let from = r#"{"endpoint":{"serviceId":"018f47d2-24d5-7a68-b9ec-6f759c39458f","endpointId":"codex-local"},"sessionId":"cursor-conversation"}"#;
    let harness = |pairs: &'static [(&'static str, &'static str)]| {
        move || {
            crate::current_session_identity::resolve_harness_session_identity(|name| {
                pairs
                    .iter()
                    .find(|(variable, _)| *variable == name)
                    .map(|(_, value)| std::ffi::OsString::from(*value))
            })
        }
    };
    let resolved = resolve_current_session_ref(&service_id, Some(from), harness(&[]))
        .expect("override without env");
    assert_eq!(String::from(resolved.session_id), "cursor-conversation");
    let still_override = resolve_current_session_ref(
        &service_id,
        Some(from),
        harness(&[("CODEX_THREAD_ID", "codex-thread")]),
    )
    .expect("override wins over env");
    assert_eq!(
        String::from(still_override.session_id),
        "cursor-conversation"
    );
    assert!(resolve_current_session_ref(&service_id, None, harness(&[])).is_err());
    let implicit = resolve_current_session_ref(
        &service_id,
        None,
        harness(&[("CURSOR_CONVERSATION_ID", "cursor-thread")]),
    )
    .expect("implicit Cursor env");
    assert_eq!(String::from(implicit.endpoint.endpoint_id), "cursor-local");
    assert_eq!(String::from(implicit.session_id), "cursor-thread");
}

#[test]
fn cancelled_acp_settlement_without_success_settings_keeps_target_and_terminal_exit() {
    let target: collaboration_client::protocol::SessionRef = serde_json::from_value(
        serde_json::json!({
            "endpoint":{"serviceId":"018f47d2-24d5-7a68-b9ec-6f759c39458f","endpointId":"codex-local"},
            "sessionId":"cancelled-thread"
        }),
    )
    .expect("target");
    let receipt = serde_json::json!({
        "stopReason":"cancelled",
        "_meta":{"codexRouter":{"interruption":{"requested":true}}}
    });
    let record = settlement_record(
        target.clone(),
        collaboration_client::protocol::ConversationTerminalReason::Cancelled,
        receipt.clone(),
    );
    let record = serde_json::to_value(record).expect("settlement JSON");
    assert_eq!(
        record["target"],
        serde_json::to_value(target).expect("target JSON")
    );
    assert_eq!(record["result"], receipt);
    assert_eq!(record["terminalReason"], "cancelled");
    assert_eq!(
        conversation_end_exit(collaboration_client::ConversationEnd::Cancelled),
        130
    );
    assert_eq!(
        conversation_end_exit(collaboration_client::ConversationEnd::TimedOut),
        124
    );
    assert_eq!(
        conversation_end_exit(collaboration_client::ConversationEnd::Detached),
        0
    );
}
