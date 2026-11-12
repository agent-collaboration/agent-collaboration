//! Integration cases retain one namespace per former Cargo test root.

#[path = "../support/acp_provider_backend.rs"]
mod acp_provider_backend;
#[path = "../support/native_delivery.rs"]
mod native_delivery;
use codex_acp_adapter::test_support::native_permission_echo;
#[path = "../support/served_api.rs"]
mod served_api;

mod acp_listener_admission;
mod acp_listener_socket_cleanup;
mod acp_provider_interactions;
mod app_server_client_rejection;
mod app_server_provider_interactions;
mod codex_conversation_operation_inspection;
mod codex_empty_thread_recovery;
mod codex_session_list_shape;
mod endpoint_address_isolation;
mod endpoint_publication;
mod interaction_schema_ownership;
mod journal_rpc;
mod local_listener;
mod native_channel_relay;
mod native_claude_catalogue;
mod native_control_calls;
mod native_empty_session_inventory;
mod native_generation_admission;
mod native_listener_recovery;
mod native_relay_handshake;
mod provider_conversation_dispatch;
mod provider_operation_store;
mod provider_session_control;
mod provider_session_event_hub;
mod provider_session_observation_control;
mod provider_session_records;
mod public_observation_recovery;
mod public_process_recovery;
mod question_api_path;
mod service_identity_storage;
mod session_delivery_contract;
mod session_delivery_dependency_boundaries;
mod session_delivery_presence;
mod session_delivery_router;
mod stored_inventory_journal;
