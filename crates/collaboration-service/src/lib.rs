//! Collaboration application operations, independent of the Host executable.
pub mod collaboration_application;
pub mod collaboration_rejection;
pub use collaboration_application::{CollaborationApplication, ResultByteBudget};
#[cfg(any(test, feature = "test-support"))]
mod application_fixture;
#[cfg(any(test, feature = "test-support"))]
pub use application_fixture::ApplicationFixture;
mod conversation_operation_projection;
pub use codex_acp_adapter::BrokeredApprovalOutcome;
pub use codex_acp_adapter::ConversationOperationRecorder;
pub use codex_acp_adapter::{ACP_SCHEMA_DIGEST, NativeStoredSessions};
pub use collaboration_protocol::{DeliveryClientReceipt, DeliveryReceipt};
pub use conversation_operation_projection::conversation_operation_snapshot;
mod codex_session_inventory;
mod inventory_failures;
mod response_budget;
pub use codex_session_inventory::CodexSessionInventory;
mod native_claude_catalogue;
mod provider_session_inventory;
pub use provider_session_inventory::ProviderSessionInventory;

#[cfg(test)]
mod native_control_failure_tests;
#[cfg(test)]
mod native_recorded_permission_tests;
