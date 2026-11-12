use super::*;
use collaboration_protocol::{CodexGeneration, EndpointId, EndpointRef};
use std::path::PathBuf;

struct FakeApprovalDelivery(DeliveryOutcome);

struct CapturingInteractionDelivery {
    notices: Arc<Mutex<Vec<collaboration_contracts::layer_zero::DeliveryRequest>>>,
    delivered: Arc<tokio::sync::Notify>,
}

impl collaboration_contracts::SessionMessageDelivery for CapturingInteractionDelivery {
    fn deliver<'a>(
        &'a self,
        request: collaboration_contracts::layer_zero::DeliveryRequest,
        _: &'a dyn collaboration_contracts::AttemptEvidenceSink,
    ) -> collaboration_contracts::DeliveryFuture<'a, collaboration_contracts::NativeDeliveryResult>
    {
        Box::pin(async move {
            self.notices.lock().await.push(request);
            self.delivered.notify_one();
            Ok(collaboration_contracts::scripted_delivery_result(
                collaboration_protocol::DeliveryReceipt {
                    outcome: DeliveryOutcome::Started,
                    reachability: Some(collaboration_protocol::SessionReachability::CodexAppServer),
                    client: None,
                },
            ))
        })
    }

    fn reconcile_attempt(
        &self,
        _: collaboration_contracts::AttemptReconciliationContext,
    ) -> collaboration_contracts::DeliveryFuture<'_, collaboration_contracts::AttemptReconciliation>
    {
        Box::pin(async { Ok(collaboration_contracts::AttemptReconciliation::StillUnknown) })
    }
}

#[tokio::test]
async fn typed_approval_and_question_notify_their_session_approver() {
    let (broker, _, _directory) =
        fixture_broker_with_requester_name(Some("Startup requester")).await;
    let requester =
        board_session_ref(&session(&broker.service_id, "provider-session")).expect("requester");
    let approver =
        board_session_ref(&session(&broker.service_id, "approver-session")).expect("approver");
    let notices = Arc::new(Mutex::new(Vec::new()));
    let delivered = Arc::new(tokio::sync::Notify::new());
    broker
        .install_session_delivery(Arc::new(CapturingInteractionDelivery {
            notices: Arc::clone(&notices),
            delivered: Arc::clone(&delivered),
        }))
        .expect("delivery route");
    let approver_identity = message_board::Identity::Session {
        session: approver.clone(),
    };
    let _approval = broker
        .request_typed_approval(
            requester.clone(),
            approver_identity.clone(),
            typed_approval_request("approval-1"),
            tokio_util::sync::CancellationToken::new(),
            tokio_util::sync::CancellationToken::new(),
            None,
        )
        .await
        .expect("approval pending");
    let question = serde_json::from_value(json!({
        "requestId":"question-1","prompt":"Choose?","fields":[
            {"kind":"text","fieldId":"answer","label":"Answer","description":null,"required":true}
        ]
    }))
    .expect("question");
    let _question = broker
        .request_question(requester.clone(), approver_identity, question, None)
        .await
        .expect("question pending");
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let notified = delivered.notified();
            if notices.lock().await.len() == 2 {
                break;
            }
            notified.await;
        }
    })
    .await
    .expect("both notices delivered");
    let notices = notices.lock().await;
    assert_eq!(notices.len(), 2);
    let mut push_ids = std::collections::HashSet::new();
    for notice in notices.iter() {
        assert_eq!(notice.correlation.as_str(), notice.payload.push_id.as_str());
        assert_eq!(notice.mode, collaboration_protocol::MessageDelivery::Auto);
        assert_eq!(
            notice.payload.load_policy,
            collaboration_contracts::LoadPolicy::MayLoad
        );
        assert_eq!(
            String::from(notice.target.endpoint.service_id.clone()),
            approver.endpoint.service_id.as_str()
        );
        assert_eq!(
            String::from(notice.target.endpoint.endpoint_id.clone()),
            approver.endpoint.endpoint_id.as_str()
        );
        assert_eq!(
            String::from(notice.target.session_id.clone()),
            "approver-session"
        );
        assert!(push_ids.insert(notice.payload.push_id.as_str().to_owned()));
        assert_eq!(
            uuid::Uuid::parse_str(notice.payload.push_id.as_str())
                .expect("push UUID")
                .get_version_num(),
            7
        );
        let text = notice.payload.line.as_str();
        let header = collaboration_protocol::parse_push_line_header(text).expect("notice header");
        let expected_line = match header.kind {
            collaboration_protocol::PushKind::Approval => "❓ Router approval @",
            collaboration_protocol::PushKind::Question => "❓ Router question @",
            _ => panic!("typed approval or question notice expected"),
        };
        assert!(text.starts_with(expected_line), "{text}");
        assert!(text.contains("Startup requester"), "{text}");
        match header.kind {
            collaboration_protocol::PushKind::Approval => {
                assert!(text.contains("Approval request: Run command"), "{text}");
            }
            _ => assert!(text.contains("Question: Choose?"), "{text}"),
        }
        let link = text
            .split_whitespace()
            .last()
            .and_then(|link| collaboration_protocol::RouterLink::parse(link).ok())
            .expect("push line link");
        assert_eq!(link.push_id(), &notice.payload.push_id);
    }
}

