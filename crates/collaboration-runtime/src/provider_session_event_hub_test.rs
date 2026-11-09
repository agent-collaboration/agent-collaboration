use std::{sync::Arc, time::Duration};

use message_board::SessionRef;
use session_event_model::{SessionEvent, SessionItem, SessionItemKind};
use tokio::sync::Mutex;

use super::ProviderSessionEventHub;
use crate::ProviderOperationStore;
use collaboration_contracts::SessionEventHub;

#[tokio::test]
async fn inventory_uses_first_user_message_and_existing_session_title() {
    // Arrange: durable inventory plus the same retained events used by attachments.
    let root = tempfile::tempdir().expect("root");
    let mut store = ProviderOperationStore::open(&root.path().join("operations.sqlite"))
        .await
        .expect("store");
    let session: SessionRef = serde_json::from_value(serde_json::json!({
        "endpoint":{"serviceId":uuid::Uuid::now_v7(),"endpointId":"claude-local"},
        "sessionId":uuid::Uuid::now_v7()
    }))
    .expect("session");
    let owner: collaboration_protocol::ProviderIdentity =
        serde_json::from_value(serde_json::json!({"humanId":"owner"})).expect("owner");
    store
        .record_session(&crate::ProviderSessionRecord {
            target: serde_json::from_value(serde_json::to_value(&session).expect("target"))
                .expect("target"),
            working_directory: root.path().display().to_string().try_into().expect("cwd"),
            requested_policy: collaboration_protocol::ProviderRequestedPolicy {
                access: collaboration_protocol::RouterAccess::WriteRestricted,
            },
            created_by: owner.clone(),
            approver: owner,
            updated_at_ms: 1,
        })
        .await
        .expect("inventory");
    let hub = ProviderSessionEventHub::new(Arc::new(Mutex::new(store)));
    let first_id = uuid::Uuid::now_v7().to_string();
    for (item_id, kind, text) in [
        (
            uuid::Uuid::now_v7().to_string(),
            SessionItemKind::AgentMessage,
            "agent greeting",
        ),
        (first_id.clone(), SessionItemKind::UserMessage, "first "),
        (
            uuid::Uuid::now_v7().to_string(),
            SessionItemKind::UserMessage,
            "later message",
        ),
    ] {
        hub.publish(
            session.clone(),
            SessionEvent::ItemStarted {
                item: SessionItem {
                    item_id,
                    kind,
                    text: Some(text.into()),
                },
            },
        )
        .await
        .expect("item");
    }
    // Act: the first message's final text arrives after another item.
    hub.publish(
        session.clone(),
        SessionEvent::ItemUpdated {
            item: SessionItem {
                item_id: first_id,
                kind: SessionItemKind::UserMessage,
                text: Some("first message".into()),
            },
        },
    )
    .await
    .expect("final first text");
    let summary = hub
        .sessions(session.endpoint.clone())
        .await
        .expect("sessions")
        .remove(0);
    // Assert: order is original user-item order, not lexical ID or last-update order.
    assert_eq!(summary.preview, "first message");
    assert_eq!(summary.name, None, "a preview never invents a title");

    // Act / Assert: only a title the provider actually reported becomes the name.
    hub.publish(
        session.clone(),
        SessionEvent::ItemStarted {
            item: SessionItem {
                item_id: uuid::Uuid::now_v7().to_string(),
                kind: SessionItemKind::SessionInfo,
                text: Some(r#"{"title":"Existing provider title"}"#.into()),
            },
        },
    )
    .await
    .expect("session info");
    let summary = hub
        .sessions(session.endpoint.clone())
        .await
        .expect("sessions")
        .remove(0);
    assert_eq!(summary.name.as_deref(), Some("Existing provider title"));

    // Inactivity clears replay items, but must not erase inventory labels.
    hub.publish(
        session.clone(),
        SessionEvent::StateChanged {
            state: session_event_model::SessionState::Unloaded,
        },
    )
    .await
    .expect("unload");
    let summary = hub
        .sessions(session.endpoint)
        .await
        .expect("sessions")
        .remove(0);
    assert_eq!(summary.preview, "first message");
    assert_eq!(summary.name.as_deref(), Some("Existing provider title"));
}

#[tokio::test]
async fn closed_and_unloaded_sessions_admit_fresh_history_replay() {
    // Arrange: both states are inactive and retain the provider identity for loading.
    for state in [
        session_event_model::SessionState::Closed,
        session_event_model::SessionState::Unloaded,
    ] {
        let root = tempfile::tempdir().expect("root");
        let store = ProviderOperationStore::open(&root.path().join("operations.sqlite"))
            .await
            .expect("store");
        let hub = ProviderSessionEventHub::new(Arc::new(Mutex::new(store)));
        let session: SessionRef = serde_json::from_value(serde_json::json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"cursor-local"},"sessionId":"restorable"})).expect("session");
        hub.publish(
            session.clone(),
            SessionEvent::StateChanged {
                state: state.clone(),
            },
        )
        .await
        .expect("inactive state");
        let mut before = hub.attach(session.clone()).await.expect("attach");
        // Act: the hub itself owns replay admission and epoch reset.
        let epoch = hub
            .begin_history_replay(session.clone())
            .await
            .expect("inactive Session must admit replay");
        // Assert: existing reset semantics remain; old subscribers must resynchronize.
        assert!(epoch > before.epoch);
        assert!(
            matches!(before.receiver.recv().await.expect("resync"), collaboration_contracts::HubEvent { event: SessionEvent::ResyncRequired { replay_epoch }, .. } if replay_epoch == epoch)
        );
        let after = hub.attach(session.clone()).await.expect("new attach");
        assert_eq!(after.epoch, epoch);
        assert!(after.snapshot.is_empty());
        assert_eq!(
            hub.state(session).await.expect("state"),
            session_event_model::SessionState::Unloaded
        );
    }
}

