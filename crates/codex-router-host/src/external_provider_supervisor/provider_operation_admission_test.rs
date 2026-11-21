//! Which stored operations count as the same request (D1 R15), and what an
//! admission blocked by earlier sends answers (D1 M8).

use super::super::provider_admission_policy::AdmissionBlockers;
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

/// Oracle: D1 spec R14 — a send refused before anything reached the agent
/// leaves its row waiting (Admitted, no effect, no dispatch time), so a
/// restart finds it "admitted, never sent"; a settled row is never reopened.
#[tokio::test]
async fn a_refused_send_returns_its_row_to_admitted_and_never_reopens_a_settled_one() {
    use collaboration_runtime::{ProviderOperationAdmission, ProviderOperationStore};
    let root = tempfile::tempdir().expect("store root");
    let mut store = ProviderOperationStore::open(&root.path().join("operations.sqlite"))
        .await
        .expect("operation store");
    let admit = |operation_id: OperationId| ProviderOperationAdmission {
        operation_id,
        operation_kind: ProviderOperationKind::ConversationPrompt,
        binding: ConversationBindingIdentity::ExternalProvider {
            binding: crate::external_provider_supervisor::tests::binding(),
        },
        admitted_at_ms: 1,
    };
    let waiting = OperationId::generate();
    store.admit(admit(waiting.clone())).await.expect("admit");
    store
        .mark_may_have_dispatched(&waiting, 2)
        .await
        .expect("mark");
    let settled = OperationId::generate();
    store.admit(admit(settled.clone())).await.expect("admit");
    store
        .mark_may_have_dispatched(&settled, 2)
        .await
        .expect("mark");
    store
        .record_terminal(
            &settled,
            ProviderOperationEffect::Applied,
            ProviderReconciliationState::Confirmed,
            None,
            3,
        )
        .await
        .expect("settle");

    let unmarked = store.unmark_undispatched(&waiting, 4).await;
    let reopened = store.unmark_undispatched(&settled, 4).await;

    let unmarked = unmarked.expect("a refused send unmarks");
    assert_eq!(unmarked.stage, ProviderOperationStage::Admitted);
    assert_eq!(unmarked.effect, ProviderOperationEffect::None);
    assert_eq!(unmarked.dispatched_at_ms, None);
    assert!(
        reopened.is_err(),
        "a settled row stays settled: {reopened:?}"
    );
    assert_eq!(
        store
            .inspect(&settled)
            .await
            .expect("inspect")
            .expect("row")
            .stage,
        ProviderOperationStage::Terminal
    );
}

/// Oracle: M8 and §14.1, written by hand: an unknown outcome's next step is
/// reconcile; a send still in flight is waited for, not reconciled; nothing
/// blocking answers nothing.
#[test]
fn a_blocked_admission_names_each_blocking_state_and_its_step() {
    // Arrange
    let unknown = OperationId::generate();
    let in_flight = OperationId::generate();
    let (u, f) = (unknown.as_str().to_owned(), in_flight.as_str().to_owned());
    let only_unknown = AdmissionBlockers {
        unknown: vec![unknown.clone()],
        in_flight: Vec::new(),
    };
    let only_in_flight = AdmissionBlockers {
        unknown: Vec::new(),
        in_flight: vec![in_flight.clone()],
    };
    let both = AdmissionBlockers {
        unknown: vec![unknown],
        in_flight: vec![in_flight],
    };

    // Act
    let answers = [
        only_unknown.answer(),
        only_in_flight.answer(),
        both.answer(),
        AdmissionBlockers::default().answer(),
    ];

    // Assert
    assert_eq!(
        answers,
        [
            Some(format!(
                "an earlier prompt's outcome is unknown; check the session, then run operation reconcile for {u}"
            )),
            Some(format!("an earlier send is still in flight; wait for {f}")),
            Some(format!(
                "an earlier prompt's outcome is unknown; check the session, then run operation reconcile for {u}; \
                 an earlier send is still in flight; wait for {f}"
            )),
            None,
        ]
    );
}
