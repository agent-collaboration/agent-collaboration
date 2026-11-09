//! Native inventory must page using observed activity, without changing durable records.
use super::test_support::ScriptedSessionBackend;
use super::*;
use crate::{ProviderOperationStore, ProviderSessionEventHub, ProviderSessionRecord};
use session_event_model::{SessionItem, SessionItemKind, SessionSettings};
use tokio::sync::Mutex;

type TestResult<T = ()> = Result<T, Box<dyn std::error::Error>>;

async fn list(context: Arc<RouterSessionAppServerContext>, cursor: Value) -> TestResult<Value> {
    Ok(handle_app_server_rpc(
        context,
        json!({
            "id":1,"method":"thread/list","params":{
                "sortKey":"updated_at","limit":1,"cursor":cursor
            }
        }),
    )
    .await?
    .response["result"]
        .clone())
}

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions; real hub/store setup may fail."
)]
async fn latest_observed_activity_moves_ahead_of_newer_stored_sessions_with_consistent_cursors()
-> TestResult {
    // Arrange: B was created after A. Real inventory/hub/dispatch stay in this path.
    let root = tempfile::tempdir()?;
    let backend = ScriptedSessionBackend::new()?;
    let first: SessionRef =
        serde_json::from_value(json!({"endpoint":backend.endpoint,"sessionId":Uuid::now_v7()}))?;
    let second: SessionRef =
        serde_json::from_value(json!({"endpoint":backend.endpoint,"sessionId":Uuid::now_v7()}))?;
    let owner: collaboration_protocol::ProviderIdentity =
        serde_json::from_value(json!({"humanId":"owner"}))?;
    let store = Arc::new(Mutex::new(
        ProviderOperationStore::open(&root.path().join("operations.sqlite")).await?,
    ));
    for (session, updated_at_ms) in [(&first, 1000), (&second, 2000)] {
        store
            .lock()
            .await
            .record_session(&ProviderSessionRecord {
                target: serde_json::from_value(serde_json::to_value(session)?)?,
                working_directory: root.path().display().to_string().try_into()?,
                requested_policy: collaboration_protocol::ProviderRequestedPolicy {
                    access: collaboration_protocol::RouterAccess::WriteRestricted,
                },
                created_by: owner.clone(),
                approver: owner.clone(),
                updated_at_ms,
            })
            .await?;
    }
    let hub = Arc::new(ProviderSessionEventHub::new(store.clone()));
    // Merely attaching must not invent activity for B.
    let _attachment = hub.attach(second.clone()).await?;
    let context = Arc::new(RouterSessionAppServerContext::new(
        backend.endpoint.clone(),
        ScriptedSessionBackend::actor()?,
        backend.clone(),
        hub.clone(),
        test_support::model_catalog(tokio::sync::watch::channel(Vec::new()).1),
    ));
    let before = list(context.clone(), Value::Null).await?;
    assert_eq!(before["data"][0]["id"], thread_alias(&second));
    assert_eq!(before["data"][0]["updatedAt"], 2);

    // Act: A has later activity despite its older durable settlement timestamp.
    let earliest = chrono::Utc::now().timestamp();
    for event in [
        SessionEvent::TurnStarted {
            turn_id: Uuid::now_v7().to_string(),
            input_id: session_event_model::InputId::generate(),
        },
        SessionEvent::ItemStarted {
            item: SessionItem {
                item_id: Uuid::now_v7().to_string(),
                kind: SessionItemKind::UserMessage,
                text: Some("new activity".into()),
            },
        },
        SessionEvent::SettingsChanged {
            settings: SessionSettings::default(),
        },
    ] {
        hub.publish(first.clone(), event).await?;
    }
    let latest = chrono::Utc::now().timestamp();
    let after = list(context.clone(), Value::Null).await?;
    // Assert: rendered time and paging both use the hub's observation time.
    assert_eq!(after["data"][0]["id"], thread_alias(&first));
    let observed = after["data"][0]["updatedAt"].as_i64().ok_or("updatedAt")?;
    assert!(
        (earliest..=latest).contains(&observed),
        "activity time {observed}, expected {earliest}..={latest}"
    );
    let next = list(context.clone(), after["nextCursor"].clone()).await?;
    assert_eq!(next["data"][0]["id"], thread_alias(&second));
    assert_eq!(next["data"][0]["updatedAt"], 2);
    assert_eq!(next["nextCursor"], Value::Null);

    // In-process replay resets retain the inventory observation, without a write.
    hub.publish(
        first.clone(),
        SessionEvent::StateChanged {
            state: session_event_model::SessionState::Unloaded,
        },
    )
    .await?;
    hub.begin_history_replay(first.clone()).await?;
    assert_eq!(
        list(context, Value::Null).await?["data"][0]["id"],
        thread_alias(&first)
    );
    let stored_first = store
        .lock()
        .await
        .session_record(&serde_json::from_value(serde_json::to_value(&first)?)?)
        .await?
        .ok_or("stored A")?;
    assert_eq!(
        stored_first.updated_at_ms, 1000,
        "activity is not persisted"
    );

    // A fresh hub has no observations and falls back to the original record times.
    let reopened = Arc::new(ProviderSessionEventHub::new(store));
    let reopened_context = Arc::new(RouterSessionAppServerContext::new(
        backend.endpoint.clone(),
        ScriptedSessionBackend::actor()?,
        backend,
        reopened,
        test_support::model_catalog(tokio::sync::watch::channel(Vec::new()).1),
    ));
    let reopened_page = list(reopened_context, Value::Null).await?;
    assert_eq!(reopened_page["data"][0]["id"], thread_alias(&second));
    assert_eq!(reopened_page["data"][0]["updatedAt"], 2);
    Ok(())
}
