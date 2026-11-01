//! A scripted native socket captures the admitted binding at the actual Start/Fork boundary.
use codex_acp_adapter::{
    AcpSchemaCatalog, AcpSessionBinding, HarnessSessionSetup, PreparedHarnessSessionStart,
    RestoredHarnessSession, SessionSetupInputs,
};
use codex_native_integration::{
    NativePayloadSchemas, NativeProtocolConnection, NativeSchemaBundle,
};
use collaboration_contracts::{
    HarnessLocalSetupRefusal, HarnessRegistrationOutcome, HarnessSessionBinding,
};
use collaboration_protocol::OperationId;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{collections::BTreeMap, os::unix::fs::PermissionsExt, sync::Arc};
use tokio_tungstenite::{
    WebSocketStream,
    tungstenite::{Message, protocol::Role},
};

#[path = "support/conversation_operation_recorder.rs"]
mod conversation_operation_recorder;
#[path = "support/native_permission_echo.rs"]
mod native_permission_echo;

#[path = "support/recording_harness_launch.rs"]
mod recording_harness_launch;
use recording_harness_launch::RecordingLaunch;

const ADDRESS: &str = "http://127.0.0.1:43210/mcp/session/11111111111111111111111111111111";
const INSTALLATION: &str = "019f0000-0000-7000-8000-000000000003";

#[tokio::test]
async fn session_launch_binding_cold_resume_uses_the_original_admitted_address() {
    let directory = tempfile::tempdir().unwrap();
    let cwd = std::fs::canonicalize(directory.path()).unwrap();
    let (client, server) = tokio::net::UnixStream::pair().unwrap();
    let connection = NativeProtocolConnection::from_websocket(
        WebSocketStream::from_raw_socket(client, Role::Client, None).await,
    );
    let response_cwd = cwd.clone();
    let capture = tokio::spawn(async move {
        let mut server = WebSocketStream::from_raw_socket(server, Role::Server, None).await;
        let frame = server.next().await.unwrap().unwrap();
        let request: Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
        server.send(Message::Text(json!({"id":request["id"],"result":{
            "cwd":response_cwd,"thread":{"id":"native-returned-conversation","cwd":response_cwd,"status":{"type":"idle"},"turns":[]}
        }}).to_string().into())).await.unwrap();
        request
    });
    let mut catalog = AcpSchemaCatalog::load().unwrap();
    let result = AcpSessionBinding::load_existing(&mut catalog, SessionSetupInputs {
        harness: HarnessSessionSetup::Restore(RestoredHarnessSession {
            session: serde_json::from_value(json!({"endpoint":{"serviceId":INSTALLATION,"endpointId":"codex-local"},"sessionId":"native-returned-conversation"})).unwrap(),
            binding: HarnessSessionBinding::from_host_record("agent-router".to_owned().try_into().unwrap(), ADDRESS.to_owned()),
        }),
        connection, schemas: payload_schemas().unwrap(),
        generation: serde_json::from_value(json!({"serviceEpoch":"019f0000-0000-7000-8000-000000000004","generation":1})).unwrap(),
        params: json!({"sessionId":"native-returned-conversation","cwd":cwd,"mcpServers":[]}),
        approval_broker: Arc::new(codex_acp_adapter::RejectingApprovalBroker),
        operation_id: None, recorder: Arc::new(conversation_operation_recorder::AcceptingConversationRecorder),
    }).await.unwrap();
    let request = capture.await.unwrap();

    assert_eq!(request["method"], "thread/resume");
    assert_eq!(
        request["params"]["config"]["mcp_servers"],
        json!({"agent-router":{"url":ADDRESS}})
    );
    assert_eq!(result.0.session_id(), "native-returned-conversation");
    assert!(result.1.is_empty());
}
fn payload_schemas() -> Result<Arc<NativePayloadSchemas>, Box<dyn std::error::Error>> {
    let mut definitions = serde_json::Map::new();
    for name in [
        "ThreadRead",
        "ThreadFork",
        "ThreadResume",
        "ThreadStart",
        "ThreadLoadedList",
        "TurnStart",
        "TurnSteer",
        "TurnInterrupt",
    ] {
        definitions.insert(format!("{name}Params"), json!({"type":"object"}));
        definitions.insert(format!("{name}Response"), json!({"type":"object"}));
    }
    let bundle = NativeSchemaBundle::from_documents(BTreeMap::from([(
        "codex_app_server_protocol.schemas.json".to_owned(),
        serde_json::to_vec(&json!({"definitions":{"v2":definitions}}))?,
    )]))?;
    Ok(Arc::new(NativePayloadSchemas::from_bundle(&bundle)?))
}

