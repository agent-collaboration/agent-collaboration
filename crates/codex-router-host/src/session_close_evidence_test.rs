use super::*;
use crate::session_store_lane::tests::StoreFixture;
use collaboration_protocol::{ConversationBindingIdentity, OperationId, SessionRef};
use collaboration_runtime::{ProviderOperationAdmission, ProviderOperationStore};

async fn admit(
    store: &mut ProviderOperationStore,
    binding: &collaboration_protocol::ProviderBindingIdentity,
    target: &SessionRef,
    kind: ProviderOperationKind,
) -> OperationId {
    let operation = OperationId::generate();
    store
        .admit(ProviderOperationAdmission {
            operation_id: operation.clone(),
            operation_kind: kind,
            binding: ConversationBindingIdentity::ExternalProvider {
                binding: binding.clone(),
            },
            admitted_at_ms: 0,
        })
        .await
        .expect("admit");
    store
        .record_target(&operation, target, 0)
        .await
        .expect("persist target");
    operation
}

#[tokio::test]
async fn all_close_outcomes_keep_local_disposition_and_remote_effect_separate_on_reopen() {
    use ProviderOperationEffect::{Applied, None as NoEffect, Unknown};
    use ProviderReconciliationState::{Confirmed, Unresolved};

    let fixture = StoreFixture::new().await;
    let target = fixture.record_session("close-results").await;
    let cases = [
        (CloseOutcome::Confirmed, true, Applied, Confirmed, "closed"),
        (
            CloseOutcome::RouterOnly,
            true,
            NoEffect,
            Confirmed,
            "closed by the Router; this harness has no close, so it wasn't told",
        ),
        (
            CloseOutcome::DrainExpired,
            true,
            NoEffect,
            Confirmed,
            "the close wasn't sent because the running turn didn't stop in time; the Router released the session locally, and the harness may still be running that turn",
        ),
        (
            CloseOutcome::NotEnqueued,
            true,
            NoEffect,
            Confirmed,
            "the Router couldn't send the close; it released the session locally (the harness wasn't told), and you can restore it",
        ),
        (
            CloseOutcome::RefusedLive,
            false,
            NoEffect,
            Confirmed,
            "the close was refused; the session is still open",
        ),
        (
            CloseOutcome::RefusedLoading,
            true,
            NoEffect,
            Confirmed,
            "the close was refused while the session was loading; the Router released it locally, and you can restore it",
        ),
        (
            CloseOutcome::OutcomeUnknown,
            true,
            Unknown,
            Unresolved,
            "the close may or may not have reached the harness; the Router released the session locally, and you can restore it",
        ),
    ];
    let mut store = fixture.store.lock().await;
    let mut observations = Vec::new();
    for (outcome, local, effect, reconciliation, summary) in cases {
        let result = classify_close(outcome);
        assert_eq!(
            result,
            CloseSettlementEvidence {
                locally_retired: local,
                effect,
                reconciliation,
                summary,
            }
        );
        let operation = admit(
            &mut store,
            &fixture.binding,
            &target,
            ProviderOperationKind::ConversationClose,
        )
        .await;
        if !matches!(
            outcome,
            CloseOutcome::RouterOnly | CloseOutcome::DrainExpired | CloseOutcome::NotEnqueued
        ) {
            store
                .mark_may_have_dispatched(&operation, 1)
                .await
                .expect("close reached transport");
        }
        store
            .record_terminal(&operation, result.effect, result.reconciliation, None, 2)
            .await
            .expect("persist classification");
        observations.push((operation, effect, reconciliation));
    }
    drop(store);

    let mut reopened = ProviderOperationStore::open(&fixture.path)
        .await
        .expect("store reopen");
    for (operation, effect, reconciliation) in observations {
        let row = reopened
            .inspect(&operation)
            .await
            .expect("reopened read")
            .expect("close row");
        assert_eq!(
            (row.stage, row.effect, row.reconciliation_state),
            (ProviderOperationStage::Terminal, effect, reconciliation)
        );
    }
}

#[tokio::test]
async fn incomplete_evidence_close_recovers_only_after_exact_retirement_or_reopen() {
    let fixture = StoreFixture::new().await;
    let target = fixture.record_session("restore-admission").await;
    let mut store = fixture.store.lock().await;
    let admitted = admit(
        &mut store,
        &fixture.binding,
        &target,
        ProviderOperationKind::ConversationClose,
    )
    .await;
    let nonterminal = admit(
        &mut store,
        &fixture.binding,
        &target,
        ProviderOperationKind::ConversationClose,
    )
    .await;
    let terminal = admit(
        &mut store,
        &fixture.binding,
        &target,
        ProviderOperationKind::ConversationClose,
    )
    .await;
    let reconciled = admit(
        &mut store,
        &fixture.binding,
        &target,
        ProviderOperationKind::ConversationClose,
    )
    .await;
    let prompt = admit(
        &mut store,
        &fixture.binding,
        &target,
        ProviderOperationKind::ConversationPrompt,
    )
    .await;
    for operation in [&nonterminal, &terminal, &reconciled, &prompt] {
        store
            .mark_may_have_dispatched(operation, 1)
            .await
            .expect("dispatch marked");
    }
    store
        .record_terminal(
            &terminal,
            ProviderOperationEffect::Unknown,
            ProviderReconciliationState::Unresolved,
            None,
            2,
        )
        .await
        .expect("legacy terminal close");
    store
        .record_not_reconcilable(&reconciled, 2)
        .await
        .expect("reconciled close");
    let live = store
        .stored_uncertain_operations(&fixture.binding)
        .await
        .expect("live evidence");
    for row in &live {
        assert_eq!(
            classify_restore_uncertainty(row, RestoreCloseEvidence::RunStillPresent),
            RestoreUncertainty::BlocksRestore
        );
        let expected = if row.operation == prompt {
            RestoreUncertainty::BlocksRestore
        } else {
            RestoreUncertainty::EarlierCloseIncomplete
        };
        assert_eq!(
            classify_restore_uncertainty(row, RestoreCloseEvidence::ExactRunRetired),
            expected
        );
    }
    drop(store);

    let mut reopened = ProviderOperationStore::open(&fixture.path)
        .await
        .expect("store reopen");
    let after = reopened
        .stored_uncertain_operations(&fixture.binding)
        .await
        .expect("reopened evidence");
    assert_eq!(live, after);
    assert_eq!(after.len(), 4);
    assert!(
        !after.iter().any(|row| row.operation == admitted),
        "Admitted/None stays outside incomplete evidence"
    );
    for row in after {
        let expected = if row.operation == prompt {
            RestoreUncertainty::BlocksRestore
        } else {
            RestoreUncertainty::EarlierCloseIncomplete
        };
        assert_eq!(
            classify_restore_uncertainty(&row, RestoreCloseEvidence::ReopenedStore),
            expected
        );
    }
    assert_eq!(
        INCOMPLETE_CLOSE_NOTE,
        "an earlier close may have reached the harness; restoring anyway"
    );
}
