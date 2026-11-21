//! Shared Host, provider and store fixture for restore recovery tests.

use super::super::*;
use collaboration_contracts::{
    DeliveryPrecondition, LoadPolicy,
    layer_zero::{DeliveryRequest, PreparedPush},
};
use collaboration_protocol::{
    AttemptId, ChannelDescription, ConversationLoadRequest, ConversationPromptRequest,
    ConversationResumeRequest, DeliveryCorrelationId, EndpointAvailability, EndpointDescription,
    MessageContent, MessageDelivery, ObservationTimestamp,
};
use collaboration_runtime::{EndpointDirectory, ProviderSessionEventHub, ProviderSessionRecord};
use router_clock::ManualClock;

pub(super) fn admission_pause() -> (
    AdmissionTestPause,
    tokio::sync::oneshot::Receiver<()>,
    tokio::sync::oneshot::Sender<()>,
) {
    let (entered, reached) = tokio::sync::oneshot::channel();
    let (resume, release) = tokio::sync::oneshot::channel();
    (AdmissionTestPause { entered, release }, reached, resume)
}

pub(super) async fn wait_for_queued_input(
    host: &ExternalProviderSupervisor,
    target: &SessionRef,
    operation: &OperationId,
) {
    let operation = uuid::Uuid::parse_str(operation.as_str()).expect("operation identity");
    host.session_owner_for(target)
        .expect("registered owner")
        .snapshots()
        .wait_for(|snapshot| {
            snapshot
                .queue
                .iter()
                .any(|entry| entry.input.operation == operation)
        })
        .await
        .expect("prompt is waiting on that owner");
}

pub(super) struct RestoreRig {
    pub(super) root: tempfile::TempDir,
    pub(super) target: SessionRef,
    pub(super) clock: Arc<ManualClock>,
}

impl RestoreRig {
    pub(super) fn new() -> Self {
        Self {
            root: tempfile::tempdir().expect("restore root"),
            target: SessionRef {
                endpoint: endpoint(),
                session_id: SessionId::try_from("saved-session".to_owned()).expect("session"),
            },
            clock: Arc::new(ManualClock::new(chrono::DateTime::UNIX_EPOCH)),
        }
    }

    pub(super) fn working_directory(&self) -> ProviderWorkingDirectory {
        ProviderWorkingDirectory::try_from(self.root.path().display().to_string())
            .expect("working directory")
    }

    pub(super) async fn store(&self) -> ProviderOperationStore {
        ProviderOperationStore::open(&self.root.path().join("operations.sqlite"))
            .await
            .expect("store")
    }

    pub(super) async fn seed_session(&self) {
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

    pub(super) async fn open_host(&self, behavior: &str) -> ExternalProviderSupervisor {
        self.open_host_with(&[behavior]).await
    }

    pub(super) async fn open_host_with(&self, behaviors: &[&str]) -> ExternalProviderSupervisor {
        self.open_host_with_adapter(
            behaviors,
            Arc::new(acp_provider_adapters::StandardAcpBehavior),
        )
        .await
    }

    pub(super) async fn open_host_with_adapter(
        &self,
        behaviors: &[&str],
        adapter: Arc<dyn acp_client_runtime::ProviderBehavior>,
    ) -> ExternalProviderSupervisor {
        let arguments = std::iter::once("plain-prompts")
            .chain(behaviors.iter().copied())
            .collect::<Vec<_>>();
        self.open_host_with_fixture_arguments(&arguments, adapter)
            .await
            .0
    }

    pub(super) async fn open_host_with_fixture_arguments(
        &self,
        behaviors: &[&str],
        adapter: Arc<dyn acp_client_runtime::ProviderBehavior>,
    ) -> (ExternalProviderSupervisor, Arc<ProviderSessionEventHub>) {
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
                ]
                .into_iter()
                .chain(behaviors.iter().map(|behavior| (*behavior).to_owned()))
                .collect(),
            ),
            acp_client_runtime::ProviderModelPicker::Standard,
            "fixture",
            "http://127.0.0.1:1/mcp",
            hub.clone(),
            target.endpoint,
            adapter,
            self.clock.clone(),
        )
        .await
        .expect("fixture runtime");
        // Keep the fixture's binding generation across reseeding so it
        // exercises the same-generation persisted uncertainty rule.
        let host = ExternalProviderSupervisor::new_with_hub(
            vec![ExternalProviderBinding {
                identity: binding(),
                runtime,
            }],
            store,
            Some(hub.clone()),
            None,
        )
        .expect("supervisor");
        (host, hub)
    }

    pub(super) fn delivery_route(
        &self,
        host: Arc<ExternalProviderSupervisor>,
    ) -> crate::ProviderAcpDeliveryRoute {
        let directory = EndpointDirectory::new(self.target.endpoint.service_id.clone());
        let binding = binding();
        directory
            .publish(EndpointDescription {
                endpoint: self.target.endpoint.clone(),
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
        crate::ProviderAcpDeliveryRoute::new(
            self.target.endpoint.service_id.clone(),
            std::iter::once(self.target.endpoint.clone()).collect(),
            directory,
            host.clone(),
            host.inner.store.clone(),
            Arc::new(NoLivePeer),
            self.clock.clone(),
        )
    }

    pub(super) fn load_request(&self, operation_id: OperationId) -> ConversationLoadRequest {
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

    pub(super) fn resume_request(&self, operation_id: OperationId) -> ConversationResumeRequest {
        ConversationResumeRequest {
            operation_id,
            target: self.target.clone(),
            generation: Some(generation()),
            working_directory: self.working_directory(),
            requested_by: self.target.clone().into(),
            approver: self.target.clone().into(),
            requested_policy: policy(),
        }
    }

    pub(super) fn wire_receipt(&self) -> serde_json::Value {
        serde_json::from_slice(
            &std::fs::read(self.root.path().join("wire.json")).expect("wire receipt"),
        )
        .expect("wire JSON")
    }

    pub(super) async fn until_wire_request(&self, method: &str) {
        while !std::fs::read(self.root.path().join("wire.json"))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|wire| wire[method].as_array().map(|rows| !rows.is_empty()))
            .unwrap_or(false)
        {
            tokio::task::yield_now().await;
        }
    }

    pub(super) fn prompt_request(&self, id: OperationId, text: &str) -> ConversationPromptRequest {
        ConversationPromptRequest {
            delivery: MessageDelivery::Auto,
            input_id: None,
            operation_id: id,
            target: self.target.clone(),
            generation: Some(generation()),
            requested_by: self.target.clone().into(),
            approver: self.target.clone().into(),
            prompt: MessageContent::HumanUser {
                text: MessageText::try_from(text.to_owned()).expect("prompt text"),
            },
        }
    }

    pub(super) async fn assert_loads(&self, host: &ExternalProviderSupervisor) {
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

pub(super) fn policy() -> ProviderRequestedPolicy {
    ProviderRequestedPolicy {
        access: RouterAccess::WriteRestricted,
    }
}

pub(super) fn wait_request(operation_id: OperationId) -> ConversationOperationWaitRequest {
    ConversationOperationWaitRequest {
        operation_id,
        timeout_seconds: PositiveSeconds::try_from(5).expect("wait budget"),
    }
}

pub(super) fn auto_message(
    target: SessionRef,
    text: &str,
    load_policy: LoadPolicy,
) -> DeliveryRequest {
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
