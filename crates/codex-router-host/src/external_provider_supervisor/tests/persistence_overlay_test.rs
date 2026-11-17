//! A result whose durable write failed keeps its own retention window
//! (design note §12.5): later settlements never evict it while the Router
//! runs. A private invariant of the supervisor's live operations; the
//! end-to-end overlay is proven in tests/integration/provider_persistence_overlay.rs.

use super::*;

/// Oracle: §12.5, written by hand: a persistence-failed result outlives
/// MAX_RETAINED_SETTLEMENTS later settlements, which share the settled
/// window and evict each other.
#[tokio::test]
async fn a_persistence_failed_result_outlives_later_settlements() {
    // Arrange
    let root = tempfile::tempdir().expect("store root");
    let store = Arc::new(Mutex::new(
        ProviderOperationStore::open(&root.path().join("operations.sqlite"))
            .await
            .expect("store"),
    ));
    let supervisor = ExternalProviderSupervisor::new(Vec::new(), store).expect("supervisor");
    let publish = |operation_id: OperationId, retention| {
        let live = Arc::new(LiveOperation::pending());
        supervisor
            .inner
            .live_operations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .operations
            .insert(operation_id.clone(), Arc::clone(&live));
        let inner = Arc::clone(&supervisor.inner);
        async move {
            inner
                .publish_result(
                    operation_id.clone(),
                    &live,
                    Err(unavailable_failure(operation_id, None)),
                    retention,
                )
                .await;
        }
    };
    let unsaved = OperationId::generate();
    let first_settled = OperationId::generate();
    publish(
        unsaved.clone(),
        provider_operation_finish::Retention::PersistenceFailed,
    )
    .await;
    publish(
        first_settled.clone(),
        provider_operation_finish::Retention::Settled,
    )
    .await;

    // Act: later settlements fill the settled window past its bound.
    for _ in 0..MAX_RETAINED_SETTLEMENTS {
        publish(
            OperationId::generate(),
            provider_operation_finish::Retention::Settled,
        )
        .await;
    }
    let retained = |operation_id: &OperationId| {
        supervisor
            .inner
            .live_operations
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .operations
            .contains_key(operation_id)
    };

    // Assert
    assert!(
        retained(&unsaved),
        "the unsaved result is still answered live"
    );
    assert!(
        !retained(&first_settled),
        "the settled window evicts its oldest result"
    );
}
