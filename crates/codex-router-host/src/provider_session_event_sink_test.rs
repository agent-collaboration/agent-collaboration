use super::*;
use acp_client_runtime::SessionEventSink;
use collaboration_contracts::SessionEventHub;
use collaboration_runtime::ProviderOperationStore;
use collaboration_runtime::ProviderSessionEventHub;
use message_board::{EndpointId, ServiceId, SessionEndpointRef, SessionId, SessionRef};
use session_event_model::{SessionEvent, TurnOutcome};
use std::sync::Arc;
use tokio::sync::Mutex;

#[tokio::test]
async fn publication_before_replay_reset_is_never_reintroduced_after_it() {
    let root = tempfile::tempdir().expect("temporary store");
    let store = ProviderOperationStore::open(&root.path().join("operations.sqlite"))
        .await
        .expect("provider store");
    let hub = Arc::new(ProviderSessionEventHub::new(Arc::new(Mutex::new(store))));
    let endpoint = SessionEndpointRef {
        service_id: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())
            .expect("service"),
        endpoint_id: EndpointId::try_from("fixture-provider".to_owned()).expect("endpoint"),
    };
    let session = SessionRef {
        endpoint: endpoint.clone(),
        session_id: SessionId::try_from("session-one".to_owned()).expect("session"),
    };
    let sink = HubSessionEventSink::new(Arc::clone(&hub), endpoint);
    sink.publish(
        "session-one",
        SessionEvent::TurnStarted {
            turn_id: "old-turn".into(),
            input_id: session_event_model::InputId::new("old-input").expect("input ID"),
        },
    )
    .expect("start old turn");
    sink.publish(
        "session-one",
        SessionEvent::TurnEnded {
            turn_id: "old-turn".into(),
            outcome: TurnOutcome::Lost {
                reason: session_event_model::TurnLostReason::ProviderRetired,
            },
        },
    )
    .expect("publish loss");
    sink.begin_history_replay("session-one")
        .await
        .expect("reset after loss");
    sink.publish(
        "session-one",
        SessionEvent::TurnStarted {
            turn_id: "replayed-turn".into(),
            input_id: session_event_model::InputId::new("replayed-input").expect("input ID"),
        },
    )
    .expect("replayed start");
    sink.shutdown().await.expect("drained consumer");
    assert!(sink.begin_history_replay("session-one").await.is_err());
    let attached = hub.attach(session).await.expect("attach replay");
    assert_eq!(attached.snapshot.len(), 1);
    assert!(matches!(
        &attached.snapshot[0].event,
        SessionEvent::TurnStarted { turn_id, .. } if turn_id == "replayed-turn"
    ));
}

#[tokio::test]
async fn idle_config_change_notifies_provider_catalog_worker() {
    let root = tempfile::tempdir().expect("temporary store");
    let store = ProviderOperationStore::open(&root.path().join("operations.sqlite"))
        .await
        .expect("provider store");
    let hub = Arc::new(ProviderSessionEventHub::new(Arc::new(Mutex::new(store))));
    let endpoint = SessionEndpointRef {
        service_id: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())
            .expect("service"),
        endpoint_id: EndpointId::try_from("fixture-provider".to_owned()).expect("endpoint"),
    };
    let sink = HubSessionEventSink::new(hub, endpoint);
    let (sender, mut receiver) = mpsc::unbounded_channel();
    sink.install_catalog_refresh(sender);
    sink.publish(
        "session-one",
        SessionEvent::ItemStarted {
            item: session_event_model::SessionItem {
                item_id: "config-1".into(),
                kind: SessionItemKind::ConfigChange,
                text: Some("model options updated".into()),
            },
        },
    )
    .expect("publish config update");
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(2), receiver.recv())
            .await
            .expect("catalog notification")
            .as_deref(),
        Some("session-one")
    );
    sink.shutdown().await.expect("drain");
}

#[tokio::test]
async fn closing_session_releases_backlog_and_publication_tracking() {
    let root = tempfile::tempdir().expect("temporary store");
    let store = ProviderOperationStore::open(&root.path().join("operations.sqlite"))
        .await
        .expect("provider store");
    let hub = Arc::new(ProviderSessionEventHub::new(Arc::new(Mutex::new(store))));
    let endpoint = SessionEndpointRef {
        service_id: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())
            .expect("service"),
        endpoint_id: EndpointId::try_from("fixture-provider".to_owned()).expect("endpoint"),
    };
    let sink = HubSessionEventSink::new(hub, endpoint);
    sink.backlog_warned
        .lock()
        .expect("backlog lock")
        .insert("session-one".to_owned());
    sink.publish(
        "session-one",
        SessionEvent::StateChanged {
            state: session_event_model::SessionState::Closed,
        },
    )
    .expect("close event queued");
    sink.not_publishing
        .lock()
        .expect("publication lock")
        .insert("session-one".to_owned());
    sink.shutdown().await.expect("close event drained");
    assert!(
        !sink
            .backlog_warned
            .lock()
            .expect("backlog lock")
            .contains("session-one")
    );
    assert!(
        !sink
            .not_publishing
            .lock()
            .expect("publication lock")
            .contains("session-one")
    );
}