#[tokio::test]
async fn provider_session_approval_legacy_projection_survives_restart_without_dual_write() {
    let (broker, generation, directory) = fixture_broker().await;
    let requester =
        board_session_ref(&session(&broker.service_id, "provider-session")).expect("requester");
    let approver =
        board_session_ref(&session(&broker.service_id, "approver-session")).expect("approver");
    broker
        .install_session_delivery(Arc::new(CapturingInteractionDelivery {
            notices: Arc::new(Mutex::new(Vec::new())),
            delivered: Arc::new(tokio::sync::Notify::new()),
        }))
        .expect("notice route");
    let target = session(&broker.service_id, "provider-session");
    let prompting_requester = session(&broker.service_id, "prompting-session");
    let receiver = broker
        .request_typed_approval(
            requester,
            message_board::Identity::Session { session: approver },
            typed_approval_request("projected-approval"),
            tokio_util::sync::CancellationToken::new(),
            tokio_util::sync::CancellationToken::new(),
            Some(crate::interaction_broker::TypedApprovalLegacyContext {
                operation_id: collaboration_protocol::OperationId::generate(),
                target: target.clone(),
                generation: generation.clone(),
                requested_by: prompting_requester.clone().into(),
            }),
        )
        .await
        .expect("pending approval");
    let pending = broker.list(true).await.approvals;
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].generation, generation);
    assert_eq!(pending[0].operation["target"], json!(target));
    assert_eq!(pending[0].requester, prompting_requester);
    assert!(!directory.join("approval-history.json").exists());
    assert_eq!(
        broker
            .list_detailed(true)
            .await
            .expect("detailed list")
            .approvals
            .len(),
        1
    );
    drop(receiver);
    drop(broker);
    let service_id = target.endpoint.service_id.clone();
    let gate = crate::NativeGenerationGate::default();
    let endpoint = target.endpoint.clone();
    let interaction_store = Arc::new(
        interaction_storage::InteractionStore::open(directory.join("interaction.sqlite"))
            .await
            .expect("interaction store after restart"),
    );
    let reloaded = ServiceInteractionBroker::load(
        service_id,
        NativeControlBackend {
            endpoint,
            gate,
            codex_home: directory.clone(),
        },
        interaction_store,
    )
    .await
    .expect("reload broker");
    assert!(reloaded.list(true).await.approvals.is_empty());
    let history = reloaded.list(false).await.approvals;
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].request_id, "projected-approval");
    assert_eq!(history[0].reason.as_deref(), Some("hostRestarted"));
    assert_eq!(history[0].operation["target"], json!(target));
    assert_eq!(history[0].requester, prompting_requester);
    assert_eq!(
        reloaded
            .list_detailed(false)
            .await
            .expect("detailed list")
            .approvals
            .len(),
        1
    );
}

