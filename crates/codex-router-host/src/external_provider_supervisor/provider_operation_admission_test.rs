//! Which stored operations count as the same request (D1 R15).

use super::same_request;
use collaboration_protocol::{
    ConversationBindingIdentity, OperationId, ProviderOperationEffect, ProviderOperationKind,
    ProviderOperationStage, ProviderReconciliationState, SessionId, SessionRef,
};
use collaboration_runtime::ProviderOperationRecord;

fn record(kind: ProviderOperationKind, target: Option<SessionRef>) -> ProviderOperationRecord {
    ProviderOperationRecord {
        operation_id: OperationId::generate(),
        operation_kind: kind,
        binding: ConversationBindingIdentity::ExternalProvider {
            binding: crate::external_provider_supervisor::tests::binding(),
        },
        target,
        stage: ProviderOperationStage::Admitted,
        effect: ProviderOperationEffect::None,
        reconciliation_state: ProviderReconciliationState::Unresolved,
        admitted_at_ms: 1,
        dispatched_at_ms: None,
        terminal_at_ms: None,
        terminal_stop_reason: None,
        updated_at_ms: 1,
    }
}

fn session(id: &str) -> SessionRef {
    SessionRef {
        endpoint: crate::external_provider_supervisor::tests::binding().endpoint,
        session_id: SessionId::try_from(id.to_owned()).expect("session ID"),
    }
}

/// Oracle: D1 spec R15 — the same ID with the same kind and session is the
/// same request; a different kind or a different session is not.
#[test]
fn a_reused_id_matches_only_the_same_kind_and_session() {
    let prompt = record(
        ProviderOperationKind::ConversationPrompt,
        Some(session("a")),
    );
    assert!(same_request(
        &prompt,
        ProviderOperationKind::ConversationPrompt,
        Some(&session("a"))
    ));
    assert!(!same_request(
        &prompt,
        ProviderOperationKind::ConversationCancel,
        Some(&session("a"))
    ));
    assert!(!same_request(
        &prompt,
        ProviderOperationKind::ConversationPrompt,
        Some(&session("b"))
    ));
    let create = record(ProviderOperationKind::ConversationCreate, None);
    assert!(same_request(
        &create,
        ProviderOperationKind::ConversationCreate,
        None
    ));
    assert!(!same_request(
        &create,
        ProviderOperationKind::ConversationPrompt,
        None
    ));
}
