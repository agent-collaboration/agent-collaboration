//! Opt-in live board proof against an already running isolated debug Host.
#[path = "board_live_support/mod.rs"]
mod board_live_support;

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
#[ignore = "requires an explicitly launched isolated debug Host and two fresh Luna threads"]
async fn two_luna_agents_exchange_a_verified_finding_through_the_board_cli() -> ProofResult<()> {
    board_live_support::exercise().await
}

#[tokio::test]
#[ignore = "requires phase one plus an owner-restarted isolated debug Host at the same proof root"]
async fn board_state_survives_owned_debug_host_restart() -> ProofResult<()> {
    board_live_support::verify_persistence_after_host_restart().await
}
