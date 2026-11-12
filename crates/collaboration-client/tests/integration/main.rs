//! Original test roots retain their case namespaces.

#[expect(
    dead_code,
    reason = "shared fixture module serves separate integration targets with different helper subsets"
)]
#[path = "../support/scripted_api.rs"]
mod scripted_api;

mod acp_aggregate_prompt_outputs;
#[cfg(test)]
mod acp_aggregate_prompt_outputs_test;
mod acp_conversation_flow;
mod acp_permission_reporting;
mod acp_real_adapter_final_reply;
mod acp_rejection_lifecycle;
mod admission_overload;
mod api_rejection_decoding;
mod api_roundtrip;
mod native_endpoint_resolution;
mod native_transport_connection;
mod permission_diagnostic;
mod provider_wait_timeout;
mod service_directory_resolution;
mod session_history_special_files;
mod stored_repository_scope;
