use crate::native_permission_echo;
use agent_automation::{
    ClaudeCodePeerEffectEvidence, PeerProcessId, PeerSessionReference, PeerWriteEffect,
    RouteEffectEvidence, SubmissionEffect,
};
use collaboration_contracts::AttemptEvidenceSink;
use collaboration_contracts::AttemptReconciliation;
use collaboration_contracts::AttemptReconciliationContext;
use collaboration_contracts::DeliveryContractError;
use collaboration_contracts::DeliveryFuture;
use collaboration_contracts::DeliveryPrecondition;
use collaboration_contracts::LoadPolicy;
use collaboration_contracts::SessionDeliveryRoute;
use collaboration_contracts::SessionMessageDelivery;
use collaboration_contracts::layer_zero::DeliveryRequest as PreparedDeliveryRequest;
use collaboration_contracts::layer_zero::PreparedPush;
use collaboration_protocol::DeliveryClientReceipt;
use collaboration_protocol::{
    CodexGeneration, DeliveryCorrelationId, DeliveryOutcome, EndpointDescription, MessageDelivery,
    MessageInputKind, MessageRepresentation, MessageText, PushId, SessionRef, UuidIdentity,
};
use collaboration_runtime::CodexAppServerDeliveryRoute;
use collaboration_runtime::EndpointDirectory;
use collaboration_runtime::NativeControlBackend;
use collaboration_runtime::NativeGenerationGate;
use collaboration_runtime::SessionDeliveryRouter;
use futures_util::{SinkExt, StreamExt};
use native_permission_echo::applied_router_sandbox;
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    os::unix::fs::DirBuilderExt,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio_tungstenite::tungstenite::Message;

struct AcceptingConversationRecorder;
impl codex_acp_adapter::ConversationOperationRecorder for AcceptingConversationRecorder {
    fn admit_create<'a>(
        &'a self,
        _: &'a collaboration_protocol::OperationId,
        _: &'a CodexGeneration,
    ) -> codex_acp_adapter::ConversationRecordFuture<'a> {
        Box::pin(async { Ok(()) })
    }
    fn before_native_dispatch<'a>(
        &'a self,
        _: &'a collaboration_protocol::OperationId,
    ) -> codex_acp_adapter::ConversationRecordFuture<'a> {
        Box::pin(async { Ok(()) })
    }
    fn record_created<'a>(
        &'a self,
        _: &'a collaboration_protocol::OperationId,
        _: &'a collaboration_protocol::SessionId,
        _permission_mode: &'a collaboration_protocol::CodexPermissionModeReport,
    ) -> codex_acp_adapter::ConversationRecordFuture<'a> {
        Box::pin(async { Ok(()) })
    }
    fn record_failure<'a>(
        &'a self,
        _: &'a collaboration_protocol::OperationId,
        _: bool,
    ) -> codex_acp_adapter::ConversationRecordFuture<'a> {
        Box::pin(async { Ok(()) })
    }
}

struct CountingEvidenceSink(AtomicUsize);

impl AttemptEvidenceSink for CountingEvidenceSink {
    fn record(
        &self,
        _: RouteEffectEvidence<SessionRef, CodexGeneration>,
    ) -> DeliveryFuture<'_, ()> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(()) })
    }
}

struct RecordingEvidenceSink(Mutex<Vec<RouteEffectEvidence<SessionRef, CodexGeneration>>>);

impl AttemptEvidenceSink for RecordingEvidenceSink {
    fn record(
        &self,
        evidence: RouteEffectEvidence<SessionRef, CodexGeneration>,
    ) -> DeliveryFuture<'_, ()> {
        let result = self
            .0
            .lock()
            .map(|mut records| records.push(evidence))
            .map_err(|_| collaboration_contracts::DeliveryContractError::EvidencePersistence);
        Box::pin(async move { result })
    }
}

fn prepared_request(
    target: SessionRef,
    body: &str,
    mode: MessageDelivery,
) -> Result<PreparedDeliveryRequest, Box<dyn std::error::Error + Send + Sync>> {
    let push_id = PushId::try_from(agent_automation::AttemptId::generate().as_str().to_owned())?;
    let correlation = DeliveryCorrelationId::try_from(push_id.as_str().to_owned())?;
    let line = MessageText::try_from(format!(
        "✉️ sender · \"{body}\" · router://{}/push/{}",
        String::from(target.endpoint.service_id.clone()),
        push_id.as_str()
    ))?;
    Ok(PreparedDeliveryRequest {
        payload: PreparedPush {
            push_id,
            line,
            load_policy: LoadPolicy::MayLoad,
        },
        target,
        mode,
        precondition: DeliveryPrecondition::Unpinned,
        correlation,
        attempt: agent_automation::AttemptId::generate(),
    })
}

