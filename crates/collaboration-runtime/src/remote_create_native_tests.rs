use super::RemoteCreateAdmissionError::Refused;
use super::{LOCAL, admission, session, work};
use crate::{NativeControlBackend, NativeGenerationGate, PreparedRemoteCreateActors};
use codex_native_integration::{NativePayloadSchemas, NativeSchemaBundle};
use collaboration_protocol::{
    ConversationOperationFailureKind, ProviderOperationEffect, ProviderRequestedSettings,
    RemoteCreateRefusal,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio_util::sync::CancellationToken;

struct NativeModelsFixture {
    _root: tempfile::TempDir,
    backend: NativeControlBackend,
    stop: CancellationToken,
    server: tokio::task::JoinHandle<Vec<String>>,
}

impl NativeModelsFixture {
    fn start() -> Self {
        let root = tempfile::tempdir_in("/tmp").expect("short private fixture directory");
        let path = root.path().join("models.sock");
        let listener = tokio::net::UnixListener::bind(&path).expect("native model socket");
        let mut definitions = serde_json::Map::new();
        for method in [
            "ThreadRead",
            "ThreadResume",
            "ThreadStart",
            "ThreadLoadedList",
            "TurnStart",
            "TurnSteer",
            "TurnInterrupt",
            "ModelList",
        ] {
            definitions.insert(format!("{method}Params"), json!({"type":"object"}));
            definitions.insert(format!("{method}Response"), json!({"type":"object"}));
        }
        let bundle = NativeSchemaBundle::from_documents(BTreeMap::from([(
            "codex_app_server_protocol.schemas.json".to_owned(),
            serde_json::to_vec(&json!({"definitions":{"v2":definitions}})).expect("schema JSON"),
        )]))
        .expect("native fixture schemas");
        let gate = NativeGenerationGate::default();
        gate.activate(
            serde_json::from_value(json!({"serviceEpoch":LOCAL,"generation":1}))
                .expect("generation"),
            path,
            Some(Arc::new(
                NativePayloadSchemas::from_bundle(&bundle).expect("schema validators"),
            )),
        )
        .expect("activate native generation");
        let backend = NativeControlBackend {
            endpoint: session(LOCAL).endpoint,
            gate,
            codex_home: root.path().to_path_buf(),
        };
        let stop = CancellationToken::new();
        let server_stop = stop.clone();
        let server = tokio::spawn(async move {
            let mut methods = Vec::new();
            let accepted = tokio::select! { _=server_stop.cancelled()=>return methods, result=listener.accept()=>result };
            let (stream, _) = accepted.expect("native connection");
            let mut socket = tokio_tungstenite::accept_async(stream)
                .await
                .expect("native websocket");
            loop {
                let next =
                    tokio::select! { _=server_stop.cancelled()=>break, next=socket.next()=>next };
                let Some(Ok(message)) = next else { break };
                let tokio_tungstenite::tungstenite::Message::Text(text) = message else {
                    continue;
                };
                let request: Value = serde_json::from_str(&text).expect("native request JSON");
                let method = request["method"]
                    .as_str()
                    .expect("native method")
                    .to_owned();
                methods.push(method.clone());
                if request.get("id").is_none() {
                    continue;
                }
                let response = match method.as_str() {
                    "initialize" => json!({"id":request["id"],"result":{}}),
                    "model/list" => {
                        json!({"id":request["id"],"result":{"data":[{"id":"catalog-row","model":"fixture-model","supportedReasoningEfforts":[{"reasoningEffort":"low","description":"fixture"}]}],"nextCursor":null}})
                    }
                    _ => {
                        json!({"id":request["id"],"error":{"code":-32601,"message":"unexpected operation"}})
                    }
                };
                socket
                    .send(tokio_tungstenite::tungstenite::Message::Text(
                        response.to_string().into(),
                    ))
                    .await
                    .expect("native response");
            }
            methods
        });
        Self {
            _root: root,
            backend,
            stop,
            server,
        }
    }

    async fn finish(self) -> Vec<String> {
        self.stop.cancel();
        tokio::time::timeout(Duration::from_secs(10), self.server)
            .await
            .expect("native server stops")
            .expect("native server succeeds")
    }
}

#[tokio::test]
async fn advertised_native_model_and_effort_are_checked_without_creating_a_thread() {
    let fixture = NativeModelsFixture::start();
    let root = tempfile::tempdir().expect("working directory");
    let mut entry = work(root.path(), true);
    entry.request.settings = Some(ProviderRequestedSettings {
        mode: None,
        model: Some("fixture-model".to_owned()),
        effort: Some("low".to_owned()),
    });
    let mut policy = admission(Some(session(LOCAL)), true);
    policy.inputs.native_backend = Some(fixture.backend.clone());
    let result = tokio::time::timeout(
        Duration::from_secs(10),
        policy.prepare_authenticated_work(&entry),
    )
    .await
    .expect("preflight returns");
    let methods = fixture.finish().await;
    assert!(result.is_ok(), "advertised native choice: {result:?}");
    let prepared = result.expect("prepared native request");
    let PreparedRemoteCreateActors::Codex { generation, .. } = prepared.actors() else {
        panic!("native actors required")
    };
    assert_eq!(u64::from(generation.generation), 1);
    assert_eq!(methods, ["initialize", "initialized", "model/list"]);
}

#[tokio::test]
async fn unadvertised_native_choices_are_refused_without_creating_a_thread() {
    for (model, effort) in [
        ("missing-model", "low"),
        ("fixture-model", "missing-effort"),
    ] {
        let fixture = NativeModelsFixture::start();
        let root = tempfile::tempdir().expect("working directory");
        let mut entry = work(root.path(), true);
        entry.request.settings = Some(ProviderRequestedSettings {
            mode: None,
            model: Some(model.to_owned()),
            effort: Some(effort.to_owned()),
        });
        let mut policy = admission(Some(session(LOCAL)), true);
        policy.inputs.native_backend = Some(fixture.backend.clone());
        let result = tokio::time::timeout(
            Duration::from_secs(10),
            policy.prepare_authenticated_work(&entry),
        )
        .await
        .expect("preflight returns");
        let methods = fixture.finish().await;
        let Err(Refused(RemoteCreateRefusal::Local { failure })) = result else {
            panic!("unadvertised choice must be refused")
        };
        assert_eq!(
            failure.kind,
            ConversationOperationFailureKind::InvalidSetting
        );
        assert_eq!(failure.effect, ProviderOperationEffect::None);
        assert_eq!(methods, ["initialize", "initialized", "model/list"]);
    }
}
