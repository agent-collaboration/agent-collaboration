use super::*;
use std::path::PathBuf;

#[tokio::test]
async fn decision_is_single_use_actor_bound_and_maps_only_offered_options() {
    let (broker, generation, _directory) = fixture_broker().await;
    let expiry = (chrono::Utc::now() + chrono::Duration::minutes(5)).to_rfc3339();
    let (params, receiver) = insert_pending(&broker, generation, expiry).await;
    assert_eq!(broker.list(true).await.approvals.len(), 1);
    let mut requester = params.clone();
    requester.actor = board_identity(&session(&broker.service_id, "requester")).expect("actor");
    assert!(matches!(
        broker.decide(requester).await,
        Err(ApprovalDecisionError::Code("selfDecision"))
    ));
    let mut wrong = params.clone();
    wrong.actor = board_identity(&session(&broker.service_id, "other")).expect("actor");
    assert!(matches!(
        broker.decide(wrong).await,
        Err(ApprovalDecisionError::Code("wrongActor"))
    ));
    let mut unavailable = params.clone();
    unavailable.decision = Some(ApprovalDecision::AllowForSession);
    assert!(matches!(
        broker.decide(unavailable).await,
        Err(ApprovalDecisionError::OptionNotOffered { .. })
    ));
    let receipt = broker
        .decide(params.clone())
        .await
        .unwrap_or_else(|error| panic!("decision: {error}"));
    assert_eq!(receipt.state, ApprovalState::Decided);
    assert!(matches!(
        broker.decide(params).await,
        Err(ApprovalDecisionError::Code("approvalNotPending"))
    ));
    assert_eq!(
        receiver.await.unwrap_or_else(|_| panic!("outcome")),
        BrokeredApprovalOutcome::Selected {
            option_id: "native-accept".to_owned()
        }
    );
    assert!(broker.list(false).await.approvals.is_empty());
}

#[tokio::test]
async fn native_request_by_its_own_approver_is_refused_without_legacy_history() {
    use codex_acp_adapter::{ApprovalBroker as _, ApprovalRoute, BrokeredApprovalRequest};
    use collaboration_protocol::RouterAccess;

    let (broker, generation, _directory) = fixture_broker().await;
    let self_approver = session(&broker.service_id, "self-approver");
    broker
        .register_route(ApprovalRoute {
            thread_id: "self-approver".to_owned(),
            created_by: self_approver.clone(),
            approver: self_approver.clone(),
            access: RouterAccess::WorkspaceWrite,
            scratch_path: "/tmp/approval-self-approver".to_owned(),
            root_message_id: None,
        })
        .await
        .expect("route registered");

    let outcome = broker
        .request(BrokeredApprovalRequest {
            thread_id: "self-approver".to_owned(),
            generation,
            request: json!({
                "method":"item/commandExecution/requestApproval",
                "params":{"options":[{"optionId":"native-accept"}]}
            }),
        })
        .await
        .expect("self-approval refusal settles");

    assert_eq!(outcome, BrokeredApprovalOutcome::Cancelled);
    assert!(broker.list(false).await.approvals.is_empty());
}

#[tokio::test]
async fn missing_native_approval_route_emits_only_typed_safe_diagnostic() {
    use codex_acp_adapter::{ApprovalBroker as _, BrokeredApprovalRequest};

    let (broker, generation, _directory) = fixture_broker().await;
    let provider_session_id = "requester";
    let request = json!({
        "method":"item/commandExecution/requestApproval",
        "params":{"options":[],"command":"PRIVATE_COMMAND_SENTINEL"}
    });
    let diagnostic = native_approval_refusal_diagnostic(
        &broker.backend.endpoint,
        provider_session_id,
        &request,
        NativeApprovalRefusalReason::MissingRoute,
    );

    assert_eq!(
        diagnostic,
        NativeApprovalRefusalDiagnostic {
            endpoint: "codex-local".to_owned(),
            provider_session_id: "requester".to_owned(),
            method: "item/commandExecution/requestApproval",
            reason_code: "nativeApprovalRouteUnavailable",
        }
    );
    assert!(
        broker
            .request(BrokeredApprovalRequest {
                thread_id: provider_session_id.to_owned(),
                generation,
                request,
            })
            .await
            .is_err()
    );
    assert!(broker.list(false).await.approvals.is_empty());
}

