//! ACP provider process admission and connection lifetime.

mod external_approval_dispatch;
mod provider_auth_status_update;
mod provider_client_initialization;
mod provider_client_operations;
mod provider_connection_task;
mod provider_cursor_create_plan;
mod provider_cursor_question;
mod provider_form_elicitation;
mod provider_initialize_request;
mod provider_lifecycle_requests;
mod provider_model_discovery;
mod provider_request_fallback;
mod provider_transport;
use crate::ProviderCapabilityReport;
use crate::approval_turn_cancellation::{ActiveApprovalContext, OwnerApprovalBindings};
pub(crate) use crate::provider_acp_error_mapping::acp_operation_error;
#[cfg(feature = "test-observation")]
pub(crate) use crate::provider_client_contract::sanitized_acp_error;
#[cfg(feature = "test-observation")]
pub(crate) use crate::provider_client_contract::sanitized_initialization_error;
pub use crate::provider_client_contract::{
    ExternalProviderAdmission, ExternalProviderApprovalRefusalReason,
    ExternalProviderCreatedSession, ExternalProviderLaunch, ExternalProviderPromptOutcome,
    ExternalProviderRuntimeError, ProviderErrorCorrelationId, ProviderSessionSummary,
};
#[cfg(any(test, feature = "test-observation"))]
pub use crate::provider_client_contract::{
    ExternalProviderApprovalRefusalWarning, ExternalProviderPermissionObservation,
    ExternalProviderPermissionOutcome, ExternalProviderToolCall, ExternalProviderToolOutcome,
};
use crate::provider_cursor_plan_items::ProviderCursorTodoHandler;
pub(crate) use crate::provider_frame_observation::ProviderFrameObservation;
#[cfg(any(test, feature = "test-observation"))]
pub(crate) use crate::provider_frame_observation::frame_limit_exceeded_count_for_test;
use crate::{InteractionPort, SessionEventSink};
use agent_client_protocol::schema::v1::{McpServer, McpServerHttp, RequestPermissionRequest};
use agent_client_protocol::{AcpAgent, Client, Lines};
use external_approval_dispatch::{
    PermissionDispatchState, record_permission_refusal, spawn_external_approval_dispatch,
};
use provider_auth_status_update::ProviderAuthStatusHandler;
use provider_cursor_create_plan::ProviderCursorCreatePlanHandler;
use provider_cursor_question::ProviderCursorQuestionHandler;
use provider_form_elicitation::ProviderFormElicitationHandler;
use provider_initialize_request::initialize_provider_connection;
use provider_request_fallback::{ProviderRequestFallback, ProviderRequestSessionGuard};
use std::collections::HashMap;
use std::path::PathBuf;
#[cfg(any(test, feature = "test-observation"))]
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, atomic::Ordering};
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(15);
/// ACP JSON-RPC frames are capped at 64 MiB to allow large tool results while
/// keeping each provider connection's transport memory bounded.
pub(crate) const MAX_ACP_FRAME_BYTES: usize = 64 * 1024 * 1024;

