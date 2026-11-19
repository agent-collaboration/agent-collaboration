//! ACP provider process admission and connection lifetime.

mod external_approval_dispatch;
mod provider_approval_dispatch;
pub use provider_approval_dispatch::ProviderTurnEnd;
mod provider_auth_status_update;
mod provider_client_initialization;
mod provider_client_operations;
mod provider_connection_retirement;
mod provider_connection_task;
mod provider_cursor_create_plan;
mod provider_cursor_question;
mod provider_form_elicitation;
mod provider_initialize_request;
mod provider_lifecycle_requests;
mod provider_model_discovery;
mod provider_prompt_cancellation;
mod provider_prompt_dispatch;
mod provider_request_fallback;
mod provider_session_admission;
mod provider_session_queue;
mod provider_session_restore;
mod provider_transport;
pub(crate) use crate::session_cell_bookkeeping::{
    busy_answer, hold_setting_at_create, settings_held, settings_hold_answer_for,
};

use crate::ProviderCapabilityReport;
use crate::approval_turn_cancellation::{ActiveApprovalContext, active_turn_cancellation};
use crate::provider_acp_error_mapping::acp_load_session_error;
pub(crate) use crate::provider_acp_error_mapping::acp_operation_error;
use crate::provider_client_contract::ProviderPromptDispatchObservation;
pub(crate) use crate::provider_client_contract::provider_frame_decode_error;
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
use crate::provider_connection_activity::{ProviderConnectionActivity, ToolCallOwnershipHandler};
pub(crate) use crate::provider_cursor_plan_items::CursorPlanItems;
use crate::provider_cursor_plan_items::ProviderCursorTodoHandler;
pub(crate) use crate::provider_frame_observation::ProviderFrameObservation;
#[cfg(any(test, feature = "test-observation"))]
pub(crate) use crate::provider_frame_observation::frame_limit_exceeded_count_for_test;
use crate::provider_prompt_content::ProviderPromptContent;
use crate::provider_session_actor::{
    ProviderSessionActivity, ProviderSessionCommand, ProviderSteeringOutcome, run_provider_session,
};
use crate::provider_session_runtime_handles::ProviderSessionRuntimeHandles;
use crate::{AcpProtocolVersion, InteractionPort, SessionEventSink};
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    LoadSessionRequest, McpServer, McpServerHttp, NewSessionRequest, NewSessionResponse,
    RequestPermissionRequest,
};
use agent_client_protocol::{AcpAgent, ActiveSession, Agent, Client, ConnectionTo, Lines};
use external_approval_dispatch::{
    PermissionDispatchState, record_permission_refusal, spawn_external_approval_dispatch,
};
use provider_auth_status_update::ProviderAuthStatusHandler;
use provider_cursor_create_plan::ProviderCursorCreatePlanHandler;
use provider_cursor_question::ProviderCursorQuestionHandler;
use provider_form_elicitation::ProviderFormElicitationHandler;
use provider_initialize_request::initialize_provider_connection;
use provider_request_fallback::{
    ProviderKnownSessions, ProviderRequestFallback, ProviderRequestSessionGuard,
};
use provider_session_admission::{
    CreateAdmissionInputs, StaticSessionActivation, fail_pending_session_admission,
    publish_pending_session_admission, register_provider_session, register_static_provider_session,
    run_create_admission,
};
pub use provider_session_queue::{
    QueueCancel, QueueMarkStep, QueueRefusal, QueueTicket, QueueTurn, SessionLoadStarted,
};
use session_event_model::InputId;
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

struct ApprovalContextGuard<P: InteractionPort> {
    contexts: Arc<std::sync::Mutex<HashMap<String, ActiveApprovalContext<P>>>>,
    provider_session_id: String,
    operation_id: P::OperationId,
}

impl<P: InteractionPort> Drop for ApprovalContextGuard<P> {
    fn drop(&mut self) {
        if let Ok(mut contexts) = self.contexts.lock()
            && contexts
                .get(&self.provider_session_id)
                .is_some_and(|context| P::operation_id(&context.approval) == self.operation_id)
        {
            contexts.remove(&self.provider_session_id);
        }
    }
}

