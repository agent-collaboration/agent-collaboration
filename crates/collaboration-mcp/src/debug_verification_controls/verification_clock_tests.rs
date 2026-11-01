//! Real owner-socket requests reach the configured clock; no native input is due.
use super::*;
use collaboration_contracts::{DeliveryFuture, TargetPresence};
use std::sync::atomic::{AtomicUsize, Ordering};

#[derive(Default)]
struct ClockFixturePorts(AtomicUsize);
impl collaboration_contracts::SessionMessageDelivery for ClockFixturePorts {
    fn deliver<'a>(
        &'a self,
        _: collaboration_contracts::layer_zero::DeliveryRequest,
        _: &'a dyn collaboration_contracts::AttemptEvidenceSink,
    ) -> DeliveryFuture<'a, collaboration_contracts::NativeDeliveryResult> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(collaboration_contracts::NativeDeliveryResult::unknown()) })
    }
    fn reconcile_attempt(
        &self,
        _: collaboration_contracts::AttemptReconciliationContext,
    ) -> DeliveryFuture<'_, collaboration_contracts::AttemptReconciliation> {
        Box::pin(async { Ok(collaboration_contracts::AttemptReconciliation::StillUnknown) })
    }
}
impl collaboration_contracts::TargetPresenceProbe for ClockFixturePorts {
    fn presence(
        &self,
        _: &collaboration_protocol::SessionRef,
    ) -> DeliveryFuture<'_, TargetPresence> {
        Box::pin(async { Ok(TargetPresence::Running) })
    }
}

#[tokio::test]
async fn clock_control_steps_the_selected_clock_and_refuses_invalid_steps_over_owner_http() {
    use std::os::unix::fs::PermissionsExt as _;
    let directory = tempfile::tempdir_in("/tmp").unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let interactions = Arc::new(
        interaction_storage::InteractionStore::open(directory.path().join("interaction.sqlite"))
            .await
            .unwrap(),
    );
    let clock = Arc::new(collaboration_service::DebugSubscriptionClock::new(
        serde_json::from_value(json!("1970-01-01T00:00:00Z")).unwrap(),
    ));
    let ports = Arc::new(ClockFixturePorts::default());
    let service = collaboration_service::SubscriptionDeliveryService::new(
        collaboration_service::SubscriptionDeliveryServiceProps {
            board_availability: collaboration_service::BoardAvailability::Unavailable,
            push_store: interactions.clone(),
            delivery: ports.clone(),
            presence: ports.clone(),
            machine_identity: collaboration_contracts::MachineIdentity::new(
                crate::api_test_harness::TEST_SERVICE_ID
                    .to_owned()
                    .try_into()
                    .unwrap(),
                Some("Clock control fixture"),
            )
            .unwrap(),
            clock: clock.clone(),
        },
    );
    service.start().await.unwrap();
    let socket = directory.path().join("control.sock");
    let (listener, _cleanup) = OwnerOnlySocket::bind(&socket).unwrap().into_parts();
    let config = crate::api_test_harness::api_config(
        crate::api_test_harness::test_identity().build(),
        directory.path(),
    );
    let api = collaboration_api_router(&config, CollaborationApiListener::UnixSocket)
        .with_debug_verification_controls(DebugVerificationControls {
            verification_clock: Some(service.clone()),
            ..Default::default()
        })
        .unwrap();
    let serving = tokio::spawn(serve_collaboration_api(
        listener,
        api,
        config.shutdown.clone(),
    ));
    let path = "/debug/notifications/clock/advance";
    let (status, response) = post_path(&socket, path, json!({"nowMs":600_000})).await;
    assert_eq!(status, 200);
    assert_eq!(response, json!({"nowMs":600_000,"readersCompleted":0}));
    assert_eq!(
        collaboration_service::SubscriptionClock::now(&*clock).timestamp_millis(),
        600_000
    );
    for invalid in [0, i64::MAX] {
        let (status, error) = post_path(&socket, path, json!({"nowMs":invalid})).await;
        assert_eq!(status, 400);
        assert_eq!(error["kind"], "invalidField");
        assert_eq!(
            collaboration_service::SubscriptionClock::now(&*clock).timestamp_millis(),
            600_000
        );
    }
    assert_eq!(ports.0.load(Ordering::SeqCst), 0);
    config.shutdown.cancel();
    serving.await.unwrap().unwrap();
    service.shutdown().await;
    drop(service);
    Arc::try_unwrap(interactions)
        .unwrap_or_else(|_| panic!("clock fixture retained interaction owner"))
        .close()
        .await
        .unwrap();
}
