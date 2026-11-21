use super::*;
use futures_util::FutureExt as _;
use hegel::generators as gs;

#[hegel::test(seed = Some(2911802), test_cases = 100)]
fn release_before_the_queue_waiter_polls_still_wakes_it(case: hegel::TestCase) {
    let binding = BindingUncertainty::default();
    let mut watcher = binding.subscribe_eligibility();
    let before = *watcher.borrow_and_update();
    for _ in 0..case.draw(gs::integers::<u8>().min_value(1).max_value(16)) {
        binding.eligibility_changed();
    }
    assert!(matches!(watcher.changed().now_or_never(), Some(Ok(()))));
    assert_ne!(*watcher.borrow_and_update(), before);
}

#[hegel::test(seed = Some(2912201), test_cases = 100)]
fn guarded_refresh_tracks_every_committed_blocker(case: hegel::TestCase) {
    let operation = OperationId::generate();
    let unrelated = OperationId::generate();
    let identity = ProviderBindingIdentity {
        endpoint: collaboration_protocol::EndpointRef {
            service_id: "0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89"
                .to_owned()
                .try_into()
                .unwrap(),
            endpoint_id: "cursor-local".to_owned().try_into().unwrap(),
        },
        binding_id: "test-binding".to_owned().try_into().unwrap(),
        runtime: collaboration_protocol::ProviderRuntimeIdentity {
            provider: collaboration_protocol::ProviderKind::Cursor,
            runtime_name: "cursor".to_owned().try_into().unwrap(),
            runtime_version: None,
        },
        transport: collaboration_protocol::ProviderTransport::StdioAcp,
        generation: collaboration_protocol::CodexGeneration {
            service_epoch: "1ff962c5-7fa3-4c18-a5ca-1bbe8db09e80"
                .to_owned()
                .try_into()
                .unwrap(),
            generation: 1.try_into().unwrap(),
        },
        capabilities: vec![collaboration_protocol::ProviderCapability {
            name: collaboration_protocol::ProviderCapabilityName::Create,
            status: collaboration_protocol::ProviderCapabilityStatus::Supported,
            evidence: collaboration_protocol::ProviderCapabilityEvidence::Advertised,
        }]
        .try_into()
        .unwrap(),
    };
    let target = SessionRef {
        endpoint: identity.endpoint.clone(),
        session_id: "same-session".to_owned().try_into().unwrap(),
    };
    let cell = BindingUncertainty::default();
    cell.identity.set(identity.clone()).unwrap();
    assert!(
        cell.outcomes
            .set(Mutex::new(BindingOutcomes::default()))
            .is_ok()
    );
    let outcomes = cell.outcomes.get().unwrap();
    lock(outcomes).record_unknown(unrelated.clone(), None, UnknownRecovery::Blocks);
    let mut row = ProviderOperationRecord {
        operation_id: operation.clone(),
        operation_kind: ProviderOperationKind::ConversationPrompt,
        binding: ConversationBindingIdentity::ExternalProvider { binding: identity },
        target: Some(target.clone()),
        stage: ProviderOperationStage::Admitted,
        effect: ProviderOperationEffect::None,
        reconciliation_state: ProviderReconciliationState::Unresolved,
        admitted_at_ms: 0,
        dispatched_at_ms: None,
        terminal_at_ms: None,
        terminal_stop_reason: None,
        updated_at_ms: 0,
    };
    for _ in 0..case.draw(gs::integers::<u8>().min_value(1).max_value(20)) {
        let (stage, effect, reconciliation, blocks) =
            match case.draw(gs::integers::<u8>().max_value(4)) {
                0 => (
                    ProviderOperationStage::Admitted,
                    ProviderOperationEffect::None,
                    ProviderReconciliationState::Unresolved,
                    false,
                ),
                1 => (
                    ProviderOperationStage::MayHaveDispatched,
                    ProviderOperationEffect::Unknown,
                    ProviderReconciliationState::Unresolved,
                    true,
                ),
                2 => (
                    ProviderOperationStage::Terminal,
                    ProviderOperationEffect::Unknown,
                    ProviderReconciliationState::Unresolved,
                    true,
                ),
                3 => (
                    ProviderOperationStage::Terminal,
                    ProviderOperationEffect::None,
                    ProviderReconciliationState::Confirmed,
                    false,
                ),
                _ => (
                    ProviderOperationStage::Terminal,
                    ProviderOperationEffect::Unknown,
                    ProviderReconciliationState::NotReconcilable,
                    false,
                ),
            };
        row.stage = stage;
        row.effect = effect;
        row.reconciliation_state = reconciliation;
        row.target = case.draw(gs::booleans()).then_some(target.clone());
        cell.refresh_durable_row(&row, None);
        let outcomes = lock(outcomes);
        let same_session = outcomes.blockers(Some(&target), false);
        let binding_wide = outcomes.blockers(None, false);
        assert_eq!(same_session.contains(&operation), blocks);
        assert_eq!(
            binding_wide.contains(&operation),
            blocks && row.target.is_none()
        );
        assert!(same_session.contains(&unrelated));
    }
}
