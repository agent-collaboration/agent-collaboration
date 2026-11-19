//! L3 normal-prefix and unavailable-socket cases through the real local Runtime and SQLite.

#[path = "owned_resource_cleanup_producer_report.rs"]
mod producer_report;

#[path = "owned_resource_cleanup_runtime_fixture.rs"]
mod runtime_fixture;

#[path = "owned_resource_cleanup_producer_runtime_tests.rs"]
mod producer_runtime_tests;
#[path = "owned_resource_cleanup_state_checks.rs"]
mod state_checks;
