//! Host composition for the extracted ACP client.

#[cfg(test)]
use std::time::Duration;
use std::{path::PathBuf, sync::Arc};

use acp_client_runtime::AgentSessionClient;
pub(crate) use acp_client_runtime::ProviderPromptDispatchObservation;
use collaboration_protocol::{
    CodexGeneration, OperationId, ProviderIdentity, ProviderPromptStopReason, SessionRef,
};
use collaboration_runtime::ProviderSessionEventHub;
use collaboration_runtime::ServiceInteractionBroker;
use message_board::SessionEndpointRef;
use tokio_util::sync::CancellationToken;

use crate::acp_interaction_port::{HostInteractionPort, NoopSessionEventSink};
use crate::provider_session_event_sink::HubSessionEventSink;
#[cfg(test)]
use crate::{ProviderSessionActivity, ProviderSteeringOutcome};
use acp_client_runtime::{GenerationPublication, SessionGeneration};
#[cfg(test)]
use acp_client_runtime::{
    acp_operation_error_for_test as acp_operation_error,
    sanitized_acp_error_for_test as sanitized_acp_error,
};

pub use acp_client_runtime::{
    ExternalProviderAdmission, ExternalProviderApprovalRefusalReason,
    ExternalProviderCreatedSession, ExternalProviderLaunch, ExternalProviderRuntimeError,
};
#[cfg(test)]
pub use acp_client_runtime::{
    ExternalProviderApprovalRefusalWarning, ExternalProviderPermissionObservation,
    ExternalProviderPermissionOutcome, ExternalProviderToolCall, ExternalProviderToolOutcome,
};

#[derive(Clone, Debug)]
pub struct ExternalProviderApprovalContext {
    pub requester: ProviderIdentity,
    pub approver: ProviderIdentity,
    pub target: SessionRef,
    pub operation_id: OperationId,
    pub binding_generation: CodexGeneration,
    pub binding_retirement: CancellationToken,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalProviderPromptOutcome {
    pub output: String,
    pub stop_reason: ProviderPromptStopReason,
    pub permission_refusal_reason: Option<ExternalProviderApprovalRefusalReason>,
}

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
    interaction_port: Arc<HostInteractionPort>,
    event_sink: Option<Arc<HubSessionEventSink>>,
    /// Test-only: holds the next create, load or resume after the client's success,
    /// before its activation is published.
    #[cfg(test)]
    activation_pause: std::sync::Mutex<
        Option<(
            tokio::sync::oneshot::Sender<()>,
            tokio::sync::oneshot::Receiver<()>,
        )>,
    >,
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
    pub async fn initialize(
        launch: ExternalProviderLaunch,
        behavior: Arc<dyn acp_client_runtime::ProviderBehavior>,
    ) -> Result<Self, ExternalProviderRuntimeError> {
        let interaction_port = Arc::new(HostInteractionPort::default());
        let client = AgentSessionClient::initialize(
            launch,
            Arc::clone(&interaction_port),
            Arc::new(NoopSessionEventSink),
            behavior,
        )
        .await?;
        Ok(Self {
            client,
            interaction_port,
            event_sink: None,
            #[cfg(test)]
            activation_pause: std::sync::Mutex::new(None),
        })
    }

    pub async fn initialize_with_mcp_http(
        launch: ExternalProviderLaunch,
        model_picker: acp_client_runtime::ProviderModelPicker,
        server_name: impl Into<String>,
        server_url: impl Into<String>,
        behavior: Arc<dyn acp_client_runtime::ProviderBehavior>,
    ) -> Result<Self, ExternalProviderRuntimeError> {
        let interaction_port = Arc::new(HostInteractionPort::default());
        let client = AgentSessionClient::initialize_with_mcp_http(
            launch,
            model_picker,
            server_name,
            server_url,
            Arc::clone(&interaction_port),
            Arc::new(NoopSessionEventSink),
            behavior,
        )
        .await?;
        Ok(Self {
            client,
            interaction_port,
            event_sink: None,
            #[cfg(test)]
            activation_pause: std::sync::Mutex::new(None),
        })
    }

