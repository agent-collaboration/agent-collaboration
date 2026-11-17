//! The supervisor-level rig of the send races: a Host supervisor and its
//! session line over the send-race fixture, driven at the backend boundary.

use super::*;

/// A Host supervisor and its session line over the send-race fixture,
/// driven at the backend boundary the collaboration service calls. A test
/// runs the service's precheck itself (`lookup_existing`), so two requests
/// can both pass it before either is admitted.
pub(super) struct DirectSupervisor {
    pub(super) root: tempfile::TempDir,
    pub(super) receipt: PathBuf,
    pub(super) binding: ProviderBindingIdentity,
    pub(super) supervisor: Arc<ExternalProviderSupervisor>,
    pub(super) route: ProviderAcpDeliveryRoute,
    pub(super) owner: ProviderIdentity,
    pub(super) delivery: MessageDelivery,
}

impl DirectSupervisor {
    pub(super) async fn start(behaviours: &[&str], sessions: usize) -> Self {
        let root = tempfile::tempdir().expect("provider root");
        let receipt = root.path().join("wire.json");
        let runtime = ExternalProviderRuntime::initialize(
            provider_fixture_launch::launch(
                "provider-steering-agent.ts",
                send_race_arguments(&receipt, behaviours),
            ),
            Arc::new(acp_provider_adapters::StandardAcpBehavior),
        )
        .await
        .expect("fixture provider");
        let store = Arc::new(tokio::sync::Mutex::new(
            ProviderOperationStore::open(&root.path().join("operations.sqlite"))
                .await
                .expect("store"),
        ));
        let owner = human("race-owner");
        // A loaded session needs no stored record to be prompted or queued.
        for _session in 0..sessions {
            let created = runtime.create_session(root.path().to_owned()).await;
            created.expect("session/new");
        }
        let binding = provider_binding(&target());
        let supervisor = Arc::new(
            ExternalProviderSupervisor::new(
                vec![ExternalProviderBinding {
                    identity: binding.clone(),
                    runtime,
                }],
                Arc::clone(&store),
            )
            .expect("supervisor"),
        );
        let route = ProviderAcpDeliveryRoute::new(
            target().endpoint.service_id,
            std::iter::once(target().endpoint).collect(),
            available_directory(&target(), &binding),
            Arc::clone(&supervisor),
            store,
            Arc::new(NoLivePeer),
            std::sync::Arc::new(router_clock::SystemRuntimeClock::new()),
        );
        Self {
            root,
            receipt,
            binding,
            supervisor,
            route,
            owner,
            delivery: delivery_for(behaviours),
        }
    }

    /// The service's precheck, once per request: no stored operation has
    /// any of these IDs yet.
    pub(super) async fn prechecks_miss(&self, requests: &[&OperationId]) -> bool {
        for operation_id in requests {
            let stored = self
                .supervisor
                .lookup_existing((*operation_id).clone())
                .await;
            if stored.expect("precheck reads the store").is_some() {
                return false;
            }
        }
        true
    }

    #[expect(
        clippy::result_large_err,
        reason = "The backend's typed failure, as the service receives it."
    )]
    pub(super) async fn prompt(
        &self,
        target: &SessionRef,
        operation_id: &OperationId,
        text: &str,
    ) -> Submitted {
        self.supervisor
            .prompt(ConversationPromptRequest {
                delivery: self.delivery,
                operation_id: operation_id.clone(),
                input_id: None,
                target: target.clone(),
                generation: None,
                requested_by: self.owner.clone(),
                approver: self.owner.clone(),
                prompt: serde_json::from_value(serde_json::json!({"kind":"humanUser","text":text}))
                    .expect("prompt content"),
            })
            .await
    }

    pub(super) async fn show(
        &self,
        operation_id: &OperationId,
    ) -> Option<ConversationOperationSnapshot> {
        self.supervisor
            .show(ConversationOperationShowRequest {
                operation_id: operation_id.clone(),
            })
            .await
            .ok()
    }

    /// The operation's settlement, once it settles.
    pub(super) async fn settled(
        &self,
        operation_id: &OperationId,
    ) -> ConversationOperationSettlement {
        loop {
            let waited = self
                .supervisor
                .wait(ConversationOperationWaitRequest {
                    operation_id: operation_id.clone(),
                    timeout_seconds: PositiveSeconds::try_from(10).expect("timeout"),
                })
                .await
                .expect("operation settles");
            if let ConversationOperationWaitOutput::Available { settlement } = waited.output {
                return settlement;
            }
        }
    }

    pub(super) async fn shutdown(self) -> tempfile::TempDir {
        self.route.shutdown_queue().await;
        self.supervisor
            .shutdown()
            .await
            .expect("supervisor shutdown");
        self.root
    }
}
