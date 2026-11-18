//! Real native actor and library retry boundaries with explicit external seams.
#[path = "mail_delivery_runner_probe.rs"]
mod runner_probe;

#[path = "mail_delivery_runtime_retention_test.rs"]
mod custody_retention;
#[path = "mail_push_direct_delivery_tests.rs"]
mod direct;
#[path = "local_mail_runtime_tests.rs"]
mod local_runtime;
#[path = "mail_materialized_session_test.rs"]
mod materialized_session;
#[path = "remote_mail_assembly_tests.rs"]
mod remote_assembly;
#[path = "remote_wake_wait_tests.rs"]
mod remote_wake_wait;
#[path = "remote_wake_replay_tests.rs"]
mod remote_wakes;
#[path = "mail_removal_backfill_test.rs"]
mod removal_backfill;
#[path = "mail_submitter_backfill_test.rs"]
mod submitter_backfill;

#[tokio::test]
async fn preceding_composed_schema_migrates_and_mismatched_retry_ledger_refuses() {
    observation_schema::assert_historical_prefixes_and_exact_ledger().await;
}

#[path = "mail_observation_schema_tests.rs"]
mod observation_schema;
