//! Test-only construction of complete collaboration applications.
use collaboration_protocol::{EndpointDescription, UuidIdentity};
use collaboration_runtime::EndpointDirectory;

#[derive(Clone)]
pub struct ApplicationFixture {
    pub(crate) runtime_clock: std::sync::Arc<dyn router_clock::RuntimeClock>,
    pub(crate) service_id: UuidIdentity,
    pub(crate) machine_identity: collaboration_contracts::MachineIdentity,
    pub(crate) service_epoch: UuidIdentity,
    pub(crate) display_names: collaboration_runtime::SessionDisplayNameCache,
    pub(crate) directory: EndpointDirectory,
    pub(crate) journal: Option<std::sync::Arc<lifecycle_observation::LifecycleStore>>,
    pub(crate) native_backend: Option<collaboration_runtime::NativeControlBackend>,
    pub(crate) session_delivery:
        Option<std::sync::Arc<dyn collaboration_contracts::SessionMessageDelivery>>,
    pub(crate) approval_broker:
        Option<std::sync::Arc<collaboration_runtime::ServiceInteractionBroker>>,
    pub(crate) interaction_store: Option<std::sync::Arc<interaction_storage::InteractionStore>>,
    pub(crate) provider_operations:
        Option<std::sync::Arc<tokio::sync::Mutex<collaboration_runtime::ProviderOperationStore>>>,
    pub(crate) provider_session_hub:
        Option<std::sync::Arc<dyn collaboration_contracts::SessionEventHub>>,
    pub(crate) claude_code_sessions:
        Option<std::sync::Arc<claude_code_peer_messaging::ClaudeCodeSessionRegistry>>,
    pub(crate) claude_background_jobs:
        Option<std::sync::Arc<claude_code_peer_messaging::NativeClaudeJobReader>>,
    pub(crate) codex_conversation_recorder:
        Option<std::sync::Arc<collaboration_runtime::CodexConversationOperationRecorder>>,
    pub(crate) provider_conversations:
        Option<std::sync::Arc<dyn collaboration_contracts::ProviderConversationBackend>>,
}
impl ApplicationFixture {
    pub fn with_machine_identity(
        mut self,
        machine_identity: collaboration_contracts::MachineIdentity,
    ) -> Result<Self, String> {
        if machine_identity.service_id() != &self.service_id {
            return Err("machine identity belongs to another service".into());
        }
        self.machine_identity = machine_identity;
        Ok(self)
    }
    pub fn runtime_clock(&self) -> std::sync::Arc<dyn router_clock::RuntimeClock> {
        self.runtime_clock.clone()
    }
    pub fn with_runtime_clock(
        mut self,
        clock: std::sync::Arc<dyn router_clock::RuntimeClock>,
    ) -> Self {
        self.runtime_clock = clock;
        self
    }

    /// The retention worker for this fixture's interaction store and approval broker.
    #[must_use]
    pub fn interaction_retention_worker(
        &self,
    ) -> collaboration_runtime::InteractionRetentionWorker {
        let worker =
            collaboration_runtime::InteractionRetentionWorker::new(self.runtime_clock.clone());
        let worker = match self.interaction_store.as_ref() {
            Some(store) => worker.with_interaction_store(std::sync::Arc::clone(store)),
            None => worker,
        };
        match self.approval_broker.as_ref() {
            Some(broker) => worker.with_interaction_broker(std::sync::Arc::clone(broker)),
            None => worker,
        }
    }

    pub fn with_interaction_store(
        mut self,
        store: std::sync::Arc<interaction_storage::InteractionStore>,
    ) -> Self {
        self.interaction_store = Some(store);
        self
    }

    pub fn with_provider_operation_store(
        mut self,
        store: std::sync::Arc<tokio::sync::Mutex<collaboration_runtime::ProviderOperationStore>>,
    ) -> Self {
        self.provider_operations = Some(store);
        self
    }

    #[must_use]
    pub fn with_provider_session_hub(
        mut self,
        hub: std::sync::Arc<dyn collaboration_contracts::SessionEventHub>,
    ) -> Self {
        self.provider_session_hub = Some(hub);
        self
    }

    #[must_use]
    pub fn with_claude_code_sessions(
        mut self,
        registry: std::sync::Arc<claude_code_peer_messaging::ClaudeCodeSessionRegistry>,
    ) -> Self {
        self.claude_code_sessions = Some(registry);
        self
    }

    #[must_use]
    pub fn with_claude_background_jobs(
        mut self,
        reader: std::sync::Arc<claude_code_peer_messaging::NativeClaudeJobReader>,
    ) -> Self {
        self.claude_background_jobs = Some(reader);
        self
    }

    pub fn with_codex_conversation_recorder(
        mut self,
        recorder: std::sync::Arc<collaboration_runtime::CodexConversationOperationRecorder>,
    ) -> Self {
        self.codex_conversation_recorder = Some(recorder);
        self
    }

    pub fn with_provider_conversation_backend(
        mut self,
        backend: std::sync::Arc<dyn collaboration_contracts::ProviderConversationBackend>,
    ) -> Self {
        self.provider_conversations = Some(backend);
        self
    }

    pub fn with_native_backend(
        mut self,
        backend: collaboration_runtime::NativeControlBackend,
    ) -> Result<Self, String> {
        if backend.endpoint.service_id != self.service_id {
            return Err("native backend belongs to another service".into());
        }
        self.native_backend = Some(backend);
        Ok(self)
    }

