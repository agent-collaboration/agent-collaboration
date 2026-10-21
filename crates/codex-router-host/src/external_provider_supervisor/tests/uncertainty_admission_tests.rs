use super::*;

#[tokio::test]
async fn previous_generation_uncertainty_allows_admission() {
    for targetless in [false, true] {
        assert_uncertainty_admission(targetless, true).await;
    }
}

#[tokio::test]
async fn current_generation_uncertainty_names_all_blocking_operations() {
    for targetless in [false, true] {
        assert_uncertainty_admission(targetless, false).await;
    }
}

async fn assert_uncertainty_admission(targetless: bool, previous_generation: bool) {
    // Arrange: genuine stored terminal Unknown operations, not fabricated outcomes.
    let root = tempfile::tempdir().unwrap();
    let runtime = ExternalProviderRuntime::initialize(create_fixture())
        .await
        .unwrap();
    let mut store = ProviderOperationStore::open(&root.path().join("operations.sqlite"))
        .await
        .unwrap();
    let target = SessionRef {
        endpoint: endpoint(),
        session_id: SessionId::try_from("uncertain-session".to_owned()).unwrap(),
    };
    let mut unknown_rows = Vec::new();
    for _ in 0..2 {
        let operation_id = OperationId::generate();
        store
            .admit(ProviderOperationAdmission {
                operation_id: operation_id.clone(),
                operation_kind: ProviderOperationKind::ConversationPrompt,
                binding: ConversationBindingIdentity::ExternalProvider { binding: binding() },
                admitted_at_ms: 1,
            })
            .await
            .unwrap();
        store
            .mark_may_have_dispatched(&operation_id, 2)
            .await
            .unwrap();
        if !targetless {
            store
                .record_target(&operation_id, &target, 3)
                .await
                .unwrap();
        }
        store
            .record_terminal(
                &operation_id,
                ProviderOperationEffect::Unknown,
                ProviderReconciliationState::NotReconcilable,
                None,
                4,
            )
            .await
            .unwrap();
        unknown_rows.push(store.inspect(&operation_id).await.unwrap().unwrap());
    }
    let store = Arc::new(Mutex::new(store));
    let mut live_binding = binding();
    if previous_generation {
        live_binding.generation.generation = GenerationNumber::try_from(2).unwrap();
    }
    let backend = ExternalProviderSupervisor::new(
        vec![ExternalProviderBinding {
            identity: live_binding.clone(),
            runtime,
        }],
        Arc::clone(&store),
    )
    .unwrap();
    let requested_id = OperationId::generate();

    // Act: exercise the owning admission boundary with the actual live identity.
    let admitted = backend
        .prepare_operation(
            requested_id.clone(),
            ProviderOperationKind::ConversationPrompt,
            live_binding,
            Some(&target),
        )
        .await;
    let mut blocking_reason = None;
    let accepted = match admitted {
        Ok(PreparedOperation::Admitted { live, .. }) => {
            // This fixture tests admission only, so settle its unstarted local task.
            *live.result.lock().await = Some(Err(unavailable_failure(requested_id.clone(), None)));
            true
        }
        Err(failure) => {
            if !previous_generation {
                assert_eq!(failure.kind, ConversationOperationFailureKind::Busy);
                blocking_reason = Some(String::from(failure.message));
            }
            false
        }
        Ok(PreparedOperation::Existing(_)) => panic!("fresh operation unexpectedly existed"),
    };

    // Assert: generation controls admission, never rewrites historical uncertainty.
    for row in &unknown_rows {
        assert_eq!(
            store.lock().await.inspect(&row.operation_id).await.unwrap(),
            Some(row.clone())
        );
    }
    backend.shutdown().await.unwrap();
    if !previous_generation {
        let reason = blocking_reason.expect("current generation refusal reason");
        for row in &unknown_rows {
            assert!(
                reason.contains(row.operation_id.as_str()),
                "Busy reason omitted the blocking operation: {reason}"
            );
        }
    }
    assert_eq!(
        accepted, previous_generation,
        "earlier generations must not block; current generation must block"
    );
}
