//! Shared native socket and schema fixtures for storeless delivery.
use codex_native_integration::{NativePayloadSchemas, NativeSchemaBundle};
use collaboration_contracts::LoadPolicy;
use collaboration_protocol::{CodexGeneration, EndpointDescription, MessageDelivery, SessionRef};
use collaboration_runtime::NativeControlBackend;
use collaboration_runtime::NativeGenerationGate;
use collaboration_runtime::test_support::native_message_dispatch::{
    NativeMessageBody, NativeMessageOutcome, NativeMessageParams, NativeMessageRequest,
    dispatch_message,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::Path, sync::Arc, time::Duration};
use tokio_tungstenite::tungstenite::Message;

type TestError = Box<dyn std::error::Error + Send + Sync>;
const SERVICE: &str = "3d26f886-1489-423f-bc3d-6c0c940ddbb2";
const EPOCH: &str = "110a92d0-e607-4947-86ca-45838ac90a96";
const THREAD: &str = "recorded-cold-thread";

fn backend(root: &Path) -> (NativeControlBackend, CodexGeneration, EndpointDescription) {
    backend_with_queue_support(root, true)
}

fn backend_with_queue_support(
    root: &Path,
    queue_supported: bool,
) -> (NativeControlBackend, CodexGeneration, EndpointDescription) {
    let mut definitions = serde_json::Map::new();
    for name in [
        "ThreadRead",
        "ThreadResume",
        "ThreadStart",
        "ThreadLoadedList",
        "TurnStart",
        "TurnSteer",
        "TurnInterrupt",
        "ThreadTurnsList",
        "ThreadQueueAdd",
        "ThreadQueueList",
    ] {
        if name == "ThreadQueueAdd" && !queue_supported {
            continue;
        }
        definitions.insert(format!("{name}Params"), json!({"type":"object"}));
        definitions.insert(format!("{name}Response"), json!({"type":"object"}));
    }
    let bundle = NativeSchemaBundle::from_documents(BTreeMap::from([(
        "codex_app_server_protocol.schemas.json".into(),
        serde_json::to_vec(&json!({"definitions":{"v2":definitions}})).unwrap(),
    )]))
    .unwrap();
    let schemas = Arc::new(NativePayloadSchemas::from_bundle(&bundle).unwrap());
    let generation: CodexGeneration =
        serde_json::from_value(json!({"serviceEpoch":EPOCH,"generation":1})).unwrap();
    let endpoint =
        serde_json::from_value(json!({"serviceId":SERVICE,"endpointId":"codex-local"})).unwrap();
    let description = serde_json::from_value(json!({"endpoint":endpoint,"label":"fixture","availability":{"state":"available","observedAt":"2026-10-11T00:00:00Z"},"channels":[{"kind":"nativeCodex","transport":"unixWebSocket","path":"native.sock","schemaDigest":schemas.schema_digest(),"generation":generation}]})).unwrap();
    let gate = NativeGenerationGate::default();
    gate.activate(generation.clone(), root.join("native.sock"), Some(schemas))
        .unwrap();
    (
        NativeControlBackend {
            endpoint,
            gate,
            codex_home: root.to_owned(),
        },
        generation,
        description,
    )
}

fn target() -> SessionRef {
    serde_json::from_value(
        json!({"endpoint":{"serviceId":SERVICE,"endpointId":"codex-local"},"sessionId":THREAD}),
    )
    .unwrap()
}

#[path = "native_loaded_permission_tests.rs"]
mod loaded_followup_tests;
