//! Consumer proofs over the actual Host composition, with weak test-only observers.

#[path = "startup_contract_tests/broker_context_tests.rs"]
mod broker_context_tests;
#[path = "startup_contract_tests/duplicate_host_test.rs"]
mod duplicate_host_test;
#[path = "startup_contract_tests/follow_attachment_tests.rs"]
mod follow_attachment_tests;
#[cfg(all(debug_assertions, feature = "runtime-test-support"))]
#[path = "startup_mail_retention_test.rs"]
mod mail_retention;
#[path = "startup_contract_tests/observation_recheck_tests.rs"]
mod observation_recheck_tests;
#[path = "../../tests/support/registered_route_project.rs"]
mod registered_route_project;
#[path = "../../tests/support/session_follow_fixture.rs"]
mod route_fixture;
#[path = "startup_contract_tests/scheduler_completion_tests.rs"]
mod scheduler_completion_tests;
#[path = "startup_contract_tests/status_cancellation_test.rs"]
mod status_cancellation_test;
#[path = "startup_contract_tests/wake_capacity_tests.rs"]
mod wake_capacity_tests;
