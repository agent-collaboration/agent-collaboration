//! Same-Host frontend reconnect through the ACP registry and the registered scratch sweep.
use super::{DAY, Fixture, private};
use codex_acp_adapter::{AcpConnectionInputs, NativeStoredSessions, serve_acp_connection};
use codex_native_integration::{NativePayloadSchemas, NativeSchemaBundle};
use collaboration_protocol::{CodexGeneration, EndpointRef, OperationId};
use collaboration_runtime::{CodexConversationOperationRecorder, UnmaterializedThreadHolder};
use futures_util::{SinkExt as _, StreamExt as _};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader},
    net::{UnixListener, UnixStream},
};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

const THREAD: &str = "thread-reconnect";
const SCOPE: &str = "session-018f47d2-24d5-7a68-b9ec-6f759c39458f";

fn schemas() -> NativePayloadSchemas {
    let mut v2 = serde_json::Map::new();
    for name in [
        "ThreadRead",
        "ThreadResume",
        "ThreadStart",
        "ThreadLoadedList",
        "TurnStart",
        "TurnSteer",
        "TurnInterrupt",
    ] {
        v2.insert(format!("{name}Params"), json!({"type":"object"}));
        v2.insert(format!("{name}Response"), json!({"type":"object"}));
    }
    let bundle = NativeSchemaBundle::from_documents(BTreeMap::from([(
        "codex_app_server_protocol.schemas.json".to_owned(),
        serde_json::to_vec(&json!({"definitions":{
            "v2":v2,"ServerRequest":{"type":"object"},"ServerNotification":{"type":"object"}
        }}))
        .expect("schema JSON"),
    )]))
    .expect("fixture bundle");
    NativePayloadSchemas::from_bundle(&bundle).expect("fixture payload schemas")
}

async fn native_thread(listener: UnixListener, cwd: PathBuf, scratch: PathBuf) {
    let (stream, _) = listener.accept().await.expect("native connection");
    let mut wire = tokio_tungstenite::accept_async(stream)
        .await
        .expect("native websocket");
    let checkout_result = json!({
        "cwd":cwd,"model":"gpt-5.6-sol","approvalPolicy":"on-request",
        "approvalsReviewer":"auto_review",
        "activePermissionProfile":{"id":"router-workspace-write","extends":":workspace"},
        "sandbox":{"type":"workspaceWrite","writableRoots":[&cwd,scratch],"networkAccess":true,
            "excludeTmpdirEnvVar":true,"excludeSlashTmp":true},
        "thread":{"id":THREAD,"cwd":cwd,"status":{"type":"idle"},"turns":[]}
    });
    for method in [
        "initialize",
        "initialized",
        "thread/start",
        "thread/read",
        "turn/start",
        "thread/read",
        "thread/resume",
    ] {
        let frame = wire
            .next()
            .await
            .expect("native frame")
            .expect("native message");
        let request: Value = serde_json::from_str(frame.to_text().expect("text")).expect("JSON");
        assert_eq!(request["method"], method);
        let result = match method {
            "initialize" => json!({}),
            "initialized" => continue,
            "thread/start" => checkout_result.clone(),
            "thread/read" => json!({"thread":{"id":THREAD,"status":{"type":"idle"},
                "model":"gpt-5.6-sol","reasoningEffort":"medium","createdAt":1,"updatedAt":1}}),
            "turn/start" => json!({"turn":{"id":"turn-a","status":"inProgress"}}),
            "thread/resume" => {
                assert_eq!(request["params"], json!({"threadId":THREAD}));
                checkout_result.clone()
            }
            _ => panic!("unexpected native method in the fixture: {method}"),
        };
        wire.send(Message::Text(
            json!({"id":request["id"],"result":result})
                .to_string()
                .into(),
        ))
        .await
        .expect("native reply");
        if method == "turn/start" {
            wire.send(Message::Text(
                json!({"method":"turn/completed","params":{
                    "threadId":THREAD,"turn":{"id":"turn-a","status":"completed"}
                }})
                .to_string()
                .into(),
            ))
            .await
            .expect("native terminal");
        }
    }
}

async fn call(frontend: &mut BufReader<UnixStream>, method: &str, params: Value) -> Value {
    let request = json!({"jsonrpc":"2.0","id":method,"method":method,"params":params});
    frontend
        .write_all(format!("{request}\n").as_bytes())
        .await
        .expect("ACP request");
    let mut line = String::new();
    frontend.read_line(&mut line).await.expect("ACP response");
    let response: Value = serde_json::from_str(&line).expect("ACP JSON");
    assert_eq!(response["id"], method);
    assert!(response.get("result").is_some(), "{response}");
    response["result"].clone()
}

