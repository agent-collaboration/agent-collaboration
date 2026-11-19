//! Local restore retirement and admission, through the real owning store.

use super::*;
use collaboration_contracts::{
    DeliveryPrecondition, LoadPolicy, SessionDeliveryRoute, UnstoredAttemptEvidenceSink,
    layer_zero::{DeliveryRequest, PreparedPush},
};
use collaboration_protocol::{
    AttemptId, ChannelDescription, ConversationLoadRequest, ConversationOperationShowRequest,
    ConversationPromptRequest, DeliveryCorrelationId, DeliveryOutcome, EndpointAvailability,
    EndpointDescription, MessageContent, MessageDelivery, ObservationTimestamp,
};
use collaboration_runtime::{EndpointDirectory, ProviderSessionEventHub, ProviderSessionRecord};
use router_clock::{ClockInstant, ManualClock};

struct RestoreRig {
    root: tempfile::TempDir,
    target: SessionRef,
    clock: Arc<ManualClock>,
}

impl RestoreRig {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().expect("restore root"),
            target: SessionRef {
                endpoint: endpoint(),
                session_id: SessionId::try_from("saved-session".to_owned()).expect("session"),
            },
            clock: Arc::new(ManualClock::new(chrono::DateTime::UNIX_EPOCH)),
        }
    }

    fn working_directory(&self) -> ProviderWorkingDirectory {
        ProviderWorkingDirectory::try_from(self.root.path().display().to_string())
            .expect("working directory")
    }

    async fn store(&self) -> ProviderOperationStore {
        ProviderOperationStore::open(&self.root.path().join("operations.sqlite"))
            .await
            .expect("store")
    }

    async fn seed_session(&self) {
        self.store()
            .await
            .record_session(&ProviderSessionRecord {
                target: self.target.clone(),
                working_directory: self.working_directory(),
                requested_policy: policy(),
                created_by: self.target.clone().into(),
                approver: self.target.clone().into(),
                updated_at_ms: 1,
                resume: Default::default(),
            })
            .await
            .expect("saved session");
    }

    async fn open_host(&self, behavior: &str) -> ExternalProviderSupervisor {
        let store = Arc::new(Mutex::new(self.store().await));
        let hub = Arc::new(ProviderSessionEventHub::new(store.clone()));
        let target: message_board::SessionRef =
            serde_json::from_value(serde_json::to_value(&self.target).expect("target JSON"))
                .expect("hub target");
        let runtime = ExternalProviderRuntime::initialize_with_mcp_http_and_hub(
            crate::external_provider_runtime::acp_scripted_fixture::launch_fixture_agent(
                "permission-mode-agent.ts",
                vec![
                    self.root.path().join("wire.json").display().to_string(),
                    "cursor".into(),
                    "no-directories".into(),
                    "advertise-restore".into(),
                    "plain-prompts".into(),
                    behavior.into(),
                ],
            ),
            acp_client_runtime::ProviderModelPicker::Standard,
            "fixture",
            "http://127.0.0.1:1/mcp",
            hub,
            target.endpoint,
            Arc::new(acp_provider_adapters::StandardAcpBehavior),
            self.clock.clone(),
        )
        .await
        .expect("fixture runtime");
        // Keep the fixture's binding generation across reseeding so it
        // exercises the same-generation persisted uncertainty rule.
        ExternalProviderSupervisor::new(
            vec![ExternalProviderBinding {
                identity: binding(),
                runtime,
            }],
            store,
        )
        .expect("supervisor")
    }

    fn load_request(&self, operation_id: OperationId) -> ConversationLoadRequest {
        ConversationLoadRequest {
            operation_id,
            target: self.target.clone(),
            generation: Some(generation()),
            working_directory: self.working_directory(),
            requested_by: self.target.clone().into(),
            approver: self.target.clone().into(),
            requested_policy: policy(),
        }
    }

    async fn assert_loads(&self, host: &ExternalProviderSupervisor) {
        let id = OperationId::generate();
        host.load(self.load_request(id.clone()))
            .await
            .expect("restore admitted");
        let settled = host.wait(wait_request(id)).await.expect("restore settled");
        assert!(
            matches!(settled.output, ConversationOperationWaitOutput::Available {
            settlement: ConversationOperationSettlement::Loaded { ref target, .. }
        } if target == &self.target),
            "{settled:?}"
        );
    }
}

fn policy() -> ProviderRequestedPolicy {
    ProviderRequestedPolicy {
        access: RouterAccess::WriteRestricted,
    }
}

