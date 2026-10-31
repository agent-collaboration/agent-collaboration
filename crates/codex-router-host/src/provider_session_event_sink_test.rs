use super::*;
use acp_client_runtime::{ApprovalPortOutcome, InteractionPort, SessionEventSink};
use collaboration_contracts::SessionEventHub;
use collaboration_protocol::{
    CodexGeneration, EndpointId as ProtocolEndpointId, EndpointRef as ProtocolEndpointRef,
    GenerationNumber, OperationId, ProviderIdentity, SessionId as ProtocolSessionId,
    SessionRef as ProtocolSessionRef, UuidIdentity,
};
use collaboration_runtime::NativeControlBackend;
use collaboration_runtime::NativeGenerationGate;
use collaboration_runtime::ProviderOperationStore;
use collaboration_runtime::ProviderSessionEventHub;
use collaboration_runtime::ServiceInteractionBroker;
use message_board::{EndpointId, ServiceId, SessionEndpointRef, SessionId, SessionRef};
use session_event_model::{SessionEvent, TurnOutcome};
use std::sync::Arc;
use tokio::sync::Mutex;

#[tokio::test]
async fn typed_broker_interactions_publish_pending_and_resolution_in_order() {
    let root = tempfile::tempdir().expect("temporary store");
    let service_id = UuidIdentity::try_from("00000000-0000-4000-8000-000000000001".to_owned())
        .expect("service ID");
    let provider_endpoint = ProtocolEndpointRef {
        service_id: service_id.clone(),
        endpoint_id: ProtocolEndpointId::try_from("fixture-provider".to_owned())
            .expect("provider endpoint"),
    };
    let approver_endpoint = ProtocolEndpointRef {
        service_id: service_id.clone(),
        endpoint_id: ProtocolEndpointId::try_from("codex-local".to_owned())
            .expect("approver endpoint"),
    };
    let target = ProtocolSessionRef {
        endpoint: provider_endpoint,
        session_id: ProtocolSessionId::try_from("session-one".to_owned()).expect("session"),
    };
    let requester = ProtocolSessionRef {
        endpoint: approver_endpoint.clone(),
        session_id: ProtocolSessionId::try_from("requester".to_owned()).expect("requester"),
    };
    let approver = ProviderIdentity::Human {
        human_id: "owner".to_owned().try_into().expect("human ID"),
    };
    let broker = ServiceInteractionBroker::load(
        service_id.clone(),
        NativeControlBackend {
            codex_home: root.path().to_owned(),
            endpoint: approver_endpoint,
            gate: NativeGenerationGate::default(),
        },
        root.path().join("approval-routes.json"),
    )
    .await
    .expect("broker");
    let store = ProviderOperationStore::open(&root.path().join("operations.sqlite"))
        .await
        .expect("provider store");
    let hub = Arc::new(ProviderSessionEventHub::new(Arc::new(Mutex::new(store))));
    let endpoint = SessionEndpointRef {
        service_id: ServiceId::try_from(String::from(service_id)).expect("service"),
        endpoint_id: EndpointId::try_from("fixture-provider".to_owned()).expect("endpoint"),
    };
    let session = SessionRef {
        endpoint: endpoint.clone(),
        session_id: SessionId::try_from("session-one".to_owned()).expect("session"),
    };
    let sink = Arc::new(HubSessionEventSink::new(Arc::clone(&hub), endpoint));
    let port = Arc::new(crate::acp_interaction_port::HostInteractionPort::default());
    port.install_broker(Arc::clone(&broker)).await;
    port.install_event_sink(Arc::clone(&sink) as Arc<dyn SessionEventSink>)
        .await;
    sink.publish(
        "session-one",
        SessionEvent::TurnStarted {
            turn_id: "turn-one".into(),
            input_id: session_event_model::InputId::new("input-one").expect("input ID"),
        },
    )
    .expect("turn start");
    let request = serde_json::from_value(serde_json::json!({
            "requestId":"approval-one", "title":"Run command", "options":[
                {"optionId":"allow-once","label":"Allow once","choice":{"effect":"allow","scope":"once"}}
            ]
        }))
        .expect("approval request");
    let question_target = target.clone();
    let request_task = tokio::spawn({
        let port = Arc::clone(&port);
        let context = crate::ExternalProviderApprovalContext {
            requester: requester.clone().into(),
            approver: approver.clone(),
            target,
            operation_id: OperationId::generate(),
            binding_generation: CodexGeneration {
                service_epoch: UuidIdentity::try_from(
                    "00000000-0000-4000-8000-000000000001".to_owned(),
                )
                .expect("epoch"),
                generation: GenerationNumber::try_from(1).expect("generation"),
            },
            binding_retirement: tokio_util::sync::CancellationToken::new(),
        };
        async move {
            port.request_approval(
                context,
                request,
                tokio_util::sync::CancellationToken::new(),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
        }
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Ok(attached) = hub.attach(session.clone()).await
                && attached.snapshot.len() == 2
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("pending interaction published");
    let actor = approver.to_board_identity().expect("board actor");
    broker
        .decide_typed_interaction(
            "approval-one",
            &actor,
            collaboration_runtime::TypedInteractionDecision::SelectApproval {
                option_id: "allow-once".into(),
                acknowledge_persistent: false,
                note: Some("note".into()),
            },
        )
        .await
        .expect("approver decision");
    assert_eq!(
        request_task.await.expect("port task"),
        ApprovalPortOutcome::Selected {
            option_id: "allow-once".into(),
            note: Some("note".into()),
        }
    );
    let question = serde_json::from_value(serde_json::json!({
        "requestId":"question-one","prompt":"Choose count","fields":[
            {"kind":"number","fieldId":"count","label":"Count","description":null,"required":true}
        ]
    }))
    .expect("question request");
    let question_task = tokio::spawn({
        let port = Arc::clone(&port);
        let context = crate::ExternalProviderApprovalContext {
            requester: requester.clone().into(),
            approver: approver.clone(),
            target: question_target.clone(),
            operation_id: OperationId::generate(),
            binding_generation: CodexGeneration {
                service_epoch: UuidIdentity::try_from(
                    "00000000-0000-4000-8000-000000000001".to_owned(),
                )
                .expect("epoch"),
                generation: GenerationNumber::try_from(1).expect("generation"),
            },
            binding_retirement: tokio_util::sync::CancellationToken::new(),
        };
        async move {
            port.request_question(
                context,
                question,
                tokio_util::sync::CancellationToken::new(),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
        }
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Ok(attached) = hub.attach(session.clone()).await
                && attached.snapshot.len() == 4
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("pending question published");
    let response = session_event_model::QuestionResponse::Answered {
        content: serde_json::from_value(serde_json::json!({"count":3})).expect("answer"),
    };
    broker
        .respond_question("question-one", &actor, response.clone())
        .await
        .expect("question answer");
    assert_eq!(question_task.await.expect("question task"), response);
    let cancel_context = || crate::ExternalProviderApprovalContext {
        requester: requester.clone().into(),
        approver: approver.clone(),
        target: question_target.clone(),
        operation_id: OperationId::generate(),
        binding_generation: CodexGeneration {
            service_epoch: UuidIdentity::try_from(
                "00000000-0000-4000-8000-000000000001".to_owned(),
            )
            .expect("epoch"),
            generation: GenerationNumber::try_from(1).expect("generation"),
        },
        binding_retirement: tokio_util::sync::CancellationToken::new(),
    };
    let cancelled_approval = serde_json::from_value(serde_json::json!({
            "requestId":"approval-cancelled", "title":"Cancel command", "options":[
                {"optionId":"allow-once","label":"Allow once","choice":{"effect":"allow","scope":"once"}}
            ]
        })).expect("approval to cancel");
    let approval_cancel_task = tokio::spawn({
        let port = Arc::clone(&port);
        let context = cancel_context();
        async move {
            port.request_approval(
                context,
                cancelled_approval,
                tokio_util::sync::CancellationToken::new(),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
        }
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Ok(attached) = hub.attach(session.clone()).await
                && attached.snapshot.len() == 6
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("cancel approval pending");
    assert_eq!(
        broker
            .decide_typed_interaction(
                "approval-cancelled",
                &actor,
                collaboration_runtime::TypedInteractionDecision::Cancel,
            )
            .await
            .expect("approver cancel"),
        collaboration_runtime::TypedInteractionDecisionOutcome::ApprovalCancelled
    );
    assert_eq!(
        approval_cancel_task.await.expect("approval cancel task"),
        ApprovalPortOutcome::Cancelled
    );
    let cancelled_question = serde_json::from_value(serde_json::json!({
        "requestId":"question-cancelled", "prompt":"Cancel question?", "fields":[
            {"kind":"boolean","fieldId":"yes","label":"Yes","description":null,"required":true}
        ]
    }))
    .expect("question to cancel");
    let question_cancel_task = tokio::spawn({
        let port = Arc::clone(&port);
        let context = cancel_context();
        async move {
            port.request_question(
                context,
                cancelled_question,
                tokio_util::sync::CancellationToken::new(),
                tokio_util::sync::CancellationToken::new(),
            )
            .await
        }
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            if let Ok(attached) = hub.attach(session.clone()).await
                && attached.snapshot.len() == 8
            {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("cancel question pending");
    assert_eq!(
        broker
            .decide_typed_interaction(
                "question-cancelled",
                &actor,
                collaboration_runtime::TypedInteractionDecision::Cancel,
            )
            .await
            .expect("approver cancel"),
        collaboration_runtime::TypedInteractionDecisionOutcome::QuestionCancelled
    );
    assert_eq!(
        question_cancel_task.await.expect("question cancel task"),
        session_event_model::QuestionResponse::Cancelled
    );
    sink.shutdown().await.expect("drain");
    let attached = hub.attach(session).await.expect("attach history");
    assert_eq!(attached.snapshot.len(), 9);
    assert!(matches!(
        &attached.snapshot[0].event,
        SessionEvent::TurnStarted { .. }
    ));
    assert!(matches!(
        &attached.snapshot[1].event,
        SessionEvent::InteractionRequested { .. }
    ));
    assert!(matches!(
        &attached.snapshot[2].event,
        SessionEvent::InteractionResolved { .. }
    ));
    assert!(matches!(
        &attached.snapshot[3].event,
        SessionEvent::InteractionRequested { .. }
    ));
    assert!(matches!(
        &attached.snapshot[4].event,
        SessionEvent::InteractionResolved { .. }
    ));
    for (requested, resolved) in [(5, 6), (7, 8)] {
        assert!(matches!(
            &attached.snapshot[requested].event,
            SessionEvent::InteractionRequested { .. }
        ));
        assert!(matches!(
            &attached.snapshot[resolved].event,
            SessionEvent::InteractionResolved { .. }
        ));
    }
}

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
