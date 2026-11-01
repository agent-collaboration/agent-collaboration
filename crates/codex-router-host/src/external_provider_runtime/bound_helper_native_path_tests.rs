//! Real MCP -> SDK -> ACP bridge -> native socket proves producer/consumer identity and starter attribution.
use super::setup_registrar_fixture::ProviderRegistrarFixture;
use acp_client_runtime::RequestedProviderSettings;
use codex_acp_adapter::{
    AcpConnectionInputs, AcpStoredSessions, CodexHarnessAdmission, ConversationOperationRecorder,
    ConversationRecordFuture,
};
use codex_native_integration::{NativePayloadSchemas, NativeSchemaBundle};
use collaboration_mcp::{
    CollaborationApiConfig, CollaborationApiListener, collaboration_api_router,
    serve_collaboration_api,
};
use collaboration_protocol::{CodexGeneration, OperationId, SessionId};
use collaboration_service::fleet_session_registrar::SessionRegistrationMode;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap, future::Future, io, os::unix::fs::PermissionsExt, pin::Pin, sync::Arc,
};
use tokio_tungstenite::tungstenite::Message;

struct EmptyStoredSessions;
impl AcpStoredSessions for EmptyStoredSessions {
    fn list(&self, _params: Value) -> Pin<Box<dyn Future<Output = io::Result<Value>> + Send + '_>> {
        Box::pin(async { Ok(json!({"sessions":[]})) })
    }
}
struct AcceptingOperationRecorder;
impl ConversationOperationRecorder for AcceptingOperationRecorder {
    fn admit_create<'a>(
        &'a self,
        _: &'a OperationId,
        _: &'a CodexGeneration,
    ) -> ConversationRecordFuture<'a> {
        Box::pin(async { Ok(()) })
    }
    fn before_native_dispatch<'a>(&'a self, _: &'a OperationId) -> ConversationRecordFuture<'a> {
        Box::pin(async { Ok(()) })
    }
    fn record_created<'a>(
        &'a self,
        _: &'a OperationId,
        _: &'a SessionId,
    ) -> ConversationRecordFuture<'a> {
        Box::pin(async { Ok(()) })
    }
    fn record_failure<'a>(&'a self, _: &'a OperationId, _: bool) -> ConversationRecordFuture<'a> {
        Box::pin(async { Ok(()) })
    }
}
fn payload_schemas() -> Result<Arc<NativePayloadSchemas>, Box<dyn std::error::Error>> {
    let mut definitions = serde_json::Map::new();
    for operation in [
        "ThreadRead",
        "ThreadFork",
        "ThreadResume",
        "ThreadStart",
        "ThreadLoadedList",
        "TurnStart",
        "TurnSteer",
        "TurnInterrupt",
    ] {
        definitions.insert(format!("{operation}Params"), json!({"type":"object"}));
        definitions.insert(format!("{operation}Response"), json!({"type":"object"}));
    }
    let bundle = NativeSchemaBundle::from_documents(BTreeMap::from([(
        "codex_app_server_protocol.schemas.json".to_owned(),
        serde_json::to_vec(&json!({"definitions":{"v2":definitions}}))?,
    )]))?;
    Ok(Arc::new(NativePayloadSchemas::from_bundle(&bundle)?))
}

