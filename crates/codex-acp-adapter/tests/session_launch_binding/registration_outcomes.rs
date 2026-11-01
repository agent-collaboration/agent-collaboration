//! A valid native conversation is retained even when organization registration cannot activate it.
use super::*;

#[tokio::test]
async fn session_launch_binding_registration_pending_or_refused_never_returns_a_ready_binding() {
    for completion in [
        HarnessRegistrationOutcome::Pending,
        HarnessRegistrationOutcome::Refused(
            serde_json::from_value(json!({"kind":"projectUnknown"})).unwrap(),
        ),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let cwd = std::fs::canonicalize(directory.path()).unwrap();
        let scope = "session-019f0000-0000-7000-8000-000000000099";
        let scratch = cwd.join("scratch").join(scope);
        std::fs::create_dir_all(&scratch).unwrap();
        std::fs::set_permissions(&scratch, std::fs::Permissions::from_mode(0o700)).unwrap();
        let launch = Arc::new(RecordingLaunch {
            completion: completion.clone(),
            ..RecordingLaunch::default()
        });
        let operation: OperationId = "019f0000-0000-7000-8000-000000000020"
            .to_owned()
            .try_into()
            .unwrap();
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
                "cwd":response_cwd,"model":"fixture-model","sandbox":sandbox,
                "activePermissionProfile":{"id":"router-workspace-write","extends":":workspace"},
                "thread":{"id":"native-returned-conversation","cwd":response_cwd,"status":{"type":"idle"}}
            }}).to_string().into())).await.unwrap();
            request
        });
        let actor = json!({"endpoint":{"serviceId":INSTALLATION,"endpointId":"codex-local"},"sessionId":"permission-route-owner"});
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
                params: json!({"cwd":cwd,"mcpServers":[],"_meta":{"codexRouter":{
                    "model":"fixture-model","effort":"low","access":"workspace-write",
                    "createdBy":actor,"approver":actor,"scratchScope":scope,"scratchPath":scratch
                }}}),
                approval_broker: Arc::new(codex_acp_adapter::RejectingApprovalBroker),
                operation_id: Some(operation),
                recorder: Arc::new(conversation_operation_recorder::AcceptingConversationRecorder),
            },
        )
        .await;
        let request = capture.await.unwrap();

        assert_eq!(request["method"], "thread/start");
        let terminal = match completion {
            HarnessRegistrationOutcome::Pending => {
                assert!(matches!(
                    answer,
                    Err(codex_acp_adapter::SessionSetupError::OutcomeUnknown)
                ));
                "registrationPending"
            }
            HarnessRegistrationOutcome::Refused(_) => {
                assert!(matches!(
                    answer,
                    Err(codex_acp_adapter::SessionSetupError::RegistrationRefused)
                ));
                "registrationRefused"
            }
            HarnessRegistrationOutcome::Registered => {
                panic!("fixture requests non-active outcomes")
            }
        };
        assert_eq!(
            *launch.events.lock().unwrap(),
            ["dispatched", "rpc", "created", terminal]
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
