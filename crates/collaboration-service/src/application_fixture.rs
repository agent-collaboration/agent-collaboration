//! Test-only construction of complete collaboration applications.
use collaboration_protocol::{EndpointDescription, UuidIdentity};
use collaboration_runtime::EndpointDirectory;

#[derive(Clone)]
pub struct ApplicationFixture {
    pub(crate) notification_publisher: Option<std::sync::Arc<crate::NotificationPublisher>>,
    pub(crate) mail_delivery: Option<std::sync::Arc<crate::MailDeliveryRuntime>>,
    pub(crate) project_claim: Option<std::sync::Arc<dyn collaboration_contracts::ProjectClaim>>,
    pub(crate) project_replication:
        std::sync::Arc<dyn collaboration_contracts::ProjectReplicationPosition>,
    pub(crate) project_admission: Option<std::sync::Arc<crate::ProjectAdmission>>,
    pub(crate) subscription_delivery: Option<crate::SubscriptionDeliveryService>,
    pub(crate) subscription_presence:
        Option<std::sync::Arc<dyn collaboration_contracts::TargetPresenceProbe>>,
    pub(crate) subscription_clock: std::sync::Arc<dyn crate::SubscriptionClock>,
    pub(crate) service_id: UuidIdentity,
    pub(crate) installation_peer: Option<fleet_sync_protocol::PeerId>,
    pub(crate) machine_identity: collaboration_contracts::MachineIdentity,
    pub(crate) configuration: collaboration_runtime::AutomationConfigurationHandle,
    pub(crate) configuration_backend:
        Option<std::sync::Arc<dyn collaboration_runtime::AutomationConfigurationBackend>>,
    pub(crate) service_epoch: UuidIdentity,
    pub(crate) display_names: collaboration_runtime::SessionDisplayNameCache,
    pub(crate) directory: EndpointDirectory,
    pub(crate) wake_wait_permits: std::sync::Arc<tokio::sync::Semaphore>,
    pub(crate) journal: Option<std::sync::Arc<lifecycle_observation::LifecycleStore>>,
    pub(crate) native_backend: Option<collaboration_runtime::NativeControlBackend>,
    pub(crate) session_delivery:
        Option<std::sync::Arc<dyn collaboration_contracts::SessionMessageDelivery>>,
    pub(crate) scheduled_run_execution:
        Option<std::sync::Arc<dyn collaboration_contracts::ScheduledRunExecution>>,
    pub(crate) approval_broker:
        Option<std::sync::Arc<collaboration_runtime::ServiceInteractionBroker>>,
    pub(crate) automation:
        Option<std::sync::Arc<tokio::sync::Mutex<automation_storage::AutomationStore>>>,
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
    pub fn with_mail_delivery_runtime(
        mut self,
        runtime: std::sync::Arc<crate::MailDeliveryRuntime>,
    ) -> Result<Self, String> {
        runtime.validate_owner(&self.router_identity(), self.interaction_store.as_ref())?;
        if let Some(store) = &self.automation {
            runtime.attach_wake_owner(
                self.service_id.clone(),
                store.clone(),
                self.wake_wait_permits.clone(),
            )?;
        }
        runtime.attach_reader_readiness()?;
        runtime.attach_project_replication(self.project_replication.clone())?;
        self.mail_delivery = Some(runtime);
        Ok(self)
    }

    pub fn with_installation_peer(
        mut self,
        peer: fleet_sync_protocol::PeerId,
    ) -> Result<Self, String> {
        if self
            .installation_peer
            .is_some_and(|existing| existing != peer)
        {
            return Err("installation peer is already configured with a different identity".into());
        }
        self.installation_peer = Some(peer);
        Ok(self)
    }

    #[must_use]
    pub fn installation_peer(&self) -> Option<fleet_sync_protocol::PeerId> {
        self.installation_peer
    }

    pub fn with_project_claim(
        mut self,
        claim: std::sync::Arc<dyn collaboration_contracts::ProjectClaim>,
    ) -> Self {
        self.project_claim = Some(claim);
        self
    }

