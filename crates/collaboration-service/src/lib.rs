//! Collaboration application operations and publication policy, independent of the Host executable.
pub mod collaboration_application;
pub use collaboration_application::{CollaborationApplication, ResultByteBudget};
mod board_views;
mod notification_publisher;
mod project_admission;
pub use notification_publisher::NotificationPublisher;
mod project_commit_publication;
#[cfg(feature = "runtime-test-support")]
pub use project_admission::ProjectAdmissionTestContext;
pub use project_admission::{
    IntentHandoffInput, IntentRecipientEnrollment, IntentReply, ProjectAdmission,
    ProjectAdmissionError, ProjectAdmissionProps, ProjectArchiveObservation,
};
pub use project_commit_publication::after_project_commit_publish_and_reconcile;
mod router_identity;
pub use router_identity::{RouterIdentity, RouterIdentityError};
#[cfg(any(test, feature = "runtime-test-support"))]
mod application_fixture;
#[cfg(any(test, feature = "runtime-test-support"))]
pub use application_fixture::ApplicationFixture;
mod subscription_delivery;
pub use subscription_delivery::{
    BoardAvailability, SubscriptionClock, SubscriptionDeliveryService,
    SubscriptionDeliveryServiceProps, SubscriptionStartupRestoration, SubscriptionWaitFilter,
    SubscriptionWaitResult, SubscriptionWindowStore, SystemSubscriptionClock,
};
mod conversation_operation_projection;
pub use codex_acp_adapter::ConversationOperationRecorder;
pub use conversation_operation_projection::conversation_operation_snapshot;
#[cfg(test)]
mod wake_creation_crash_tests;
pub use collaboration_protocol::{DeliveryClientReceipt, DeliveryReceipt};
mod push_record_delivery;
mod push_record_resolver;
pub use codex_acp_adapter::BrokeredApprovalOutcome;
pub use codex_acp_adapter::{ACP_SCHEMA_DIGEST, NativeStoredSessions};
mod codex_session_inventory;
pub use codex_session_inventory::CodexSessionInventory;
mod instruction_projection;
mod mail_push_delivery;
mod mail_push_outcome;
mod native_claude_catalogue;
mod provider_session_inventory;
pub use provider_session_inventory::ProviderSessionInventory;
mod wakeup_projection;
pub use mail_push_delivery::{MailPushDelivery, MailPushDeliveryProps};
mod mail_delivery_runtime;
pub use mail_delivery_runtime::{LocalCancelReference, MailDeliveryRuntime};
mod attempt_history_projection;
mod automation_collection_cursor;
mod automation_collection_projection;
mod automation_event_cursor;
mod automation_event_projection;
mod automation_inspection_failure;
mod delivery_projection;
#[cfg(test)]
mod frozen_mail_query_fixture;
#[cfg(test)]
#[path = "../../collaboration-runtime/tests/support/native_worker_faults.rs"]
mod native_worker_faults;
mod operation_receipt_projection;
mod run_projection;
mod schedule_activation;
mod schedule_preparation;
mod schedule_projection;
#[cfg(test)]
mod test_project_runtime;
#[cfg(test)]
mod test_subject_workstream;
mod wakeup_subscription;

#[cfg(test)]
mod delivery_route_projection_tests;
#[cfg(test)]
mod native_control_failure_tests;

#[cfg(test)]
mod mail_delivery_projection_tests;