#[tokio::test]
async fn session_launch_binding_start_and_fork_use_the_admitted_address_and_keep_the_returned_id() {
    for fork in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let cwd = std::fs::canonicalize(directory.path()).unwrap();
        let scope = "session-019f0000-0000-7000-8000-000000000099";
        let scratch = cwd.join("scratch").join(scope);
        std::fs::create_dir_all(&scratch).unwrap();
        std::fs::set_permissions(&scratch, std::fs::Permissions::from_mode(0o700)).unwrap();
        let launch = Arc::new(RecordingLaunch::default());
        let operation: OperationId =
            serde_json::from_value(json!("019f0000-0000-7000-8000-000000000020")).unwrap();
        let endpoint =
            serde_json::from_value(json!({"serviceId":INSTALLATION,"endpointId":"codex-local"}))
                .unwrap();
        let binding = HarnessSessionBinding::from_host_record(
            "agent-router".to_owned().try_into().unwrap(),
            ADDRESS.to_owned(),
        );
        let (client, server) = tokio::net::UnixStream::pair().unwrap();
        let connection = NativeProtocolConnection::from_websocket(
            WebSocketStream::from_raw_socket(client, Role::Client, None).await,
        );
        let server_trace = Arc::clone(&launch);
        let response_cwd = cwd.clone();
        let capture = tokio::spawn(async move {
            let mut server = WebSocketStream::from_raw_socket(server, Role::Server, None).await;
            let frame = server.next().await.unwrap().unwrap();
            let request: Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
            server_trace.event("rpc").unwrap();
            let sandbox = native_permission_echo::applied_router_sandbox(&request);
            server.send(Message::Text(json!({"id":request["id"],"result":{
                "cwd":response_cwd,"model":"fixture-model", "sandbox":sandbox,
                "activePermissionProfile":{"id":"router-workspace-write","extends":":workspace"},
                "thread":{"id":"native-returned-conversation","cwd":response_cwd,"status":{"type":"idle"}}
            }}).to_string().into())).await.unwrap();
            request
        });
        let actor = json!({"endpoint":{"serviceId":INSTALLATION,"endpointId":"codex-local"},"sessionId":"permission-route-owner"});
        let mut params = json!({"cwd":cwd,"mcpServers":[],"_meta":{"codexRouter":{
            "model":"fixture-model","effort":"low","access":"workspace-write",
            "createdBy":actor,"approver":actor,"scratchScope":scope,"scratchPath":scratch
        }}});
        if fork {
            params["_meta"]["codexRouter"]["forkThreadId"] = json!("source-thread");
        }
        let mut catalog = AcpSchemaCatalog::load().unwrap();
        let result = AcpSessionBinding::create(
            &mut catalog,
            SessionSetupInputs {
                harness: HarnessSessionSetup::Start(PreparedHarnessSessionStart {
                    port: launch.clone(),
                    operation_id: operation.clone(),
                    endpoint,
                    binding,
                }),
                connection,
                schemas: payload_schemas().unwrap(),
                generation: serde_json::from_value(
                    json!({"serviceEpoch":"019f0000-0000-7000-8000-000000000004","generation":1}),
                )
                .unwrap(),
                params,
                approval_broker: Arc::new(codex_acp_adapter::RejectingApprovalBroker),
                operation_id: Some(operation),
                recorder: Arc::new(conversation_operation_recorder::AcceptingConversationRecorder),
            },
        )
        .await
        .unwrap();
        let request = capture.await.unwrap();

        assert_eq!(
            request["method"],
            if fork { "thread/fork" } else { "thread/start" }
        );
        assert_eq!(
            request["params"]["config"]["mcp_servers"],
            json!({"agent-router":{"url":ADDRESS}})
        );
        assert_eq!(result.session_id(), "native-returned-conversation");
        assert_eq!(
            *launch.events.lock().unwrap(),
            ["dispatched", "rpc", "created", "registered"]
        );
        assert_eq!(
            launch
                .created
                .lock()
                .unwrap()
                .first()
                .unwrap()
                .session_id
                .as_str(),
            "native-returned-conversation"
        );
    }
}