enum ProviderCommand<P: InteractionPort> {
    Create {
        cwd: PathBuf,
        /// The session's own scratch folder, granted to agents that accept
        /// additional directories and allowed by the in-folder rule.
        scratch_folder: Option<PathBuf>,
        /// The Router's work roots for a session it starts in auto (D1 R18).
        work_roots: crate::SessionWorkRoots,
        settings: crate::RequestedProviderSettings,
        reply: tokio::sync::oneshot::Sender<CreateAdmissionReply>,
    },
    Restore {
        provider_session_id: String,
        cwd: PathBuf,
        mode: provider_session_restore::RestoreHistoryMode,
        /// What the restore rebuilds of the session's setup (D1 R18).
        setup: crate::SessionRestoreSetup,
        /// Told the run's generation as soon as the session's cell admits the
        /// load, before the agent answers it.
        admitted:
            Option<tokio::sync::oneshot::Sender<provider_session_machines::SessionGeneration>>,
        reply: tokio::sync::oneshot::Sender<
            Result<RestoredSessionAdmission, ExternalProviderRuntimeError>,
        >,
    },
    /// Test support: drops the session's command sender without closing the
    /// session, so its actor ends on its own, as when its channel closes.
    #[cfg(any(test, feature = "test-support"))]
    EndActor { provider_session_id: String },
    List {
        cwd: Option<PathBuf>,
        reply: tokio::sync::oneshot::Sender<
            Result<Vec<ProviderSessionSummary>, ExternalProviderRuntimeError>,
        >,
    },
    Close {
        provider_session_id: String,
        /// The close attempt the session's cell admitted when the client
        /// asked to close; the connection settles it.
        attempt: provider_session_machines::session_lifecycle_machine::CloseAttempt,
        /// The agent offers no session/close: the session is only released
        /// here, and no request reaches the agent (D1 R14).
        release_only: bool,
        reply: tokio::sync::oneshot::Sender<Result<(), ExternalProviderRuntimeError>>,
    },
    Prompt {
        provider_session_id: String,
        input_id: InputId,
        operation_id: Option<P::OperationId>,
        prompt: crate::ProviderTurnInput,
        dispatch: Option<tokio::sync::oneshot::Sender<ProviderPromptDispatchObservation>>,
        reply: tokio::sync::oneshot::Sender<
            Result<ExternalProviderPromptOutcome, ExternalProviderRuntimeError>,
        >,
    },
    Cancel {
        provider_session_id: String,
        expected_operation_id: Option<P::OperationId>,
        reply: tokio::sync::oneshot::Sender<Result<(), ExternalProviderRuntimeError>>,
    },
    Steer {
        provider_session_id: String,
        input_id: InputId,
        prompt: ProviderPromptContent,
        reply: crate::provider_session_actor::SteeringReply<P>,
    },
    InspectSession {
        provider_session_id: String,
        reply: tokio::sync::oneshot::Sender<ProviderSessionActivity>,
    },
    InspectActiveOperation {
        provider_session_id: String,
        reply: tokio::sync::oneshot::Sender<Option<P::OperationId>>,
    },
    WaitSessionIdle {
        provider_session_id: String,
        reply: tokio::sync::oneshot::Sender<Result<(), ExternalProviderRuntimeError>>,
    },
    SetSetting {
        provider_session_id: String,
        request: crate::ProviderSettingRequest,
        reply: tokio::sync::oneshot::Sender<
            Result<crate::ProviderSettingApplication, ExternalProviderRuntimeError>,
        >,
    },
}

enum PendingSessionAdmission<P: InteractionPort> {
    Create {
        result: Box<Result<ProviderSessionRegistration<P>, ExternalProviderRuntimeError>>,
        /// The generation the connection admitted the session under; the
        /// actor starts only once it has it.
        activation: tokio::sync::oneshot::Sender<provider_session_machines::SessionGeneration>,
        reply: tokio::sync::oneshot::Sender<CreateAdmissionReply>,
    },
    Restore {
        provider_session_id: String,
        /// The generation this restore was admitted under: its completion
        /// counts only while that is still the session's recorded run.
        generation: provider_session_machines::SessionGeneration,
        /// The load attempt the session's cell admitted; a close or another
        /// load since then makes it stale.
        attempt: provider_session_machines::session_lifecycle_machine::LoadAttempt,
        result: Box<Result<RestoredProviderSession, ExternalProviderRuntimeError>>,
        /// The session's in-folder roots, granted once the restore registers.
        in_folder: Option<crate::in_folder_allowance::InFolderGrant>,
        reply: tokio::sync::oneshot::Sender<
            Result<RestoredSessionAdmission, ExternalProviderRuntimeError>,
        >,
    },
    Close {
        provider_session_id: String,
        /// The close attempt the session's cell admitted.
        attempt: provider_session_machines::session_lifecycle_machine::CloseAttempt,
        result: Result<(), ExternalProviderRuntimeError>,
        reply: tokio::sync::oneshot::Sender<Result<(), ExternalProviderRuntimeError>>,
    },
}

