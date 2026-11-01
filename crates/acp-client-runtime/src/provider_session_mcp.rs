//! MCP selection belongs to a single create or restore command.
use agent_client_protocol::schema::v1::{McpServer, McpServerHttp};

/// The Host validates a bound address before passing it here. The HTTP variant
/// replaces the connection's collaboration entry; it never falls back to it.
/// Intentionally lacks Debug and Serialize because the URL can be a capability.
#[derive(Clone)]
pub enum ProviderSessionMcp {
    /// The Access enrollment path retained until F17.
    ConnectionDefaults,
    Http {
        server_name: String,
        server_url: String,
    },
}

impl ProviderSessionMcp {
    pub(crate) fn requires_http(&self) -> bool {
        matches!(self, Self::Http { .. })
    }

    pub(crate) fn into_servers(self, connection_defaults: &[McpServer]) -> Vec<McpServer> {
        match self {
            Self::ConnectionDefaults => connection_defaults.to_vec(),
            Self::Http {
                server_name,
                server_url,
            } => vec![McpServer::Http(McpServerHttp::new(server_name, server_url))],
        }
    }
}
