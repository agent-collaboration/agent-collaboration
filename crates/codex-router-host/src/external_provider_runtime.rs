//! Host composition for the extracted ACP client.

#[cfg(test)]
use provider_session_machines::session_lifecycle_machine::RestoreVia;
#[cfg(test)]
use std::path::PathBuf;
use std::sync::Arc;
#[cfg(test)]
use std::time::Duration;

use acp_client_runtime::AgentSessionClient;
#[cfg(test)]
pub(crate) use acp_client_runtime::ProviderPromptDispatchObservation;
#[cfg(test)]
use collaboration_protocol::SessionRef;
use collaboration_protocol::{OperationId, ProviderPromptStopReason};
use collaboration_runtime::ProviderSessionEventHub;
use message_board::SessionEndpointRef;
use tokio_util::sync::CancellationToken;

#[cfg(test)]
use crate::{ProviderSessionActivity, ProviderSteeringOutcome};
use crate::{
    acp_interaction_port::HostInteractionPort, session_owner_ingress::SessionOwnerIngress,
};
#[cfg(test)]
use acp_client_runtime::{
    acp_operation_error_for_test as acp_operation_error,
    sanitized_acp_error_for_test as sanitized_acp_error,
};

pub use acp_client_runtime::{
    ExternalProviderAdmission, ExternalProviderApprovalRefusalReason, ExternalProviderLaunch,
    ExternalProviderRuntimeError,
};
#[cfg(test)]
pub use acp_client_runtime::{
    ExternalProviderApprovalRefusalWarning, ExternalProviderPermissionObservation,
    ExternalProviderPermissionOutcome, ExternalProviderToolCall, ExternalProviderToolOutcome,
};

#[cfg(test)]
use crate::external_provider_approval_context::ExternalProviderApprovalContext;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalProviderPromptOutcome {
    pub output: String,
    pub stop_reason: ProviderPromptStopReason,
    pub permission_refusal_reason: Option<ExternalProviderApprovalRefusalReason>,
}

#[cfg(test)]
fn host_prompt_outcome(
    outcome: acp_client_runtime::ExternalProviderPromptOutcome,
) -> Result<ExternalProviderPromptOutcome, ExternalProviderRuntimeError> {
    let stop_reason = match outcome.stop_reason {
        session_event_model::StopReason::EndTurn => ProviderPromptStopReason::EndTurn,
        session_event_model::StopReason::MaxTokens => ProviderPromptStopReason::MaxTokens,
        session_event_model::StopReason::MaxTurnRequests => {
            ProviderPromptStopReason::MaxTurnRequests
        }
        session_event_model::StopReason::Refusal => ProviderPromptStopReason::Refusal,
        session_event_model::StopReason::Cancelled => ProviderPromptStopReason::Cancelled,
        session_event_model::StopReason::Unknown(value) => {
            let safe_value = if !value.is_empty()
                && value.len() <= 32
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'_')
            {
                value
            } else {
                "unrecognized".to_owned()
            };
            ProviderPromptStopReason::Unknown(safe_value)
        }
    };
    Ok(ExternalProviderPromptOutcome {
        output: outcome.output,
        stop_reason,
        permission_refusal_reason: outcome.permission_refusal_reason,
    })
}

#[cfg(test)]
#[path = "external_provider_prompt_projection_test.rs"]
mod prompt_projection_tests;

#[cfg(test)]
#[path = "external_provider_runtime_initialization_test.rs"]
mod initialization_authentication_tests;

pub struct ExternalProviderRuntime {
    client: AgentSessionClient<HostInteractionPort>,
    owner_ingress: Arc<SessionOwnerIngress>,
    owner_door: std::sync::OnceLock<Arc<crate::session_owner_door::SessionOwnerDoor>>,
    #[cfg(test)]
    pub(crate) preparation_fixture:
        Arc<crate::session_owner_preparation_fixture::PreparationFixture>,
    #[cfg(test)]
    fixture: tokio::sync::OnceCell<owner_fixture::RuntimeFixture>,
    #[cfg(test)]
    fixture_hub: Option<(Arc<ProviderSessionEventHub>, SessionEndpointRef)>,
    /// The binding's work roots and each session's requested mode (D1 R18).
    session_setup: crate::provider_session_setup::BindingSessionSetup,
}

