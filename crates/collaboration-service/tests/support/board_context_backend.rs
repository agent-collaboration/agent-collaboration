//! A native metadata stand-in; project routing and project reads stay real.
use super::TestError;
use collaboration_runtime::NativeControlBackend;
use collaboration_runtime::NativeGenerationGate;
use collaboration_service::ServiceIdentity;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio_tungstenite::tungstenite::Message;

pub struct BoardContextBackend {
    cancel: tokio_util::sync::CancellationToken,
    worker: tokio::task::JoinHandle<Result<(), TestError>>,
}
impl BoardContextBackend {
    pub async fn start(
        root: &Path,
        identity: ServiceIdentity,
    ) -> Result<(ServiceIdentity, Self), TestError> {
        let repository = create_context_repository(root)?;
        let socket = root.join("context-native.sock");
        let listener = tokio::net::UnixListener::bind(&socket)?;
        let generation: collaboration_protocol::CodexGeneration = serde_json::from_value(
            json!({"serviceEpoch":"00000000-0000-4000-8000-000000000002","generation":1}),
        )?;
        let endpoint: collaboration_protocol::EndpointRef = serde_json::from_value(
            json!({"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"}),
        )?;
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
        let bundle =
            codex_native_integration::NativeSchemaBundle::from_documents(BTreeMap::from([(
                "codex_app_server_protocol.schemas.json".to_owned(),
                serde_json::to_vec(&json!({"definitions":{"v2":definitions}}))?,
            )]))?;
        let schemas = Arc::new(codex_native_integration::NativePayloadSchemas::from_bundle(
            &bundle,
        )?);
        let description = serde_json::from_value(
            json!({"endpoint":endpoint,"label":"Context metadata stand-in","availability":{"state":"available","observedAt":"2026-10-13T00:00:00Z"},"channels":[{"kind":"nativeCodex","transport":"unixWebSocket","path":"context-native.sock","schemaDigest":schemas.schema_digest(),"generation":generation}]}),
        )?;
        let gate = NativeGenerationGate::default();
        gate.activate(generation, socket, Some(schemas))?;
        let identity = identity
            .with_endpoints(vec![description])?
            .with_native_backend(NativeControlBackend {
                endpoint,
                gate,
                codex_home: root.to_owned(),
            })?;
        let identity = identity.with_provider_operation_store(Arc::new(tokio::sync::Mutex::new(
            collaboration_runtime::ProviderOperationStore::open(
                &root.join("context-providers.sqlite"),
            )
            .await?,
        )));
        let cancel = tokio_util::sync::CancellationToken::new();
        let worker_cancel = cancel.clone();
        let worker = tokio::spawn(async move {
            loop {
                let stream = tokio::select! {_ = worker_cancel.cancelled()=>return Ok(()),result=listener.accept()=>result?.0};
                let mut socket = tokio_tungstenite::accept_async(stream).await?;
                loop {
                    let frame = tokio::select! {_ = worker_cancel.cancelled()=>return Ok(()),frame=socket.next()=>frame};
                    let Some(frame) = frame else {
                        break;
                    };
                    let frame = match frame {
                        Ok(frame)=>frame,
                        Err(tokio_tungstenite::tungstenite::Error::Protocol(tokio_tungstenite::tungstenite::error::ProtocolError::ResetWithoutClosingHandshake))=>break,
                        Err(error)=>return Err(error.into()),
                    };
                    if frame.is_close() {
                        break;
                    }
                    let request: Value = serde_json::from_str(frame.to_text()?)?;
                    let result = match request.get("method").and_then(Value::as_str) {
                        Some("initialize") => json!({}),
                        Some("initialized") => continue,
                        Some("thread/read")
                            if request.pointer("/params/includeTurns") == Some(&json!(false)) =>
                        {
                            json!({"thread":{"id":request.pointer("/params/threadId"),"cwd":repository}})
                        }
                        _ => return Err("unexpected context metadata operation".into()),
                    };
                    socket
                        .send(Message::Text(
                            json!({"id":request.get("id"),"result":result})
                                .to_string()
                                .into(),
                        ))
                        .await?;
                }
            }
        });
        Ok((identity, Self { cancel, worker }))
    }
    pub async fn stop(self) -> Result<(), TestError> {
        self.cancel.cancel();
        self.worker.await?
    }
}

fn create_context_repository(root: &Path) -> Result<PathBuf, TestError> {
    let repository = root.join("context-repository");
    std::fs::create_dir_all(&repository)?;
    let status = std::process::Command::new("git")
        .args(["init", "--quiet"])
        .arg(&repository)
        .status()?;
    if !status.success() {
        return Err("fixture git init failed".into());
    }
    Ok(repository)
}
