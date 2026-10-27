//! The typed collaboration application service.
//!
//! One public async method per collaboration operation, grouped by family. Each family takes
//! the existing typed `collaboration-protocol` and `message-board` requests and returns their
//! typed results or the family's typed failure. Transport adapters only decode requests, call
//! these methods and encode what comes back; no operation logic lives in a transport.
//!
//! Each family owns only the capabilities its constructor receives.
//! [`CollaborationApplication`] shares the constructed families and lends callers their APIs: board (`board_operations`); messages (`message_operations`);
//! conversations (`conversation_operations`); wakes (`wake_operations`); schedules, runs,
//! instructions and automation inspection (`automation_operations`, `schedule_operations`,
//! `automation_inspection_operations`, `automation_history_operations`); approvals and
//! questions (`interaction_operations`); sessions, the journal and the address book
//! (`session_operations`, `journal_operations`); provider observation
//! (`provider_observation_operations`). Heavier paging logic stays in the crate's domain modules
//! (`codex_session_inventory`, `provider_session_inventory`, `schedule_preparation`).
#![expect(
    clippy::result_large_err,
    reason = "typed failures are the published payloads, returned once per IO-bound request"
)]
mod automation_history_operations;
mod automation_inspection_operations;
mod automation_operations;
mod board_operations;
mod conversation_operations;
mod interaction_operations;
mod journal_operations;
mod message_operations;
mod provider_observation_operations;
mod schedule_operations;
mod session_context_reader;
mod session_native_control;
mod session_operations;
pub use crate::inventory_failures::{
    NativeSessionFailure, NativeSessionFailureKind, NativeSessionStage, ProviderInventoryFailure,
    ProviderInventoryFailureKind,
};
pub use crate::response_budget::{API_RESULT_BUDGET, RESULT_LIMIT_BYTES, ResultByteBudget};
pub use session_context_reader::SessionContextReader;
pub use session_native_control::NativeSessionControl;
pub(crate) mod wake_operations;

pub use automation_operations::AutomationOperations;
pub use board_operations::{BoardOperations, ThreadWaitBudgetError, thread_wait_root_notice_limit};
pub use conversation_operations::{ConversationFailure, ConversationOperations};
pub use interaction_operations::{InteractionFailure, InteractionOperations, QuestionRejection};
pub use journal_operations::{
    JournalCursorInvalidation, JournalFailure, JournalOperations, JournalUnavailableKind,
};
pub use message_operations::{
    MessageFailure, MessageFailureKind, MessageFailureStage, MessageOperations,
};
pub use provider_observation_operations::{
    ObservationFailure, ObservationOperations, ObservedEventSink,
};
pub use schedule_operations::ScheduleOperationFailure;
pub use session_operations::{
    EndpointDirectoryUnavailable, InstallationIdentityUnavailable, SessionOperations,
};
#[cfg(test)]
pub(crate) use session_operations::{
    classify_native_call_failure, rename_method_unsupported, valid_session_rename_name,
};
pub use wake_operations::{WakeOperations, WakeWaitFailure};

#[cfg(test)]
#[path = "thread_wait_budget_tests.rs"]
mod thread_wait_budget_tests;
use collaboration_contracts::RouterIdentity;

/// Constructed operation families serving one Router installation and run.
#[derive(Clone)]
pub struct CollaborationApplication {
    identity: RouterIdentity,
    board: std::sync::Arc<BoardOperations>,
    conversations: std::sync::Arc<ConversationOperations>,
    messages: std::sync::Arc<MessageOperations>,
    observation: std::sync::Arc<ObservationOperations>,
    sessions: std::sync::Arc<SessionOperations>,
    automation: std::sync::Arc<AutomationOperations>,
    wakes: std::sync::Arc<WakeOperations>,
    interactions: std::sync::Arc<InteractionOperations>,
}
impl CollaborationApplication {
    #[must_use]
    #[expect(
        clippy::too_many_arguments,
        reason = "Constructor injection keeps each independently owned capability explicit instead of recreating a service locator."
    )]
    pub fn new(
        identity: RouterIdentity,
        board: BoardOperations,
        conversations: ConversationOperations,
        messages: MessageOperations,
        observation: ObservationOperations,
        sessions: SessionOperations,
        automation: AutomationOperations,
        wakes: WakeOperations,
        interactions: InteractionOperations,
    ) -> Self {
        Self {
            identity,
            board: std::sync::Arc::new(board),
            conversations: std::sync::Arc::new(conversations),
            messages: std::sync::Arc::new(messages),
            observation: std::sync::Arc::new(observation),
            sessions: std::sync::Arc::new(sessions),
            automation: std::sync::Arc::new(automation),
            wakes: std::sync::Arc::new(wakes),
            interactions: std::sync::Arc::new(interactions),
        }
    }
    #[must_use]
    pub fn service_id(&self) -> &collaboration_protocol::UuidIdentity {
        self.identity.service_id()
    }
    #[must_use]
    pub fn service_epoch(&self) -> &collaboration_protocol::UuidIdentity {
        self.identity.service_epoch()
    }
    #[must_use]
    pub fn installation_peer(&self) -> Option<fleet_sync_protocol::PeerId> {
        self.identity.installation_peer()
    }
    #[must_use]
    pub fn machine_label(&self) -> &collaboration_protocol::MachineLabel {
        self.identity.machine_identity().machine_label()
    }
    #[must_use]
    pub fn board(&self) -> &BoardOperations {
        &self.board
    }
    #[must_use]
    pub fn conversations(&self) -> &ConversationOperations {
        &self.conversations
    }
    #[must_use]
    pub fn messages(&self) -> &MessageOperations {
        &self.messages
    }
    #[must_use]
    pub fn observation(&self) -> &ObservationOperations {
        &self.observation
    }
    #[must_use]
    pub fn sessions(&self) -> &SessionOperations {
        &self.sessions
    }
    #[must_use]
    pub fn automation(&self) -> &AutomationOperations {
        &self.automation
    }
    #[must_use]
    pub fn wakes(&self) -> &WakeOperations {
        &self.wakes
    }
    #[must_use]
    pub fn interactions(&self) -> &InteractionOperations {
        &self.interactions
    }
}

// Rejection tests compose existing failure builders without exposing application internals.
#[cfg(test)]
pub(crate) use automation_operations::run_failure_context;
#[cfg(test)]
pub(crate) use schedule_operations::{ScheduleFailureContext, schedule_failure};
#[cfg(test)]
pub(crate) use session_native_control::INVALID_NATIVE_PARAMETERS;

#[cfg(test)]
pub(crate) fn instruction_rejection_fixture(
    kind: collaboration_protocol::InstructionFailureKind,
) -> collaboration_protocol::InstructionFailure {
    use automation_operations::{
        InstructionContext, InstructionFailureReason, instruction_failure,
    };
    use collaboration_protocol::{InstructionStage, LocalMutationState};

    instruction_failure(
        InstructionContext {
            operation_id: None,
            instruction_id: None,
            mutation: true,
            current_revision_id: None,
        },
        InstructionFailureReason {
            kind,
            stage: InstructionStage::Storage,
            message: "failure",
            mutation: LocalMutationState::None,
        },
    )
}
