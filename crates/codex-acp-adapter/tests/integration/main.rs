//! Original test roots retain their case namespaces.

#[path = "../../src/test_support/conversation_operation_recorder.rs"]
mod conversation_operation_recorder;
#[path = "../../src/test_support/native_permission_echo.rs"]
mod native_permission_echo;
#[path = "../../src/test_support/recording_scratch_scopes.rs"]
mod recording_scratch_scopes;

mod acp_connection_dispatch;
mod acp_connection_transport;
mod acp_frame_transport;
mod acp_server_request_routing;
mod assistant_text_projection;
mod cancellation_reload;
mod connection_negotiation;
mod history_projection;
mod mcp_configuration;
mod native_prompt_execution;
mod native_prompt_terminal;
mod permission_translation;
mod prompt_content_translation;
mod prompt_detach;
mod prompt_settlement;
mod session_creation;
mod stable_protocol_schema;
mod stored_session_listing;
mod tool_progress_projection;
