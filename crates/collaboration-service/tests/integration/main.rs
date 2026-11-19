//! Integration cases retain one namespace per former Cargo test root.

#[path = "../support/acp_provider_backend.rs"]
mod acp_provider_backend;
use collaboration_application_test_support::board_fixture;
#[path = "../support/native_delivery.rs"]
mod native_delivery;
use codex_acp_adapter::test_support::native_permission_echo;
#[path = "../support/read_cost_counter.rs"]
mod read_cost_counter;
#[path = "../support/served_api.rs"]
mod served_api;
#[path = "../support/wake_push_draft.rs"]
mod wake_push_test_support;

use board_fixture::native_mail_owner;
use read_cost_counter::sql_cost_classification;

mod acp_listener_admission;
mod acp_listener_socket_cleanup;
mod acp_provider_interactions;
mod app_server_client_rejection;
mod app_server_provider_interactions;
mod automation_collection_path;
mod codex_app_server_delivery_route;
mod codex_app_server_held_thread_route;
mod codex_app_server_wake_push_route;
mod codex_conversation_operation_inspection;
mod codex_detached_create_delivery;
mod codex_empty_thread_recovery;
mod codex_scheduled_existing_targets;
mod codex_session_list_shape;
mod collaboration_application_boundaries;
mod delivery_inspection;
mod endpoint_address_isolation;
mod endpoint_publication;
mod historical_delivery_receipts;
mod instruction_service_path;
mod interaction_schema_ownership;
mod journal_rpc;
mod local_listener;
mod message_admission;
mod message_delivery_paths;
mod native_channel_relay;
mod native_claude_catalogue;
mod native_control_calls;
mod native_empty_session_inventory;
mod native_generation_admission;
mod native_inspect_permission_mode;
mod native_listener_recovery;
mod native_relay_handshake;
mod package_roundtrip_boundary;
mod project_registry;
mod project_store_registry;
mod provider_conversation_dispatch;
mod provider_operation_store;
mod provider_session_control;
mod provider_session_event_hub;
mod provider_session_observation_control;
mod provider_session_records;
mod public_observation_recovery;
mod public_process_recovery;
mod push_surface;
mod question_api_path;
mod read_cost_counter_contracts;
mod reader_state_corrections;
mod run_reconciliation_path;
mod schedule_activation_gate;
mod schedule_native_preparation;
mod schedule_package_path;
mod schedule_service_path;
mod scheduled_run_path;
mod service_identity_storage;
mod session_delivery_contract;
mod session_delivery_dependency_boundaries;
mod session_delivery_presence;
mod session_delivery_router;
mod skipped_wakeup_wait;
mod stored_inventory_journal;
mod wakeup_cursor_contract;
mod wakeup_first_fire;
mod wakeup_native_delivery;
mod wakeup_repeating_delivery;
mod wakeup_service_path;

mod machine_sender_binding;
