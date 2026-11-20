//! Collaboration application operations and publication policy, independent of the Host executable.
pub mod collaboration_application;
pub mod collaboration_rejection;
pub use collaboration_application::{CollaborationApplication, ResultByteBudget};
#[cfg(any(test, feature = "test-support"))]
mod application_fixture;
#[cfg(any(test, feature = "test-support"))]
pub use application_fixture::ApplicationFixture;
mod subscription_delivery;
pub use subscription_delivery::{
    HeldMailSignals, SubscriptionDeliveryService, SubscriptionDeliveryServiceProps,
};
mod conversation_operation_projection;
pub use codex_acp_adapter::ConversationOperationRecorder;
pub use collaboration_protocol::{DeliveryClientReceipt, DeliveryReceipt};
pub use conversation_operation_projection::conversation_operation_snapshot;
mod push_record_delivery;
mod push_record_resolver;
pub use codex_acp_adapter::BrokeredApprovalOutcome;
pub use codex_acp_adapter::{ACP_SCHEMA_DIGEST, NativeStoredSessions};
mod codex_session_inventory;
mod inventory_failures;
mod response_budget;
pub use codex_session_inventory::CodexSessionInventory;
mod mail_push_delivery;
mod mail_push_outcome;
mod native_claude_catalogue;
mod provider_session_inventory;
pub use mail_push_delivery::{MailPushDelivery, MailPushDeliveryProps};
pub use provider_session_inventory::ProviderSessionInventory;
mod mail_delivery_runtime;
#[cfg(any(test, feature = "test-support"))]
pub use mail_delivery_runtime::NativeMailOwner;
pub use mail_delivery_runtime::{LocalCancelReference, MachineSenderBinding, MailDeliveryRuntime};
#[cfg(test)]
mod native_control_failure_tests;

#[cfg(test)]
mod mail_delivery_projection_tests;

#[cfg(test)]
mod native_recorded_permission_tests;
#[cfg(test)]
mod native_result_tests;