    pub(crate) async fn initialize_with_mcp_http_and_hub(
        launch: ExternalProviderLaunch,
        model_picker: acp_client_runtime::ProviderModelPicker,
        server_name: impl Into<String>,
        server_url: impl Into<String>,
        hub: Arc<ProviderSessionEventHub>,
        endpoint: SessionEndpointRef,
        behavior: Arc<dyn acp_client_runtime::ProviderBehavior>,
    ) -> Result<Self, ExternalProviderRuntimeError> {
        let interaction_port = Arc::new(HostInteractionPort::default());
        let event_sink = Arc::new(HubSessionEventSink::new(hub, endpoint));
        let published_sink: Arc<dyn acp_client_runtime::SessionEventSink> = event_sink.clone();
        interaction_port
            .install_event_sink(Arc::clone(&published_sink))
            .await;
        let result = AgentSessionClient::initialize_with_mcp_http(
            launch,
            model_picker,
            server_name,
            server_url,
            Arc::clone(&interaction_port),
            published_sink,
            behavior,
        )
        .await;
        let client = match result {
            Ok(client) => client,
            Err(error) => {
                let _ = event_sink.shutdown().await;
                return Err(error);
            }
        };
        Ok(Self {
            client,
            interaction_port,
            event_sink: Some(event_sink),
            #[cfg(test)]
            activation_pause: std::sync::Mutex::new(None),
        })
    }

    #[cfg(test)]
    async fn initialize_with_timeout(
        launch: ExternalProviderLaunch,
        initialize_timeout: Duration,
        behavior: Arc<dyn acp_client_runtime::ProviderBehavior>,
    ) -> Result<Self, ExternalProviderRuntimeError> {
        let interaction_port = Arc::new(HostInteractionPort::default());
        let client = AgentSessionClient::initialize_with_timeout(
            launch,
            initialize_timeout,
            Arc::clone(&interaction_port),
            Arc::new(NoopSessionEventSink),
            behavior,
        )
        .await?;
        Ok(Self {
            client,
            interaction_port,
            event_sink: None,
            #[cfg(test)]
            activation_pause: std::sync::Mutex::new(None),
        })
    }

    pub async fn install_approval_broker(&self, broker: Arc<ServiceInteractionBroker>) {
        self.interaction_port.install_broker(broker).await;
    }

    pub fn admission(&self) -> &ExternalProviderAdmission {
        self.client.admission()
    }

    pub(crate) async fn capability_report(
        &self,
        provider_session_id: &str,
    ) -> acp_client_runtime::ProviderCapabilityReport {
        self.client.capability_report(provider_session_id).await
    }

    pub(crate) fn install_catalog_refresh(
        &self,
        sender: tokio::sync::mpsc::UnboundedSender<String>,
    ) {
        if let Some(event_sink) = &self.event_sink {
            event_sink.install_catalog_refresh(sender);
        }
    }

    pub(crate) fn install_live_event_observer(
        &self,
        observer: Arc<dyn crate::provider_session_event_sink::LiveSessionEventObserver>,
    ) {
        if let Some(event_sink) = &self.event_sink {
            event_sink.install_live_event_observer(observer);
        }
    }

    pub fn retirement(&self) -> CancellationToken {
        self.client.retirement()
    }

    pub async fn set_endpoint_id(&self, endpoint_id: String) {
        self.client.set_endpoint_id(endpoint_id).await;
    }

    pub async fn create_session(
        &self,
        cwd: PathBuf,
    ) -> Result<String, ExternalProviderRuntimeError> {
        self.create_session_with_observation(cwd)
            .await
            .map(|created| created.provider_session_id)
    }

    pub async fn create_session_with_observation(
        &self,
        cwd: PathBuf,
    ) -> Result<ExternalProviderCreatedSession, ExternalProviderRuntimeError> {
        self.create_session_with_settings(
            cwd,
            None,
            acp_client_runtime::RequestedProviderSettings::default(),
        )
        .await
    }

