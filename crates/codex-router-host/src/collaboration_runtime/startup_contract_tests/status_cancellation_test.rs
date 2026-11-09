//! A pending real mail read must release its owner when status is cancelled.
use super::*;
use durable_steps::AtomicStore;

struct NoCalls;
impl fleet_tokio_objects::CallRouter for NoCalls {
    async fn dispatch(
        &self,
        _: fleet_tokio_objects::CallEnvelope,
    ) -> Result<serde_json::Value, durable_steps::CallFailure> {
        Err(durable_steps::CallFailure::NotSent {
            reason: durable_steps::NotSentReason::Overloaded,
        })
    }
}

#[tokio::test]
async fn cancelling_a_pending_fleet_read_releases_store_ownership_before_runtime_cleanup() {
    // Arrange: actual runtime/store ownership, with its writer held at a causal gate.
    let fixture = start_host(false).await;
    let store = fixture
        .runtime
        .interactions
        .upgrade()
        .expect("interaction owner");
    let weak = Arc::downgrade(&store);
    let host = Arc::new(fleet_tokio_objects::TokioHost::new(
        Arc::new(NoCalls),
        runtime_primitives::SystemClock,
    ));
    let atomic = interaction_storage::InteractionAtomicStore::new(store.clone(), host);
    let entered = Arc::new(tokio::sync::Notify::new());
    let writer_entered = entered.clone();
    let (release, released) = tokio::sync::oneshot::channel();
    let holding = tokio::spawn(async move {
        atomic
            .transaction(move |_| {
                Box::pin(async move {
                    writer_entered.notify_one();
                    released.await.unwrap();
                    Ok(())
                })
            })
            .await
            .unwrap();
    });
    entered.notified().await;
    let mut read = fixture.runtime.observe_fleet();
    assert!(matches!(
        futures_util::poll!(&mut read),
        std::task::Poll::Pending
    ));

    // Act: lifecycle retirement cancels this future before closing its runtime.
    drop(read);
    release.send(()).unwrap();
    holding.await.unwrap();
    drop(store);
    fixture.runtime.shutdown().await.expect("runtime cleanup");

    // Assert: the pending diagnostic did not retain the real database owner.
    assert!(
        weak.upgrade().is_none(),
        "cancelled status retained the interaction owner"
    );
}
