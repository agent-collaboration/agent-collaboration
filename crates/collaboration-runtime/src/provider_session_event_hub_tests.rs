use std::{sync::Arc, time::Duration};

use message_board::SessionRef;
use session_event_model::{SessionEvent, SessionItem, SessionItemKind};
use tokio::sync::Mutex;

use super::ProviderSessionEventHub;
use crate::ProviderOperationStore;

#[tokio::test]
async fn rejected_active_history_replay_preserves_current_skill_view_and_subscription() {
    let root = tempfile::tempdir().expect("root");
    let store = ProviderOperationStore::open(&root.path().join("operations.sqlite"))
        .await
        .expect("store");
    let hub = ProviderSessionEventHub::new(Arc::new(Mutex::new(store)));
    let session: SessionRef = serde_json::from_value(serde_json::json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},"sessionId":"active"})).expect("session");
    hub.publish(
        session.clone(),
        SessionEvent::StateChanged {
            state: session_event_model::SessionState::Running,
        },
    )
    .await
    .expect("active state");
    let view = session_event_model::SkillView::Available {
        skills: vec![session_event_model::ProviderSkill {
            invocation_name: session_event_model::SkillInvocationName::new("review").expect("name"),
            description: "Review".into(),
            scope: session_event_model::ProviderSkillScope::System,
        }],
        errors: vec![],
    };
    hub.publish_skill_view(session.clone(), view.clone())
        .await
        .expect("source view");
    let history = hub
        .histories
        .lock()
        .await
        .get(&session)
        .cloned()
        .expect("history");
    let receiver = history.lock().await.skill_view.subscribe();
    assert!(matches!(
        hub.begin_history_replay(session).await,
        Err(collaboration_contracts::SessionEventHubError::Unavailable)
    ));
    assert_eq!(*receiver.borrow(), view);
    assert!(!receiver.has_changed().expect("subscription remains open"));
}

#[tokio::test]
async fn locked_attach_on_one_session_does_not_stall_another_publish() {
    let root = tempfile::tempdir().expect("temp root");
    let store = ProviderOperationStore::open(&root.path().join("operations.sqlite"))
        .await
        .expect("store");
    let hub = Arc::new(ProviderSessionEventHub::new(Arc::new(Mutex::new(store))));
    let first: SessionRef = serde_json::from_value(serde_json::json!({
        "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},
        "sessionId":"first"
    }))
    .expect("first session");
    let second: SessionRef = serde_json::from_value(serde_json::json!({
        "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},
        "sessionId":"second"
    }))
    .expect("second session");
    for target in [&first, &second] {
        hub.publish(
            target.clone(),
            SessionEvent::ItemStarted {
                item: SessionItem {
                    item_id: "one".into(),
                    kind: SessionItemKind::UserMessage,
                    text: Some("hello".into()),
                },
            },
        )
        .await
        .expect("seed item");
    }
    let first_history = hub
        .histories
        .lock()
        .await
        .get(&first)
        .cloned()
        .expect("first history");
    let _held_attach_lock = first_history.lock().await;
    tokio::time::timeout(
        Duration::from_millis(250),
        hub.publish(
            second,
            SessionEvent::ItemStarted {
                item: SessionItem {
                    item_id: "two".into(),
                    kind: SessionItemKind::UserMessage,
                    text: Some("world".into()),
                },
            },
        ),
    )
    .await
    .expect("independent publish must not wait")
    .expect("publish");
}