    pub fn with_project_replication(
        mut self,
        replication: std::sync::Arc<dyn collaboration_contracts::ProjectReplicationPosition>,
    ) -> Self {
        self.project_replication = replication;
        self
    }
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

    pub fn with_notification_publisher(
        mut self,
        publisher: std::sync::Arc<crate::NotificationPublisher>,
    ) -> Self {
        self.notification_publisher = Some(publisher);
        self.attach_follow_admission();
        self
    }

    pub fn with_project_admission(
        mut self,
        admission: std::sync::Arc<crate::ProjectAdmission>,
    ) -> Self {
        self.project_admission = Some(admission);
        self.attach_follow_admission();
        self
    }
    pub fn with_subscription_delivery_service(
        mut self,
        service: crate::SubscriptionDeliveryService,
        presence: std::sync::Arc<dyn collaboration_contracts::TargetPresenceProbe>,
    ) -> Self {
        self.subscription_clock = service.subscription_clock();
        self.subscription_delivery = Some(service);
        self.subscription_presence = Some(presence);
        self.attach_follow_admission();
        self
    }
    fn attach_follow_admission(&self) {
        if let (Some(service), Some(admission), Some(publisher)) = (
            &self.subscription_delivery,
            &self.project_admission,
            &self.notification_publisher,
        ) {
            service.attach_follow_admission(
                std::sync::Arc::downgrade(admission),
                std::sync::Arc::downgrade(publisher),
            );
        }
    }

    pub fn automation_retention_worker(
        &self,
    ) -> Option<collaboration_runtime::AutomationRetentionWorker> {
        self.automation.as_ref().map(|store| {
            let worker =
                collaboration_runtime::AutomationRetentionWorker::new(std::sync::Arc::clone(store));
            let worker = match self.interaction_store.as_ref() {
                Some(store) => worker.with_native_push_store(std::sync::Arc::clone(store)),
                None => worker,
            };
            match self.approval_broker.as_ref() {
                Some(broker) => worker.with_interaction_broker(std::sync::Arc::clone(broker)),
                None => worker,
            }
        })
    }
    pub fn with_automation_configuration(
        mut self,
        handle: collaboration_runtime::AutomationConfigurationHandle,
        backend: std::sync::Arc<dyn collaboration_runtime::AutomationConfigurationBackend>,
    ) -> Self {
        self.configuration = handle;
        self.configuration_backend = Some(backend);
        self
    }

    pub fn schedule_timing_worker(&self) -> Option<collaboration_runtime::ScheduleTimingWorker> {
        self.automation
            .as_ref()
            .zip(self.scheduled_run_execution.as_ref())
            .zip(self.interaction_store.as_ref())
            .map(|((store, execution), interactions)| {
                collaboration_runtime::ScheduleTimingWorker::new(
                    std::sync::Arc::clone(store),
                    std::sync::Arc::clone(interactions),
                    std::sync::Arc::clone(execution),
                    self.native_backend.clone(),
                    self.configuration.clone(),
                    self.machine_identity.clone(),
                )
            })
    }

    pub fn wake_timing_worker(&self) -> Option<collaboration_runtime::WakeTimingWorker> {
        self.automation
            .as_ref()
            .zip(self.session_delivery.as_ref())
            .zip(self.interaction_store.as_ref())
            .map(|((store, delivery), interactions)| {
                collaboration_runtime::WakeTimingWorker::new(
                    std::sync::Arc::clone(store),
                    std::sync::Arc::clone(delivery),
                    self.configuration.clone(),
                    self.machine_identity.clone(),
                    std::sync::Arc::clone(interactions),
                )
            })
    }

    pub fn with_automation_store(
        mut self,
        store: std::sync::Arc<tokio::sync::Mutex<automation_storage::AutomationStore>>,
    ) -> Self {
        self.automation = Some(store);
        self
    }

    pub fn with_interaction_store(
        mut self,
        store: std::sync::Arc<interaction_storage::InteractionStore>,
    ) -> Self {
        self.interaction_store = Some(store);
        self
    }

