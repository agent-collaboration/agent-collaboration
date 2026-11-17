//! A result whose durable write failed keeps its own retention window
//! (design note §12.5): later settlements never evict it while the Router
//! runs. A private invariant of the supervisor's live operations; the
//! end-to-end overlay is proven in tests/integration/provider_persistence_overlay.rs.

use super::*;

/// Oracle: §12.5, written by hand: a persistence-failed result outlives
/// MAX_RETAINED_SETTLEMENTS later settlements, which share the settled
/// window and evict each other; after them, its live owner still holds
/// exactly the unsaved result (the moved case's retained-result check,
/// Rule 7).
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
    let publish = |operation_id: OperationId, result, retention| {
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
                .publish_result(operation_id, &live, result, retention)
                .await;
        }
    };
    let unsaved = OperationId::generate();
    let first_settled = OperationId::generate();
    // The overlay's own outcome, distinct from the later settlements'.
    let unsaved_result = failure(
        ConversationOperationFailureKind::Unavailable,
        ConversationOperationFailureStage::Settlement,
        ProviderOperationEffect::Applied,
        "applied; the durable record still says may-have-dispatched (write failed)",
        unsaved.clone(),
        None,
    );
    publish(
        unsaved.clone(),
        Err(unsaved_result.clone()),
        provider_operation_finish::Retention::PersistenceFailed,
    )
    .await;
    publish(
        first_settled.clone(),
        Err(unavailable_failure(first_settled.clone(), None)),
        provider_operation_finish::Retention::Settled,
    )
    .await;

    // Act: later settlements fill the settled window past its bound.
    for _ in 0..MAX_RETAINED_SETTLEMENTS {
        let later = OperationId::generate();
        publish(
            later.clone(),
            Err(unavailable_failure(later, None)),
            provider_operation_finish::Retention::Settled,
        )
        .await;
    }
    // The result the unsaved operation's live owner still answers.
    let retained_live = supervisor
        .inner
        .live_operations
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .operations
        .get(&unsaved)
        .map(Arc::clone);
    let waited_after_later_settlements = match retained_live {
        Some(live) => live.result.lock().await.clone(),
        None => None,
    }
    .expect("the unsaved result is retained");
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
    let outcome = |waited: &Result<_, ConversationOperationFailure>| match waited {
        Err(failure) => Some((
            failure.kind,
            failure.effect,
            String::from(failure.message.clone()),
        )),
        Ok(_) => None,
    };
    let published = unsaved_result;
    assert!(
        retained(&unsaved),
        "the unsaved result is still answered live"
    );
    assert_eq!(
        outcome(&waited_after_later_settlements),
        Some((
            published.kind,
            published.effect,
            String::from(published.message),
        )),
        "later settlements don't evict the unsaved result: {waited_after_later_settlements:?}"
    );
    assert!(
        !retained(&first_settled),
        "the settled window evicts its oldest result"
    );
}
