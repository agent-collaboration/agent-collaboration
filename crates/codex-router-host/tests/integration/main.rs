//! Original test roots retain their case namespaces.

use collaboration_application_test_support::board_fixture;
#[path = "../support/host_replacement_cleanup.rs"]
mod host_replacement_cleanup;
#[path = "../support/operator_client.rs"]
mod operator_client;
#[path = "../support/provider_acp_message_fifo_support.rs"]
mod provider_acp_message_fifo_support;
#[path = "../support/provider_conversation_submission.rs"]
mod provider_conversation_submission;
#[path = "../support/provider_fixture_launch.rs"]
mod provider_fixture_launch;
#[cfg(test)]
#[path = "../support/registered_route_project.rs"]
mod registered_route_project;

mod acp_client_dependency_structure;
mod acp_metadata_projection;
mod auth_file_reload;
mod automation_composition;
mod automation_configuration_rpc;
mod automation_retention_runtime;
mod automation_settings_recovery;
mod backend_publication;
mod board_failure_isolation;
mod claude_code_peer_delivery_route;
mod claude_code_peer_scheduled_runs;
mod claude_peer_held_dm;
mod claude_peer_subscription_resume;
mod collaboration_runtime;
mod configuration_effect_receipt;
mod external_provider_composition;
mod external_provider_recovery;
mod external_provider_supervisor;
mod fixed_mcp_bind;
mod global_harness_selection;
mod host_replacement_failures;
mod journal_failure_isolation;
mod local_mail_cancellation;
mod maintenance_authority;
mod message_upgrade_startup;
mod native_claude_catalogue;
mod operator_transport_contract;
mod process_lifecycle;
mod production_enrollment;
mod provider_acp_delivery_route;
mod provider_acp_loaded_only_races;
mod provider_acp_message_fifo;
mod provider_acp_queue_route;
mod provider_acp_scheduled_runs;
mod provider_acp_steering_route;
mod provider_content_commands;
mod provider_endpoint_routing_structure;
mod provider_face_composition;
mod provider_native_models;
mod provider_native_skills;
mod provider_pending_hub_races;
mod provider_startup_isolation;
mod remote_wake;
mod router_settings;
mod runtime_lifecycle;
mod runtime_restart;
mod session_message_route_composition;
mod shared_fleet_store;
mod singleton_authority;
mod stale_endpoint_sockets;
mod update_reexec;
mod wakeup_timer_composition;

/// Qualify a former libtest name with its retained Cargo-root namespace.
fn child_test_selector(caller_module: &str, case_name: &str) -> String {
    let relative = caller_module
        .split_once("::")
        .map_or(caller_module, |(_, path)| path);
    let root = relative.split_once("::").map_or(relative, |(root, _)| root);
    format!("{root}::{case_name}")
}

fn assert_one_child_test_passed(stdout: &[u8]) {
    let stdout = String::from_utf8_lossy(stdout);
    assert!(
        stdout.contains("test result: ok. 1 passed;"),
        "the child selector must complete exactly one case: {stdout}"
    );
}