fn wait_request(operation_id: OperationId) -> ConversationOperationWaitRequest {
    ConversationOperationWaitRequest {
        operation_id,
        timeout_seconds: PositiveSeconds::try_from(5).expect("wait budget"),
    }
}

fn auto_message(target: SessionRef, text: &str, load_policy: LoadPolicy) -> DeliveryRequest {
    let push_id = PushId::try_from(AttemptId::generate().as_str().to_owned()).expect("push ID");
    let correlation =
        DeliveryCorrelationId::try_from(push_id.as_str().to_owned()).expect("correlation");
    DeliveryRequest {
        payload: PreparedPush {
            line: MessageText::try_from(format!(
                "✉️ sender · \"{text}\" · router://{}/push/{}",
                String::from(target.endpoint.service_id.clone()),
                push_id.as_str()
            ))
            .expect("prepared input"),
            push_id,
            load_policy,
        },
        target,
        mode: MessageDelivery::Auto,
        precondition: DeliveryPrecondition::Unpinned,
        correlation,
        attempt: AttemptId::generate(),
    }
}

#[tokio::test]
async fn a_cold_auto_restore_timeout_finishes_the_input_without_holding_it() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = Arc::new(rig.open_host("silent-first-restore").await);
    let directory = EndpointDirectory::new(rig.target.endpoint.service_id.clone());
    let binding = binding();
    directory
        .publish(EndpointDescription {
            endpoint: rig.target.endpoint.clone(),
            label: NonEmptyText::try_from("Cursor fixture".to_owned()).expect("label"),
            availability: EndpointAvailability::Available {
                observed_at: ObservationTimestamp::try_from("2026-11-19T12:00:00Z".to_owned())
                    .expect("timestamp"),
            },
            channels: vec![ChannelDescription::ExternalProvider {
                transport: binding.transport,
                binding_id: binding.binding_id,
                binding_generation: binding.generation.generation,
                runtime: binding.runtime,
                capabilities: binding.capabilities,
                auto_realization: None,
            }],
        })
        .expect("endpoint");
    let route = crate::ProviderAcpDeliveryRoute::new(
        rig.target.endpoint.service_id.clone(),
        std::iter::once(rig.target.endpoint.clone()).collect(),
        directory,
        host.clone(),
        host.inner.store.clone(),
        Arc::new(NoLivePeer),
        rig.clock.clone(),
    );
    let not_loaded = route
        .deliver(
            auto_message(rig.target.clone(), "load forbidden", LoadPolicy::LoadedOnly),
            &UnstoredAttemptEvidenceSink,
        )
        .await
        .expect("loaded-only receipt");
    assert!(matches!(
        not_loaded.receipt().outcome,
        DeliveryOutcome::NotSubmitted {
            retryable: true,
            ..
        }
    ));
    assert!(
        !rig.root.path().join("wire.json").exists(),
        "the loaded-only attempt reached ACP"
    );

    let cold_delivery = route.deliver(
        auto_message(rig.target.clone(), "retire this input", LoadPolicy::MayLoad),
        &UnstoredAttemptEvidenceSink,
    );
    tokio::pin!(cold_delivery);
    let timeout = std::time::Duration::from_secs(30);
    tokio::select! {
        () = rig.clock.not_due_check_now(ClockInstant::ZERO.after(timeout)) => {},
        receipt = &mut cold_delivery => panic!("cold load ended before its deadline: {receipt:?}"),
    }
    rig.clock.advance(timeout).expect("reach restore deadline");
    let actual = cold_delivery.await.expect("terminal cold delivery receipt");
    assert!(
        matches!(actual.receipt().outcome, DeliveryOutcome::NotSubmitted { ref reason, retryable: false } if reason.contains("did not answer the restore in time, so retry this session")),
        "{actual:?}"
    );

    rig.assert_loads(&host).await;
    let next = auto_message(
        rig.target.clone(),
        "deliver this newer input",
        LoadPolicy::MayLoad,
    );
    let next_id =
        OperationId::try_from(next.attempt.as_str().to_owned()).expect("next operation ID");
    let sent = route
        .deliver(next, &UnstoredAttemptEvidenceSink)
        .await
        .expect("later input delivered");
    assert!(
        matches!(sent.receipt().outcome, DeliveryOutcome::Started),
        "{sent:?}"
    );
    let completed = host
        .wait(wait_request(next_id))
        .await
        .expect("later input settles");
    assert!(matches!(
        completed.output,
        ConversationOperationWaitOutput::Available {
            settlement: ConversationOperationSettlement::PromptCompleted { .. }
        }
    ));
    let observed: serde_json::Value = serde_json::from_slice(
        &std::fs::read(rig.root.path().join("wire.json")).expect("wire receipt"),
    )
    .expect("wire JSON");
    assert_eq!(
        observed["restores"]
            .as_array()
            .expect("restore requests")
            .len(),
        2
    );
    assert_eq!(
        observed["prompts"],
        serde_json::json!([rig.target.session_id])
    );
    route.shutdown_queue().await;
    host.shutdown().await.expect("stop Host");
}