#[tokio::test]
async fn bound_helper_sdk_start_and_fork_consume_preparation_and_keep_the_bound_starter() {
    for fork in [false, true] {
        let root = tempfile::tempdir_in("/tmp").unwrap();
        let service = root.path().join("service");
        let project = root.path().join("project");
        std::fs::create_dir(&service).unwrap();
        std::fs::set_permissions(&service, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::create_dir(&project).unwrap();
        let http = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = http.local_addr().unwrap();
        let fixture = ProviderRegistrarFixture::at_origin(
            SessionRegistrationMode::MachineTokens,
            &format!("http://{address}"),
        )
        .unwrap();
        let _caller_create = fixture
            .create_setup(&project, &RequestedProviderSettings::default())
            .await
            .unwrap();
        let caller = json!({"endpoint":{"serviceId":"019f0000-0000-7000-8000-000000000001","endpointId":"claude-local"},"sessionId":"bound-starter"});
        fixture
            .registrar
            .record_provider_created(
                &fixture.operation,
                serde_json::from_value(caller.clone()).unwrap(),
            )
            .await
            .unwrap();
        fixture
            .registrar
            .complete_created_setup(&fixture.operation)
            .await
            .unwrap();
        let native_listener = tokio::net::UnixListener::bind(service.join("native.sock")).unwrap();
        let native_peer = tokio::spawn(async move {
            let (stream, _) = native_listener.accept().await.unwrap();
            let mut peer = tokio_tungstenite::accept_async(stream).await.unwrap();
            let frame = peer.next().await.unwrap().unwrap();
            let initialize: Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
            assert_eq!(initialize["method"], "initialize");
            peer.send(Message::Text(
                json!({"id":initialize["id"],"result":{}})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
            let initialized = peer.next().await.unwrap().unwrap();
            let initialized: Value = serde_json::from_str(initialized.to_text().unwrap()).unwrap();
            assert_eq!(initialized["method"], "initialized");
            let frame = peer.next().await.unwrap().unwrap();
            let request: Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
            peer.send(Message::Text(json!({"id":request["id"],"result":{
                "cwd":request["params"]["cwd"],"model":request["params"]["model"],
                "thread":{"id":"actual-native-helper","cwd":request["params"]["cwd"],"status":{"type":"idle"}}
            }}).to_string().into())).await.unwrap();
            request
        });
        let acp = tokio::net::UnixListener::bind(service.join("bridge.sock")).unwrap();
        let endpoint: collaboration_protocol::EndpointRef = serde_json::from_value(
            json!({"serviceId":"019f0000-0000-7000-8000-000000000001","endpointId":"codex-local"}),
        )
        .unwrap();
        let retirement = tokio_util::sync::CancellationToken::new();
        let holder = Arc::new(collaboration_runtime::UnmaterializedThreadHolder::new());
        let inputs = AcpConnectionInputs {
            harness: CodexHarnessAdmission::MachineTokens {
                endpoint: endpoint.clone(),
                port: fixture.native_launch_port(),
            },
            backend_path: service.join("native.sock"),
            generation: serde_json::from_value(
                json!({"serviceEpoch":"019f0000-0000-7000-8000-000000000005","generation":1}),
            )
            .unwrap(),
            schemas: payload_schemas().unwrap(),
            stored_sessions: Arc::new(EmptyStoredSessions),
            retired: retirement.clone(),
            approval_broker: Arc::new(codex_acp_adapter::RejectingApprovalBroker),
            holder: holder.clone(),
            recorder: Arc::new(AcceptingOperationRecorder),
        };
        let acp_server = tokio::spawn(async move {
            let (stream, _) = acp.accept().await?;
            codex_acp_adapter::serve_acp_connection(stream, inputs).await
        });
        let description = collaboration_protocol::EndpointDescription {
            endpoint,
            label: "fixture native".to_owned().try_into().unwrap(),
            availability: collaboration_protocol::EndpointAvailability::Available {
                observed_at: "2026-11-01T00:00:00Z".to_owned().try_into().unwrap(),
            },
            channels: vec![collaboration_protocol::ChannelDescription::Acp {
                transport: collaboration_protocol::AcpCarrier::UnixJsonLines,
                path: "bridge.sock".to_owned().try_into().unwrap(),
                schema_digest: format!("sha256:{}", collaboration_protocol::ACP_SCHEMA_DIGEST)
                    .try_into()
                    .unwrap(),
            }],
        };
        let application = collaboration_service::ApplicationFixture::new(
            "019f0000-0000-7000-8000-000000000001",
            "019f0000-0000-7000-8000-000000000005",
        )
        .unwrap()
        .with_endpoints(vec![description])
        .unwrap()
        .build();
        let (_relation, receiver) =
            tokio::sync::watch::channel(collaboration_protocol::RouterExecutableRelation::Match);
        let shutdown = tokio_util::sync::CancellationToken::new();
        let config = CollaborationApiConfig {
            bound_conversation_preparation: Some(fixture.bound_preparation()),
            application,
            session_registrar: Some(fixture.registrar.clone()),
            service_directory: service,
            native_definitions: None,
            router_executable_relation: receiver,
            concurrent_requests: collaboration_mcp::DEFAULT_CONCURRENT_REQUESTS,
            shutdown: shutdown.clone(),
        };
        let router = collaboration_api_router(
            &config,
            CollaborationApiListener::LoopbackTcp {
                address,
                proxy_authority: None,
            },
        );
        let api_server = tokio::spawn(serve_collaboration_api(http, router, shutdown.clone()));
        let mut arguments = json!({"operationId":"019f0000-0000-7000-8000-000000000021","endpoint":{"serviceId":"019f0000-0000-7000-8000-000000000001","endpointId":"codex-local"},
            "workingDirectory":project,"projectId":"019f0000-0000-7000-8000-000000000002","access":"write-restricted","createdBy":caller,
            "model":"fixture-model","effort":"low","rootMessageId":"019f0000-0000-7000-8000-000000000099"});
        if fork {
            arguments["fork"] = json!("source-conversation");
        }
        let response = reqwest::Client::new().post(format!("http://{address}/mcp/session/35353535353535353535353535353535"))
            .header("Content-Type","application/json").header("Accept","application/json, text/event-stream").header("MCP-Protocol-Version","2025-11-25")
            .json(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"conversation_create","arguments":arguments}})).send().await.unwrap();
        let body = response.text().await.unwrap();
        let encoded = body
            .lines()
            .find_map(|line| {
                line.strip_prefix("data: ")
                    .filter(|value| !value.is_empty())
            })
            .unwrap_or(body.as_str());
        let answer: Value = serde_json::from_str(encoded).unwrap();
        if answer["result"]["structuredContent"]["kind"] != "created" {
            retirement.cancel();
            shutdown.cancel();
            acp_server.abort();
            native_peer.abort();
            api_server.await.unwrap().unwrap();
            panic!("bound helper did not create: {answer}");
        }
        let native = native_peer.await.unwrap();
        retirement.cancel();
        shutdown.cancel();
        api_server.await.unwrap().unwrap();
        acp_server.await.unwrap().unwrap();
        holder.drain_host_tasks().await;

        assert_eq!(
            native["method"],
            if fork { "thread/fork" } else { "thread/start" }
        );
        let registrations = fixture.authority.registrations.lock().unwrap().clone();
        assert_eq!(registrations.len(), 2);
        assert_eq!(
            registrations[1].session.session_id.as_str(),
            "actual-native-helper"
        );
        assert_eq!(
            serde_json::to_value(&registrations[1].started_by).unwrap(),
            json!({"kind":"session","session":caller})
        );
        assert_eq!(
            registrations[1].isolation.kind,
            fleet_identity_protocol::IsolationKind::Host
        );
        assert_eq!(
            registrations[1].machine_environment,
            fleet_identity_protocol::MachineEnvironment::CloudVm
        );
        assert_ne!(
            registrations[0].isolation.instance_id,
            registrations[1].isolation.instance_id
        );
        let records = fixture.repository.records.lock().unwrap().clone();
        let helper = records
            .iter()
            .find(|record| {
                record
                    .lifecycle()
                    .session()
                    .is_some_and(|session| session.session_id.as_str() == "actual-native-helper")
            })
            .unwrap();
        assert_eq!(
            native["params"]["config"]["mcp_servers"],
            json!({"agent-router":{"url":helper.preparation().binding_address.expose_address()}})
        );
        assert_eq!(
            answer["result"]["structuredContent"]["target"]["sessionId"],
            "actual-native-helper"
        );
    }
}
