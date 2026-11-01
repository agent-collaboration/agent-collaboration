//! The ACP client awaits its consumer's retention and activation decisions around initial settings.
use crate::{ProviderSessionMcp, ProviderSettingKind};
use agent_client_protocol::schema::v1::{McpServer, McpServerHttp};
use std::{future::Future, pin::Pin, sync::Arc};

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("provider session setup owner is unavailable")]
pub struct ProviderSessionSetupUnavailable;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProviderSessionSetupReadiness {
    Ready,
    Pending,
    Refused,
}

pub type ProviderSessionSetupFuture<'a, TOutcome> =
    Pin<Box<dyn Future<Output = Result<TOutcome, ProviderSessionSetupUnavailable>> + Send + 'a>>;

pub trait ProviderSessionSetupPort: Send + Sync {
    fn created<'a>(&'a self, provider_session_id: &'a str) -> ProviderSessionSetupFuture<'a, ()>;
    fn setup_passed<'a>(
        &'a self,
        provider_session_id: &'a str,
    ) -> ProviderSessionSetupFuture<'a, ProviderSessionSetupReadiness>;
    fn setup_refused<'a>(
        &'a self,
        provider_session_id: &'a str,
        setting: ProviderSettingKind,
    ) -> ProviderSessionSetupFuture<'a, ()>;
}

/// A managed create always carries its own HTTP binding and setup owner together.
/// It is never serialized or printed; restoring still takes ProviderSessionMcp.
#[derive(Clone)]
pub enum ProviderSessionCreateSetup {
    AccessEnrollment {
        mcp: ProviderSessionMcp,
    },
    Managed {
        server_name: String,
        server_url: String,
        port: Arc<dyn ProviderSessionSetupPort>,
    },
}
impl ProviderSessionCreateSetup {
    pub(crate) fn requires_http(&self) -> bool {
        match self {
            Self::AccessEnrollment { mcp } => mcp.requires_http(),
            Self::Managed { .. } => true,
        }
    }
    pub(crate) fn into_parts(
        self,
        defaults: &[McpServer],
    ) -> (Vec<McpServer>, Option<Arc<dyn ProviderSessionSetupPort>>) {
        match self {
            Self::AccessEnrollment { mcp } => (mcp.into_servers(defaults), None),
            Self::Managed {
                server_name,
                server_url,
                port,
            } => (
                vec![McpServer::Http(McpServerHttp::new(server_name, server_url))],
                Some(port),
            ),
        }
    }
}