    pub async fn create_session_with_settings(
        &self,
        cwd: PathBuf,
        scratch_folder: Option<PathBuf>,
        settings: acp_client_runtime::RequestedProviderSettings,
    ) -> Result<ExternalProviderCreatedSession, ExternalProviderRuntimeError> {
        self.create_session_with_work_roots(
            cwd,
            scratch_folder,
            settings,
            acp_client_runtime::SessionWorkRoots::default(),
        )
        .await
    }

    /// Creates a session that also works in the Router's work roots (D1 R18).
    pub async fn create_session_with_work_roots(
        &self,
        cwd: PathBuf,
        scratch_folder: Option<PathBuf>,
        settings: acp_client_runtime::RequestedProviderSettings,
        work_roots: acp_client_runtime::SessionWorkRoots,
    ) -> Result<ExternalProviderCreatedSession, ExternalProviderRuntimeError> {
        // The generation the connection admitted the session under, kept
        // with the outcome rather than looked up later (D1 R14).
        let (created, generation) = self
            .client
            .create_session_admitted(cwd, scratch_folder, settings, work_roots)
            .await;
        let active_session = match &created {
            Ok(created) => Some(created.provider_session_id.as_str()),
            Err(ExternalProviderRuntimeError::CreatedWithoutSettings {
                provider_session_id,
                ..
            }) => Some(provider_session_id.as_str()),
            Err(ExternalProviderRuntimeError::InvalidSetting {
                provider_session_id,
                disposition: acp_client_runtime::InvalidSettingSessionDisposition::RemainsCreated,
                ..
            }) => Some(provider_session_id.as_str()),
            _ => None,
        };
        if let Some(session_id) = active_session {
            #[cfg(test)]
            self.pause_before_activation().await;
            self.publish_active_session(session_id, generation).await?;
        }
        created
    }

    pub async fn settings_unresolved(&self, provider_session_id: &str) -> bool {
        self.client.settings_unresolved(provider_session_id).await
    }

    pub async fn setting_outcome_unknown(
        &self,
        provider_session_id: &str,
        kind: acp_client_runtime::ProviderSettingKind,
    ) -> bool {
        self.client
            .setting_outcome_unknown(provider_session_id, kind)
            .await
    }

    pub async fn set_setting(
        &self,
        provider_session_id: String,
        request: acp_client_runtime::ProviderSettingRequest,
    ) -> Result<acp_client_runtime::EffectiveProviderSettings, ExternalProviderRuntimeError> {
        self.client.set_setting(provider_session_id, request).await
    }

    pub async fn accept_session_settings(
        &self,
        provider_session_id: String,
    ) -> Result<acp_client_runtime::EffectiveProviderSettings, ExternalProviderRuntimeError> {
        self.client
            .accept_session_settings(provider_session_id)
            .await
    }

    pub async fn settings_catalog(
        &self,
        provider_session_id: &str,
    ) -> Option<acp_client_runtime::ProviderSettingsCatalog> {
        self.client.settings_catalog(provider_session_id).await
    }

    pub async fn load_session(
        &self,
        provider_session_id: String,
        cwd: PathBuf,
    ) -> Result<(), ExternalProviderRuntimeError> {
        // The generation the connection admitted this load under (D1 R14).
        let generation = self
            .client
            .load_session_with_generation(provider_session_id.clone(), cwd)
            .await?;
        #[cfg(test)]
        self.pause_before_activation().await;
        self.publish_active_session(&provider_session_id, Some(generation))
            .await
    }

    pub async fn resume_session(
        &self,
        provider_session_id: String,
        cwd: PathBuf,
    ) -> Result<(), ExternalProviderRuntimeError> {
        let generation = self
            .client
            .resume_session_with_generation(provider_session_id.clone(), cwd)
            .await?;
        #[cfg(test)]
        self.pause_before_activation().await;
        if let Some(event_sink) = &self.event_sink {
            let invalidated = event_sink
                .begin_history_unavailable(&provider_session_id, generation)
                .await
                .map_err(|_| ExternalProviderRuntimeError::HistoryReplayUnavailable)?;
            if invalidated == GenerationPublication::Ended {
                return Err(ExternalProviderRuntimeError::SessionReleased);
            }
        }
        self.publish_active_session(&provider_session_id, Some(generation))
            .await
    }

