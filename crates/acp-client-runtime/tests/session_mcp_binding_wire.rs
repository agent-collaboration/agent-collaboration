//! Per-command MCP addresses reach the real ACP stdio transport without a connection-wide cache.
use acp_client_runtime::{
    AgentSessionClient, ApprovalPortOutcome, EventSinkClosed, ExternalProviderLaunch,
    ExternalProviderRuntimeError, HistoryReplayFuture, InteractionFuture, InteractionPort,
    ProviderModelPicker, ProviderPersistenceTarget, ProviderSessionMcp, RefusedApprovalOffer,
    RequestedProviderSettings, SessionEventSink,
};
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};
use tokio_util::sync::CancellationToken;

const FIRST_ADDRESS: &str = "http://127.0.0.1:43210/mcp/session/11111111111111111111111111111111";
const SECOND_ADDRESS: &str = "http://127.0.0.1:43210/mcp/session/22222222222222222222222222222222";
const CONNECTION_ADDRESS: &str = "http://127.0.0.1:43210/mcp";
const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../verification/fixtures/provider-session-mcp-agent.ts"
);

struct NoInteraction;
impl InteractionPort for NoInteraction {
    type Context = ();
    type OperationId = u64;
    fn operation_id(_context: &()) -> u64 {
        1
    }
    fn binding_retirement(_context: &()) -> CancellationToken {
        CancellationToken::new()
    }
    fn request_approval(
        &self,
        _context: (),
        _request: session_event_model::ApprovalRequest,
        _turn: CancellationToken,
        _agent: CancellationToken,
    ) -> InteractionFuture<'_, ApprovalPortOutcome> {
        Box::pin(async { ApprovalPortOutcome::Cancelled })
    }
    fn request_question(
        &self,
        _context: (),
        _request: session_event_model::QuestionRequest,
        _turn: CancellationToken,
        _agent: CancellationToken,
    ) -> InteractionFuture<'_, session_event_model::QuestionResponse> {
        Box::pin(async { session_event_model::QuestionResponse::Cancelled })
    }
    fn record_refusal(
        &self,
        _context: (),
        _refusal: RefusedApprovalOffer,
    ) -> InteractionFuture<'_, ()> {
        Box::pin(async {})
    }
    fn cancel_all(&self, _context: (), _reason: &'static str) -> InteractionFuture<'_, ()> {
        Box::pin(async {})
    }
    fn cancel_retired(&self) -> InteractionFuture<'_, ()> {
        Box::pin(async {})
    }
}

struct NoEvents;
impl SessionEventSink for NoEvents {
    fn begin_history_replay(&self, _session: &str) -> HistoryReplayFuture<'_> {
        Box::pin(async { Ok(()) })
    }
    fn publish(
        &self,
        _session: &str,
        _event: session_event_model::SessionEvent,
    ) -> Result<(), EventSinkClosed> {
        Ok(())
    }
}

fn session_mcp(address: &str) -> ProviderSessionMcp {
    ProviderSessionMcp::Http {
        server_name: "collaboration".to_owned(),
        server_url: address.to_owned(),
    }
}

async fn fixture_client(
    root: &Path,
    mode: &str,
) -> Result<AgentSessionClient<NoInteraction>, ExternalProviderRuntimeError> {
    AgentSessionClient::initialize_with_mcp_http(
        ExternalProviderLaunch {
            executable: "node".into(),
            arguments: vec![
                FIXTURE.into(),
                mode.into(),
                root.join("requests.json").to_string_lossy().into_owned(),
            ],
            environment: Vec::new(),
            persistence_target: ProviderPersistenceTarget::Unspecified,
        },
        ProviderModelPicker::Standard,
        "collaboration",
        CONNECTION_ADDRESS,
        Arc::new(NoInteraction),
        Arc::new(NoEvents),
    )
    .await
}

fn captured_requests(root: &Path) -> Result<Vec<Value>, Box<dyn std::error::Error>> {
    Ok(serde_json::from_slice(&std::fs::read(
        root.join("requests.json"),
    )?)?)
}

