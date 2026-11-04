use super::*;

#[tokio::test]
async fn same_generation_unknown_holds_push_with_blocking_operation_id() {
    // Arrange: real storage, delivery owner, router, supervisor and ACP transport.
    let root = tempfile::tempdir().expect("provider root");
    let capture = root.path().join("requests.jsonl");
    let target = target();
    let binding = provider_binding(&target);
    let runtime = ExternalProviderRuntime::initialize(
        loadable_provider_fixture(&capture, &root.path().join("unused-prompt.sock")),
        std::sync::Arc::new(acp_provider_adapters::StandardAcpBehavior),
    )
    .await
    .expect("actual initialized ACP process");
    let store = Arc::new(tokio::sync::Mutex::new(
        ProviderOperationStore::open(&root.path().join("operations.sqlite"))
            .await
            .expect("provider store"),
    ));
    store
        .lock()
        .await
        .record_session(&ProviderSessionRecord {
            target: target.clone(),
            working_directory: ProviderWorkingDirectory::try_from(
                root.path().display().to_string(),
            )
            .expect("cwd"),
            requested_policy: ProviderRequestedPolicy {
                access: RouterAccess::WriteRestricted,
            },
            created_by: target.clone().into(),
            approver: target.clone().into(),
            updated_at_ms: 1,
        })
        .await
        .expect("saved provider session");
    let blocker = OperationId::generate();
    {
        let mut operations = store.lock().await;
        operations
            .admit(ProviderOperationAdmission {
                operation_id: blocker.clone(),
                operation_kind: ProviderOperationKind::ConversationPrompt,
                binding: collaboration_protocol::ConversationBindingIdentity::ExternalProvider {
                    binding: binding.clone(),
                },
                admitted_at_ms: 1,
            })
            .await
            .expect("blocking operation");
        operations
            .record_target(&blocker, &target, 2)
            .await
            .expect("blocking target");
        operations
            .mark_may_have_dispatched(&blocker, 3)
            .await
            .expect("Unknown effect");
    }
    let before = store.lock().await.inspect(&blocker).await.expect("before");
    let supervisor = Arc::new(
        ExternalProviderSupervisor::new(
            vec![ExternalProviderBinding {
                identity: binding.clone(),
                runtime,
            }],
            store.clone(),
        )
        .expect("supervisor"),
    );
    let route = Arc::new(ProviderAcpDeliveryRoute::new(
        target.endpoint.service_id.clone(),
        std::iter::once(target.endpoint.clone()).collect(),
        available_directory(&target, &binding),
        supervisor.clone(),
        store.clone(),
        Arc::new(NoLivePeer),
    ));
    let router = Arc::new(SessionDeliveryRouter::new(vec![route.clone()]));
    let pushes = Arc::new(
        InteractionStore::open(&root.path().join("interaction.sqlite"))
            .await
            .expect("push store"),
    );
    let owner = SubscriptionDeliveryService::new(SubscriptionDeliveryServiceProps {
        board_availability: BoardAvailability::Unavailable,
        push_store: pushes.clone(),
        delivery: router.clone(),
        presence: router,
        machine_identity: MachineIdentity::new(
            target.endpoint.service_id.clone(),
            Some("uncertainty proof"),
        )
        .expect("machine"),
        clock: Arc::new(SystemSubscriptionClock),
    });
    owner.start().await.expect("owner starts");

    // Act: persist and deliver a genuine Auto push through the owning service.
    let push_id = save_direct_message(&pushes, &target, MessageDelivery::Auto).await;
    let held = tokio::time::timeout(
        Duration::from_secs(5),
        owner.deliver_direct_message(target, push_id),
    )
    .await
    .expect("delivery deadline")
    .expect("push outcome");
    let after = store.lock().await.inspect(&blocker).await.expect("after");
    owner.shutdown().await;
    drop(owner);
    route.shutdown_queue().await;
    supervisor.shutdown().await.expect("shutdown");
    Arc::try_unwrap(pushes)
        .unwrap_or_else(|_| panic!("push owner retained"))
        .close()
        .await
        .expect("push owner closed");

    // Assert: the public held receipt preserves the actual admission failure ID.
    assert_eq!(held.delivery_state, PushDeliveryState::Held);
    let receipt = held.last_outcome.expect("held push receipt");
    assert!(
        matches!(receipt.outcome, DeliveryOutcome::NotSubmitted { reason, .. }
        if reason.contains(blocker.as_str())),
        "held push must name the blocking operation"
    );
    assert_eq!(before, after, "delivery must not rewrite uncertainty");
    let requests = std::fs::read_to_string(capture).expect("actual ACP requests");
    let requests = requests
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).expect("request"))
        .collect::<Vec<_>>();
    assert_eq!(requests.len(), 1, "blocked push must not send a prompt");
    assert_eq!(requests[0]["method"], "session/load");
}