#[tokio::test]
async fn session_launch_binding_local_refusals_record_the_returned_id_before_abandoning_setup() {
    for cause in [
        HarnessLocalSetupRefusal::ModelMismatch,
        HarnessLocalSetupRefusal::EffectiveWorkingDirectoryMismatch,
        HarnessLocalSetupRefusal::WorkspaceRootsMismatch,
    ] {
        let directory = tempfile::tempdir().unwrap();
        let cwd = std::fs::canonicalize(directory.path()).unwrap();
        let scope = "session-019f0000-0000-7000-8000-000000000099";
        let scratch = cwd.join("scratch").join(scope);
        std::fs::create_dir_all(&scratch).unwrap();
        std::fs::set_permissions(&scratch, std::fs::Permissions::from_mode(0o700)).unwrap();
        let launch = Arc::new(RecordingLaunch::default());
        let operation: OperationId =
            serde_json::from_value(json!("019f0000-0000-7000-8000-000000000020")).unwrap();
        let (client, server) = tokio::net::UnixStream::pair().unwrap();
        let connection = NativeProtocolConnection::from_websocket(
            WebSocketStream::from_raw_socket(client, Role::Client, None).await,
        );
        let server_trace = Arc::clone(&launch);
        let response_cwd = if cause == HarnessLocalSetupRefusal::EffectiveWorkingDirectoryMismatch {
            cwd.join("different-directory")
        } else {
            cwd.clone()
        };
        let capture = tokio::spawn(async move {
            let mut server = WebSocketStream::from_raw_socket(server, Role::Server, None).await;
            let frame = server.next().await.unwrap().unwrap();
            let request: Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
            server_trace.event("rpc").unwrap();
            let model = if cause == HarnessLocalSetupRefusal::ModelMismatch {
                "different-model"
            } else {
                "fixture-model"
            };
            server.send(Message::Text(json!({"id":request["id"],"result":{
                "cwd":response_cwd,"model":model,"runtimeWorkspaceRoots":[],
                "thread":{"id":"native-returned-conversation","cwd":response_cwd,"status":{"type":"idle"}}
            }}).to_string().into())).await.unwrap();
            request
        });
        let actor = json!({"endpoint":{"serviceId":INSTALLATION,"endpointId":"codex-local"},"sessionId":"permission-route-owner"});
        let mut params = json!({"cwd":cwd,"mcpServers":[],"_meta":{"codexRouter":{
            "model":"fixture-model","effort":"low","access":"workspace-write",
            "createdBy":actor,"approver":actor,"scratchScope":scope,"scratchPath":scratch
        }}});
        if cause == HarnessLocalSetupRefusal::WorkspaceRootsMismatch {
            params["additionalDirectories"] = json!([cwd.join("requested-additional-directory")]);
        }
        let mut catalog = AcpSchemaCatalog::load().unwrap();
        let answer = AcpSessionBinding::create(
            &mut catalog,
            SessionSetupInputs {
                harness: HarnessSessionSetup::Start(PreparedHarnessSessionStart {
                    port: launch.clone(),
                    operation_id: operation.clone(),
                    endpoint: serde_json::from_value(
                        json!({"serviceId":INSTALLATION,"endpointId":"codex-local"}),
                    )
                    .unwrap(),
                    binding: HarnessSessionBinding::from_host_record(
                        "agent-router".to_owned().try_into().unwrap(),
                        ADDRESS.to_owned(),
                    ),
                }),
                connection,
                schemas: payload_schemas().unwrap(),
                generation: serde_json::from_value(
                    json!({"serviceEpoch":"019f0000-0000-7000-8000-000000000004","generation":1}),
                )
                .unwrap(),
                params,
                approval_broker: Arc::new(codex_acp_adapter::RejectingApprovalBroker),
                operation_id: Some(operation),
                recorder: Arc::new(conversation_operation_recorder::AcceptingConversationRecorder),
            },
        )
        .await;
        let request = capture.await.unwrap();

        assert_eq!(request["method"], "thread/start");
        match cause {
            HarnessLocalSetupRefusal::InitialSettingRefused { .. } => {
                panic!("native fixture selects only native setup checks")
            }
            HarnessLocalSetupRefusal::ModelMismatch => assert!(matches!(
                answer,
                Err(codex_acp_adapter::SessionSetupError::ModelMismatch { .. })
            )),
            HarnessLocalSetupRefusal::EffectiveWorkingDirectoryMismatch
            | HarnessLocalSetupRefusal::WorkspaceRootsMismatch => assert!(matches!(
                answer,
                Err(codex_acp_adapter::SessionSetupError::ConfigurationMismatch)
            )),
        }
        assert_eq!(
            *launch.events.lock().unwrap(),
            ["dispatched", "rpc", "created", "setupRefused"]
        );
        assert_eq!(*launch.refused.lock().unwrap(), [cause]);
        assert_eq!(
            launch
                .created
                .lock()
                .unwrap()
                .first()
                .unwrap()
                .session_id
                .as_str(),
            "native-returned-conversation"
        );
    }
}

#[path = "session_launch_binding/registration_outcomes.rs"]
mod registration_outcomes;