struct ProviderCommand {
    cwd: Option<PathBuf>,
    reply: tokio::sync::oneshot::Sender<
        Result<Vec<ProviderSessionSummary>, ExternalProviderRuntimeError>,
    >,
}
/// Owns the provider process and ACP connection independently of caller tasks.
pub struct AgentSessionClient<P: InteractionPort> {
    admission: ExternalProviderAdmission,
    base_capabilities: ProviderCapabilityReport,
    auth_status: Arc<tokio::sync::RwLock<session_event_model::ProviderAuthStatus>>,
    clock: Arc<dyn router_clock::RuntimeClock>,
    shutdown: CancellationToken,
    /// A locally requested stop, distinct from transport failure breaking the task.
    sink_closed: CancellationToken,
    retirement: CancellationToken,
    task: tokio::sync::Mutex<Option<JoinHandle<()>>>,
    shutdown_failed: Arc<std::sync::atomic::AtomicBool>,
    frame_observation: Arc<ProviderFrameObservation>,
    commands: tokio::sync::mpsc::Sender<ProviderCommand>,
    owner_wire: Arc<crate::owner_wire::OwnerAcpWire>,
    provider_behavior: Arc<dyn crate::ProviderBehavior>,
    #[cfg(any(test, feature = "test-observation"))]
    permission_request_count: Arc<AtomicU64>,
    #[cfg(any(test, feature = "test-observation"))]
    permission_outcome: Arc<std::sync::atomic::AtomicU8>,
    interaction_port: Arc<P>,
    _event_sink: Arc<dyn SessionEventSink>,
    approval_contexts: Arc<std::sync::Mutex<HashMap<String, ActiveApprovalContext<P>>>>,
    permission_refusal_reasons:
        Arc<std::sync::Mutex<HashMap<P::OperationId, ExternalProviderApprovalRefusalReason>>>,
    endpoint_id: Arc<tokio::sync::RwLock<Option<String>>>,
    #[cfg(any(test, feature = "test-observation"))]
    approval_refusal_warnings: Arc<std::sync::Mutex<Vec<ExternalProviderApprovalRefusalWarning>>>,
    #[cfg(any(test, feature = "test-observation"))]
    test_tool_calls: Arc<std::sync::Mutex<Vec<ExternalProviderToolCall>>>,
}

impl<P: InteractionPort> AgentSessionClient<P> {
    #[must_use]
    pub fn owner_approval_bindings(&self) -> OwnerApprovalBindings<P> {
        OwnerApprovalBindings::new(
            Arc::clone(&self.approval_contexts),
            Arc::clone(&self.interaction_port),
        )
    }

    #[must_use]
    pub fn owner_wire(&self) -> Arc<crate::owner_wire::OwnerAcpWire> {
        Arc::clone(&self.owner_wire)
    }

    #[must_use]
    pub fn runtime_clock(&self) -> Arc<dyn router_clock::RuntimeClock> {
        Arc::clone(&self.clock)
    }

    pub fn can_reuse_setting_confirmation(
        &self,
        kind: provider_session_machines::ProviderSettingKind,
    ) -> bool {
        self.provider_behavior.can_reuse_setting_confirmation(kind)
    }

    pub fn prepare_owner_prompt(
        &self,
        input: crate::ProviderTurnInput,
        capabilities: &ProviderCapabilityReport,
    ) -> Result<crate::ProviderPromptContent, ExternalProviderRuntimeError> {
        use crate::provider_prompt_content::join_submitted_text_blocks;
        use session_event_model::PromptContent;
        let (contents, history_text, selected) = match input {
            crate::ProviderTurnInput::Prompt(submission) => {
                let original =
                    join_submitted_text_blocks(submission.content.iter().filter_map(|block| {
                        match block {
                            PromptContent::Text { text } => Some(text.as_str()),
                            _ => None,
                        }
                    }));
                let selected = submission.selected.clone();
                (
                    self.provider_behavior.prepare(submission)?,
                    Some(original),
                    selected,
                )
            }
            crate::ProviderTurnInput::Compact => match self.provider_behavior.compact()? {
                crate::PreparedCompact::Prompt(content) => (content, None, Vec::new()),
            },
        };
        let mut prepared =
            crate::ProviderPromptContent::from_session_contents(contents, capabilities)?;
        prepared.set_input_context(history_text, selected);
        Ok(prepared)
    }
}

impl<P: InteractionPort> std::fmt::Debug for AgentSessionClient<P> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AgentSessionClient")
            .field("admission", &self.admission)
            .finish_non_exhaustive()
    }
}

impl<P: InteractionPort> Drop for AgentSessionClient<P> {
    fn drop(&mut self) {
        self.retirement.cancel();
        self.shutdown.cancel();
    }
}
