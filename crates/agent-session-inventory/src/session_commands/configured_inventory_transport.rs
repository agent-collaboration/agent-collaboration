//! Read the existing stateless v3 JSON MCP API; no session, credential or recovery owner.
use super::picker_runtime_inventory::native_inventory_pager::NativeInventoryReadClient;
use super::router_connection_registry::{RouterConnectionProfile, RouterConnectionTransport};
use super::source_inventory_failure::InventoryReadFailure;
use super::source_ssh_protocol::{SourceOperation, SourceReply};
use crate::picker_runtime_status::NativeLiveUnavailableReason;
use collaboration_client::{
    ClientError,
    protocol::{
        EndpointInventory, NativeSessionListParams, NativeSessionListResult,
        ProviderSessionListParams, ProviderSessionListResult,
    },
};
use rmcp::model::{
    CallToolRequest, CallToolRequestParams, ClientJsonRpcMessage, ClientRequest, NumberOrString,
    ServerJsonRpcMessage, ServerResult,
};
use serde::{Serialize, de::DeserializeOwned};
use std::time::Duration;

const READ_TIMEOUT: Duration = Duration::from_secs(30);
// Match the existing API's bounded envelope budget, without buffering an unbounded reply.
const MAX_READ_REPLY_BYTES: usize = 4 * collaboration_client::protocol::MAX_CONTROL_FRAME_BYTES;

pub struct HttpConfiguredInventoryClient {
    client: reqwest::Client,
    url: String,
}

impl HttpConfiguredInventoryClient {
    pub fn new(profile: &RouterConnectionProfile) -> Result<Self, ClientError> {
        Self::build(
            profile,
            #[cfg(test)]
            None,
        )
    }

    #[cfg(test)]
    // Standalone package tests use WebPKI roots; fixture trust stays out of non-test builds.
    fn with_test_root(
        profile: &RouterConnectionProfile,
        certificate: reqwest::Certificate,
    ) -> Result<Self, ClientError> {
        Self::build(profile, Some(certificate))
    }

    fn build(
        profile: &RouterConnectionProfile,
        #[cfg(test)] test_root: Option<reqwest::Certificate>,
    ) -> Result<Self, ClientError> {
        // Credential presentation is operator-owned; never guess a header or read a secret.
        let RouterConnectionTransport::Remote {
            mcp_address,
            credential,
            ..
        } = &profile.transport
        else {
            return Err(ClientError::InvalidRequest(
                "HTTP source connection required",
            ));
        };
        if credential.is_some() {
            return Err(ClientError::InvalidRequest(
                "configured read credential presentation is unqualified",
            ));
        }
        let address = url::Url::parse(mcp_address.as_str())
            .map_err(|_| ClientError::InvalidRequest("invalid configured source address"))?;
        let mut client_builder = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(READ_TIMEOUT);
        match address.scheme() {
            // This private client only posts to its fixed URL and never follows redirects.
            // Plain HTTP cannot use TLS roots; avoid native certificate-store IO here.
            "http" => client_builder = client_builder.tls_built_in_root_certs(false),
            "https" => {}
            _ => {
                return Err(ClientError::InvalidRequest(
                    "invalid configured source scheme",
                ));
            }
        }
        #[cfg(test)]
        if let Some(certificate) = test_root {
            client_builder = client_builder.add_root_certificate(certificate);
        }
        let client = client_builder
            .build()
            .map_err(|_| ClientError::Protocol("configured read transport unavailable"))?;
        Ok(Self {
            client,
            url: mcp_address.as_str().to_owned(),
        })
    }

    pub async fn inventory(&self) -> Result<EndpointInventory, ClientError> {
        self.call("endpoints_list", &serde_json::json!({})).await
    }

    pub async fn provider_page(
        &self,
        request: &ProviderSessionListParams,
    ) -> Result<ProviderSessionListResult, InventoryReadFailure> {
        self.read_call("provider_sessions_list", request).await
    }

    async fn call<TResult: DeserializeOwned>(
        &self,
        tool: &str,
        arguments: &impl Serialize,
    ) -> Result<TResult, ClientError> {
        self.read_call(tool, arguments)
            .await
            .map_err(InventoryReadFailure::into_client)
    }