impl std::fmt::Debug for ExternalProviderRuntime {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExternalProviderRuntime")
            .field("admission", self.client.admission())
            .finish_non_exhaustive()
    }
}

impl ExternalProviderRuntime {
    pub(crate) fn owner_clock(&self) -> Arc<dyn router_clock::RuntimeClock> {
        self.client.runtime_clock()
    }

    pub(crate) fn can_reuse_setting_confirmation(
        &self,
        kind: provider_session_machines::ProviderSettingKind,
    ) -> bool {
        self.client.can_reuse_setting_confirmation(kind)
    }

    pub(crate) async fn prepare_owner_prompt(
        &self,
        provider_id: &str,
        input: acp_client_runtime::ProviderTurnInput,
    ) -> Result<acp_client_runtime::ProviderPromptContent, ExternalProviderRuntimeError> {
        let capabilities = self.capability_report(provider_id).await;
        self.client.prepare_owner_prompt(input, &capabilities)
    }

    pub fn session_owner_door(
        &self,
        store: Arc<tokio::sync::Mutex<collaboration_runtime::ProviderOperationStore>>,
        hub: Arc<collaboration_runtime::ProviderSessionEventHub>,
    ) -> Arc<crate::session_owner_door::SessionOwnerDoor> {
        self.owner_door
            .get_or_init(|| {
                let door = Arc::new(crate::session_owner_door::SessionOwnerDoor::new(
                    self.client.owner_wire(),
                    self.client.owner_approval_bindings(),
                    self.client.runtime_clock(),
                    store,
                    hub,
                    self.retirement(),
                    #[cfg(test)]
                    self.preparation_fixture.clone(),
                ));
                self.owner_ingress.attach(&door);
                door
            })
            .clone()
    }

    pub async fn initialize(
        launch: ExternalProviderLaunch,
        behavior: Arc<dyn acp_client_runtime::ProviderBehavior>,
    ) -> Result<Self, ExternalProviderRuntimeError> {
        let owner_ingress = Arc::new(SessionOwnerIngress::default());
        let interaction_port = Arc::new(HostInteractionPort(owner_ingress.clone()));
        let client = AgentSessionClient::initialize(
            launch,
            Arc::clone(&interaction_port),
            owner_ingress.clone(),
            behavior,
        )
        .await?;
        Ok(Self {
            client,
            owner_ingress,
            owner_door: std::sync::OnceLock::new(),
            #[cfg(test)]
            preparation_fixture: Arc::default(),
            #[cfg(test)]
            fixture: tokio::sync::OnceCell::new(),
            #[cfg(test)]
            fixture_hub: None,
            session_setup: crate::provider_session_setup::BindingSessionSetup::default(),
        })
    }

