//! Scripted native socket for loaded and unloaded delivery scenarios.
use super::*;

pub(super) struct NativeDeliveryFixture {
    pub(super) directory: tempfile::TempDir,
    pub(super) backend: NativeControlBackend,
    pub(super) generation: CodexGeneration,
    pub(super) endpoint: EndpointDescription,
}

impl NativeDeliveryFixture {
    pub(super) fn create() -> Result<Self, TestError> {
        let directory = tempfile::tempdir()?;
        let (backend, generation, endpoint) = backend(directory.path());
        Ok(Self {
            directory,
            backend,
            generation,
            endpoint,
        })
    }

    pub(super) fn message_request(&self) -> NativeMessageRequest<'_> {
        NativeMessageRequest {
            params: NativeMessageParams {
                target: target(),
                generation: self.generation.clone(),
                body: NativeMessageBody::PreparedPush(
                    "validate loaded policy".to_owned().try_into().unwrap(),
                ),
                delivery: MessageDelivery::Auto,
                client_user_message_id: Some("loaded-policy-input".to_owned().try_into().unwrap()),
            },
            id: json!("loaded-policy-input"),
            service_id: &self.backend.endpoint.service_id,
            backend: &self.backend,
            endpoints: std::slice::from_ref(&self.endpoint),
            held_connection: None,
            attempt_deadline: None,
        }
    }
}

pub(super) struct LoadedDeliveryScenario {
    pub(super) first_unloaded: bool,
    pub(super) mode: MessageDelivery,
}

/// Native replies and synthetic schema/endpoint metadata stand in for upstream; storage and sockets are real.
type PolicyServerTask = tokio::task::JoinHandle<Result<Vec<Vec<Value>>, TestError>>;

pub(super) fn policy_server(
    fixture: &NativeDeliveryFixture,
    scenario: LoadedDeliveryScenario,
) -> Result<PolicyServerTask, TestError> {
    let listener = tokio::net::UnixListener::bind(fixture.directory.path().join("native.sock"))?;
    let cwd = fixture.directory.path().to_string_lossy().into_owned();
    Ok(tokio::spawn(async move {
        let mut exchanges = Vec::new();
        for request_index in 0..2 {
            let (stream, _) = listener.accept().await?;
            let mut socket = tokio_tungstenite::accept_async(stream).await?;
            let mut requests = Vec::new();
            while let Some(Ok(Message::Text(text))) = socket.next().await {
                let request: Value = serde_json::from_str(&text)?;
                let method = request["method"].as_str().ok_or("method missing")?;
                let result = match method {
                    "initialize" => json!({}),
                    "initialized" => continue,
                    "thread/read" => {
                        json!({"thread":{"id":THREAD,"cwd":cwd,"status":{"type":if request_index==0 && scenario.first_unloaded {"notLoaded"} else if scenario.mode==MessageDelivery::Steer {"active"} else {"idle"}}}})
                    }
                    "turn/start" => json!({"turn":{"id":"accepted-native-turn"}}),
                    "thread/turns/list" => {
                        json!({"data":[{"id":"active-turn","status":"inProgress"}]})
                    }
                    "turn/steer" => json!({"turnId":"active-turn"}),
                    "thread/queue/add" => json!({"queuedSubmission":{"id":"queued-input"}}),
                    _ => return Err("unexpected native method".into()),
                };
                requests.push(request.clone());
                socket
                    .send(Message::Text(
                        json!({"id":request["id"],"result":result})
                            .to_string()
                            .into(),
                    ))
                    .await?;
            }
            exchanges.push(requests);
        }
        Ok(exchanges)
    }))
}
