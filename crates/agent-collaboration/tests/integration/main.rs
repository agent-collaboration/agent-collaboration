//! Original test roots retain their case namespaces.

#[path = "../automation_live_support/debug_backend_restart.rs"]
mod debug_backend_restart;
#[path = "../delivery_matrix/support.rs"]
mod delivery_matrix_support;
#[path = "../support/local_mail_surfaces.rs"]
mod local_mail_surfaces;
#[path = "../delivery_matrix/native_push_correlation.rs"]
mod native_push_correlation_fixture;
#[path = "../support/local_mail_native_sql.rs"]
mod native_sql;
#[path = "../delivery_matrix/peer_fixture.rs"]
mod peer_fixture;
#[path = "../automation_live_support/proof_context.rs"]
mod proof_context;
#[path = "../delivery_matrix/public_push_receipt.rs"]
mod public_push_receipt_fixture;
use collaboration_application_test_support::native_mail_owner;

mod acp_bridge_command;
mod admission_overload_cli;
mod approval_list_cli;
mod automation_argument_errors;
mod claude_background_cli;
mod conversation_carriers;
mod conversation_fault_entry_paths;
mod debug_luna_acceptance;
mod delivery_matrix_debug_acceptance;
mod endpoint_discovery_command;
mod event_observation_commands;
mod executable_contract;
mod human_approver_cli;
mod journal_read_commands;
mod local_mail_cli_mcp;
mod local_mail_failures_cli_mcp;
mod message_argument_feedback;
mod messaging_skill_dx;
mod native_bridge_command;
mod native_control_commands;
mod native_push_correlation;
mod native_sandbox_access;
mod permission_diagnostics;
mod provider_conversation_cli;
mod provider_inventory_source_reads;
mod provider_load_error_e2e;
mod provider_session_list_cli;
mod provider_source_actions;
mod public_push_receipt;
mod push_delivery_matrix;
mod push_message_cli;
mod served_command_groups;
mod skill_cli_contract;
mod whoami_machine_identity;
