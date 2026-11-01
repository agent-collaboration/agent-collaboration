//! Observe Host admission through the actual setup task and initialized native Unix websocket.
use super::{SetupTaskInputs, run_session_setup};
use crate::{CodexHarnessAdmission, RejectingApprovalBroker};
use collaboration_contracts::HarnessSessionBinding;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{os::unix::fs::PermissionsExt, sync::Arc};
use tokio_tungstenite::tungstenite::Message;

#[path = "../tests/support/native_permission_echo.rs"]
mod native_permission_echo;
#[path = "../tests/support/recording_harness_launch.rs"]
mod recording_harness_launch;

#[tokio::test]
async fn native_setup_task_consumes_host_admission_before_dispatch_and_returns_the_registered_id() {
    let directory = tempfile::tempdir().unwrap();
    let cwd = std::fs::canonicalize(directory.path()).unwrap();
    let scope = "session-019f0000-0000-7000-8000-000000000099";
    let scratch = cwd.join("scratch").join(scope);
    std::fs::create_dir_all(&scratch).unwrap();
    std::fs::set_permissions(&scratch, std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = cwd.join("native.sock");
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    let address = "http://127.0.0.1:43210/mcp/session/11111111111111111111111111111111";
    let launch = Arc::new(recording_harness_launch::RecordingLaunch {
        binding: Some(HarnessSessionBinding::from_host_record(
            "agent-router".to_owned().try_into().unwrap(),
            address.to_owned(),
        )),
        ..Default::default()
    });
    let server_trace = Arc::clone(&launch);
    let response_cwd = cwd.clone();
    let capture = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut server = tokio_tungstenite::accept_async(stream).await.unwrap();
        let frame = server.next().await.unwrap().unwrap();
        let initialize: Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
        assert_eq!(initialize["method"], "initialize");
        server
            .send(Message::Text(
                json!({"id":initialize["id"],"result":{}})
                    .to_string()
                    .into(),
            ))
            .await
            .unwrap();
        let initialized = server.next().await.unwrap().unwrap();
        let initialized: Value = serde_json::from_str(initialized.to_text().unwrap()).unwrap();
        assert_eq!(initialized["method"], "initialized");
        let frame = server.next().await.unwrap().unwrap();
        let request: Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
        server_trace.event("rpc").unwrap();
        let sandbox = native_permission_echo::applied_router_sandbox(&request);
        server.send(Message::Text(json!({"id":request["id"],"result":{
            "cwd":response_cwd,"model":"fixture-model","sandbox":sandbox,
            "activePermissionProfile":{"id":"router-workspace-write","extends":":workspace"},
            "thread":{"id":"native-returned-conversation","cwd":response_cwd,"status":{"type":"idle"}}
        }}).to_string().into())).await.unwrap();
        request
    });
    let endpoint =
        json!({"serviceId":"019f0000-0000-7000-8000-000000000003","endpointId":"codex-local"});
    let actor = json!({"endpoint":endpoint,"sessionId":"permission-route-owner"});
    let result = run_session_setup(SetupTaskInputs {
        harness: CodexHarnessAdmission::MachineTokens { endpoint: serde_json::from_value(endpoint).unwrap(), port: launch.clone() },
        known_session: None, adopt_unmaterialized: false, backend_path: socket,
        schemas: super::tests::native_schemas(),
        generation: serde_json::from_value(json!({"serviceEpoch":"019f0000-0000-7000-8000-000000000004","generation":1})).unwrap(),
        params: json!({"cwd":cwd,"mcpServers":[],"_meta":{"codexRouter":{
            "projectId":"019f0000-0000-7000-8000-000000000005","model":"fixture-model","effort":"low","access":"workspace-write",
            "createdBy":actor,"approver":actor,"scratchScope":scope,"scratchPath":scratch
        }}}), create_new: true,
        operation_id: Some("019f0000-0000-7000-8000-000000000020".to_owned().try_into().unwrap()),
        recorder: Arc::new(super::tests::AcceptingRecorder), cancellation_barrier: None,
        approval_broker: Arc::new(RejectingApprovalBroker),
    }).await;
    let request = capture.await.unwrap();

    assert_eq!(request["method"], "thread/start");
    assert_eq!(
        request["params"]["config"]["mcp_servers"],
        json!({"agent-router":{"url":address}})
    );
    assert!(result.outcome.is_ok());
    assert_eq!(
        result.binding.unwrap().session_id(),
        "native-returned-conversation"
    );
    assert_eq!(
        *launch.events.lock().unwrap(),
        ["prepared", "dispatched", "rpc", "created", "registered"]
    );
}

#[tokio::test]
async fn native_setup_task_refuses_an_inactive_restore_before_opening_the_backend() {
    let directory = tempfile::tempdir().unwrap();
    let launch = Arc::new(recording_harness_launch::RecordingLaunch::default());
    let endpoint = serde_json::from_value(
        json!({"serviceId":"019f0000-0000-7000-8000-000000000003","endpointId":"codex-local"}),
    )
    .unwrap();

    let result = run_session_setup(SetupTaskInputs {
        harness: CodexHarnessAdmission::MachineTokens {
            endpoint,
            port: launch.clone(),
        },
        known_session: None,
        adopt_unmaterialized: false,
        backend_path: directory.path().join("must-not-be-opened.sock"),
        schemas: super::tests::native_schemas(),
        generation: serde_json::from_value(
            json!({"serviceEpoch":"019f0000-0000-7000-8000-000000000004","generation":1}),
        )
        .unwrap(),
        params: json!({"sessionId":"retired-conversation","cwd":directory.path(),"mcpServers":[]}),
        create_new: false,
        operation_id: None,
        recorder: Arc::new(super::tests::AcceptingRecorder),
        cancellation_barrier: None,
        approval_broker: Arc::new(RejectingApprovalBroker),
    })
    .await;

    assert!(matches!(
        result.outcome,
        Err(crate::SessionSetupError::LaunchAdmissionFailed(
            collaboration_contracts::HarnessLaunchFailure::InactiveBinding
        ))
    ));
    assert!(result.binding.is_none());
    assert_eq!(*launch.events.lock().unwrap(), ["restoreChecked"]);
}
