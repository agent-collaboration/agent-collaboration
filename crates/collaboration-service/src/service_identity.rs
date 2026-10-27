//! The Router's identity and composed collaboration dependencies.
use collaboration_protocol::{EndpointAvailability, EndpointDescription, UuidIdentity};
use collaboration_runtime::EndpointDirectory;
#[path = "subscription_delivery/mod.rs"]
pub mod subscription_delivery;

#[derive(Clone)]
pub struct ServiceIdentity {
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
impl ServiceIdentity {
    pub fn with_mail_delivery_runtime(
        mut self,
        runtime: std::sync::Arc<crate::MailDeliveryRuntime>,
    ) -> Result<Self, String> {
        if !runtime.matches_identity(&self) {
            return Err(
                "mail runtime has a different service, installation peer or physical store owner"
                    .to_owned(),
            );
        }
        if let Some(store) = &self.automation {
            runtime.attach_wake_owner(crate::mail_delivery_runtime::LocalWakeOwner {
                service: self.service_id.clone(),
                store: store.clone(),
                permits: self.wake_wait_permits.clone(),
            })?;
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
                    collaboration_runtime::WakeDeliverySender::new(
                        std::sync::Arc::clone(interactions),
                        std::sync::Arc::clone(delivery),
                        self.configuration.clone(),
                        self.machine_identity.clone(),
                    ),
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
        let mut identities = std::collections::HashSet::new();
        if endpoints.len() > 64 {
            return Err("too many endpoints".into());
        }
        for endpoint in &endpoints {
            if endpoint.endpoint.service_id != self.service_id {
                return Err("endpoint belongs to another service".into());
            }
            if !identities.insert(endpoint.endpoint.clone()) {
                return Err("duplicate endpoint identity".into());
            }
            if endpoint.channels.len() > 2
                || (endpoint.channels.is_empty()
                    && !matches!(
                        endpoint.availability,
                        EndpointAvailability::Unavailable { .. }
                    ))
            {
                return Err("invalid channel count".into());
            }
        }
        for endpoint in endpoints {
            self.directory
                .publish(endpoint)
                .map_err(|error| error.to_string())?;
        }
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
