//! Original test roots retain their case namespaces.

#[path = "../support/board_cli_execution.rs"]
mod board_cli_execution;
#[path = "../../../collaboration-service/tests/support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "The external service fixture also serves capabilities unused by CLI cases."
)]
mod board_fixture;
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
#[path = "../delivery_matrix/subscription_notice_observer.rs"]
mod subscription_notice_observer;
use board_fixture::native_mail_owner;

mod acp_bridge_command;
mod admission_overload_cli;
mod approval_list_cli;
mod automation_argument_errors;
mod automation_configuration_cli;
mod board_aggregate_cli;
mod board_cli;
mod board_debug_acceptance;
mod board_fleet_lifecycle_cli;
mod board_model_cli;
mod board_request_effects_cli;
mod board_skill_dx;
mod board_skill_dx_recovery;
mod board_task_corrections_cli;
mod board_views_cli;
mod board_write_uncertainty;
mod central_cli;
mod claude_background_cli;
mod conversation_carriers;
mod conversation_fault_entry_paths;
mod debug_luna_acceptance;
mod delivery_matrix_debug_acceptance;
mod endpoint_discovery_command;
mod event_observation_commands;
mod executable_contract;
mod human_approver_cli;
mod instruction_cli_path;
mod journal_read_commands;
mod local_mail_cli_mcp;
mod local_mail_failures_cli_mcp;
mod m2b_repository_subjects;
mod m2b_surface_parity;
mod message_argument_feedback;
mod messaging_skill_dx;
mod native_bridge_command;
mod native_control_commands;
mod native_push_correlation;
mod native_sandbox_access;
mod permission_diagnostics;
mod project_creation_reply_cli;
mod project_description_cli;
mod provider_conversation_cli;
mod provider_inventory_source_reads;
mod provider_load_error_e2e;
mod provider_session_list_cli;
mod provider_source_actions;
mod public_push_receipt;
mod push_delivery_matrix;
mod push_message_cli;
mod remote_mail_cli_mcp;
mod run_cli_path;
mod schedule_cli_path;
mod served_command_groups;
mod skill_cli_contract;
mod skipped_wakeup_cli;
mod subscription_duration_error_cli;
mod summary_skip_cli;
mod thread_subscription_cli;
mod wake_rules_cli_mcp;
mod wakeup_cli_path;
mod whoami_machine_identity;
