//! Retain the original root registrations while sharing each execution body once.

#[tokio::test]
async fn aggregate_existing_prompt_handles_missing_reply_and_history_setup_failure() {
    crate::acp_aggregate_prompt_outputs_test::aggregate_existing_prompt_handles_missing_reply_and_history_setup_failure().await;
}

#[tokio::test]
async fn aggregate_prompt_does_not_retain_more_than_one_thousand_updates() {
    crate::acp_aggregate_prompt_outputs_test::aggregate_prompt_does_not_retain_more_than_one_thousand_updates().await;
}
