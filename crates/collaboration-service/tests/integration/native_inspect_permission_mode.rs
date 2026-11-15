//! Codex show reports the permission mode the Host recorded when it created the
//! session (R3), through the real native control entry and a scripted native
//! peer; a session the Host did not record shows its access only.

#![expect(
    clippy::expect_used,
    reason = "Invalid test fixtures must fail immediately instead of masking the failed setup."
)]

use codex_acp_adapter::ConversationOperationRecorder;
use collaboration_protocol::{
    CodexGeneration, CodexPermissionModeReport, EndpointDescription, NonEmptyText, OperationId,
    ProviderModeReason, RouterAccess, SessionId, SessionRef,
};
use collaboration_runtime::{
    CodexConversationOperationRecorder, NativeControlBackend, NativeGenerationGate,
    ProviderOperationStore,
};
use collaboration_service::ApplicationFixture;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{collections::BTreeMap, os::unix::fs::DirBuilderExt, sync::Arc};
use tokio_tungstenite::tungstenite::Message;

use crate::served_api;

const SERVICE: &str = "00000000-0000-4000-8000-000000000001";
const EPOCH: &str = "00000000-0000-4000-8000-000000000002";

fn schemas() -> (codex_native_integration::NativeSchemaBundle, String) {
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
        serde_json::to_vec(&json!({"definitions":{"v2":definitions}})).expect("schema"),
    )]))
    .expect("bundle");
    let digest = format!(
        "sha256:{}",
        bundle
            .digest()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    );
    (bundle, digest)
}

fn session(id: &str) -> SessionRef {
    serde_json::from_value(json!({
        "endpoint":{"serviceId":SERVICE,"endpointId":"codex-local"},"sessionId":id
    }))
    .expect("target")
}

/// Oracle: R3 and the Lead's show decision (push 01a1dc97): inspect returns the
/// create-time report for a session the Host created, and nothing invented for
/// one it did not.
#[tokio::test]
async fn codex_show_returns_the_create_time_permission_report() {
    // Arrange: a real operation store, recorder and native control entry.
    let root = std::env::temp_dir().join(format!("native-inspect-mode-{}", std::process::id()));
    let _stale = std::fs::remove_dir_all(&root);
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&root)
        .expect("directory");
    let backend_path = root.join("backend.sock");
    let backend_listener = tokio::net::UnixListener::bind(&backend_path).expect("backend");
    let generation: CodexGeneration =
        serde_json::from_value(json!({"serviceEpoch":EPOCH,"generation":1})).expect("generation");
    let created = session("created-thread");
    let unrecorded = session("unrecorded-thread");
    let (bundle, digest) = schemas();
    let endpoint: EndpointDescription = serde_json::from_value(json!({
        "endpoint":created.endpoint,"label":"Fixture Codex",
        "availability":{"state":"available","observedAt":"2026-09-25T00:00:00Z"},
        "channels":[{"kind":"nativeCodex","transport":"unixWebSocket","path":"codex-native.sock",
            "schemaDigest":digest,"generation":generation}]
    }))
    .expect("endpoint");
    let gate = NativeGenerationGate::default();
    gate.activate(
        generation.clone(),
        backend_path.clone(),
        Some(Arc::new(
            codex_native_integration::NativePayloadSchemas::from_bundle(&bundle).expect("schemas"),
        )),
    )
    .expect("activate");
    let store = Arc::new(tokio::sync::Mutex::new(
        ProviderOperationStore::open(&root.join("operations.sqlite"))
            .await
            .expect("store"),
    ));
    let recorder = Arc::new(CodexConversationOperationRecorder::new(
        Arc::clone(&store),
        created.endpoint.clone(),
        NonEmptyText::try_from("/tmp/codex-acp.sock".to_owned()).expect("listener"),
    ));
    let identity = ApplicationFixture::new(SERVICE, EPOCH)
        .expect("identity")
        .with_endpoints(vec![endpoint])
        .expect("endpoints")
        .with_native_backend(NativeControlBackend {
            codex_home: root.clone(),
            endpoint: created.endpoint.clone(),
            gate,
        })
        .expect("native backend")
        .with_provider_operation_store(Arc::clone(&store))
        .with_codex_conversation_recorder(Arc::clone(&recorder));
    let served = served_api::ServedApi::start(identity).expect("serve");
    let native = tokio::spawn(async move {
        // Each inspect opens its own native connection and reads one thread.
        for _ in 0..2 {
            let (stream, _) = backend_listener.accept().await.expect("native accept");
            let mut socket = tokio_tungstenite::accept_async(stream)
                .await
                .expect("native upgrade");
            loop {
                let message = socket.next().await.expect("frame").expect("receive");
                let request: Value =
                    serde_json::from_str(message.to_text().expect("text")).expect("json");
                let (result, read) = match request["method"].as_str() {
                    Some("initialize") => (json!({}), false),
                    Some("initialized") => continue,
                    Some("thread/read") => (
                        json!({"thread":{"id":request["params"]["threadId"],"status":{"type":"idle"},"turns":[]}}),
                        true,
                    ),
                    method => panic!("unexpected native request {method:?}"),
                };
                socket
                    .send(Message::Text(
                        json!({"jsonrpc":"2.0","id":request["id"],"result":result})
                            .to_string()
                            .into(),
                    ))
                    .await
                    .expect("native reply");
                if read {
                    break;
                }
            }
        }
    });
    let report = CodexPermissionModeReport {
        access: RouterAccess::WorkspaceWrite,
        requested_mode: Some("auto".to_owned()),
        actual: Some("workspace-write".to_owned()),
        reason: Some(ProviderModeReason::AutoNotApplied),
    };
    let operation_id = OperationId::generate();
    recorder
        .admit_create(&operation_id, &generation)
        .await
        .expect("admit");
    recorder
        .before_native_dispatch(&operation_id)
        .await
        .expect("dispatch");
    recorder
        .record_created(
            &operation_id,
            &SessionId::try_from("created-thread".to_owned()).expect("session"),
            &report,
        )
        .await
        .expect("record");
    let client = served.client("inspect-mode").await.expect("connect");

    // Act
    let shown = client
        .inspect_session(&created)
        .await
        .expect("inspect created");
    let unknown = client
        .inspect_session(&unrecorded)
        .await
        .expect("inspect unrecorded");

    // Assert: the created session shows its report; the other shows none.
    assert_eq!(shown.effective_settings, Some(report.effective_settings()));
    assert_eq!(
        serde_json::to_value(&shown).expect("json")["effectiveSettings"]["modeReason"],
        "autoNotApplied"
    );
    assert_eq!(unknown.effective_settings, None);
    assert!(
        serde_json::to_value(&unknown)
            .expect("json")
            .get("effectiveSettings")
            .is_none()
    );
    native.await.expect("native fake");
    drop(client);
    served.stop().await.expect("stop");
    let _removed = std::fs::remove_dir_all(&root);
}