    #[must_use]
    pub fn with_session_delivery(
        mut self,
        delivery: std::sync::Arc<dyn collaboration_contracts::SessionMessageDelivery>,
    ) -> Self {
        self.session_delivery = Some(delivery);
        self
    }

    #[must_use]
    pub fn session_message_delivery(
        &self,
    ) -> Option<std::sync::Arc<dyn collaboration_contracts::SessionMessageDelivery>> {
        self.session_delivery.clone()
    }

    #[must_use]
    pub fn session_display_name_cache(&self) -> collaboration_runtime::SessionDisplayNameCache {
        self.display_names.clone()
    }
    #[must_use]
    pub fn with_approval_broker(
        mut self,
        broker: std::sync::Arc<collaboration_runtime::ServiceInteractionBroker>,
    ) -> Self {
        broker.install_display_names(self.display_names.clone());
        self.interaction_store = Some(broker.interaction_store());
        broker.install_push_context(self.machine_identity.clone());
        self.approval_broker = Some(broker);
        self
    }
    /// Registers a bounded inventory without inferring endpoint liveness.
    pub fn with_endpoints(self, endpoints: Vec<EndpointDescription>) -> Result<Self, String> {
        self.directory
            .publish_inventory(endpoints)
            .map_err(|error| error.to_string())?;
        Ok(self)
    }

    #[must_use]
    pub fn endpoint_directory(&self) -> EndpointDirectory {
        self.directory.clone()
    }

    pub fn with_journal(
        mut self,
        journal: std::sync::Arc<lifecycle_observation::LifecycleStore>,
    ) -> Self {
        self.journal = Some(journal);
        self
    }

    pub fn new(service_id: &str, service_epoch: &str) -> Result<Self, String> {
        let service_id =
            UuidIdentity::try_from(service_id.to_owned()).map_err(|error| error.to_string())?;
        let machine_identity =
            collaboration_contracts::MachineIdentity::new(service_id.clone(), None)
                .map_err(|error| error.to_string())?;
        Ok(Self {
            service_id: service_id.clone(),
            machine_identity,
            service_epoch: UuidIdentity::try_from(service_epoch.to_owned())
                .map_err(|error| error.to_string())?,
            display_names: collaboration_runtime::SessionDisplayNameCache::default(),
            journal: None,
            native_backend: None,
            session_delivery: None,
            approval_broker: None,
            interaction_store: None,
            provider_operations: None,
            provider_session_hub: None,
            claude_code_sessions: None,
            claude_background_jobs: None,
            codex_conversation_recorder: None,
            provider_conversations: None,
            runtime_clock: std::sync::Arc::new(
                router_clock::ManualClock::starting_at(
                    runtime_primitives::SyncTime::from_unix_millis(1792432800000),
                )
                .map_err(|error| error.to_string())?,
            ),
            directory: EndpointDirectory::new(service_id),
        })
    }
}

impl ApplicationFixture {
    #[expect(
        clippy::expect_used,
        reason = "This test-only builder validates these values when they are configured."
    )]
    pub fn router_identity(&self) -> collaboration_contracts::RouterIdentity {
        collaboration_contracts::RouterIdentity::new(
            self.service_id.clone(),
            self.service_epoch.clone(),
            self.machine_identity.clone(),
        )
        .expect("validated fixture identity")
    }
    #[must_use]
    pub fn build(self) -> crate::CollaborationApplication {
        let native_control = crate::collaboration_application::NativeSessionControl::new(
            self.router_identity(),
            self.directory.clone(),
            self.native_backend.clone(),
            self.approval_broker.clone(),
            self.display_names.clone(),
        );
        let codex_inventory = crate::CodexSessionInventory::new(
            self.router_identity(),
            self.directory.clone(),
            self.native_backend.clone(),
            self.display_names.clone(),
            self.journal.clone(),
        );
        let provider_inventory = crate::ProviderSessionInventory::new(
            self.router_identity(),
            self.directory.clone(),
            self.provider_operations.clone(),
            self.provider_session_hub.clone(),
            self.display_names.clone(),
            self.claude_code_sessions.clone(),
            self.claude_background_jobs.clone(),
        );
        let journal_operations = crate::collaboration_application::JournalOperations::new(
            self.router_identity(),
            self.directory.clone(),
            self.journal.clone(),
        );
        let sessions = crate::collaboration_application::SessionOperations::new(
            self.router_identity(),
            self.directory.clone(),
            native_control,
            codex_inventory,
            provider_inventory,
            journal_operations,
        );
        let conversations = crate::collaboration_application::ConversationOperations::new(
            self.router_identity(),
            self.provider_conversations.clone(),
            self.provider_operations.clone(),
            self.codex_conversation_recorder.clone(),
            self.directory.clone(),
        );
        let observation = crate::collaboration_application::ObservationOperations::new(
            self.router_identity(),
            self.provider_session_hub.clone(),
            self.directory.clone(),
        );
        let interactions = crate::collaboration_application::InteractionOperations::new(
            self.approval_broker.clone(),
        );
        crate::CollaborationApplication::new(
            self.router_identity(),
            conversations,
            observation,
            sessions,
            interactions,
        )
    }
}