#[path = "codex_app_server_delivery_route/generation_tests.rs"]
mod generation_tests;

#[tokio::test]
async fn held_empty_codex_thread_rejects_steer_then_starts_first_message_on_its_connection()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    exercise_held_empty_thread().await
}

async fn exercise_held_empty_thread() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use codex_acp_adapter::{AcpConnectionInputs, AcpStoredSessions, serve_acp_connection};
    use std::{future::Future, io, pin::Pin};
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    struct EmptyStoredSessions;
    impl AcpStoredSessions for EmptyStoredSessions {
        fn list(&self, _: Value) -> Pin<Box<dyn Future<Output = io::Result<Value>> + Send + '_>> {
            Box::pin(async { Ok(json!({"sessions":[]})) })
        }
    }
    let root_id = uuid::Uuid::now_v7().to_string();
    let root_suffix = root_id
        .get(24..)
        .ok_or("root id must have an ASCII UUID suffix")?;
    let root = std::env::temp_dir().join(format!("held-{root_suffix}"));
    std::fs::DirBuilder::new().mode(0o700).create(&root)?;
    let scratch_scope = "session-00000000-0000-4000-8000-000000000099";
    let scratch_parent = root.join("scratch");
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&scratch_parent)?;
    let scratch = scratch_parent.join(scratch_scope);
    std::fs::DirBuilder::new().mode(0o700).create(&scratch)?;
    let socket_path = root.join("native.sock");
    let listener = tokio::net::UnixListener::bind(&socket_path)?;
    let service_id = UuidIdentity::try_from("00000000-0000-4000-8000-000000000001".to_owned())?;
    let target: SessionRef = serde_json::from_value(json!({
        "endpoint":{"serviceId":service_id,"endpointId":"codex-local"},"sessionId":"empty-thread"
    }))?;
    let generation: CodexGeneration =
        serde_json::from_value(json!({"serviceEpoch":service_id,"generation":1}))?;
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
    let description: EndpointDescription = serde_json::from_value(json!({
        "endpoint":target.endpoint,"label":"Fixture","availability":{"state":"available","observedAt":"2026-09-24T00:00:00Z"},
        "channels":[{"kind":"nativeCodex","transport":"unixWebSocket","path":"native.sock","schemaDigest":schemas.schema_digest(),"generation":generation}]
    }))?;
    let directory = EndpointDirectory::new(service_id.clone());
    directory.publish(description.clone())?;
    let gate = NativeGenerationGate::default();
    gate.activate(
        generation.clone(),
        socket_path.clone(),
        Some(Arc::clone(&schemas)),
    )?;
    let holder = Arc::new(collaboration_runtime::UnmaterializedThreadHolder::new());
    let backend_config = NativeControlBackend {
        endpoint: target.endpoint.clone(),
        gate,
        codex_home: root.clone(),
    };
    let route = Arc::new(CodexAppServerDeliveryRoute::new(
        service_id.clone(),
        directory,
        backend_config,
        Arc::clone(&holder),
    ));
    let evidence = Arc::new(RecordingEvidenceSink(Mutex::new(Vec::new())));
    let observed = Arc::clone(&evidence);
    let (delivery_prepared, mut delivery_ready) = tokio::sync::oneshot::channel();
    let backend = tokio::spawn(async move {
        let (stream, _) = listener.accept().await?;
        let mut wire = tokio_tungstenite::accept_async(stream).await?;
        let methods: &[&str] = &[
            "initialize",
            "initialized",
            "thread/start",
            "thread/queue/add",
            "turn/start",
        ];
        for &method in methods {
            if method == "turn/start" {
                // Real app-server connections remain idle during local store
                // setup. Start the RPC deadline when the caller can send it.
                tokio::time::timeout(Duration::from_secs(30), &mut delivery_ready).await??;
            }
            let frame = tokio::time::timeout(Duration::from_secs(3), wire.next())
                .await?
                .ok_or("native connection closed")??;
            let request: Value = serde_json::from_str(frame.to_text()?)?;
            if request.get("method").and_then(Value::as_str) != Some(method) {
                return Err::<(), Box<dyn std::error::Error + Send + Sync>>(
                    format!("unexpected native method: {:?}", request.get("method")).into(),
                );
            }
            if method == "initialized" {
                continue;
            }
            if method == "turn/start" {
                let effects = observed.0.lock().map_err(|_| "evidence lock")?;
                if !effects.iter().any(|effect| {
                    matches!(effect, RouteEffectEvidence::CodexAppServer(native)
                    if native.submission == SubmissionEffect::Dispatching)
                }) {
                    return Err("held message reached native I/O before evidence".into());
                }
            }
            let result = match method {
                "initialize" => json!({}),
                "thread/start" => json!({
                    "cwd":"/work","model":"gpt-5.6-sol","approvalPolicy":"on-request","approvalsReviewer":"auto_review",
                    "activePermissionProfile":{"id":"router-workspace-write","extends":":workspace"},
                    "sandbox":applied_router_sandbox(&request),
                    "thread":{"id":"empty-thread","cwd":"/work","turns":[]}
                }),
                "thread/queue/add" => json!({"queuedSubmission":{"id":"queued-one"}}),
                _ => json!({"turn":{"id":"first-turn"}}),
            };
            let response = json!({"id":request.get("id"),"result":result});
            wire.send(Message::Text(response.to_string().into()))
                .await?;
        }
        if tokio::time::timeout(Duration::from_millis(25), listener.accept())
            .await
            .is_ok()
        {
            return Err("held thread delivery opened a second native connection".into());
        }
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    });
    let (client, server) = tokio::net::UnixStream::pair()?;
    let serving = tokio::spawn(serve_acp_connection(
        server,
        AcpConnectionInputs {
            scratch_scopes: std::sync::Arc::new(codex_acp_adapter::test_support::recording_scratch_scopes::RecordingScratchScopes::default()),
            backend_path: socket_path.clone(),
            generation,
            schemas: Arc::clone(&schemas),
            stored_sessions: Arc::new(EmptyStoredSessions),
            holder: holder.clone(),
            recorder: Arc::new(AcceptingConversationRecorder),
            retired: tokio_util::sync::CancellationToken::new(),
        },
    ));
    let (read, mut write) = client.into_split();
    let mut read = BufReader::new(read);
    write.write_all(b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{\"protocolVersion\":1}}\n").await?;
    let mut response = String::new();
    read.read_line(&mut response).await?;
    write
        .write_all(
            format!(
                "{}\n",
                json!({"jsonrpc":"2.0","id":2,"method":"session/new","params":{
                    "cwd":"/work","mcpServers":[],"_meta":{"codexRouter":{
                        "operationId":collaboration_protocol::OperationId::generate(),
                        "model":"gpt-5.6-sol","effort":"medium","access":"workspace-write",
                        "scratchScope":scratch_scope,"scratchPath":scratch,
                        "createdBy":target,"approver":target
                    }}
                }})
            )
            .as_bytes(),
        )
        .await?;
    response.clear();
    tokio::time::timeout(Duration::from_secs(3), read.read_line(&mut response)).await??;
    let created: Value = serde_json::from_str(&response)?;
    if created.pointer("/result/sessionId").and_then(Value::as_str) != Some("empty-thread") {
        return Err(format!("session/new failed: {created}").into());
    }
    write.shutdown().await?;
    serving.await??;
    if !holder.contains("empty-thread") {
        return Err("closing session/new did not hold its empty native thread".into());
    }
    let make_request = |mode| prepared_request(target.clone(), "hello", mode);
    let steer = route
        .deliver(make_request(MessageDelivery::Steer)?, evidence.as_ref())
        .await?;
    if !matches!(
        steer.receipt().outcome.clone(),
        DeliveryOutcome::NotSubmitted { .. }
    ) || !holder.contains("empty-thread")
    {
        return Err("steer consumed the unmaterialized binding".into());
    }
    let queued = route
        .deliver(make_request(MessageDelivery::Queue)?, evidence.as_ref())
        .await?;
    if !matches!(queued.receipt().outcome.clone(), DeliveryOutcome::Queued)
        || !holder.contains("empty-thread")
    {
        return Err("queue did not use and retain the held native connection".into());
    }
    let routed: Arc<dyn SessionDeliveryRoute> = route;
    let delivery = collaboration_runtime::SessionDeliveryRouter::new(vec![routed]);
    delivery_prepared
        .send(())
        .map_err(|_| "native fixture stopped before delivery setup")?;
    let started = collaboration_contracts::SessionMessageDelivery::deliver(
        &delivery,
        make_request(MessageDelivery::Auto)?,
        evidence.as_ref(),
    )
    .await?
    .into_receipt();
    if !matches!(started.outcome, DeliveryOutcome::Started)
        || !matches!(
            started.client,
            Some(DeliveryClientReceipt::CodexAppServer(_))
        )
        || holder.contains("empty-thread")
    {
        return Err("first message did not start through the held connection".into());
    }
    backend.await??;
    std::fs::remove_file(socket_path)?;
    std::fs::remove_dir(scratch)?;
    std::fs::remove_dir(scratch_parent)?;
    std::fs::remove_dir(root)?;
    Ok(())
}