#[tokio::test]
async fn session_mcp_binding_create_load_and_resume_keep_their_own_full_addresses()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let client = fixture_client(root.path(), "http").await?;
    let first = client
        .create_session_with_settings(
            root.path().into(),
            RequestedProviderSettings::default(),
            session_mcp(FIRST_ADDRESS),
        )
        .await?;
    let second = client
        .create_session_with_settings(
            root.path().into(),
            RequestedProviderSettings::default(),
            session_mcp(SECOND_ADDRESS),
        )
        .await?;
    client
        .close_session(first.provider_session_id.clone())
        .await?;
    client
        .load_session(
            first.provider_session_id.clone(),
            root.path().into(),
            session_mcp(FIRST_ADDRESS),
        )
        .await?;
    client
        .close_session(first.provider_session_id.clone())
        .await?;
    client
        .resume_session(
            first.provider_session_id,
            root.path().into(),
            session_mcp(FIRST_ADDRESS),
        )
        .await?;
    client.shutdown().await;
    if client.shutdown_failed() {
        return Err("ACP client shutdown failed".into());
    }

    let requests = captured_requests(root.path())?;
    let setup: Vec<_> = requests
        .iter()
        .filter(|request| request["method"] != "session/close")
        .collect();
    if setup.len() != 4 {
        return Err(format!("expected four setup requests, received {}", setup.len()).into());
    }
    for (request, (method, address)) in setup.iter().zip([
        ("session/new", FIRST_ADDRESS),
        ("session/new", SECOND_ADDRESS),
        ("session/load", FIRST_ADDRESS),
        ("session/resume", FIRST_ADDRESS),
    ]) {
        if request["method"] != method {
            return Err(format!("expected {method}, received {}", request["method"]).into());
        }
        let expected = json!([{"type":"http","name":"collaboration","url":address,"headers":[]}]);
        if request["params"]["mcpServers"] != expected {
            return Err(format!(
                "wrong MCP list for {method}: {}",
                request["params"]["mcpServers"]
            )
            .into());
        }
    }
    if second.provider_session_id != "fixture-session-2" {
        return Err("second create did not return its own provider session".into());
    }
    Ok(())
}

#[tokio::test]
async fn session_mcp_binding_http_requirement_refuses_before_any_provider_command()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let client = fixture_client(root.path(), "no-http").await?;
    let create = client
        .create_session_with_settings(
            root.path().into(),
            RequestedProviderSettings::default(),
            session_mcp(FIRST_ADDRESS),
        )
        .await;
    let load = client
        .load_session(
            "fixture-session".into(),
            root.path().into(),
            session_mcp(FIRST_ADDRESS),
        )
        .await;
    let resume = client
        .resume_session(
            "fixture-session".into(),
            root.path().into(),
            session_mcp(FIRST_ADDRESS),
        )
        .await;
    client.shutdown().await;
    if !matches!(
        create,
        Err(ExternalProviderRuntimeError::UnsupportedCapability {
            capability: "mcp/http"
        })
    ) {
        return Err("HTTP MCP create was not refused before dispatch".into());
    }
    if !matches!(
        load,
        Err(ExternalProviderRuntimeError::UnsupportedCapability {
            capability: "mcp/http"
        })
    ) {
        return Err("HTTP MCP load was not refused before dispatch".into());
    }
    if !matches!(
        resume,
        Err(ExternalProviderRuntimeError::UnsupportedCapability {
            capability: "mcp/http"
        })
    ) {
        return Err("HTTP MCP resume was not refused before dispatch".into());
    }
    if !captured_requests(root.path())?.is_empty() {
        return Err("unsupported HTTP MCP emitted a provider command".into());
    }
    Ok(())
}

#[tokio::test]
async fn session_mcp_binding_access_enrollment_keeps_the_existing_non_http_create()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::tempdir()?;
    let client = fixture_client(root.path(), "no-http").await?;
    client
        .create_session_with_settings(
            root.path().into(),
            RequestedProviderSettings::default(),
            ProviderSessionMcp::ConnectionDefaults,
        )
        .await?;
    client.shutdown().await;
    let requests = captured_requests(root.path())?;
    if requests.len() != 1 {
        return Err("legacy create emitted an unexpected command count".into());
    }
    if requests[0]["method"] != "session/new" {
        return Err("legacy create emitted a different method".into());
    }
    if requests[0]["params"]["mcpServers"] != json!([]) {
        return Err("legacy non-HTTP create changed its MCP list".into());
    }
    Ok(())
}