    async fn read_call<TResult: DeserializeOwned>(
        &self,
        tool: &str,
        arguments: &impl Serialize,
    ) -> Result<TResult, InventoryReadFailure> {
        let invalid = || InventoryReadFailure::Invalid { code: None };
        let arguments = serde_json::to_value(arguments).map_err(|_| invalid())?;
        let serde_json::Value::Object(arguments) = arguments else {
            return Err(invalid());
        };
        let id = NumberOrString::Number(1);
        let request = ClientJsonRpcMessage::request(
            ClientRequest::CallToolRequest(CallToolRequest::new(
                CallToolRequestParams::new(tool.to_owned()).with_arguments(arguments),
            )),
            id.clone(),
        );
        let mut response = self
            .client
            .post(&self.url)
            .header(
                reqwest::header::ACCEPT,
                "application/json, text/event-stream",
            )
            .header("mcp-protocol-version", "2025-11-25")
            .json(&request)
            .send()
            .await
            .map_err(|error| {
                InventoryReadFailure::Unavailable(if error.is_timeout() {
                    NativeLiveUnavailableReason::TimedOut
                } else {
                    NativeLiveUnavailableReason::TransportUnavailable
                })
            })?;
        if response.status().is_server_error() {
            return Err(InventoryReadFailure::Unavailable(
                NativeLiveUnavailableReason::TransportUnavailable,
            ));
        }
        if response.status() != reqwest::StatusCode::OK {
            return Err(InventoryReadFailure::Invalid {
                code: Some(i64::from(response.status().as_u16())),
            });
        }
        if !response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value
                    .split(';')
                    .next()
                    .is_some_and(|media| media.trim().eq_ignore_ascii_case("application/json"))
            })
        {
            return Err(invalid());
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|error| {
            InventoryReadFailure::Unavailable(if error.is_timeout() {
                NativeLiveUnavailableReason::TimedOut
            } else {
                NativeLiveUnavailableReason::TransportUnavailable
            })
        })? {
            if chunk.len() > MAX_READ_REPLY_BYTES.saturating_sub(bytes.len()) {
                return Err(invalid());
            }
            bytes.extend_from_slice(&chunk);
        }
        let message: ServerJsonRpcMessage =
            serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        let value = match message {
            ServerJsonRpcMessage::Response(response) if response.id == id => {
                let ServerResult::CallToolResult(result) = response.result else {
                    return Err(invalid());
                };
                if result.is_error == Some(true) {
                    return Err(result
                        .structured_content
                        .map(InventoryReadFailure::tool_error)
                        .unwrap_or_else(invalid));
                }
                result.structured_content.ok_or_else(invalid)?
            }
            ServerJsonRpcMessage::Error(error) if error.id.as_ref() == Some(&id) => {
                return Err(InventoryReadFailure::published(
                    i64::from(error.error.code.0),
                    error.error.data.as_ref(),
                ));
            }
            _ => return Err(invalid()),
        };
        serde_json::from_value(value).map_err(|_| invalid())
    }
}

impl NativeInventoryReadClient for HttpConfiguredInventoryClient {
    async fn read_native_page(
        &mut self,
        request: NativeSessionListParams,
    ) -> Result<NativeSessionListResult, ClientError> {
        self.call("sessions_list", &request).await
    }
}

pub enum ConfiguredInventoryClient {
    Remote(HttpConfiguredInventoryClient),
    Ssh(Box<RouterConnectionProfile>),
}

impl ConfiguredInventoryClient {
    pub fn new(profile: &RouterConnectionProfile) -> Result<Self, ClientError> {
        match &profile.transport {
            RouterConnectionTransport::Remote { .. } => {
                Ok(Self::Remote(HttpConfiguredInventoryClient::new(profile)?))
            }
            RouterConnectionTransport::Ssh(_) => Ok(Self::Ssh(Box::new(profile.clone()))),
        }
    }

    pub async fn inventory(&self) -> Result<EndpointInventory, ClientError> {
        match self {
            Self::Remote(client) => client.inventory().await,
            Self::Ssh(profile) => match ssh_read(profile, SourceOperation::Endpoints).await? {
                SourceReply::Endpoints { inventory } => Ok(inventory),
                _ => Err(ClientError::Protocol(
                    "source reply kind does not match inventory",
                )),
            },
        }
    }

    pub async fn provider_page(
        &self,
        request: &ProviderSessionListParams,
    ) -> Result<ProviderSessionListResult, InventoryReadFailure> {
        match self {
            Self::Remote(client) => client.provider_page(request).await,
            Self::Ssh(profile) => match ssh_read(
                profile,
                SourceOperation::ProviderSessions {
                    params: request.clone(),
                },
            )
            .await
            .map_err(|error| InventoryReadFailure::from_client(&error))?
            {
                SourceReply::ProviderSessions { page } => Ok(page),
                _ => Err(InventoryReadFailure::Invalid { code: None }),
            },
        }
    }
}

impl NativeInventoryReadClient for ConfiguredInventoryClient {
    async fn read_native_page(
        &mut self,
        request: NativeSessionListParams,
    ) -> Result<NativeSessionListResult, ClientError> {
        match self {
            Self::Remote(client) => client.read_native_page(request).await,
            Self::Ssh(profile) => {
                match ssh_read(profile, SourceOperation::NativeSessions { params: request }).await?
                {
                    SourceReply::NativeSessions { page } => Ok(page),
                    _ => Err(ClientError::Protocol(
                        "source reply kind does not match native page",
                    )),
                }
            }
        }
    }
}

async fn ssh_read(
    profile: &RouterConnectionProfile,
    operation: SourceOperation,
) -> Result<SourceReply, ClientError> {
    super::source_ssh_transport::read_source(profile, operation)
        .await
        .map_err(source_read_client_error)
}

fn source_read_client_error(error: super::source_ssh_protocol::SourceReadError) -> ClientError {
    use super::source_ssh_protocol::{SourceReadError, SourceRejection};
    match error {
        SourceReadError::Rejected(SourceRejection::InvalidNativeInventory {
            inventory_source,
            reason,
        }) => InventoryReadFailure::InvalidNative {
            inventory_source,
            reason,
        }
        .into_client(),
        SourceReadError::ReadUnavailable
        | SourceReadError::Rejected(SourceRejection::SourceUnavailable) => {
            ClientError::Transport(std::io::Error::new(
                std::io::ErrorKind::ConnectionAborted,
                "configured SSH read unavailable",
            ))
        }
        SourceReadError::InvalidReply
        | SourceReadError::Rejected(_)
        | SourceReadError::UnguardedNativeResume => {
            ClientError::Protocol("configured SSH read failed")
        }
    }
}

#[cfg(test)]
#[path = "configured_inventory_transport_tests.rs"]
mod tests;