struct ProviderSessionRegistration<P: InteractionPort> {
    provider_session_id: String,
    commands: tokio::sync::mpsc::UnboundedSender<
        ProviderSessionCommand<
            P,
            provider_connection_task::admitted_setting_reply::AdmittedSettingReply,
        >,
    >,
    response: NewSessionResponse,
    settings_catalog: crate::ProviderSettingsCatalog,
    permission_mode: crate::PermissionModeReport,
    setup_error: Option<ExternalProviderRuntimeError>,
    /// The run's in-folder grant and its wish to keep fast off, written by
    /// the connection under the generation it admits the run with.
    in_folder: Option<crate::in_folder_allowance::SessionInFolderGrant>,
    keep_fast_off: bool,
}

/// A create's outcome and, when the connection admitted the session (even
/// one created without its settings), the generation it was admitted under.
pub(crate) struct CreateAdmissionReply {
    pub(crate) result: Result<ExternalProviderCreatedSession, ExternalProviderRuntimeError>,
    pub(crate) generation: Option<provider_session_machines::SessionGeneration>,
}

/// A registered load or resume: the generation the connection admitted it
/// under, and the session's permission mode judged against the mode its
/// create asked for (D1 R18; owner Decision 3).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RestoredSessionAdmission {
    pub generation: provider_session_machines::SessionGeneration,
    pub permission_mode: crate::PermissionModeReport,
    pub mode_difference: Option<crate::RestoredModeDifference>,
}

struct RestoredProviderSession {
    session: ActiveSession<'static, Agent>,
    item_projection: Option<crate::provider_item_projection::ProviderItemProjection>,
    settings_catalog: crate::ProviderSettingsCatalog,
    /// The mode the session runs after the restore put back its recorded
    /// mode, judged against that record (V6; owner Decision 3).
    restored_mode: crate::provider_setting_application::RestoredPermissionMode,
}

/// Owns the provider process and ACP connection independently of caller tasks.
pub struct AgentSessionClient<P: InteractionPort> {
    admission: ExternalProviderAdmission,
    base_capabilities: ProviderCapabilityReport,
    session_capabilities: Arc<crate::run_owned_state::RunOwnedMap<ProviderCapabilityReport>>,
    auth_status: Arc<tokio::sync::RwLock<session_event_model::ProviderAuthStatus>>,
    session_settings: Arc<crate::run_owned_state::RunOwnedMap<crate::ProviderSettingsCatalog>>,
    /// Each provider session's typed lifecycle, shared with the connection.
    session_cells: Arc<crate::session_cell_table::SessionCellTable>,
    shutdown: CancellationToken,
    sink_closed: CancellationToken,
    retirement: CancellationToken,
    task: tokio::sync::Mutex<Option<JoinHandle<()>>>,
    shutdown_failed: Arc<std::sync::atomic::AtomicBool>,
    frame_observation: Arc<ProviderFrameObservation>,
    commands: tokio::sync::mpsc::Sender<ProviderCommand<P>>,
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
    /// Crate tests read whether a session's in-folder grant is in Router auto.
    #[cfg(test)]
    in_folder_grants: crate::in_folder_allowance::InFolderGrants,
}

impl<P: InteractionPort> AgentSessionClient<P> {
    /// `Some(router_auto)` for a session whose current run has an in-folder
    /// grant, `None` without one.
    #[cfg(test)]
    pub(crate) fn in_folder_router_auto_for_test(&self, provider_session_id: &str) -> Option<bool> {
        self.in_folder_grants
            .read(provider_session_id)
            .map(|grant| grant.router_auto)
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

#[cfg(test)]
#[path = "agent_session_client/turn_admission_test.rs"]
mod turn_admission_tests;

#[cfg(test)]
#[path = "agent_session_client/submitted_prompt_test.rs"]
mod submitted_prompt_tests;
