//! Persisted broker and socket fixture for native loaded-policy scenarios.
use super::*;

pub(super) struct RecordedPolicyFixture {
    pub(super) directory: tempfile::TempDir,
    pub(super) backend: NativeControlBackend,
    pub(super) generation: CodexGeneration,
    pub(super) endpoint: EndpointDescription,
    pub(super) broker: Arc<ServiceInteractionBroker>,
    pub(super) filesystem: serde_json::Map<String, Value>,
}

impl RecordedPolicyFixture {
    pub(super) async fn create() -> Result<Self, TestError> {
        Self::create_with_authority(true).await
    }
    pub(super) async fn create_without_route() -> Result<Self, TestError> {
        Self::create_with_authority(false).await
    }
    async fn create_with_authority(register_route: bool) -> Result<Self, TestError> {
        let directory = tempfile::tempdir()?;
        let (backend, generation, endpoint) = backend(directory.path());
        let scratch = directory.path().join("scratch");
        tokio::fs::create_dir(&scratch).await?;
        let interaction_store = Arc::new(
            interaction_storage::InteractionStore::open(
                directory.path().join("interaction.sqlite"),
            )
            .await?,
        );
        let broker = ServiceInteractionBroker::load(
            SERVICE.to_owned().try_into()?,
            backend.clone(),
            Arc::clone(&interaction_store),
        )
        .await?;
        if register_route {
            broker
                .register_route(ApprovalRoute {
                    thread_id: THREAD.into(),
                    created_by: target(),
                    approver: target(),
                    access: collaboration_protocol::RouterAccess::WorkspaceWrite,
                    scratch_path: scratch.to_string_lossy().into_owned(),
                    root_message_id: None,
                })
                .await?;
        }
        drop(broker);
        drop(interaction_store);
        let interaction_store = Arc::new(
            interaction_storage::InteractionStore::open(
                directory.path().join("interaction.sqlite"),
            )
            .await?,
        );
        let broker = ServiceInteractionBroker::load(
            SERVICE.to_owned().try_into()?,
            backend.clone(),
            interaction_store,
        )
        .await?;
        Ok(Self {
            filesystem: expected_filesystem(&scratch)?,
            directory,
            backend,
            generation,
            endpoint,
            broker,
        })
    }

    pub(super) fn message_request<'a>(
        &'a self,
        broker: &'a std::sync::Weak<dyn ApprovalBroker>,
    ) -> NativeMessageRequest<'a> {
        NativeMessageRequest {
            attempt_evidence: None,
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
            load_policy: LoadPolicy::MayLoad,
            approval_broker: Some(broker),
            recorded_route: None,
            attempt_deadline: None,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum ReplyPolicy {
    Matching,
    Narrowed,
}
pub(super) struct PolicyServerCase {
    pub(super) cold: ReplyPolicy,
    pub(super) loaded: ReplyPolicy,
    pub(super) loaded_delivery: MessageDelivery,
}

/// Native replies and synthetic schema/endpoint metadata stand in for upstream; storage and sockets are real.
type PolicyServerTask = tokio::task::JoinHandle<Result<Vec<Vec<Value>>, TestError>>;

pub(super) fn policy_server(
    fixture: &RecordedPolicyFixture,
    scenario: PolicyServerCase,
) -> Result<PolicyServerTask, TestError> {
    let listener = tokio::net::UnixListener::bind(fixture.directory.path().join("native.sock"))?;
    let cwd = fixture.directory.path().to_string_lossy().into_owned();
    let roots = fixture
        .filesystem
        .iter()
        .filter(|(_, access)| *access == &json!("write"))
        .map(|(path, _)| path.clone())
        .collect::<Vec<_>>();
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
                        json!({"thread":{"id":THREAD,"cwd":cwd,"status":{"type":if request_index==0 {"notLoaded"} else if scenario.loaded_delivery==MessageDelivery::Steer {"active"} else {"idle"}}}})
                    }
                    "thread/resume" => {
                        let policy = if request_index == 0 {
                            scenario.cold
                        } else {
                            scenario.loaded
                        };
                        json!({"thread":{"id":THREAD,"cwd":cwd},"cwd":cwd,"activePermissionProfile":{"id":"router-workspace-write"},"sandbox":{"type":"workspaceWrite","networkAccess":true,"writableRoots":if matches!(policy, ReplyPolicy::Matching) {roots.clone()} else {vec![]}}})
                    }
                    "turn/start" => {
                        json!({"turn":{"id":"accepted-native-turn"}})
                    }
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
