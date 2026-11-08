//! Opt-in unscripted developer-experience evaluation for the board communication skill.
#[path = "board_skill_dx_support/mod.rs"]
mod board_skill_dx_support;

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
#[ignore = "requires an explicitly launched isolated debug Host and two fresh Luna skill sessions"]
async fn luna_agents_use_board_skill_without_supplied_commands_or_resource_ids() -> ProofResult<()>
{
    board_skill_dx_support::exercise().await
}
