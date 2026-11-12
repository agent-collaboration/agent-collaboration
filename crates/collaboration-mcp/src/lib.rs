//! The collaboration API: one stateless MCP server over the typed collaboration application,
//! served on every listener the Host holds.

mod collaboration_api_router;
mod local_collaboration;
mod loopback_bind_address;
mod mcp_server;
mod native_schema_definitions;
mod router_executable_check;
mod tool_call_registry;

pub use collaboration_api_router::{
    COLLABORATION_API_PATH, CollaborationApiConfig, CollaborationApiListener,
    CollaborationApiRouter, DEFAULT_CONCURRENT_REQUESTS, collaboration_api_router,
    serve_collaboration_api,
};
pub use loopback_bind_address::{LoopbackBindAddress, LoopbackBindAddressError};
pub use native_schema_definitions::NativeSchemaDefinitions;
pub use router_executable_check::{FixedRouterExecutableRelation, RouterExecutableCheck};
#[cfg(feature = "test-support")]
pub mod test_support;

#[cfg(test)]
mod api_test_harness;
#[cfg(test)]
mod carrier_response_loss_http_tests;
#[cfg(test)]
mod codex_conversation_cancel_http_tests;
#[cfg(test)]
mod collaboration_api_router_tests;
#[cfg(test)]
mod mcp_remediation_tests;
#[cfg(test)]
mod observation_streaming_tests;
#[cfg(test)]
mod permission_entry_path_tests;
#[cfg(test)]
mod provider_conversation_http_tests;
#[cfg(test)]
mod provider_conversation_live_http_tests;
#[cfg(test)]
mod provider_stdio_diagnostic_http_tests;
mod proxy_authority;
#[cfg(test)]
mod proxy_authority_tests;
#[cfg(test)]
mod unix_origin_tests;
pub use proxy_authority::ProxyAuthority;