    /// Publishes the session active in `generation`. A generation closed
    /// meanwhile publishes nothing, and the session counts as closed.
    async fn publish_active_session(
        &self,
        provider_session_id: &str,
        generation: Option<SessionGeneration>,
    ) -> Result<(), ExternalProviderRuntimeError> {
        if let (Some(event_sink), Some(generation)) = (&self.event_sink, generation) {
            let capabilities = self
                .client
                .capability_report(provider_session_id)
                .await
                .to_session_model();
            let published = event_sink
                .publish_activation(provider_session_id, generation, capabilities)
                .await
                .map_err(|_| ExternalProviderRuntimeError::SinkClosed)?;
            if published == GenerationPublication::Ended {
                return Err(ExternalProviderRuntimeError::SessionReleased);
            }
        }
        Ok(())
    }

    #[cfg(test)]
    async fn pause_before_activation(&self) {
        let pause = self
            .activation_pause
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some((reached, release)) = pause {
            let _ = reached.send(());
            let _ = release.await;
        }
    }

    /// Test-only: the next create, load or resume stops after the client's success,
    /// before its activation; returns (reached, release).
    #[cfg(test)]
    pub(crate) fn pause_next_activation(
        &self,
    ) -> (
        tokio::sync::oneshot::Receiver<()>,
        tokio::sync::oneshot::Sender<()>,
    ) {
        let (reached_sender, reached) = tokio::sync::oneshot::channel();
        let (release, release_receiver) = tokio::sync::oneshot::channel();
        *self
            .activation_pause
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some((reached_sender, release_receiver));
        (reached, release)
    }

    pub async fn close_session(
        &self,
        provider_session_id: String,
    ) -> Result<(), ExternalProviderRuntimeError> {
        // The connection publishes Closed for the run it closed (D1 R14).
        self.client.close_session(provider_session_id).await
    }

    /// Closes a session on an agent that offers no session/close: the turn is
    /// interrupted and the session released by the Router alone (D1 R14).
    pub async fn release_session(
        &self,
        provider_session_id: String,
    ) -> Result<(), ExternalProviderRuntimeError> {
        self.client.release_session(provider_session_id).await
    }

    pub async fn steer_session(
        &self,
        provider_session_id: String,
        prompt: String,
    ) -> Result<crate::ProviderSteeringOutcome, ExternalProviderRuntimeError> {
        self.steer_session_with_input(
            provider_session_id,
            session_event_model::InputId::generate(),
            prompt,
        )
        .await
    }

    pub async fn steer_session_with_input(
        &self,
        provider_session_id: String,
        input_id: session_event_model::InputId,
        prompt: String,
    ) -> Result<crate::ProviderSteeringOutcome, ExternalProviderRuntimeError> {
        self.client
            .steer_contents_with_input(
                provider_session_id,
                input_id,
                session_event_model::ProviderSubmission::plain(text_contents(prompt)?),
            )
            .await
    }

    pub async fn steer_session_contents_with_input(
        &self,
        provider_session_id: String,
        input_id: session_event_model::InputId,
        contents: session_event_model::ProviderSubmission,
    ) -> Result<crate::ProviderSteeringOutcome, ExternalProviderRuntimeError> {
        self.client
            .steer_contents_with_input(provider_session_id, input_id, contents)
            .await
    }

    pub async fn session_activity(
        &self,
        provider_session_id: String,
    ) -> Result<crate::ProviderSessionActivity, ExternalProviderRuntimeError> {
        self.client.session_activity(provider_session_id).await
    }

    pub async fn active_prompt_operation(
        &self,
        provider_session_id: String,
    ) -> Result<Option<OperationId>, ExternalProviderRuntimeError> {
        self.client
            .active_prompt_operation(provider_session_id)
            .await
    }

    pub async fn wait_session_idle(
        &self,
        provider_session_id: String,
    ) -> Result<(), ExternalProviderRuntimeError> {
        self.client.wait_session_idle(provider_session_id).await
    }