#[tokio::test]
async fn human_and_old_provider_approval_rows_stay_detailed_only() {
    let (broker, generation, directory) = fixture_broker().await;
    let target = session(&broker.service_id, "provider-session");
    let requester = board_session_ref(&target).expect("provider requester");
    let approver =
        board_session_ref(&session(&broker.service_id, "approver-session")).expect("approver");
    broker
        .install_session_delivery(Arc::new(CapturingInteractionDelivery {
            notices: Arc::new(Mutex::new(Vec::new())),
            delivered: Arc::new(tokio::sync::Notify::new()),
        }))
        .expect("notice route");
    for (request_id, requested_by) in [
        (
            "human-requester",
            collaboration_protocol::ProviderIdentity::Human {
                human_id: "owner".to_owned().try_into().expect("human ID"),
            },
        ),
        ("old-row", target.clone().into()),
    ] {
        broker
            .request_typed_approval(
                requester.clone(),
                message_board::Identity::Session {
                    session: approver.clone(),
                },
                typed_approval_request(request_id),
                tokio_util::sync::CancellationToken::new(),
                tokio_util::sync::CancellationToken::new(),
                Some(crate::interaction_broker::TypedApprovalLegacyContext {
                    operation_id: collaboration_protocol::OperationId::generate(),
                    target: target.clone(),
                    generation: generation.clone(),
                    requested_by,
                }),
            )
            .await
            .expect("pending approval");
    }
    assert_eq!(broker.list(true).await.approvals.len(), 1);
    assert_eq!(
        broker
            .list_detailed(true)
            .await
            .expect("detailed list")
            .approvals
            .len(),
        2
    );
    drop(broker);
    let mut observer = <sqlx::SqliteConnection as sqlx::Connection>::connect_with(
        &sqlx::sqlite::SqliteConnectOptions::new().filename(directory.join("interaction.sqlite")),
    )
    .await
    .expect("owned SQLite history");
    let record_json: String = sqlx::query_scalar(
        "SELECT record_json FROM typed_interaction_history WHERE request_id='old-row'",
    )
    .fetch_one(&mut observer)
    .await
    .expect("stored old row");
    let mut stored: serde_json::Value = serde_json::from_str(&record_json).expect("stored record");
    stored["legacy_metadata"]
        .as_object_mut()
        .expect("legacy metadata")
        .remove("requestedBy");
    sqlx::query("UPDATE typed_interaction_history SET record_json=? WHERE request_id='old-row'")
        .bind(serde_json::to_string(&stored).expect("old record bytes"))
        .execute(&mut observer)
        .await
        .expect("older record shape");
    sqlx::Connection::close(observer)
        .await
        .expect("close observer");
    let interaction_store = Arc::new(
        interaction_storage::InteractionStore::open(directory.join("interaction.sqlite"))
            .await
            .expect("interaction store after restart"),
    );
    let reloaded = ServiceInteractionBroker::load(
        target.endpoint.service_id.clone(),
        NativeControlBackend {
            endpoint: target.endpoint,
            gate: crate::NativeGenerationGate::default(),
            codex_home: directory.clone(),
        },
        interaction_store,
    )
    .await
    .expect("reload older history");
    assert!(reloaded.list(false).await.approvals.is_empty());
    assert_eq!(
        reloaded
            .list_detailed(false)
            .await
            .expect("detailed list")
            .approvals
            .len(),
        2
    );
}

#[tokio::test]
async fn unreachable_approver_cancellation_keeps_its_typed_state_in_both_lists() {
    let (broker, generation, _) = fixture_broker().await;
    let requester =
        board_session_ref(&session(&broker.service_id, "provider-session")).expect("requester");
    let approver =
        board_session_ref(&session(&broker.service_id, "approver-session")).expect("approver");
    broker
        .install_session_delivery(Arc::new(CapturingInteractionDelivery {
            notices: Arc::new(Mutex::new(Vec::new())),
            delivered: Arc::new(tokio::sync::Notify::new()),
        }))
        .expect("notice route");
    let _receiver = broker
        .request_typed_approval(
            requester,
            message_board::Identity::Session { session: approver },
            typed_approval_request("unreachable-approver"),
            tokio_util::sync::CancellationToken::new(),
            tokio_util::sync::CancellationToken::new(),
            Some(crate::interaction_broker::TypedApprovalLegacyContext {
                operation_id: collaboration_protocol::OperationId::generate(),
                target: session(&broker.service_id, "provider-session"),
                generation,
                requested_by: session(&broker.service_id, "provider-session").into(),
            }),
        )
        .await
        .expect("pending approval");
    broker
        .cancel_typed_approval("unreachable-approver", "approverUnreachable")
        .await
        .expect("cancel unreachable");
    let legacy = broker.list(false).await.approvals;
    assert_eq!(legacy.len(), 1);
    assert_eq!(legacy[0].state, ApprovalState::ApproverUnreachable);
    assert_eq!(legacy[0].reason.as_deref(), Some("approverUnreachable"));
    let detailed = broker
        .list_detailed(false)
        .await
        .expect("detailed list")
        .approvals;
    assert_eq!(detailed.len(), 1);
    assert_eq!(detailed[0].state, ApprovalState::ApproverUnreachable);
    assert_eq!(detailed[0].reason.as_deref(), Some("approverUnreachable"));
}

