//! One Turso transaction owner for typed interactions and local message delivery.
mod interaction_store;
pub use interaction_store::{InteractionStore, StorageError};
mod interaction_atomic_store;
pub use collaboration_protocol::{PushDeliveryState, PushRecord, PushRecordDraft};
pub use interaction_atomic_store::InteractionAtomicStore;
mod interaction_history;
pub use interaction_history::{
    InteractionHistoryError, InteractionHistoryRecord, InteractionHistoryState,
    InteractionHistoryStore, LegacyApprovalMetadata, QuestionHistoryState, QuestionResponse,
    RefusedApprovalOption, RefusedTypedApproval,
};
mod board_intent_progress;
mod interaction_notice_pushes;
mod local_automation_pushes;
mod local_cancel_deliveries;
mod local_delivery_retries;
mod mail_delivery_observations;
pub use local_cancel_deliveries::{
    LocalCancelDelivery, LocalCancelPreparation, delete_tentative_local_cancel_in_scope,
    prepare_local_cancel_in_scope, read_local_cancel_in_scope,
    record_local_cancel_outcome_in_scope,
};
pub use local_delivery_retries::{
    clear_local_delivery_retry_in_scope, record_local_delivery_retry_in_scope,
};
pub use mail_delivery_observations::{
    MailDeliveryObservation, record_mail_route_evidence_in_scope,
};
mod cancelled_push_projection;
pub use cancelled_push_projection::record_cancelled_after_delivery_in_scope;
mod mail_push_projection;
pub use mail_push_projection::{
    MailPushUpdate, apply_mail_push_outcome_in_scope, insert_mail_push_in_scope,
    mark_mail_push_attempted_in_scope, mark_released_mail_pending_in_scope,
    read_mail_push_in_scope,
};
mod installation_peer_directory;
mod mail_retirement_progress;
mod pending_poll_mail;
pub use pending_poll_mail::{
    PendingPollMail, delete_pending_poll_in_scope, insert_pending_poll_in_scope,
    read_pending_poll_in_scope,
};
mod mail_read_contexts;
mod mail_submission_epochs;
pub use mail_read_contexts::{StoredMailReadContext, record_mail_read_context_in_scope};
mod message_send_answers;
pub use installation_peer_directory::InstallationPeerBinding;
pub use mail_submission_epochs::{
    MailSubmissionEpoch, read_mail_submission_epoch_in_scope, record_mail_submission_epoch_in_scope,
};
pub use message_send_answers::{
    MessageSendAnswer, read_message_send_answer_in_scope, record_message_send_admission,
    record_message_send_outcome,
};
mod carrier_push_recovery;
mod carrier_push_transitions;
mod held_mail;
mod owed_wake_admission;
mod push_record_rows;
mod push_records;
mod push_retention;
mod subscription_delivery_progress;
mod worker_progress_id;
pub use push_records::{
    DirectMessageHistoryQuery, MAX_PUSH_LIST_LIMIT, MAX_PUSH_PRUNE_BATCH, PushInboxQuery,
};
pub use subscription_delivery_progress::{
    SubscriptionDeliveryProgress, SubscriptionDeliveryProgressProps, SubscriptionRootProgress,
    SubscriptionWindowProgress, SubscriptionWindowSelection,
};

#[cfg(test)]
mod interaction_atomic_store_tests;
#[cfg(test)]
mod schema_prefix_fixture;
