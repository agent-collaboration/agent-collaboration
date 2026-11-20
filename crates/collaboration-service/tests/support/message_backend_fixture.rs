//! Scripted socket for testing the real Codex delivery route.
use collaboration_contracts::{
    DeliveryContractError, DeliveryPrecondition, LoadPolicy, SessionDeliveryRoute,
    UnstoredAttemptEvidenceSink,
    layer_zero::{DeliveryRequest, PreparedPush},
};
use collaboration_protocol::{
    CodexGeneration, DeliveryReceipt, EndpointDescription, MessageDelivery, SessionRef,
};
use collaboration_runtime::{
    CodexAppServerDeliveryRoute, EndpointDirectory, NativeControlBackend, NativeGenerationGate,
    UnmaterializedThreadHolder,
};
use futures_util::{SinkExt as _, StreamExt as _};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio_tungstenite::tungstenite::Message;

pub enum NativeReply {
    Result(Value),
    NotificationThenResult { notification: Value, result: Value },
    Reject,
    RejectWith { code: i64, message: &'static str },
    Disconnect,
}
pub struct NativeStep {
    pub method: &'static str,
    pub reply: NativeReply,
}
pub struct MessageScenario {
    pub delivery: MessageDelivery,
    pub steps: Vec<NativeStep>,
}
type FixtureError = Box<dyn std::error::Error + Send + Sync>;

pub async fn exercise(
    scenario: MessageScenario,
) -> Result<(Result<DeliveryReceipt, DeliveryContractError>, Vec<Value>), FixtureError> {
    // The fixture owns its native socket.
    let directory = tempfile::Builder::new().prefix("mf-").tempdir()?;
    let root = directory.path().to_path_buf();
    let socket_path = root.join("n.sock");
    let listener = tokio::net::UnixListener::bind(&socket_path)?;
    let service_id = "00000000-0000-4000-8000-000000000001";
    let generation: CodexGeneration =
        serde_json::from_value(json!({"serviceEpoch":service_id,"generation":1}))?;
    let target: SessionRef = serde_json::from_value(
        json!({"endpoint":{"serviceId":service_id,"endpointId":"codex-local"},"sessionId":"target"}),
    )?;
    let mut definitions = serde_json::Map::new();
    for name in [
        "ThreadRead",
        "ThreadResume",
        "ThreadStart",
        "ThreadTurnsList",
        "ThreadLoadedList",
        "TurnStart",
        "TurnSteer",
        "TurnInterrupt",
        "ThreadQueueAdd",
    ] {
        definitions.insert(format!("{name}Params"), json!({"type":"object"}));
        definitions.insert(format!("{name}Response"), json!({"type":"object"}));
    }
    let bundle = codex_native_integration::NativeSchemaBundle::from_documents(BTreeMap::from([(
        "codex_app_server_protocol.schemas.json".to_owned(),
        serde_json::to_vec(&json!({"definitions":{"v2":definitions}}))?,
    )]))?;
    let schemas = Arc::new(codex_native_integration::NativePayloadSchemas::from_bundle(
        &bundle,
    )?);
    let description: EndpointDescription = serde_json::from_value(
        json!({"endpoint":target.endpoint,"label":"Fixture","availability":{"state":"available","observedAt":"2026-09-06T00:00:00Z"},"channels":[{"kind":"nativeCodex","transport":"unixWebSocket","path":"codex-native.sock","schemaDigest":schemas.schema_digest(),"generation":generation}]}),
    )?;
    let gate = NativeGenerationGate::default();
    gate.activate(generation.clone(), socket_path.clone(), Some(schemas))?;
    let native_backend = NativeControlBackend {
        codex_home: root.clone(),
        endpoint: target.endpoint.clone(),
        gate,
    };

    let endpoints = EndpointDirectory::new(target.endpoint.service_id.clone());
    endpoints.publish(description)?;
    let route = CodexAppServerDeliveryRoute::new(
        target.endpoint.service_id.clone(),
        endpoints,
        native_backend,
        Arc::new(UnmaterializedThreadHolder::new()),
    );
    let mode = scenario.delivery;
    let backend = tokio::spawn(async move {
        let (stream, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept()).await??;
        let mut socket = tokio_tungstenite::accept_async(stream).await?;
        let init: Value = serde_json::from_str(
            socket
                .next()
                .await
                .ok_or("missing native frame")??
                .to_text()?,
        )?;
        assert_eq!(
            init.get("method").and_then(Value::as_str),
            Some("initialize")
        );
        socket
            .send(Message::Text(
                json!({"id":init.get("id").ok_or("missing initialization ID")?,"result":{}})
                    .to_string()
                    .into(),
            ))
            .await?;
        let initialized: Value = serde_json::from_str(
            socket
                .next()
                .await
                .ok_or("missing native frame")??
                .to_text()?,
        )?;
        assert_eq!(
            initialized.get("method").and_then(Value::as_str),
            Some("initialized")
        );
        let mut requests = Vec::new();
        for step in scenario.steps {
            let frame = match tokio::time::timeout(Duration::from_secs(2), socket.next()).await? {
                Some(Ok(frame)) => frame,
                _ => return Err("native socket closed before expected operation".into()),
            };
            let request: Value = serde_json::from_str(frame.to_text()?)?;
            assert_eq!(
                request.get("method").and_then(Value::as_str),
                Some(step.method)
            );
            let response = match step.reply {
                NativeReply::Result(result) => {
                    json!({"id":request.get("id").ok_or("missing request ID")?,"result":result})
                }
                NativeReply::NotificationThenResult {
                    notification,
                    result,
                } => {
                    socket
                        .send(Message::Text(notification.to_string().into()))
                        .await?;
                    json!({"id":request.get("id").ok_or("missing request ID")?,"result":result})
                }
                NativeReply::Reject => {
                    json!({"id":request.get("id").ok_or("missing request ID")?,"error":{"code":-32602,"message":"Fixture rejection"}})
                }
                NativeReply::RejectWith { code, message } => {
                    json!({"id":request.get("id").ok_or("missing request ID")?,"error":{"code":code,"message":message}})
                }
                NativeReply::Disconnect => {
                    requests.push(request);
                    return Ok::<_, FixtureError>(requests);
                }
            };
            requests.push(request);
            socket
                .send(Message::Text(response.to_string().into()))
                .await?;
        }
        // No fallback, extra mutation or retry may follow the scripted terminal response.
        if let Some(Ok(frame)) = tokio::time::timeout(Duration::from_secs(2), socket.next()).await?
        {
            assert!(frame.is_close(), "unexpected extra native frame: {frame}");
        }
        Ok::<_, FixtureError>(requests)
    });

    let id = uuid::Uuid::now_v7().to_string();
    let request = DeliveryRequest {
        payload: PreparedPush {
            push_id: id.clone().try_into()?,
            line: format!("Fixture message · router://{service_id}/push/{id}").try_into()?,
            load_policy: LoadPolicy::MayLoad,
        },
        target,
        mode,
        precondition: DeliveryPrecondition::EndpointGeneration {
            expected: generation,
        },
        correlation: id.try_into()?,
        attempt: agent_automation::AttemptId::generate(),
    };
    let result = tokio::time::timeout(
        Duration::from_secs(3),
        route.deliver(request, &UnstoredAttemptEvidenceSink),
    )
    .await?
    .map(|result| result.into_receipt());
    let requests = backend.await??;
    Ok((result, requests))
}

pub fn read(status: &str) -> NativeStep {
    NativeStep {
        method: "thread/read",
        reply: NativeReply::Result(json!({"thread":{"id":"target","status":{"type":status}}})),
    }
}
pub fn outcome_data(
    result: Result<DeliveryReceipt, DeliveryContractError>,
) -> Result<Value, &'static str> {
    match result {
        Ok(receipt) => serde_json::to_value(receipt.outcome).map_err(|_| "invalid outcome"),
        Err(_) => Err("expected a typed delivery outcome"),
    }
}