impl collaboration_contracts::SessionMessageDelivery for FakeApprovalDelivery {
    fn deliver<'a>(
        &'a self,
        _: collaboration_contracts::layer_zero::DeliveryRequest,
        _: &'a dyn collaboration_contracts::AttemptEvidenceSink,
    ) -> collaboration_contracts::DeliveryFuture<'a, collaboration_contracts::NativeDeliveryResult>
    {
        Box::pin(async move {
            Ok(collaboration_contracts::scripted_delivery_result(
                collaboration_protocol::DeliveryReceipt {
                    outcome: self.0.clone(),
                    reachability: Some(collaboration_protocol::SessionReachability::CodexAppServer),
                    client: None,
                },
            ))
        })
    }

    fn reconcile_attempt(
        &self,
        _: collaboration_contracts::AttemptReconciliationContext,
    ) -> collaboration_contracts::DeliveryFuture<'_, collaboration_contracts::AttemptReconciliation>
    {
        Box::pin(async { Ok(collaboration_contracts::AttemptReconciliation::StillUnknown) })
    }
}

#[tokio::test]
async fn approval_notice_uses_selected_delivery_outcome() {
    for (outcome, delivered) in [
        (DeliveryOutcome::Started, true),
        (DeliveryOutcome::Unknown, true),
        (
            DeliveryOutcome::NotSubmitted {
                retryable: true,
                reason: "starting".into(),
            },
            false,
        ),
        (
            DeliveryOutcome::Rejected(collaboration_protocol::DeliveryRejection {
                reason: collaboration_protocol::DeliveryRejectionReason::Busy,
                next_action: collaboration_protocol::DeliveryNextAction::InspectTarget,
                client_code: None,
                detail: None,
                claims: None,
                native_failure: None,
            }),
            false,
        ),
    ] {
        let (broker, generation, _) = fixture_broker().await;
        let expiry = (chrono::Utc::now() + chrono::Duration::minutes(5)).to_rfc3339();
        let (params, _receiver) = insert_pending(&broker, generation, expiry).await;
        let record = broker
            .pending
            .lock()
            .await
            .get(&params.request_id)
            .expect("pending fixture")
            .record
            .clone();
        broker
            .install_session_delivery(Arc::new(FakeApprovalDelivery(outcome)))
            .expect("delivery injection");
        assert_eq!(broker.deliver(&record).await.is_ok(), delivered);
    }
}