    pub fn push_handoff_worker(&self) -> Option<collaboration_runtime::PushHandoffWorker> {
        self.automation
            .as_ref()
            .zip(self.interaction_store.as_ref())
            .map(|(automation, interactions)| {
                collaboration_runtime::PushHandoffWorker::new(
                    std::sync::Arc::clone(automation),
                    std::sync::Arc::clone(interactions),
                )
            })
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
    pub fn with_scheduled_run_execution(
        mut self,
        execution: std::sync::Arc<dyn collaboration_contracts::ScheduledRunExecution>,
    ) -> Self {
        self.scheduled_run_execution = Some(execution);
        self
    }
    #[must_use]
    pub fn with_approval_broker(
        mut self,
        broker: std::sync::Arc<collaboration_runtime::ServiceInteractionBroker>,
    ) -> Self {
        broker.install_display_names(self.display_names.clone());
        let interactions = broker.interaction_store();
        self.interaction_store = Some(interactions.clone());
        broker.install_push_context(interactions, self.machine_identity.clone());
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
            notification_publisher: None,
            project_replication: std::sync::Arc::new(
                collaboration_contracts::UnacknowledgedProjectReplication,
            ),
            project_claim: None,
            configuration: collaboration_runtime::AutomationConfigurationHandle::default(),
            configuration_backend: None,
            service_id: service_id.clone(),
            installation_peer: None,
            mail_delivery: None,
            machine_identity,
            service_epoch: UuidIdentity::try_from(service_epoch.to_owned())
                .map_err(|error| error.to_string())?,
            display_names: collaboration_runtime::SessionDisplayNameCache::default(),
            journal: None,
            native_backend: None,
            session_delivery: None,
            scheduled_run_execution: None,
            approval_broker: None,
            wake_wait_permits: std::sync::Arc::new(tokio::sync::Semaphore::new(16)),
            automation: None,
            interaction_store: None,
            provider_operations: None,
            provider_session_hub: None,
            claude_code_sessions: None,
            claude_background_jobs: None,
            codex_conversation_recorder: None,
            provider_conversations: None,
            project_admission: None,
            subscription_delivery: None,
            subscription_presence: None,
            subscription_clock: std::sync::Arc::new(crate::SystemSubscriptionClock),
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
            self.installation_peer,
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
        let session_context = crate::collaboration_application::SessionContextReader::new(
            self.router_identity(),
            native_control.clone(),
            self.provider_operations.clone(),
            self.claude_code_sessions.clone(),
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
        let board = crate::collaboration_application::BoardOperations::new(
            self.router_identity(),
            self.project_admission.clone(),
            self.project_claim.clone(),
            self.notification_publisher.clone(),
            self.subscription_delivery.clone(),
            self.subscription_presence.clone(),
            self.subscription_clock.clone(),
            self.interaction_store.clone(),
            self.project_replication.clone(),
            session_context,
            self.project_admission
                .as_ref()
                .map(|admission| admission.registry.clone()),
        );
        let messages = crate::collaboration_application::MessageOperations::new(
            self.router_identity(),
            self.interaction_store.clone(),
            self.mail_delivery.clone(),
            self.display_names.clone(),
            self.project_admission
                .as_ref()
                .map(|admission| admission.registry.clone()),
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
        let automation = crate::collaboration_application::AutomationOperations::new(
            self.router_identity(),
            self.automation.clone(),
            self.configuration.clone(),
            self.configuration_backend.clone(),
            self.session_delivery.clone(),
            self.interaction_store.clone(),
            self.scheduled_run_execution.clone(),
            self.native_backend.clone(),
        );
        let interactions = crate::collaboration_application::InteractionOperations::new(
            self.approval_broker.clone(),
        );
        let wakes = crate::collaboration_application::WakeOperations::new(
            &self.service_id.clone(),
            self.automation.clone().as_ref(),
        )
        .with_wait_capacity(&self.wake_wait_permits.clone())
        .with_remote(self.mail_delivery.as_ref());
        crate::CollaborationApplication::new(
            self.router_identity(),
            board,
            conversations,
            messages,
            observation,
            sessions,
            automation,
            wakes,
            interactions,
        )
    }
}
