use collaboration_client::ClientError;
use collaboration_protocol::{CodexGeneration, EndpointDescription, SessionRef};
use collaboration_service::ApplicationFixture;

use collaboration_runtime::NativeControlBackend;
use collaboration_runtime::NativeGenerationGate;

use crate::served_api;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{collections::BTreeMap, os::unix::fs::DirBuilderExt, sync::Arc};
use tokio_tungstenite::tungstenite::Message;

/// A fixed instant well in the past, so a computed idle time can only be positive.
const BUSY_THREAD_UPDATED_AT_SECONDS: i64 = 1_700_000_000;

#[tokio::test]
async fn sdk_inspection_and_exact_interrupt_use_native_backend_with_generation_guards() {
    // Arrange: an isolated API and backend socket plus explicitly fixture-only schemas.
    let root = std::env::temp_dir().join(format!("native-control-{}", std::process::id()));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&root)
        .unwrap_or_else(|error| panic!("directory: {error}"));
    let backend_path = root.join("backend.sock");
    let listener = tokio::net::UnixListener::bind(&backend_path)
        .unwrap_or_else(|error| panic!("backend: {error}"));
    let service_id = "00000000-0000-4000-8000-000000000001";
    let epoch = "00000000-0000-4000-8000-000000000002";
    let generation: CodexGeneration =
        serde_json::from_value(json!({"serviceEpoch":epoch,"generation":1}))
            .unwrap_or_else(|error| panic!("generation: {error}"));
    let target: SessionRef = serde_json::from_value(json!({"endpoint":{"serviceId":service_id,"endpointId":"codex-local"},"sessionId":"proof-thread"})).unwrap_or_else(|error| panic!("target: {error}"));
    let mut description: EndpointDescription = serde_json::from_value(json!({"endpoint":target.endpoint,"label":"Fixture Codex","availability":{"state":"available","observedAt":"2026-09-05T12:00:00Z"},"channels":[{"kind":"nativeCodex","transport":"unixWebSocket","path":"codex-native.sock","schemaDigest":null,"generation":generation}]})).unwrap_or_else(|error| panic!("description: {error}"));
    let mut definitions = serde_json::Map::new();
    for name in [
        "ThreadRead",
        "ThreadResume",
        "ThreadStart",
        "ThreadLoadedList",
        "ThreadTurnsList",
        "TurnStart",
        "TurnSteer",
        "TurnInterrupt",
        "ThreadQueueAdd",
        "ThreadSetName",
    ] {
        definitions.insert(format!("{name}Params"), json!({"type":"object"}));
        definitions.insert(format!("{name}Response"), json!({"type":"object"}));
    }
    let bundle = codex_native_integration::NativeSchemaBundle::from_documents(BTreeMap::from([(
        "codex_app_server_protocol.schemas.json".to_owned(),
        serde_json::to_vec(&json!({"definitions":{"v2":definitions}}))
            .unwrap_or_else(|error| panic!("schema: {error}")),
    )]))
    .unwrap_or_else(|error| panic!("bundle: {error}"));
    let digest = format!(
        "sha256:{}",
        bundle
            .digest()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    if let Some(collaboration_protocol::ChannelDescription::NativeCodex { schema_digest, .. }) =
        description.channels.first_mut()
    {
        *schema_digest = Some(
            digest
                .try_into()
                .unwrap_or_else(|error| panic!("digest: {error}")),
        );
    }
    let schemas = Arc::new(
        codex_native_integration::NativePayloadSchemas::from_bundle(&bundle)
            .unwrap_or_else(|error| panic!("schemas: {error}")),
    );
    let gate = NativeGenerationGate::default();
    gate.activate(generation.clone(), backend_path.clone(), Some(schemas))
        .unwrap_or_else(|error| panic!("activate: {error}"));
    let native_backend = NativeControlBackend {
        codex_home: root.clone(),
        endpoint: target.endpoint.clone(),
        gate,
    };
    let identity = ApplicationFixture::new(service_id, epoch)
        .unwrap_or_else(|error| panic!("identity: {error}"))
        .with_endpoints(vec![description.clone()])
        .unwrap_or_else(|error| panic!("endpoints: {error}"))
        .with_native_backend(native_backend.clone())
        .unwrap_or_else(|error| panic!("binding: {error}"));
    let served = served_api::ServedApi::start(identity.clone())
        .unwrap_or_else(|error| panic!("serve: {error}"));
    // The native Thread schema requires both timestamps as unix seconds.
    let thread_updated_at = chrono::Utc::now().timestamp() - 45;
    let thread_created_at = thread_updated_at - 600;
    let valid_short_title = "Fix the retry path";
    assert!(
        collaboration_protocol::SessionDisplayName::try_from(valid_short_title.to_owned()).is_ok()
    );
    let backend = tokio::spawn(async move {
        let mut current_name = None::<String>;
        for (method, expected, result) in [
            (
                "thread/read",
                json!({"threadId":"proof-thread","includeTurns":false}),
                json!({"thread":{"id":"proof-thread","cwd":"/tmp","status":{"type":"idle"},"createdAt":thread_created_at,"updatedAt":thread_updated_at,"sandbox":{"type":"workspaceWrite"},"approvalPolicy":"on-request","approvalsReviewer":"auto_review"}}),
            ),
            (
                "thread/loaded/list",
                json!({"limit":1,"cursor":null}),
                json!({"data":["proof-thread"],"nextCursor":null}),
            ),
            (
                "thread/name/set",
                json!({"threadId":"proof-thread","name":"🔎 Review"}),
                json!({}),
            ),
            (
                "thread/loaded/list",
                json!({"limit":1,"cursor":null}),
                json!({"data":["proof-thread"],"nextCursor":null}),
            ),
            (
                "turn/interrupt",
                json!({"threadId":"proof-thread","turnId":"proof-turn"}),
                json!({}),
            ),
            (
                "thread/name/set",
                json!({"threadId":"proof-thread","name":"After send"}),
                json!({}),
            ),
        ] {
            let (stream, _) = listener
                .accept()
                .await
                .unwrap_or_else(|error| panic!("accept: {error}"));
            let mut socket = tokio_tungstenite::accept_async(stream)
                .await
                .unwrap_or_else(|error| panic!("upgrade: {error}"));
            let steps = if method == "thread/loaded/list" {
                vec![
                    "initialize",
                    "initialized",
                    method,
                    "thread/read",
                    "thread/turns/list",
                ]
            } else if method == "thread/name/set" {
                if expected["name"] == "After send" {
                    vec!["initialize", "initialized", "thread/read", method]
                } else {
                    vec![
                        "initialize",
                        "initialized",
                        "thread/read",
                        method,
                        "thread/read",
                    ]
                }
            } else {
                vec!["initialize", "initialized", method]
            };
            for expected_method in steps {
                let frame = socket
                    .next()
                    .await
                    .unwrap_or_else(|| panic!("frame"))
                    .unwrap_or_else(|error| panic!("receive: {error}"));
                let request: Value = serde_json::from_str(
                    frame
                        .to_text()
                        .unwrap_or_else(|error| panic!("text: {error}")),
                )
                .unwrap_or_else(|error| panic!("JSON: {error}"));
                assert_eq!(request["method"], expected_method);
                if expected_method == method {
                    if method == "thread/name/set" {
                        current_name = Some(
                            expected["name"]
                                .as_str()
                                .unwrap_or_else(|| panic!("expected rename name"))
                                .to_owned(),
                        );
                    }
                    assert_eq!(request["params"], expected);
                }
                if expected_method != "initialized" {
                    if method == "thread/name/set"
                        && expected["name"] == "After send"
                        && expected_method == method
                    {
                        socket
                            .close(None)
                            .await
                            .unwrap_or_else(|error| panic!("close lost rename socket: {error}"));
                        break;
                    }
                    let response = if expected_method == method {
                        result.clone()
                    } else if expected_method == "thread/read" {
                        // Only the inventory read models a busy thread; the message paths
                        // branch on status and must keep their idle fixture.
                        let status = if method == "thread/loaded/list" {
                            json!({"type":"active","activeFlags":[]})
                        } else {
                            json!({"type":"idle"})
                        };
                        json!({"thread":{"id":"proof-thread","name":current_name,"title":valid_short_title,"cwd":"/tmp","status":status,"updatedAt":BUSY_THREAD_UPDATED_AT_SECONDS,"sandbox":{"type":"workspaceWrite"},"approvalPolicy":"on-request","approvalsReviewer":"auto_review"}})
                    } else if expected_method == "thread/turns/list" {
                        json!({"data":[{"id":"proof-turn","items":[{"type":"userMessage","id":"proof-user-message","content":[]}]}],"nextCursor":null,"backwardsCursor":null})
                    } else {
                        json!({})
                    };
                    socket
                        .send(Message::Text(
                            json!({"id":request["id"],"result":response})
                                .to_string()
                                .into(),
                        ))
                        .await
                        .unwrap_or_else(|error| panic!("send: {error}"));
                }
            }
        }
    });
    // Act: no request is sent for the empty sentinel; native inspection, message submission, queueing and interruption follow.
    let client = served
        .client("proof")
        .await
        .unwrap_or_else(|error| panic!("connect: {error}"));
    assert!(
        client
            .interrupt_turn(&target, &generation, "")
            .await
            .is_err()
    );
    let inspection = client
        .inspect_session(&target)
        .await
        .unwrap_or_else(|error| panic!("inspect: {error}"));
    assert_eq!(inspection.effective_access, None);
    assert!(matches!(
        inspection.settings_observation,
        collaboration_protocol::SettingsObservation::Unavailable {
            reason: collaboration_protocol::SettingsUnavailableReason::ThreadReadOmitsSettings
        }
    ));
    let inventory = client
        .list_sessions(collaboration_protocol::NativeSessionListParams {
            endpoint: target.endpoint.clone(),
            view: collaboration_protocol::NativeSessionView::Loaded,
            scope: collaboration_protocol::NativeSessionScope::Any,
            source: collaboration_protocol::NativeSessionSource::All,
            include_empty_sessions: false,
            query: None,
            page_size: 1,
            cursor: None,
        })
        .await
        .unwrap();
    assert_eq!(inventory.sessions.len(), 1);
    assert_eq!(inventory.sessions[0].target, target);
    assert_eq!(inventory.generation.as_ref(), Some(&generation));
    // A busy row reports the running status and a real idle time, never a hardcoded zero.
    let collaboration_protocol::NativeSessionObservation::Runtime { status, turn_id } =
        &inventory.sessions[0].observation
    else {
        panic!("a loaded row must carry a runtime observation");
    };
    assert_eq!(
        serde_json::to_value(status).unwrap_or_else(|error| panic!("status: {error}"))["type"],
        "active"
    );
    // ActiveThreadStatus in codex_app_server_protocol.v2.schemas.json names no turn id.
    assert_eq!(turn_id.as_deref(), None);
    assert!(
        inventory.sessions[0].idle_seconds > 0,
        "idle seconds must be computed from updatedAt, not hardcoded"
    );
    let renamed = client
        .rename_session(collaboration_protocol::NativeRenameParams {
            target: target.clone(),
            name: "🔎 Review".into(),
        })
        .await
        .unwrap_or_else(|error| panic!("rename: {error}"));
    assert_eq!(renamed.name, "🔎 Review");
    assert_eq!(renamed.previous_name, None);
    let inventory_after_rename = client
        .list_sessions(collaboration_protocol::NativeSessionListParams {
            endpoint: target.endpoint.clone(),
            view: collaboration_protocol::NativeSessionView::Loaded,
            scope: collaboration_protocol::NativeSessionScope::Any,
            source: collaboration_protocol::NativeSessionSource::All,
            include_empty_sessions: false,
            query: None,
            page_size: 1,
            cursor: None,
        })
        .await
        .unwrap_or_else(|error| panic!("sessions list after rename: {error}"));
    let interruption = client
        .interrupt_turn(&target, &generation, "proof-turn")
        .await
        .unwrap_or_else(|error| panic!("interrupt: {error}"));
    let mut stale = generation.clone();
    stale.generation = 2_u64
        .try_into()
        .unwrap_or_else(|error| panic!("generation: {error}"));
    let rejected = client.interrupt_turn(&target, &stale, "proof-turn").await;
    // A rejection leaves the client usable: each call is its own request.
    let lost_rename = client
        .rename_session(collaboration_protocol::NativeRenameParams {
            target: target.clone(),
            name: "After send".into(),
        })
        .await;
    drop(client);
    served
        .stop()
        .await
        .unwrap_or_else(|error| panic!("serve: {error}"));
    backend
        .await
        .unwrap_or_else(|error| panic!("backend: {error}"));
    if let Some(collaboration_protocol::ChannelDescription::NativeCodex { schema_digest, .. }) =
        description.channels.first_mut()
    {
        *schema_digest = None;
    }
    identity
        .endpoint_directory()
        .publish(description)
        .unwrap_or_else(|error| panic!("schema withdrawal: {error}"));
    let served = served_api::ServedApi::start(identity)
        .unwrap_or_else(|error| panic!("second serve: {error}"));
    let client = served
        .client("withdrawn-schema-proof")
        .await
        .unwrap_or_else(|error| panic!("second connect: {error}"));
    let unsupported = client.inspect_session(&target).await;
    drop(client);
    served
        .stop()
        .await
        .unwrap_or_else(|error| panic!("second serve: {error}"));
    std::fs::remove_file(backend_path).unwrap_or_else(|error| panic!("socket cleanup: {error}"));
    std::fs::remove_dir(root).unwrap_or_else(|error| panic!("directory cleanup: {error}"));
    // Assert.
    assert_eq!(inspection.target, target);
    assert_eq!(inspection.generation, generation);
    assert_eq!(
        interruption.kind,
        collaboration_protocol::NativeInterruptKind::InterruptCompleted
    );
    assert_eq!(
        inventory_after_rename.sessions[0].name.as_deref(),
        Some("🔎 Review")
    );
    assert!(
        matches!(lost_rename, Err(ClientError::Rejected { data: Some(data), .. })
        if data["kind"] == "outcomeUnknown"
            && data["message"].as_str().is_some_and(|message| message.contains("native app-server closed the socket")))
    );
    assert!(
        matches!(rejected, Err(ClientError::Rejected { data: Some(data), .. }) if data["kind"] == "staleGeneration")
    );
    assert!(
        matches!(unsupported, Err(ClientError::Rejected { data: Some(data), .. }) if data["kind"] == "unsupportedCapability")
    );
}