#[tokio::test]
async fn legacy_approval_record_is_delivered_as_an_interaction_notice() {
    let (broker, generation, _directory) = fixture_broker().await;
    let requester = session(&broker.service_id, "legacy-requester");
    let approver = session(&broker.service_id, "legacy-approver");
    let notices = Arc::new(Mutex::new(Vec::new()));
    let delivered = Arc::new(tokio::sync::Notify::new());
    broker
        .install_session_delivery(Arc::new(CapturingInteractionDelivery {
            notices: Arc::clone(&notices),
            delivered,
        }))
        .expect("delivery route");
    let request = ApprovalRequestRecord {
        request_id: "legacy-approval-request".to_owned(),
        requester: requester.clone(),
        approver: approver.clone(),
        generation,
        state: ApprovalState::PendingClientDecision,
        reason: Some("The file is outside the workspace.".to_owned()),
        offered_options: vec![collaboration_protocol::ApprovalOfferedOption {
            option_id: "allow-once".to_owned(),
            label: Some("Allow once".to_owned()),
            scope: collaboration_protocol::ApprovalOptionScope::AllowOnce,
        }],
        presentation: Some(collaboration_protocol::ApprovalPresentation {
            tool_name: Some("Filesystem".to_owned()),
            title: Some("Read a private file".to_owned()),
            kind: Some("readFile".to_owned()),
            arguments: vec![collaboration_protocol::ApprovalArgument {
                name: "path".to_owned(),
                value: "/tmp/secret.txt".to_owned(),
            }],
            permission_details: vec!["read /tmp/secret.txt".to_owned()],
        }),
        decision: None,
        operation: json!({"params":{"path":"/tmp/secret.txt"}}),
        expires_at: (chrono::Utc::now() + chrono::Duration::minutes(5)).to_rfc3339(),
    };

    broker
        .deliver(&request)
        .await
        .expect("legacy approval notice delivered");

    let notices = notices.lock().await;
    assert_eq!(notices.len(), 1);
    let prepared = &notices[0];
    assert_eq!(prepared.target, request.approver);
    assert_eq!(
        prepared.correlation.as_str(),
        prepared.payload.push_id.as_str()
    );
    assert_eq!(prepared.mode, collaboration_protocol::MessageDelivery::Auto);
    let line = prepared.payload.line.as_str();
    assert!(
        line.starts_with("❓ Router approval @Approval Fixture"),
        "{line}"
    );
    assert!(
        line.contains("Approval request: Read a private file"),
        "{line}"
    );
    assert!(!line.contains("Agent communication"));
    assert!(!line.contains("Self-declared sender:"));
}

pub(super) fn session(service_id: &UuidIdentity, id: &str) -> SessionRef {
    SessionRef {
        endpoint: EndpointRef {
            service_id: service_id.clone(),
            endpoint_id: EndpointId::try_from("codex-local".to_owned())
                .unwrap_or_else(|error| panic!("endpoint: {error}")),
        },
        session_id: id
            .to_owned()
            .try_into()
            .unwrap_or_else(|error| panic!("session: {error}")),
    }
}

fn typed_approval_request(request_id: &str) -> session_event_model::ApprovalRequest {
    serde_json::from_value(json!({
        "requestId":request_id,"title":"Run command","options":[
            {"optionId":"allow-once","label":"Allow once","choice":{"effect":"allow","scope":"once"}}
        ]
    }))
    .expect("typed approval request")
}

#[test]
fn legacy_presentation_maps_only_fields_retained_by_typed_subjects() {
    let tool: session_event_model::ApprovalRequest = serde_json::from_value(json!({
        "requestId":"tool","title":"Run command",
        "subject":{"type":"tool_call","toolCall":{"toolCallId":"tool-1","kind":"execute","title":"Shell"}},
        "options":[{"optionId":"allow","label":"Allow","choice":{"effect":"allow","scope":"once"}}]
    })).expect("tool approval");
    let tool_presentation = super::legacy_presentation_from_typed(&tool);
    assert_eq!(tool_presentation.title.as_deref(), Some("Run command"));
    assert_eq!(tool_presentation.kind.as_deref(), Some("execute"));
    assert!(tool_presentation.tool_name.is_none());
    assert!(tool_presentation.permission_details.is_empty());
    let command: session_event_model::ApprovalRequest = serde_json::from_value(json!({
        "requestId":"command","title":"Run command",
        "subject":{"type":"command","command":"echo hello","cwd":"/tmp/project"},
        "options":[{"optionId":"allow","label":"Allow","choice":{"effect":"allow","scope":"once"}}]
    }))
    .expect("command approval");
    let command_presentation = super::legacy_presentation_from_typed(&command);
    assert_eq!(
        command_presentation.arguments,
        vec![
            collaboration_protocol::ApprovalArgument {
                name: "command".into(),
                value: "echo hello".into()
            },
            collaboration_protocol::ApprovalArgument {
                name: "cwd".into(),
                value: "/tmp/project".into()
            },
        ]
    );
}

#[path = "interaction_broker_tests/history_fixtures.rs"]
mod history_fixtures;
use history_fixtures::stored_interaction_records;

fn select_typed(option_id: &str) -> crate::interaction_broker::TypedInteractionDecision {
    crate::interaction_broker::TypedInteractionDecision::SelectApproval {
        option_id: option_id.to_owned(),
        acknowledge_persistent: false,
        note: None,
    }
}

