//! Test-only hooks; the normal Host and CLI dependency graphs do not enable these.
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