    pub async fn cancel_prompt_operation(
        &self,
        provider_session_id: String,
        operation_id: OperationId,
    ) -> Result<(), ExternalProviderRuntimeError> {
        self.client
            .cancel_prompt_operation(provider_session_id, operation_id)
            .await
    }

    #[cfg(test)]
    pub async fn cancel_active_prompt(
        &self,
        provider_session_id: String,
    ) -> Result<(), ExternalProviderRuntimeError> {
        self.client.cancel_active_prompt(provider_session_id).await
    }

    pub async fn shutdown(&self) {
        self.client.shutdown().await;
        if let Some(event_sink) = &self.event_sink
            && let Err(error) = event_sink.shutdown().await
        {
            tracing::error!(%error, "provider Session event consumer failed to drain");
        }
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

    #[cfg(test)]
    pub async fn prompt(
        &self,
        provider_session_id: String,
        prompt: String,
    ) -> Result<ExternalProviderPromptOutcome, ExternalProviderRuntimeError> {
        self.prompt_with_approval_context(
            provider_session_id.clone(),
            prompt,
            test_approval_context(&provider_session_id, None),
        )
        .await
    }

    pub async fn prompt_with_approval_context(
        &self,
        provider_session_id: String,
        prompt: String,
        context: ExternalProviderApprovalContext,
    ) -> Result<ExternalProviderPromptOutcome, ExternalProviderRuntimeError> {
        self.prompt_with_approval_context_for_input(
            provider_session_id,
            session_event_model::InputId::generate(),
            prompt,
            context,
        )
        .await
    }

    pub async fn prompt_with_approval_context_for_input(
        &self,
        provider_session_id: String,
        input_id: session_event_model::InputId,
        prompt: String,
        context: ExternalProviderApprovalContext,
    ) -> Result<ExternalProviderPromptOutcome, ExternalProviderRuntimeError> {
        self.submit_with_approval_dispatch_for_input(
            provider_session_id,
            input_id,
            acp_client_runtime::ProviderTurnInput::Prompt(
                session_event_model::ProviderSubmission::plain(text_contents(prompt)?),
            ),
            context,
            None,
        )
        .await
    }

    pub(crate) async fn submit_with_approval_dispatch_for_input(
        &self,
        provider_session_id: String,
        input_id: session_event_model::InputId,
        contents: acp_client_runtime::ProviderTurnInput,
        context: ExternalProviderApprovalContext,
        dispatch: Option<tokio::sync::oneshot::Sender<ProviderPromptDispatchObservation>>,
    ) -> Result<ExternalProviderPromptOutcome, ExternalProviderRuntimeError> {
        host_prompt_outcome(
            self.client
                .submit_with_approval_dispatch_for_input(
                    provider_session_id,
                    input_id,
                    contents,
                    context,
                    dispatch,
                )
                .await?,
        )
    }

    #[cfg(test)]
    async fn prompt_for_operation(
        &self,
        provider_session_id: String,
        operation_id: Option<OperationId>,
        prompt: String,
        dispatch: Option<tokio::sync::oneshot::Sender<ProviderPromptDispatchObservation>>,
    ) -> Result<ExternalProviderPromptOutcome, ExternalProviderRuntimeError> {
        self.submit_with_approval_dispatch_for_input(
            provider_session_id.clone(),
            session_event_model::InputId::generate(),
            acp_client_runtime::ProviderTurnInput::Prompt(
                session_event_model::ProviderSubmission::plain(text_contents(prompt)?),
            ),
            test_approval_context(&provider_session_id, operation_id),
            dispatch,
        )
        .await
    }
}

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

#[path = "external_provider_runtime/provider_runtime_queue.rs"]
mod provider_runtime_queue;

#[cfg(test)]
#[path = "external_provider_runtime/fixture_tests.rs"]
mod fixture_tests;
#[cfg(test)]
use fixture_tests::*;

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
#[cfg(test)]
#[path = "external_provider_runtime/approval_push_fixture.rs"]
mod approval_push_fixture;