    pub async fn initialize_with_mcp_http(
        launch: ExternalProviderLaunch,
        model_picker: acp_client_runtime::ProviderModelPicker,
        server_name: impl Into<String>,
        server_url: impl Into<String>,
        behavior: Arc<dyn acp_client_runtime::ProviderBehavior>,
    ) -> Result<Self, ExternalProviderRuntimeError> {
        let owner_ingress = Arc::new(SessionOwnerIngress::default());
        let interaction_port = Arc::new(HostInteractionPort(owner_ingress.clone()));
        let client = AgentSessionClient::initialize_with_mcp_http(
            launch,
            model_picker,
            server_name,
            server_url,
            Arc::clone(&interaction_port),
            owner_ingress.clone(),
            behavior,
            Arc::new(router_clock::SystemRuntimeClock::new()),
        )
        .await?;
        Ok(Self {
            client,
            owner_ingress,
            owner_door: std::sync::OnceLock::new(),
            #[cfg(test)]
            preparation_fixture: Arc::default(),
            #[cfg(test)]
            fixture: tokio::sync::OnceCell::new(),
            #[cfg(test)]
            fixture_hub: None,
            session_setup: crate::provider_session_setup::BindingSessionSetup::default(),
        })
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "The hub and ACP connection share the Host clock."
    )]
    pub(crate) async fn initialize_with_mcp_http_and_hub(
        launch: ExternalProviderLaunch,
        model_picker: acp_client_runtime::ProviderModelPicker,
        server_name: impl Into<String>,
        server_url: impl Into<String>,
        _hub: Arc<ProviderSessionEventHub>,
        _endpoint: SessionEndpointRef,
        behavior: Arc<dyn acp_client_runtime::ProviderBehavior>,
        clock: Arc<dyn router_clock::RuntimeClock>,
    ) -> Result<Self, ExternalProviderRuntimeError> {
        let owner_ingress = Arc::new(SessionOwnerIngress::default());
        let interaction_port = Arc::new(HostInteractionPort(owner_ingress.clone()));
        let client = AgentSessionClient::initialize_with_mcp_http(
            launch,
            model_picker,
            server_name,
            server_url,
            Arc::clone(&interaction_port),
            owner_ingress.clone(),
            behavior,
            clock,
        )
        .await?;
        Ok(Self {
            client,
            owner_ingress,
            owner_door: std::sync::OnceLock::new(),
            #[cfg(test)]
            preparation_fixture: Arc::default(),
            #[cfg(test)]
            fixture: tokio::sync::OnceCell::new(),
            #[cfg(test)]
            fixture_hub: Some((_hub, _endpoint)),
            session_setup: crate::provider_session_setup::BindingSessionSetup::default(),
        })
    }

    #[cfg(feature = "test-support")]
    pub async fn initialize_with_fixture_clock(
        launch: ExternalProviderLaunch,
        behavior: Arc<dyn acp_client_runtime::ProviderBehavior>,
        hub: Arc<ProviderSessionEventHub>,
        endpoint: SessionEndpointRef,
        clock: Arc<dyn router_clock::RuntimeClock>,
    ) -> Result<Self, ExternalProviderRuntimeError> {
        Self::initialize_with_mcp_http_and_hub(
            launch,
            acp_client_runtime::ProviderModelPicker::Standard,
            "fixture",
            "http://127.0.0.1:1/mcp",
            hub,
            endpoint,
            behavior,
            clock,
        )
        .await
    }

    #[cfg(test)]
    async fn initialize_with_timeout(
        launch: ExternalProviderLaunch,
        initialize_timeout: Duration,
        behavior: Arc<dyn acp_client_runtime::ProviderBehavior>,
    ) -> Result<Self, ExternalProviderRuntimeError> {
        let owner_ingress = Arc::new(SessionOwnerIngress::default());
        let interaction_port = Arc::new(HostInteractionPort(owner_ingress.clone()));
        let client = AgentSessionClient::initialize_with_timeout(
            launch,
            initialize_timeout,
            Arc::clone(&interaction_port),
            owner_ingress.clone(),
            behavior,
        )
        .await?;
        Ok(Self {
            client,
            owner_ingress,
            owner_door: std::sync::OnceLock::new(),
            preparation_fixture: Arc::default(),
            #[cfg(test)]
            fixture: tokio::sync::OnceCell::new(),
            #[cfg(test)]
            fixture_hub: None,
            session_setup: crate::provider_session_setup::BindingSessionSetup::default(),
        })
    }

    pub(crate) fn take_permission_refusal(
        &self,
        operation: &OperationId,
    ) -> Option<ExternalProviderApprovalRefusalReason> {
        self.client.take_permission_refusal(operation)
    }

    pub(crate) fn frame_limit_exceeded(&self) -> bool {
        self.client.frame_limit_exceeded()
    }

    pub fn admission(&self) -> &ExternalProviderAdmission {
        self.client.admission()
    }

    pub(crate) async fn capability_report(
        &self,
        provider_session_id: &str,
    ) -> acp_client_runtime::ProviderCapabilityReport {
        let connection = self.client.connection_capability_report().await;
        let observed = self
            .owner_door
            .get()
            .and_then(|door| door.existing_provider_session(provider_session_id))
            .and_then(|owner| owner.snapshots().borrow().projection.capabilities.clone());
        match observed {
            Some(observed) => connection.with_session_model(&observed),
            None => connection,
        }
    }

    pub fn retirement(&self) -> CancellationToken {
        self.client.retirement()
    }

    pub async fn set_endpoint_id(&self, endpoint_id: String) {
        self.client.set_endpoint_id(endpoint_id).await;
    }

    pub(crate) fn session_setup(&self) -> &crate::provider_session_setup::BindingSessionSetup {
        &self.session_setup
    }

    pub async fn shutdown(&self) {
        if let Some(door) = self.owner_door.get() {
            door.shutdown().await;
        }
        self.client.shutdown().await;
    }

    pub fn shutdown_failed(&self) -> bool {
        self.client.shutdown_failed()
    }

    #[cfg(test)]
    pub fn permission_observation(&self) -> ExternalProviderPermissionObservation {
        self.client.permission_observation()
    }

    #[cfg(test)]
    pub fn approval_refusal_warnings(&self) -> Vec<ExternalProviderApprovalRefusalWarning> {
        self.client.approval_refusal_warnings()
    }

    #[cfg(test)]
    fn take_test_tool_calls(&self) -> Vec<ExternalProviderToolCall> {
        self.client.take_test_tool_calls()
    }

    #[cfg(test)]
    fn active_approval_operation(&self, provider_session_id: &str) -> Option<OperationId> {
        self.client.active_approval_operation(provider_session_id)
    }

    #[cfg(test)]
    fn active_approval_count(&self) -> usize {
        self.client.active_approval_count()
    }

    #[cfg(test)]
    async fn abort_owner_for_test(&self) {
        self.client.abort_owner_for_test().await;
    }
}