#[tokio::test]
async fn rejected_active_history_replay_preserves_current_skill_view_and_subscription() {
    use session_event_model::{
        ApprovalChoice, ApprovalEffect, ApprovalRequest, ApprovalScope, OfferedOption,
        OfferedOptionId, OfferedOptions, OptionsOrigin, PendingInteraction, PendingInteractions,
        SessionState,
    };
    let request = ApprovalRequest {
        request_id: "pending".into(),
        title: "pending approval".into(),
        description: None,
        subject: None,
        options_origin: OptionsOrigin::AgentOffered,
        options: OfferedOptions::new(vec![OfferedOption {
            option_id: OfferedOptionId::new("allow").expect("option"),
            label: "Allow".into(),
            choice: ApprovalChoice::new(ApprovalEffect::Allow, ApprovalScope::Once),
        }])
        .expect("options"),
    };
    let pending = PendingInteractions::new(vec![PendingInteraction::Approval {
        approver: message_board::Identity::Human {
            human_id: message_board::HumanId::try_from("approver".to_owned()).expect("human"),
        },
        request: Box::new(request),
    }])
    .expect("pending");
    for state in [
        SessionState::Idle,
        SessionState::Running,
        SessionState::AuthenticationRequired,
        SessionState::RequiresAction { pending },
    ] {
        assert_active_replay_preserves_state(state).await;
    }
}

async fn assert_active_replay_preserves_state(state: session_event_model::SessionState) {
    let root = tempfile::tempdir().expect("root");
    let store = ProviderOperationStore::open(&root.path().join("operations.sqlite"))
        .await
        .expect("store");
    let hub = ProviderSessionEventHub::new(Arc::new(Mutex::new(store)));
    let session: SessionRef = serde_json::from_value(serde_json::json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},"sessionId":"active"})).expect("session");
    hub.publish(
        session.clone(),
        SessionEvent::StateChanged {
            state: state.clone(),
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
    let mut before = hub
        .attach(session.clone())
        .await
        .expect("attached current history");
    assert!(matches!(
        hub.begin_history_replay(session.clone()).await,
        Err(collaboration_contracts::SessionEventHubError::Unavailable)
    ));
    assert_eq!(*receiver.borrow(), view);
    assert!(!receiver.has_changed().expect("subscription remains open"));
    let after = hub
        .attach(session.clone())
        .await
        .expect("history unchanged");
    assert_eq!(after.epoch, before.epoch);
    assert_eq!(after.snapshot, before.snapshot);
    assert_eq!(hub.state(session).await.expect("state unchanged"), state);
    assert!(matches!(
        before.receiver.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
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
