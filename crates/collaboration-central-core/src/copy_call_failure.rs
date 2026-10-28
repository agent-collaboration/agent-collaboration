//! Only the closed no-effect Copy carrier can become a retryable session answer.
use durable_steps::CallFailure;
use fleet_sync_protocol::{
    CopyUnavailableReason, DiagnosticText, OutcomeUnknownReason, ReplicationAnswer,
};

pub(crate) fn unavailable(failure: &CallFailure) -> Option<CopyUnavailableReason> {
    match failure {
        CallFailure::Retryable { reason } => {
            CopyUnavailableReason::decode_retry(&reason.code, &reason.detail).ok()
        }
        _ => None,
    }
}
pub(crate) fn answer(failure: CallFailure) -> ReplicationAnswer {
    match unavailable(&failure) {
        Some(reason) => ReplicationAnswer::CopyUnavailable { reason },
        None => ReplicationAnswer::OutcomeUnknown {
            reason: OutcomeUnknownReason::StoreBoundary {
                detail: DiagnosticText::from_error("copy object reply unavailable"),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_validated_copy_carriers_preserve_no_effect_certainty() {
        for reason in [
            CopyUnavailableReason::Uninitialized,
            CopyUnavailableReason::StorageFull,
            CopyUnavailableReason::MigrationPending { version: 3 },
            CopyUnavailableReason::Damaged {
                detail: "copy unavailable".into(),
            },
        ] {
            assert_eq!(
                answer(fleet_sync_receiver::retryable_copy(reason.clone())),
                ReplicationAnswer::CopyUnavailable { reason }
            );
        }
        for (code, detail) in [
            ("copy_storage_full", "unexpected"),
            ("copy_migration_pending", "03"),
            ("future", ""),
            ("copy_migration_pending", "not-version"),
        ] {
            assert!(matches!(
                answer(CallFailure::Retryable {
                    reason: durable_steps::CallRetryReason {
                        code: code.into(),
                        detail: detail.into()
                    }
                }),
                ReplicationAnswer::OutcomeUnknown { .. }
            ));
        }
        assert!(matches!(
            answer(CallFailure::AnswerLost),
            ReplicationAnswer::OutcomeUnknown { .. }
        ));
    }
}
