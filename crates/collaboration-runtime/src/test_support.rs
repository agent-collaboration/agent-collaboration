//! Test-only hooks; the normal Host and CLI dependency graphs do not enable these.
pub use crate::scheduled_route_fixture;
pub use crate::scheduled_run_worker::ScheduledRunWorker;
pub use crate::wakeup_delivery_sender::WakeDeliverySender;
/// Test-only wake draft conversion at the durable evaluation boundary.
pub fn build_wake_push_draft(
    wake: &agent_automation::WakeRecord<
        collaboration_protocol::SavedMessage,
        collaboration_protocol::WakeCreator,
    >,
    fire: &agent_automation::FirstFire,
) -> Result<collaboration_protocol::PushRecordDraft, automation_storage::StorageError> {
    crate::wakeup_delivery_sender::build_wake_push_draft(wake, fire)
}

/// Test-only native boundary inputs and observations.
pub mod native_message_dispatch {
    pub use crate::native_message_dispatch::{
        NativeMessageBody, NativeMessageOutcome, NativeMessageParams, NativeMessageRequest,
        dispatch_message,
    };
}

/// Test-only native boundary inputs and observations.
pub mod message_effect_state {
    pub use crate::message_effect_state::{
        MessageEffects, NativeFailureDisposition, NativeFailureKind, NativeMessageAttemptEvidence,
        NativeMessageFailure,
    };
}

/// Test-only native boundary inputs and observations.
pub mod native_recorded_policy {
    pub use crate::native_recorded_policy::{
        RecordedNativeRouteClassification, classify_recorded_native_route,
        prepare_classified_native_policy,
    };
}

/// Test-only native boundary inputs and observations.
pub mod codex_queue_reconciliation {
    pub use crate::codex_queue_reconciliation::reconcile;
}

/// Test-only native boundary inputs and observations.
pub mod scheduled_run_evidence_sink {
    pub use crate::scheduled_run_evidence_sink::StoredRunEvidenceSink;
}

pub use crate::codex_app_server_delivery_route::native_result::interpret_native_response;