#[tokio::test]
async fn materialized_codex_reconnect_keeps_the_lease_through_the_host_sweep() {
    tokio::time::timeout(Duration::from_secs(15), async {
        let mut fixture = Fixture::start(|_, _| {}).await;
        let scope = fixture.folder(SCOPE);
        private(&scope);
        let cwd = fixture._root.path().to_path_buf();
        let socket_root = tempfile::tempdir_in("/tmp").expect("short fixture socket root");
        let socket = socket_root.path().join("native.sock");
        let listener = UnixListener::bind(&socket).expect("fixture listener");
        let native = tokio::spawn(native_thread(listener, cwd.clone(), scope.clone()));
        let holder = Arc::new(UnmaterializedThreadHolder::new());
        let endpoint: EndpointRef = serde_json::from_value(json!({
            "serviceId":fixture.runtime.service_id,"endpointId":"codex-local"
        }))
        .expect("endpoint");
        let generation: CodexGeneration = serde_json::from_value(json!({
            "serviceEpoch":fixture.runtime.service_epoch,"generation":1
        }))
        .expect("generation");
        let schemas = Arc::new(schemas());
        let recorder = Arc::new(CodexConversationOperationRecorder::new(
            fixture
                .runtime
                .provider_store
                .clone()
                .expect("provider store"),
            endpoint.clone(),
            "scratch-reconnect-acp"
                .to_owned()
                .try_into()
                .expect("listener name"),
        ));
        let open = Arc::clone(&fixture.runtime.open_scratch_scopes);
        let connect = || {
            let (client, server) = UnixStream::pair().expect("frontend sockets");
            let serving = tokio::spawn(serve_acp_connection(
                server,
                AcpConnectionInputs {
                    backend_path: socket.clone(),
                    generation: generation.clone(),
                    schemas: schemas.clone(),
                    stored_sessions: Arc::new(NativeStoredSessions::new(
                        cwd.clone(),
                        "fixture".into(),
                    )),
                    holder: holder.clone(),
                    recorder: recorder.clone(),
                    retired: CancellationToken::new(),
                    scratch_scopes: open.clone(),
                },
            ));
            (BufReader::new(client), serving)
        };
        let actor = json!({"endpoint":endpoint,"sessionId":"creator"});
        let (mut first, first_serving) = connect();
        call(&mut first, "initialize", json!({"protocolVersion":1})).await;
        let created = call(
            &mut first,
            "session/new",
            json!({
                "cwd":cwd,"mcpServers":[],"_meta":{"codexRouter":{
                    "operationId":OperationId::generate(),"model":"gpt-5.6-sol","effort":"medium",
                    "access":"workspace-write","scratchScope":SCOPE,"scratchPath":scope,
                    "createdBy":actor,"approver":actor
                }}
            }),
        )
        .await;
        assert_eq!(created["sessionId"], THREAD);
        let prompt = call(
            &mut first,
            "session/prompt",
            json!({
                "sessionId":THREAD,"prompt":[{"type":"text","text":"fixture"}]
            }),
        )
        .await;
        assert_eq!(prompt["stopReason"], "end_turn");
        first.shutdown().await.expect("first frontend EOF");
        let serving = first_serving.await.expect("first frontend task");
        serving.expect("first frontend shutdown");
        assert!(
            open.is_open(&scope),
            "frontend shutdown dropped the materialized lease"
        );

        let (mut second, second_serving) = connect();
        call(&mut second, "initialize", json!({"protocolVersion":1})).await;
        call(
            &mut second,
            "session/load",
            json!({
                "sessionId":THREAD,"cwd":cwd,"mcpServers":[]
            }),
        )
        .await;
        native
            .await
            .expect("reconnect used the original native connection");
        assert!(open.is_open(&scope));
        fixture.clock.advance(40 * DAY);
        fixture.start_sweep();
        let pass = fixture.next_pass().await;
        assert_eq!(pass.kept_open, vec![SCOPE.to_owned()]);
        assert!(scope.is_dir());

        second.shutdown().await.expect("second frontend EOF");
        second_serving
            .await
            .expect("second frontend task")
            .expect("second frontend shutdown");
        holder.drain_host_tasks().await;
        assert!(open.is_open(&scope), "the holder owns the returned lease");
        drop(holder);
        assert!(
            !open.is_open(&scope),
            "retiring the holder releases the lease"
        );
        drop(recorder);
        fixture.stop().await;
    })
    .await
    .expect("scratch reconnect and sweep finish");
}
