//! Host lifecycle publication is ordered with replay and visible on return.

use crate::{ExternalProviderLaunch, ExternalProviderRuntime};
use acp_client_runtime::{ProviderPersistenceTarget, ProviderSettingKind};
use collaboration_contracts::SessionEventHub;
use collaboration_protocol::{ProviderRequestedPolicy, ProviderWorkingDirectory, RouterAccess};
use collaboration_runtime::ProviderOperationStore;
use collaboration_runtime::ProviderSessionEventHub;
use collaboration_runtime::ProviderSessionRecord;
use message_board::{EndpointId, ServiceId, SessionEndpointRef, SessionId, SessionRef};
use session_event_model::{SessionEvent, SessionState};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::Mutex;

const LIFECYCLE_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/host-hub-lifecycle-agent.ts"
);

const SETTINGS_UPDATES_FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/host-hub-lifecycle-agent.ts"
);

type Fixture = (
    tempfile::TempDir,
    Arc<Mutex<ProviderOperationStore>>,
    Arc<ProviderSessionEventHub>,
    ExternalProviderRuntime,
    SessionRef,
);

fn require_lifecycle_fact(
    condition: bool,
    message: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    if condition {
        Ok(())
    } else {
        Err(std::io::Error::other(message).into())
    }
}

/// Regression: SDK-only replay sinks do not exercise the Host's Closed hub state.
#[tokio::test]
async fn closed_session_loads_real_history_through_the_host_hub() {
    // Arrange: real existing behavior peer, Host runtime, ordered sink and production hub.
    let root = tempfile::tempdir().expect("root");
    let receipt = root.path().join("peer.jsonl");
    let store = Arc::new(Mutex::new(
        ProviderOperationStore::open(&root.path().join("operations.sqlite"))
            .await
            .expect("store"),
    ));
    let hub = Arc::new(ProviderSessionEventHub::new(store));
    let endpoint = SessionEndpointRef {
        service_id: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())
            .expect("service"),
        endpoint_id: EndpointId::try_from("cursor-local".to_owned()).expect("endpoint"),
    };
    let runtime = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        ExternalProviderRuntime::initialize_with_mcp_http_and_hub(
            ExternalProviderLaunch {
                executable: PathBuf::from("node"),
                arguments: vec![
                    concat!(
                        env!("CARGO_MANIFEST_DIR"),
                        "/../acp-client-runtime/tests/support/behavior-agent.ts"
                    )
                    .into(),
                    receipt.display().to_string(),
                    "lifecycle".into(),
                ],
                environment: vec![],
                persistence_target: ProviderPersistenceTarget::Unspecified,
            },
            acp_client_runtime::ProviderModelPicker::Standard,
            "fixture",
            "http://127.0.0.1:1/mcp",
            Arc::clone(&hub),
            endpoint.clone(),
            Arc::new(acp_provider_adapters::StandardAcpBehavior),
        ),
    )
    .await
    .expect("bounded initialization")
    .expect("runtime");
    // Act/Assert: a fallible scenario always reaches owned runtime shutdown, including RED.
    let observed = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let provider_id = runtime.create_session(root.path().to_path_buf()).await?;
        let session = SessionRef {
            endpoint,
            session_id: SessionId::try_from(provider_id.clone())?,
        };
        for text in ["first owned input", "second owned input"] {
            let prompt = runtime.prompt(provider_id.clone(), text.into()).await?;
            require_lifecycle_fact(prompt.output == "beforeafter", "real prompt output missing")?;
            let mut subscribed = hub.attach(session.clone()).await?;
            runtime.close_session(provider_id.clone()).await?;
            loop {
                if matches!(
                    subscribed.receiver.recv().await?.event,
                    SessionEvent::StateChanged {
                        state: SessionState::Closed
                    }
                ) {
                    break;
                }
            }
            require_lifecycle_fact(
                hub.state(session.clone()).await? == SessionState::Closed,
                "close must remain visibly Closed",
            )?;
            if text == "first owned input" {
                runtime
                    .resume_session(provider_id.clone(), root.path().to_path_buf())
                    .await?;
                let resumed = hub.attach(session.clone()).await?;
                require_lifecycle_fact(
                    !resumed.snapshot.iter().any(|item| {
                        matches!(
                            item.event,
                            SessionEvent::ItemStarted { .. } | SessionEvent::ItemUpdated { .. }
                        )
                    }),
                    "resume must not replay",
                )?;
            }
        }
        let before_load = hub.attach(session.clone()).await?;
        runtime
            .load_session(provider_id.clone(), root.path().to_path_buf())
            .await?;
        let loaded = hub.attach(session.clone()).await?;
        require_lifecycle_fact(
            loaded.epoch > before_load.epoch,
            "load must publish a new replay epoch",
        )?;
        let replayed = loaded
            .snapshot
            .iter()
            .filter_map(|event| match &event.event {
                SessionEvent::ItemStarted { item } | SessionEvent::ItemUpdated { item }
                    if matches!(item.kind, session_event_model::SessionItemKind::UserMessage | session_event_model::SessionItemKind::AgentMessage) =>
                {
                    Some((item.kind.clone(),item.text.as_deref()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        require_lifecycle_fact(
            replayed == [
                (session_event_model::SessionItemKind::UserMessage,Some("first owned input")),
                (session_event_model::SessionItemKind::AgentMessage,Some("beforeafter")),
                (session_event_model::SessionItemKind::UserMessage,Some("second owned input")),
                (session_event_model::SessionItemKind::AgentMessage,Some("beforeafter")),
            ],
            "load must replay actual owned history in order",
        )?;
        let historical_ends=loaded.snapshot.iter().filter(|item|matches!(&item.event,SessionEvent::TurnEnded {outcome:session_event_model::TurnOutcome::Ended {stop_reason:session_event_model::StopReason::Unknown(reason),local_cause:None},..} if reason=="replayed")).count();
        require_lifecycle_fact(historical_ends==2,"both historical turns must be marked replayed, never live completions")?;
        require_lifecycle_fact(
            hub.state(session.clone()).await? == SessionState::Idle,
            "loaded Session must be active and Idle",
        )?;
        require_lifecycle_fact(
            runtime
                .prompt(provider_id.clone(), "after actual load".into())
                .await?
                .output
                == "beforeafter",
            "loaded Session must give a new reply",
        )?;
        runtime.close_session(provider_id.clone()).await?;
        let rows = std::fs::read_to_string(&receipt)?
            .lines()
            .map(serde_json::from_str::<serde_json::Value>)
            .collect::<Result<Vec<_>, _>>()?;
        let loads = rows
            .iter()
            .filter(|row| {
                row.get("method").and_then(serde_json::Value::as_str) == Some("session/load")
            })
            .collect::<Vec<_>>();
        require_lifecycle_fact(
            loads.len() == 1
                && loads.iter().all(|row| {
                    row.pointer("/params/sessionId")
                        .and_then(serde_json::Value::as_str)
                        == Some(provider_id.as_str())
                }),
            "exactly one real load must name the original Session",
        )?;
        Ok::<(), Box<dyn std::error::Error>>(())
    })
    .await;
    tokio::time::timeout(std::time::Duration::from_secs(3), runtime.shutdown())
        .await
        .expect("owned shutdown");
    assert!(
        matches!(observed, Ok(Ok(()))),
        "real Host lifecycle: {observed:?}"
    );
}

async fn fixture(mode: &str) -> Fixture {
    fixture_with_script(LIFECYCLE_FIXTURE, &[mode]).await
}

async fn fixture_with_script(script: &str, arguments: &[&str]) -> Fixture {
    let root = tempfile::tempdir().expect("fixture root");
    let store = Arc::new(Mutex::new(
        ProviderOperationStore::open(&root.path().join("operations.sqlite"))
            .await
            .expect("store"),
    ));
    let hub = Arc::new(ProviderSessionEventHub::new(Arc::clone(&store)));
    let endpoint = SessionEndpointRef {
        service_id: ServiceId::try_from("00000000-0000-4000-8000-000000000001".to_owned())
            .expect("service"),
        endpoint_id: EndpointId::try_from("cursor-local".to_owned()).expect("endpoint"),
    };
    let session = SessionRef {
        endpoint: endpoint.clone(),
        session_id: SessionId::try_from("fixture-session".to_owned()).expect("session"),
    };
    let runtime = ExternalProviderRuntime::initialize_with_mcp_http_and_hub(
        ExternalProviderLaunch {
            persistence_target: ProviderPersistenceTarget::Unspecified,
            executable: PathBuf::from("node"),
            arguments: [script.to_owned()]
                .into_iter()
                .chain(arguments.iter().map(|argument| (*argument).to_owned()))
                .collect(),
            environment: Vec::new(),
        },
        acp_client_runtime::ProviderModelPicker::Standard,
        "fixture",
        "http://127.0.0.1:1/mcp",
        Arc::clone(&hub),
        endpoint,
        std::sync::Arc::new(acp_provider_adapters::StandardAcpBehavior),
    )
    .await
    .expect("runtime");
    (root, store, hub, runtime, session)
}

async fn record_session(store: &Arc<Mutex<ProviderOperationStore>>, session: &SessionRef) {
    let target: collaboration_protocol::SessionRef =
        serde_json::from_value(serde_json::to_value(session).expect("session JSON"))
            .expect("control SessionRef");
    let mut owner = target.clone();
    owner.session_id =
        collaboration_protocol::SessionId::try_from("owner-session".to_owned()).expect("owner ID");
    store
        .lock()
        .await
        .record_session(&ProviderSessionRecord {
            target: target.clone(),
            working_directory: ProviderWorkingDirectory::try_from("/tmp".to_owned()).expect("cwd"),
            requested_policy: ProviderRequestedPolicy {
                access: RouterAccess::WriteRestricted,
            },
            created_by: owner.clone().into(),
            approver: owner.into(),
            updated_at_ms: 1,
        })
        .await
        .expect("session record");
}

async fn assert_listed_idle(hub: &ProviderSessionEventHub, session: &SessionRef) {
    let rows = hub
        .sessions(session.endpoint.clone())
        .await
        .expect("hub inventory");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, SessionState::Idle);
    let attached = hub.attach(session.clone()).await.expect("hub attach");
    assert!(matches!(
        attached.snapshot.get(attached.snapshot.len().saturating_sub(2)).map(|event| &event.event),
        Some(SessionEvent::CapabilitiesChanged { capabilities })
            if capabilities.elicitation
                && matches!(capabilities.auth_status, session_event_model::ProviderAuthStatus::NotReported)
    ));
    assert!(matches!(
        attached
            .snapshot
            .get(attached.snapshot.len().saturating_sub(3))
            .map(|event| &event.event),
        Some(SessionEvent::SettingsChanged { .. })
    ));
    assert!(matches!(
        attached.snapshot.last().map(|event| &event.event),
        Some(SessionEvent::StateChanged {
            state: SessionState::Idle
        })
    ));
}

#[tokio::test]
async fn create_publishes_idle_before_the_session_is_returned() {
    let (_root, store, hub, runtime, session) = fixture("create").await;
    let provider_session_id = runtime
        .create_session(PathBuf::from("/tmp"))
        .await
        .expect("session/new");
    assert_eq!(provider_session_id, "fixture-session");
    record_session(&store, &session).await;
    assert_listed_idle(&hub, &session).await;
    runtime.shutdown().await;
}

#[tokio::test]
async fn load_replays_historical_items_before_publishing_idle() {
    let (_root, store, hub, runtime, session) = fixture("load").await;
    record_session(&store, &session).await;
    runtime
        .load_session("fixture-session".into(), PathBuf::from("/tmp"))
        .await
        .expect("session/load");
    assert_listed_idle(&hub, &session).await;
    let attached = hub.attach(session).await.expect("replayed attach");
    assert!(matches!(
        attached.snapshot.first().map(|event| &event.event),
        Some(SessionEvent::TurnStarted { .. })
    ));
    assert!(
        attached
            .snapshot
            .iter()
            .any(|event| matches!(event.event, SessionEvent::ItemStarted { .. }))
    );
    assert!(matches!(
        attached.snapshot[attached.snapshot.len() - 4].event,
        SessionEvent::TurnEnded { .. }
    ));
    runtime.shutdown().await;
}

#[tokio::test]
async fn resume_invalidates_history_before_publishing_idle() {
    let (_root, store, hub, runtime, session) = fixture("resume").await;
    record_session(&store, &session).await;
    runtime
        .resume_session("fixture-session".into(), PathBuf::from("/tmp"))
        .await
        .expect("session/resume");
    assert_listed_idle(&hub, &session).await;
    let attached = hub.attach(session).await.expect("resumed attach");
    assert_eq!(attached.snapshot.len(), 3);
    runtime.shutdown().await;
}

#[tokio::test]
async fn create_setting_change_and_idle_update_refresh_hub_model() {
    let (root, store, hub, runtime, session) =
        fixture_with_script(SETTINGS_UPDATES_FIXTURE, &["settings"]).await;
    runtime
        .create_session(root.path().to_path_buf())
        .await
        .expect("session/new");
    record_session(&store, &session).await;
    let created = hub
        .sessions(session.endpoint.clone())
        .await
        .expect("created inventory");
    assert_eq!(created[0].model.as_deref(), Some("a"));
    assert_eq!(created[0].mode.as_deref(), Some("default"));

    runtime
        .set_setting(
            "fixture-session".into(),
            acp_client_runtime::ProviderSettingRequest {
                kind: ProviderSettingKind::Model,
                value: "b".into(),
                intent: session_event_model::SettingApplicationIntent::ExplicitChange,
                expected_model: None,
            },
        )
        .await
        .expect("set model");
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let rows = hub
                .sessions(session.endpoint.clone())
                .await
                .expect("model inventory");
            if rows[0].model.as_deref() == Some("b") {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("model setting event");
    runtime
        .set_setting(
            "fixture-session".into(),
            acp_client_runtime::ProviderSettingRequest {
                kind: ProviderSettingKind::Mode,
                value: "ask".into(),
                intent: session_event_model::SettingApplicationIntent::ExplicitChange,
                expected_model: None,
            },
        )
        .await
        .expect("set mode");
    let idle_update = tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let rows = hub
                .sessions(session.endpoint.clone())
                .await
                .expect("idle inventory");
            if rows[0].model.as_deref() == Some("c") && rows[0].mode.as_deref() == Some("ask") {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    if idle_update.is_err() {
        panic!(
            "idle config update missing; snapshot: {:?}",
            hub.attach(session.clone())
                .await
                .expect("diagnostic attach")
                .snapshot
        );
    }
    runtime.shutdown().await;
}