#[cfg(test)]
fn text_contents(
    prompt: String,
) -> Result<Vec<session_event_model::PromptContent>, ExternalProviderRuntimeError> {
    session_event_model::PromptContent::text(prompt)
        .map(|content| vec![content])
        .map_err(|_| ExternalProviderRuntimeError::Operation("invalid prompt text".to_owned()))
}

#[cfg(test)]
fn test_approval_context(
    provider_session_id: &str,
    operation_id: Option<OperationId>,
) -> ExternalProviderApprovalContext {
    let service_id = "0ff962c5-7fa3-4c18-a5ca-1bbe8db09e89";
    let target: SessionRef = serde_json::from_value(serde_json::json!({
        "endpoint": {"serviceId": service_id, "endpointId": "cursor-local"},
        "sessionId": provider_session_id,
    }))
    .expect("test provider target");
    ExternalProviderApprovalContext {
        owner_scope: None,
        requester: target.clone().into(),
        approver: target.clone().into(),
        target,
        operation_id: operation_id.unwrap_or_else(OperationId::generate),
        binding_generation: serde_json::from_value(serde_json::json!({
            "serviceEpoch": service_id, "generation": 1
        }))
        .expect("test generation"),
        binding_retirement: CancellationToken::new(),
    }
}

#[cfg(test)]
#[path = "external_provider_runtime/fixture_tests.rs"]
mod fixture_tests;
#[cfg(test)]
use fixture_tests::*;

#[cfg(test)]
#[path = "external_provider_runtime/owner_fixture.rs"]
mod owner_fixture;

#[cfg(test)]
#[path = "external_provider_runtime/live_test.rs"]
mod live_tests;
#[cfg(test)]
#[path = "external_provider_runtime/robustness_test.rs"]
mod robustness_tests;
#[cfg(test)]
#[path = "external_provider_runtime/runtime_test.rs"]
mod tests;

#[cfg(test)]
#[path = "external_provider_runtime/acp_scripted_fixture.rs"]
pub(crate) mod acp_scripted_fixture;
#[cfg(test)]
#[path = "external_provider_runtime/approval_dispatch_test.rs"]
mod approval_dispatch_tests;
