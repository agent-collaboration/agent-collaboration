//! The typed collaboration application service.
//!
//! One public async method per collaboration operation, grouped by family. Each family takes
//! the existing typed `collaboration-protocol` requests and returns their typed results or the
//! family's typed failure. Transport adapters only decode requests, call these methods and
//! encode what comes back; no operation logic lives in a transport.
//!
//! Each family owns only the capabilities its constructor receives.
//! [`CollaborationApplication`] shares the constructed families and lends callers their APIs:
//! conversations (`conversation_operations`); approvals and questions
//! (`interaction_operations`); sessions, the journal and the address book
//! (`session_operations`, `journal_operations`); provider observation
//! (`provider_observation_operations`). Heavier paging logic stays in the crate's domain modules
//! (`codex_session_inventory`, `provider_session_inventory`).
mod conversation_operations;
mod interaction_operations;
mod journal_operations;
mod provider_observation_operations;
mod session_native_control;
mod session_operations;
pub use crate::inventory_failures::{
    NativeSessionFailure, NativeSessionFailureKind, NativeSessionStage, ProviderInventoryFailure,
    ProviderInventoryFailureKind,
};
pub use crate::response_budget::{API_RESULT_BUDGET, RESULT_LIMIT_BYTES, ResultByteBudget};
pub use session_native_control::NativeSessionControl;

pub use conversation_operations::{ConversationFailure, ConversationOperations};
pub use interaction_operations::{InteractionFailure, InteractionOperations, QuestionRejection};
pub use journal_operations::{
    JournalCursorInvalidation, JournalFailure, JournalOperations, JournalUnavailableKind,
};
pub use provider_observation_operations::{
    ObservationFailure, ObservationOperations, ObservedEventSink,
};
pub use session_operations::{EndpointDirectoryUnavailable, SessionOperations};
#[cfg(test)]
pub(crate) use session_operations::{
    classify_native_call_failure, rename_method_unsupported, valid_session_rename_name,
};

use collaboration_contracts::RouterIdentity;

/// Constructed operation families serving one Router installation and run.
#[derive(Clone)]
pub struct CollaborationApplication {
    identity: RouterIdentity,
    conversations: std::sync::Arc<ConversationOperations>,
    observation: std::sync::Arc<ObservationOperations>,
    sessions: std::sync::Arc<SessionOperations>,
    interactions: std::sync::Arc<InteractionOperations>,
}
impl CollaborationApplication {
    #[must_use]
    pub fn new(
        identity: RouterIdentity,
        conversations: ConversationOperations,
        observation: ObservationOperations,
        sessions: SessionOperations,
        interactions: InteractionOperations,
    ) -> Self {
        Self {
            identity,
            conversations: std::sync::Arc::new(conversations),
            observation: std::sync::Arc::new(observation),
            sessions: std::sync::Arc::new(sessions),
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
    pub fn machine_label(&self) -> &collaboration_protocol::MachineLabel {
        self.identity.machine_identity().machine_label()
    }
    #[must_use]
    pub fn conversations(&self) -> &ConversationOperations {
        &self.conversations
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
    pub fn interactions(&self) -> &InteractionOperations {
        &self.interactions
    }
}

#[cfg(test)]
pub(crate) use session_native_control::INVALID_NATIVE_PARAMETERS;