#[tokio::test]
async fn native_request_with_empty_option_mapping_is_refused_without_legacy_history() {
    use codex_acp_adapter::{ApprovalBroker as _, ApprovalRoute, BrokeredApprovalRequest};
    use collaboration_protocol::RouterAccess;

    let (broker, generation, _directory) = fixture_broker().await;
    let requester = session(&broker.service_id, "requester");
    let approver = session(&broker.service_id, "approver");
    broker
        .register_route(ApprovalRoute {
            thread_id: "requester".to_owned(),
            created_by: requester.clone(),
            approver: approver.clone(),
            access: RouterAccess::WorkspaceWrite,
            scratch_path: "/tmp/approval-empty-options".to_owned(),
            root_message_id: None,
        })
        .await
        .expect("route registered");

    let outcome = broker
        .request(BrokeredApprovalRequest {
            thread_id: "requester".to_owned(),
            generation,
            request: json!({
                "method":"item/commandExecution/requestApproval",
                "params":{"options":[]}
            }),
        })
        .await
        .expect("invalid native approval is refused");

    assert_eq!(outcome, BrokeredApprovalOutcome::Cancelled);
    assert!(broker.pending.lock().await.is_empty());
    assert!(broker.list(false).await.approvals.is_empty());
}

#[tokio::test]
async fn expired_and_old_generation_decisions_remove_pending_legacy_requests() {
    let (broker, generation, _directory) = fixture_broker().await;
    let (expired, _receiver) = insert_pending(
        &broker,
        generation.clone(),
        (chrono::Utc::now() - chrono::Duration::seconds(1)).to_rfc3339(),
    )
    .await;
    assert!(matches!(
        broker.decide(expired).await,
        Err(ApprovalDecisionError::Code("expired"))
    ));
    assert!(broker.list(false).await.approvals.is_empty());

    let (broker, generation, _directory) = fixture_broker().await;
    let (stale, _receiver) = insert_pending(
        &broker,
        generation,
        (chrono::Utc::now() + chrono::Duration::minutes(5)).to_rfc3339(),
    )
    .await;
    broker
        .backend
        .gate
        .retire()
        .unwrap_or_else(|error| panic!("retire: {error}"));
    let next_generation: CodexGeneration = serde_json::from_value(json!({
        "serviceEpoch": String::from(broker.service_id.clone()), "generation": 2
    }))
    .unwrap_or_else(|error| panic!("next generation: {error}"));
    broker
        .backend
        .gate
        .activate(
            next_generation,
            PathBuf::from("/tmp/approval-backend-2.sock"),
            None,
        )
        .unwrap_or_else(|error| panic!("reactivate: {error}"));
    assert!(matches!(
        broker.decide(stale).await,
        Err(ApprovalDecisionError::Code("oldGeneration"))
    ));
    assert!(broker.list(false).await.approvals.is_empty());
}