pub(super) async fn fixture_broker() -> (Arc<ServiceInteractionBroker>, CodexGeneration, PathBuf) {
    fixture_broker_with_requester_name(None).await
}

async fn fixture_broker_with_requester_name(
    requester_name: Option<&str>,
) -> (Arc<ServiceInteractionBroker>, CodexGeneration, PathBuf) {
    let service_id = crate::new_service_uuid().unwrap_or_else(|error| panic!("service: {error}"));
    let generation: CodexGeneration = serde_json::from_value(json!({
        "serviceEpoch": String::from(service_id.clone()), "generation": 1
    }))
    .unwrap_or_else(|error| panic!("generation: {error}"));
    let gate = crate::NativeGenerationGate::default();
    gate.activate(
        generation.clone(),
        PathBuf::from("/tmp/approval-backend.sock"),
        None,
    )
    .unwrap_or_else(|error| panic!("activate: {error}"));
    let endpoint = EndpointRef {
        service_id: service_id.clone(),
        endpoint_id: "codex-local"
            .to_owned()
            .try_into()
            .unwrap_or_else(|error| panic!("endpoint: {error}")),
    };
    let directory = std::env::temp_dir().join(format!(
        "approval-broker-{}",
        String::from(crate::new_service_uuid().unwrap_or_else(|error| panic!("path: {error}")))
    ));
    tokio::fs::create_dir_all(&directory)
        .await
        .unwrap_or_else(|error| panic!("directory: {error}"));
    let interaction_store = Arc::new(
        interaction_storage::InteractionStore::open(directory.join("interaction.sqlite"))
            .await
            .unwrap_or_else(|error| panic!("interaction store: {error}")),
    );
    let broker = ServiceInteractionBroker::load(
        service_id.clone(),
        NativeControlBackend {
            endpoint,
            gate,
            codex_home: directory.clone(),
        },
        interaction_store,
    )
    .await
    .unwrap_or_else(|error| panic!("broker: {error}"));
    let display_names = crate::SessionDisplayNameCache::default();
    if let Some(name) = requester_name {
        display_names.remember(session(&broker.service_id, "provider-session"), name);
    }
    broker.install_display_names(display_names);
    broker.install_push_context(
        collaboration_contracts::MachineIdentity::new(service_id, Some("Approval Fixture"))
            .unwrap_or_else(|error| panic!("machine identity: {error}")),
    );
    (broker, generation, directory)
}

async fn insert_pending(
    broker: &ServiceInteractionBroker,
    generation: CodexGeneration,
    expires_at: String,
) -> (
    ApprovalDecideParams,
    oneshot::Receiver<BrokeredApprovalOutcome>,
) {
    let requester = session(&broker.service_id, "requester");
    let approver = session(&broker.service_id, "approver");
    let request_id = "approval-test".to_owned();
    let record = ApprovalRequestRecord {
        request_id: request_id.clone(),
        requester,
        approver: approver.clone(),
        generation,
        state: ApprovalState::PendingClientDecision,
        reason: None,
        offered_options: vec![ApprovalOfferedOption {
            option_id: "native-accept".to_owned(),
            label: None,
            scope: ApprovalOptionScope::AllowOnce,
        }],
        presentation: None,
        decision: None,
        operation: json!({"params":{"options":[]}}),
        expires_at,
    };
    broker
        .record(record.clone())
        .await
        .expect("persist pending approval");
    let (completion, receiver) = oneshot::channel();
    broker.pending.lock().await.insert(
        request_id.clone(),
        PendingApproval {
            record: record.clone(),
            offered: BTreeMap::from([(ApprovalDecision::Allow, "native-accept".to_owned())]),
            completion,
        },
    );
    (
        ApprovalDecideParams {
            request_id,
            decision: Some(ApprovalDecision::Allow),
            option_id: None,
            acknowledge_persistent: false,
            note: None,
            actor: board_identity(&approver).expect("approver identity"),
        },
        receiver,
    )
}

#[path = "interaction_broker_tests/decision_history_tests.rs"]
mod decision_history_tests;
#[path = "interaction_broker_tests/question_lifecycle_tests.rs"]
mod question_lifecycle_tests;