#[tokio::test]
async fn inspect_control_response_preserves_native_fake_rejection_message() {
    let root =
        std::env::temp_dir().join(format!("native-inspect-rejection-{}", std::process::id()));
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&root)
        .unwrap_or_else(|error| panic!("directory: {error}"));
    let backend_path = root.join("backend.sock");
    let backend_listener = tokio::net::UnixListener::bind(&backend_path)
        .unwrap_or_else(|error| panic!("backend: {error}"));
    let service_id = "00000000-0000-4000-8000-000000000001";
    let epoch = "00000000-0000-4000-8000-000000000002";
    let generation: CodexGeneration = serde_json::from_value(json!({
        "serviceEpoch":epoch,"generation":1
    }))
    .unwrap_or_else(|error| panic!("generation: {error}"));
    let target: SessionRef = serde_json::from_value(json!({
        "endpoint":{"serviceId":service_id,"endpointId":"codex-local"},
        "sessionId":"unreadable-thread"
    }))
    .unwrap_or_else(|error| panic!("target: {error}"));
    let mut definitions = serde_json::Map::new();
    for name in [
        "ThreadRead",
        "ThreadResume",
        "ThreadStart",
        "ThreadLoadedList",
        "TurnStart",
        "TurnSteer",
        "TurnInterrupt",
        "ThreadQueueAdd",
        "ThreadSetName",
    ] {
        definitions.insert(format!("{name}Params"), json!({"type":"object"}));
        definitions.insert(format!("{name}Response"), json!({"type":"object"}));
    }
    let bundle = codex_native_integration::NativeSchemaBundle::from_documents(BTreeMap::from([(
        "codex_app_server_protocol.schemas.json".to_owned(),
        serde_json::to_vec(&json!({"definitions":{"v2":definitions}}))
            .unwrap_or_else(|error| panic!("schema: {error}")),
    )]))
    .unwrap_or_else(|error| panic!("bundle: {error}"));
    let digest = format!(
        "sha256:{}",
        bundle
            .digest()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    let mut endpoint: EndpointDescription = serde_json::from_value(json!({
        "endpoint":target.endpoint,"label":"Fixture Codex",
        "availability":{"state":"available","observedAt":"2026-09-25T00:00:00Z"},
        "channels":[{"kind":"nativeCodex","transport":"unixWebSocket","path":"codex-native.sock",
            "schemaDigest":digest,"generation":generation}]
    }))
    .unwrap_or_else(|error| panic!("endpoint: {error}"));
    let schemas = Arc::new(
        codex_native_integration::NativePayloadSchemas::from_bundle(&bundle)
            .unwrap_or_else(|error| panic!("schemas: {error}")),
    );
    let gate = NativeGenerationGate::default();
    gate.activate(generation, backend_path.clone(), Some(schemas))
        .unwrap_or_else(|error| panic!("activate: {error}"));
    if let Some(collaboration_protocol::ChannelDescription::NativeCodex { schema_digest, .. }) =
        endpoint.channels.first_mut()
    {
        *schema_digest = Some(
            digest
                .try_into()
                .unwrap_or_else(|error| panic!("digest: {error}")),
        );
    }
    let native_backend = NativeControlBackend {
        codex_home: root.clone(),
        endpoint: target.endpoint.clone(),
        gate,
    };
    let identity = ApplicationFixture::new(service_id, epoch)
        .unwrap_or_else(|error| panic!("identity: {error}"))
        .with_endpoints(vec![endpoint])
        .unwrap_or_else(|error| panic!("endpoints: {error}"))
        .with_native_backend(native_backend)
        .unwrap_or_else(|error| panic!("native backend: {error}"));
    let served =
        served_api::ServedApi::start(identity).unwrap_or_else(|error| panic!("serve: {error}"));
    let native_task = tokio::spawn(async move {
        let (stream, _) = backend_listener.accept().await.expect("native accept");
        let mut socket = tokio_tungstenite::accept_async(stream)
            .await
            .expect("native upgrade");
        loop {
            let message = socket
                .next()
                .await
                .expect("native frame")
                .expect("native receive");
            let request: Value =
                serde_json::from_str(message.to_text().expect("native text")).expect("native JSON");
            match request["method"].as_str() {
                Some("initialize") => {
                    socket
                        .send(Message::Text(
                            json!({"jsonrpc":"2.0","id":request["id"],"result":{}})
                                .to_string()
                                .into(),
                        ))
                        .await
                        .expect("initialize response");
                }
                Some("initialized") => {}
                Some("thread/read") => {
                    assert_eq!(request["params"]["threadId"], "unreadable-thread");
                    let response = json!({"jsonrpc":"2.0","id":request["id"],"error":{
                        "code":-32600,"message":"native thread is unreadable: fixture refusal"
                    }});
                    socket
                        .send(Message::Text(response.to_string().into()))
                        .await
                        .expect("native rejection");
                    break;
                }
                method => panic!("unexpected native request {method:?}"),
            }
        }
    });

    let client = served
        .client("inspect-test")
        .await
        .unwrap_or_else(|error| panic!("connect: {error}"));
    let result = client.inspect_session(&target).await;

    assert!(
        matches!(result, Err(ClientError::Rejected { code: -32050, ref data })
        if data.as_ref().and_then(|data| data.get("message")).and_then(Value::as_str)
            == Some("native thread is unreadable: fixture refusal"))
    );
    native_task
        .await
        .unwrap_or_else(|error| panic!("native fake: {error}"));
    drop(client);
    served
        .stop()
        .await
        .unwrap_or_else(|error| panic!("serve: {error}"));
    std::fs::remove_file(backend_path).unwrap_or_else(|error| panic!("socket cleanup: {error}"));
    std::fs::remove_dir(root).unwrap_or_else(|error| panic!("directory cleanup: {error}"));
}