#[tokio::test]
async fn malformed_legacy_routes_fail_closed_and_are_preserved() {
    let directory = tempfile::tempdir().expect("temporary import directory");
    let routes = directory.path().join("approval-routes.json");
    tokio::fs::write(&routes, br#"[{"threadId":"thread","access":"invalid"}]"#)
        .await
        .expect("write corrupt route fixture");
    let interaction_store = Arc::new(
        interaction_storage::InteractionStore::open(directory.path().join("interaction.sqlite"))
            .await
            .expect("interaction store"),
    );
    let imported_at_ms = chrono::DateTime::parse_from_rfc3339("2026-10-31T12:00:00Z")
        .expect("fixed import time")
        .timestamp_millis();
    let imported = ServiceInteractionBroker::import_legacy_approval_routes(
        interaction_store.as_ref(),
        &routes,
        imported_at_ms,
    )
    .await;
    assert!(matches!(imported, Err(ApprovalBrokerError::Unavailable)));
    assert!(routes.exists(), "corrupt legacy route file was deleted");
    assert!(
        interaction_store
            .approval_route_json("thread", imported_at_ms + 1)
            .await
            .expect("read absent route")
            .is_none()
    );
}

#[tokio::test]
async fn valid_legacy_routes_are_removed_after_loading() {
    use codex_acp_adapter::{ApprovalBroker as _, ApprovalRoute};
    use collaboration_protocol::RouterAccess;

    let service_id = crate::new_service_uuid().expect("service id");
    let generation: CodexGeneration = serde_json::from_value(json!({
        "serviceEpoch": String::from(service_id.clone()), "generation": 1
    }))
    .expect("generation");
    let gate = crate::NativeGenerationGate::default();
    gate.activate(
        generation,
        PathBuf::from("/tmp/approval-route-import.sock"),
        None,
    )
    .expect("activate native generation");
    let endpoint = EndpointRef {
        service_id: service_id.clone(),
        endpoint_id: "codex-local".to_owned().try_into().expect("endpoint id"),
    };
    let directory = tempfile::tempdir().expect("temporary broker directory");
    let routes_path = directory.path().join("approval-routes.json");
    let route = ApprovalRoute {
        thread_id: "route-thread".to_owned(),
        created_by: SessionRef {
            endpoint: endpoint.clone(),
            session_id: "creator".to_owned().try_into().expect("creator id"),
        },
        approver: SessionRef {
            endpoint: endpoint.clone(),
            session_id: "approver".to_owned().try_into().expect("approver id"),
        },
        access: RouterAccess::WorkspaceWrite,
        scratch_path: "/tmp/approval-route-scratch".to_owned(),
        root_message_id: None,
    };
    tokio::fs::write(
        &routes_path,
        serde_json::to_vec(&vec![route.clone()]).expect("serialize route fixture"),
    )
    .await
    .expect("write legacy route fixture");

    let imported_at_ms = chrono::DateTime::parse_from_rfc3339("2026-10-31T12:00:00Z")
        .expect("fixed import time")
        .timestamp_millis();
    let interaction_store = Arc::new(
        interaction_storage::InteractionStore::open(directory.path().join("interaction.sqlite"))
            .await
            .expect("interaction store"),
    );
    let database_path = directory.path().join("interaction.sqlite");
    let native_backend = NativeControlBackend {
        endpoint,
        gate,
        codex_home: directory.path().to_path_buf(),
    };
    ServiceInteractionBroker::import_legacy_approval_routes(
        interaction_store.as_ref(),
        &routes_path,
        imported_at_ms,
    )
    .await
    .expect("import valid legacy route file");
    let broker = ServiceInteractionBroker::load(
        service_id.clone(),
        native_backend.clone(),
        Arc::clone(&interaction_store),
    )
    .await
    .expect("load broker from valid route file");

    assert!(
        !routes_path.exists(),
        "a valid legacy route file remains after import"
    );
    let loaded_route = broker
        .route(&route.thread_id)
        .await
        .expect("lookup route")
        .expect("imported route");
    assert_eq!(
        serde_json::to_value(loaded_route).expect("serialize loaded route"),
        serde_json::to_value(&route).expect("serialize expected route")
    );
    drop(broker);
    drop(interaction_store);

    let restarted_store = Arc::new(
        interaction_storage::InteractionStore::open(database_path)
            .await
            .expect("reopen interaction store"),
    );
    let restarted_broker =
        ServiceInteractionBroker::load(service_id, native_backend, restarted_store)
            .await
            .expect("restart broker from interaction database");
    assert!(
        restarted_broker
            .route(&route.thread_id)
            .await
            .expect("lookup restarted route")
            .is_some(),
        "route did not survive broker and database restart"
    );
}

#[tokio::test]
async fn typed_interaction_rejects_self_approver_and_corrupt_stored_rows() {
    use message_board::Identity;

    let (broker, _generation, _directory) = fixture_broker().await;
    let requester: message_board::SessionRef = serde_json::from_value(
        serde_json::to_value(session(&broker.service_id, "provider-session"))
            .expect("requester JSON"),
    )
    .expect("board requester");
    assert!(matches!(
        broker
            .request_typed_approval(
                requester.clone(),
                Identity::Session { session: requester },
                typed_approval_request("self-request"),
                tokio_util::sync::CancellationToken::new(),
                tokio_util::sync::CancellationToken::new(),
                None
            )
            .await,
        Err(crate::interaction_broker::InteractionHistoryError::SelfApprover)
    ));
    let fault =
        crate::interaction_broker::native_history_fault_fixture::NativeHistoryFaults::new(&broker);
    fault.execute_all(&["INSERT INTO typed_interaction_history VALUES ('bad', '{\"requestId\":\"other\"}', '2026-10-01T00:00:00.000000000Z')"])
        .await.expect("corrupt actual native record");
    assert_eq!(
        fault
            .text("SELECT record_json FROM typed_interaction_history WHERE request_id='bad'")
            .await
            .expect("native corruption readback"),
        "{\"requestId\":\"other\"}"
    );
    let reload = ServiceInteractionBroker::load(
        broker.service_id.clone(),
        broker.backend.clone(),
        broker.interaction_store(),
    )
    .await;
    assert!(matches!(reload, Err(ApprovalBrokerError::Unavailable)));
}

#[tokio::test]
async fn typed_approval_rejects_foreign_service_participants_without_pending_history() {
    use message_board::Identity;

    let (broker, _, _) = fixture_broker().await;
    let foreign_service = crate::new_service_uuid().expect("foreign service");
    let foreign_requester =
        board_session_ref(&session(&foreign_service, "provider-session")).expect("requester");
    let local_approver =
        board_session_ref(&session(&broker.service_id, "approver")).expect("approver");
    assert!(matches!(
        broker
            .request_typed_approval(
                foreign_requester,
                Identity::Session {
                    session: local_approver,
                },
                typed_approval_request("foreign-service"),
                tokio_util::sync::CancellationToken::new(),
                tokio_util::sync::CancellationToken::new(),
                None
            )
            .await,
        Err(crate::interaction_broker::InteractionHistoryError::Unavailable)
    ));
    let local_requester =
        board_session_ref(&session(&broker.service_id, "provider-session")).expect("requester");
    let foreign_approver =
        board_session_ref(&session(&foreign_service, "approver")).expect("foreign approver");
    assert!(matches!(
        broker
            .request_typed_approval(
                local_requester,
                Identity::Session {
                    session: foreign_approver,
                },
                typed_approval_request("foreign-approver"),
                tokio_util::sync::CancellationToken::new(),
                tokio_util::sync::CancellationToken::new(),
                None
            )
            .await,
        Err(crate::interaction_broker::InteractionHistoryError::Unavailable)
    ));
    assert!(broker.list_interactions().await.is_empty());
}

#[tokio::test]
async fn refused_typed_offer_preserves_reviewed_fields_and_order_without_legacy_write() {
    use message_board::{HumanId, Identity};

    let (broker, _, directory) = fixture_broker().await;
    let requester =
        board_session_ref(&session(&broker.service_id, "provider-session")).expect("requester");
    let approver = Identity::Human {
        human_id: HumanId::try_from("owner".to_owned()).expect("human ID"),
    };
    let refusal = crate::interaction_broker::RefusedTypedApproval {
        request_id: "malformed-offer".into(),
        title: "Run command".into(),
        description: Some("Reviewed description".into()),
        subject: None,
        options: vec![
            crate::interaction_broker::RefusedApprovalOption {
                option_id: "duplicate".into(),
                label: "First".into(),
                provider_kind: "AllowOnce".into(),
            },
            crate::interaction_broker::RefusedApprovalOption {
                option_id: "duplicate".into(),
                label: "Second".into(),
                provider_kind: "RejectOnce".into(),
            },
        ],
        reason: "permission options contain a duplicate identifier".into(),
    };
    broker
        .record_typed_refusal(requester, approver, refusal.clone())
        .await
        .expect("persist refusal");
    let recorded = broker.list_interactions().await;
    assert!(matches!(
        &recorded[0],
        crate::interaction_broker::InteractionHistoryRecord::RefusedApproval { refusal: stored, .. }
            if stored.request_id == "malformed-offer" && stored.options[0].label == "First" && stored.options[1].label == "Second"
    ));
    assert!(!directory.join("approval-history.json").exists());
    let detailed = broker.list_detailed(false).await.expect("detailed list");
    let row = detailed
        .approvals
        .iter()
        .find(|row| row.request_id == "malformed-offer")
        .expect("refusal listed");
    assert_eq!(row.state, collaboration_protocol::ApprovalState::Refused);
    assert_eq!(row.reason.as_deref(), Some(refusal.reason.as_str()));
    let reloaded = ServiceInteractionBroker::load(
        broker.service_id.clone(),
        broker.backend.clone(),
        broker.interaction_store(),
    )
    .await
    .expect("reload typed refusal");
    assert_eq!(reloaded.list_interactions().await, recorded);
}

#[tokio::test]
async fn self_approver_refusal_is_recorded_and_listed_with_a_fix() {
    let (broker, _, _) = fixture_broker().await;
    let requester =
        board_session_ref(&session(&broker.service_id, "provider-session")).expect("requester");
    let refusal = crate::interaction_broker::RefusedTypedApproval {
        request_id: "self-offer".into(),
        title: "Run command".into(),
        description: None,
        subject: None,
        options: Vec::new(),
        reason: "set a different approver".into(),
    };
    broker
        .record_typed_refusal(
            requester.clone(),
            message_board::Identity::Session { session: requester },
            refusal,
        )
        .await
        .expect("self refusal recorded");
    let row = broker
        .list_detailed(false)
        .await
        .expect("detailed list")
        .approvals
        .into_iter()
        .find(|row| row.request_id == "self-offer")
        .expect("self refusal listed");
    assert_eq!(
        row.state,
        collaboration_protocol::ApprovalState::ApproverIsRequester
    );
    assert_eq!(row.reason.as_deref(), Some("set a different approver"));
}

// R17: the exact option ID is returned to the waiting agent. Persistent scope
// and destination remain visible in the pending request before the decision.