#[tokio::test]
async fn a_retired_load_allows_a_same_session_retry_before_and_after_store_reopen() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("silent-first-restore").await;
    let id = OperationId::generate();
    host.load(rig.load_request(id.clone()))
        .await
        .expect("first load admitted");
    let timeout = std::time::Duration::from_secs(30);
    rig.clock
        .not_due_check_now(ClockInstant::ZERO.after(timeout))
        .await;
    rig.clock
        .advance(timeout)
        .expect("reach the restore deadline");
    let failure = host
        .wait(wait_request(id.clone()))
        .await
        .expect_err("first load has no answer");
    assert_eq!(failure.effect, ProviderOperationEffect::Unknown);
    assert!(String::from(failure.message).contains("retry this session"));
    let snapshot = host
        .show(ConversationOperationShowRequest {
            operation_id: id.clone(),
        })
        .await
        .expect("first load record");
    assert_eq!(snapshot.stage, ProviderOperationStage::Terminal);
    assert_eq!(
        snapshot.reconciliation,
        ProviderReconciliationState::Unresolved
    );

    rig.assert_loads(&host).await;
    host.shutdown().await.expect("stop the first Host");
    drop(host);
    let reopened = rig.open_host("plain-prompts").await;
    rig.assert_loads(&reopened).await;
    let earlier = rig
        .store()
        .await
        .inspect(&id)
        .await
        .expect("read retired load")
        .expect("retired load retained");
    assert_eq!(earlier.effect, ProviderOperationEffect::Unknown);
    assert_eq!(
        earlier.reconciliation_state,
        ProviderReconciliationState::Unresolved
    );
    reopened.shutdown().await.expect("stop reopened Host");
}

#[tokio::test]
async fn a_disconnect_during_restore_keeps_unknown_effect_and_reconnect_guidance() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("disconnect-during-restore").await;
    let id = OperationId::generate();
    host.load(rig.load_request(id.clone()))
        .await
        .expect("load admitted");
    let failure = host
        .wait(wait_request(id.clone()))
        .await
        .expect_err("connection closed during restore");
    assert_eq!(failure.effect, ProviderOperationEffect::Unknown);
    assert!(String::from(failure.message).contains("reconnect and retry this session"));
    let row = rig
        .store()
        .await
        .inspect(&id)
        .await
        .expect("operation read")
        .expect("load recorded");
    assert_eq!(row.stage, ProviderOperationStage::Terminal);
    assert_eq!(
        row.reconciliation_state,
        ProviderReconciliationState::Unresolved
    );
    host.shutdown().await.expect("stop Host");
}

#[tokio::test]
async fn an_uncertain_prompt_still_blocks_restore_before_and_after_store_reopen() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let host = rig.open_host("refuse-prompt").await;
    rig.assert_loads(&host).await;
    let id = OperationId::generate();
    host.prompt(ConversationPromptRequest {
        delivery: MessageDelivery::Auto,
        input_id: None,
        operation_id: id.clone(),
        target: rig.target.clone(),
        generation: Some(generation()),
        requested_by: rig.target.clone().into(),
        approver: rig.target.clone().into(),
        prompt: MessageContent::HumanUser {
            text: MessageText::try_from("test input".to_owned()).expect("prompt text"),
        },
    })
    .await
    .expect("prompt admitted");
    let uncertainty = host
        .wait(wait_request(id.clone()))
        .await
        .expect_err("prompt effect is uncertain");
    assert_eq!(uncertainty.effect, ProviderOperationEffect::Unknown);
    assert_restore_blocked(&rig, &host, &id).await;
    host.shutdown().await.expect("stop first Host");
    drop(host);

    let reopened = rig.open_host("plain-prompts").await;
    assert_restore_blocked(&rig, &reopened, &id).await;
    reopened.shutdown().await.expect("stop reopened Host");
}

async fn assert_restore_blocked(
    rig: &RestoreRig,
    host: &ExternalProviderSupervisor,
    earlier: &OperationId,
) {
    let blocked = host
        .load(rig.load_request(OperationId::generate()))
        .await
        .expect_err("uncertain prompt must still guard admission");
    assert_eq!(blocked.kind, ConversationOperationFailureKind::Busy);
    assert!(String::from(blocked.message).contains(earlier.as_str()));
}
