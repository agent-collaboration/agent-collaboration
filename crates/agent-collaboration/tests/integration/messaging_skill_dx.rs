//! Two-phase, opt-in proof of goal-driven direct messaging through the repo skill and real CLI.
#[path = "messaging_skill_dx_support/mod.rs"]
mod messaging_skill_dx_support;

mod proof_context {
    pub(super) use crate::proof_context::*;
    mod native_notification_diagnostics {
        mod tests {
            #[test]
            fn retains_retry_class_without_provider_body_or_unrelated_thread_data() {
                crate::proof_context::native_notification_diagnostics::tests::retains_retry_class_without_provider_body_or_unrelated_thread_data()
            }

            #[test]
            fn unknown_provider_payload_is_not_copied_into_diagnostics() {
                crate::proof_context::native_notification_diagnostics::tests::unknown_provider_payload_is_not_copied_into_diagnostics()
            }

            #[test]
            fn underlying_transport_hints_do_not_retain_urls_or_error_bodies() {
                crate::proof_context::native_notification_diagnostics::tests::underlying_transport_hints_do_not_retain_urls_or_error_bodies()
            }
        }
    }
}

use proof_context::ProofResult;

#[tokio::test]
#[ignore = "phase one requires an explicitly launched isolated debug Host and one fresh Luna recipient"]
async fn prepare_unloaded_recipient_for_messaging_skill_dx() -> ProofResult<()> {
    messaging_skill_dx_support::prepare_recipient().await
}

#[tokio::test]
#[ignore = "phase two requires the same proof root after its owner restarts the isolated debug Host"]
async fn luna_sender_discovers_unloaded_recipient_and_completes_cli_round_trip() -> ProofResult<()>
{
    messaging_skill_dx_support::exercise_round_trip().await
}
