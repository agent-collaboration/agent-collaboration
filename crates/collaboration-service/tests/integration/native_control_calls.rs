use collaboration_client::ClientError;
use collaboration_protocol::{CodexGeneration, EndpointDescription, SessionRef};
use collaboration_runtime::NativeControlBackend;
use collaboration_runtime::NativeGenerationGate;
use collaboration_service::ApplicationFixture;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{collections::BTreeMap, os::unix::fs::DirBuilderExt, sync::Arc};
use tokio_tungstenite::tungstenite::Message;

use crate::served_api;

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
