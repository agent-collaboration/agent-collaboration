//! Public initialization entry points for one ACP provider connection.

#[cfg(any(test, feature = "test-observation"))]
use super::Duration;
use super::{
    AgentSessionClient, Arc, ExternalProviderLaunch, ExternalProviderRuntimeError,
    INITIALIZE_TIMEOUT, InteractionPort, McpServer, McpServerHttp, SessionEventSink,
};

impl<P: InteractionPort> AgentSessionClient<P> {
    /// Native model-setting proof uses the production transport and picker
    /// negotiation without injecting an unrelated collaboration MCP server.
    #[cfg(feature = "test-observation")]
    pub async fn initialize_with_model_picker(
        launch: ExternalProviderLaunch,
        model_picker: crate::ProviderModelPicker,
        interaction_port: Arc<P>,
        event_sink: Arc<dyn SessionEventSink>,
        behavior: Arc<dyn crate::ProviderBehavior>,
    ) -> Result<Self, ExternalProviderRuntimeError> {
        Self::initialize_with_timeout_and_mcp_servers(
            launch,
            model_picker,
            INITIALIZE_TIMEOUT,
            Vec::new(),
            interaction_port,
            event_sink,
            behavior,
            Arc::new(router_clock::SystemRuntimeClock::new()),
        )
        .await
    }

    pub async fn initialize(
        launch: ExternalProviderLaunch,
        interaction_port: Arc<P>,
        event_sink: Arc<dyn SessionEventSink>,
        behavior: Arc<dyn crate::ProviderBehavior>,
    ) -> Result<Self, ExternalProviderRuntimeError> {
        Self::initialize_with_timeout_and_mcp_servers(
            launch,
            crate::ProviderModelPicker::Standard,
            INITIALIZE_TIMEOUT,
            Vec::new(),
            interaction_port,
            event_sink,
            behavior,
            Arc::new(router_clock::SystemRuntimeClock::new()),
        )
        .await
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "The ACP connection's inputs include the Host clock."
    )]
    pub async fn initialize_with_mcp_http(
        launch: ExternalProviderLaunch,
        model_picker: crate::ProviderModelPicker,
        server_name: impl Into<String>,
        server_url: impl Into<String>,
        interaction_port: Arc<P>,
        event_sink: Arc<dyn SessionEventSink>,
        behavior: Arc<dyn crate::ProviderBehavior>,
        clock: Arc<dyn router_clock::RuntimeClock>,
    ) -> Result<Self, ExternalProviderRuntimeError> {
        Self::initialize_with_timeout_and_mcp_servers(
            launch,
            model_picker,
            INITIALIZE_TIMEOUT,
            vec![McpServer::Http(McpServerHttp::new(server_name, server_url))],
            interaction_port,
            event_sink,
            behavior,
            clock,
        )
        .await
    }

    #[cfg(any(test, feature = "test-observation"))]
    pub async fn initialize_with_timeout(
        launch: ExternalProviderLaunch,
        initialize_timeout: Duration,
        interaction_port: Arc<P>,
        event_sink: Arc<dyn SessionEventSink>,
        behavior: Arc<dyn crate::ProviderBehavior>,
    ) -> Result<Self, ExternalProviderRuntimeError> {
        Self::initialize_with_timeout_and_mcp_servers(
            launch,
            crate::ProviderModelPicker::Standard,
            initialize_timeout,
            Vec::new(),
            interaction_port,
            event_sink,
            behavior,
            Arc::new(router_clock::SystemRuntimeClock::new()),
        )
        .await
    }
}

impl<P: InteractionPort> AgentSessionClient<P> {
    /// A connection to an in-process agent, for scenario tests (design note
    /// §11.8): the same connection as `initialize`, over the client's end of
    /// a duplex pair, with `agent` serving the other end, instead of a
    /// process's stdio. Test support only.
    #[cfg(feature = "test-support")]
    pub async fn initialize_in_process(
        connection: tokio::io::DuplexStream,
        agent: tokio::task::JoinHandle<()>,
        interaction_port: Arc<P>,
        event_sink: Arc<dyn SessionEventSink>,
        behavior: Arc<dyn crate::ProviderBehavior>,
        clock: Arc<dyn router_clock::RuntimeClock>,
    ) -> Result<Self, ExternalProviderRuntimeError> {
        Self::initialize_over_transport(
            super::provider_transport::ProviderTransport::in_process(connection, agent),
            crate::ProviderPersistenceTarget::Unspecified,
            crate::ProviderModelPicker::Standard,
            INITIALIZE_TIMEOUT,
            Vec::new(),
            interaction_port,
            event_sink,
            behavior,
            clock,
        )
        .await
    }
}
